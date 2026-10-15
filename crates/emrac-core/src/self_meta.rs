//! The release index emrac keeps about itself (u12): what releases exist,
//! the news, and the state of the last synchronisation. See
//! `docs/plan/r2/u12-self-service.md`.
//!
//! The index is one JSON document, `releases.json`, fetched from the
//! configured source, validated whole, staged next to the active copy and
//! renamed over it only when everything checks. Nothing in it is ever
//! executed; its strings are checked for control characters before they can
//! reach a terminal.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::SelfConfig;
use crate::error::{Error, Result};

pub const SCHEMA: u32 = 1;
const MAX_INDEX: usize = 2 * 1024 * 1024;
const MAX_TEXT: usize = 2000;
/// A cache older than this is called stale.
pub const STALE_DAYS: u64 = 7;
const TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = concat!("emrac/", env!("CARGO_PKG_VERSION"));

/// The version of the files emrac keeps in the user's home (history, config,
/// pins, snapshots). A downgrade to a release with a smaller number is refused
/// (u15).
pub const STATE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// `x.y.z`, as in the tarball's `Cargo.toml`.
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `stable` or `prerelease`.
    pub channel: String,
    #[serde(default = "one")]
    pub state_schema: u32,
    #[serde(default)]
    pub summary: String,
    /// The release page.
    #[serde(default)]
    pub url: String,
    pub tarball: String,
    /// SHA-256 of the tarball, 64 hex digits (stored lowercase).
    pub sha256: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewsItem {
    pub date: String,
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// The release it is about, when it is about one.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Index {
    pub schema: u32,
    #[serde(default)]
    pub generated: Option<String>,
    #[serde(default)]
    pub releases: Vec<Release>,
    #[serde(default)]
    pub news: Vec<NewsItem>,
}

/// What the last synchronisation did (`sync.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    pub last_attempt: u64,
    #[serde(default)]
    pub last_ok: Option<u64>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub error: Option<String>,
    /// How authenticity was established for the stored index.
    #[serde(default)]
    pub signature: String,
}

const CATEGORIES: [&str; 6] = [
    "release",
    "security",
    "breaking",
    "deprecation",
    "bugfix",
    "notice",
];

// ---------------------------------------------------------------- parsing

/// `x.y.z` (an optional `-suffix` is ignored for ordering) as numbers.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let out = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(out)
}

fn date_ok(d: &str) -> bool {
    let b = d.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| d[r].parse::<u32>().ok();
    matches!(
        (num(0..4), num(5..7), num(8..10)),
        (Some(1990..=2200), Some(1..=12), Some(1..=31))
    )
}

fn tag_ok(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 40
        && !t.starts_with(['-', '.'])
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn text_ok(s: &str, max: usize) -> bool {
    s.len() <= max && !s.chars().any(char::is_control)
}

/// The host of an `https://` or `file://` address.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty() && !host.contains('@')).then_some(host)
}

fn url_ok(url: &str, source_host: Option<&str>) -> bool {
    text_ok(url, 500) && host_of(url).is_some_and(|h| h == "github.com" || Some(h) == source_host)
}

/// Parses and validates a whole index. `source_host` is the host the index
/// itself came from (tarballs may live there or on github.com).
pub fn parse_index(text: &str, source_host: Option<&str>) -> std::result::Result<Index, String> {
    if text.len() > MAX_INDEX {
        return Err(format!("the index is over {} MiB", MAX_INDEX / 1024 / 1024));
    }
    let mut index: Index =
        serde_json::from_str(text).map_err(|e| format!("it is not a valid index: {e}"))?;
    if index.schema != SCHEMA {
        return Err(format!(
            "it uses schema {}, this emrac understands schema {SCHEMA}",
            index.schema
        ));
    }
    let mut tags = std::collections::HashSet::new();
    for r in &mut index.releases {
        let at = |why: &str| format!("release '{}': {why}", r.tag);
        if !tag_ok(&r.tag) {
            return Err(format!("'{}' is not a usable tag", r.tag));
        }
        if !tags.insert(r.tag.clone()) {
            return Err(at("listed twice"));
        }
        if parse_version(&r.version).is_none() || r.version.contains(['-', '+']) {
            return Err(at(&format!("'{}' is not a version like 1.2.3", r.version)));
        }
        if !date_ok(&r.date) {
            return Err(at("the date is not YYYY-MM-DD"));
        }
        if !matches!(r.channel.as_str(), "stable" | "prerelease") {
            return Err(at("the channel is not stable or prerelease"));
        }
        if r.state_schema == 0 {
            return Err(at("the state schema is 0"));
        }
        if r.sha256.len() != 64 || !r.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(at("the SHA-256 is not 64 hex digits"));
        }
        r.sha256.make_ascii_lowercase();
        if !url_ok(&r.tarball, source_host) {
            return Err(at(
                "the tarball is not an https address on github.com or on the index's own host",
            ));
        }
        if !r.url.is_empty() && !url_ok(&r.url, source_host) {
            return Err(at(
                "the release page is not an https address on an allowed host",
            ));
        }
        if !text_ok(&r.summary, MAX_TEXT) {
            return Err(at("the summary is too long or has control characters"));
        }
    }
    for n in &index.news {
        let at = |why: &str| format!("news item '{}': {why}", n.title);
        if !date_ok(&n.date) {
            return Err(at("the date is not YYYY-MM-DD"));
        }
        if !CATEGORIES.contains(&n.category.as_str()) {
            return Err(at(&format!("'{}' is not a known category", n.category)));
        }
        if n.title.is_empty() || !text_ok(&n.title, 200) {
            return Err(at("the title is empty, too long or has control characters"));
        }
        if !text_ok(&n.summary, MAX_TEXT) {
            return Err(at("the summary is too long or has control characters"));
        }
        if n.version
            .as_deref()
            .is_some_and(|v| parse_version(v).is_none())
        {
            return Err(at("the version is not x.y.z"));
        }
        if n.url.as_deref().is_some_and(|u| !url_ok(u, source_host)) {
            return Err(at("the link is not an https address on an allowed host"));
        }
    }
    Ok(index)
}

// --------------------------------------------------------------- ordering

fn key(r: &Release) -> ((u64, u64, u64), &str, &str) {
    (
        parse_version(&r.version).unwrap_or_default(),
        r.date.as_str(),
        r.tag.as_str(),
    )
}

/// Whether the channel setting lets `r` through.
pub fn eligible(r: &Release, channel: &str) -> bool {
    channel == "prerelease" || r.channel == "stable"
}

/// The newest release the channel allows, whatever is installed.
pub fn latest<'a>(index: &'a Index, channel: &str) -> Option<&'a Release> {
    index
        .releases
        .iter()
        .filter(|r| eligible(r, channel))
        .max_by(|a, b| key(a).cmp(&key(b)))
}

/// The newest eligible release whose version is above `installed`.
pub fn newer_than<'a>(index: &'a Index, channel: &str, installed: &str) -> Option<&'a Release> {
    let have = parse_version(installed)?;
    latest(index, channel).filter(|r| parse_version(&r.version).is_some_and(|v| v > have))
}

/// Eligible releases with a version below `installed`, newest first.
pub fn older_than<'a>(index: &'a Index, channel: &str, installed: &str) -> Vec<&'a Release> {
    let Some(have) = parse_version(installed) else {
        return Vec::new();
    };
    let mut out: Vec<&Release> = index
        .releases
        .iter()
        .filter(|r| eligible(r, channel) && parse_version(&r.version).is_some_and(|v| v < have))
        .collect();
    out.sort_by(|a, b| key(b).cmp(&key(a)));
    out
}

// -------------------------------------------------------------- the files

pub fn dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/self"))
}

fn err(msg: impl Into<String>) -> Error {
    Error::SelfService(msg.into())
}

fn io(path: &Path, e: std::io::Error) -> Error {
    err(format!("{}: {e}", path.display()))
}

/// The stored index and the state of the last sync. Both `None` before the
/// first successful sync.
pub struct Stored {
    pub index: Option<Index>,
    pub state: Option<SyncState>,
    /// The stored index could not be read back; why.
    pub index_problem: Option<String>,
}

pub fn load() -> Result<Stored> {
    load_from(&dir()?)
}

fn load_from(dir: &Path) -> Result<Stored> {
    let state = fs::read_to_string(dir.join("sync.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let (index, index_problem) = match fs::read_to_string(dir.join("releases.json")) {
        Ok(text) => match parse_index(&text, None) {
            Ok(i) => (Some(i), None),
            // The stored copy was validated when it was kept; a failure now
            // means it was changed since (or by an older emrac).
            Err(why) => (None, Some(why)),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(Stored {
        index,
        state,
        index_problem,
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, bytes).map_err(|e| io(&part, e))?;
    fs::rename(&part, path).map_err(|e| io(path, e))
}

// ------------------------------------------------------------------ fetch

/// Reads `source` (an `https://` address or a `file://` path), at most
/// `MAX_INDEX` bytes.
pub fn fetch(source: &str) -> Result<Vec<u8>> {
    if let Some(path) = source.strip_prefix("file://") {
        let mut f = fs::File::open(path).map_err(|e| err(format!("{path}: {e}")))?;
        return read_capped(&mut f, source);
    }
    if !source.starts_with("https://") {
        return Err(err(format!(
            "'{source}' is not an https:// or file:// address"
        )));
    }
    let mut response = ureq::get(source)
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => err(format!(
                "{source} does not exist (404): no release index is published there yet"
            )),
            other => err(format!("couldn't fetch {source}: {other}")),
        })?;
    read_capped(&mut response.body_mut().as_reader(), source)
}

fn read_capped(r: &mut dyn Read, what: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.take(MAX_INDEX as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| err(format!("couldn't read {what}: {e}")))?;
    if buf.len() > MAX_INDEX {
        return Err(err(format!(
            "{what} is over {} MiB",
            MAX_INDEX / 1024 / 1024
        )));
    }
    Ok(buf)
}

// -------------------------------------------------------------- signature

/// Reads gpg's `--status-fd` output: the fingerprint of a good signature, if
/// there is one and it belongs to `wanted` (the primary key or the signing
/// subkey; compared case-insensitively).
pub fn parse_gpg_status(status: &str, wanted: &str) -> std::result::Result<String, String> {
    let wanted = wanted.to_ascii_uppercase();
    let mut good = false;
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("[GNUPG:] ") else {
            continue;
        };
        let mut f = rest.split_whitespace();
        match f.next() {
            Some("GOODSIG") => good = true,
            Some("BADSIG") => return Err("the signature does not match the index".into()),
            Some("ERRSIG") => {
                return Err(
                    "gpg could not check the signature (is the key in your keyring?)".into(),
                )
            }
            Some("EXPKEYSIG" | "REVKEYSIG" | "EXPSIG") => {
                return Err("the signing key is expired or revoked".into())
            }
            Some("VALIDSIG") => {
                let fields: Vec<&str> = f.collect();
                let signing = fields.first().copied().unwrap_or("");
                let primary = fields.get(9).copied().unwrap_or("");
                if signing.eq_ignore_ascii_case(&wanted) || primary.eq_ignore_ascii_case(&wanted) {
                    return if good {
                        Ok(primary_or(primary, signing))
                    } else {
                        Err("gpg did not report a good signature".into())
                    };
                }
                return Err(format!(
                    "it is signed by {}, not by the key you trust ({wanted})",
                    primary_or(primary, signing)
                ));
            }
            _ => {}
        }
    }
    Err("no valid signature was reported".into())
}

fn primary_or(primary: &str, signing: &str) -> String {
    if primary.is_empty() { signing } else { primary }.to_ascii_uppercase()
}

fn verify_signature(
    index: &[u8],
    sig: &[u8],
    fingerprint: &str,
    gnupg_home: Option<&Path>,
) -> Result<String> {
    let tmp = std::env::temp_dir().join(format!("emrac-sig-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| io(&tmp, e))?;
    let result = (|| {
        fs::write(tmp.join("releases.json"), index).map_err(|e| io(&tmp, e))?;
        fs::write(tmp.join("releases.json.asc"), sig).map_err(|e| io(&tmp, e))?;
        let mut gpg = Command::new("gpg");
        if let Some(home) = gnupg_home {
            gpg.arg("--homedir").arg(home);
        }
        let out = gpg
            .args(["--batch", "--status-fd", "1", "--verify"])
            .arg(tmp.join("releases.json.asc"))
            .arg(tmp.join("releases.json"))
            .output()
            .map_err(|e| err(format!("couldn't run gpg to check the signature: {e}")))?;
        parse_gpg_status(&String::from_utf8_lossy(&out.stdout), fingerprint).map_err(err)
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

// ------------------------------------------------------------------- sync

/// What `sync --dry-run` would do, in words.
#[derive(Debug, Clone, Serialize)]
pub struct SyncPlan {
    pub source: String,
    pub channel: String,
    pub network: bool,
    pub signature: String,
    pub stored_releases: Option<usize>,
    pub last_ok: Option<u64>,
    pub actions: Vec<String>,
}

pub fn plan_sync(cfg: &SelfConfig, stored: &Stored) -> SyncPlan {
    let signature = if cfg.require_signature {
        format!("required: signed by {}", cfg.signing_key)
    } else {
        "not required: integrity from the address only".to_string()
    };
    let mut actions = vec![
        format!("fetch the release index from {}", cfg.source),
        "check it: schema, every field, the tarball addresses".to_string(),
    ];
    if cfg.require_signature {
        actions.push(format!("fetch {}.asc and verify it with gpg", cfg.source));
    }
    actions
        .push("keep it beside the active index and replace that only if everything checked".into());
    SyncPlan {
        source: cfg.source.clone(),
        channel: cfg.channel.clone(),
        network: cfg.source.starts_with("https://"),
        signature,
        stored_releases: stored.index.as_ref().map(|i| i.releases.len()),
        last_ok: stored.state.as_ref().and_then(|s| s.last_ok),
        actions,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub new_releases: Vec<String>,
    pub new_news: usize,
    pub releases: usize,
    pub unchanged: bool,
    pub signature: String,
}

/// Fetches, validates, stages and commits the index. On any failure the
/// active index is untouched and the failure is recorded in `sync.json`.
pub fn sync(cfg: &SelfConfig, now: u64) -> Result<SyncOutcome> {
    sync_in(&dir()?, cfg, now)
}

fn sync_in(dir: &Path, cfg: &SelfConfig, now: u64) -> Result<SyncOutcome> {
    fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let before = load_from(dir)?;
    let mut state = before.state.clone().unwrap_or_default();
    state.last_attempt = now;
    state.source = cfg.source.clone();

    let attempt = (|| {
        if cfg.require_signature && cfg.signing_key.is_empty() {
            return Err(err(
                "a signature is required but no key is set: `emrac config set self.signing_key <fingerprint>`",
            ));
        }
        let bytes = fetch(&cfg.source)?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| err("the index is not text (not UTF-8)"))?;
        let index = parse_index(&text, host_of(&cfg.source))
            .map_err(|why| err(format!("the index from {} was rejected: {why}", cfg.source)))?;
        let signature = if cfg.require_signature {
            let sig = fetch(&format!("{}.asc", cfg.source))
                .map_err(|e| err(format!("the signature is required but: {e}")))?;
            let who = verify_signature(&bytes, &sig, &cfg.signing_key, None)?;
            write_atomic(&dir.join("releases.json.asc"), &sig)?;
            format!("signed by {who}")
        } else {
            let _ = fs::remove_file(dir.join("releases.json.asc"));
            "not required (integrity from the address only)".to_string()
        };
        Ok((bytes, index, signature))
    })();

    match attempt {
        Err(e) => {
            state.error = Some(e.to_string());
            let _ = write_atomic(
                &dir.join("sync.json"),
                &serde_json::to_vec_pretty(&state).unwrap(),
            );
            Err(e)
        }
        Ok((bytes, index, signature)) => {
            let old = before.index.clone();
            let known: std::collections::HashSet<&str> = old
                .as_ref()
                .map(|i| i.releases.iter().map(|r| r.tag.as_str()).collect())
                .unwrap_or_default();
            let new_releases: Vec<String> = index
                .releases
                .iter()
                .filter(|r| !known.contains(r.tag.as_str()))
                .map(|r| r.tag.clone())
                .collect();
            let old_news = old.as_ref().map_or(0, |i| i.news.len());
            let new_news = index.news.len().saturating_sub(old_news);
            let unchanged = old.as_ref() == Some(&index);

            // Stage, keep the previous, then replace.
            let active = dir.join("releases.json");
            let staged = dir.join("releases.json.new");
            fs::write(&staged, &bytes).map_err(|e| io(&staged, e))?;
            if active.exists() && !unchanged {
                let _ = fs::copy(&active, dir.join("releases.json.prev"));
            }
            fs::rename(&staged, &active).map_err(|e| io(&active, e))?;

            state.last_ok = Some(now);
            state.error = None;
            state.signature = signature.clone();
            write_atomic(
                &dir.join("sync.json"),
                &serde_json::to_vec_pretty(&state).unwrap(),
            )?;
            Ok(SyncOutcome {
                new_releases,
                new_news,
                releases: index.releases.len(),
                unchanged,
                signature,
            })
        }
    }
}

// ------------------------------------------------------------------- news

/// How old the cache is, in words, and whether that is stale.
pub fn age_label(last_ok: Option<u64>, now: u64) -> (String, bool) {
    match last_ok {
        None => ("never synced".to_string(), true),
        Some(t) => {
            let days = now.saturating_sub(t) / 86_400;
            let label = match days {
                0 => "synced today".to_string(),
                1 => "synced 1 day ago".to_string(),
                d => format!("synced {d} days ago"),
            };
            if days > STALE_DAYS {
                (
                    format!("{label}, stale: more than {STALE_DAYS} days old"),
                    true,
                )
            } else {
                (label, false)
            }
        }
    }
}

/// The news, newest first; `limit` of them unless it is `None`.
pub fn news_items(index: &Index, limit: Option<usize>) -> Vec<&NewsItem> {
    let mut items: Vec<&NewsItem> = index.news.iter().collect();
    items.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.title.cmp(&b.title)));
    if let Some(n) = limit {
        items.truncate(n);
    }
    items
}

/// Whether a news item is about a release newer than `installed`.
pub fn is_newer(item: &NewsItem, installed: &str) -> bool {
    match (
        item.version.as_deref().and_then(parse_version),
        parse_version(installed),
    ) {
        (Some(v), Some(have)) => v > have,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "a20701e9d20b4a677c1dc1c6333682d09f7f5c65a3c0afad26396630bf1a6c40";

    fn release(tag: &str, version: &str, date: &str, channel: &str) -> String {
        format!(
            r#"{{"tag":"{tag}","version":"{version}","date":"{date}","channel":"{channel}","state_schema":1,"summary":"s","url":"https://github.com/pardhuvarmax/emrac/releases/tag/{tag}","tarball":"https://github.com/pardhuvarmax/emrac/archive/refs/tags/{tag}.tar.gz","sha256":"{SHA}"}}"#
        )
    }

    fn index_text(releases: &[String], news: &str) -> String {
        format!(
            r#"{{"schema":1,"generated":"2026-10-15","releases":[{}],"news":[{news}]}}"#,
            releases.join(",")
        )
    }

    fn good() -> String {
        index_text(
            &[
                release("r1", "0.1.0", "2026-10-13", "stable"),
                release("r2", "0.2.0", "2026-10-14", "stable"),
                release("r3-rc", "0.3.0", "2026-10-20", "prerelease"),
            ],
            r#"{"date":"2026-10-14","category":"release","title":"Emrac r2","summary":"Second release","version":"0.2.0","url":"https://github.com/pardhuvarmax/emrac/releases/tag/r2"}"#,
        )
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d =
                std::env::temp_dir().join(format!("emrac-selfmeta-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_good_index_parses_and_normalises() {
        let i = parse_index(&good(), None).unwrap();
        assert_eq!(i.releases.len(), 3);
        assert_eq!(i.news.len(), 1);
        let upper = good().replace(SHA, &SHA.to_uppercase());
        assert_eq!(parse_index(&upper, None).unwrap().releases[0].sha256, SHA);
    }

    #[test]
    fn each_bad_field_rejects_the_whole_index() {
        let bad = |from: &str, to: &str| {
            let text = good().replacen(from, to, 1);
            parse_index(&text, None).expect_err(to)
        };
        assert!(bad(r#""schema":1"#, r#""schema":2"#).contains("schema 2"));
        assert!(bad(r#""version":"0.1.0""#, r#""version":"0.1""#).contains("version"));
        assert!(bad("2026-10-13", "2026-13-13").contains("date"));
        assert!(bad(r#""channel":"stable""#, r#""channel":"nightly""#).contains("channel"));
        assert!(bad(SHA, "abc").contains("SHA-256"));
        assert!(bad(
            "https://github.com/pardhuvarmax/emrac/archive",
            "https://evil.example/archive"
        )
        .contains("tarball"));
        assert!(bad(
            "https://github.com/pardhuvarmax/emrac/archive",
            "http://github.com/pardhuvarmax/emrac/archive"
        )
        .contains("tarball"));
        assert!(bad(r#""tag":"r1""#, r#""tag":"r2""#).contains("twice"));
        assert!(bad(r#""tag":"r1""#, r#""tag":"../r1""#).contains("tag"));
        assert!(bad(r#""summary":"s""#, r#""summary":"a\u001b[31mred""#).contains("control"));
        assert!(bad(r#""category":"release""#, r#""category":"gossip""#).contains("category"));
        assert!(parse_index("not json", None).is_err());
        assert!(parse_index(&"x".repeat(MAX_INDEX + 1), None)
            .unwrap_err()
            .contains("MiB"));
    }

    #[test]
    fn the_index_host_may_serve_tarballs_too() {
        let text = good().replace(
            "https://github.com/pardhuvarmax/emrac/archive",
            "https://mirror.example/emrac/archive",
        );
        assert!(parse_index(&text, None).is_err());
        assert!(parse_index(&text, Some("mirror.example")).is_ok());
    }

    #[test]
    fn versions_order_and_channels_filter() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        let i = parse_index(&good(), None).unwrap();
        assert_eq!(latest(&i, "stable").unwrap().tag, "r2");
        assert_eq!(latest(&i, "prerelease").unwrap().tag, "r3-rc");
        assert_eq!(newer_than(&i, "stable", "0.1.0").unwrap().tag, "r2");
        assert!(
            newer_than(&i, "stable", "0.2.0").is_none(),
            "already latest"
        );
        assert_eq!(newer_than(&i, "prerelease", "0.2.0").unwrap().tag, "r3-rc");
        assert!(newer_than(&i, "stable", "garbage").is_none());
        let older: Vec<&str> = older_than(&i, "prerelease", "0.3.0")
            .iter()
            .map(|r| r.tag.as_str())
            .collect();
        assert_eq!(older, ["r2", "r1"]);
        assert!(older_than(&i, "stable", "0.1.0").is_empty());
    }

    #[test]
    fn the_gpg_status_must_name_the_trusted_key() {
        let fpr = "AAAABBBBCCCCDDDDEEEEFFFF0000111122223333";
        let sub = "99998888777766665555444433332222111100AA";
        let status = format!(
            "[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 1122334455667788 Me <me@x>\n[GNUPG:] VALIDSIG {sub} 2026-10-15 1790000000 0 4 0 1 10 00 {fpr}\n"
        );
        assert_eq!(parse_gpg_status(&status, fpr).unwrap(), fpr);
        assert_eq!(parse_gpg_status(&status, &fpr.to_lowercase()).unwrap(), fpr);
        assert_eq!(
            parse_gpg_status(&status, sub).unwrap(),
            fpr,
            "the subkey also counts"
        );
        assert!(parse_gpg_status(&status, "0000")
            .unwrap_err()
            .contains("not by the key"));
        assert!(parse_gpg_status("[GNUPG:] BADSIG 1 x\n", fpr)
            .unwrap_err()
            .contains("does not match"));
        assert!(parse_gpg_status("[GNUPG:] ERRSIG 1 1 8 1 0 9\n", fpr)
            .unwrap_err()
            .contains("keyring"));
        assert!(parse_gpg_status("[GNUPG:] EXPKEYSIG 1 x\n", fpr)
            .unwrap_err()
            .contains("expired"));
        assert!(parse_gpg_status("", fpr).is_err());
    }

    #[test]
    fn syncing_stages_commits_and_reports_what_changed() {
        let s = Scratch::new("sync");
        let src = s.0.join("src.json");
        fs::write(&src, good()).unwrap();
        let cfg = SelfConfig {
            source: format!("file://{}", src.display()),
            ..SelfConfig::default()
        };
        let dir = s.0.join("self");

        let first = sync_in(&dir, &cfg, 1_000).unwrap();
        assert_eq!(first.new_releases, ["r1", "r2", "r3-rc"]);
        assert_eq!(first.new_news, 1);
        assert!(!first.unchanged);
        assert!(first.signature.contains("address only"));

        let again = sync_in(&dir, &cfg, 2_000).unwrap();
        assert!(again.unchanged && again.new_releases.is_empty());

        // A new release arrives.
        let text = index_text(
            &[
                release("r1", "0.1.0", "2026-10-13", "stable"),
                release("r2", "0.2.0", "2026-10-14", "stable"),
                release("r3-rc", "0.3.0", "2026-10-20", "prerelease"),
                release("r3", "0.3.0", "2026-10-25", "stable"),
            ],
            "",
        );
        fs::write(&src, text).unwrap();
        let third = sync_in(&dir, &cfg, 3_000).unwrap();
        assert_eq!(third.new_releases, ["r3"]);
        assert!(dir.join("releases.json.prev").exists());
        let stored = load_from(&dir).unwrap();
        assert_eq!(stored.state.unwrap().last_ok, Some(3_000));
        assert_eq!(stored.index.unwrap().releases.len(), 4);
    }

    #[test]
    fn a_bad_or_missing_index_leaves_the_active_one_alone_and_records_why() {
        let s = Scratch::new("bad");
        let src = s.0.join("src.json");
        fs::write(&src, good()).unwrap();
        let cfg = SelfConfig {
            source: format!("file://{}", src.display()),
            ..SelfConfig::default()
        };
        let dir = s.0.join("self");
        sync_in(&dir, &cfg, 1_000).unwrap();
        let active = fs::read_to_string(dir.join("releases.json")).unwrap();

        fs::write(&src, good().replace(SHA, "zz")).unwrap();
        let e = sync_in(&dir, &cfg, 2_000).unwrap_err().to_string();
        assert!(e.contains("rejected"), "{e}");
        assert_eq!(
            fs::read_to_string(dir.join("releases.json")).unwrap(),
            active
        );
        let st = load_from(&dir).unwrap().state.unwrap();
        assert_eq!((st.last_ok, st.last_attempt), (Some(1_000), 2_000));
        assert!(st.error.unwrap().contains("rejected"));

        fs::remove_file(&src).unwrap();
        assert!(sync_in(&dir, &cfg, 3_000).is_err());
        let need_key = SelfConfig {
            require_signature: true,
            ..cfg.clone()
        };
        assert!(sync_in(&dir, &need_key, 4_000)
            .unwrap_err()
            .to_string()
            .contains("no key"));
    }

    #[test]
    fn a_real_gpg_signature_is_checked_against_the_trusted_fingerprint() {
        if Command::new("gpg").arg("--version").output().is_err() {
            eprintln!("gpg is not installed: skipped");
            return;
        }
        let s = Scratch::new("gpg");
        let home = s.0.join("gnupg");
        fs::create_dir_all(&home).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let gpg = |args: &[&str]| {
            Command::new("gpg")
                .arg("--homedir")
                .arg(&home)
                .args([
                    "--batch",
                    "--yes",
                    "--pinentry-mode",
                    "loopback",
                    "--passphrase",
                    "",
                ])
                .args(args)
                .current_dir(&s.0)
                .output()
                .unwrap()
        };
        let made = gpg(&[
            "--quick-gen-key",
            "test <t@example.org>",
            "default",
            "default",
            "never",
        ]);
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let listing = String::from_utf8(gpg(&["--list-keys", "--with-colons"]).stdout).unwrap();
        let fpr = listing
            .lines()
            .find(|l| l.starts_with("fpr:"))
            .and_then(|l| l.split(':').nth(9))
            .unwrap()
            .to_string();
        fs::write(s.0.join("releases.json"), good()).unwrap();
        assert!(gpg(&[
            "--armor",
            "--detach-sign",
            "-o",
            "releases.json.asc",
            "releases.json"
        ])
        .status
        .success());
        let sig = fs::read(s.0.join("releases.json.asc")).unwrap();
        let bytes = good().into_bytes();
        assert_eq!(
            verify_signature(&bytes, &sig, &fpr, Some(&home)).unwrap(),
            fpr
        );
        let other = "0123456789ABCDEF0123456789ABCDEF01234567";
        assert!(verify_signature(&bytes, &sig, other, Some(&home))
            .unwrap_err()
            .to_string()
            .contains("not by the key"));
        let tampered = good()
            .replace("Second release", "Second RELEASE")
            .into_bytes();
        assert!(verify_signature(&tampered, &sig, &fpr, Some(&home)).is_err());
    }

    #[test]
    fn the_cache_age_is_worded_and_stale_after_a_week() {
        assert_eq!(age_label(None, 100), ("never synced".to_string(), true));
        assert_eq!(age_label(Some(100), 100).0, "synced today");
        assert_eq!(age_label(Some(0), 86_400).0, "synced 1 day ago");
        let (text, stale) = age_label(Some(0), 9 * 86_400);
        assert!(stale && text.contains("stale"), "{text}");
        assert!(!age_label(Some(0), 7 * 86_400).1);
    }

    #[test]
    fn news_is_newest_first_limited_and_marked_when_about_a_newer_release() {
        let mut i = parse_index(&good(), None).unwrap();
        i.news.push(NewsItem {
            date: "2026-10-20".into(),
            category: "security".into(),
            title: "Fix".into(),
            summary: String::new(),
            version: Some("0.3.0".into()),
            url: None,
        });
        let items = news_items(&i, Some(1));
        assert_eq!(items[0].title, "Fix");
        assert!(is_newer(items[0], "0.2.0") && !is_newer(items[0], "0.3.0"));
        assert_eq!(news_items(&i, None).len(), 2);
    }
}

//! Where `emrac score` gets its outside evidence (u20): the Arch rebuilders,
//! the Arch packaging history on GitLab, the OpenSSF Scorecard and OSV. Every
//! answer is fetched over https only (a `file://` address works for tests),
//! limited in size, parsed strictly, cached, and used from the cache when
//! offline or when the source is down. Parsing is pure and tested.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::score_more::{OsvFact, PackagingFact, ReproFact, ReproStatus, ScorecardFact};
use crate::{Error, Result};

const MAX_BYTES: u64 = 4 << 20;
const TIMEOUT: Duration = Duration::from_secs(20);
/// Rebuilder answers change with every build; the others rarely.
pub const REPRO_TTL: u64 = 6 * 3600;
pub const SLOW_TTL: u64 = 7 * 86_400;

fn bad(why: impl Into<String>) -> Error {
    Error::Security(why.into())
}

// -------------------------------------------------------------- addresses

pub const REPRO_URL: &str = "https://reproducible.archlinux.org/api/v0/pkgs/list?name={name}";
pub const COMMITS_URL: &str = "https://gitlab.archlinux.org/api/v4/projects/archlinux%2Fpackaging%2Fpackages%2F{name}/repository/commits?path=PKGBUILD&per_page=100";
pub const SCORECARD_URL: &str = "https://api.securityscorecards.dev/projects/github.com/{name}";
pub const OSV_URL: &str = "https://api.osv.dev/v1/query";

/// An address from `env` (for tests and mirrors) or the default, with `{name}`
/// replaced by `value`, percent-encoded.
pub fn address(env: &str, default: &str, value: &str) -> String {
    let template = std::env::var(env).unwrap_or_else(|_| default.to_string());
    template.replace("{name}", &encode(value))
}

/// Percent-encoding of everything but letters, digits and `-._~`.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ----------------------------------------------------------------- fetching

#[derive(Debug, Clone)]
pub struct Fetched {
    /// `None`: the source answered "no such thing".
    pub body: Option<String>,
    pub fetched_at: u64,
    /// Said when the answer is older than a refresh would give.
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    fetched_at: u64,
    body: Option<String>,
}

pub struct Request<'a> {
    pub kind: &'a str,
    /// What the answer is about (used in the cache file name).
    pub key: &'a str,
    pub url: String,
    /// A JSON body: the request is a POST.
    pub post: Option<String>,
    pub ttl: u64,
}

fn cache_file(kind: &str, key: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '+') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    Ok(PathBuf::from(home)
        .join(".local/share/emrac/evidence")
        .join(format!("{kind}-{safe}.json")))
}

fn read_cache(path: &std::path::Path) -> Option<CacheFile> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_cache(path: &std::path::Path, file: &CacheFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec(file).unwrap_or_default())?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

/// One request, over the network or from a file. `Ok(None)` is a 404.
fn fetch(url: &str, post: Option<&str>) -> Result<Option<String>> {
    if let Some(path) = url.strip_prefix("file://") {
        return match fs::read(path) {
            Ok(b) => Ok(Some(
                String::from_utf8(b).map_err(|_| bad(format!("{path} is not text")))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(bad(format!("{path}: {e}"))),
        };
    }
    if !url.starts_with("https://") {
        return Err(bad(format!(
            "{url} is not an https:// address; only https is fetched"
        )));
    }
    let agent = concat!("emrac/", env!("CARGO_PKG_VERSION"));
    let result = match post {
        Some(body) => ureq::post(url)
            .header("User-Agent", agent)
            .header("Content-Type", "application/json")
            .config()
            .timeout_global(Some(TIMEOUT))
            .build()
            .send(body),
        None => ureq::get(url)
            .header("User-Agent", agent)
            .config()
            .timeout_global(Some(TIMEOUT))
            .build()
            .call(),
    };
    let mut response = match result {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(bad(format!("couldn't fetch {url}: {e}"))),
    };
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| bad(format!("couldn't read {url}: {e}")))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| bad(format!("{url} did not answer with text")))
}

/// The answer from the cache while it is fresh, else a new fetch (not when
/// `offline`), else an older cache with a note, else an error.
pub fn get(req: &Request<'_>, offline: bool, now: u64) -> Result<Fetched> {
    let path = cache_file(req.kind, req.key)?;
    get_with(&path, req, offline, now)
}

pub fn get_with(
    path: &std::path::Path,
    req: &Request<'_>,
    offline: bool,
    now: u64,
) -> Result<Fetched> {
    let cached = read_cache(path);
    if let Some(c) = &cached {
        if now.saturating_sub(c.fetched_at) < req.ttl {
            return Ok(Fetched {
                body: c.body.clone(),
                fetched_at: c.fetched_at,
                note: None,
            });
        }
    }
    if offline {
        return match cached {
            Some(c) => Ok(Fetched {
                body: c.body,
                fetched_at: c.fetched_at,
                note: Some("--offline is set; using the saved answer".into()),
            }),
            None => Err(bad("--offline is set and nothing was saved")),
        };
    }
    match fetch(&req.url, req.post.as_deref()) {
        Ok(body) => {
            let _ = write_cache(
                path,
                &CacheFile {
                    fetched_at: now,
                    body: body.clone(),
                },
            );
            Ok(Fetched {
                body,
                fetched_at: now,
                note: None,
            })
        }
        Err(e) => match cached {
            Some(c) => Ok(Fetched {
                body: c.body,
                fetched_at: c.fetched_at,
                note: Some(format!("could not refresh ({e}); using the saved answer")),
            }),
            None => Err(e),
        },
    }
}

// ------------------------------------------------------------------ parsing

fn value(body: &str) -> Result<serde_json::Value> {
    serde_json::from_str(body).map_err(|e| bad(format!("an answer that is not JSON: {e}")))
}

/// The newest rebuilder record for `name`, preferring the one for `want`.
pub fn parse_repro(
    body: &str,
    name: &str,
    want: &str,
    fetched_at: u64,
    now: u64,
    stale_note: Option<String>,
) -> Result<Option<ReproFact>> {
    let v = value(body)?;
    let list = v
        .as_array()
        .ok_or_else(|| bad("the rebuilder answer is not a list"))?;
    let mut mine: Vec<&serde_json::Value> = list
        .iter()
        .filter(|r| {
            r["name"].as_str() == Some(name)
                && r["distro"].as_str().is_none_or(|d| d == "archlinux")
        })
        .collect();
    mine.sort_by(|a, b| {
        let key = |r: &serde_json::Value| {
            (
                r["version"].as_str() == Some(want),
                r["built_at"].as_str().unwrap_or("").to_string(),
            )
        };
        key(b).cmp(&key(a))
    });
    let Some(r) = mine.first() else {
        return Ok(None);
    };
    let status = match r["status"].as_str().unwrap_or("") {
        "GOOD" => ReproStatus::Good,
        "BAD" => ReproStatus::Bad,
        _ => ReproStatus::Unknown,
    };
    Ok(Some(ReproFact {
        status,
        version: r["version"].as_str().unwrap_or("").to_string(),
        suite: r["suite"].as_str().unwrap_or("").to_string(),
        built_at: r["built_at"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(19)
            .collect(),
        attested: r["has_attestation"].as_bool().unwrap_or(false),
        age_hours: now.saturating_sub(fetched_at) / 3600,
        stale_note,
    }))
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the date part of `2026-10-15T10:28:34...` (the time of day
/// and the offset are ignored: a day is precise enough).
pub fn day_start(text: &str) -> Option<u64> {
    let date = text.get(..10)?;
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1970..=2200).contains(&y) {
        return None;
    }
    u64::try_from(days_from_civil(y, m, d) * 86_400).ok()
}

pub fn parse_commits(body: &str, base: &str, now: u64) -> Result<Option<PackagingFact>> {
    let v = value(body)?;
    let list = v
        .as_array()
        .ok_or_else(|| bad("the commit list is not a list"))?;
    let days: Vec<u64> = list
        .iter()
        .filter_map(|c| day_start(c["committed_date"].as_str()?))
        .collect();
    let Some(newest) = days.iter().max() else {
        return Ok(None);
    };
    let authors: HashSet<&str> = list
        .iter()
        .filter_map(|c| c["author_name"].as_str())
        .collect();
    Ok(Some(PackagingFact {
        base: base.to_string(),
        days_since_change: now.saturating_sub(*newest) / 86_400,
        recent_commits: days
            .iter()
            .filter(|d| now.saturating_sub(**d) <= 365 * 86_400)
            .count(),
        authors: authors.len(),
        commits_seen: days.len(),
    }))
}

pub fn parse_scorecard(body: &str) -> Result<Option<ScorecardFact>> {
    let v = value(body)?;
    let Some(score) = v["score"].as_f64() else {
        return Ok(None);
    };
    if !(0.0..=10.0).contains(&score) {
        return Err(bad("a Scorecard score outside 0 to 10"));
    }
    let mut weak: Vec<(String, i32)> = v["checks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some((c["name"].as_str()?.to_string(), c["score"].as_i64()? as i32))
                })
                .filter(|(_, s)| (0..5).contains(s))
                .collect()
        })
        .unwrap_or_default();
    weak.sort_by_key(|(n, s)| (*s, n.clone()));
    Ok(Some(ScorecardFact {
        score,
        date: v["date"].as_str().unwrap_or("").chars().take(10).collect(),
        weak,
    }))
}

pub fn parse_osv(body: &str, version: &str) -> Result<OsvFact> {
    let v = value(body)?;
    let mut ids: Vec<String> = v["vulns"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["withdrawn"].is_null())
                .filter_map(|x| x["id"].as_str())
                .filter(|id| {
                    id.len() <= 64
                        && id.chars().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
                        })
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    Ok(OsvFact {
        version: version.to_string(),
        ids,
    })
}

/// `owner/repo` of a GitHub address, else `None`.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git://"))?;
    let (host, path) = rest.split_once('/')?;
    if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
        return None;
    }
    let mut segs = path.split(['/', '?', '#']).filter(|s| !s.is_empty());
    let owner = segs.next()?;
    let repo = segs.next()?.trim_end_matches(".git");
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(owner) && ok(repo)).then(|| format!("{owner}/{repo}"))
}

/// The upstream version in an Arch version: no epoch, no pkgrel.
pub fn upstream_version(version: &str) -> String {
    let v = version.split_once(':').map_or(version, |(_, rest)| rest);
    v.rsplit_once('-').map_or(v, |(a, _)| a).to_string()
}

/// A name that is safe to put in an address: what a package or a package base
/// may be called.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_792_000_000; // 2026-10-14

    #[test]
    fn names_are_encoded_and_validated() {
        assert_eq!(encode("gtk+3"), "gtk%2B3");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert!(
            valid_name("lib32-gtk+3") && valid_name("python-zope.interface") && valid_name("a@b")
        );
        for bad in ["", "-x", "../x", "a/b", "a b", "a?b", ".hidden"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn github_addresses_are_recognised_and_nothing_else() {
        assert_eq!(
            github_repo("https://github.com/curl/curl").as_deref(),
            Some("curl/curl")
        );
        assert_eq!(
            github_repo("https://github.com/neovim/neovim/").as_deref(),
            Some("neovim/neovim")
        );
        assert_eq!(
            github_repo("git://github.com/a/b.git").as_deref(),
            Some("a/b")
        );
        assert_eq!(
            github_repo("https://www.github.com/a/b/issues?x=1").as_deref(),
            Some("a/b")
        );
        for no in [
            "https://gitlab.com/a/b",
            "https://github.com.evil.example/a/b",
            "https://github.com/a",
            "https://github.com/../b",
            "ftp://github.com/a/b",
            "https://example.org/github.com/a/b",
            "not a url",
        ] {
            assert_eq!(github_repo(no), None, "{no}");
        }
    }

    #[test]
    fn the_upstream_version_drops_epoch_and_release() {
        assert_eq!(upstream_version("1:8.0.0-2"), "8.0.0");
        assert_eq!(upstream_version("7.1.0.r9.g54d9411-3"), "7.1.0.r9.g54d9411");
        assert_eq!(upstream_version("1.0"), "1.0");
    }

    #[test]
    fn dates_become_days() {
        assert_eq!(day_start("1970-01-02T00:00:00"), Some(86_400));
        assert_eq!(
            day_start("2026-10-15T10:28:34.000+02:00"),
            Some(1_792_022_400)
        );
        assert_eq!(day_start("2026-13-01"), None);
        assert_eq!(day_start("nonsense!!"), None);
        assert_eq!(day_start("12"), None);
    }

    #[test]
    fn the_rebuilder_record_for_the_wanted_version_wins() {
        let body = r#"[
          {"name":"p","version":"1-1","status":"BAD","distro":"archlinux","suite":"extra","built_at":"2026-10-01T10:00:00.5","has_attestation":false},
          {"name":"p","version":"2-1","status":"GOOD","distro":"archlinux","suite":"extra","built_at":"2026-10-10T10:00:00.5","has_attestation":true},
          {"name":"other","version":"2-1","status":"BAD","distro":"archlinux","suite":"extra","built_at":"2026-10-11T10:00:00"}]"#;
        let r = parse_repro(body, "p", "1-1", NOW, NOW + 7200, None)
            .unwrap()
            .unwrap();
        assert_eq!((r.status, r.version.as_str()), (ReproStatus::Bad, "1-1"));
        let r = parse_repro(body, "p", "9-9", NOW, NOW + 7200, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            (r.status, r.version.as_str(), r.attested, r.age_hours),
            (ReproStatus::Good, "2-1", true, 2)
        );
        assert_eq!(
            parse_repro(body, "nope", "1", NOW, NOW, None).unwrap(),
            None
        );
        assert_eq!(parse_repro("[]", "p", "1", NOW, NOW, None).unwrap(), None);
        assert!(parse_repro("{}", "p", "1", NOW, NOW, None).is_err());
        assert!(parse_repro("not json", "p", "1", NOW, NOW, None).is_err());
    }

    #[test]
    fn an_undecided_or_unexpected_status_is_unknown() {
        let body = r#"[{"name":"p","version":"1","status":"UNKWN","distro":"archlinux","suite":"core","built_at":"2026-10-01T00:00:00"},{"name":"q","version":"1","status":"surprise","suite":"core","built_at":"x"}]"#;
        assert_eq!(
            parse_repro(body, "p", "1", NOW, NOW, None)
                .unwrap()
                .unwrap()
                .status,
            ReproStatus::Unknown
        );
        assert_eq!(
            parse_repro(body, "q", "1", NOW, NOW, None)
                .unwrap()
                .unwrap()
                .status,
            ReproStatus::Unknown
        );
    }

    #[test]
    fn commits_give_age_activity_and_authors() {
        let body = r#"[
          {"committed_date":"2026-10-10T10:00:00.000+02:00","author_name":"A"},
          {"committed_date":"2026-06-10T10:00:00.000+02:00","author_name":"B"},
          {"committed_date":"2024-01-10T10:00:00.000+02:00","author_name":"A"}]"#;
        let p = parse_commits(body, "base", NOW).unwrap().unwrap();
        assert_eq!(
            (
                p.days_since_change,
                p.recent_commits,
                p.authors,
                p.commits_seen
            ),
            (4, 2, 2, 3)
        );
        assert_eq!(parse_commits("[]", "b", NOW).unwrap(), None);
        assert!(parse_commits("{\"message\":\"404\"}", "b", NOW).is_err());
    }

    #[test]
    fn a_scorecard_lists_its_weak_checks_and_refuses_nonsense() {
        let body = r#"{"date":"2026-10-12","score":6.4,"checks":[{"name":"Code-Review","score":0},{"name":"Fuzzing","score":-1},{"name":"License","score":10},{"name":"Maintained","score":3}]}"#;
        let s = parse_scorecard(body).unwrap().unwrap();
        assert_eq!(s.score, 6.4);
        assert_eq!(
            s.weak,
            vec![
                ("Code-Review".to_string(), 0),
                ("Maintained".to_string(), 3)
            ]
        );
        assert_eq!(parse_scorecard("{}").unwrap(), None);
        assert!(parse_scorecard(r#"{"score":99}"#).is_err());
    }

    #[test]
    fn osv_ids_are_counted_once_and_withdrawn_ones_dropped() {
        let body = r#"{"vulns":[{"id":"A-1"},{"id":"A-1"},{"id":"B-2","withdrawn":"2025-01-01"},{"id":"bad id!"},{"id":"C-3"}]}"#;
        let o = parse_osv(body, "1.0").unwrap();
        assert_eq!(o.ids, vec!["A-1", "C-3"]);
        assert!(parse_osv("{}", "1.0").unwrap().ids.is_empty());
    }

    #[test]
    fn answers_are_cached_refreshed_and_kept_when_the_source_fails() {
        let dir = std::env::temp_dir().join(format!("emrac-ss-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.json");
        let cache = dir.join("cache.json");
        std::fs::write(&src, "[1]").unwrap();
        let req = |ttl| Request {
            kind: "t",
            key: "k",
            url: format!("file://{}", src.display()),
            post: None,
            ttl,
        };
        let a = get_with(&cache, &req(100), false, 1000).unwrap();
        assert_eq!((a.body.as_deref(), a.note), (Some("[1]"), None));
        std::fs::write(&src, "[2]").unwrap();
        // fresh: not asked again
        assert_eq!(
            get_with(&cache, &req(100), false, 1050)
                .unwrap()
                .body
                .as_deref(),
            Some("[1]")
        );
        // old: refreshed
        assert_eq!(
            get_with(&cache, &req(100), false, 1200)
                .unwrap()
                .body
                .as_deref(),
            Some("[2]")
        );
        // offline: the saved answer, said so
        let o = get_with(&cache, &req(100), true, 9000).unwrap();
        assert_eq!(o.body.as_deref(), Some("[2]"));
        assert!(o.note.unwrap().contains("--offline"));
        // a missing source is a 404: an answer of "nothing"
        std::fs::remove_file(&src).unwrap();
        assert_eq!(get_with(&cache, &req(100), false, 9999).unwrap().body, None);
        // a source that fails keeps the saved answer, with a note
        let broken = Request {
            url: "http://insecure.example/x".into(),
            ..req(100)
        };
        let k = get_with(&cache, &broken, false, 99_999).unwrap();
        assert!(k.note.unwrap().contains("could not refresh"));
        // nothing saved and nothing to fetch is an error
        let other = dir.join("none.json");
        assert!(get_with(&other, &broken, false, 1).is_err());
        assert!(get_with(&other, &req(100), true, 1).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_https_is_fetched() {
        assert!(fetch("http://example.org/x", None).is_err());
        assert!(fetch("ftp://example.org/x", None).is_err());
    }
}

//! AUR trust signals (u20, part 1; `additions.md` A1–A4): what the AUR's own
//! metadata, a lookalike name, and the change since you last built a package
//! say about whether to trust a build. Rules only: no scores, no guessing.
//! Every function is pure; `Sources::aur_trust` gathers the facts.
//! See `docs/plan/r2/u20-trust.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Note,
    Warning,
    /// Stops for the same default-no question as a high audit finding.
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signal {
    pub level: Level,
    pub code: &'static str,
    pub text: String,
}

fn signal(level: Level, code: &'static str, text: impl Into<String>) -> Signal {
    Signal {
        level,
        code,
        text: text.into(),
    }
}

// ----------------------------------------------------------- lookalikes

/// Whether two names are one edit apart: a letter added, dropped or changed,
/// or two neighbouring letters swapped.
pub fn one_edit_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        return match diff.as_slice() {
            [_] => true,
            [i, j] => *j == i + 1 && a[*i] == b[*j] && a[*j] == b[*i],
            _ => false,
        };
    }
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let skip = (0..long.len()).find(|&i| i >= short.len() || short[i] != long[i]);
    skip.is_some_and(|i| long[..i] == short[..i] && long[i + 1..] == short[i..])
}

fn words(name: &str) -> Vec<&str> {
    name.split(['-', '_']).filter(|w| !w.is_empty()).collect()
}

/// Official or installed names that `name` imitates. `known` are names it
/// must not be mistaken for; `name` itself is never one of them.
pub fn lookalikes<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut sorted_words = words(name);
    sorted_words.sort_unstable();
    for other in known {
        if other == name || other.len() < 4 || name.len() < 4 {
            continue;
        }
        let near = one_edit_apart(name, other);
        let mut other_words = words(other);
        other_words.sort_unstable();
        // The same words in another order, or with the separators changed.
        let reordered = words(name).len() > 1 && sorted_words == other_words;
        if near || reordered {
            out.push(other.to_string());
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

/// `x` when `name` is `x-bin`, `x-git` or similar of a name that exists.
fn variant_of<'a>(name: &str, official: &BTreeSet<&'a str>) -> Option<&'a str> {
    for suffix in ["-bin", "-appimage", "-prebuilt"] {
        if let Some(base) = name.strip_suffix(suffix) {
            if let Some(found) = official.get(base) {
                return Some(found);
            }
        }
    }
    None
}

// ------------------------------------------------------ the AUR's data

/// What the AUR says about a package.
#[derive(Debug, Clone, Default)]
pub struct AurFacts {
    pub name: String,
    pub maintainer: Option<String>,
    pub first_submitted: Option<i64>,
    pub votes: u32,
    pub out_of_date: bool,
    pub provides: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
}

/// What emrac recorded when this package was last built.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Recorded {
    pub maintainer: Option<String>,
    /// Hosts the sources came from.
    #[serde(default)]
    pub hosts: BTreeSet<String>,
    /// Pinned `validpgpkeys`.
    #[serde(default)]
    pub keys: BTreeSet<String>,
}

/// The signals from the AUR's data and the name alone.
pub fn signals_from_aur(
    f: &AurFacts,
    official: &BTreeSet<&str>,
    installed: &[String],
    recorded: Option<&Recorded>,
    now: i64,
) -> Vec<Signal> {
    let mut out = Vec::new();

    if !official.contains(f.name.as_str()) {
        let near = lookalikes(
            &f.name,
            official
                .iter()
                .copied()
                .chain(installed.iter().map(String::as_str)),
        );
        if !near.is_empty() {
            out.push(signal(
                Level::Warning,
                "lookalike",
                format!(
                    "the name looks like {}, which is a different package",
                    near.join(", ")
                ),
            ));
        }
    }
    if let Some(base) = variant_of(&f.name, official) {
        out.push(signal(
            Level::Note,
            "variant",
            format!("a binary repackaging of the official package '{base}'"),
        ));
    }
    // `x-git` providing and conflicting with `x` is how a development build
    // is meant to work; only claiming some other official name is odd.
    let own_base = [
        "-git",
        "-svn",
        "-hg",
        "-bzr",
        "-nightly",
        "-bin",
        "-appimage",
        "-prebuilt",
    ]
    .iter()
    .find_map(|suffix| f.name.strip_suffix(suffix))
    .unwrap_or(&f.name);
    let claimed: Vec<&str> = f
        .provides
        .iter()
        .chain(&f.conflicts)
        .chain(&f.replaces)
        .map(|s| s.split(['<', '>', '=']).next().unwrap_or(s))
        .filter(|n| official.contains(n) && *n != own_base)
        .collect();
    if !claimed.is_empty() {
        let mut uniq: Vec<&str> = claimed;
        uniq.sort_unstable();
        uniq.dedup();
        out.push(signal(
            Level::Warning,
            "claims-official",
            format!(
                "provides, conflicts with or replaces the official package {}",
                uniq.join(", ")
            ),
        ));
    }

    match &f.maintainer {
        None => out.push(signal(
            Level::Warning,
            "orphaned",
            "nobody maintains it (orphaned)",
        )),
        Some(now_maintainer) => {
            if let Some(Recorded {
                maintainer: Some(was),
                ..
            }) = recorded
            {
                if was != now_maintainer {
                    out.push(signal(
                        Level::High,
                        "maintainer-changed",
                        format!("the maintainer changed from {was} to {now_maintainer} since you last built it"),
                    ));
                }
            }
        }
    }
    if let Some(first) = f.first_submitted {
        let days = (now - first).max(0) / 86_400;
        if days < 30 {
            out.push(signal(
                Level::Warning,
                "new",
                format!(
                    "first submitted {days} day{} ago",
                    if days == 1 { "" } else { "s" }
                ),
            ));
            if f.votes < 5 {
                out.push(signal(
                    Level::Note,
                    "few-users",
                    format!(
                        "only {} vote{}",
                        f.votes,
                        if f.votes == 1 { "" } else { "s" }
                    ),
                ));
            }
        }
    }
    if f.out_of_date {
        out.push(signal(
            Level::Note,
            "out-of-date",
            "flagged out of date on the AUR",
        ));
    }
    out
}

// ------------------------------------------------------------ .SRCINFO

/// The parts of a `.SRCINFO` that matter here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Srcinfo {
    pub pkgver: String,
    pub pkgrel: String,
    pub sources: Vec<String>,
    pub checksums: Vec<String>,
    pub keys: BTreeSet<String>,
}

pub fn parse_srcinfo(text: &str) -> Srcinfo {
    let mut s = Srcinfo::default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(" = ") else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "pkgver" if s.pkgver.is_empty() => s.pkgver = value,
            "pkgrel" if s.pkgrel.is_empty() => s.pkgrel = value,
            k if k == "source" || k.starts_with("source_") => s.sources.push(value),
            k if k.ends_with("sums") || k.contains("sums_") => s.checksums.push(value),
            "validpgpkeys" => {
                s.keys.insert(value.to_ascii_uppercase());
            }
            _ => {}
        }
    }
    s
}

/// `host` of a source address (`name::https://host/path` is understood);
/// `None` for a local file.
pub fn source_host(source: &str) -> Option<String> {
    let url = source.rsplit_once("::").map_or(source, |(_, u)| u);
    let url = url.strip_prefix("git+").unwrap_or(url);
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

pub fn hosts_of(info: &Srcinfo) -> BTreeSet<String> {
    info.sources.iter().filter_map(|s| source_host(s)).collect()
}

/// The signals from comparing the last reviewed `.SRCINFO` (`old`) with the
/// one about to be built (`new`) and what was recorded.
pub fn signals_from_change(
    old: Option<&Srcinfo>,
    new: &Srcinfo,
    recorded: Option<&Recorded>,
) -> Vec<Signal> {
    let mut out = Vec::new();
    if let Some(old) = old {
        let same_version = old.pkgver == new.pkgver && old.pkgrel == new.pkgrel;
        let sources_differ = old.sources != new.sources || old.checksums != new.checksums;
        // A VCS source has a fixed address and a `SKIP` checksum: a change of the
        // address alone is still a change; a changed checksum list that is all
        // SKIP on both sides is not.
        if same_version && sources_differ {
            out.push(signal(
                Level::High,
                "source-changed",
                format!(
                    "the sources or their checksums changed but the version is still {}-{}",
                    new.pkgver, new.pkgrel
                ),
            ));
        }
    }
    let known_hosts: BTreeSet<String> = recorded
        .map(|r| r.hosts.clone())
        .filter(|h| !h.is_empty())
        .or_else(|| old.map(hosts_of).filter(|h| !h.is_empty()))
        .unwrap_or_default();
    if !known_hosts.is_empty() {
        let moved: Vec<String> = hosts_of(new).difference(&known_hosts).cloned().collect();
        if !moved.is_empty() {
            out.push(signal(
                Level::Warning,
                "source-host",
                format!(
                    "a source now comes from {}, which it did not before",
                    moved.join(", ")
                ),
            ));
        }
    }
    if let Some(r) = recorded.filter(|r| !r.keys.is_empty() || !new.keys.is_empty()) {
        if r.keys != new.keys && (!r.keys.is_empty()) {
            let added: Vec<&String> = new.keys.difference(&r.keys).collect();
            let gone: Vec<&String> = r.keys.difference(&new.keys).collect();
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!(
                    "new: {}",
                    added
                        .iter()
                        .map(|k| short_key(k))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !gone.is_empty() {
                parts.push(format!(
                    "gone: {}",
                    gone.iter()
                        .map(|k| short_key(k))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            out.push(signal(
                Level::High,
                "keys-changed",
                format!(
                    "the signing keys changed since you last built it ({})",
                    parts.join("; ")
                ),
            ));
        } else if r.keys.is_empty() && !new.keys.is_empty() {
            out.push(signal(
                Level::Note,
                "keys-added",
                "it now names signing keys to verify the sources",
            ));
        }
    }
    out
}

fn short_key(k: &str) -> String {
    if k.len() > 16 {
        format!("…{}", &k[k.len() - 16..])
    } else {
        k.to_string()
    }
}

/// What to remember after a build you went ahead with.
pub fn record_of(f: &AurFacts, new: &Srcinfo) -> Recorded {
    Recorded {
        maintainer: f.maintainer.clone(),
        hosts: hosts_of(new),
        keys: new.keys.clone(),
    }
}

// ----------------------------------------------------------------- state

/// `~/.local/share/emrac/aur-trust.json`.
pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/aur-trust.json"))
}

pub type Store = BTreeMap<String, Recorded>;

static HELD: std::sync::Mutex<BTreeMap<String, Recorded>> = std::sync::Mutex::new(BTreeMap::new());

/// Keeps what was seen while reviewing `name` until it is built (`commit`).
pub fn hold(name: &str, rec: Recorded) {
    HELD.lock().unwrap().insert(name.to_string(), rec);
}

/// Writes what `hold` kept for `name`; does nothing if nothing was held.
pub fn commit(name: &str) -> Result<()> {
    let held = HELD.lock().unwrap().remove(name);
    match held {
        Some(rec) => remember(name, rec),
        None => Ok(()),
    }
}

pub fn load() -> Store {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn remember(name: &str, rec: Recorded) -> Result<()> {
    let mut store = load();
    store.insert(name.to_string(), rec);
    let p = path()?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io {
            context: format!("couldn't create {}", dir.display()),
            source: e,
        })?;
    }
    let tmp = p.with_extension("json.part");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&store).expect("serializable"),
    )
    .and_then(|()| std::fs::rename(&tmp, &p))
    .map_err(|e| Error::Io {
        context: format!("couldn't write {}", p.display()),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn official<'a>(names: &[&'a str]) -> BTreeSet<&'a str> {
        names.iter().copied().collect()
    }

    fn facts(name: &str) -> AurFacts {
        AurFacts {
            name: name.into(),
            maintainer: Some("alice".into()),
            first_submitted: Some(0),
            votes: 50,
            ..AurFacts::default()
        }
    }

    const NOW: i64 = 400 * 86_400;

    fn codes(s: &[Signal]) -> Vec<&'static str> {
        s.iter().map(|x| x.code).collect()
    }

    #[test]
    fn names_one_edit_apart_are_found_and_others_are_not() {
        for (a, b) in [
            ("firefox", "firefx"),
            ("firefox", "firefoxx"),
            ("firefox", "firfeox"),
            ("firefox", "firefax"),
        ] {
            assert!(one_edit_apart(a, b), "{a} {b}");
            assert!(one_edit_apart(b, a), "{b} {a}");
        }
        for (a, b) in [
            ("firefox", "firefox"),
            ("firefox", "chromium"),
            ("firefox", "fire"),
            ("abc", "xyz"),
        ] {
            assert!(!one_edit_apart(a, b), "{a} {b}");
        }
    }

    #[test]
    fn reordered_words_and_one_letter_slips_are_lookalikes() {
        let known = ["firefox", "python-requests", "git", "gitt", "wget"];
        assert_eq!(lookalikes("firefx", known.iter().copied()), ["firefox"]);
        assert_eq!(
            lookalikes("requests-python", known.iter().copied()),
            ["python-requests"]
        );
        assert_eq!(
            lookalikes("python_requests", known.iter().copied()),
            ["python-requests"],
            "separators changed"
        );
        assert!(
            lookalikes("firefox", known.iter().copied()).is_empty(),
            "itself"
        );
        assert!(
            lookalikes("git", ["gix"].iter().copied()).is_empty(),
            "too short to judge"
        );
        assert!(lookalikes("my-tool", known.iter().copied()).is_empty());
    }

    #[test]
    fn the_aur_data_gives_the_signals_it_should() {
        let off = official(&["firefox", "vlc", "git"]);
        let s = signals_from_aur(&facts("firefx"), &off, &[], None, NOW);
        assert_eq!(codes(&s), ["lookalike"]);
        let mut f = facts("vlc-bin");
        f.maintainer = None;
        f.out_of_date = true;
        let s = signals_from_aur(&f, &off, &[], None, NOW);
        assert_eq!(codes(&s), ["variant", "orphaned", "out-of-date"]);
        let mut f = facts("fancy-git-tool");
        f.provides = vec!["git=2.0".into()];
        f.first_submitted = Some(NOW - 5 * 86_400);
        f.votes = 1;
        let s = signals_from_aur(&f, &off, &[], None, NOW);
        assert_eq!(codes(&s), ["claims-official", "new", "few-users"]);
        assert!(s[1].text.contains("5 days"), "{:?}", s[1]);
        // A development build of an official package is meant to provide it.
        let mut dev = facts("vlc-git");
        dev.provides = vec!["vlc".into()];
        dev.conflicts = vec!["vlc".into()];
        assert!(signals_from_aur(&dev, &off, &[], None, NOW).is_empty());
        dev.provides.push("git".into());
        assert_eq!(
            codes(&signals_from_aur(&dev, &off, &[], None, NOW)),
            ["claims-official"]
        );
        let calm = signals_from_aur(&facts("my-own-thing"), &off, &[], None, NOW);
        assert!(calm.is_empty(), "{calm:?}");
        // near an installed (AUR) package too
        let s = signals_from_aur(
            &facts("quickshel-git"),
            &off,
            &["quickshell-git".to_string()],
            None,
            NOW,
        );
        assert_eq!(codes(&s), ["lookalike"]);
    }

    #[test]
    fn a_changed_maintainer_is_high_only_against_what_was_recorded() {
        let off = official(&[]);
        let rec = Recorded {
            maintainer: Some("bob".into()),
            ..Recorded::default()
        };
        let s = signals_from_aur(&facts("tool"), &off, &[], Some(&rec), NOW);
        assert_eq!(codes(&s), ["maintainer-changed"]);
        assert_eq!(s[0].level, Level::High);
        let same = Recorded {
            maintainer: Some("alice".into()),
            ..Recorded::default()
        };
        assert!(signals_from_aur(&facts("tool"), &off, &[], Some(&same), NOW).is_empty());
        assert!(
            signals_from_aur(&facts("tool"), &off, &[], None, NOW).is_empty(),
            "nothing recorded: nothing to compare"
        );
    }

    const OLD: &str = "pkgbase = x\n\tpkgver = 1.0\n\tpkgrel = 1\n\tsource = x-1.0.tar.gz::https://github.com/a/x/archive/1.0.tar.gz\n\tsha256sums = aaaa\n\tvalidpgpkeys = abcdef0123456789ABCDEF0123456789abcdef01\n\npkgname = x\n";

    #[test]
    fn srcinfo_is_read_and_hosts_found() {
        let s = parse_srcinfo(OLD);
        assert_eq!((s.pkgver.as_str(), s.pkgrel.as_str()), ("1.0", "1"));
        assert_eq!(s.sources.len(), 1);
        assert_eq!(s.checksums, ["aaaa"]);
        assert_eq!(
            s.keys.iter().next().unwrap(),
            "ABCDEF0123456789ABCDEF0123456789ABCDEF01"
        );
        assert_eq!(hosts_of(&s).into_iter().collect::<Vec<_>>(), ["github.com"]);
        for (src, host) in [
            ("https://Example.org:8443/a/b", Some("example.org")),
            ("git+https://github.com/a/b.git#tag=1", Some("github.com")),
            ("name::ssh://git@host.example/repo", Some("host.example")),
            ("local.patch", None),
            ("file.tar.gz::local", None),
        ] {
            assert_eq!(source_host(src).as_deref(), host, "{src}");
        }
    }

    #[test]
    fn sources_that_change_under_the_same_version_are_high_and_a_new_host_is_a_warning() {
        let old = parse_srcinfo(OLD);
        let tampered = parse_srcinfo(&OLD.replace("aaaa", "bbbb"));
        let s = signals_from_change(Some(&old), &tampered, None);
        assert_eq!(codes(&s), ["source-changed"]);
        assert_eq!(s[0].level, Level::High);
        let bumped = parse_srcinfo(
            &OLD.replace("aaaa", "bbbb")
                .replace("pkgver = 1.0", "pkgver = 1.1"),
        );
        assert!(
            signals_from_change(Some(&old), &bumped, None).is_empty(),
            "a bump explains the change"
        );
        let moved = parse_srcinfo(
            &OLD.replace("github.com", "evil.example")
                .replace("pkgver = 1.0", "pkgver = 1.1"),
        );
        let s = signals_from_change(Some(&old), &moved, None);
        assert_eq!(codes(&s), ["source-host"]);
        assert!(s[0].text.contains("evil.example"));
        assert!(signals_from_change(Some(&old), &old, None).is_empty());
        assert!(
            signals_from_change(None, &old, None).is_empty(),
            "a first build has nothing to compare"
        );
    }

    #[test]
    fn pinned_keys_must_not_change() {
        let new = parse_srcinfo(OLD);
        let rec = record_of(&facts("x"), &new);
        assert!(signals_from_change(None, &new, Some(&rec)).is_empty());
        let other = parse_srcinfo(&OLD.replace(
            "abcdef0123456789ABCDEF0123456789abcdef01",
            "1111111111111111111111111111111111111111",
        ));
        let s = signals_from_change(None, &other, Some(&rec));
        assert_eq!(codes(&s), ["keys-changed"]);
        assert_eq!(s[0].level, Level::High);
        assert!(
            s[0].text.contains("new:") && s[0].text.contains("gone:"),
            "{:?}",
            s[0]
        );
        // Keys appearing where there were none is only worth a note.
        let none = Recorded {
            keys: BTreeSet::new(),
            ..rec.clone()
        };
        assert_eq!(
            codes(&signals_from_change(None, &new, Some(&none))),
            ["keys-added"]
        );
    }

    #[test]
    fn what_is_remembered_survives_a_round_trip() {
        let new = parse_srcinfo(OLD);
        let rec = record_of(&facts("x"), &new);
        assert_eq!(rec.maintainer.as_deref(), Some("alice"));
        let json = serde_json::to_string(&rec).unwrap();
        assert_eq!(serde_json::from_str::<Recorded>(&json).unwrap(), rec);
    }
}

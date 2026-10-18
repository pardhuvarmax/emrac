//! The Arch Security Tracker (u18 addendum): what `https://security.archlinux.org`
//! says about a package version, kept in a small cache so a score does not need
//! the network every time. See `docs/plan/r3/u18-package-score.md`.
//!
//! The rule is the one `arch-audit` uses: a group applies to a version when its
//! status is `Vulnerable`, the package is named in it, the version is not older
//! than `affected`, and it is older than `fixed` (or no fix is recorded). The
//! tracker does not date its entries, so an old open group with no fix can be
//! stale; every match is listed by its id for that reason.

use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_URL: &str = "https://security.archlinux.org/issues/all.json";
/// The cache is used without asking the network while it is younger than this.
pub const REFRESH_AFTER: u64 = 86_400;
const MAX_BYTES: u64 = 24 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);

fn bad(why: impl Into<String>) -> Error {
    Error::Security(why.into())
}

/// `EMRAC_SECURITY_URL` (a `file://` path for tests, or a mirror) or the real address.
pub fn url() -> String {
    std::env::var("EMRAC_SECURITY_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub affected: String,
    #[serde(default)]
    pub fixed: Option<String>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn parse(s: &str) -> Severity {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Unknown,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
            Severity::Unknown => "unknown severity",
        }
    }

    /// Points a group of this severity takes off a security score.
    pub fn points(self) -> i32 {
        match self {
            Severity::Critical => 40,
            Severity::High => 25,
            Severity::Medium => 12,
            Severity::Low | Severity::Unknown => 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Match {
    pub id: String,
    pub severity: Severity,
    pub cves: Vec<String>,
    pub affected: String,
    /// The version that fixes it, when one is recorded.
    pub fixed: Option<String>,
}

/// The groups that apply to `package` at `version`, most severe first.
pub fn matches(
    groups: &[Group],
    package: &str,
    version: &str,
    cmp: &dyn Fn(&str, &str) -> Ordering,
) -> Vec<Match> {
    let mut out: Vec<Match> = groups
        .iter()
        .filter(|g| g.status == "Vulnerable" && g.packages.iter().any(|p| p == package))
        .filter(|g| g.affected.is_empty() || cmp(version, &g.affected) != Ordering::Less)
        .filter(|g| {
            g.fixed
                .as_deref()
                .is_none_or(|f| cmp(version, f) == Ordering::Less)
        })
        .map(|g| Match {
            id: g.name.clone(),
            severity: Severity::parse(&g.severity),
            cves: g.issues.clone(),
            affected: g.affected.clone(),
            fixed: g.fixed.clone(),
        })
        .collect();
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.id.cmp(&b.id)));
    out
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Group>> {
    serde_json::from_slice(bytes).map_err(|e| bad(format!("not a tracker list: {e}")))
}

// ------------------------------------------------------------------ the cache

#[derive(Serialize, Deserialize)]
struct CacheFile {
    fetched_at: u64,
    groups: Vec<Group>,
}

pub fn cache_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/security-tracker.json"))
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

fn fetch(url: &str) -> Result<Vec<Group>> {
    if let Some(path) = url.strip_prefix("file://") {
        return parse(&fs::read(path).map_err(|e| bad(format!("{path}: {e}")))?);
    }
    let mut response = ureq::get(url)
        .header("User-Agent", concat!("emrac/", env!("CARGO_PKG_VERSION")))
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .map_err(|e| bad(format!("couldn't fetch {url}: {e}")))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| bad(format!("couldn't read {url}: {e}")))?;
    parse(&bytes)
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub groups: Vec<Group>,
    /// Unix seconds when the list was fetched.
    pub fetched_at: u64,
    /// Said when the data is older than a refresh would give (offline, or the fetch failed).
    pub note: Option<String>,
}

/// The tracker's groups: the cache while it is fresh, else a new fetch (not when
/// `offline`), else an older cache with a note, else an error saying why nothing
/// could be checked.
pub fn load(offline: bool, now: u64) -> Result<Loaded> {
    load_with(&cache_path()?, &url(), offline, now)
}

pub fn load_with(cache: &std::path::Path, url: &str, offline: bool, now: u64) -> Result<Loaded> {
    let cached = read_cache(cache);
    if let Some(c) = &cached {
        if now.saturating_sub(c.fetched_at) < REFRESH_AFTER {
            return Ok(Loaded {
                groups: c.groups.clone(),
                fetched_at: c.fetched_at,
                note: None,
            });
        }
    }
    if offline {
        return match cached {
            Some(c) => Ok(Loaded {
                groups: c.groups,
                fetched_at: c.fetched_at,
                note: Some("--offline is set; using the saved list".into()),
            }),
            None => Err(bad("--offline is set and no list has been saved yet")),
        };
    }
    match fetch(url) {
        Ok(groups) => {
            let _ = write_cache(
                cache,
                &CacheFile {
                    fetched_at: now,
                    groups: groups.clone(),
                },
            );
            Ok(Loaded {
                groups,
                fetched_at: now,
                note: None,
            })
        }
        Err(e) => match cached {
            Some(c) => Ok(Loaded {
                groups: c.groups,
                fetched_at: c.fetched_at,
                note: Some(format!("could not refresh ({e}); using the saved list")),
            }),
            None => Err(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(a: &str, b: &str) -> Ordering {
        // numeric dotted versions are enough for the tests
        let n =
            |v: &str| -> Vec<u64> { v.split(['.', '-']).filter_map(|x| x.parse().ok()).collect() };
        n(a).cmp(&n(b))
    }

    fn group(
        name: &str,
        pkgs: &[&str],
        status: &str,
        sev: &str,
        affected: &str,
        fixed: Option<&str>,
    ) -> Group {
        Group {
            name: name.into(),
            packages: pkgs.iter().map(|s| s.to_string()).collect(),
            status: status.into(),
            severity: sev.into(),
            affected: affected.into(),
            fixed: fixed.map(str::to_string),
            issues: vec![format!("CVE-2026-{}", name.len())],
        }
    }

    #[test]
    fn a_group_applies_between_affected_and_fixed_and_only_while_vulnerable() {
        let g = vec![group(
            "AVG-1",
            &["openssl"],
            "Vulnerable",
            "High",
            "3.0.0-1",
            Some("3.0.5-1"),
        )];
        let at = |v: &str| matches(&g, "openssl", v, &cmp).len();
        assert_eq!(at("2.9.0-1"), 0, "older than affected");
        assert_eq!(at("3.0.0-1"), 1, "affected itself is in");
        assert_eq!(at("3.0.4-9"), 1);
        assert_eq!(at("3.0.5-1"), 0, "fixed itself is out");
        assert_eq!(at("3.1.0-1"), 0, "newer than the fix");
        // other statuses and other packages never apply
        for status in ["Fixed", "Not affected", "Unknown", "Testing"] {
            let g = vec![group("AVG-2", &["openssl"], status, "High", "1.0-1", None)];
            assert!(matches(&g, "openssl", "3.0-1", &cmp).is_empty(), "{status}");
        }
        assert!(matches(&g, "curl", "3.0.1-1", &cmp).is_empty());
    }

    #[test]
    fn with_no_fix_recorded_every_version_from_affected_on_matches() {
        let g = vec![group(
            "AVG-3",
            &["pam"],
            "Vulnerable",
            "Medium",
            "1.7.0-2",
            None,
        )];
        assert_eq!(matches(&g, "pam", "1.7.0-2", &cmp).len(), 1);
        assert_eq!(
            matches(&g, "pam", "9.0-1", &cmp).len(),
            1,
            "the tracker says nothing was fixed"
        );
        assert!(matches(&g, "pam", "1.6.0-1", &cmp).is_empty());
    }

    #[test]
    fn matches_come_most_severe_first_then_by_id_and_carry_their_cves() {
        let g = vec![
            group("AVG-9", &["x"], "Vulnerable", "Low", "1-1", None),
            group(
                "AVG-7",
                &["x"],
                "Vulnerable",
                "Critical",
                "1-1",
                Some("5-1"),
            ),
            group("AVG-8", &["x"], "Vulnerable", "Critical", "1-1", None),
            group("AVG-6", &["x", "y"], "Vulnerable", "Weird", "1-1", None),
        ];
        let m = matches(&g, "x", "2-1", &cmp);
        let ids: Vec<&str> = m.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["AVG-7", "AVG-8", "AVG-9", "AVG-6"]);
        assert_eq!(m[0].severity, Severity::Critical);
        assert_eq!(m[3].severity, Severity::Unknown);
        assert_eq!(m[0].fixed.as_deref(), Some("5-1"));
        assert!(m[0].cves[0].starts_with("CVE-"));
    }

    #[test]
    fn severity_words_points_and_order() {
        assert_eq!(Severity::parse("HIGH"), Severity::High);
        assert_eq!(Severity::parse("whatever"), Severity::Unknown);
        assert!(
            Severity::Critical > Severity::High
                && Severity::High > Severity::Medium
                && Severity::Medium > Severity::Low
        );
        assert_eq!(
            [
                Severity::Critical,
                Severity::High,
                Severity::Medium,
                Severity::Low,
                Severity::Unknown
            ]
            .map(Severity::points),
            [40, 25, 12, 5, 5]
        );
        assert_eq!(Severity::Unknown.word(), "unknown severity");
    }

    #[test]
    fn the_real_list_shape_parses_and_a_broken_one_does_not() {
        let json = br#"[{"name":"AVG-1","packages":["vim"],"status":"Vulnerable","severity":"High","type":"x","affected":"9-1","fixed":null,"ticket":null,"issues":["CVE-1"],"advisories":[]}]"#;
        let g = parse(json).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].fixed, None);
        assert!(parse(b"not json").is_err());
        assert!(parse(b"{}").is_err(), "an object is not a list");
        assert!(parse(b"[]").unwrap().is_empty());
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-tracker-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_cache_is_used_while_fresh_refreshed_when_old_and_kept_when_the_fetch_fails() {
        let d = dir("cache");
        let cache = d.join("cache.json");
        let list = d.join("list.json");
        let url = format!("file://{}", list.display());
        fs::write(&list, br#"[{"name":"AVG-1","packages":["a"],"status":"Vulnerable","severity":"High","affected":"1-1","fixed":null,"issues":[]}]"#).unwrap();
        let now = 10_000_000;
        // nothing cached, offline: an error that says why
        let e = load_with(&cache, &url, true, now).unwrap_err().to_string();
        assert!(
            e.contains("--offline is set and no list has been saved"),
            "{e}"
        );
        // online: fetched and saved
        let l = load_with(&cache, &url, false, now).unwrap();
        assert_eq!(
            (l.groups.len(), l.fetched_at, l.note.is_none()),
            (1, now, true)
        );
        // fresh cache: used without the address (remove it to prove it)
        fs::remove_file(&list).unwrap();
        let l = load_with(&cache, &url, false, now + 3600).unwrap();
        assert_eq!(
            (l.groups.len(), l.fetched_at),
            (1, now),
            "the saved time is kept"
        );
        assert!(l.note.is_none());
        // old cache, fetch fails: the saved list with a note
        let l = load_with(&cache, &url, false, now + 3 * 86_400).unwrap();
        assert_eq!(l.groups.len(), 1);
        assert!(l.note.unwrap().contains("could not refresh"));
        // old cache, offline: the saved list, saying so
        let l = load_with(&cache, &url, true, now + 3 * 86_400).unwrap();
        assert!(l
            .note
            .unwrap()
            .contains("--offline is set; using the saved list"));
        // old cache and a working address: refreshed
        fs::write(&list, b"[]").unwrap();
        let l = load_with(&cache, &url, false, now + 3 * 86_400).unwrap();
        assert_eq!((l.groups.len(), l.fetched_at), (0, now + 3 * 86_400));
        // a nothing-cached, failing address is an error
        let _ = fs::remove_file(&cache);
        fs::remove_file(&list).unwrap();
        assert!(load_with(&cache, &url, false, now).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}

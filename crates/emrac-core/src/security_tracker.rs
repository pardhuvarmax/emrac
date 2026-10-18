//! The Arch Security Tracker (u18 addendum): what `https://security.archlinux.org`
//! says about a package version, kept in a small cache so a score does not need
//! the network every time. See `docs/plan/r3/u18-package-score.md`.
//!
//! The rule is the one `arch-audit` uses: a group applies to a version when its
//! status is `Vulnerable`, the package is named in it, the version is not older
//! than `affected`, and it is older than `fixed` (or no fix is recorded). The
//! tracker does not date its entries and never closes some old ones, so each
//! match is given a *standing* from evidence in its own data: a fix is known,
//! it is open and recent, or it is open and possibly stale (the newest CVE is
//! more than two years old, or the entry is about another major series than the
//! version in question). A person can also ignore an entry; the ignore lapses
//! if the entry later records a fix. Every match is listed by its id.

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

/// How much an applying entry is believed.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// A fix is recorded and this version is older than it.
    FixKnown,
    /// No fix is recorded; the entry looks current.
    OpenRecent,
    /// No fix is recorded; the entry looks old.
    PossiblyStale,
    /// The person ignored it.
    Ignored,
}

impl Standing {
    pub fn word(self) -> &'static str {
        match self {
            Standing::FixKnown => "fix available",
            Standing::OpenRecent => "open",
            Standing::PossiblyStale => "possibly stale",
            Standing::Ignored => "ignored by you",
        }
    }

    /// Percent of the entry's severity points that count.
    pub fn percent(self) -> i32 {
        match self {
            Standing::FixKnown | Standing::OpenRecent => 100,
            Standing::PossiblyStale => 50,
            Standing::Ignored => 0,
        }
    }

    /// Whether a gate (a warning or a block before an install) may use it.
    pub fn confirmed(self) -> bool {
        matches!(self, Standing::FixKnown | Standing::OpenRecent)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    #[serde(flatten)]
    pub found: Match,
    pub standing: Standing,
    /// The evidence for the standing, in a sentence.
    pub why: String,
}

impl Entry {
    /// Points this entry takes off a score.
    pub fn points(&self) -> i32 {
        self.found.severity.points() * self.standing.percent() / 100
    }
}

/// Entries older than this many years, by their newest CVE, are suspect when no fix is recorded.
pub const STALE_YEARS: i32 = 2;

/// The year in the newest of `cves` (`CVE-2022-2068` gives 2022).
pub fn newest_cve_year(cves: &[String]) -> Option<i32> {
    cves.iter()
        .filter_map(|c| {
            c.strip_prefix("CVE-")?
                .split('-')
                .next()?
                .parse::<i32>()
                .ok()
        })
        .max()
}

/// The first number of a version, without its epoch: `1:2.4.1-4` gives 2.
pub fn major_of(version: &str) -> Option<u64> {
    let v = version.split_once(':').map_or(version, |(_, rest)| rest);
    let digits: String = v.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A person's decision to set an entry aside.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ignore {
    pub id: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// The fix the entry recorded when it was ignored (`None`: no fix). If the
    /// entry later records one, the ignore lapses.
    #[serde(default)]
    pub fixed_at_ignore: Option<String>,
    pub created: u64,
}

/// Every applying entry with its standing. `this_year` is the current year.
pub fn assess(
    groups: &[Group],
    package: &str,
    version: &str,
    cmp: &dyn Fn(&str, &str) -> Ordering,
    ignored: &[Ignore],
    this_year: i32,
) -> Vec<Entry> {
    let mut out: Vec<Entry> = matches(groups, package, version, cmp)
        .into_iter()
        .map(|m| {
            let ignore = ignored.iter().find(|i| i.id == m.id);
            let (standing, why) = if let Some(i) = ignore.filter(|i| i.fixed_at_ignore == m.fixed) {
                (
                    Standing::Ignored,
                    format!(
                        "you ignored it{}",
                        i.reason.as_ref().map_or(String::new(), |r| format!(": {r}"))
                    ),
                )
            } else if let Some(fix) = &m.fixed {
                let lapsed = if ignore.is_some() {
                    " (your ignore lapsed: the entry now records a fix)"
                } else {
                    ""
                };
                (
                    Standing::FixKnown,
                    format!("fixed in {fix}, this version is older{lapsed}"),
                )
            } else {
                let old_years = newest_cve_year(&m.cves).map(|y| this_year - y);
                let other_series = match (major_of(version), major_of(&m.affected)) {
                    (Some(a), Some(b)) => a != b,
                    _ => false,
                };
                if let Some(years) = old_years.filter(|y| *y > STALE_YEARS) {
                    (
                        Standing::PossiblyStale,
                        format!("no fix is recorded and its newest CVE is {years} years old"),
                    )
                } else if other_series {
                    (
                        Standing::PossiblyStale,
                        format!(
                            "no fix is recorded and the entry is about the {} series, not {version}",
                            m.affected
                        ),
                    )
                } else {
                    let age = old_years.map_or("its age is not known".to_string(), |y| {
                        format!("its newest CVE is from {}", this_year - y)
                    });
                    (Standing::OpenRecent, format!("no fix is recorded; {age}"))
                }
            };
            Entry { found: m, standing, why }
        })
        .collect();
    out.sort_by(|a, b| {
        (b.standing.confirmed(), b.found.severity)
            .cmp(&(a.standing.confirmed(), a.found.severity))
            .then_with(|| a.found.id.cmp(&b.found.id))
    });
    out
}

/// The current year, from a Unix time.
pub fn year_of(now: u64) -> i32 {
    crate::history::format_utc_date(now)
        .get(..4)
        .and_then(|y| y.parse().ok())
        .unwrap_or(2026)
}

/// `assess` with libalpm's version order, your ignore list and the year of `now`.
pub fn assess_now(groups: &[Group], package: &str, version: &str, now: u64) -> Vec<Entry> {
    assess(
        groups,
        package,
        version,
        &|a, b| alpm::vercmp(a, b),
        &read_ignores(),
        year_of(now),
    )
}

// ------------------------------------------------------------------ report

/// An installed package with the tracker entries that apply to its version.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VulnPackage {
    pub name: String,
    pub version: String,
    pub entries: Vec<Entry>,
    /// The version the repositories have now, when it differs.
    pub repo_version: Option<String>,
}

impl VulnPackage {
    /// Whether the repositories already have the fix for `entry`.
    pub fn fix_in_repos(&self, entry: &Entry, cmp: &dyn Fn(&str, &str) -> Ordering) -> bool {
        match (&entry.found.fixed, &self.repo_version) {
            (Some(fix), Some(repo)) => cmp(repo, fix) != Ordering::Less,
            _ => false,
        }
    }

    pub fn worst_confirmed(&self) -> Option<Severity> {
        self.entries
            .iter()
            .filter(|e| e.standing.confirmed())
            .map(|e| e.found.severity)
            .max()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VulnReport {
    pub list_age_hours: u64,
    /// Said when the list is older than a refresh would give.
    pub note: Option<String>,
    /// Groups in the list.
    pub groups: usize,
    /// Installed packages looked at.
    pub checked: usize,
    pub packages: Vec<VulnPackage>,
}

impl VulnReport {
    /// Confirmed High or Critical entries, across all packages.
    pub fn hard(&self) -> usize {
        self.packages
            .iter()
            .flat_map(|p| &p.entries)
            .filter(|e| e.standing.confirmed() && e.found.severity >= Severity::High)
            .count()
    }
}

/// Every installed package against the list.
pub fn report(
    list: &Loaded,
    installed: &[(String, String)],
    repo_version: &dyn Fn(&str) -> Option<String>,
    cmp: &dyn Fn(&str, &str) -> Ordering,
    ignored: &[Ignore],
    now: u64,
) -> VulnReport {
    let mut by_package: std::collections::HashMap<&str, Vec<Group>> =
        std::collections::HashMap::new();
    for g in list.groups.iter().filter(|g| g.status == "Vulnerable") {
        for p in &g.packages {
            by_package.entry(p.as_str()).or_default().push(g.clone());
        }
    }
    let year = year_of(now);
    let mut packages: Vec<VulnPackage> = installed
        .iter()
        .filter_map(|(name, version)| {
            let groups = by_package.get(name.as_str())?;
            let entries = assess(groups, name, version, cmp, ignored, year);
            (!entries.is_empty()).then(|| VulnPackage {
                name: name.clone(),
                version: version.clone(),
                entries,
                repo_version: repo_version(name).filter(|r| r != version),
            })
        })
        .collect();
    packages.sort_by(|a, b| {
        b.worst_confirmed()
            .cmp(&a.worst_confirmed())
            .then_with(|| a.name.cmp(&b.name))
    });
    VulnReport {
        list_age_hours: now.saturating_sub(list.fetched_at) / 3600,
        note: list.note.clone(),
        groups: list.groups.len(),
        checked: installed.len(),
        packages,
    }
}

// -------------------------------------------------------------- ignore list

pub fn ignore_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/vuln-ignore.json"))
}

pub fn read_ignores_from(path: &std::path::Path) -> Vec<Ignore> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn read_ignores() -> Vec<Ignore> {
    ignore_path()
        .map(|p| read_ignores_from(&p))
        .unwrap_or_default()
}

fn write_ignores(path: &std::path::Path, list: &[Ignore]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| bad(format!("{}: {e}", dir.display())))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(list).unwrap_or_default())
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(|e| bad(format!("{}: {e}", path.display())))
}

/// `AVG-` and digits, nothing else.
pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("AVG-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 8 && n.chars().all(|c| c.is_ascii_digit()))
}

/// Adds (or replaces) an ignore for a group that exists in `groups`.
pub fn add_ignore_to(
    path: &std::path::Path,
    groups: &[Group],
    id: &str,
    reason: Option<&str>,
    now: u64,
) -> Result<Ignore> {
    if !valid_id(id) {
        return Err(bad(format!(
            "'{id}' is not an entry id; they look like AVG-2765"
        )));
    }
    let group = groups
        .iter()
        .find(|g| g.name == id)
        .ok_or_else(|| bad(format!("{id} is not in the tracker list that was loaded")))?;
    let entry = Ignore {
        id: id.to_string(),
        reason: reason.map(|r| r.chars().take(200).collect()),
        fixed_at_ignore: group.fixed.clone(),
        created: now,
    };
    let mut list = read_ignores_from(path);
    list.retain(|i| i.id != id);
    list.push(entry.clone());
    list.sort_by(|a, b| a.id.cmp(&b.id));
    write_ignores(path, &list)?;
    Ok(entry)
}

pub fn remove_ignore_from(path: &std::path::Path, id: &str) -> Result<()> {
    let mut list = read_ignores_from(path);
    let before = list.len();
    list.retain(|i| i.id != id);
    if list.len() == before {
        return Err(bad(format!("{id} is not ignored")));
    }
    write_ignores(path, &list)
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Group>> {
    let groups: Vec<Group> =
        serde_json::from_slice(bytes).map_err(|e| bad(format!("not a tracker list: {e}")))?;
    if let Some(g) = groups.iter().find(|g| !valid_id(&g.name)) {
        return Err(bad(format!(
            "'{}' is not an entry id; this is not a tracker list",
            g.name
        )));
    }
    Ok(groups)
}

/// A new list must not be much smaller than the saved one: a truncated or
/// tampered list would otherwise make every vulnerability disappear.
pub const SHRINK_LIMIT_PERCENT: usize = 50;

fn check_not_shrunk(new: &[Group], saved: Option<&CacheFile>) -> Result<()> {
    if let Some(old) = saved {
        if new.len() * 100 < old.groups.len() * SHRINK_LIMIT_PERCENT {
            return Err(bad(format!(
                "the new list has {} entries against {} saved; a list that shrinks that much is refused",
                new.len(),
                old.groups.len()
            )));
        }
    }
    Ok(())
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
    if !url.starts_with("https://") {
        return Err(bad(format!(
            "{url} is not an https:// address; the list is only fetched over https"
        )));
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
    match fetch(url).and_then(|g| check_not_shrunk(&g, cached.as_ref()).map(|()| g)) {
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
        fs::write(
            &list,
            br#"[{"name":"AVG-2","packages":["b"],"status":"Fixed"}]"#,
        )
        .unwrap();
        let l = load_with(&cache, &url, false, now + 3 * 86_400).unwrap();
        assert_eq!((l.groups.len(), l.fetched_at), (1, now + 3 * 86_400));
        assert_eq!(
            l.groups[0].name, "AVG-2",
            "the new list replaced the saved one"
        );
        // an empty list over a saved one is refused: it would make everything disappear
        fs::write(&list, b"[]").unwrap();
        let l = load_with(&cache, &url, false, now + 6 * 86_400).unwrap();
        assert_eq!(l.groups[0].name, "AVG-2");
        assert!(l.note.unwrap().contains("refused"));
        // a nothing-cached, failing address is an error
        let _ = fs::remove_file(&cache);
        fs::remove_file(&list).unwrap();
        assert!(load_with(&cache, &url, false, now).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    fn open_group(
        id: &str,
        pkg: &str,
        affected: &str,
        fixed: Option<&str>,
        cves: &[&str],
    ) -> Group {
        Group {
            name: id.into(),
            packages: vec![pkg.into()],
            status: "Vulnerable".into(),
            severity: "High".into(),
            affected: affected.into(),
            fixed: fixed.map(str::to_string),
            issues: cves.iter().map(|c| c.to_string()).collect(),
        }
    }

    fn standing_of(g: Group, version: &str, ignored: &[Ignore]) -> (Standing, String) {
        let e = assess(&[g], "p", version, &cmp, ignored, 2026).remove(0);
        (e.standing, e.why)
    }

    #[test]
    fn a_recorded_fix_is_a_known_fix_whatever_the_age() {
        let g = open_group("AVG-1", "p", "1.0-1", Some("3.0-1"), &["CVE-2015-1"]);
        let (s, why) = standing_of(g, "2.0-1", &[]);
        assert_eq!(s, Standing::FixKnown);
        assert!(
            why.contains("fixed in 3.0-1, this version is older"),
            "{why}"
        );
    }

    #[test]
    fn an_open_entry_with_a_recent_cve_in_the_same_series_is_open() {
        for cves in [
            &["CVE-2026-1"][..],
            &["CVE-2025-9", "CVE-2024-3"],
            &["CVE-2024-1"],
        ] {
            let g = open_group("AVG-2", "p", "2.0-1", None, cves);
            assert_eq!(
                standing_of(g, "2.5-1", &[]).0,
                Standing::OpenRecent,
                "{cves:?}"
            );
        }
        // no CVE listed: the age is not known, and it is not called stale for that
        let (s, why) = standing_of(open_group("AVG-3", "p", "2.0-1", None, &[]), "2.5-1", &[]);
        assert_eq!(s, Standing::OpenRecent);
        assert!(why.contains("its age is not known"), "{why}");
    }

    #[test]
    fn an_open_entry_whose_newest_cve_is_over_two_years_old_is_possibly_stale() {
        let at = |year: &str| {
            standing_of(
                open_group("AVG-4", "p", "2.0-1", None, &[year]),
                "2.5-1",
                &[],
            )
        };
        assert_eq!(
            at("CVE-2024-1").0,
            Standing::OpenRecent,
            "exactly two years is not over two"
        );
        let (s, why) = at("CVE-2023-1");
        assert_eq!(s, Standing::PossiblyStale);
        assert!(why.contains("its newest CVE is 3 years old"), "{why}");
        // the newest of several decides
        let g = open_group("AVG-5", "p", "2.0-1", None, &["CVE-2019-1", "CVE-2026-2"]);
        assert_eq!(standing_of(g, "2.5-1", &[]).0, Standing::OpenRecent);
    }

    #[test]
    fn an_entry_about_another_major_series_is_possibly_stale_even_if_recent() {
        let g = open_group("AVG-6", "p", "1.1.1.o-1", None, &["CVE-2026-1"]);
        let (s, why) = standing_of(g, "3.6.5-1", &[]);
        assert_eq!(s, Standing::PossiblyStale);
        assert!(
            why.contains("the entry is about the 1.1.1.o-1 series, not 3.6.5-1"),
            "{why}"
        );
        // an epoch does not count as the series
        let g = open_group("AVG-7", "p", "1:2.4-1", None, &["CVE-2026-1"]);
        assert_eq!(standing_of(g, "1:2.9-1", &[]).0, Standing::OpenRecent);
        assert_eq!(
            (
                major_of("1:2.4.1-4"),
                major_of("5.15.8.arch1-1"),
                major_of("zzz")
            ),
            (Some(2), Some(5), None)
        );
    }

    #[test]
    fn the_standing_sets_the_points_and_what_a_gate_may_use() {
        let g = open_group("AVG-8", "p", "1.0-1", None, &["CVE-2019-1"]);
        let stale = assess(std::slice::from_ref(&g), "p", "1.5-1", &cmp, &[], 2026).remove(0);
        assert_eq!(
            (stale.standing, stale.points(), stale.standing.confirmed()),
            (Standing::PossiblyStale, 12, false),
            "half of 25"
        );
        let fresh = assess(
            &[open_group("AVG-8", "p", "1.0-1", None, &["CVE-2026-1"])],
            "p",
            "1.5-1",
            &cmp,
            &[],
            2026,
        )
        .remove(0);
        assert_eq!((fresh.points(), fresh.standing.confirmed()), (25, true));
        let ig = [Ignore {
            id: "AVG-8".into(),
            reason: None,
            fixed_at_ignore: None,
            created: 1,
        }];
        let ignored = assess(&[g], "p", "1.5-1", &cmp, &ig, 2026).remove(0);
        assert_eq!(
            (
                ignored.standing,
                ignored.points(),
                ignored.standing.confirmed()
            ),
            (Standing::Ignored, 0, false)
        );
        assert_eq!(Standing::FixKnown.word(), "fix available");
    }

    #[test]
    fn an_ignore_holds_until_the_entry_records_a_fix() {
        let ig = vec![Ignore {
            id: "AVG-9".into(),
            reason: Some("disputed upstream".into()),
            fixed_at_ignore: None,
            created: 1,
        }];
        let (s, why) = standing_of(
            open_group("AVG-9", "p", "1.0-1", None, &["CVE-2026-1"]),
            "1.5-1",
            &ig,
        );
        assert_eq!(s, Standing::Ignored);
        assert!(why.contains("you ignored it: disputed upstream"), "{why}");
        // the entry now records a fix this version is older than: the ignore lapses
        let (s, why) = standing_of(
            open_group("AVG-9", "p", "1.0-1", Some("2.0-1"), &["CVE-2026-1"]),
            "1.5-1",
            &ig,
        );
        assert_eq!(s, Standing::FixKnown);
        assert!(why.contains("your ignore lapsed"), "{why}");
        // another entry is not affected by it
        let other = standing_of(
            open_group("AVG-10", "p", "1.0-1", None, &["CVE-2026-1"]),
            "1.5-1",
            &ig,
        );
        assert_eq!(other.0, Standing::OpenRecent);
    }

    #[test]
    fn entries_come_confirmed_first_then_by_severity() {
        let mut stale = open_group("AVG-20", "p", "1.0-1", None, &["CVE-2015-1"]);
        stale.severity = "Critical".into();
        let recent = open_group("AVG-21", "p", "1.0-1", None, &["CVE-2026-1"]);
        let e = assess(&[stale, recent], "p", "1.5-1", &cmp, &[], 2026);
        assert_eq!(
            e[0].found.id, "AVG-21",
            "the confirmed one first, though the stale one is more severe"
        );
        assert_eq!(e[1].found.id, "AVG-20");
    }

    #[test]
    fn years_come_from_the_cve_ids() {
        let y = |c: &[&str]| newest_cve_year(&c.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(y(&["CVE-2022-2068", "CVE-2024-1"]), Some(2024));
        assert_eq!(y(&["nonsense", "CVE-x-1"]), None);
        assert_eq!(y(&[]), None);
    }

    #[test]
    fn the_ignore_list_is_kept_validated_and_sorted() {
        let d = dir("ignore");
        let path = d.join("ignore.json");
        let groups = vec![
            open_group("AVG-2765", "p", "1.0-1", None, &[]),
            open_group("AVG-1", "p", "1.0-1", Some("2-1"), &[]),
        ];
        let a = add_ignore_to(&path, &groups, "AVG-2765", Some("old"), 5).unwrap();
        assert_eq!((a.fixed_at_ignore, a.created), (None, 5));
        let b = add_ignore_to(&path, &groups, "AVG-1", None, 6).unwrap();
        assert_eq!(
            b.fixed_at_ignore.as_deref(),
            Some("2-1"),
            "the fix at the time is kept"
        );
        let ids: Vec<String> = read_ignores_from(&path).into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["AVG-1", "AVG-2765"]);
        // again replaces, not duplicates
        add_ignore_to(&path, &groups, "AVG-1", Some("again"), 7).unwrap();
        assert_eq!(read_ignores_from(&path).len(), 2);
        for bad in [
            "avg-1",
            "AVG-",
            "AVG-x",
            "CVE-2022-1",
            "AVG-1; rm",
            "../AVG-1",
            "",
        ] {
            assert!(
                add_ignore_to(&path, &groups, bad, None, 1).is_err(),
                "{bad:?}"
            );
        }
        assert!(add_ignore_to(&path, &groups, "AVG-9999", None, 1)
            .unwrap_err()
            .to_string()
            .contains("is not in the tracker list"));
        remove_ignore_from(&path, "AVG-1").unwrap();
        assert!(remove_ignore_from(&path, "AVG-1").is_err());
        assert_eq!(read_ignores_from(&path).len(), 1);
        let long = "x".repeat(500);
        let c = add_ignore_to(&path, &groups, "AVG-2765", Some(&long), 8).unwrap();
        assert_eq!(c.reason.unwrap().len(), 200, "a reason is cut");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_list_that_is_not_a_tracker_list_or_shrinks_too_much_is_refused() {
        assert!(parse(br#"[{"name":"not-an-id","packages":[],"status":"Fixed"}]"#).is_err());
        assert!(parse(br#"[{"name":"AVG-1","packages":[],"status":"Fixed"}]"#).is_ok());
        let d = dir("shrink");
        let cache = d.join("c.json");
        let list = d.join("l.json");
        let url = format!("file://{}", list.display());
        let many: Vec<String> = (1..=10)
            .map(|i| format!(r#"{{"name":"AVG-{i}","packages":["a"],"status":"Fixed"}}"#))
            .collect();
        fs::write(&list, format!("[{}]", many.join(","))).unwrap();
        load_with(&cache, &url, false, 1_000).unwrap();
        // four of ten: less than half is refused, the saved list is kept with a note
        fs::write(&list, format!("[{}]", many[..4].join(","))).unwrap();
        let l = load_with(&cache, &url, false, 1_000 + 3 * 86_400).unwrap();
        assert_eq!(l.groups.len(), 10);
        assert!(l
            .note
            .unwrap()
            .contains("a list that shrinks that much is refused"));
        // five of ten is allowed
        fs::write(&list, format!("[{}]", many[..5].join(","))).unwrap();
        assert_eq!(
            load_with(&cache, &url, false, 1_000 + 3 * 86_400)
                .unwrap()
                .groups
                .len(),
            5
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn only_https_is_fetched() {
        for url in [
            "http://security.archlinux.org/issues/all.json",
            "ftp://x/y",
            "security.archlinux.org",
        ] {
            let e = fetch(url).unwrap_err().to_string();
            assert!(e.contains("only fetched over https"), "{url}: {e}");
        }
    }
}

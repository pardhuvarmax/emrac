//! What emrac and pacman keep on disk, and what is safe to remove. Pure
//! decisions (`plan`) are kept apart from the scan and the deletion so they can
//! be tested without touching anything. See `docs/plan/r1/u28-cache.md`.

use std::collections::{HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    Source,
    Build,
    Binary,
    Package,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::Source, Scope::Build, Scope::Binary, Scope::Package];

    /// The scope a word or its letter names: `source`/`s`, `build`/`b`,
    /// `binary`/`n`, `package`/`p`.
    pub fn parse(text: &str) -> Option<Scope> {
        match text.to_ascii_lowercase().as_str() {
            "source" | "s" => Some(Scope::Source),
            "build" | "b" => Some(Scope::Build),
            "binary" | "n" => Some(Scope::Binary),
            "package" | "p" => Some(Scope::Package),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::Source => "source",
            Scope::Build => "build",
            Scope::Binary => "binary",
            Scope::Package => "package",
        }
    }
}

/// One file or tree in a cache.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub path: PathBuf,
    pub scope: Scope,
    /// The package it belongs to (build folder, or parsed from the file name).
    pub package: String,
    /// Set for package files: the version in the file name.
    pub version: Option<String>,
    pub bytes: u64,
    /// Unix seconds.
    pub modified: u64,
    pub is_tree: bool,
}

/// What an operation does to an entry, and why.
#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub entry: Entry,
    pub remove: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Remove what is safe to lose.
    Clean,
    /// Remove everything in the scope.
    Purge,
    /// Remove the oldest of what `clean` would, until each scope is under the
    /// limit (bytes).
    Trim(u64),
}

/// What the decisions depend on besides the entries.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Installed package versions.
    pub installed: HashMap<String, String>,
    /// Versions named by recent transactions, which `undo`/`redo` need.
    pub recorded: HashSet<(String, String)>,
    /// Remove only entries at least this many seconds old.
    pub older_than: Option<u64>,
    pub now: u64,
    /// How many seconds old a source file must be to go (default 30 days).
    pub source_age: u64,
    /// The lean policy (`docs/plan/r1/u29-lean-cache.md`): package files are kept
    /// only for what undoing the last transactions needs and what cannot be
    /// downloaded again.
    pub lean: bool,
    /// Versions needed to undo the last committed transactions.
    pub undo_needs: HashSet<(String, String)>,
    /// Installed packages that are not in any repository (AUR builds).
    pub foreign: HashSet<String>,
    /// The version of each installed official package that the repositories
    /// currently offer.
    pub in_repo: HashMap<String, String>,
}

pub const DEFAULT_SOURCE_AGE: u64 = 30 * 86_400;

/// `(name, version)` from `<name>-<pkgver>-<pkgrel>-<arch>.pkg.tar[.ext]`.
pub fn parse_package_file(file_name: &str) -> Option<(String, String)> {
    let (head, ext) = file_name.split_once(".pkg.tar")?;
    if !(ext.is_empty()
        || [".zst", ".xz", ".gz", ".bz2", ".lz4", ".lrz", ".lzo", ".Z"].contains(&ext))
    {
        return None;
    }
    let (rest, arch) = head.rsplit_once('-')?;
    let (rest, pkgrel) = rest.rsplit_once('-')?;
    let (name, pkgver) = rest.rsplit_once('-')?;
    if name.is_empty() || pkgver.is_empty() || arch.is_empty() || pkgrel.is_empty() {
        return None;
    }
    Some((name.to_string(), format!("{pkgver}-{pkgrel}")))
}

/// `30d`, `12h`, `2w`, `90m`, `45s` (a bare number is seconds), in seconds.
pub fn parse_duration(text: &str) -> Result<u64> {
    let bad = || Error::InvalidCacheValue {
        what: "duration",
        value: text.to_string(),
        hint: "use a number and a unit, such as 30d, 12h or 2w",
    };
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().map_err(|_| bad())?;
    let unit = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        _ => return Err(bad()),
    };
    Ok(number * unit)
}

/// `500M`, `10G`, `10GB`, `10GiB`, `1.5G` (1024-based, like the sizes emrac prints).
pub fn parse_size(text: &str) -> Result<u64> {
    let bad = || Error::InvalidCacheValue {
        what: "size",
        value: text.to_string(),
        hint: "use a number and a unit, such as 500M or 10G",
    };
    let lower = text.trim().to_ascii_lowercase();
    let split = lower
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(lower.len());
    let (number, unit) = lower.split_at(split);
    let number: f64 = number.parse().map_err(|_| bad())?;
    let factor: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return Err(bad()),
    };
    if number < 0.0 || !number.is_finite() {
        return Err(bad());
    }
    Ok((number * factor) as u64)
}

/// Decides what each entry's fate is under `op` within `scopes`.
pub fn plan(
    entries: &[Entry],
    scopes: &[Scope],
    op: Op,
    facts: &Facts,
    newer: &dyn Fn(&str, &str) -> bool,
) -> Vec<Decision> {
    let in_scope: Vec<&Entry> = entries
        .iter()
        .filter(|e| scopes.contains(&e.scope))
        .collect();
    let kept = protected(entries, facts, newer);
    let old_enough = |e: &Entry| {
        facts
            .older_than
            .is_none_or(|age| facts.now.saturating_sub(e.modified) >= age)
    };

    let mut decisions: Vec<Decision> = in_scope
        .iter()
        .map(|&e| {
            let (remove, reason) = match op {
                Op::Purge => (true, "purged".to_string()),
                Op::Clean | Op::Trim(_) => clean_verdict(e, facts, &kept, &old_enough),
            };
            Decision {
                entry: e.clone(),
                remove,
                reason,
            }
        })
        .collect();

    if let Op::Trim(limit) = op {
        // Remove the oldest of what clean would, scope by scope, only until
        // the scope fits.
        for scope in scopes {
            let total: u64 = decisions
                .iter()
                .filter(|d| d.entry.scope == *scope)
                .map(|d| d.entry.bytes)
                .sum();
            let mut candidates: Vec<usize> = (0..decisions.len())
                .filter(|&i| decisions[i].entry.scope == *scope && decisions[i].remove)
                .collect();
            candidates.sort_by_key(|&i| decisions[i].entry.modified);
            let mut remaining = total;
            let mut take: HashSet<usize> = HashSet::new();
            for i in candidates {
                if remaining <= limit {
                    break;
                }
                remaining -= decisions[i].entry.bytes;
                take.insert(i);
            }
            for (i, d) in decisions.iter_mut().enumerate() {
                if d.entry.scope == *scope && d.remove {
                    if take.contains(&i) {
                        d.reason = format!("oldest, to get under the limit ({})", d.reason);
                    } else {
                        d.remove = false;
                        d.reason = "already under the limit".to_string();
                    }
                }
            }
        }
    }
    decisions
}

fn clean_verdict(
    e: &Entry,
    facts: &Facts,
    kept: &HashMap<(String, String), &'static str>,
    old_enough: &dyn Fn(&Entry) -> bool,
) -> (bool, String) {
    if let Some(version) = &e.version {
        if let Some(why) = kept.get(&(e.package.clone(), version.clone())) {
            return (false, format!("kept: {why}"));
        }
    }
    if !old_enough(e) {
        return (false, "newer than --older-than".to_string());
    }
    match e.scope {
        Scope::Build => (
            true,
            "extraction and staging tree; the next build recreates it".into(),
        ),
        Scope::Source => {
            if facts.now.saturating_sub(e.modified) >= facts.source_age {
                (true, "old downloaded source".into())
            } else {
                (false, "kept: downloaded recently".into())
            }
        }
        Scope::Binary | Scope::Package => (true, "an older package file nothing needs".into()),
    }
}

/// The package files to keep whatever the operation: the installed version, the
/// newest older one, and any version a recent transaction names.
fn protected(
    entries: &[Entry],
    facts: &Facts,
    newer: &dyn Fn(&str, &str) -> bool,
) -> HashMap<(String, String), &'static str> {
    let mut by_name: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in entries {
        if let (Scope::Binary | Scope::Package, Some(v)) = (e.scope, &e.version) {
            by_name.entry(&e.package).or_default().push(v);
        }
    }
    let mut kept = HashMap::new();
    if facts.lean {
        for (name, versions) in by_name {
            let Some(installed) = facts.installed.get(name) else {
                continue;
            };
            // Anything newer is a pending upgrade (a download kept for the
            // next install).
            for v in versions.iter().filter(|v| newer(v, installed)) {
                kept.insert(
                    (name.to_string(), v.to_string()),
                    "newer than the installed version: a pending upgrade",
                );
            }
            if facts.foreign.contains(name) {
                // The Archive has no AUR packages: the builds are the only copy.
                kept.insert(
                    (name.to_string(), installed.clone()),
                    "an AUR build; the installed one can't be downloaded again",
                );
                let previous = versions
                    .iter()
                    .filter(|v| **v != installed.as_str() && newer(installed, v))
                    .fold(None::<&str>, |best, v| match best {
                        Some(b) if newer(v, b) => Some(v),
                        None => Some(v),
                        keep => keep,
                    });
                if let Some(v) = previous {
                    kept.insert(
                        (name.to_string(), v.to_string()),
                        "an AUR build; the previous one can't be downloaded again",
                    );
                }
            } else if facts.in_repo.get(name).is_some_and(|v| v != installed) {
                kept.insert(
                    (name.to_string(), installed.clone()),
                    "no longer in the repositories (held back or not yet upgraded)",
                );
            }
        }
        for (name, version) in &facts.undo_needs {
            kept.entry((name.clone(), version.clone()))
                .or_insert("needed to undo a recent transaction");
        }
        return kept;
    }
    for (name, versions) in by_name {
        if let Some(installed) = facts.installed.get(name) {
            kept.insert(
                (name.to_string(), installed.clone()),
                "the installed version",
            );
            // Anything newer is a pending upgrade (a download kept for the
            // next install).
            for v in versions.iter().filter(|v| newer(v, installed)) {
                kept.insert(
                    (name.to_string(), v.to_string()),
                    "newer than the installed version: a pending upgrade",
                );
            }
            // The newest version that is older than the installed one.
            let previous = versions
                .iter()
                .filter(|v| **v != installed.as_str() && newer(installed, v))
                .fold(None::<&str>, |best, v| match best {
                    Some(b) if newer(v, b) => Some(v),
                    None => Some(v),
                    keep => keep,
                });
            if let Some(v) = previous {
                kept.insert(
                    (name.to_string(), v.to_string()),
                    "the previous version, for downgrade and undo",
                );
            }
        }
    }
    for (name, version) in &facts.recorded {
        kept.entry((name.clone(), version.clone()))
            .or_insert("named by a recent transaction, for undo");
    }
    kept
}

/// What `Sources::cache_apply` did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub removed: usize,
    pub freed: u64,
    pub failed: Vec<(PathBuf, String)>,
}

/// Bytes in a tree, not following symlinks.
pub fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| tree_size(&e.path())).sum()
}

fn modified_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Everything in the four caches. `pacman_dirs` are pacman's cache folders.
pub fn scan(home: &Path, pacman_dirs: &[PathBuf]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut add = |path: PathBuf, scope: Scope, package: &str, version: Option<String>| {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return;
        };
        let is_tree = meta.is_dir();
        out.push(Entry {
            bytes: if is_tree {
                tree_size(&path)
            } else {
                meta.len()
            },
            modified: modified_secs(&meta),
            is_tree,
            path,
            scope,
            package: package.to_string(),
            version,
        });
    };

    let build_root = home.join(".cache/emrac/build");
    if let Ok(dirs) = std::fs::read_dir(&build_root) {
        for dir in dirs.flatten() {
            // A split package's folder is a link to its base's; the base is
            // counted on its own.
            let Ok(kind) = dir.file_type() else { continue };
            if !kind.is_dir() {
                continue;
            }
            let package = dir.file_name().to_string_lossy().into_owned();
            let tracked = tracked_files(&dir.path());
            let Ok(items) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for item in items.flatten() {
                let name = item.file_name().to_string_lossy().into_owned();
                let path = item.path();
                if name == "src" || name == "pkg" {
                    add(path, Scope::Build, &package, None);
                } else if let Some((pkg, version)) = parse_package_file(&name) {
                    add(path, Scope::Binary, &pkg, Some(version));
                } else if name.ends_with(".pkg.tar.zst.sig")
                    || name.contains(".pkg.tar.") && name.ends_with(".sig")
                {
                    // goes with its package file
                } else if name == ".git" || tracked.contains(&name) || name == ".SRCINFO" {
                    // the checkout itself, never cache
                } else {
                    add(path, Scope::Source, &package, None);
                }
            }
        }
    }

    let archive_root = home.join(".cache/emrac/archive");
    if let Ok(dirs) = std::fs::read_dir(&archive_root) {
        for dir in dirs.flatten() {
            let Ok(files) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if let Some((pkg, version)) = parse_package_file(&name) {
                    add(file.path(), Scope::Binary, &pkg, Some(version));
                }
            }
        }
    }

    for dir in pacman_dirs {
        let Ok(files) = std::fs::read_dir(dir) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if let Some((pkg, version)) = parse_package_file(&name) {
                add(file.path(), Scope::Package, &pkg, Some(version));
            }
        }
    }
    out
}

/// Top-level names git tracks in `dir` (the checkout's own files).
fn tracked_files(dir: &Path) -> HashSet<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["ls-files", "-z"])
        .stdin(std::process::Stdio::null())
        .output();
    let Ok(out) = out else {
        return HashSet::new();
    };
    out.stdout
        .split(|b| *b == 0)
        .filter_map(|p| std::str::from_utf8(p).ok())
        .filter_map(|p| p.split('/').next())
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Removes a tree without following symlinks, making read-only directories
/// writable first (Go module caches are read-only).
pub fn remove_tree(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        for entry in std::fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Is `path` under one of `roots` (no `..` tricks)? Deletion refuses anything else.
pub fn is_under(path: &Path, roots: &[PathBuf]) -> bool {
    path.components()
        .all(|c| !matches!(c, std::path::Component::ParentDir))
        && roots.iter().any(|r| path.starts_with(r))
}

/// Whether `path` belongs to whoever owns `home` (so no `sudo` is needed to
/// remove it).
pub fn owned_like(path: &Path, home: &Path) -> bool {
    match (std::fs::symlink_metadata(path), std::fs::metadata(home)) {
        (Ok(p), Ok(h)) => p.uid() == h.uid(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(a: &str, b: &str) -> bool {
        // numeric-enough for the tests: "1-1" < "2-1" < "10-1"
        let n = |s: &str| s.split('-').next().unwrap().parse::<u64>().unwrap();
        n(a) > n(b)
    }

    fn file(scope: Scope, name: &str, version: &str, bytes: u64, modified: u64) -> Entry {
        Entry {
            path: PathBuf::from(format!("/c/{name}-{version}.pkg.tar.zst")),
            scope,
            package: name.into(),
            version: Some(version.into()),
            bytes,
            modified,
            is_tree: false,
        }
    }

    fn tree(scope: Scope, name: &str, bytes: u64, modified: u64) -> Entry {
        Entry {
            path: PathBuf::from(format!("/b/{name}/src")),
            scope,
            package: name.into(),
            version: None,
            bytes,
            modified,
            is_tree: true,
        }
    }

    fn facts() -> Facts {
        Facts {
            installed: HashMap::from([("foo".to_string(), "3-1".to_string())]),
            now: 1_000_000,
            source_age: DEFAULT_SOURCE_AGE,
            ..Facts::default()
        }
    }

    fn removed(d: &[Decision]) -> Vec<String> {
        let mut v: Vec<String> = d
            .iter()
            .filter(|d| d.remove)
            .map(|d| {
                format!(
                    "{}{}",
                    d.entry.package,
                    d.entry
                        .version
                        .as_deref()
                        .map(|v| format!("-{v}"))
                        .unwrap_or_default()
                )
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn package_file_names_give_name_and_version() {
        assert_eq!(
            parse_package_file("spotify-1:1.3.4.258-2-x86_64.pkg.tar.zst"),
            Some(("spotify".into(), "1:1.3.4.258-2".into()))
        );
        assert_eq!(
            parse_package_file("lib32-foo-bar-2.0-10-any.pkg.tar.xz"),
            Some(("lib32-foo-bar".into(), "2.0-10".into()))
        );
        assert_eq!(parse_package_file("foo-1-1-any.pkg.tar.zst.sig"), None);
        assert_eq!(parse_package_file("notes.txt"), None);
    }

    #[test]
    fn durations_sizes_and_scopes_parse_and_reject_nonsense() {
        assert_eq!(parse_duration("30d").unwrap(), 30 * 86_400);
        assert_eq!(parse_duration("2w").unwrap(), 14 * 86_400);
        assert_eq!(parse_duration("90").unwrap(), 90);
        assert!(parse_duration("soon").is_err());
        assert!(parse_duration("5y").is_err());
        assert_eq!(parse_size("10GB").unwrap(), 10 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("500m").unwrap(), 500 * 1024 * 1024);
        assert_eq!(
            parse_size("1.5G").unwrap(),
            (1.5 * 1024.0 * 1024.0 * 1024.0) as u64
        );
        assert!(parse_size("lots").is_err());
        assert!(parse_size("-1G").is_err());
        for (text, scope) in [
            ("source", Scope::Source),
            ("s", Scope::Source),
            ("b", Scope::Build),
            ("N", Scope::Binary),
            ("package", Scope::Package),
        ] {
            assert_eq!(Scope::parse(text), Some(scope), "{text}");
        }
        assert_eq!(Scope::parse("x"), None);
        assert_eq!(
            Scope::parse("bin"),
            None,
            "only the fixed words and letters"
        );
    }

    #[test]
    fn clean_keeps_the_installed_and_the_previous_version_and_removes_the_rest() {
        let entries = [
            file(Scope::Package, "foo", "1-1", 10, 1),
            file(Scope::Package, "foo", "2-1", 10, 2),
            file(Scope::Package, "foo", "3-1", 10, 3),
            file(Scope::Package, "foo", "4-1", 10, 4), // a pending upgrade: kept
            file(Scope::Package, "gone", "1-1", 10, 1), // not installed
        ];
        let d = plan(&entries, &[Scope::Package], Op::Clean, &facts(), &newer);
        assert_eq!(removed(&d), ["foo-1-1", "gone-1-1"]);
        let kept: Vec<&str> = d
            .iter()
            .filter(|d| !d.remove)
            .map(|d| d.reason.as_str())
            .collect();
        assert!(kept.iter().any(|r| r.contains("installed version")));
        assert!(kept.iter().any(|r| r.contains("previous version")));
    }

    fn lean() -> Facts {
        let mut f = facts();
        f.lean = true;
        f.installed.insert("aur".into(), "5-1".into());
        f.installed.insert("held".into(), "1-1".into());
        f.foreign.insert("aur".into());
        // the repositories offer foo 3-1 (installed), but held 2-1 (installed is older)
        f.in_repo.insert("foo".into(), "3-1".into());
        f.in_repo.insert("held".into(), "2-1".into());
        f
    }

    #[test]
    fn lean_drops_the_installed_copy_of_an_up_to_date_official_package() {
        let entries = [
            file(Scope::Package, "foo", "2-1", 10, 1), // previous, but not the last upgrade's
            file(Scope::Package, "foo", "3-1", 10, 2), // installed, current in the repos
        ];
        let d = plan(&entries, &[Scope::Package], Op::Clean, &lean(), &newer);
        assert_eq!(removed(&d), ["foo-2-1", "foo-3-1"]);
    }

    #[test]
    fn lean_keeps_what_undoing_the_last_transaction_needs() {
        let mut f = lean();
        f.undo_needs.insert(("foo".into(), "2-1".into()));
        f.undo_needs.insert(("gone".into(), "1-1".into())); // a removed package
        let entries = [
            file(Scope::Package, "foo", "1-1", 10, 1),
            file(Scope::Package, "foo", "2-1", 10, 2),
            file(Scope::Binary, "gone", "1-1", 10, 1),
        ];
        let d = plan(
            &entries,
            &[Scope::Package, Scope::Binary],
            Op::Clean,
            &f,
            &newer,
        );
        assert_eq!(removed(&d), ["foo-1-1"]);
        assert!(d
            .iter()
            .any(|d| d.reason.contains("undo a recent transaction")));
    }

    #[test]
    fn lean_keeps_aur_builds_held_back_packages_and_pending_upgrades() {
        let entries = [
            file(Scope::Binary, "aur", "4-1", 10, 1), // the previous build
            file(Scope::Binary, "aur", "5-1", 10, 2), // the installed build
            file(Scope::Binary, "aur", "1-1", 10, 0), // an old build
            file(Scope::Package, "held", "1-1", 10, 1), // installed, no longer in the repos
            file(Scope::Package, "foo", "9-1", 10, 1), // newer than installed: pending
        ];
        let d = plan(
            &entries,
            &[Scope::Package, Scope::Binary],
            Op::Clean,
            &lean(),
            &newer,
        );
        assert_eq!(removed(&d), ["aur-1-1"]);
        let reasons: Vec<&str> = d.iter().map(|d| d.reason.as_str()).collect();
        assert!(reasons
            .iter()
            .any(|r| r.contains("no longer in the repositories")));
        assert!(reasons.iter().any(|r| r.contains("pending upgrade")));
    }

    #[test]
    fn the_standard_policy_is_unchanged_by_the_lean_facts() {
        let entries = [
            file(Scope::Package, "foo", "2-1", 10, 1),
            file(Scope::Package, "foo", "3-1", 10, 2),
        ];
        let d = plan(&entries, &[Scope::Package], Op::Clean, &facts(), &newer);
        assert!(removed(&d).is_empty(), "installed and previous are kept");
    }

    #[test]
    fn a_version_a_recent_transaction_names_is_kept() {
        let mut f = facts();
        f.recorded.insert(("gone".into(), "1-1".into()));
        let entries = [file(Scope::Binary, "gone", "1-1", 10, 1)];
        let d = plan(&entries, &[Scope::Binary], Op::Clean, &f, &newer);
        assert!(removed(&d).is_empty());
        assert!(d[0].reason.contains("recent transaction"));
    }

    #[test]
    fn purge_ignores_the_protections_and_trim_stops_at_the_limit() {
        let entries = [
            file(Scope::Package, "foo", "3-1", 40, 3), // installed: kept by clean
            file(Scope::Package, "foo", "1-1", 40, 1),
            file(Scope::Package, "bar", "1-1", 40, 2),
            file(Scope::Package, "baz", "1-1", 40, 4),
        ];
        let d = plan(&entries, &[Scope::Package], Op::Purge, &facts(), &newer);
        assert_eq!(removed(&d).len(), 4);
        // 160 bytes, limit 100: the oldest eligible go until it fits. foo-1-1
        // is the previous version of an installed package, so it is not eligible.
        let d = plan(&entries, &[Scope::Package], Op::Trim(100), &facts(), &newer);
        assert_eq!(
            removed(&d),
            ["bar-1-1", "baz-1-1"],
            "oldest first, then stop"
        );
        let d = plan(
            &entries,
            &[Scope::Package],
            Op::Trim(1000),
            &facts(),
            &newer,
        );
        assert!(removed(&d).is_empty(), "already under the limit");
        // The installed file is never trimmed: 40 bytes can't be reached.
        let d = plan(&entries, &[Scope::Package], Op::Trim(0), &facts(), &newer);
        assert!(!removed(&d).contains(&"foo-3-1".to_string()));
    }

    #[test]
    fn older_than_spares_recent_entries_and_sources_wait_thirty_days() {
        let mut f = facts();
        f.older_than = Some(500_000);
        let entries = [
            tree(Scope::Build, "old", 5, 100_000),
            tree(Scope::Build, "new", 5, 900_000),
        ];
        let d = plan(&entries, &[Scope::Build], Op::Clean, &f, &newer);
        assert_eq!(removed(&d), ["old"]);
        let src = |modified| Entry {
            is_tree: false,
            ..tree(Scope::Source, "s", 5, modified)
        };
        let f2 = Facts {
            now: 40 * 86_400,
            source_age: DEFAULT_SOURCE_AGE,
            ..Facts::default()
        };
        let d = plan(
            &[src(1), src(35 * 86_400)],
            &[Scope::Source],
            Op::Clean,
            &f2,
            &newer,
        );
        assert_eq!(d.iter().filter(|d| d.remove).count(), 1);
    }

    #[test]
    fn only_paths_under_the_roots_may_be_deleted() {
        let roots = [PathBuf::from("/home/u/.cache/emrac")];
        assert!(is_under(
            Path::new("/home/u/.cache/emrac/build/x/src"),
            &roots
        ));
        assert!(!is_under(
            Path::new("/home/u/.cache/emrac/../.ssh/id"),
            &roots
        ));
        assert!(!is_under(Path::new("/etc/passwd"), &roots));
    }

    #[test]
    fn a_tree_with_read_only_directories_and_an_outward_symlink_is_removed_safely() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("emrac-cache-test-{}", std::process::id()));
        let _ = remove_tree(&base);
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious"), "keep").unwrap();
        let tree_root = base.join("src");
        let ro = tree_root.join("mod/cache");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::write(ro.join("f"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, tree_root.join("link")).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        remove_tree(&tree_root).unwrap();
        assert!(!tree_root.exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("precious")).unwrap(),
            "keep"
        );
        remove_tree(&base).unwrap();
    }
}

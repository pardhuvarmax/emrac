//! Named restore points for the installed package set, and the diff that
//! `emrac restore` is planned from. Storage takes explicit paths so tests
//! never touch `$HOME`; the diff is pure.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::history::{format_utc, now};
use crate::plan::PlanAction;
use crate::rollback::Download;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotPackage {
    pub name: String,
    pub version: String,
    /// Installed on purpose rather than as a dependency.
    pub explicit: bool,
    /// Not in any sync database (almost always AUR-built).
    pub foreign: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    /// Unix seconds, UTC.
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub packages: Vec<SnapshotPackage>,
}

impl Snapshot {
    /// `YYYY-MM-DD HH:MM` in UTC.
    pub fn timestamp_utc(&self) -> String {
        format_utc(self.timestamp)
    }
}

/// One package `restore` has to change to match the snapshot.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestoreItem {
    pub name: String,
    /// The version installed now; `None` when the package is missing.
    pub from: Option<String>,
    /// The version the snapshot recorded.
    pub to: String,
    /// `Install`, `Upgrade` or `Downgrade`.
    pub action: PlanAction,
    pub foreign: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct RestoreDiff {
    pub items: Vec<RestoreItem>,
    /// Installed now but not in the snapshot: `(name, version)`. Only
    /// removed by `restore --prune`.
    pub extras: Vec<(String, String)>,
}

impl RestoreDiff {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.extras.is_empty()
    }
}

/// Where a restored package comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RestoreSource {
    /// A package file already on this machine.
    File,
    /// Downloaded from the Arch Linux Archive after confirmation.
    Archive,
    /// A missing package whose snapshot version is still in the repositories.
    Repository,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreEntry {
    #[serde(flatten)]
    pub item: RestoreItem,
    pub source: RestoreSource,
}

/// A planned restore: nothing has been changed or downloaded yet.
#[derive(Debug, Clone, Serialize)]
pub struct RestorePlan {
    pub snapshot: String,
    pub entries: Vec<RestoreEntry>,
    /// Installed but not in the snapshot.
    pub extras: Vec<(String, String)>,
    /// `--prune`: the extras are removed.
    pub prune: bool,
    /// Package files for `pacman -U` (sources `File` and `Archive`).
    pub files: Vec<PathBuf>,
    /// The subset of `files` still to be downloaded.
    pub downloads: Vec<Download>,
    /// Names for `pacman -S` (source `Repository`).
    pub repository: Vec<String>,
}

impl RestorePlan {
    /// Nothing to do: every change is empty and nothing is to be pruned.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && !(self.prune && !self.extras.is_empty())
    }
}

/// What it takes to make `installed` (`(name, version)`) match `snapshot`.
/// `is_newer(current, candidate)` compares versions the pacman way.
pub fn diff(
    snapshot: &Snapshot,
    installed: &[(String, String)],
    is_newer: &dyn Fn(&str, &str) -> bool,
) -> RestoreDiff {
    let mut items = Vec::new();
    for pkg in &snapshot.packages {
        let current = installed
            .iter()
            .find(|(name, _)| *name == pkg.name)
            .map(|(_, version)| version.as_str());
        let action = match current {
            None => PlanAction::Install,
            Some(current) if current == pkg.version => continue,
            Some(current) if is_newer(current, &pkg.version) => PlanAction::Upgrade,
            Some(_) => PlanAction::Downgrade,
        };
        items.push(RestoreItem {
            name: pkg.name.clone(),
            from: current.map(str::to_string),
            to: pkg.version.clone(),
            action,
            foreign: pkg.foreign,
        });
    }

    let extras = installed
        .iter()
        .filter(|(name, _)| !snapshot.packages.iter().any(|p| p.name == *name))
        .cloned()
        .collect();

    RestoreDiff { items, extras }
}

/// Names are file names: keep them boring.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidSnapshotName(name.to_string()))
    }
}

/// Snapshots live one JSON file each in `~/.local/share/emrac/snapshots`.
pub struct SnapshotStore;

impl SnapshotStore {
    pub fn dir() -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoDataDir)?;
        Ok(PathBuf::from(home).join(".local/share/emrac/snapshots"))
    }

    pub fn create(
        name: Option<&str>,
        note: Option<&str>,
        packages: Vec<SnapshotPackage>,
    ) -> Result<Snapshot> {
        Self::create_in(&Self::dir()?, name, note, packages, now())
    }

    pub fn load(name: &str) -> Result<Snapshot> {
        Self::load_from(&Self::dir()?, name)
    }

    /// Newest first.
    pub fn list() -> Result<Vec<Snapshot>> {
        Self::list_from(&Self::dir()?)
    }

    pub fn delete(name: &str) -> Result<()> {
        Self::delete_in(&Self::dir()?, name)
    }

    fn create_in(
        dir: &Path,
        name: Option<&str>,
        note: Option<&str>,
        mut packages: Vec<SnapshotPackage>,
        timestamp: u64,
    ) -> Result<Snapshot> {
        let name = match name {
            Some(name) => {
                validate_name(name)?;
                name.to_string()
            }
            None => {
                let next = Self::list_from(dir)?
                    .iter()
                    .filter_map(|s| s.name.strip_prefix("snap-")?.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                format!("snap-{next}")
            }
        };
        let path = file_path(dir, &name);
        if path.exists() {
            return Err(Error::SnapshotExists(name));
        }

        packages.sort_by(|a, b| a.name.cmp(&b.name));
        let snapshot = Snapshot {
            name,
            timestamp,
            note: note.map(str::to_string),
            packages,
        };

        let write_err = |source| Error::SnapshotWrite {
            path: path.display().to_string(),
            source,
        };
        fs::create_dir_all(dir).map_err(write_err)?;
        let json = serde_json::to_string_pretty(&snapshot).expect("Snapshot is serializable");
        fs::write(&path, json).map_err(write_err)?;
        Ok(snapshot)
    }

    fn load_from(dir: &Path, name: &str) -> Result<Snapshot> {
        validate_name(name)?;
        let path = file_path(dir, name);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(Error::UnknownSnapshot(name.to_string()));
            }
            Err(source) => {
                return Err(Error::Io {
                    context: format!("couldn't read the snapshot at {}", path.display()),
                    source,
                });
            }
        };
        serde_json::from_str(&raw).map_err(|err| Error::SnapshotCorrupt {
            name: name.to_string(),
            why: err.to_string(),
        })
    }

    fn list_from(dir: &Path) -> Result<Vec<Snapshot>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::Io {
                    context: format!("couldn't read your snapshots in {}", dir.display()),
                    source,
                });
            }
        };
        // A snapshot that doesn't parse is skipped, as in the history.
        let mut snapshots: Vec<Snapshot> = entries
            .flatten()
            .filter_map(|e| fs::read_to_string(e.path()).ok())
            .filter_map(|raw| serde_json::from_str(&raw).ok())
            .collect();
        snapshots.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| b.name.cmp(&a.name))
        });
        Ok(snapshots)
    }

    fn delete_in(dir: &Path, name: &str) -> Result<()> {
        validate_name(name)?;
        let path = file_path(dir, name);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                Err(Error::UnknownSnapshot(name.to_string()))
            }
            Err(source) => Err(Error::SnapshotWrite {
                path: path.display().to_string(),
                source,
            }),
        }
    }
}

fn file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str) -> SnapshotPackage {
        SnapshotPackage {
            name: name.into(),
            version: version.into(),
            explicit: true,
            foreign: false,
        }
    }

    fn snap(packages: Vec<SnapshotPackage>) -> Snapshot {
        Snapshot {
            name: "s".into(),
            timestamp: 0,
            note: None,
            packages,
        }
    }

    fn inst(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    // Numeric comparison is enough for the tests.
    fn newer(current: &str, candidate: &str) -> bool {
        candidate.parse::<u32>().unwrap() > current.parse::<u32>().unwrap()
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("emrac-snap-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn diff_of_identical_sets_is_empty() {
        let s = snap(vec![pkg("a", "1"), pkg("b", "2")]);
        assert!(diff(&s, &inst(&[("a", "1"), ("b", "2")]), &newer).is_empty());
    }

    #[test]
    fn diff_classifies_install_upgrade_downgrade() {
        let s = snap(vec![pkg("gone", "1"), pkg("old", "1"), pkg("new", "5")]);
        let d = diff(&s, &inst(&[("old", "3"), ("new", "2")]), &newer);
        let actions: Vec<_> = d
            .items
            .iter()
            .map(|i| (i.name.as_str(), i.action))
            .collect();
        assert_eq!(
            actions,
            [
                ("gone", PlanAction::Install),
                ("old", PlanAction::Downgrade),
                ("new", PlanAction::Upgrade),
            ]
        );
        assert_eq!(d.items[0].from, None);
        assert_eq!(d.items[1].from.as_deref(), Some("3"));
        assert_eq!(d.items[1].to, "1");
    }

    #[test]
    fn diff_lists_packages_not_in_the_snapshot_as_extras() {
        let s = snap(vec![pkg("a", "1")]);
        let d = diff(&s, &inst(&[("a", "1"), ("extra", "9")]), &newer);
        assert!(d.items.is_empty());
        assert_eq!(d.extras, [("extra".to_string(), "9".to_string())]);
        assert!(!d.is_empty());
    }

    #[test]
    fn names_are_validated() {
        for ok in ["snap-1", "before.upgrade", "a_b-C9"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".hidden", "a/b", "../x", "a b", "é", &"x".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn create_load_list_delete_round_trip() {
        let dir = scratch("roundtrip");
        let a = SnapshotStore::create_in(
            &dir,
            None,
            Some("first"),
            vec![pkg("z", "1"), pkg("a", "2")],
            100,
        )
        .unwrap();
        assert_eq!(a.name, "snap-1");
        assert_eq!(a.packages[0].name, "a", "sorted by name");
        let b = SnapshotStore::create_in(&dir, Some("named"), None, vec![], 200).unwrap();
        let c = SnapshotStore::create_in(&dir, None, None, vec![], 300).unwrap();
        assert_eq!(c.name, "snap-2");

        let loaded = SnapshotStore::load_from(&dir, "snap-1").unwrap();
        assert_eq!(loaded.note.as_deref(), Some("first"));
        assert_eq!(loaded.packages, a.packages);

        let names: Vec<_> = SnapshotStore::list_from(&dir)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, [c.name.as_str(), b.name.as_str(), "snap-1"]);

        SnapshotStore::delete_in(&dir, "named").unwrap();
        assert!(matches!(
            SnapshotStore::load_from(&dir, "named"),
            Err(Error::UnknownSnapshot(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_names_are_refused_and_unknown_ones_reported() {
        let dir = scratch("exists");
        SnapshotStore::create_in(&dir, Some("x"), None, vec![], 1).unwrap();
        assert!(matches!(
            SnapshotStore::create_in(&dir, Some("x"), None, vec![], 2),
            Err(Error::SnapshotExists(_))
        ));
        assert!(matches!(
            SnapshotStore::delete_in(&dir, "nope"),
            Err(Error::UnknownSnapshot(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_lists_empty_and_corrupt_files_are_handled() {
        let dir = scratch("corrupt");
        assert!(SnapshotStore::list_from(&dir).unwrap().is_empty());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bad.json"), "{nope").unwrap();
        assert!(SnapshotStore::list_from(&dir).unwrap().is_empty());
        assert!(matches!(
            SnapshotStore::load_from(&dir, "bad"),
            Err(Error::SnapshotCorrupt { .. })
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}

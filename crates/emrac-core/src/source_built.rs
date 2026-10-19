//! Which packages you built from source here with `emrac install --source`
//! (u23), so that an upgrade that would replace one with the repository's binary
//! says so before it does. A source build has the repository's version, so
//! nothing replaces it until the repository moves to a newer one.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The version that was built.
    pub version: String,
    pub at: u64,
}

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/source-built.json"))
}

pub fn read_from(path: &Path) -> Vec<Entry> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn read() -> Vec<Entry> {
    path().map(|p| read_from(&p)).unwrap_or_default()
}

fn write(path: &Path, list: &[Entry]) -> Result<()> {
    let io = |context: String, source| Error::Io { context, source };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(format!("couldn't create {}", dir.display()), e))?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f =
        fs::File::create(&tmp).map_err(|e| io(format!("couldn't write {}", tmp.display()), e))?;
    f.write_all(&serde_json::to_vec_pretty(list).unwrap_or_default())
        .and_then(|()| f.sync_all())
        .map_err(|e| io(format!("couldn't write {}", tmp.display()), e))?;
    fs::rename(&tmp, path).map_err(|e| io(format!("couldn't replace {}", path.display()), e))
}

/// Notes that `name` at `version` was built here.
pub fn record_to(path: &Path, name: &str, version: &str, now: u64) -> Result<()> {
    let mut list = read_from(path);
    list.retain(|e| e.name != name);
    list.push(Entry {
        name: name.to_string(),
        version: version.to_string(),
        at: now,
    });
    list.sort_by(|a, b| a.name.cmp(&b.name));
    write(path, &list)
}

/// Forgets these packages (they were replaced, or removed).
pub fn forget_from(path: &Path, names: &[String]) -> Result<()> {
    let mut list = read_from(path);
    let before = list.len();
    list.retain(|e| !names.contains(&e.name));
    if list.len() != before {
        write(path, &list)?;
    }
    Ok(())
}

/// A change in an upgrade plan: the package, what is installed, what it becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub from: Option<String>,
    pub to: String,
}

/// The source builds an upgrade would replace: the package is in the plan and
/// what is installed is still the version that was built here.
pub fn replaced_by(entries: &[Entry], changes: &[Change]) -> Vec<Change> {
    changes
        .iter()
        .filter(|c| {
            entries
                .iter()
                .any(|e| e.name == c.name && Some(&e.version) == c.from.as_ref())
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(name: &str, from: Option<&str>, to: &str) -> Change {
        Change {
            name: name.into(),
            from: from.map(str::to_string),
            to: to.into(),
        }
    }

    #[test]
    fn what_was_built_here_is_kept_replaced_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("emrac-sb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sb.json");
        assert!(read_from(&file).is_empty());
        record_to(&file, "sl", "5.05-6", 100).unwrap();
        record_to(&file, "figlet", "2.2.5-6", 101).unwrap();
        record_to(&file, "sl", "5.05-7", 200).unwrap();
        let list = read_from(&file);
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["figlet", "sl"]);
        assert_eq!(list[1].version, "5.05-7");
        forget_from(&file, &["sl".to_string(), "absent".to_string()]).unwrap();
        assert_eq!(read_from(&file).len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_upgrade_replaces_a_source_build_only_when_it_is_what_is_installed() {
        let entries = vec![
            Entry { name: "sl".into(), version: "5.05-6".into(), at: 1 },
            Entry { name: "gone".into(), version: "1-1".into(), at: 1 },
        ];
        let changes = vec![
            change("sl", Some("5.05-6"), "5.05-7"),
            change("vim", Some("9.1-1"), "9.2-1"),
            change("gone", Some("2-1"), "2-2"),
            change("newpkg", None, "1-1"),
        ];
        let hit = replaced_by(&entries, &changes);
        assert_eq!(hit, vec![change("sl", Some("5.05-6"), "5.05-7")], "a different installed version is not our build");
        assert!(replaced_by(&[], &changes).is_empty());
    }
}

//! The build policy of the packages you asked to be built from source
//! (`emrac install --source`, u23): a persistent record, separate from pins, that
//! says "this one follows the repository's version but is built here from Arch's
//! packaging source". The installed package looks exactly like the repository's
//! binary, so emrac keeps its own record of the intent: the version built, the
//! packaging revision it came from, when, and whether a rebuild is waiting. The
//! upgrade plan reads it and plans the rebuild; nothing replaces a source build
//! with the binary without saying so.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// One package whose policy is "build from source".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The version that was built here last.
    pub version: String,
    /// When (Unix seconds).
    pub at: u64,
    /// The revision of Arch's packaging repository the build came from.
    #[serde(default)]
    pub recipe: Option<String>,
    /// A rebuild is waiting: the repository's binary was installed (by an
    /// upgrade, or because a rebuild failed) and the policy has not been carried out.
    #[serde(default)]
    pub pending: bool,
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

/// Notes that `name` was built here at `version` (from packaging revision `recipe`).
pub fn record_to(
    path: &Path,
    name: &str,
    version: &str,
    recipe: Option<&str>,
    now: u64,
) -> Result<()> {
    let mut list = read_from(path);
    list.retain(|e| e.name != name);
    list.push(Entry {
        name: name.to_string(),
        version: version.to_string(),
        at: now,
        recipe: recipe.map(str::to_string),
        pending: false,
    });
    list.sort_by(|a, b| a.name.cmp(&b.name));
    write(path, &list)
}

/// A rebuild is waiting for these packages (the policy stays).
pub fn mark_pending(path: &Path, names: &[String]) -> Result<()> {
    let mut list = read_from(path);
    let mut changed = false;
    for e in list
        .iter_mut()
        .filter(|e| names.contains(&e.name) && !e.pending)
    {
        e.pending = true;
        changed = true;
    }
    if changed {
        write(path, &list)?;
    }
    Ok(())
}

/// Ends the policy for these packages: they follow the repository's binary.
pub fn forget_from(path: &Path, names: &[String]) -> Result<bool> {
    let mut list = read_from(path);
    let before = list.len();
    list.retain(|e| !names.contains(&e.name));
    let changed = list.len() != before;
    if changed {
        write(path, &list)?;
    }
    Ok(changed)
}

/// What is known about whether the package that is installed is the one built here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltState {
    /// The installed version is the one that was built here.
    BuiltHere,
    /// A rebuild is waiting (the binary is installed).
    Pending,
    /// Another version is installed than the one recorded: nothing says it was
    /// built here, so the policy is kept but the build is not claimed.
    Unknown { installed: String },
    /// The package is not installed.
    NotInstalled,
}

pub fn state_of(entry: &Entry, installed: Option<&str>) -> BuiltState {
    match installed {
        None => BuiltState::NotInstalled,
        Some(_) if entry.pending => BuiltState::Pending,
        Some(v) if v == entry.version => BuiltState::BuiltHere,
        Some(v) => BuiltState::Unknown {
            installed: v.to_string(),
        },
    }
}

/// The packages of an upgrade that have a source-build policy.
pub fn followed(entries: &[Entry], names: &[&str]) -> Vec<Entry> {
    entries
        .iter()
        .filter(|e| names.contains(&e.name.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, version: &str, pending: bool) -> Entry {
        Entry {
            name: name.into(),
            version: version.into(),
            at: 1,
            recipe: None,
            pending,
        }
    }

    #[test]
    fn the_policy_is_kept_marked_pending_and_ended() {
        let dir = std::env::temp_dir().join(format!("emrac-sb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sb.json");
        assert!(read_from(&file).is_empty());
        record_to(&file, "sl", "5.05-6", Some("abc123"), 100).unwrap();
        record_to(&file, "figlet", "2.2.5-6", None, 101).unwrap();
        let list = read_from(&file);
        assert_eq!(
            list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["figlet", "sl"]
        );
        assert_eq!(list[1].recipe.as_deref(), Some("abc123"));
        mark_pending(&file, &["sl".to_string(), "absent".to_string()]).unwrap();
        assert!(
            read_from(&file)
                .iter()
                .find(|e| e.name == "sl")
                .unwrap()
                .pending
        );
        // a rebuild clears it
        record_to(&file, "sl", "5.05-7", Some("def456"), 200).unwrap();
        let sl = read_from(&file)
            .into_iter()
            .find(|e| e.name == "sl")
            .unwrap();
        assert!(!sl.pending && sl.version == "5.05-7");
        assert!(forget_from(&file, &["sl".to_string()]).unwrap());
        assert!(!forget_from(&file, &["sl".to_string()]).unwrap());
        assert_eq!(read_from(&file).len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_old_record_without_the_new_fields_still_reads() {
        let old = r#"[{"name":"sl","version":"5.05-6","at":10}]"#;
        let list: Vec<Entry> = serde_json::from_str(old).unwrap();
        assert_eq!(list[0].recipe, None);
        assert!(!list[0].pending);
    }

    #[test]
    fn it_says_what_is_known_about_the_installed_package() {
        let e = entry("sl", "5.05-6", false);
        assert_eq!(state_of(&e, Some("5.05-6")), BuiltState::BuiltHere);
        assert_eq!(
            state_of(&e, Some("5.05-7")),
            BuiltState::Unknown {
                installed: "5.05-7".into()
            },
            "another version is installed: the build is not claimed"
        );
        assert_eq!(
            state_of(&entry("sl", "5.05-6", true), Some("5.05-7")),
            BuiltState::Pending
        );
        assert_eq!(state_of(&e, None), BuiltState::NotInstalled);
    }

    #[test]
    fn an_upgrade_follows_the_policy_of_the_packages_it_contains() {
        let entries = vec![entry("sl", "5.05-6", false), entry("gone", "1-1", false)];
        let hit = followed(&entries, &["sl", "vim", "newpkg"]);
        assert_eq!(
            hit.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["sl"]
        );
        assert!(followed(&[], &["sl"]).is_empty());
    }
}

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
    /// What the package database said about the package right after it was
    /// installed: the build date and the packager. Supporting evidence of origin,
    /// not proof (both can be reproduced); a repository file of the same version
    /// has other values.
    #[serde(default)]
    pub build_date: Option<u64>,
    #[serde(default)]
    pub packager: Option<String>,
}

/// An installed package as pacman's database records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub build_date: u64,
    pub packager: String,
}

/// Reads `name` from a pacman `local` database directory (`desc` files), which
/// is current whatever process installed the package.
pub fn installed_in(local: &Path, name: &str) -> Option<Installed> {
    for dir in fs::read_dir(local).ok()?.flatten() {
        let file = dir.file_name().to_string_lossy().into_owned();
        if !file.starts_with(&format!("{name}-")) {
            continue;
        }
        let Ok(desc) = fs::read_to_string(dir.path().join("desc")) else {
            continue;
        };
        let field = |key: &str| -> Option<String> {
            let mut lines = desc.lines();
            lines.find(|l| *l == key)?;
            lines.next().map(str::to_string)
        };
        if field("%NAME%").as_deref() != Some(name) {
            continue;
        }
        return Some(Installed {
            version: field("%VERSION%")?,
            build_date: field("%BUILDDATE%")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            packager: field("%PACKAGER%").unwrap_or_default(),
        });
    }
    None
}

pub fn installed(name: &str) -> Option<Installed> {
    installed_in(Path::new("/var/lib/pacman/local"), name)
}

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/source-built.json"))
}

/// The policies recorded. No file is no policy; a file that cannot be read or
/// parsed is an error, never an empty list: an upgrade must not install binaries
/// over source builds, nor a write replace the other entries, because the record
/// is damaged. The damaged file is left as it is.
pub fn read_from(path: &Path) -> Result<Vec<Entry>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let sealed = sealed_names(path);
            if sealed.is_empty() {
                return Ok(Vec::new());
            }
            return Err(Error::SourcePolicy(format!(
                "the record of what you build from source ({}) is missing, but it held policies for {} (kept in {}). Intent is unknown, so emrac will not upgrade or write over it. Put the file back from a backup, or delete {} to start again (`emrac from-source add <pkg>` declares them again)",
                path.display(),
                sealed.join(", "),
                seal_path(path).display(),
                seal_path(path).display()
            )));
        }
        Err(e) => return Err(damaged(path, &e.to_string())),
    };
    serde_json::from_slice(&bytes).map_err(|e| damaged(path, &e.to_string()))
}

fn damaged(path: &Path, why: &str) -> Error {
    Error::SourcePolicy(format!(
        "the record of what you build from source ({}) can't be read: {why}. Intent is unknown, so emrac will not upgrade or write over it. Look at the file, and fix it or move it aside (the policies are lost then: `emrac from-source add <pkg>` declares them again)",
        path.display()
    ))
}

pub fn read() -> Result<Vec<Entry>> {
    read_from(&path()?)
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
    fs::rename(&tmp, path).map_err(|e| io(format!("couldn't replace {}", path.display()), e))?;
    write_seal(path, list)
}

/// Beside the record, the names it holds. A record that vanishes while its seal
/// still lists policies was deleted or lost: that is unknown intent, not "none".
fn seal_path(path: &Path) -> PathBuf {
    path.with_extension("seal")
}

fn write_seal(path: &Path, list: &[Entry]) -> Result<()> {
    let seal = seal_path(path);
    if list.is_empty() {
        let _ = fs::remove_file(&seal);
        return Ok(());
    }
    let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
    let tmp = seal.with_extension("seal.tmp");
    fs::write(&tmp, serde_json::to_vec(&names).unwrap_or_default())
        .and_then(|()| fs::rename(&tmp, &seal))
        .map_err(|source| Error::Io {
            context: format!("couldn't write {}", seal.display()),
            source,
        })
}

fn sealed_names(path: &Path) -> Vec<String> {
    fs::read(seal_path(path))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Notes that `name` was built here at `version` (from packaging revision `recipe`).
pub fn record_to(
    path: &Path,
    name: &str,
    version: &str,
    recipe: Option<&str>,
    evidence: Option<&Installed>,
    now: u64,
) -> Result<()> {
    let mut list = read_from(path)?;
    list.retain(|e| e.name != name);
    list.push(Entry {
        name: name.to_string(),
        version: version.to_string(),
        at: now,
        recipe: recipe.map(str::to_string),
        pending: false,
        build_date: evidence.map(|i| i.build_date),
        packager: evidence.map(|i| i.packager.clone()),
    });
    list.sort_by(|a, b| a.name.cmp(&b.name));
    write(path, &list)
}

/// A rebuild is waiting for these packages (the policy stays).
pub fn mark_pending(path: &Path, names: &[String]) -> Result<()> {
    let mut list = read_from(path)?;
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
    let mut list = read_from(path)?;
    let before = list.len();
    list.retain(|e| !names.contains(&e.name));
    let changed = list.len() != before;
    if changed {
        write(path, &list)?;
    }
    Ok(changed)
}

/// What is known about the installed package against the recorded policy. The
/// record is intent; the package database says what is installed; only evidence
/// that agrees is called verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltState {
    /// The version, build date and packager are those recorded after the build.
    SourceVerified,
    /// The version is the recorded one but the build date or packager differ: a
    /// repository file (or another build) replaced it.
    RepositoryDetected,
    /// The version matches but the record has no build evidence (it predates it):
    /// nothing establishes the origin.
    OriginUnverified,
    /// A rebuild is waiting (the binary is installed).
    Pending,
    /// Another version is installed than the one recorded: the policy conflicts
    /// with the installed state, and the build is not claimed.
    Mismatch { installed: String },
    /// The package is not installed.
    NotInstalled,
}

impl BuiltState {
    /// The policy is not being honoured, or cannot be shown to be.
    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            BuiltState::RepositoryDetected | BuiltState::Pending | BuiltState::Mismatch { .. }
        )
    }
}

pub fn state_of(entry: &Entry, installed: Option<&Installed>) -> BuiltState {
    let Some(i) = installed else {
        return BuiltState::NotInstalled;
    };
    if entry.pending {
        return BuiltState::Pending;
    }
    if i.version != entry.version {
        return BuiltState::Mismatch {
            installed: i.version.clone(),
        };
    }
    match (entry.build_date, entry.packager.as_deref()) {
        (Some(d), Some(p)) if d == i.build_date && p == i.packager => BuiltState::SourceVerified,
        (Some(_), _) => BuiltState::RepositoryDetected,
        _ => BuiltState::OriginUnverified,
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
            build_date: None,
            packager: None,
        }
    }

    #[test]
    fn the_policy_is_kept_marked_pending_and_ended() {
        let dir = std::env::temp_dir().join(format!("emrac-sb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sb.json");
        assert!(read_from(&file).unwrap().is_empty());
        record_to(&file, "sl", "5.05-6", Some("abc123"), None, 100).unwrap();
        record_to(&file, "figlet", "2.2.5-6", None, None, 101).unwrap();
        let list = read_from(&file).unwrap();
        assert_eq!(
            list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["figlet", "sl"]
        );
        assert_eq!(list[1].recipe.as_deref(), Some("abc123"));
        mark_pending(&file, &["sl".to_string(), "absent".to_string()]).unwrap();
        assert!(
            read_from(&file)
                .unwrap()
                .iter()
                .find(|e| e.name == "sl")
                .unwrap()
                .pending
        );
        // a rebuild clears it
        record_to(&file, "sl", "5.05-7", Some("def456"), None, 200).unwrap();
        let sl = read_from(&file)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "sl")
            .unwrap();
        assert!(!sl.pending && sl.version == "5.05-7");
        assert!(forget_from(&file, &["sl".to_string()]).unwrap());
        assert!(!forget_from(&file, &["sl".to_string()]).unwrap());
        assert_eq!(read_from(&file).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_record_is_an_error_and_is_never_written_over() {
        let dir = std::env::temp_dir().join(format!("emrac-sbd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sb.json");
        fs::write(&file, b"[{\"name\": \"sl\", ").unwrap();
        assert!(matches!(read_from(&file), Err(Error::SourcePolicy(_))));
        assert!(record_to(&file, "figlet", "1-1", None, None, 1).is_err());
        assert!(mark_pending(&file, &["sl".to_string()]).is_err());
        assert!(forget_from(&file, &["sl".to_string()]).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"[{\"name\": \"sl\", ");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_deleted_record_is_unknown_intent_not_no_policy() {
        let dir = std::env::temp_dir().join(format!("emrac-sbs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sb.json");
        record_to(&file, "sl", "5.05-6", None, None, 1).unwrap();
        fs::remove_file(&file).unwrap();
        let err = read_from(&file).unwrap_err().to_string();
        assert!(err.contains("missing") && err.contains("sl"), "{err}");
        assert!(record_to(&file, "figlet", "1-1", None, None, 2).is_err());
        // ending the last policy removes the seal: an empty record is fine
        fs::write(&file, b"[]").unwrap();
        assert!(read_from(&file).unwrap().is_empty());
        record_to(&file, "sl", "5.05-6", None, None, 3).unwrap();
        assert!(forget_from(&file, &["sl".to_string()]).unwrap());
        fs::remove_file(&file).unwrap();
        assert!(read_from(&file).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_old_record_without_the_new_fields_still_reads() {
        let old = r#"[{"name":"sl","version":"5.05-6","at":10}]"#;
        let list: Vec<Entry> = serde_json::from_str(old).unwrap();
        assert_eq!(list[0].recipe, None);
        assert!(!list[0].pending);
    }

    fn installed(version: &str, date: u64, packager: &str) -> Installed {
        Installed {
            version: version.into(),
            build_date: date,
            packager: packager.into(),
        }
    }

    #[test]
    fn it_says_what_is_known_about_the_installed_package() {
        let mut e = entry("sl", "5.05-6", false);
        e.build_date = Some(1000);
        e.packager = Some("Unknown Packager".into());
        let mine = installed("5.05-6", 1000, "Unknown Packager");
        assert_eq!(state_of(&e, Some(&mine)), BuiltState::SourceVerified);
        // `pacman -S sl` / `emrac reinstall sl`: the same version, another file
        let repo = installed("5.05-6", 999, "Arch Maintainer <m@archlinux.org>");
        assert_eq!(state_of(&e, Some(&repo)), BuiltState::RepositoryDetected);
        assert_eq!(
            state_of(&e, Some(&installed("5.05-7", 1000, "Unknown Packager"))),
            BuiltState::Mismatch {
                installed: "5.05-7".into()
            },
        );
        // a record without evidence claims nothing
        assert_eq!(
            state_of(&entry("sl", "5.05-6", false), Some(&mine)),
            BuiltState::OriginUnverified
        );
        let mut pending = e.clone();
        pending.pending = true;
        assert_eq!(state_of(&pending, Some(&repo)), BuiltState::Pending);
        assert_eq!(state_of(&e, None), BuiltState::NotInstalled);
        assert!(BuiltState::RepositoryDetected.needs_attention());
        assert!(!BuiltState::SourceVerified.needs_attention());
        assert!(!BuiltState::OriginUnverified.needs_attention());
    }

    #[test]
    fn the_package_database_is_read_for_the_build_date_and_packager() {
        let dir = std::env::temp_dir().join(format!("emrac-sbl-{}", std::process::id()));
        let pkg = dir.join("sl-5.05-6");
        fs::create_dir_all(&pkg).unwrap();
        fs::create_dir_all(dir.join("sl-extras-1-1")).unwrap();
        fs::write(
            pkg.join("desc"),
            "%NAME%\nsl\n\n%VERSION%\n5.05-6\n\n%BUILDDATE%\n1700000000\n\n%PACKAGER%\nUnknown Packager\n\n",
        )
        .unwrap();
        let got = installed_in(&dir, "sl").unwrap();
        assert_eq!(got, installed("5.05-6", 1700000000, "Unknown Packager"));
        assert!(installed_in(&dir, "absent").is_none());
        fs::remove_dir_all(&dir).unwrap();
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

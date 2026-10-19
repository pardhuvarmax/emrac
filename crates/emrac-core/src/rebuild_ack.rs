//! Acknowledging "this AUR package may need building again": the note stays
//! true, you have decided to leave it, and `doctor` and `rebuild --check` stop
//! listing it. The acknowledgement keeps what was said when you made it (which
//! libraries were missing, which update could matter), and it lapses by itself
//! when the package is built again (another version or build time) and when the
//! note says something new: a different missing library, a later library update.
//!
//! Only what *may* break can be acknowledged; a package that is broken now is
//! still listed, because acknowledging does not mend it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::stale_builds::Foreign;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ack {
    pub name: String,
    /// The installed version when it was acknowledged.
    pub version: String,
    /// Its build time then (Unix seconds): a rebuild changes it.
    pub built: u64,
    pub acked_at: u64,
    #[serde(default)]
    pub reason: Option<String>,
    /// What the note said then, line by line (`may break: ...`).
    #[serde(default)]
    pub said: Vec<String>,
}

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/rebuild-ack.json"))
}

pub fn read_from(path: &Path) -> Vec<Ack> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn read() -> Vec<Ack> {
    path().map(|p| read_from(&p)).unwrap_or_default()
}

fn write(path: &Path, list: &[Ack]) -> Result<()> {
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

/// Adds (or replaces) the acknowledgement of `pkg`.
pub fn add_to(
    path: &Path,
    pkg: &Foreign,
    said: &[String],
    reason: Option<&str>,
    now: u64,
) -> Result<Ack> {
    let ack = Ack {
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        built: pkg.built,
        acked_at: now,
        reason: reason.map(|r| r.chars().take(200).collect()),
        said: said.to_vec(),
    };
    let mut list = read_from(path);
    list.retain(|a| a.name != pkg.name);
    list.push(ack.clone());
    list.sort_by(|a, b| a.name.cmp(&b.name));
    write(path, &list)?;
    Ok(ack)
}

/// Removes the acknowledgement of `name`; whether there was one.
pub fn remove_from(path: &Path, name: &str) -> Result<bool> {
    let mut list = read_from(path);
    let before = list.len();
    list.retain(|a| a.name != name);
    if list.len() == before {
        return Ok(false);
    }
    write(path, &list)?;
    Ok(true)
}

/// Whether the acknowledgement is of this very build of the package.
pub fn applies(ack: &Ack, pkg: &Foreign) -> bool {
    ack.name == pkg.name && ack.version == pkg.version && ack.built == pkg.built
}

/// Whether the note on `pkg` says nothing that the acknowledgement did not
/// already cover: every line it says now was said then.
pub fn covers(acks: &[Ack], pkg: &Foreign, now_says: &[String]) -> bool {
    acks.iter()
        .find(|a| applies(a, pkg))
        .is_some_and(|a| now_says.iter().all(|line| a.said.contains(line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, built: u64) -> Foreign {
        Foreign {
            name: name.into(),
            version: version.into(),
            built,
            depends: vec![],
            files: vec![],
        }
    }

    #[test]
    fn an_acknowledgement_is_kept_replaced_and_removed() {
        let dir = std::env::temp_dir().join(format!("emrac-ack-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ack.json");
        assert!(read_from(&file).is_empty());
        add_to(&file, &pkg("b", "1-1", 10), &[], Some("fine"), 100).unwrap();
        add_to(&file, &pkg("a", "2-1", 20), &[], None, 101).unwrap();
        let list = read_from(&file);
        assert_eq!(
            list.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(list[1].reason.as_deref(), Some("fine"));
        add_to(&file, &pkg("b", "1-2", 30), &[], None, 200).unwrap();
        assert_eq!(
            read_from(&file)
                .iter()
                .find(|a| a.name == "b")
                .unwrap()
                .version,
            "1-2"
        );
        assert!(remove_from(&file, "a").unwrap());
        assert!(!remove_from(&file, "a").unwrap());
        assert_eq!(read_from(&file).len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn it_applies_to_that_build_and_lapses_when_the_package_is_built_again() {
        let ack = Ack {
            name: "p".into(),
            version: "1-1".into(),
            built: 10,
            acked_at: 100,
            reason: None,
            said: vec![],
        };
        assert!(applies(&ack, &pkg("p", "1-1", 10)));
        assert!(!applies(&ack, &pkg("p", "1-2", 10)), "another version");
        assert!(!applies(&ack, &pkg("p", "1-1", 11)), "built again");
        assert!(!applies(&ack, &pkg("q", "1-1", 10)), "another package");
    }

    #[test]
    fn it_covers_what_was_said_and_not_something_new() {
        let p = pkg("p", "1-1", 10);
        let said = vec!["needs libjvm".to_string(), "needs libtcl".to_string()];
        let ack = Ack {
            name: "p".into(),
            version: "1-1".into(),
            built: 10,
            acked_at: 100,
            reason: None,
            said: said.clone(),
        };
        assert!(covers(std::slice::from_ref(&ack), &p, &said));
        assert!(
            covers(std::slice::from_ref(&ack), &p, &said[..1]),
            "less is covered"
        );
        let more = vec!["needs libjvm".to_string(), "needs libnew".to_string()];
        assert!(
            !covers(std::slice::from_ref(&ack), &p, &more),
            "something new brings it back"
        );
        assert!(!covers(&[], &p, &said), "not acknowledged");
        assert!(
            !covers(&[ack], &pkg("p", "1-2", 10), &said),
            "another version"
        );
    }
}

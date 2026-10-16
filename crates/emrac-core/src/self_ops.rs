//! The journal of emrac's operations on itself (u13): `self upgrade` and
//! `self downgrade` (u14, u15). Same idea as the transaction journal (u10): a
//! file written first and locked for the whole operation, so an operation that
//! was cut off is found next time. Kept under `~/.local/share/emrac/self/`.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpEntry {
    /// `upgrade` or `downgrade`.
    pub op: String,
    pub from: String,
    pub to: String,
    pub tag: String,
    pub started: u64,
    /// `prepare`, `stage`, `activate` or `verify`: how far it got.
    pub phase: String,
    /// The folder holding the copies of config and state taken first.
    #[serde(default)]
    pub backup: Option<String>,
}

impl OpEntry {
    /// One line for a person.
    pub fn describe(&self) -> String {
        format!(
            "{} {} → {} ({}), started {} UTC, got as far as: {}",
            self.op,
            self.from,
            self.to,
            self.tag,
            crate::history::format_utc(self.started),
            self.phase
        )
    }

    /// What is known about the system after a cut-off at this phase.
    pub fn consequence(&self) -> &'static str {
        match self.phase.as_str() {
            "prepare" | "stage" => "nothing on the system was changed: only copies and downloads were made",
            "activate" => "the installed copy may or may not have been replaced: `emrac --version` says which (a package: `pacman -Q emrac`; a copy replaced in place: the old file is in ~/.local/share/emrac/self/previous)",
            _ => "the new version was installed; it had not been checked yet",
        }
    }
}

pub enum State {
    Nothing,
    Running,
    Interrupted(Box<Interrupted>),
}

pub struct Interrupted {
    pub entry: Option<OpEntry>,
    file: File,
    path: PathBuf,
}

impl Interrupted {
    pub fn clear(self) -> Result<()> {
        fs::remove_file(&self.path).map_err(|e| {
            Error::SelfService(format!("couldn't remove {}: {e}", self.path.display()))
        })?;
        drop(self.file);
        Ok(())
    }
}

/// An operation in progress. Dropping it without `finish` leaves the journal
/// behind, as a crash would.
pub struct Guard {
    file: File,
    path: PathBuf,
    entry: OpEntry,
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::SelfService(format!("{}: {e}", path.display()))
}

pub fn path() -> Result<PathBuf> {
    Ok(crate::self_meta::dir()?.join("journal.json"))
}

pub fn state() -> Result<State> {
    state_at(&path()?)
}

fn state_at(path: &Path) -> Result<State> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::Nothing),
        Err(e) => return Err(io(path, e)),
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(State::Running),
        Err(TryLockError::Error(e)) => return Err(io(path, e)),
    }
    let mut text = String::new();
    let _ = file.read_to_string(&mut text);
    Ok(State::Interrupted(Box::new(Interrupted {
        entry: serde_json::from_str(&text).ok(),
        file,
        path: path.to_path_buf(),
    })))
}

/// Starts an operation: takes the lock (refusing if another emrac holds it)
/// and writes the first entry.
pub fn begin(op: &str, from: &str, to: &str, tag: &str, now: u64) -> Result<Guard> {
    begin_at(&path()?, op, from, to, tag, now)
}

fn begin_at(path: &Path, op: &str, from: &str, to: &str, tag: &str, now: u64) -> Result<Guard> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|e| io(path, e))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(Error::SelfService(
                "another emrac is upgrading or downgrading itself right now: wait for it to finish"
                    .into(),
            ))
        }
        Err(TryLockError::Error(e)) => return Err(io(path, e)),
    }
    let mut guard = Guard {
        file,
        path: path.to_path_buf(),
        entry: OpEntry {
            op: op.into(),
            from: from.into(),
            to: to.into(),
            tag: tag.into(),
            started: now,
            phase: "prepare".into(),
            backup: None,
        },
    };
    guard.write()?;
    Ok(guard)
}

impl Guard {
    fn write(&mut self) -> Result<()> {
        let json = serde_json::to_string(&self.entry).expect("OpEntry is serializable");
        self.file.set_len(0).map_err(|e| io(&self.path, e))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| io(&self.path, e))?;
        self.file
            .write_all(json.as_bytes())
            .and_then(|()| self.file.sync_all())
            .map_err(|e| io(&self.path, e))
    }

    pub fn phase(&mut self, phase: &str) -> Result<()> {
        self.entry.phase = phase.into();
        self.write()
    }

    pub fn backup(&mut self, dir: &Path) -> Result<()> {
        self.entry.backup = Some(dir.display().to_string());
        self.write()
    }

    /// The operation ended (well or not, and was reported): remove the journal.
    pub fn finish(self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-selfops-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_running_operation_is_locked_and_a_cut_off_one_is_found_with_its_phase() {
        let d = scratch("lock");
        let p = d.join("journal.json");
        assert!(matches!(state_at(&p).unwrap(), State::Nothing));
        let mut g = begin_at(&p, "upgrade", "0.2.0", "0.3.0", "r3", 7).unwrap();
        g.phase("activate").unwrap();
        g.backup(&d.join("backup-7")).unwrap();
        assert!(matches!(state_at(&p).unwrap(), State::Running));
        assert!(begin_at(&p, "upgrade", "0.2.0", "0.3.0", "r3", 8).is_err());
        drop(g); // a crash: the lock goes, the file stays
                 // A child forked by another test in this process can hold the
                 // descriptor for a moment before it execs: ask again briefly.
        let mut found = state_at(&p).unwrap();
        for _ in 0..100 {
            if !matches!(found, State::Running) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            found = state_at(&p).unwrap();
        }
        match found {
            State::Interrupted(i) => {
                let e = i.entry.clone().unwrap();
                assert_eq!(
                    (e.phase.as_str(), e.tag.as_str(), e.started),
                    ("activate", "r3", 7)
                );
                assert!(e.backup.as_deref().unwrap().ends_with("backup-7"));
                assert!(e.describe().contains("0.2.0 → 0.3.0"));
                assert!(e.consequence().contains("may or may not"));
                i.clear().unwrap();
            }
            _ => panic!("expected an interruption"),
        }
        assert!(matches!(state_at(&p).unwrap(), State::Nothing));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn finishing_removes_the_journal_and_each_phase_has_a_consequence() {
        let d = scratch("finish");
        let p = d.join("journal.json");
        begin_at(&p, "downgrade", "0.3.0", "0.2.0", "r2", 1)
            .unwrap()
            .finish();
        assert!(!p.exists());
        let mut e = OpEntry {
            op: "upgrade".into(),
            from: "a".into(),
            to: "b".into(),
            tag: "t".into(),
            started: 0,
            phase: "stage".into(),
            backup: None,
        };
        assert!(e.consequence().contains("nothing on the system"));
        e.phase = "verify".into();
        assert!(e.consequence().contains("had not been checked"));
        let _ = fs::remove_dir_all(d);
    }
}

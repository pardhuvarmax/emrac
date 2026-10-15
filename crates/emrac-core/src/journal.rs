//! The transaction journal (u10): a file written before a plan is handed to
//! pacman and removed after the result is recorded, so a transaction that was
//! cut off (power loss, a killed terminal, a hang) is found next time.
//! See `docs/plan/r2/u10-journal.md`.
//!
//! Liveness is an exclusive `flock` held on the file for the whole
//! transaction. The kernel drops it when the process dies for any reason, so
//! "the file exists and can be locked" means the run that wrote it is gone.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::plan::{Plan, PlanAction, PlannedPackage, PlannedRemoval};

const PACMAN_LOG: &str = "/var/log/pacman.log";

/// What the journal records when a transaction starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub started: u64,
    pub action: PlanAction,
    pub packages: Vec<PlannedPackage>,
    #[serde(default)]
    pub removals: Vec<PlannedRemoval>,
    /// Size of pacman's log when the transaction began: whatever pacman wrote
    /// after this point belongs to this run.
    pub pacman_log_offset: u64,
    pub emrac_version: String,
}

impl Entry {
    pub fn new(plan: &Plan, started: u64, pacman_log_offset: u64) -> Entry {
        Entry {
            started,
            action: plan.action,
            packages: plan.packages.clone(),
            removals: plan.removals.clone(),
            pacman_log_offset,
            emrac_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// The plan this entry was made from (sizes are not kept).
    pub fn plan(&self) -> Plan {
        Plan {
            action: self.action,
            packages: self.packages.clone(),
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: self.removals.clone(),
            provider_choices: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Where a planned package stands after a cut-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The planned change is in place.
    Done,
    /// Still as it was before.
    NotReached,
    /// Neither: something else changed it (the text says what is installed).
    Other(String),
}

/// One `[ALPM]` action in pacman's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogAction {
    pub action: String,
    pub package: String,
}

/// What pacman's log says about the range that belongs to the transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogRange {
    pub actions: Vec<LogAction>,
    /// `transaction started` lines with no `transaction completed` after them.
    pub unfinished: bool,
    /// Whether pacman ran at all in the range.
    pub pacman_ran: bool,
}

/// Reads the `[ALPM]` lines of `text` (pacman's log from the saved offset).
pub fn parse_pacman_log(text: &str) -> LogRange {
    let mut range = LogRange::default();
    let (mut started, mut completed) = (0usize, 0usize);
    for line in text.lines() {
        let Some(rest) = line.split_once("[ALPM] ").map(|(_, r)| r) else {
            continue;
        };
        if rest.starts_with("transaction started") {
            started += 1;
            range.pacman_ran = true;
        } else if rest.starts_with("transaction completed") {
            completed += 1;
        } else if let Some((action, tail)) = rest.split_once(' ') {
            if matches!(
                action,
                "installed" | "upgraded" | "downgraded" | "removed" | "reinstalled"
            ) {
                if let Some(package) = tail.split_whitespace().next() {
                    range.actions.push(LogAction {
                        action: action.to_string(),
                        package: package.to_string(),
                    });
                }
            }
        }
    }
    range.unfinished = started > completed;
    range
}

/// What a cut-off transaction left behind, from pacman's log and the
/// installed versions.
#[derive(Debug, Clone, Default)]
pub struct Assessment {
    pub done: Vec<String>,
    pub not_reached: Vec<String>,
    pub other: Vec<String>,
    /// pacman started a transaction in the range and never finished it.
    pub pacman_unfinished: bool,
    pub pacman_ran: bool,
}

impl Assessment {
    /// The sentence recorded as the error of the history record.
    pub fn summary(&self, entry: &Entry) -> String {
        let when = crate::history::format_utc(entry.started);
        let mut parts = Vec::new();
        if !self.done.is_empty() {
            parts.push(format!("done: {}", self.done.join(", ")));
        }
        if !self.not_reached.is_empty() {
            parts.push(format!("not reached: {}", self.not_reached.join(", ")));
        }
        if !self.other.is_empty() {
            parts.push(format!("changed otherwise: {}", self.other.join(", ")));
        }
        let state = if !self.pacman_ran {
            "pacman never started, so nothing was installed or removed".to_string()
        } else if self.pacman_unfinished {
            format!(
                "pacman started a transaction and its log has no end for it ({})",
                parts.join("; ")
            )
        } else {
            parts.join("; ")
        };
        format!(
            "interrupted: this transaction (started {when} UTC) was cut off before emrac recorded it — {state}"
        )
    }
}

/// Works out where each planned package stands. `installed` gives the
/// version installed now.
pub fn assess(
    entry: &Entry,
    range: &LogRange,
    installed: &dyn Fn(&str) -> Option<String>,
) -> Assessment {
    let mut a = Assessment {
        pacman_unfinished: range.unfinished,
        pacman_ran: range.pacman_ran,
        ..Assessment::default()
    };
    for p in &entry.packages {
        let now = installed(&p.name);
        let standing = match entry.action {
            PlanAction::Remove | PlanAction::Purge => {
                if now.is_none() {
                    Standing::Done
                } else {
                    Standing::NotReached
                }
            }
            PlanAction::Reinstall => {
                if range
                    .actions
                    .iter()
                    .any(|l| l.action == "reinstalled" && l.package == p.name)
                {
                    Standing::Done
                } else {
                    Standing::NotReached
                }
            }
            PlanAction::Install | PlanAction::Upgrade | PlanAction::Downgrade => {
                match (&now, &p.previous_version) {
                    (Some(v), _) if *v == p.version => Standing::Done,
                    (None, _) if entry.action == PlanAction::Install => Standing::NotReached,
                    (Some(v), Some(prev)) if v == prev => Standing::NotReached,
                    (Some(v), _) => Standing::Other(format!("{v} is installed")),
                    (None, _) => Standing::Other("not installed".into()),
                }
            }
        };
        let label = |extra: &str| {
            if extra.is_empty() {
                format!("{} {}", p.name, p.version)
            } else {
                format!("{} {} ({extra})", p.name, p.version)
            }
        };
        match standing {
            Standing::Done => a.done.push(label("")),
            Standing::NotReached => a.not_reached.push(label("")),
            Standing::Other(why) => a.other.push(label(&why)),
        }
    }
    a
}

/// A transaction whose process is gone. Holds the journal's lock until
/// `clear`.
pub struct Interrupted {
    /// `None`: the file could not be read as a journal (cut off while it was
    /// being written).
    pub entry: Option<Entry>,
    file: File,
    path: PathBuf,
}

impl Interrupted {
    /// The part of pacman's log that belongs to the transaction.
    pub fn log_range(&self) -> LogRange {
        let offset = self.entry.as_ref().map_or(0, |e| e.pacman_log_offset);
        parse_pacman_log(&read_log_from(Path::new(PACMAN_LOG), offset))
    }

    /// Removes the journal (the interruption has been recorded or dismissed).
    pub fn clear(self) -> Result<()> {
        fs::remove_file(&self.path)
            .map_err(|e| Error::Journal(format!("couldn't remove {}: {e}", self.path.display())))?;
        drop(self.file);
        Ok(())
    }
}

/// What `state` found.
pub enum State {
    Nothing,
    /// Another emrac is changing packages right now.
    Running,
    Interrupted(Box<Interrupted>),
}

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/journal.json"))
}

/// Looks at the journal without starting anything.
pub fn state() -> Result<State> {
    state_at(&path()?)
}

fn state_at(path: &Path) -> Result<State> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::Nothing),
        Err(e) => return Err(io_err(path, e)),
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(State::Running),
        Err(TryLockError::Error(e)) => return Err(io_err(path, e)),
    }
    let mut text = String::new();
    let _ = file.read_to_string(&mut text);
    Ok(State::Interrupted(Box::new(Interrupted {
        entry: serde_json::from_str(&text).ok(),
        file,
        path: path.to_path_buf(),
    })))
}

/// Now, in unix seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The journal of the transaction in progress in this process, if any.
static ACTIVE: Mutex<Option<File>> = Mutex::new(None);

/// Writes the journal for `plan` and takes the lock. Fails when another
/// emrac holds it. A leftover from a cut-off run (lockable) is replaced:
/// callers deal with it through `state` first.
pub fn begin(plan: &Plan, now: u64) -> Result<()> {
    let file = begin_at(&path()?, plan, now, log_size(Path::new(PACMAN_LOG)))?;
    *ACTIVE.lock().unwrap() = Some(file);
    Ok(())
}

fn begin_at(path: &Path, plan: &Plan, now: u64, offset: u64) -> Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| io_err(path, e))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(Error::Journal(
                "another emrac is changing packages right now: wait for it to finish".into(),
            ))
        }
        Err(TryLockError::Error(e)) => return Err(io_err(path, e)),
    }
    let entry = Entry::new(plan, now, offset);
    let json = serde_json::to_string(&entry).expect("Entry is serializable");
    file.set_len(0).map_err(|e| io_err(path, e))?;
    file.seek(SeekFrom::Start(0)).map_err(|e| io_err(path, e))?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| io_err(path, e))?;
    Ok(file)
}

/// The transaction ended and has been recorded: remove the journal. Does
/// nothing when none was begun.
pub fn finish() {
    if ACTIVE.lock().unwrap().take().is_some() {
        if let Ok(p) = path() {
            let _ = fs::remove_file(p);
        }
    }
}

fn log_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// The log's text from `offset` on. A log shorter than the offset was
/// rotated since: it is read from the start.
fn read_log_from(path: &Path, offset: u64) -> String {
    let Ok(mut f) = File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = if offset > len { 0 } else { offset };
    let mut bytes = Vec::new();
    if f.seek(SeekFrom::Start(from)).is_err() || f.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::Journal(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlannedPackage;

    fn pkg(name: &str, version: &str, previous: Option<&str>) -> PlannedPackage {
        PlannedPackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            explicit: true,
            previous_version: previous.map(str::to_string),
        }
    }

    fn plan(action: PlanAction, packages: Vec<PlannedPackage>) -> Plan {
        Plan {
            action,
            packages,
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: Vec::new(),
            provider_choices: Vec::new(),
            warnings: Vec::new(),
        }
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("emrac-journal-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("journal.json")
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const LOG: &str = "\
[2026-10-15T10:00:00+0000] [PACMAN] Running 'pacman -S foo'
[2026-10-15T10:00:01+0000] [ALPM] transaction started
[2026-10-15T10:00:02+0000] [ALPM] installed foo (1.0-1)
[2026-10-15T10:00:03+0000] [ALPM] upgraded bar (1.0-1 -> 2.0-1)
[2026-10-15T10:00:04+0000] [ALPM] transaction completed
[2026-10-15T10:01:00+0000] [ALPM] transaction started
[2026-10-15T10:01:01+0000] [ALPM] removed baz (3.0-1)
";

    #[test]
    fn the_pacman_log_is_read_for_actions_and_an_unfinished_transaction() {
        let r = parse_pacman_log(LOG);
        assert_eq!(r.actions.len(), 3);
        assert_eq!(r.actions[1].action, "upgraded");
        assert_eq!(r.actions[1].package, "bar");
        assert!(r.pacman_ran);
        assert!(r.unfinished, "the second transaction has no end");
        let whole = parse_pacman_log(&LOG.lines().take(5).collect::<Vec<_>>().join("\n"));
        assert!(!whole.unfinished);
        let none = parse_pacman_log("[2026-10-15T10:00:00+0000] [PACMAN] Running 'pacman -Q'\n");
        assert!(!none.pacman_ran && none.actions.is_empty());
    }

    #[test]
    fn a_log_shorter_than_the_offset_is_read_from_the_start() {
        let s = Scratch::new("rotate");
        let log = s.0.join("pacman.log");
        fs::write(&log, LOG).unwrap();
        assert!(read_log_from(&log, 0).contains("installed foo"));
        let from = LOG.find("[2026-10-15T10:01:00").unwrap() as u64;
        let tail = read_log_from(&log, from);
        assert!(tail.contains("removed baz") && !tail.contains("installed foo"));
        assert!(read_log_from(&log, 10_000_000).contains("installed foo"));
        assert_eq!(read_log_from(&s.0.join("missing"), 0), "");
    }

    #[test]
    fn each_planned_package_is_done_not_reached_or_otherwise_changed() {
        let entry = Entry::new(
            &plan(
                PlanAction::Upgrade,
                vec![
                    pkg("a", "2.0-1", Some("1.0-1")),
                    pkg("b", "2.0-1", Some("1.0-1")),
                    pkg("c", "2.0-1", Some("1.0-1")),
                ],
            ),
            100,
            0,
        );
        let installed = |n: &str| match n {
            "a" => Some("2.0-1".to_string()),
            "b" => Some("1.0-1".to_string()),
            _ => Some("1.5-1".to_string()),
        };
        let range = parse_pacman_log(LOG);
        let a = assess(&entry, &range, &installed);
        assert_eq!(a.done, vec!["a 2.0-1"]);
        assert_eq!(a.not_reached, vec!["b 2.0-1"]);
        assert_eq!(a.other, vec!["c 2.0-1 (1.5-1 is installed)"]);
        assert!(a.pacman_unfinished);
        let text = a.summary(&entry);
        assert!(text.starts_with("interrupted:"), "{text}");
        assert!(text.contains("no end for it"), "{text}");
    }

    #[test]
    fn removals_and_installs_and_reinstalls_are_told_by_what_is_installed_or_logged() {
        let none = |_: &str| None;
        let all = |n: &str| Some(format!("1.0-1{n}"));
        let remove = Entry::new(
            &plan(PlanAction::Remove, vec![pkg("x", "1.0-1", None)]),
            1,
            0,
        );
        assert_eq!(assess(&remove, &LogRange::default(), &none).done.len(), 1);
        assert_eq!(
            assess(&remove, &LogRange::default(), &all)
                .not_reached
                .len(),
            1
        );
        let install = Entry::new(
            &plan(PlanAction::Install, vec![pkg("x", "1.0-1", None)]),
            1,
            0,
        );
        let a = assess(&install, &LogRange::default(), &none);
        assert_eq!(a.not_reached.len(), 1);
        assert!(!a.pacman_ran);
        assert!(a.summary(&install).contains("pacman never started"));
        let re = Entry::new(
            &plan(PlanAction::Reinstall, vec![pkg("foo", "1.0-1", None)]),
            1,
            0,
        );
        let logged = parse_pacman_log("[t] [ALPM] reinstalled foo (1.0-1)\n");
        assert_eq!(assess(&re, &logged, &all).done.len(), 1);
        assert_eq!(assess(&re, &LogRange::default(), &all).not_reached.len(), 1);
    }

    #[test]
    fn a_journal_that_can_be_locked_is_interrupted_and_one_that_cannot_is_running() {
        let s = Scratch::new("lock");
        assert!(matches!(state_at(&s.file()).unwrap(), State::Nothing));
        let p = plan(PlanAction::Install, vec![pkg("x", "1.0-1", None)]);
        let held = begin_at(&s.file(), &p, 42, 7).unwrap();
        // Held by this process: another opener sees it as running.
        assert!(matches!(state_at(&s.file()).unwrap(), State::Running));
        assert!(begin_at(&s.file(), &p, 43, 7).is_err());
        // The holder goes away without finishing (as in a crash).
        drop(held);
        match state_at(&s.file()).unwrap() {
            State::Interrupted(i) => {
                let e = i.entry.as_ref().expect("the entry is readable");
                assert_eq!((e.started, e.pacman_log_offset), (42, 7));
                assert_eq!(e.packages[0].name, "x");
                i.clear().unwrap();
            }
            _ => panic!("a leftover journal is an interruption"),
        }
        assert!(matches!(state_at(&s.file()).unwrap(), State::Nothing));
    }

    #[test]
    fn a_half_written_journal_is_still_an_interruption() {
        let s = Scratch::new("torn");
        fs::write(s.file(), r#"{"started": 5, "act"#).unwrap();
        match state_at(&s.file()).unwrap() {
            State::Interrupted(i) => assert!(i.entry.is_none()),
            _ => panic!("expected an interruption"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_journal_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let s = Scratch::new("mode");
        let p = plan(PlanAction::Install, vec![pkg("x", "1.0-1", None)]);
        let _held = begin_at(&s.file(), &p, 1, 0).unwrap();
        let mode = fs::metadata(s.file()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

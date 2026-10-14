//! The record of what `emrac pacnew` changed: one JSON object per line in
//! `~/.local/share/emrac/pacnew-log.jsonl`. A change is written as `started`
//! before anything happens and again as `done` or `failed` after, so a crash
//! leaves a visible half-finished entry. See `docs/plan/u30-pacnew-revert.md`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Started,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// `cfg-N`.
    pub id: String,
    /// Unix seconds.
    pub time: u64,
    /// The live path.
    pub file: String,
    pub package: Option<String>,
    /// `keep`, `take`, `merge`, `edit` or `revert`.
    pub action: String,
    pub class: String,
    /// SHA-256 of the live file before and after.
    pub before: String,
    pub after: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Copy of the live file made before the change.
    pub live_backup: Option<PathBuf>,
    /// Copy of the `.pacnew` made before the change.
    pub pending_backup: Option<PathBuf>,
    /// Where the `.pacnew` was, and its mode and owner.
    pub pending: String,
    #[serde(default)]
    pub pending_mode: u32,
    #[serde(default)]
    pub pending_uid: u32,
    #[serde(default)]
    pub pending_gid: u32,
    pub state: State,
    /// For a `revert`: the id it undid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Record {
    /// `YYYY-MM-DD HH:MM` in UTC.
    pub fn time_utc(&self) -> String {
        crate::history::format_utc(self.time)
    }
}

pub fn log_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoHomeDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/pacnew-log.jsonl"))
}

/// Appends `record` (a new line; the latest line of an id wins).
pub fn append(path: &Path, record: &Record) -> Result<()> {
    let io = |context: String| move |source| Error::Io { context, source };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(io(format!("couldn't create {}", parent.display())))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io(format!("couldn't open {}", path.display())))?;
    let line = serde_json::to_string(record).expect("a Record always serializes");
    writeln!(file, "{line}").map_err(io(format!("couldn't write {}", path.display())))
}

/// One entry per id, oldest first, each in its latest state. Unreadable lines
/// are skipped.
pub fn read(path: &Path) -> Result<Vec<Record>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(Error::Io {
                context: format!("couldn't read {}", path.display()),
                source,
            })
        }
    };
    let mut out: Vec<Record> = Vec::new();
    for record in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
    {
        match out.iter_mut().find(|r| r.id == record.id) {
            Some(existing) => *existing = record,
            None => out.push(record),
        }
    }
    Ok(out)
}

/// `cfg-N` for the next record.
pub fn next_id(records: &[Record]) -> String {
    let max = records
        .iter()
        .filter_map(|r| r.id.strip_prefix("cfg-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("cfg-{}", max + 1)
}

/// What a recorded change looks like now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    Applied,
    Reverted,
    /// The live file no longer matches what the change left.
    ChangedSince,
    /// The live file can't be read without root, so it wasn't compared.
    Unchecked,
    Interrupted,
    Failed,
}

impl Standing {
    pub fn label(self) -> &'static str {
        match self {
            Standing::Applied => "applied",
            Standing::Reverted => "reverted",
            Standing::ChangedSince => "changed since",
            Standing::Unchecked => "applied (not checked: needs root)",
            Standing::Interrupted => "interrupted",
            Standing::Failed => "failed",
        }
    }
}

/// `record`'s standing, given every record and the live file's hash now
/// (`None` if it can't be read).
pub fn standing(record: &Record, all: &[Record], live_hash_now: Option<&str>) -> Standing {
    match record.state {
        State::Started => return Standing::Interrupted,
        State::Failed => return Standing::Failed,
        State::Done => {}
    }
    if all
        .iter()
        .any(|r| r.reverts.as_deref() == Some(&record.id) && r.state == State::Done)
    {
        return Standing::Reverted;
    }
    match live_hash_now {
        Some(h) if h == record.after => Standing::Applied,
        Some(_) => Standing::ChangedSince,
        None => Standing::Unchecked,
    }
}

/// SHA-256 of `bytes`, as `sha256sum` prints it.
pub fn sha256(bytes: &[u8]) -> Result<String> {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::CommandSpawn("sha256sum".to_string(), e))?;
    let mut stdin = child.stdin.take().expect("piped");
    let data = bytes.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child
        .wait_with_output()
        .map_err(|e| Error::CommandSpawn("sha256sum".to_string(), e))?;
    let _ = writer.join();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| Error::CommandFailed {
            command: "sha256sum".to_string(),
            stderr: "no output".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, state: State, after: &str) -> Record {
        Record {
            id: id.into(),
            time: 1,
            file: "/etc/x.conf".into(),
            package: Some("x".into()),
            action: "take".into(),
            class: "untouched".into(),
            before: "aaa".into(),
            after: after.into(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            live_backup: Some("/b/x".into()),
            pending_backup: Some("/b/x.pacnew".into()),
            pending: "/etc/x.conf.pacnew".into(),
            pending_mode: 0o644,
            pending_uid: 0,
            pending_gid: 0,
            state,
            reverts: None,
            error: None,
        }
    }

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("emrac-pnlog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("sub/pacnew-log.jsonl")
    }

    #[test]
    fn the_latest_line_of_an_id_wins_and_ids_count_up() {
        let path = temp_log("rt");
        assert!(read(&path).unwrap().is_empty(), "no file is an empty log");
        append(&path, &record("cfg-1", State::Started, "bbb")).unwrap();
        append(&path, &record("cfg-1", State::Done, "bbb")).unwrap();
        append(&path, &record("cfg-2", State::Started, "ccc")).unwrap();
        std::fs::write(
            &path,
            format!("garbage\n{}", std::fs::read_to_string(&path).unwrap()),
        )
        .unwrap();
        let all = read(&path).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].state, State::Done);
        assert_eq!(all[1].state, State::Started);
        assert_eq!(next_id(&all), "cfg-3");
        assert_eq!(next_id(&[]), "cfg-1");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_change_is_applied_reverted_changed_since_or_interrupted() {
        let done = record("cfg-1", State::Done, "bbb");
        let all = vec![done.clone()];
        assert_eq!(standing(&done, &all, Some("bbb")), Standing::Applied);
        assert_eq!(standing(&done, &all, Some("zzz")), Standing::ChangedSince);
        assert_eq!(standing(&done, &all, None), Standing::Unchecked);
        let mut undo = record("cfg-2", State::Done, "aaa");
        undo.reverts = Some("cfg-1".into());
        let all = vec![done.clone(), undo.clone()];
        assert_eq!(standing(&done, &all, Some("aaa")), Standing::Reverted);
        let started = record("cfg-3", State::Started, "x");
        assert_eq!(
            standing(&started, std::slice::from_ref(&started), None),
            Standing::Interrupted
        );
        let failed = record("cfg-4", State::Failed, "x");
        assert_eq!(
            standing(&failed, std::slice::from_ref(&failed), None),
            Standing::Failed
        );
        // A revert that did not finish does not count as one.
        let mut half = record("cfg-5", State::Started, "aaa");
        half.reverts = Some("cfg-1".into());
        assert_eq!(
            standing(&done, &[done.clone(), half], Some("bbb")),
            Standing::Applied
        );
    }

    #[test]
    fn sha256_matches_the_known_digest() {
        assert_eq!(
            sha256(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}

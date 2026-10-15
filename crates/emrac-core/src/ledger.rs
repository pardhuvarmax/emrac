//! The ledger: a second copy of the history that the user's own programs
//! cannot rewrite. A root-owned, append-only file at `/var/lib/emrac/ledger.jsonl`;
//! only the hidden `emrac ledger-append` helper, run through `sudo`, writes it.
//! See `docs/plan/u8-ledger.md`.
//!
//! Each ledger line wraps one line of the user's history, byte for byte:
//! `{"at":…,"line":"<the user's line>","lprev":…,"lseq":…,"sha":…,"tx":"tx-3"}`.
//! `lseq`/`lprev` are the ledger's own chain, `sha` is the SHA-256 of `line`
//! (how the two copies are matched), and `at` is root's clock, not the user's.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::history::TransactionRecord;
use crate::history_log::{self as log, sha256_hex, Break, GENESIS};

pub const LEDGER_PATH: &str = "/var/lib/emrac/ledger.jsonl";

/// One history line is never this big; a bigger one is refused.
pub const MAX_LINE: usize = 64 * 1024;
/// Most lines one helper call takes.
pub const MAX_BATCH: usize = 2000;

const ATTR_TOOL: &str = "/usr/bin/chattr";
const ATTR_LIST: &str = "/usr/bin/lsattr";

/// Whether this process runs as root (what owns `/proc/self` is the effective user).
pub fn running_as_root() -> bool {
    fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

/// Where the ledger is. As root it is always `LEDGER_PATH`; for anyone else
/// `EMRAC_LEDGER` may point elsewhere (tests, and a look at a copy).
pub fn path(is_root: bool) -> PathBuf {
    if !is_root {
        if let Some(p) = std::env::var_os("EMRAC_LEDGER").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(LEDGER_PATH)
}

fn io_err(path: &Path, source: std::io::Error) -> Error {
    Error::HistoryWrite {
        path: path.display().to_string(),
        source,
    }
}

/// Whether `line` is something the ledger takes: a transaction record or
/// one of the markers, a JSON object, not too big. Returns a label for
/// the `tx` / `type` field.
fn accept(line: &str) -> std::result::Result<String, String> {
    if line.len() > MAX_LINE {
        return Err(format!(
            "a line is {} bytes, over the {MAX_LINE} limit",
            line.len()
        ));
    }
    let value: Value = serde_json::from_str(line).map_err(|_| "a line is not JSON".to_string())?;
    let Value::Object(map) = &value else {
        return Err("a line is not a JSON object".into());
    };
    match map.get("type").and_then(Value::as_str) {
        Some("ack") => {
            let ok = map
                .get("ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().all(|i| i.as_str().is_some()));
            if ok {
                Ok("ack".into())
            } else {
                Err("an ack line has no list of ids".into())
            }
        }
        Some(kind @ ("archive" | "chain-start")) => Ok(kind.into()),
        Some(other) => Err(format!("unknown line type '{other}'")),
        None => {
            let record: TransactionRecord = serde_json::from_value(value)
                .map_err(|_| "a line is not a transaction record".to_string())?;
            if record
                .id
                .strip_prefix("tx-")
                .is_some_and(|n| n.parse::<u64>().is_ok())
            {
                Ok(record.id)
            } else {
                Err(format!("'{}' is not a transaction id", record.id))
            }
        }
    }
}

fn is_marker(label: &str) -> bool {
    !label.starts_with("tx-")
}

/// A parsed ledger line.
#[derive(Debug, Clone)]
pub struct Entry {
    pub lseq: u64,
    pub at: u64,
    pub sha: String,
    /// `tx-N`, or `ack`, `archive`, `chain-start`.
    pub label: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ledger {
    #[serde(skip)]
    pub entries: Vec<Entry>,
    pub count: usize,
    /// First place the ledger's own chain does not hold.
    #[serde(rename = "break")]
    pub problem: Option<Break>,
}

/// Reads the ledger and checks its own chain. A missing file is `None`.
pub fn read(path: &Path) -> Result<Option<Ledger>> {
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::Io {
                context: format!("couldn't read the ledger at {}", path.display()),
                source: e,
            })
        }
    };
    let file = path.file_name().map_or_else(
        || "ledger.jsonl".into(),
        |f| f.to_string_lossy().into_owned(),
    );
    let mut entries = Vec::new();
    let mut problem = None;
    let mut prev = GENESIS.to_string();
    let mut last_id: Option<String> = None;
    for (i, line) in raw.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let fail = |reason: &str| Break {
            file: file.clone(),
            line: i + 1,
            reason: reason.to_string(),
            before: last_id.clone(),
            after: None,
        };
        let parsed: Option<Value> = serde_json::from_str(line).ok();
        let field = |k: &str| parsed.as_ref().and_then(|v| v.get(k));
        let (Some(lseq), Some(lprev), Some(sha), Some(inner)) = (
            field("lseq").and_then(Value::as_u64),
            field("lprev").and_then(Value::as_str),
            field("sha").and_then(Value::as_str),
            field("line").and_then(Value::as_str),
        ) else {
            problem = Some(fail("is not a ledger line"));
            break;
        };
        if lseq != i as u64 + 1 {
            problem = Some(fail(
                "the sequence is not continuous — a line was removed or added",
            ));
            break;
        }
        if lprev != prev {
            problem = Some(fail(
                "does not follow the line before it — an earlier line was changed",
            ));
            break;
        }
        if sha != sha256_hex(inner.as_bytes()) {
            problem = Some(fail("its recorded hash does not match its content"));
            break;
        }
        let label = field("tx")
            .or_else(|| field("type"))
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        if label.starts_with("tx-") {
            last_id = Some(label.clone());
        }
        entries.push(Entry {
            lseq,
            at: field("at").and_then(Value::as_u64).unwrap_or(0),
            sha: sha.to_string(),
            label,
            line: inner.to_string(),
        });
        prev = sha256_hex(line.as_bytes());
    }
    Ok(Some(Ledger {
        count: entries.len(),
        entries,
        problem,
    }))
}

/// What the helper does: checks every line, then appends the ones the
/// ledger does not hold yet. Nothing is written if any line is refused.
/// Returns how many lines were added. With `set_attr`, a ledger created by
/// this call gets the append-only attribute where the filesystem has one.
pub fn append(path: &Path, lines: &[String], now: u64, set_attr: bool) -> Result<usize> {
    Ok(append_detailed(path, lines, now, set_attr)?.added)
}

/// What one call to the helper did, for the line it leaves in the journal.
#[derive(Debug, Clone, Default)]
pub struct Appended {
    pub added: usize,
    pub first: String,
    pub last: String,
    /// Hash of the ledger's newest line afterwards.
    pub head: String,
    /// This call created the ledger (and ran `chattr`).
    pub created: bool,
}

pub fn append_detailed(
    path: &Path,
    lines: &[String],
    now: u64,
    set_attr: bool,
) -> Result<Appended> {
    if lines.len() > MAX_BATCH {
        return Err(Error::History(format!(
            "ledger-append takes at most {MAX_BATCH} lines at a time"
        )));
    }
    let mut labelled = Vec::new();
    for line in lines {
        let label = accept(line).map_err(|why| Error::History(format!("ledger refused: {why}")))?;
        labelled.push((line, label));
    }

    let existing = match read(path)? {
        Some(l) => {
            if let Some(b) = l.problem {
                return Err(Error::History(format!(
                    "ledger refused: its own chain is broken — {}",
                    b.describe()
                )));
            }
            Some(l)
        }
        None => None,
    };
    let have: HashSet<&str> = existing
        .iter()
        .flat_map(|l| l.entries.iter().map(|e| e.sha.as_str()))
        .collect();
    let (mut seq, mut prev) = match raw_last(path)? {
        Some((seq, hash)) => (seq + 1, hash),
        None => (1, GENESIS.to_string()),
    };

    let mut out = String::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut added = 0;
    let (mut first, mut last) = (String::new(), String::new());
    for (line, label) in labelled {
        let sha = sha256_hex(line.as_bytes());
        if have.contains(sha.as_str()) || !seen.insert(sha.clone()) {
            continue;
        }
        let key = if is_marker(&label) { "type" } else { "tx" };
        let wrapped = serde_json::to_string(&json!({
            "lseq": seq, "lprev": prev, "at": now, "sha": sha, key: label, "line": line,
        }))
        .expect("serializable");
        prev = sha256_hex(wrapped.as_bytes());
        seq += 1;
        added += 1;
        if first.is_empty() {
            first = label.clone();
        }
        last = label.clone();
        out.push_str(&wrapped);
        out.push('\n');
    }
    if added == 0 {
        return Ok(Appended::default());
    }

    let creating = !path.exists();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o644)
        .open(path)
        .map_err(|e| io_err(path, e))?;
    file.write_all(out.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| io_err(path, e))?;
    if creating && set_attr {
        // Best effort: not every filesystem has the attribute (doctor says so).
        let _ = Command::new(ATTR_TOOL).arg("+a").arg(path).output();
    }
    Ok(Appended {
        added,
        first,
        last,
        head: prev,
        created: creating,
    })
}

/// `lseq` and the hash of the last raw line, if any.
fn raw_last(path: &Path) -> Result<Option<(u64, String)>> {
    let lines = log::read_lines(path)?;
    let Some(last) = lines.last() else {
        return Ok(None);
    };
    let seq = serde_json::from_str::<Value>(last)
        .ok()
        .and_then(|v| v.get("lseq").and_then(Value::as_u64))
        .unwrap_or(lines.len() as u64);
    Ok(Some((seq, sha256_hex(last.as_bytes()))))
}

// ---- how the file looks -----------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Protection {
    pub root_owned: bool,
    /// `None`: this system can't tell (no `lsattr`, or the filesystem has no attributes).
    pub append_only: Option<bool>,
    pub world_writable: bool,
}

pub fn protection(path: &Path) -> Option<Protection> {
    let meta = fs::metadata(path).ok()?;
    let append_only = Command::new(ATTR_LIST)
        .arg("-d")
        .arg(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_lsattr(&String::from_utf8_lossy(&o.stdout)));
    Some(Protection {
        root_owned: meta.uid() == 0,
        append_only,
        world_writable: meta.mode() & 0o002 != 0,
    })
}

/// `lsattr -d` prints the flags, then the path: `-----a--------e------- /f`.
fn parse_lsattr(output: &str) -> Option<bool> {
    let flags = output.split_whitespace().next()?;
    (flags.len() >= 5 && flags.chars().all(|c| c == '-' || c.is_ascii_alphabetic()))
        .then(|| flags.contains('a'))
}

// ---- comparing the two copies ------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct Comparison {
    /// In the ledger, gone from the user's history.
    pub missing: Vec<String>,
    /// In both, but different.
    pub changed: Vec<String>,
    /// Archive files whose content is not what the ledger recorded.
    pub archives: Vec<String>,
    /// User lines the ledger does not hold yet.
    pub pending: usize,
}

impl Comparison {
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty() && self.archives.is_empty()
    }
}

/// `user_lines`: the archive files' lines, then the live log's (head marker
/// included). `history_path` locates the archive files.
pub fn compare(ledger: &Ledger, history_path: &Path) -> Result<Comparison> {
    let live = log::read_lines(history_path)?;
    let mut user = log::all_lines(history_path, true)?;
    if let Some(head) = live
        .first()
        .filter(|l| log::head_of(&[(*l).clone()]).is_some())
    {
        user.push(head.clone());
    }
    let user_shas: HashSet<String> = user.iter().map(|l| sha256_hex(l.as_bytes())).collect();
    let mut by_id: HashMap<String, String> = HashMap::new();
    for line in &user {
        if let Ok(r) = serde_json::from_str::<TransactionRecord>(line) {
            by_id.insert(r.id, sha256_hex(line.as_bytes()));
        }
    }

    let mut c = Comparison::default();
    for entry in &ledger.entries {
        if entry.label.starts_with("tx-") {
            match by_id.get(&entry.label) {
                None => c.missing.push(entry.label.clone()),
                Some(sha) if *sha != entry.sha => c.changed.push(entry.label.clone()),
                Some(_) => {}
            }
        } else if entry.label == "ack" && !user_shas.contains(&entry.sha) {
            c.missing.push("an acknowledgement".into());
        }
    }
    let ledger_shas: HashSet<&str> = ledger.entries.iter().map(|e| e.sha.as_str()).collect();
    c.pending = user
        .iter()
        .filter(|l| !ledger_shas.contains(sha256_hex(l.as_bytes()).as_str()))
        .count();

    // The newest archive marker the ledger holds says what each archive file is.
    let newest = ledger
        .entries
        .iter()
        .rev()
        .find(|e| e.label == "archive")
        .and_then(|e| log::head_of(std::slice::from_ref(&e.line)));
    if let Some(head) = newest {
        let dir = log::archive_dir(history_path);
        for info in &head.archives {
            let ok = fs::read(dir.join(&info.file)).is_ok_and(|b| sha256_hex(&b) == info.sha256);
            if !ok {
                c.archives.push(info.file.clone());
            }
        }
    }
    c.missing.sort();
    c.missing.dedup();
    Ok(c)
}

/// The user's lines the ledger lacks, oldest first: what to send.
pub fn pending_lines(ledger: Option<&Ledger>, history_path: &Path) -> Result<Vec<String>> {
    let have: HashSet<&str> = ledger
        .iter()
        .flat_map(|l| l.entries.iter().map(|e| e.sha.as_str()))
        .collect();
    let live = log::read_lines(history_path)?;
    let mut user = log::all_lines(history_path, true)?;
    if let Some(head) = live
        .first()
        .filter(|l| log::head_of(&[(*l).clone()]).is_some())
    {
        user.insert(0, head.clone());
    }
    Ok(user
        .into_iter()
        .filter(|l| accept(l).is_ok() && !have.contains(sha256_hex(l.as_bytes()).as_str()))
        .collect())
}

// ---- repairing -----------------------------------------------------------

/// What `repair` would write.
pub struct Rebuilt {
    pub lines: Vec<String>,
    /// Transactions in it.
    pub records: usize,
}

/// The user's log as the ledger remembers it. Records that an intact archive
/// file holds stay out of it (the newest archive marker says where they went).
pub fn rebuild(ledger: &Ledger, history_path: &Path) -> Rebuilt {
    let newest_head = ledger
        .entries
        .iter()
        .rev()
        .find(|e| e.label == "archive")
        .and_then(|e| log::head_of(std::slice::from_ref(&e.line)).map(|h| (e.line.clone(), h)));
    let mut archived: HashSet<String> = HashSet::new();
    let mut head_line = None;
    if let Some((line, head)) = newest_head {
        let dir = log::archive_dir(history_path);
        let mut intact = true;
        for info in &head.archives {
            match fs::read(dir.join(&info.file)) {
                Ok(b) if sha256_hex(&b) == info.sha256 => {
                    for l in String::from_utf8_lossy(&b)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                    {
                        archived.insert(sha256_hex(l.as_bytes()));
                    }
                }
                _ => intact = false,
            }
        }
        if intact {
            head_line = Some(line);
        } else {
            archived.clear();
        }
    }
    let mut lines: Vec<String> = head_line.into_iter().collect();
    lines.extend(
        ledger
            .entries
            .iter()
            .filter(|e| e.label != "archive" && !archived.contains(&e.sha))
            .map(|e| e.line.clone()),
    );
    let records = lines
        .iter()
        .filter(|l| serde_json::from_str::<TransactionRecord>(l).is_ok())
        .count();
    Rebuilt { lines, records }
}

/// Replaces the user's log with `rebuilt`, keeping the old file as
/// `history.jsonl.broken`.
pub fn write_rebuilt(history_path: &Path, rebuilt: &Rebuilt) -> Result<()> {
    let mut broken = history_path.as_os_str().to_owned();
    broken.push(".broken");
    if history_path.exists() {
        fs::rename(history_path, PathBuf::from(&broken)).map_err(|e| io_err(history_path, e))?;
    }
    if let Some(dir) = history_path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    }
    let mut text = rebuilt.lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    fs::write(history_path, text).map_err(|e| io_err(history_path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("emrac-ledger-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn ledger(&self) -> PathBuf {
            self.0.join("var/ledger.jsonl")
        }
        fn history(&self) -> PathBuf {
            self.0.join("data/history.jsonl")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rec(n: u64) -> Value {
        json!({"id": format!("tx-{n}"), "timestamp": n * 100, "action": "install",
               "packages": [], "status": "committed"})
    }

    fn user_log(s: &Scratch, count: u64) {
        for n in 1..=count {
            log::append(&s.history(), vec![rec(n)], n * 100).unwrap();
        }
    }

    fn all_pending(s: &Scratch) -> Vec<String> {
        pending_lines(read(&s.ledger()).unwrap().as_ref(), &s.history()).unwrap()
    }

    #[test]
    fn lsattr_output_says_whether_the_file_is_append_only() {
        assert_eq!(
            parse_lsattr("-----a--------e------- /var/lib/emrac/ledger.jsonl\n"),
            Some(true)
        );
        assert_eq!(
            parse_lsattr("--------------e------- /var/lib/aaa\n"),
            Some(false)
        );
        assert_eq!(parse_lsattr("---------------------- /tmp/x\n"), Some(false));
        assert_eq!(parse_lsattr(""), None);
        assert_eq!(parse_lsattr("lsattr: Inappropriate ioctl for device"), None);
    }

    #[test]
    fn the_helper_takes_records_and_markers_and_nothing_else() {
        for ok in [
            r#"{"id":"tx-3","timestamp":1,"action":"install","packages":[],"status":"failed"}"#,
            r#"{"type":"ack","ids":["tx-3"],"timestamp":1}"#,
            r#"{"type":"chain-start","timestamp":1,"earlier":2,"seq":1,"prev":"x"}"#,
            r#"{"type":"archive","timestamp":1}"#,
        ] {
            assert!(accept(ok).is_ok(), "{ok}");
        }
        for bad in [
            "not json",
            "[1,2]",
            r#"{"type":"shell","cmd":"rm -rf /"}"#,
            r#"{"type":"ack","ids":"tx-3"}"#,
            r#"{"id":"tx-3"}"#,
            r#"{"id":"../../etc/passwd","timestamp":1,"action":"install","packages":[],"status":"failed"}"#,
        ] {
            assert!(accept(bad).is_err(), "{bad}");
        }
        let huge = format!(r#"{{"type":"ack","ids":["{}"]}}"#, "x".repeat(MAX_LINE));
        assert!(accept(&huge).unwrap_err().contains("limit"));
    }

    #[test]
    fn appending_chains_dedupes_and_wraps_the_line_unchanged() {
        let s = Scratch::new("append");
        user_log(&s, 3);
        let lines = log::read_lines(&s.history()).unwrap();
        assert_eq!(append(&s.ledger(), &lines, 7, false).unwrap(), 3);
        // The same lines again, and a repeat inside one call: nothing new.
        assert_eq!(append(&s.ledger(), &lines, 8, false).unwrap(), 0);
        let mut twice = vec![lines[0].clone()];
        twice.extend(twice.clone());
        assert_eq!(append(&s.ledger(), &twice, 9, false).unwrap(), 0);

        let l = read(&s.ledger()).unwrap().unwrap();
        assert!(l.problem.is_none(), "{:?}", l.problem);
        assert_eq!(l.count, 3);
        assert_eq!(l.entries[1].line, lines[1], "byte for byte");
        assert_eq!(l.entries[2].label, "tx-3");
        assert_eq!(l.entries[0].at, 7);
        // More later continues the same chain.
        let more = vec![serde_json::to_string(&rec(4)).unwrap()];
        assert_eq!(append(&s.ledger(), &more, 10, false).unwrap(), 1);
        assert!(read(&s.ledger()).unwrap().unwrap().problem.is_none());
    }

    #[test]
    fn one_bad_line_means_nothing_is_written() {
        let s = Scratch::new("atomic");
        let good = serde_json::to_string(&rec(1)).unwrap();
        let r = append(&s.ledger(), &[good, "garbage".into()], 1, false);
        assert!(matches!(r, Err(Error::History(m)) if m.contains("refused")));
        assert!(!s.ledger().exists());
        let many: Vec<String> = (0..=MAX_BATCH).map(|_| "{}".to_string()).collect();
        assert!(append(&s.ledger(), &many, 1, false).is_err());
    }

    #[test]
    fn an_edited_removed_or_forged_ledger_line_is_caught() {
        let s = Scratch::new("tamper");
        user_log(&s, 4);
        append(
            &s.ledger(),
            &log::read_lines(&s.history()).unwrap(),
            1,
            false,
        )
        .unwrap();
        let original = fs::read_to_string(s.ledger()).unwrap();
        let lines: Vec<&str> = original.lines().collect();

        let edit = |text: String| {
            fs::write(s.ledger(), text).unwrap();
            read(&s.ledger()).unwrap().unwrap().problem
        };
        // Changing what a record says: its recorded hash no longer matches.
        let b = edit(original.replacen("tx-2", "tx-9", 1)).expect("an edit is caught");
        assert!(
            b.reason.contains("hash") || b.reason.contains("follow"),
            "{}",
            b.reason
        );
        // Removing a line.
        let mut l = lines.clone();
        l.remove(1);
        let b = edit(l.join("\n") + "\n").expect("a removal is caught");
        assert_eq!(b.line, 2);
        // Adding a line that is not from the helper.
        let mut l = lines.clone();
        l.insert(2, r#"{"hello":"world"}"#);
        assert!(edit(l.join("\n") + "\n").is_some());
        // Restored, it is fine again — and a broken ledger takes no more lines.
        assert!(edit(original.clone()).is_none());
        let mut l = lines.clone();
        l.remove(0);
        fs::write(s.ledger(), l.join("\n") + "\n").unwrap();
        let more = vec![serde_json::to_string(&rec(5)).unwrap()];
        assert!(matches!(
            append(&s.ledger(), &more, 2, false),
            Err(Error::History(m)) if m.contains("chain is broken")
        ));
    }

    #[test]
    fn comparing_names_missing_changed_and_pending_records() {
        let s = Scratch::new("compare");
        user_log(&s, 3);
        append(
            &s.ledger(),
            &log::read_lines(&s.history()).unwrap(),
            1,
            false,
        )
        .unwrap();
        let ledger = read(&s.ledger()).unwrap().unwrap();
        assert!(compare(&ledger, &s.history()).unwrap().is_clean());

        // A fourth record the ledger has not seen yet is only "pending".
        log::append(&s.history(), vec![rec(4)], 400).unwrap();
        let c = compare(&ledger, &s.history()).unwrap();
        assert!(c.is_clean());
        assert_eq!(c.pending, 1);
        assert_eq!(all_pending(&s).len(), 1);

        // Edit tx-2, delete tx-3.
        let text = fs::read_to_string(s.history()).unwrap();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[1] = lines[1].replace("install", "remove");
        lines.remove(2);
        fs::write(s.history(), lines.join("\n") + "\n").unwrap();
        let c = compare(&ledger, &s.history()).unwrap();
        assert_eq!(c.changed, ["tx-2"]);
        assert_eq!(c.missing, ["tx-3"]);
        assert!(!c.is_clean());
    }

    #[test]
    fn a_skipped_append_is_caught_up_in_order_by_the_next() {
        let s = Scratch::new("catchup");
        user_log(&s, 2);
        assert_eq!(all_pending(&s).len(), 2);
        append(&s.ledger(), &all_pending(&s), 1, false).unwrap();
        // Two transactions happen while sudo is unavailable.
        user_log_from(&s, 3, 5);
        let pending = all_pending(&s);
        assert_eq!(pending.len(), 3);
        assert!(pending[0].contains("tx-3") && pending[2].contains("tx-5"));
        append(&s.ledger(), &pending, 2, false).unwrap();
        assert!(all_pending(&s).is_empty());
        let l = read(&s.ledger()).unwrap().unwrap();
        assert!(l.problem.is_none());
        assert_eq!(
            l.entries
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            ["tx-1", "tx-2", "tx-3", "tx-4", "tx-5"]
        );
    }

    fn user_log_from(s: &Scratch, from: u64, to: u64) {
        for n in from..=to {
            log::append(&s.history(), vec![rec(n)], n * 100).unwrap();
        }
    }

    #[test]
    fn the_history_is_rebuilt_from_the_ledger_and_verifies() {
        let s = Scratch::new("rebuild");
        user_log(&s, 4);
        append(
            &s.ledger(),
            &log::read_lines(&s.history()).unwrap(),
            1,
            false,
        )
        .unwrap();
        let original = fs::read_to_string(s.history()).unwrap();
        fs::remove_file(s.history()).unwrap();

        let ledger = read(&s.ledger()).unwrap().unwrap();
        let rebuilt = rebuild(&ledger, &s.history());
        assert_eq!(rebuilt.records, 4);
        write_rebuilt(&s.history(), &rebuilt).unwrap();
        assert_eq!(fs::read_to_string(s.history()).unwrap(), original);
        assert!(log::verify(&s.history()).unwrap().problem.is_none());

        // A damaged file is kept aside.
        fs::write(s.history(), "ruined\n").unwrap();
        write_rebuilt(&s.history(), &rebuilt).unwrap();
        assert_eq!(
            fs::read_to_string(s.0.join("data/history.jsonl.broken")).unwrap(),
            "ruined\n"
        );
    }

    #[test]
    fn a_rebuild_leaves_archived_records_in_their_archive() {
        let s = Scratch::new("rebuild-archive");
        // 25 records, 5 archived (the newest 20 stay live).
        for n in 1..=25 {
            let mut r = rec(n);
            r["timestamp"] = (n * 100).into();
            log::append(&s.history(), vec![r], n * 100).unwrap();
        }
        log::archive(&s.history(), u64::MAX, 99_999)
            .unwrap()
            .unwrap();
        append(&s.ledger(), &all_pending(&s), 1, false).unwrap();
        let live_before = fs::read_to_string(s.history()).unwrap();
        fs::remove_file(s.history()).unwrap();

        let ledger = read(&s.ledger()).unwrap().unwrap();
        let rebuilt = rebuild(&ledger, &s.history());
        assert_eq!(rebuilt.records, 20, "the archived five stay in their file");
        write_rebuilt(&s.history(), &rebuilt).unwrap();
        assert_eq!(fs::read_to_string(s.history()).unwrap(), live_before);
        let r = log::verify(&s.history()).unwrap();
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert!(compare(&ledger, &s.history()).unwrap().is_clean());
    }

    #[test]
    fn the_ledger_notices_an_archive_file_that_changed() {
        let s = Scratch::new("archive-check");
        for n in 1..=25 {
            log::append(&s.history(), vec![rec(n)], n * 100).unwrap();
        }
        log::archive(&s.history(), u64::MAX, 99_999)
            .unwrap()
            .unwrap();
        append(&s.ledger(), &all_pending(&s), 1, false).unwrap();
        let ledger = read(&s.ledger()).unwrap().unwrap();
        assert!(compare(&ledger, &s.history()).unwrap().is_clean());

        let file = log::archive_dir(&s.history()).join("tx-1-to-tx-5.jsonl");
        let text = fs::read_to_string(&file).unwrap();
        fs::write(&file, text.replacen("install", "remove", 1)).unwrap();
        let c = compare(&ledger, &s.history()).unwrap();
        assert_eq!(c.archives, ["tx-1-to-tx-5.jsonl"]);
    }
}

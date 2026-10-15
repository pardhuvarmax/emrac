//! The mechanics under `history.rs`: a hash chain over the log's lines, the
//! archive that moves old lines out of the live log, and the acknowledgement
//! markers. See `docs/plan/u7-history-integrity.md`.
//!
//! Every line written by this version carries `seq` and `prev`, the SHA-256
//! of the previous line byte for byte. Lines from before the chain existed
//! ("legacy") have neither; the first line of the chain, a `chain-start`
//! marker, names how many of them there were and the hash of the last.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::history::TransactionRecord;

/// What `prev` holds on the very first line of a log.
pub(crate) const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The newest records an archive never touches: the cache policy and
/// `undo`/`redo` read this many.
pub const RECENT_KEPT: usize = 20;

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn io_read(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        context: format!("couldn't read your history at {}", path.display()),
        source,
    }
}

fn io_write(path: &Path, source: std::io::Error) -> Error {
    Error::HistoryWrite {
        path: path.display().to_string(),
        source,
    }
}

/// Non-empty lines, oldest first. A missing file is an empty log.
pub(crate) fn read_lines(path: &Path) -> Result<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(io_read(path, err)),
    }
}

fn parse(line: &str) -> Option<Value> {
    serde_json::from_str(line).ok()
}

fn kind(v: &Value) -> Option<&str> {
    v.get("type").and_then(Value::as_str)
}

fn is_head_marker(line: &str) -> bool {
    parse(line).is_some_and(|v| kind(&v) == Some("archive"))
}

/// The summary of one archive file, as the head marker lists it.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ArchiveInfo {
    pub file: String,
    pub sha256: String,
    pub lines: usize,
    pub records: usize,
    pub first_id: String,
    pub last_id: String,
    pub first_time: u64,
    pub last_time: u64,
}

/// The marker at the top of a live log that has had lines archived.
#[derive(Debug, Clone)]
pub(crate) struct Head {
    pub max_id: u64,
    /// `seq` of the last archived line; 0 if it was a legacy line.
    pub last_seq: u64,
    pub last_hash: String,
    pub legacy_moved: u64,
    pub archives: Vec<ArchiveInfo>,
}

impl Head {
    fn from_line(line: &str) -> Option<Head> {
        let v = parse(line)?;
        if kind(&v) != Some("archive") {
            return None;
        }
        Some(Head {
            max_id: v.get("max_id")?.as_u64()?,
            last_seq: v.get("last_seq")?.as_u64()?,
            last_hash: v.get("last_hash")?.as_str()?.to_string(),
            legacy_moved: v.get("legacy_moved").and_then(Value::as_u64).unwrap_or(0),
            archives: serde_json::from_value(v.get("archives")?.clone()).ok()?,
        })
    }

    fn to_line(&self, timestamp: u64) -> String {
        serde_json::to_string(&json!({
            "type": "archive",
            "timestamp": timestamp,
            "max_id": self.max_id,
            "last_seq": self.last_seq,
            "last_hash": self.last_hash,
            "legacy_moved": self.legacy_moved,
            "archives": self.archives,
        }))
        .expect("serializable")
    }
}

pub(crate) fn head_of(lines: &[String]) -> Option<Head> {
    lines.first().and_then(|l| Head::from_line(l))
}

pub(crate) fn archive_dir(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("history-archive")
}

// ---- appending ----------------------------------------------------------

struct Link {
    seq: u64,
    prev: String,
    /// A `chain-start` line to write first (a log from before the chain).
    preface: Option<Map<String, Value>>,
}

fn next_link(lines: &[String], timestamp: u64) -> Link {
    let Some(last) = lines.last() else {
        return Link {
            seq: 1,
            prev: GENESIS.into(),
            preface: None,
        };
    };
    let value = parse(last);
    if let Some(seq) = value
        .as_ref()
        .and_then(|v| v.get("seq"))
        .and_then(Value::as_u64)
    {
        return Link {
            seq: seq + 1,
            prev: sha256_hex(last.as_bytes()),
            preface: None,
        };
    }
    if let Some(head) = Head::from_line(last) {
        if head.last_seq > 0 {
            return Link {
                seq: head.last_seq + 1,
                prev: head.last_hash,
                preface: None,
            };
        }
        return chain_start(head.legacy_moved, head.last_hash, timestamp);
    }
    let moved = head_of(lines).map_or(0, |h| h.legacy_moved);
    let earlier = lines.iter().filter(|l| !is_head_marker(l)).count() as u64 + moved;
    chain_start(earlier, sha256_hex(last.as_bytes()), timestamp)
}

fn chain_start(earlier: u64, prev: String, timestamp: u64) -> Link {
    let mut m = Map::new();
    m.insert("type".into(), "chain-start".into());
    m.insert("timestamp".into(), timestamp.into());
    m.insert("earlier".into(), earlier.into());
    m.insert("seq".into(), 1.into());
    m.insert("prev".into(), prev.into());
    Link {
        seq: 1,
        prev: String::new(),
        preface: Some(m),
    }
}

/// Appends `values` (JSON objects) as chained lines in one write.
pub(crate) fn append(path: &Path, values: Vec<Value>, timestamp: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_write(path, e))?;
    }
    let lines = read_lines(path)?;
    let link = next_link(&lines, timestamp);
    let (mut seq, mut prev) = (link.seq, link.prev);
    let mut out = String::new();
    if let Some(start) = link.preface {
        let line = serde_json::to_string(&Value::Object(start)).expect("serializable");
        prev = sha256_hex(line.as_bytes());
        seq = 2;
        out.push_str(&line);
        out.push('\n');
    }
    for value in values {
        let Value::Object(mut map) = value else {
            unreachable!("history lines are objects")
        };
        map.insert("seq".into(), seq.into());
        map.insert("prev".into(), prev.clone().into());
        let line = serde_json::to_string(&Value::Object(map)).expect("serializable");
        prev = sha256_hex(line.as_bytes());
        seq += 1;
        out.push_str(&line);
        out.push('\n');
    }
    // One write_all in append mode: a crash can lose these lines but can't
    // damage earlier ones.
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(out.as_bytes()))
        .map_err(|e| io_write(path, e))
}

/// Starts the chain on a log that predates it, so that old lines can be
/// archived behind it. Does nothing when the log is already chained.
fn ensure_chain(path: &Path, timestamp: u64) -> Result<()> {
    let lines = read_lines(path)?;
    if let Some(start) = next_link(&lines, timestamp).preface {
        let mut line = serde_json::to_string(&Value::Object(start)).expect("serializable");
        line.push('\n');
        OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(|e| io_write(path, e))?;
    }
    Ok(())
}

// ---- verifying ----------------------------------------------------------

/// The first place the chain does not hold.
#[derive(Debug, Clone, Serialize)]
pub struct Break {
    /// The file the line is in (`history.jsonl` or an archive file name).
    pub file: String,
    /// 1-based; 0 when the problem is a whole file.
    pub line: usize,
    pub reason: String,
    /// The transaction id on the last line that was fine, and on this one.
    pub before: Option<String>,
    pub after: Option<String>,
}

impl Break {
    pub fn describe(&self) -> String {
        let place = if self.line == 0 {
            self.file.clone()
        } else {
            format!("{} line {}", self.file, self.line)
        };
        let between = match (&self.before, &self.after) {
            (Some(a), Some(b)) => format!(" (between {a} and {b})"),
            (Some(a), None) => format!(" (after {a})"),
            (None, Some(b)) => format!(" (at {b})"),
            (None, None) => String::new(),
        };
        format!("{place}: {}{between}", self.reason)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    /// Lines that carry the chain.
    pub chained: usize,
    /// Lines from before the chain existed.
    pub legacy: usize,
    /// Archive files checked.
    pub archives: usize,
    /// `seq` and hash of the newest line: write these down somewhere the log
    /// can't reach, and a truncation can be seen.
    pub head_seq: Option<u64>,
    pub head_hash: Option<String>,
    #[serde(rename = "break")]
    pub problem: Option<Break>,
}

enum State {
    Legacy { count: u64, last: Option<String> },
    Chained { seq: u64, prev: String },
}

struct Walker {
    state: State,
    chained: usize,
    legacy: usize,
    last_id: Option<String>,
    head: Option<(u64, String)>,
}

impl Walker {
    fn new() -> Self {
        Walker {
            state: State::Legacy {
                count: 0,
                last: None,
            },
            chained: 0,
            legacy: 0,
            last_id: None,
            head: None,
        }
    }

    fn feed(&mut self, file: &str, n: usize, line: &str) -> std::result::Result<(), Break> {
        let value = parse(line);
        let id = value
            .as_ref()
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let seq = value
            .as_ref()
            .and_then(|v| v.get("seq"))
            .and_then(Value::as_u64);
        let prev = value
            .as_ref()
            .and_then(|v| v.get("prev"))
            .and_then(Value::as_str);
        let hash = sha256_hex(line.as_bytes());

        let verdict: std::result::Result<(), String> = match &self.state {
            State::Legacy { .. } if seq.is_none() => Ok(()),
            State::Legacy { count, last } => {
                let expected = last.clone().unwrap_or_else(|| GENESIS.to_string());
                let is_start = value.as_ref().and_then(kind) == Some("chain-start");
                let earlier = value
                    .as_ref()
                    .and_then(|v| v.get("earlier"))
                    .and_then(Value::as_u64);
                if seq != Some(1) {
                    Err("the chain should start at sequence 1 here".into())
                } else if prev != Some(expected.as_str()) {
                    Err("does not follow the line before it — a line was edited, removed or added".into())
                } else if last.is_some() && (!is_start || earlier != Some(*count)) {
                    Err("lines from before the chain were added or removed".into())
                } else {
                    Ok(())
                }
            }
            State::Chained {
                seq: expected_seq,
                prev: expected_prev,
            } => match seq {
                None => Err("has no chain fields (was it written by hand?)".into()),
                Some(s) if s != *expected_seq => Err(format!(
                    "expected sequence {expected_seq}, found {s} — a line was removed, added or moved"
                )),
                Some(_) if prev != Some(expected_prev.as_str()) => Err(
                    "the line before it was edited (its hash is not the one recorded here)".into(),
                ),
                Some(_) => Ok(()),
            },
        };
        if let Err(reason) = verdict {
            return Err(Break {
                file: file.to_string(),
                line: n,
                reason,
                before: self.last_id.clone(),
                after: id,
            });
        }

        match (&mut self.state, seq) {
            (State::Legacy { count, last }, None) => {
                *count += 1;
                *last = Some(hash);
                self.legacy += 1;
            }
            (State::Legacy { .. }, Some(_)) => {
                self.state = State::Chained {
                    seq: 2,
                    prev: hash.clone(),
                };
                self.chained += 1;
                self.head = Some((1, hash));
            }
            (State::Chained { seq: next, prev }, Some(s)) => {
                *next = s + 1;
                *prev = hash.clone();
                self.chained += 1;
                self.head = Some((s, hash));
            }
            (State::Chained { .. }, None) => unreachable!("checked above"),
        }
        if id.is_some() {
            self.last_id = id;
        }
        Ok(())
    }
}

pub(crate) fn verify(path: &Path) -> Result<VerifyReport> {
    let lines = read_lines(path)?;
    let file_name = path.file_name().map_or_else(
        || "history.jsonl".into(),
        |f| f.to_string_lossy().into_owned(),
    );
    let mut report = VerifyReport {
        chained: 0,
        legacy: 0,
        archives: 0,
        head_seq: None,
        head_hash: None,
        problem: None,
    };

    let head = head_of(&lines);
    let rest: &[String] = if head.is_some() { &lines[1..] } else { &lines };
    let finish = |w: Walker, mut report: VerifyReport, problem: Option<Break>| {
        report.chained += w.chained;
        report.legacy += w.legacy;
        if let Some((seq, hash)) = w.head {
            report.head_seq = Some(seq);
            report.head_hash = Some(hash);
        }
        report.problem = problem;
        report
    };

    let mut walker = Walker::new();
    if let Some(head) = &head {
        let dir = archive_dir(path);
        let mut contents: Vec<(String, Vec<String>)> = Vec::new();
        for info in &head.archives {
            let problem = |reason: String| Break {
                file: info.file.clone(),
                line: 0,
                reason,
                before: Some(info.first_id.clone()),
                after: Some(info.last_id.clone()),
            };
            let bytes = match fs::read(dir.join(&info.file)) {
                Ok(b) => b,
                Err(_) => {
                    return Ok(finish(
                        walker,
                        report,
                        Some(problem(
                            "the archive file is missing or can't be read".into(),
                        )),
                    ))
                }
            };
            if sha256_hex(&bytes) != info.sha256 {
                return Ok(finish(
                    walker,
                    report,
                    Some(problem(
                        "the archive has changed since it was archived (its SHA-256 differs)"
                            .into(),
                    )),
                ));
            }
            report.archives += 1;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            contents.push((
                info.file.clone(),
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string)
                    .collect(),
            ));
        }
        for (file, lines) in &contents {
            for (i, line) in lines.iter().enumerate() {
                if let Err(b) = walker.feed(file, i + 1, line) {
                    return Ok(finish(walker, report, Some(b)));
                }
            }
        }
        let ends_where_said = match &walker.state {
            State::Chained { prev, .. } => *prev == head.last_hash,
            State::Legacy { last, .. } => last.as_deref() == Some(head.last_hash.as_str()),
        };
        if !ends_where_said {
            let b = Break {
                file: file_name.clone(),
                line: 1,
                reason: "the archives do not end where the log says they do".into(),
                before: walker.last_id.clone(),
                after: None,
            };
            return Ok(finish(walker, report, Some(b)));
        }
    }
    for (i, line) in rest.iter().enumerate() {
        let n = i + 1 + usize::from(head.is_some());
        if let Err(b) = walker.feed(&file_name, n, line) {
            return Ok(finish(walker, report, Some(b)));
        }
    }
    Ok(finish(walker, report, None))
}

// ---- anchors ------------------------------------------------------------

/// How the history stands against an anchor (a sequence number and the
/// start of the hash of that line, kept somewhere else).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum AnchorCheck {
    /// The line at that number still hashes to the anchor, and every line up to it chains.
    Matches,
    /// There is a line at that number but it is not the one that was anchored.
    Differs { found: String },
    /// The history stops before the anchor: lines were cut off the end.
    Shorter { head_seq: u64 },
    /// The chain breaks before the anchor, so it cannot vouch for anything after the break.
    Broken { at: Break },
}

pub(crate) fn check_anchor(path: &Path, seq: u64, hash_prefix: &str) -> Result<AnchorCheck> {
    let lines = all_lines(path, true)?;
    let mut walker = Walker::new();
    let file = path.file_name().map_or_else(
        || "history.jsonl".into(),
        |f| f.to_string_lossy().into_owned(),
    );
    for (i, line) in lines.iter().enumerate() {
        if let Err(b) = walker.feed(&file, i + 1, line) {
            return Ok(AnchorCheck::Broken { at: b });
        }
        if walker.head.as_ref().is_some_and(|(s, _)| *s == seq) {
            let hash = sha256_hex(line.as_bytes());
            return Ok(if hash.starts_with(hash_prefix) {
                AnchorCheck::Matches
            } else {
                AnchorCheck::Differs {
                    found: hash[..hash_prefix.len().min(hash.len())].to_string(),
                }
            });
        }
    }
    Ok(AnchorCheck::Shorter {
        head_seq: walker.head.map_or(0, |(s, _)| s),
    })
}

// ---- archiving ----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ArchivePlan {
    pub file: String,
    pub lines: usize,
    pub records: usize,
    pub first_id: String,
    pub last_id: String,
    pub first_time: u64,
    pub last_time: u64,
    /// Transactions that stay in the live log.
    pub kept_records: usize,
    #[serde(skip)]
    cut: usize,
}

fn tx(line: &str) -> Option<TransactionRecord> {
    serde_json::from_str(line).ok()
}

fn id_number(id: &str) -> u64 {
    id.strip_prefix("tx-")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// How many leading lines of `rest` (the log without its head marker) can
/// move: a prefix, each line older than `cutoff`, none of the newest
/// `RECENT_KEPT` transactions, and no transaction that a kept rollback
/// still names.
fn movable_prefix(rest: &[String], cutoff: u64) -> usize {
    let tx_positions: Vec<usize> = rest
        .iter()
        .enumerate()
        .filter(|(_, l)| tx(l).is_some())
        .map(|(i, _)| i)
        .collect();
    let protected_from = if tx_positions.len() > RECENT_KEPT {
        tx_positions[tx_positions.len() - RECENT_KEPT]
    } else {
        0
    };
    let mut cut = 0;
    for (i, line) in rest.iter().enumerate().take(protected_from) {
        let old = if let Some(r) = tx(line) {
            r.timestamp < cutoff
        } else {
            match parse(line) {
                Some(v) if matches!(kind(&v), Some("chain-start" | "ack")) => {
                    v.get("timestamp")
                        .and_then(Value::as_u64)
                        .unwrap_or(u64::MAX)
                        < cutoff
                }
                _ => false,
            }
        };
        if !old {
            break;
        }
        cut = i + 1;
    }
    // A kept rollback keeps the record it reversed, so `redo` still finds it.
    loop {
        let lowered = rest[cut..]
            .iter()
            .filter_map(|l| tx(l)?.rollback_of)
            .filter_map(|target| {
                rest[..cut]
                    .iter()
                    .position(|l| tx(l).is_some_and(|r| r.id == target))
            })
            .min();
        match lowered {
            Some(i) => cut = i,
            None => return cut,
        }
    }
}

pub(crate) fn plan_archive(path: &Path, cutoff: u64) -> Result<Option<ArchivePlan>> {
    let lines = read_lines(path)?;
    let rest: &[String] = if head_of(&lines).is_some() {
        &lines[1..]
    } else {
        &lines
    };
    let cut = movable_prefix(rest, cutoff);
    let moved: Vec<TransactionRecord> = rest[..cut].iter().filter_map(|l| tx(l)).collect();
    let (Some(first), Some(last)) = (moved.first(), moved.last()) else {
        return Ok(None);
    };
    Ok(Some(ArchivePlan {
        file: format!("{}-to-{}.jsonl", first.id, last.id),
        lines: cut,
        records: moved.len(),
        first_id: first.id.clone(),
        last_id: last.id.clone(),
        first_time: first.timestamp,
        last_time: last.timestamp,
        kept_records: rest[cut..].iter().filter(|l| tx(l).is_some()).count(),
        cut,
    }))
}

pub(crate) fn archive(path: &Path, cutoff: u64, now: u64) -> Result<Option<ArchivePlan>> {
    ensure_chain(path, now)?;
    let Some(plan) = plan_archive(path, cutoff)? else {
        return Ok(None);
    };
    let lines = read_lines(path)?;
    let old_head = head_of(&lines);
    let rest: &[String] = if old_head.is_some() {
        &lines[1..]
    } else {
        &lines
    };
    let (moved, kept) = rest.split_at(plan.cut);

    let mut content = moved.join("\n");
    content.push('\n');
    let sha = sha256_hex(content.as_bytes());
    let dir = archive_dir(path);
    let target = dir.join(&plan.file);
    fs::create_dir_all(&dir).map_err(|e| io_write(&dir, e))?;
    if let Ok(existing) = fs::read(&target) {
        // An earlier run stopped after writing the archive: the same bytes are fine.
        if sha256_hex(&existing) != sha {
            return Err(Error::History(format!(
                "{} already exists and holds something else; move it away and run this again",
                target.display()
            )));
        }
    } else {
        write_synced(&target, content.as_bytes())?;
        let back = fs::read(&target).map_err(|e| io_read(&target, e))?;
        if sha256_hex(&back) != sha {
            return Err(Error::History(format!(
                "{} did not read back the same as it was written; the history was left as it was",
                target.display()
            )));
        }
    }

    let mut head = old_head.unwrap_or(Head {
        max_id: 0,
        last_seq: 0,
        last_hash: String::new(),
        legacy_moved: 0,
        archives: Vec::new(),
    });
    let last_line = moved.last().expect("a plan moves at least one line");
    head.max_id = head.max_id.max(
        moved
            .iter()
            .filter_map(|l| tx(l))
            .map(|r| id_number(&r.id))
            .max()
            .unwrap_or(0),
    );
    head.legacy_moved += moved
        .iter()
        .filter(|l| parse(l).and_then(|v| v.get("seq").cloned()).is_none())
        .count() as u64;
    head.last_seq = parse(last_line)
        .and_then(|v| v.get("seq").and_then(Value::as_u64))
        .unwrap_or(0);
    head.last_hash = sha256_hex(last_line.as_bytes());
    head.archives.push(ArchiveInfo {
        file: plan.file.clone(),
        sha256: sha,
        lines: plan.lines,
        records: plan.records,
        first_id: plan.first_id.clone(),
        last_id: plan.last_id.clone(),
        first_time: plan.first_time,
        last_time: plan.last_time,
    });

    let mut live = head.to_line(now);
    live.push('\n');
    for line in kept {
        live.push_str(line);
        live.push('\n');
    }
    // A rename, so a crash leaves the old log or the new one, never half.
    write_synced(path, live.as_bytes())?;
    Ok(Some(plan))
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = File::create(&tmp).map_err(|e| io_write(&tmp, e))?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| io_write(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_write(path, e))
}

// ---- reading everything -------------------------------------------------

/// The lines of the archive files (oldest first) and then of the live log,
/// without the head marker.
pub(crate) fn all_lines(path: &Path, with_archive: bool) -> Result<Vec<String>> {
    let live = read_lines(path)?;
    let head = head_of(&live);
    let mut out = Vec::new();
    if let (true, Some(head)) = (with_archive, &head) {
        let dir = archive_dir(path);
        for info in &head.archives {
            let file = dir.join(&info.file);
            out.extend(read_lines(&file)?);
        }
    }
    out.extend(live.into_iter().skip(usize::from(head.is_some())));
    Ok(out)
}

pub(crate) fn ack_ids(lines: &[String]) -> std::collections::HashSet<String> {
    lines
        .iter()
        .filter_map(|l| parse(l))
        .filter(|v| kind(v) == Some("ack"))
        .filter_map(|v| v.get("ids").cloned())
        .filter_map(|ids| serde_json::from_value::<Vec<String>>(ids).ok())
        .flatten()
        .collect()
}

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::history_log::{self as log, ArchivePlan};
pub use crate::history_log::{ArchiveInfo, Break, VerifyReport, RECENT_KEPT};
use crate::plan::{Plan, PlanAction, PlannedPackage, PlannedRemoval};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxStatus {
    Committed,
    Failed,
}

/// One executed transaction. Only transactions that actually ran are
/// recorded — dry runs, declined confirmations and empty plans are not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub id: String,
    /// Unix seconds, UTC.
    pub timestamp: u64,
    pub action: PlanAction,
    pub packages: Vec<PlannedPackage>,
    /// Packages the transaction additionally removed to resolve conflicts or
    /// replacements (`emrac undo` cannot restore these).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<PlannedRemoval>,
    pub status: TxStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Set on a record written by `rollback`/`undo`: the id of the
    /// transaction it reversed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of: Option<String>,
}

impl TransactionRecord {
    /// `YYYY-MM-DD HH:MM` in UTC.
    pub fn timestamp_utc(&self) -> String {
        format_utc(self.timestamp)
    }
}

/// Append-only transaction log at `~/.local/share/emrac/history.jsonl`.
pub struct History;

impl History {
    pub fn path() -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoDataDir)?;
        Ok(PathBuf::from(home).join(".local/share/emrac/history.jsonl"))
    }

    /// Records the outcome of an executed plan. `outcome` is the
    /// transaction's own result; it is only inspected, never consumed, so
    /// callers can return it unchanged afterwards.
    pub fn record(
        plan: &Plan,
        outcome: &Result<()>,
        rollback_of: Option<&str>,
    ) -> Result<TransactionRecord> {
        Self::record_to(&Self::path()?, plan, outcome, rollback_of, now())
    }

    /// Every record in the live log, oldest first.
    pub fn all() -> Result<Vec<TransactionRecord>> {
        read_from(&Self::path()?)
    }

    /// Records that were archived, then the live ones; oldest first.
    pub fn all_with_archive() -> Result<Vec<TransactionRecord>> {
        Self::all_with_archive_from(&Self::path()?)
    }

    /// The archive files the live log lists.
    pub fn archives() -> Result<Vec<ArchiveInfo>> {
        Ok(log::head_of(&log::read_lines(&Self::path()?)?)
            .map(|h| h.archives)
            .unwrap_or_default())
    }

    /// Ids of failed transactions the user has acknowledged.
    pub fn acknowledged(with_archive: bool) -> Result<HashSet<String>> {
        Ok(log::ack_ids(&log::all_lines(&Self::path()?, with_archive)?))
    }

    /// Acknowledges failed transactions: appends one marker, and the records
    /// stay. Returns the ids newly acknowledged. `ids` empty means every
    /// failed transaction not yet acknowledged.
    pub fn acknowledge(ids: &[String]) -> Result<Vec<String>> {
        Self::acknowledge_at(&Self::path()?, ids, now())
    }

    /// Acknowledges ledger-watch events by id (`ev-…`): one marker, chained
    /// like the failed-transaction one. Returns the ids newly acknowledged.
    pub fn acknowledge_events(ids: &[String]) -> Result<Vec<String>> {
        Self::acknowledge_events_at(&Self::path()?, ids, now())
    }

    fn acknowledge_events_at(path: &Path, ids: &[String], ts: u64) -> Result<Vec<String>> {
        let already = log::ack_ids(&log::all_lines(path, true)?);
        let mut fresh: Vec<String> = Vec::new();
        for id in ids {
            if !already.contains(id) && !fresh.contains(id) {
                fresh.push(id.clone());
            }
        }
        if fresh.is_empty() {
            return Ok(fresh);
        }
        let marker = serde_json::json!({"type": "ack", "ids": fresh, "timestamp": ts});
        log::append(path, vec![marker], ts)?;
        Ok(fresh)
    }

    /// The anchor code for the newest line: `<seq>:<first 32 hex of its hash>`.
    /// `None` while nothing is chained yet.
    pub fn anchor() -> Result<Option<String>> {
        let report = log::verify(&Self::path()?)?;
        Ok(report
            .head_seq
            .zip(report.head_hash)
            .map(|(seq, hash)| format!("{seq}:{}", &hash[..32])))
    }

    /// Checks the history against an anchor code taken earlier.
    pub fn check_anchor(code: &str) -> Result<log::AnchorCheck> {
        let bad = || {
            Error::History(format!(
                "'{code}' is not an anchor code — they look like 42:0123456789abcdef0123456789abcdef (from `emrac history anchor`)"
            ))
        };
        let (seq, hash) = code.trim().split_once(':').ok_or_else(bad)?;
        let seq: u64 = seq.parse().map_err(|_| bad())?;
        if seq == 0
            || hash.len() < 16
            || hash.len() > 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(bad());
        }
        log::check_anchor(&Self::path()?, seq, &hash.to_ascii_lowercase())
    }

    /// Checks the hash chain over the live log and its archives.
    pub fn verify() -> Result<VerifyReport> {
        log::verify(&Self::path()?)
    }

    /// What `archive` would move: records older than `older_than` seconds.
    pub fn archive_plan(older_than: u64) -> Result<Option<ArchivePlan>> {
        log::plan_archive(&Self::path()?, now().saturating_sub(older_than))
    }

    /// Moves them into `history-archive/` and leaves a marker behind.
    pub fn archive(older_than: u64) -> Result<Option<ArchivePlan>> {
        let now = now();
        log::archive(&Self::path()?, now.saturating_sub(older_than), now)
    }

    fn all_with_archive_from(path: &Path) -> Result<Vec<TransactionRecord>> {
        Ok(log::all_lines(path, true)?
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    fn acknowledge_at(path: &Path, ids: &[String], timestamp: u64) -> Result<Vec<String>> {
        let records = read_from(path)?;
        let already = log::ack_ids(&log::all_lines(path, false)?);
        let wanted: Vec<String> = if ids.is_empty() {
            records
                .iter()
                .filter(|r| r.status == TxStatus::Failed)
                .map(|r| r.id.clone())
                .collect()
        } else {
            for id in ids {
                let record = records
                    .iter()
                    .find(|r| &r.id == id)
                    .ok_or_else(|| Error::UnknownTransaction(id.clone()))?;
                if record.status != TxStatus::Failed {
                    return Err(Error::History(format!(
                        "{id} did not fail, so there is nothing to acknowledge"
                    )));
                }
            }
            ids.to_vec()
        };
        let mut fresh: Vec<String> = wanted
            .into_iter()
            .filter(|id| !already.contains(id))
            .collect();
        fresh.dedup();
        if fresh.is_empty() {
            return Ok(fresh);
        }
        let marker = serde_json::json!({"type": "ack", "ids": fresh, "timestamp": timestamp});
        log::append(path, vec![marker], timestamp)?;
        Ok(fresh)
    }

    /// Newest first. `since` is a unix timestamp lower bound (inclusive).
    pub fn list(limit: Option<usize>, since: Option<u64>) -> Result<Vec<TransactionRecord>> {
        Self::list_from(&Self::path()?, limit, since)
    }

    fn record_to(
        path: &Path,
        plan: &Plan,
        outcome: &Result<()>,
        rollback_of: Option<&str>,
        timestamp: u64,
    ) -> Result<TransactionRecord> {
        let live = log::read_lines(path)?;
        let archived_max = log::head_of(&live).map_or(0, |h| h.max_id);
        let next = read_from(path)?
            .iter()
            .filter_map(|r| r.id.strip_prefix("tx-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .max(archived_max)
            + 1;
        let record = TransactionRecord {
            id: format!("tx-{next}"),
            timestamp,
            action: plan.action,
            packages: plan.packages.clone(),
            removed: plan.removals.clone(),
            status: if outcome.is_ok() {
                TxStatus::Committed
            } else {
                TxStatus::Failed
            },
            error: outcome.as_ref().err().map(|e| e.to_string()),
            rollback_of: rollback_of.map(str::to_string),
        };
        let value = serde_json::to_value(&record).expect("TransactionRecord is serializable");
        log::append(path, vec![value], timestamp)?;
        Ok(record)
    }

    /// Like `list`, reading the archive files too.
    pub fn list_with_archive(
        limit: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<TransactionRecord>> {
        Self::filter(Self::all_with_archive()?, limit, since)
    }

    fn list_from(
        path: &Path,
        limit: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<TransactionRecord>> {
        Self::filter(read_from(path)?, limit, since)
    }

    fn filter(
        mut records: Vec<TransactionRecord>,
        limit: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<TransactionRecord>> {
        if let Some(since) = since {
            records.retain(|r| r.timestamp >= since);
        }
        records.reverse();
        if let Some(limit) = limit {
            records.truncate(limit);
        }
        Ok(records)
    }
}

/// Oldest first. A missing file is an empty history; lines that don't parse
/// are skipped so one bad line can't make the whole history unreadable.
fn read_from(path: &Path) -> Result<Vec<TransactionRecord>> {
    Ok(log::read_lines(path)?
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses `YYYY-MM-DD` as midnight UTC, returned as a unix timestamp.
pub fn parse_date(input: &str) -> Result<u64> {
    let invalid = || Error::InvalidDate(input.to_string());
    let mut parts = input.split('-');
    let (y, m, d) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), None) => (y, m, d),
        _ => return Err(invalid()),
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return Err(invalid());
    }
    let y: i64 = y.parse().map_err(|_| invalid())?;
    let m: i64 = m.parse().map_err(|_| invalid())?;
    let d: i64 = d.parse().map_err(|_| invalid())?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(invalid());
    }
    let days = days_from_civil(y, m, d);
    // Rejects dates like 2026-02-30 that would silently roll over.
    if civil_from_days(days) != (y, m, d) {
        return Err(invalid());
    }
    u64::try_from(days * 86_400).map_err(|_| invalid())
}

/// `YYYY-MM-DD` in UTC.
pub fn format_utc_date(timestamp: u64) -> String {
    format_utc(timestamp)[..10].to_string()
}

pub(crate) fn format_utc(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let secs = timestamp % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}

// Howard Hinnant's civil-date algorithms (days since 1970-01-01).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                env::temp_dir().join(format!("emrac-history-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("data/emrac/history.jsonl")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn plan(action: PlanAction, names: &[&str]) -> Plan {
        Plan {
            action,
            packages: names
                .iter()
                .map(|n| PlannedPackage {
                    name: n.to_string(),
                    version: "1.0-1".into(),
                    repo: "extra".into(),
                    explicit: true,
                    previous_version: None,
                })
                .collect(),
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: Vec::new(),
            warnings: Vec::new(),
            provider_choices: Vec::new(),
        }
    }

    fn record(
        path: &Path,
        plan: &Plan,
        outcome: &Result<()>,
        timestamp: u64,
    ) -> Result<TransactionRecord> {
        History::record_to(path, plan, outcome, None, timestamp)
    }

    fn failure() -> Result<()> {
        Err(Error::Aur("boom".into()))
    }

    #[test]
    fn missing_file_is_empty_history_and_is_not_created() {
        let s = Scratch::new("missing");
        assert!(History::list_from(&s.file(), None, None)
            .unwrap()
            .is_empty());
        assert!(!s.file().exists());
    }

    #[test]
    fn record_creates_dirs_and_roundtrips() {
        let s = Scratch::new("roundtrip");
        let rec = record(
            &s.file(),
            &plan(PlanAction::Install, &["a", "b"]),
            &Ok(()),
            100,
        )
        .unwrap();
        assert_eq!(rec.id, "tx-1");
        assert_eq!(rec.status, TxStatus::Committed);
        assert!(rec.error.is_none());

        let listed = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "tx-1");
        assert_eq!(listed[0].action, PlanAction::Install);
        assert_eq!(listed[0].timestamp, 100);
        assert_eq!(listed[0].packages.len(), 2);
        assert_eq!(listed[0].packages[0].name, "a");
    }

    #[test]
    fn failed_outcome_is_recorded_with_error_text() {
        let s = Scratch::new("failed");
        let rec = record(&s.file(), &plan(PlanAction::Upgrade, &["a"]), &failure(), 5).unwrap();
        assert_eq!(rec.status, TxStatus::Failed);
        assert!(rec.error.unwrap().contains("boom"));
    }

    #[test]
    fn ids_increment_and_list_is_newest_first_with_limit() {
        let s = Scratch::new("order");
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            record(
                &s.file(),
                &plan(PlanAction::Install, &[name]),
                &Ok(()),
                i as u64,
            )
            .unwrap();
        }
        let all = History::list_from(&s.file(), None, None).unwrap();
        let ids: Vec<_> = all.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["tx-3", "tx-2", "tx-1"]);

        let limited = History::list_from(&s.file(), Some(2), None).unwrap();
        let ids: Vec<_> = limited.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["tx-3", "tx-2"]);
    }

    #[test]
    fn since_is_an_inclusive_lower_bound_applied_before_limit() {
        let s = Scratch::new("since");
        for t in [10, 20, 30] {
            record(&s.file(), &plan(PlanAction::Remove, &["a"]), &Ok(()), t).unwrap();
        }
        let got = History::list_from(&s.file(), None, Some(20)).unwrap();
        assert_eq!(
            got.iter().map(|r| r.timestamp).collect::<Vec<_>>(),
            [30, 20]
        );
        let got = History::list_from(&s.file(), Some(1), Some(20)).unwrap();
        assert_eq!(got.iter().map(|r| r.timestamp).collect::<Vec<_>>(), [30]);
    }

    #[test]
    fn corrupt_lines_are_skipped_and_ids_continue_past_them() {
        let s = Scratch::new("corrupt");
        record(&s.file(), &plan(PlanAction::Install, &["a"]), &Ok(()), 1).unwrap();
        let mut f = OpenOptions::new().append(true).open(s.file()).unwrap();
        f.write_all(b"not json\n{\"id\":\"tx-9\"}\n\n").unwrap();
        drop(f);

        let listed = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(listed.len(), 1);
        let rec = record(&s.file(), &plan(PlanAction::Install, &["b"]), &Ok(()), 2).unwrap();
        assert_eq!(rec.id, "tx-2");
    }

    #[test]
    fn unreadable_history_is_an_io_error_not_empty() {
        let s = Scratch::new("isdir");
        assert!(matches!(
            History::list_from(&s.0, None, None),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn write_failure_is_reported() {
        use std::os::unix::fs::PermissionsExt;

        let s = Scratch::new("readonly");
        let locked = s.0.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let result = record(
            &locked.join("sub/history.jsonl"),
            &plan(PlanAction::Install, &["a"]),
            &Ok(()),
            1,
        );
        // Restore so Scratch's drop can clean up.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // (Running as root bypasses the permission bit; nothing to assert then.)
        if let Err(err) = result {
            assert!(matches!(err, Error::HistoryWrite { .. }));
        }
    }

    #[test]
    fn dates_convert_against_known_values() {
        assert_eq!(format_utc(0), "1970-01-01 00:00");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09 01:46");
        assert_eq!(format_utc(1_781_136_000), "2026-06-11 00:00");
        assert_eq!(parse_date("1970-01-01").unwrap(), 0);
        assert_eq!(parse_date("2001-09-09").unwrap(), 999_993_600);
        assert_eq!(parse_date("2024-02-29").unwrap(), 1_709_164_800);
        for ts in [0u64, 86_399, 951_782_400, 1_709_164_800, 4_102_444_800] {
            let day = ts / 86_400 * 86_400;
            assert_eq!(parse_date(&format_utc(ts)[..10]).unwrap(), day);
        }
    }

    #[test]
    fn bad_dates_are_rejected() {
        for bad in [
            "",
            "2026",
            "2026-1-1",
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2025-02-29",
            "2026-04-31",
            "26-01-01",
            "2026-01-01-01",
            "abcd-ef-gh",
            "2026/01/01",
            "-2026-01-01",
        ] {
            assert!(
                matches!(parse_date(bad), Err(Error::InvalidDate(_))),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn rollback_of_is_persisted_and_old_lines_still_parse() {
        let s = Scratch::new("rollback_of");
        let p = plan(PlanAction::Remove, &["a"]);
        History::record_to(&s.file(), &p, &Ok(()), Some("tx-1"), 7).unwrap();
        let got = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(got[0].rollback_of.as_deref(), Some("tx-1"));

        // A line in the u3 format: no `rollback_of`, no `previous_version`.
        let old = r#"{"id":"tx-5","timestamp":1,"action":"upgrade","packages":[{"name":"a","version":"2","repo":"extra","explicit":true}],"status":"committed"}"#;
        fs::write(s.file(), format!("{old}\n")).unwrap();
        let got = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].rollback_of.is_none());
        assert!(got[0].packages[0].previous_version.is_none());
    }

    // ---- the chain, the archive and acknowledgements (u7) ----------------

    fn fill(s: &Scratch, n: u64) {
        for t in 1..=n {
            record(
                &s.file(),
                &plan(PlanAction::Install, &["a"]),
                &Ok(()),
                t * 1000,
            )
            .unwrap();
        }
    }

    fn lines(s: &Scratch) -> Vec<String> {
        fs::read_to_string(s.file())
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn put(s: &Scratch, lines: &[String]) {
        fs::write(s.file(), lines.join("\n") + "\n").unwrap();
    }

    fn verdict(s: &Scratch) -> VerifyReport {
        log::verify(&s.file()).unwrap()
    }

    #[test]
    fn every_record_carries_a_chain_and_it_verifies() {
        let s = Scratch::new("chain-ok");
        fill(&s, 4);
        let r = verdict(&s);
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert_eq!((r.chained, r.legacy), (4, 0));
        assert_eq!(r.head_seq, Some(4));
        let first: serde_json::Value = serde_json::from_str(&lines(&s)[0]).unwrap();
        assert_eq!(first["seq"], 1);
        assert_eq!(first["prev"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn an_edited_line_breaks_the_chain_at_the_next_one() {
        let s = Scratch::new("chain-edit");
        fill(&s, 4);
        let mut l = lines(&s);
        l[1] = l[1].replace("\"install\"", "\"remove\"");
        assert_ne!(l[1], lines(&s)[1]);
        put(&s, &l);
        let b = verdict(&s).problem.expect("an edit is caught");
        assert_eq!(b.line, 3);
        assert_eq!(b.before.as_deref(), Some("tx-2"));
        assert_eq!(b.after.as_deref(), Some("tx-3"));
        assert!(b.reason.contains("edited"), "{}", b.reason);
    }

    #[test]
    fn a_deleted_inserted_or_reordered_line_breaks_the_chain() {
        let s = Scratch::new("chain-shape");
        fill(&s, 5);
        let original = lines(&s);

        let mut l = original.clone();
        l.remove(2);
        put(&s, &l);
        let b = verdict(&s).problem.expect("a deletion is caught");
        assert_eq!((b.line, b.before.as_deref()), (3, Some("tx-2")));

        let mut l = original.clone();
        l.insert(2, original[1].clone());
        put(&s, &l);
        assert!(verdict(&s).problem.is_some(), "a duplicated line is caught");

        let mut l = original.clone();
        l.swap(2, 3);
        put(&s, &l);
        assert!(verdict(&s).problem.is_some(), "a reorder is caught");

        let mut l = original.clone();
        l.insert(3, "{\"id\":\"tx-99\"}".to_string());
        put(&s, &l);
        let b = verdict(&s).problem.expect("a forged line is caught");
        assert!(b.reason.contains("no chain fields"), "{}", b.reason);
    }

    #[test]
    fn a_cut_off_tail_cannot_be_seen_locally_but_the_head_shows_it() {
        let s = Scratch::new("chain-tail");
        fill(&s, 4);
        let before = verdict(&s);
        let mut l = lines(&s);
        l.truncate(2);
        put(&s, &l);
        let after = verdict(&s);
        assert!(after.problem.is_none(), "nothing local can know");
        assert_ne!(after.head_hash, before.head_hash);
        assert_eq!(after.head_seq, Some(2));
    }

    #[test]
    fn a_log_from_before_the_chain_is_joined_by_a_chain_start() {
        let s = Scratch::new("chain-legacy");
        fill(&s, 3);
        // What an older emrac wrote: no seq, no prev.
        let legacy: Vec<String> = lines(&s)
            .iter()
            .map(|l| {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                v.as_object_mut().unwrap().remove("seq");
                v.as_object_mut().unwrap().remove("prev");
                serde_json::to_string(&v).unwrap()
            })
            .collect();
        put(&s, &legacy);
        assert!(verdict(&s).problem.is_none());
        assert_eq!(verdict(&s).legacy, 3);

        let rec = record(&s.file(), &plan(PlanAction::Remove, &["b"]), &Ok(()), 9).unwrap();
        assert_eq!(rec.id, "tx-4");
        let l = lines(&s);
        assert_eq!(l.len(), 5);
        let start: serde_json::Value = serde_json::from_str(&l[3]).unwrap();
        assert_eq!(start["type"], "chain-start");
        assert_eq!(start["earlier"], 3);
        let r = verdict(&s);
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert_eq!((r.legacy, r.chained), (3, 2));
        // Old emrac versions and the history commands still read it.
        assert_eq!(History::list_from(&s.file(), None, None).unwrap().len(), 4);

        // The last line from before the chain is covered by it.
        let mut l = lines(&s);
        l[2] = l[2].replace("\"install\"", "\"remove\"");
        put(&s, &l);
        assert!(verdict(&s).problem.is_some());
    }

    #[test]
    fn the_archive_moves_only_old_records_and_never_the_newest_twenty() {
        let s = Scratch::new("archive-window");
        fill(&s, 30);
        // Everything is "old" (cutoff far in the future) but 20 stay.
        let plan = log::plan_archive(&s.file(), u64::MAX).unwrap().unwrap();
        assert_eq!((plan.records, plan.kept_records), (10, 20));
        assert_eq!(
            (plan.first_id.as_str(), plan.last_id.as_str()),
            ("tx-1", "tx-10")
        );

        // A window that only covers 4 records moves 4.
        let plan = log::plan_archive(&s.file(), 4001).unwrap().unwrap();
        assert_eq!(plan.records, 4);
        // Fewer than 21 records: nothing moves, however old.
        let few = Scratch::new("archive-few");
        fill(&few, 20);
        assert!(log::plan_archive(&few.file(), u64::MAX).unwrap().is_none());
    }

    #[test]
    fn archiving_keeps_the_chain_ids_and_the_live_records() {
        let s = Scratch::new("archive-run");
        fill(&s, 30);
        let moved = log::archive(&s.file(), u64::MAX, 99_999).unwrap().unwrap();
        assert_eq!(moved.records, 10);

        let live = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(live.len(), 20);
        assert_eq!(live.last().unwrap().id, "tx-11");
        assert_eq!(
            log::head_of(&log::read_lines(&s.file()).unwrap())
                .unwrap()
                .max_id,
            10
        );
        let archived = fs::read_to_string(
            s.file()
                .parent()
                .unwrap()
                .join("history-archive/tx-1-to-tx-10.jsonl"),
        )
        .unwrap();
        assert_eq!(archived.lines().count(), 10);

        let r = verdict(&s);
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert_eq!((r.archives, r.chained), (1, 30));

        // Ids keep counting, and appending after the marker keeps the chain.
        let rec = record(&s.file(), &plan(PlanAction::Install, &["z"]), &Ok(()), 5).unwrap();
        assert_eq!(rec.id, "tx-31");
        assert!(verdict(&s).problem.is_none());

        // The new record pushed one more out of the newest twenty.
        let more = log::archive(&s.file(), u64::MAX, 100_000).unwrap().unwrap();
        assert_eq!(more.last_id, "tx-11");
        assert!(verdict(&s).problem.is_none());
        // Now nothing is left to move: another run is a no-op.
        assert!(log::archive(&s.file(), u64::MAX, 100_001)
            .unwrap()
            .is_none());
        // Everything is still readable.
        assert_eq!(History::all_with_archive_from(&s.file()).unwrap().len(), 31);
    }

    #[test]
    fn a_second_archive_extends_the_first_and_both_verify() {
        let s = Scratch::new("archive-twice");
        fill(&s, 30);
        log::archive(&s.file(), u64::MAX, 1).unwrap();
        fill_more(&s, 30, 10);
        let moved = log::archive(&s.file(), u64::MAX, 2).unwrap().unwrap();
        assert_eq!(
            (moved.first_id.as_str(), moved.last_id.as_str()),
            ("tx-11", "tx-20")
        );
        let r = verdict(&s);
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert_eq!(r.archives, 2);
        assert_eq!(History::all_with_archive_from(&s.file()).unwrap().len(), 40);
    }

    fn fill_more(s: &Scratch, after: u64, n: u64) {
        for t in 1..=n {
            record(
                &s.file(),
                &plan(PlanAction::Install, &["a"]),
                &Ok(()),
                (after + t) * 1000,
            )
            .unwrap();
        }
    }

    #[test]
    fn a_changed_or_missing_archive_is_reported() {
        let s = Scratch::new("archive-tamper");
        fill(&s, 30);
        log::archive(&s.file(), u64::MAX, 1).unwrap();
        let file = s
            .file()
            .parent()
            .unwrap()
            .join("history-archive/tx-1-to-tx-10.jsonl");

        let original = fs::read_to_string(&file).unwrap();
        fs::write(&file, original.replacen("install", "remove", 1)).unwrap();
        let b = verdict(&s).problem.expect("an edited archive is caught");
        assert!(b.reason.contains("changed"), "{}", b.reason);

        fs::remove_file(&file).unwrap();
        let b = verdict(&s).problem.expect("a missing archive is caught");
        assert!(b.reason.contains("missing"), "{}", b.reason);
    }

    #[test]
    fn a_record_a_kept_rollback_names_stays_in_the_live_log() {
        let s = Scratch::new("archive-rollback");
        fill(&s, 25);
        // tx-26 reverses tx-2, which would otherwise be archived.
        History::record_to(
            &s.file(),
            &plan(PlanAction::Remove, &["a"]),
            &Ok(()),
            Some("tx-2"),
            26_000,
        )
        .unwrap();
        let plan = log::plan_archive(&s.file(), u64::MAX).unwrap();
        // tx-1 alone may move; tx-2 and everything after it stays.
        let plan = plan.expect("tx-1 can still move");
        assert_eq!((plan.records, plan.last_id.as_str()), (1, "tx-1"));
    }

    #[test]
    fn archiving_a_log_from_before_the_chain_starts_the_chain_first() {
        let s = Scratch::new("archive-legacy");
        let legacy: Vec<String> = (1..=25)
            .map(|n| {
                format!(
                    "{{\"id\":\"tx-{n}\",\"timestamp\":{},\"action\":\"install\",\"packages\":[],\"status\":\"committed\"}}",
                    n * 1000
                )
            })
            .collect();
        fs::create_dir_all(s.file().parent().unwrap()).unwrap();
        put(&s, &legacy);
        let moved = log::archive(&s.file(), u64::MAX, 50_000).unwrap().unwrap();
        assert_eq!(moved.records, 5);
        let r = verdict(&s);
        assert!(r.problem.is_none(), "{:?}", r.problem);
        assert_eq!(r.legacy, 25);
        let rec = record(
            &s.file(),
            &plan(PlanAction::Install, &["q"]),
            &Ok(()),
            60_000,
        )
        .unwrap();
        assert_eq!(rec.id, "tx-26");
        assert!(verdict(&s).problem.is_none());
    }

    #[test]
    fn event_acknowledgements_are_chained_and_not_repeated() {
        let s = Scratch::new("ack-events");
        record(&s.file(), &plan(PlanAction::Install, &["a"]), &Ok(()), 1).unwrap();
        let ids = vec!["ev-aaaaaaaaaaaa".to_string(), "ev-bbbbbbbbbbbb".to_string()];
        let done = History::acknowledge_events_at(&s.file(), &ids, 5).unwrap();
        assert_eq!(done, ids);
        assert!(History::acknowledge_events_at(&s.file(), &ids, 6)
            .unwrap()
            .is_empty());
        let acked = log::ack_ids(&log::all_lines(&s.file(), true).unwrap());
        assert!(acked.contains("ev-aaaaaaaaaaaa") && acked.contains("ev-bbbbbbbbbbbb"));
        assert!(verdict(&s).problem.is_none());
    }

    #[test]
    fn acknowledging_hides_a_failure_from_the_count_and_not_from_history() {
        let s = Scratch::new("ack");
        record(&s.file(), &plan(PlanAction::Install, &["a"]), &Ok(()), 1).unwrap();
        record(&s.file(), &plan(PlanAction::Upgrade, &["a"]), &failure(), 2).unwrap();
        record(&s.file(), &plan(PlanAction::Upgrade, &["a"]), &failure(), 3).unwrap();

        let got = History::acknowledge_at(&s.file(), &["tx-2".into()], 10).unwrap();
        assert_eq!(got, ["tx-2"]);
        let acked = log::ack_ids(&log::all_lines(&s.file(), false).unwrap());
        assert!(acked.contains("tx-2") && !acked.contains("tx-3"));
        // The record is still there, still failed.
        let all = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].status, TxStatus::Failed);

        // Again: nothing new. Then all the rest.
        assert!(History::acknowledge_at(&s.file(), &["tx-2".into()], 11)
            .unwrap()
            .is_empty());
        assert_eq!(
            History::acknowledge_at(&s.file(), &[], 12).unwrap(),
            ["tx-3"]
        );
        // It is a chained record like any other.
        assert!(verdict(&s).problem.is_none());
        assert_eq!(verdict(&s).chained, 5);
        // The next id is still right after markers.
        let rec = record(&s.file(), &plan(PlanAction::Install, &["b"]), &Ok(()), 13).unwrap();
        assert_eq!(rec.id, "tx-4");
    }

    #[test]
    fn only_a_failed_transaction_that_exists_can_be_acknowledged() {
        let s = Scratch::new("ack-refuse");
        record(&s.file(), &plan(PlanAction::Install, &["a"]), &Ok(()), 1).unwrap();
        assert!(matches!(
            History::acknowledge_at(&s.file(), &["tx-1".into()], 2),
            Err(Error::History(_))
        ));
        assert!(matches!(
            History::acknowledge_at(&s.file(), &["tx-8".into()], 2),
            Err(Error::UnknownTransaction(_))
        ));
        assert_eq!(lines(&s).len(), 1, "a refusal writes nothing");
    }

    fn anchor_at(s: &Scratch, seq: u64) -> String {
        let l = lines(s);
        sha_prefix(&l[seq as usize - 1], 32)
    }

    fn sha_prefix(line: &str, n: usize) -> String {
        log::sha256_hex(line.as_bytes())[..n].to_string()
    }

    #[test]
    fn an_anchor_still_matches_after_the_log_grows_or_is_archived() {
        let s = Scratch::new("anchor-ok");
        fill(&s, 25);
        let code = anchor_at(&s, 3);
        assert!(matches!(
            log::check_anchor(&s.file(), 3, &code).unwrap(),
            log::AnchorCheck::Matches
        ));
        fill_more(&s, 25, 3);
        assert!(matches!(
            log::check_anchor(&s.file(), 3, &code).unwrap(),
            log::AnchorCheck::Matches
        ));
        log::archive(&s.file(), u64::MAX, 1).unwrap().unwrap();
        assert!(
            matches!(
                log::check_anchor(&s.file(), 3, &code).unwrap(),
                log::AnchorCheck::Matches
            ),
            "an archived line is still checked"
        );
        // The newest line is an anchor too.
        let head = verdict(&s);
        let newest = &head.head_hash.clone().unwrap()[..32];
        assert!(matches!(
            log::check_anchor(&s.file(), head.head_seq.unwrap(), newest).unwrap(),
            log::AnchorCheck::Matches
        ));
    }

    #[test]
    fn an_anchor_catches_a_cut_tail_an_edit_and_a_rewrite_that_rebuilds_the_chain() {
        let s = Scratch::new("anchor-bad");
        fill(&s, 5);
        let code = anchor_at(&s, 4);
        let original = lines(&s);

        // A cut-off tail: the chain alone cannot see it, the anchor does.
        put(&s, &original[..2]);
        assert!(matches!(
            log::check_anchor(&s.file(), 4, &code).unwrap(),
            log::AnchorCheck::Shorter { head_seq: 2 }
        ));

        // An edit before the anchor breaks the chain on the way to it.
        let mut l = original.clone();
        l[1] = l[1].replace("install", "remove");
        put(&s, &l);
        assert!(matches!(
            log::check_anchor(&s.file(), 4, &code).unwrap(),
            log::AnchorCheck::Broken { .. }
        ));

        // The anchored line itself changed.
        let mut l = original.clone();
        l[3] = l[3].replace("install", "remove");
        put(&s, &l);
        assert!(matches!(
            log::check_anchor(&s.file(), 4, &code).unwrap(),
            log::AnchorCheck::Differs { .. }
        ));

        // Someone edits line 2 and recomputes every later `prev`: the chain
        // is whole again, the anchor is not.
        let mut l = original.clone();
        l[1] = l[1].replace("install", "remove");
        for i in 2..l.len() {
            let mut v: serde_json::Value = serde_json::from_str(&l[i]).unwrap();
            v["prev"] = log::sha256_hex(l[i - 1].as_bytes()).into();
            l[i] = serde_json::to_string(&v).unwrap();
        }
        put(&s, &l);
        assert!(verdict(&s).problem.is_none(), "the rebuilt chain verifies");
        assert!(matches!(
            log::check_anchor(&s.file(), 4, &code).unwrap(),
            log::AnchorCheck::Differs { .. }
        ));
    }

    #[test]
    fn anchor_codes_are_validated() {
        for bad in [
            "",
            "42",
            "x:abcdefabcdefabcd",
            "0:abcdefabcdefabcd",
            "3:xyz",
            "3:abc",
        ] {
            assert!(
                matches!(History::check_anchor(bad), Err(Error::History(_))),
                "{bad}"
            );
        }
    }
}

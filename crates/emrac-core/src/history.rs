use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::plan::{Plan, PlanAction, PlannedPackage};

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
    pub status: TxStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    pub fn record(plan: &Plan, outcome: &Result<()>) -> Result<TransactionRecord> {
        Self::record_to(&Self::path()?, plan, outcome, now())
    }

    /// Newest first. `since` is a unix timestamp lower bound (inclusive).
    pub fn list(limit: Option<usize>, since: Option<u64>) -> Result<Vec<TransactionRecord>> {
        Self::list_from(&Self::path()?, limit, since)
    }

    fn record_to(
        path: &Path,
        plan: &Plan,
        outcome: &Result<()>,
        timestamp: u64,
    ) -> Result<TransactionRecord> {
        let write_err = |source| Error::HistoryWrite {
            path: path.display().to_string(),
            source,
        };

        let next = read_from(path)?
            .iter()
            .filter_map(|r| r.id.strip_prefix("tx-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let record = TransactionRecord {
            id: format!("tx-{next}"),
            timestamp,
            action: plan.action,
            packages: plan.packages.clone(),
            status: if outcome.is_ok() {
                TxStatus::Committed
            } else {
                TxStatus::Failed
            },
            error: outcome.as_ref().err().map(|e| e.to_string()),
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(write_err)?;
        }
        let mut line = serde_json::to_string(&record).expect("TransactionRecord is serializable");
        line.push('\n');
        // One write_all in append mode: a crash can lose this line but
        // can't damage earlier ones.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(write_err)?;
        Ok(record)
    }

    fn list_from(
        path: &Path,
        limit: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<TransactionRecord>> {
        let mut records = read_from(path)?;
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
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(Error::Io {
                context: format!("couldn't read your history at {}", path.display()),
                source: err,
            });
        }
    };
    Ok(raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

fn now() -> u64 {
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

fn format_utc(timestamp: u64) -> String {
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
                })
                .collect(),
            total_download_size: 0,
            total_installed_size_delta: 0,
        }
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
        let rec = History::record_to(
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
        let rec = History::record_to(&s.file(), &plan(PlanAction::Upgrade, &["a"]), &failure(), 5)
            .unwrap();
        assert_eq!(rec.status, TxStatus::Failed);
        assert!(rec.error.unwrap().contains("boom"));
    }

    #[test]
    fn ids_increment_and_list_is_newest_first_with_limit() {
        let s = Scratch::new("order");
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            History::record_to(
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
            History::record_to(&s.file(), &plan(PlanAction::Remove, &["a"]), &Ok(()), t).unwrap();
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
        History::record_to(&s.file(), &plan(PlanAction::Install, &["a"]), &Ok(()), 1).unwrap();
        let mut f = OpenOptions::new().append(true).open(s.file()).unwrap();
        f.write_all(b"not json\n{\"id\":\"tx-9\"}\n\n").unwrap();
        drop(f);

        let listed = History::list_from(&s.file(), None, None).unwrap();
        assert_eq!(listed.len(), 1);
        let rec =
            History::record_to(&s.file(), &plan(PlanAction::Install, &["b"]), &Ok(()), 2).unwrap();
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
        let result = History::record_to(
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
}

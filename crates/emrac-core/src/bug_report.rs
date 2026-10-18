//! The failed-transactions part of `emrac bug-report` (u13).

use crate::history::{TransactionRecord, TxStatus};

/// How many failures a report lists.
const SHOWN: usize = 3;

/// The newest failed transactions with the first line of each error.
pub fn failed_section(records: &[TransactionRecord]) -> String {
    let mut out = String::from("\n## Recent failed transactions\n\n");
    let failed: Vec<&TransactionRecord> = records
        .iter()
        .filter(|r| r.status == TxStatus::Failed)
        .collect();
    if failed.is_empty() {
        out.push_str("none recorded\n");
    }
    for r in failed.iter().rev().take(SHOWN) {
        let names: Vec<&str> = r.packages.iter().map(|p| p.name.as_str()).collect();
        out.push_str(&format!(
            "- {} {:?} {}: {}\n",
            r.timestamp_utc(),
            r.action,
            names.join(", "),
            r.error
                .as_deref()
                .and_then(|e| e.lines().next())
                .unwrap_or("no error recorded")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanAction, PlannedPackage};

    fn record(time: u64, status: TxStatus, error: Option<&str>, name: &str) -> TransactionRecord {
        TransactionRecord {
            id: format!("tx-{time}"),
            timestamp: time,
            action: PlanAction::Install,
            packages: vec![PlannedPackage {
                name: name.into(),
                version: "1".into(),
                repo: "extra".into(),
                explicit: true,
                previous_version: None,
            }],
            removed: vec![],
            status,
            error: error.map(str::to_string),
            rollback_of: None,
            plan_digest: None,
        }
    }

    #[test]
    fn no_failures_says_so() {
        let ok = [record(1, TxStatus::Committed, None, "a")];
        assert!(failed_section(&ok).contains("none recorded"));
        assert!(failed_section(&[]).contains("none recorded"));
    }

    #[test]
    fn at_most_three_newest_first_and_only_the_first_line_of_the_error() {
        let all: Vec<_> = (1..=5)
            .map(|i| {
                record(
                    i,
                    TxStatus::Failed,
                    Some(&format!("boom {i}\nsecret detail")),
                    &format!("p{i}"),
                )
            })
            .chain([record(9, TxStatus::Committed, None, "fine")])
            .collect();
        let text = failed_section(&all);
        assert_eq!(text.matches("\n- ").count(), 3);
        assert!(text.find("boom 5").unwrap() < text.find("boom 4").unwrap());
        assert!(
            !text.contains("boom 2") && !text.contains("secret detail") && !text.contains("fine")
        );
    }

    #[test]
    fn a_failure_without_an_error_text_still_appears() {
        let text = failed_section(&[record(1, TxStatus::Failed, None, "a")]);
        assert!(text.contains("no error recorded") && text.contains("a:"));
    }
}

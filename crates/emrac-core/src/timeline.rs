//! `emrac timeline` (u10): what happened to the system, day by day, from the
//! records emrac keeps (transactions, builds, snapshots, config file actions).
//! See `docs/plan/r3/u10-timeline.md`.

use serde::Serialize;

use crate::history::{format_utc_date, TransactionRecord, TxStatus};
use crate::pacnew_log::{Record as PacnewRecord, State as PacnewState};
use crate::plan::{PlanAction, PlannedPackage};
use crate::recommend::BuildTime;
use crate::snapshot::Snapshot;

/// Names shown for one event before "and N more".
const SHOWN: usize = 4;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Installed,
    Upgraded,
    Downgraded,
    Reinstalled,
    Removed,
    Failed,
    RolledBack,
    Built,
    BuildFailed,
    Snapshot,
    Config,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Event {
    /// Unix seconds, UTC.
    pub time: u64,
    pub kind: Kind,
    pub text: String,
    /// Which record it came from.
    pub source: &'static str,
    /// Packages it names (for `--pkg`).
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only events at or after this Unix time.
    pub since: u64,
    pub package: Option<String>,
}

/// `a 1 -> 2, b 3` style list, cut after `SHOWN`.
fn list(items: Vec<String>) -> String {
    let more = items.len().saturating_sub(SHOWN);
    let mut shown: Vec<String> = items.into_iter().take(SHOWN).collect();
    if more > 0 {
        shown.push(format!("and {more} more"));
    }
    shown.join(", ")
}

fn pkg_text(p: &PlannedPackage, with_change: bool) -> String {
    match (&p.previous_version, with_change) {
        (Some(prev), true) => format!("{} {prev} -> {}", p.name, p.version),
        _ => format!("{} {}", p.name, p.version),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

fn from_transaction(r: &TransactionRecord) -> Event {
    let names: Vec<String> = r.packages.iter().map(|p| p.name.clone()).collect();
    if r.rollback_of.is_some() && r.status == TxStatus::Committed {
        return Event {
            time: r.timestamp,
            kind: Kind::RolledBack,
            text: format!(
                "Rolled back {}: {}",
                r.rollback_of.as_deref().unwrap_or("a transaction"),
                list(r.packages.iter().map(|p| pkg_text(p, true)).collect())
            ),
            source: "history",
            packages: names,
        };
    }
    let verb = match r.action {
        PlanAction::Install => "install",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Downgrade => "downgrade",
        PlanAction::Reinstall => "reinstall",
        PlanAction::Remove => "remove",
        PlanAction::Purge => "purge",
    };
    if r.status == TxStatus::Failed {
        let why = r.error.as_deref().map(first_line).unwrap_or("");
        let why = if why.is_empty() {
            String::new()
        } else {
            format!(" ({why})")
        };
        return Event {
            time: r.timestamp,
            kind: Kind::Failed,
            text: format!("Failed to {verb} {}{why}", list(names.clone())),
            source: "history",
            packages: names,
        };
    }
    let (kind, word, change) = match r.action {
        PlanAction::Install => (Kind::Installed, "Installed", false),
        PlanAction::Upgrade => (Kind::Upgraded, "Upgraded", true),
        PlanAction::Downgrade => (Kind::Downgraded, "Downgraded", true),
        PlanAction::Reinstall => (Kind::Reinstalled, "Reinstalled", false),
        PlanAction::Remove => (Kind::Removed, "Removed", false),
        PlanAction::Purge => (Kind::Removed, "Purged", false),
    };
    Event {
        time: r.timestamp,
        kind,
        text: format!(
            "{word} {}",
            list(r.packages.iter().map(|p| pkg_text(p, change)).collect())
        ),
        source: "history",
        packages: names,
    }
}

fn duration(secs: u64) -> String {
    if secs < 90 {
        format!("{secs} s")
    } else if secs < 5400 {
        format!("{} min", (secs + 30) / 60)
    } else {
        format!("{:.1} h", secs as f64 / 3600.0)
    }
}

fn from_build(b: &BuildTime) -> Event {
    let (kind, text) = if b.ok {
        (
            Kind::Built,
            format!("Built {} {} in {}", b.name, b.version, duration(b.secs)),
        )
    } else {
        (
            Kind::BuildFailed,
            format!(
                "Build of {} {} failed after {}",
                b.name,
                b.version,
                duration(b.secs)
            ),
        )
    };
    Event {
        time: b.time,
        kind,
        text,
        source: "build times",
        packages: vec![b.name.clone()],
    }
}

fn from_snapshot(s: &Snapshot) -> Event {
    Event {
        time: s.timestamp,
        kind: Kind::Snapshot,
        text: format!("Snapshot \"{}\" ({} packages)", s.name, s.packages.len()),
        source: "snapshots",
        packages: s.packages.iter().map(|p| p.name.clone()).collect(),
    }
}

fn from_pacnew(r: &PacnewRecord) -> Option<Event> {
    if r.state != PacnewState::Done {
        return None;
    }
    let word = match r.action.as_str() {
        "keep" => "Kept your version of",
        "take" => "Took the package's version of",
        "merge" => "Merged",
        "edit" => "Edited",
        "revert" => "Reverted the change to",
        other => other,
    };
    Some(Event {
        time: r.time,
        kind: Kind::Config,
        text: format!("{word} {}", r.file),
        source: "config log",
        packages: r.package.iter().cloned().collect(),
    })
}

/// Every event that passes `filter`, newest first.
pub fn collect(
    history: &[TransactionRecord],
    builds: &[BuildTime],
    snapshots: &[Snapshot],
    pacnew: &[PacnewRecord],
    filter: &Filter,
) -> Vec<Event> {
    let mut events: Vec<Event> = history
        .iter()
        .map(from_transaction)
        .chain(builds.iter().map(from_build))
        .chain(snapshots.iter().map(from_snapshot))
        .chain(pacnew.iter().filter_map(from_pacnew))
        .filter(|e| e.time >= filter.since)
        .filter(|e| {
            filter
                .package
                .as_ref()
                .is_none_or(|p| e.packages.iter().any(|n| n == p))
        })
        .collect();
    events.sort_by_key(|e| std::cmp::Reverse(e.time));
    events
}

/// `HH:MM` in UTC.
pub fn clock(time: u64) -> String {
    let s = time % 86_400;
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

/// The events grouped by UTC day, newest day first: `Today`, `Yesterday`, then the date.
pub fn group(events: Vec<Event>, now: u64) -> Vec<(String, Vec<Event>)> {
    let mut out: Vec<(String, Vec<Event>)> = Vec::new();
    for e in events {
        let day = e.time / 86_400;
        let today = now / 86_400;
        let label = if day == today {
            "Today".to_string()
        } else if day + 1 == today {
            "Yesterday".to_string()
        } else {
            format_utc_date(e.time)
        };
        match out.last_mut() {
            Some((l, list)) if *l == label => list.push(e),
            _ => out.push((label, vec![e])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SnapshotPackage;

    fn pkg(name: &str, version: &str, prev: Option<&str>) -> PlannedPackage {
        PlannedPackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            explicit: true,
            previous_version: prev.map(str::to_string),
        }
    }

    fn tx(
        action: PlanAction,
        status: TxStatus,
        time: u64,
        packages: Vec<PlannedPackage>,
    ) -> TransactionRecord {
        TransactionRecord {
            id: format!("tx-{time}"),
            timestamp: time,
            action,
            packages,
            removed: vec![],
            status,
            error: None,
            rollback_of: None,
            plan_digest: None,
        }
    }

    fn none() -> Filter {
        Filter::default()
    }

    const DAY: u64 = 86_400;

    #[test]
    fn transactions_read_as_sentences() {
        let h = [
            tx(
                PlanAction::Install,
                TxStatus::Committed,
                100,
                vec![pkg("firefox", "130", None)],
            ),
            tx(
                PlanAction::Upgrade,
                TxStatus::Committed,
                200,
                vec![pkg("mesa", "24.2.3", Some("24.2.1"))],
            ),
            tx(
                PlanAction::Remove,
                TxStatus::Committed,
                300,
                vec![pkg("discord", "1", None)],
            ),
        ];
        let e = collect(&h, &[], &[], &[], &none());
        let t: Vec<&str> = e.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "Removed discord 1",
                "Upgraded mesa 24.2.1 -> 24.2.3",
                "Installed firefox 130"
            ]
        );
        assert_eq!(e[0].kind, Kind::Removed);
    }

    #[test]
    fn a_long_list_is_cut_with_a_count() {
        let many: Vec<_> = (0..7).map(|i| pkg(&format!("p{i}"), "1", None)).collect();
        let e = collect(
            &[tx(PlanAction::Install, TxStatus::Committed, 5, many)],
            &[],
            &[],
            &[],
            &none(),
        );
        assert_eq!(e[0].text, "Installed p0 1, p1 1, p2 1, p3 1, and 3 more");
        assert_eq!(e[0].packages.len(), 7, "--pkg still sees all seven");
    }

    #[test]
    fn a_failed_transaction_carries_the_first_line_of_the_error() {
        let mut r = tx(
            PlanAction::Install,
            TxStatus::Failed,
            9,
            vec![pkg("foo", "1", None)],
        );
        r.error = Some("conflicting files\nmore detail".into());
        let e = collect(&[r], &[], &[], &[], &none());
        assert_eq!(e[0].kind, Kind::Failed);
        assert_eq!(e[0].text, "Failed to install foo (conflicting files)");
    }

    #[test]
    fn a_rollback_says_what_it_reversed() {
        let mut r = tx(
            PlanAction::Downgrade,
            TxStatus::Committed,
            9,
            vec![pkg("mesa", "24.2.1", Some("24.2.3"))],
        );
        r.rollback_of = Some("tx-5".into());
        let e = collect(&[r], &[], &[], &[], &none());
        assert_eq!(e[0].kind, Kind::RolledBack);
        assert!(
            e[0].text
                .starts_with("Rolled back tx-5: mesa 24.2.3 -> 24.2.1"),
            "{}",
            e[0].text
        );
    }

    #[test]
    fn builds_snapshots_and_config_actions_join_the_story() {
        let builds = [
            BuildTime {
                time: 10,
                name: "nvim".into(),
                version: "0.11".into(),
                secs: 240,
                cores: 4,
                ok: true,
            },
            BuildTime {
                time: 20,
                name: "bad".into(),
                version: "1".into(),
                secs: 30,
                cores: 4,
                ok: false,
            },
        ];
        let snap = Snapshot {
            name: "before".into(),
            timestamp: 15,
            note: None,
            packages: vec![SnapshotPackage {
                name: "a".into(),
                version: "1".into(),
                explicit: true,
                foreign: false,
            }],
        };
        let e = collect(&[], &builds, &[snap], &[], &none());
        let t: Vec<&str> = e.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "Build of bad 1 failed after 30 s",
                "Snapshot \"before\" (1 packages)",
                "Built nvim 0.11 in 4 min"
            ]
        );
        assert_eq!(
            e.iter().map(|e| e.source).collect::<Vec<_>>(),
            ["build times", "snapshots", "build times"]
        );
    }

    #[test]
    fn since_and_package_filters_apply_to_every_source() {
        let h = [
            tx(
                PlanAction::Install,
                TxStatus::Committed,
                50,
                vec![pkg("old", "1", None)],
            ),
            tx(
                PlanAction::Install,
                TxStatus::Committed,
                150,
                vec![pkg("mesa", "1", None)],
            ),
            tx(
                PlanAction::Install,
                TxStatus::Committed,
                160,
                vec![pkg("other", "1", None)],
            ),
        ];
        let b = [BuildTime {
            time: 170,
            name: "mesa".into(),
            version: "2".into(),
            secs: 1,
            cores: 1,
            ok: true,
        }];
        let f = Filter {
            since: 100,
            package: Some("mesa".into()),
        };
        let e = collect(&h, &b, &[], &[], &f);
        assert_eq!(e.len(), 2);
        assert!(e.iter().all(|e| e.packages == ["mesa"]));
        assert!(collect(
            &h,
            &b,
            &[],
            &[],
            &Filter {
                since: 1000,
                package: None
            }
        )
        .is_empty());
    }

    #[test]
    fn days_are_labelled_and_split_at_midnight_utc() {
        let now = 10 * DAY + 3600;
        let at = |t| Event {
            time: t,
            kind: Kind::Built,
            text: "x".into(),
            source: "build times",
            packages: vec![],
        };
        let g = group(
            vec![
                at(10 * DAY + 500),
                at(10 * DAY),
                at(10 * DAY - 1),
                at(8 * DAY + 5),
            ],
            now,
        );
        let labels: Vec<&str> = g.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Today", "Yesterday", "1970-01-09"]);
        assert_eq!(g[0].1.len(), 2);
    }

    #[test]
    fn the_clock_is_hours_and_minutes_utc() {
        assert_eq!(clock(DAY + 18 * 3600 + 42 * 60 + 59), "18:42");
        assert_eq!(clock(0), "00:00");
    }
}

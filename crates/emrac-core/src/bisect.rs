//! `emrac bisect` (u24): find the transaction that broke something by putting
//! the system back into states it really had.
//!
//! The search runs over transactions, never over packages: the state after
//! transaction *k* is one the machine was in, every package at the version it
//! had then, so no state visited is a partial upgrade by construction.

use std::collections::BTreeMap;

use crate::history::{TransactionRecord, TxStatus};
use crate::plan::PlanAction;
use crate::snapshot::SnapshotPackage;

/// A state the system was in.
#[derive(Debug, Clone)]
pub struct Point {
    /// `start`, or `after tx-12`.
    pub label: String,
    /// The transaction that led to this state; `None` for the first point.
    pub tx: Option<String>,
    pub timestamp: u64,
    pub packages: Vec<SnapshotPackage>,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    /// Oldest first; the last point is what is installed now.
    pub points: Vec<Point>,
    /// Packages whose installed version the history does not explain. They are
    /// kept at the installed version in every state.
    pub unexplained: Vec<String>,
}

/// Rebuilds the states from what is installed now, undoing `records` (oldest
/// first) from the newest. Only committed transactions are states.
pub fn reconstruct(installed: &[SnapshotPackage], records: &[TransactionRecord]) -> Timeline {
    let records: Vec<&TransactionRecord> = records
        .iter()
        .filter(|r| r.status == TxStatus::Committed)
        .collect();
    let mut state: BTreeMap<String, SnapshotPackage> = installed
        .iter()
        .map(|p| (p.name.clone(), p.clone()))
        .collect();
    let mut unexplained: Vec<String> = Vec::new();
    let mut points = vec![point_of("now", None, now_stamp(&records), &state)];

    for r in records.iter().rev() {
        // `state` is the state after `r`; undo it
        match r.action {
            PlanAction::Remove => {
                for p in &r.packages {
                    state
                        .entry(p.name.clone())
                        .or_insert_with(|| SnapshotPackage {
                            name: p.name.clone(),
                            version: p.version.clone(),
                            explicit: p.explicit,
                            foreign: p.repo == "aur",
                        });
                }
            }
            _ => {
                for p in &r.packages {
                    match state.get(&p.name) {
                        Some(now) if now.version == p.version => {}
                        _ => {
                            if !unexplained.contains(&p.name) {
                                unexplained.push(p.name.clone());
                            }
                            continue;
                        }
                    }
                    match &p.previous_version {
                        Some(before) => {
                            if let Some(entry) = state.get_mut(&p.name) {
                                entry.version = before.clone();
                            }
                        }
                        None => {
                            state.remove(&p.name);
                        }
                    }
                }
            }
        }
        // packages removed to resolve a conflict come back
        for gone in &r.removed {
            state
                .entry(gone.name.clone())
                .or_insert_with(|| SnapshotPackage {
                    name: gone.name.clone(),
                    version: gone.version.clone(),
                    explicit: false,
                    foreign: false,
                });
        }
        // the point just built is the state after the previous record; label the
        // one we came from by this record
        let last = points.len() - 1;
        points[last].label = format!("after {}", r.id);
        points[last].tx = Some(r.id.clone());
        points[last].timestamp = r.timestamp;
        points.push(point_of("start", None, 0, &state));
    }
    points.reverse();
    // the oldest point is the state before the oldest record we know of
    if let Some(first) = points.first_mut() {
        first.timestamp = records
            .first()
            .map(|r| r.timestamp.saturating_sub(1))
            .unwrap_or(0);
    }
    // the newest point is what is installed now
    if let Some(last) = points.last_mut() {
        if records.is_empty() {
            last.label = "now".to_string();
        }
    }
    // unexplained packages stay at their installed version everywhere
    for name in &unexplained {
        if let Some(cur) = installed.iter().find(|p| &p.name == name) {
            for p in points.iter_mut() {
                match p.packages.iter_mut().find(|x| &x.name == name) {
                    Some(x) => x.version = cur.version.clone(),
                    None => p.packages.push(cur.clone()),
                }
            }
        }
    }
    unexplained.sort();
    Timeline {
        points,
        unexplained,
    }
}

fn now_stamp(records: &[&TransactionRecord]) -> u64 {
    records.last().map(|r| r.timestamp).unwrap_or(0)
}

fn point_of(
    label: &str,
    tx: Option<String>,
    timestamp: u64,
    state: &BTreeMap<String, SnapshotPackage>,
) -> Point {
    Point {
        label: label.to_string(),
        tx,
        timestamp,
        packages: state.values().cloned().collect(),
    }
}

/// How the user names the state that was good (or bad).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// The system was fine after this transaction.
    Tx(String),
    /// The state at this moment (Unix seconds, UTC).
    At(u64),
}

impl Reference {
    /// `tx-12`, a date (`2026-10-10`, the end of that UTC day) or a time back
    /// from now (`7d`, `12h`).
    pub fn parse(text: &str, now: u64) -> Option<Reference> {
        let text = text.trim();
        if text.starts_with("tx-") {
            return Some(Reference::Tx(text.to_string()));
        }
        if let Some(day) = days_since_epoch(text) {
            return Some(Reference::At(day * 86_400 + 86_399));
        }
        let back = crate::build_stats::parse_since(text)?;
        Some(Reference::At(now.saturating_sub(back)))
    }
}

/// `YYYY-MM-DD` as days since 1970-01-01.
fn days_since_epoch(text: &str) -> Option<u64> {
    let mut it = text.split('-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u64::try_from(era * 146_097 + doe - 719_468).ok()
}

/// The index of the state a reference means: after the transaction, or after the
/// newest transaction at or before the moment.
pub fn locate(points: &[Point], reference: &Reference) -> Option<usize> {
    match reference {
        Reference::Tx(id) => points.iter().position(|p| p.tx.as_deref() == Some(id)),
        Reference::At(ts) => {
            let mut found = None;
            for (i, p) in points.iter().enumerate() {
                if p.timestamp <= *ts {
                    found = Some(i);
                }
            }
            found
        }
    }
}

/// What differs between two states: `(name, version in a, version in b)`.
pub fn changed(a: &Point, b: &Point) -> Vec<(String, Option<String>, Option<String>)> {
    let version = |p: &Point, name: &str| {
        p.packages
            .iter()
            .find(|x| x.name == name)
            .map(|x| x.version.clone())
    };
    let mut names: Vec<&str> = a
        .packages
        .iter()
        .chain(&b.packages)
        .map(|p| p.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter_map(|n| {
            let (x, y) = (version(a, n), version(b, n));
            (x != y).then(|| (n.to_string(), x, y))
        })
        .collect()
}

/// The judgement of one state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Good,
    Bad,
    /// Cannot tell (the test said so, or the state could not be reached).
    Skip,
}

/// The search: `lo` is good, `hi` is bad; the answer is the first bad state.
#[derive(Debug, Clone)]
pub struct Search {
    lo: usize,
    hi: usize,
    skipped: Vec<usize>,
}

/// Where the search stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Look at this state next.
    Next(usize),
    /// The first bad state.
    Found(usize),
    /// Every state left between good and bad was skipped: the culprit is one of these.
    Inconclusive { after: usize, up_to: usize },
}

impl Search {
    pub fn new(good: usize, bad: usize) -> Search {
        Search {
            lo: good,
            hi: bad,
            skipped: Vec::new(),
        }
    }

    /// How many states will be looked at, at most (without skips).
    pub fn steps(good: usize, bad: usize) -> u32 {
        let n = bad.saturating_sub(good);
        if n <= 1 {
            0
        } else {
            usize::BITS - (n - 1).leading_zeros()
        }
    }

    pub fn state(&self) -> Outcome {
        if self.hi - self.lo <= 1 {
            return Outcome::Found(self.hi);
        }
        let mid = self.lo + (self.hi - self.lo) / 2;
        // the unskipped state nearest the middle
        let mut best: Option<usize> = None;
        for c in (self.lo + 1)..self.hi {
            if self.skipped.contains(&c) {
                continue;
            }
            let d = c.abs_diff(mid);
            if best.is_none_or(|b| d < b.abs_diff(mid)) {
                best = Some(c);
            }
        }
        match best {
            Some(c) => Outcome::Next(c),
            None => Outcome::Inconclusive {
                after: self.lo,
                up_to: self.hi,
            },
        }
    }

    pub fn mark(&mut self, index: usize, verdict: Verdict) {
        match verdict {
            Verdict::Good => self.lo = index,
            Verdict::Bad => self.hi = index,
            Verdict::Skip => self.skipped.push(index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlannedPackage;

    fn pkg(name: &str, v: &str) -> SnapshotPackage {
        SnapshotPackage {
            name: name.into(),
            version: v.into(),
            explicit: true,
            foreign: false,
        }
    }

    fn rec(
        id: &str,
        ts: u64,
        action: PlanAction,
        list: &[(&str, &str, Option<&str>)],
    ) -> TransactionRecord {
        TransactionRecord {
            id: id.into(),
            timestamp: ts,
            action,
            packages: list
                .iter()
                .map(|(n, v, prev)| PlannedPackage {
                    name: n.to_string(),
                    version: v.to_string(),
                    repo: "core".into(),
                    explicit: true,
                    previous_version: prev.map(str::to_string),
                })
                .collect(),
            removed: vec![],
            status: TxStatus::Committed,
            error: None,
            rollback_of: None,
            plan_digest: None,
        }
    }

    fn version_in(p: &Point, name: &str) -> Option<String> {
        p.packages
            .iter()
            .find(|x| x.name == name)
            .map(|x| x.version.clone())
    }

    #[test]
    fn the_states_are_rebuilt_by_undoing_the_history_from_the_newest() {
        let installed = [pkg("a", "3"), pkg("b", "1"), pkg("c", "2")];
        let records = [
            rec("tx-1", 10, PlanAction::Install, &[("b", "1", None)]),
            rec("tx-2", 20, PlanAction::Upgrade, &[("a", "2", Some("1"))]),
            rec(
                "tx-3",
                30,
                PlanAction::Upgrade,
                &[("a", "3", Some("2")), ("c", "2", Some("1"))],
            ),
        ];
        let t = reconstruct(&installed, &records);
        assert!(t.unexplained.is_empty());
        assert_eq!(t.points.len(), 4);
        assert_eq!(t.points[3].label, "after tx-3");
        assert_eq!(version_in(&t.points[3], "a").as_deref(), Some("3"));
        assert_eq!(version_in(&t.points[2], "a").as_deref(), Some("2"));
        assert_eq!(version_in(&t.points[2], "c").as_deref(), Some("1"));
        assert_eq!(version_in(&t.points[1], "a").as_deref(), Some("1"));
        assert_eq!(version_in(&t.points[1], "b").as_deref(), Some("1"));
        // before tx-1, b was not installed
        assert_eq!(version_in(&t.points[0], "b"), None);
        assert_eq!(t.points[1].tx.as_deref(), Some("tx-1"));
    }

    #[test]
    fn a_removal_comes_back_and_a_conflict_removal_too() {
        let installed = [pkg("a", "1")];
        let mut r2 = rec("tx-2", 20, PlanAction::Install, &[("a", "1", None)]);
        r2.removed = vec![crate::plan::PlannedRemoval {
            name: "old".into(),
            version: "9".into(),
            reason: "conflicts with a".into(),
        }];
        let records = [
            rec("tx-1", 10, PlanAction::Install, &[("gone", "5", None)]),
            rec("tx-1b", 15, PlanAction::Remove, &[("gone", "5", None)]),
            r2,
        ];
        let t = reconstruct(&installed, &records);
        let last = t.points.len() - 1;
        assert_eq!(version_in(&t.points[last], "old"), None, "removed in tx-2");
        assert_eq!(version_in(&t.points[last - 1], "old").as_deref(), Some("9"));
        assert_eq!(version_in(&t.points[last - 1], "gone"), None);
        assert_eq!(
            version_in(&t.points[last - 2], "gone").as_deref(),
            Some("5")
        );
    }

    #[test]
    fn a_change_the_history_does_not_explain_is_named_and_held_still() {
        // pacman was run by hand: `a` is 4, but the history ends at 3
        let installed = [pkg("a", "4")];
        let records = [rec(
            "tx-1",
            10,
            PlanAction::Upgrade,
            &[("a", "3", Some("2"))],
        )];
        let t = reconstruct(&installed, &records);
        assert_eq!(t.unexplained, ["a"]);
        for p in &t.points {
            assert_eq!(version_in(p, "a").as_deref(), Some("4"), "{}", p.label);
        }
    }

    #[test]
    fn failed_transactions_are_not_states() {
        let installed = [pkg("a", "2")];
        let mut failed = rec("tx-2", 20, PlanAction::Upgrade, &[("a", "9", Some("2"))]);
        failed.status = TxStatus::Failed;
        let records = [
            rec("tx-1", 10, PlanAction::Upgrade, &[("a", "2", Some("1"))]),
            failed,
        ];
        let t = reconstruct(&installed, &records);
        assert_eq!(t.points.len(), 2);
        assert!(t.unexplained.is_empty());
    }

    #[test]
    fn a_reference_is_a_transaction_or_a_moment() {
        let installed = [pkg("a", "3")];
        let records = [
            rec("tx-1", 10, PlanAction::Upgrade, &[("a", "2", Some("1"))]),
            rec("tx-2", 20, PlanAction::Upgrade, &[("a", "3", Some("2"))]),
        ];
        let t = reconstruct(&installed, &records);
        assert_eq!(locate(&t.points, &Reference::Tx("tx-1".into())), Some(1));
        assert_eq!(locate(&t.points, &Reference::Tx("tx-9".into())), None);
        assert_eq!(locate(&t.points, &Reference::At(25)), Some(2));
        assert_eq!(locate(&t.points, &Reference::At(15)), Some(1));
        let diff = changed(&t.points[1], &t.points[2]);
        assert_eq!(
            diff,
            [("a".to_string(), Some("2".into()), Some("3".into()))]
        );
    }

    /// Runs the search against a hidden first-bad state.
    fn find(good: usize, bad: usize, culprit: usize, skips: &[usize]) -> (Outcome, u32) {
        let mut s = Search::new(good, bad);
        let mut looked = 0;
        loop {
            match s.state() {
                Outcome::Next(i) => {
                    looked += 1;
                    s.mark(
                        i,
                        if skips.contains(&i) {
                            Verdict::Skip
                        } else if i >= culprit {
                            Verdict::Bad
                        } else {
                            Verdict::Good
                        },
                    );
                }
                other => return (other, looked),
            }
        }
    }

    #[test]
    fn the_search_finds_every_culprit_in_every_window_within_the_step_bound() {
        for width in 1..=40usize {
            for culprit in 1..=width {
                let (out, looked) = find(0, width, culprit, &[]);
                assert_eq!(
                    out,
                    Outcome::Found(culprit),
                    "width {width} culprit {culprit}"
                );
                assert!(
                    looked <= Search::steps(0, width),
                    "width {width}: {looked} looks"
                );
            }
        }
        assert_eq!(Search::steps(0, 1), 0);
        assert_eq!(Search::steps(0, 8), 3);
        assert_eq!(Search::steps(3, 12), 4);
    }

    #[test]
    fn skipped_states_are_stepped_around_and_a_fully_skipped_window_is_said() {
        let (out, _) = find(0, 10, 7, &[5]);
        assert_eq!(out, Outcome::Found(7));
        // the culprit hides among skipped states: the range is what is known
        let (out, _) = find(0, 4, 3, &[1, 2, 3]);
        assert_eq!(out, Outcome::Inconclusive { after: 0, up_to: 4 });
    }

    #[test]
    fn references_are_read_as_a_transaction_a_date_or_a_time_back() {
        assert_eq!(
            Reference::parse("tx-12", 0),
            Some(Reference::Tx("tx-12".into()))
        );
        // 2026-10-10 ends at 2026-10-10 23:59:59 UTC
        assert_eq!(
            Reference::parse("2026-10-10", 0),
            Some(Reference::At(1_791_676_799))
        );
        assert_eq!(
            Reference::parse("1970-01-02", 0),
            Some(Reference::At(2 * 86_400 - 1))
        );
        assert_eq!(
            Reference::parse("7d", 1_000_000),
            Some(Reference::At(1_000_000 - 7 * 86_400))
        );
        assert_eq!(Reference::parse("soon", 0), None);
        assert_eq!(Reference::parse("2026-13-01", 0), None);
    }
}

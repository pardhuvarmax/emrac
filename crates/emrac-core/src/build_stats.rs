//! `emrac stats` (u9): what emrac has built and installed here, from the two
//! records it already keeps: the build-times file and the transaction history.
//!
//! Only what was measured is reported. "Hours saved" and "storage saved" would
//! need a measured binary install next to a measured build; emrac measures only
//! the build, so those figures are not made up (`docs/plan/r3/u9-stats-benchmark.md`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::history::{TransactionRecord, TxStatus};
use crate::plan::PlanAction;
use crate::recommend::BuildTime;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PackageBuilds {
    pub name: String,
    pub builds: u32,
    pub average_secs: u64,
    pub last_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct BuildStats {
    pub total: u32,
    pub failed: u32,
    pub packages: u32,
    /// Seconds spent in successful builds.
    pub total_secs: u64,
    /// Over successful builds; 0 when there were none.
    pub average_secs: u64,
    pub longest: Option<(String, u64)>,
    /// Most built first, at most `TOP`.
    pub most_built: Vec<PackageBuilds>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct TransactionStats {
    pub committed: u32,
    pub failed: u32,
    pub installs: u32,
    pub upgrades: u32,
    pub removals: u32,
    /// Other committed kinds: downgrades, reinstalls.
    pub other: u32,
    /// Package installs and upgrades from the AUR.
    pub aur_packages: u32,
    /// Package installs and upgrades from the official repositories.
    pub repo_packages: u32,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Stats {
    /// Seconds back from `now` the report covers; `None` is everything kept.
    pub since_secs: Option<u64>,
    /// Unix time of the oldest record counted, if any.
    pub first: Option<u64>,
    pub builds: BuildStats,
    pub transactions: TransactionStats,
}

const TOP: usize = 5;

/// `30d`, `12h`, `8w`, `90m` as seconds; a bare number is days.
pub fn parse_since(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, unit) = match text.char_indices().last() {
        Some((i, c)) if c.is_ascii_alphabetic() => (&text[..i], c),
        _ => (text, 'd'),
    };
    let n: u64 = digits.parse().ok()?;
    let per = match unit {
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return None,
    };
    n.checked_mul(per).filter(|&s| s > 0)
}

pub fn compute(
    builds: &[BuildTime],
    history: &[TransactionRecord],
    since_secs: Option<u64>,
    now: u64,
) -> Stats {
    let floor = since_secs.map_or(0, |s| now.saturating_sub(s));
    let builds: Vec<&BuildTime> = builds.iter().filter(|b| b.time >= floor).collect();
    let history: Vec<&TransactionRecord> =
        history.iter().filter(|r| r.timestamp >= floor).collect();

    let mut b = BuildStats {
        total: builds.len() as u32,
        failed: builds.iter().filter(|b| !b.ok).count() as u32,
        ..Default::default()
    };
    let mut per: BTreeMap<&str, Vec<&BuildTime>> = BTreeMap::new();
    for build in builds.iter().filter(|b| b.ok) {
        per.entry(&build.name).or_default().push(build);
        b.total_secs += build.secs;
        if b.longest.as_ref().is_none_or(|(_, s)| build.secs > *s) {
            b.longest = Some((build.name.clone(), build.secs));
        }
    }
    let ok = b.total - b.failed;
    b.average_secs = if ok == 0 {
        0
    } else {
        b.total_secs / u64::from(ok)
    };
    b.packages = builds
        .iter()
        .map(|x| x.name.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    let mut most: Vec<PackageBuilds> = per
        .into_iter()
        .map(|(name, list)| PackageBuilds {
            name: name.to_string(),
            builds: list.len() as u32,
            average_secs: list.iter().map(|x| x.secs).sum::<u64>() / list.len() as u64,
            last_secs: list.iter().max_by_key(|x| x.time).map_or(0, |x| x.secs),
        })
        .collect();
    most.sort_by(|a, b| b.builds.cmp(&a.builds).then_with(|| a.name.cmp(&b.name)));
    most.truncate(TOP);
    b.most_built = most;

    let mut t = TransactionStats::default();
    for r in &history {
        if r.status == TxStatus::Failed {
            t.failed += 1;
            continue;
        }
        t.committed += 1;
        match r.action {
            PlanAction::Install => t.installs += 1,
            PlanAction::Upgrade => t.upgrades += 1,
            PlanAction::Remove | PlanAction::Purge => t.removals += 1,
            PlanAction::Downgrade | PlanAction::Reinstall => t.other += 1,
        }
        if matches!(r.action, PlanAction::Install | PlanAction::Upgrade) {
            for p in &r.packages {
                if p.repo.eq_ignore_ascii_case("aur") {
                    t.aur_packages += 1;
                } else {
                    t.repo_packages += 1;
                }
            }
        }
    }

    let first = builds
        .iter()
        .map(|b| b.time)
        .chain(history.iter().map(|r| r.timestamp))
        .min();
    Stats {
        since_secs,
        first,
        builds: b,
        transactions: t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlannedPackage;

    fn build(name: &str, secs: u64, time: u64, ok: bool) -> BuildTime {
        BuildTime {
            time,
            name: name.into(),
            version: "1".into(),
            secs,
            cores: 4,
            ok,
        }
    }

    fn tx(action: PlanAction, status: TxStatus, time: u64, repos: &[&str]) -> TransactionRecord {
        TransactionRecord {
            id: format!("t{time}"),
            timestamp: time,
            action,
            packages: repos
                .iter()
                .map(|r| PlannedPackage {
                    name: "p".into(),
                    version: "1".into(),
                    repo: (*r).into(),
                    explicit: true,
                    previous_version: None,
                })
                .collect(),
            removed: vec![],
            status,
            error: None,
            rollback_of: None,
            plan_digest: None,
        }
    }

    #[test]
    fn since_accepts_units_and_a_bare_number_of_days() {
        assert_eq!(parse_since("30d"), Some(30 * 86_400));
        assert_eq!(parse_since("12h"), Some(12 * 3_600));
        assert_eq!(parse_since("8w"), Some(8 * 604_800));
        assert_eq!(parse_since("90m"), Some(5_400));
        assert_eq!(parse_since("7"), Some(7 * 86_400));
        for bad in ["", "d", "0d", "3x", "-1d", "1.5d"] {
            assert_eq!(parse_since(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_recorded_gives_zeros_and_no_first_date() {
        let s = compute(&[], &[], None, 1000);
        assert_eq!(s, Stats::default());
    }

    #[test]
    fn failed_builds_count_but_are_left_out_of_the_times() {
        let b = [
            build("a", 100, 10, true),
            build("a", 300, 20, true),
            build("b", 5000, 30, false),
        ];
        let s = compute(&b, &[], None, 100).builds;
        assert_eq!((s.total, s.failed, s.packages), (3, 1, 2));
        assert_eq!(s.total_secs, 400);
        assert_eq!(s.average_secs, 200);
        assert_eq!(s.longest, Some(("a".to_string(), 300)));
        assert_eq!(s.most_built.len(), 1);
        assert_eq!(s.most_built[0].builds, 2);
        assert_eq!(s.most_built[0].average_secs, 200);
        assert_eq!(s.most_built[0].last_secs, 300);
    }

    #[test]
    fn only_failed_builds_have_no_average() {
        let s = compute(&[build("a", 9, 1, false)], &[], None, 100).builds;
        assert_eq!((s.total, s.failed, s.average_secs), (1, 1, 0));
        assert_eq!(s.longest, None);
    }

    #[test]
    fn since_drops_older_records_from_both_sources() {
        let b = [build("old", 10, 100, true), build("new", 20, 950, true)];
        let h = [
            tx(PlanAction::Install, TxStatus::Committed, 100, &["core"]),
            tx(PlanAction::Install, TxStatus::Committed, 960, &["core"]),
        ];
        let s = compute(&b, &h, Some(100), 1000);
        assert_eq!(s.builds.total, 1);
        assert_eq!(s.transactions.committed, 1);
        assert_eq!(s.first, Some(950));
    }

    #[test]
    fn transactions_are_split_by_kind_and_the_aur_share_is_counted() {
        let h = [
            tx(
                PlanAction::Install,
                TxStatus::Committed,
                1,
                &["aur", "extra"],
            ),
            tx(
                PlanAction::Upgrade,
                TxStatus::Committed,
                2,
                &["core", "extra", "AUR"],
            ),
            tx(PlanAction::Remove, TxStatus::Committed, 3, &["extra"]),
            tx(PlanAction::Reinstall, TxStatus::Committed, 4, &["aur"]),
            tx(PlanAction::Install, TxStatus::Failed, 5, &["aur"]),
        ];
        let t = compute(&[], &h, None, 10).transactions;
        assert_eq!((t.committed, t.failed), (4, 1));
        assert_eq!((t.installs, t.upgrades, t.removals, t.other), (1, 1, 1, 1));
        // only installs and upgrades count packages
        assert_eq!((t.aur_packages, t.repo_packages), (2, 3));
    }

    #[test]
    fn the_most_built_list_is_ordered_and_cut() {
        let mut b = Vec::new();
        for (i, n) in ["a", "b", "c", "d", "e", "f", "g"].iter().enumerate() {
            for _ in 0..=i {
                b.push(build(n, 10, 1, true));
            }
        }
        let s = compute(&b, &[], None, 10).builds;
        let names: Vec<&str> = s.most_built.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["g", "f", "e", "d", "c"]);
    }
}

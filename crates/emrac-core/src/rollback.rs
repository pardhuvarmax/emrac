//! Planning for `rollback`/`undo`: what is the inverse of a recorded
//! transaction, and which of its packages are still in the state it left
//! them in. Pure logic — the installed-version and package-file lookups are
//! passed in, so none of this needs libalpm or pacman.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::history::{TransactionRecord, TxStatus};
use crate::plan::{Plan, PlanAction, PlannedPackage};

#[derive(Debug, Clone, Serialize)]
pub struct SkippedPackage {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RollbackPlan {
    /// The transaction being reversed.
    pub target: String,
    /// The inverse operation: `Remove` for an install, `Install` for a
    /// remove, `Downgrade` for an upgrade. Only packages still in their
    /// post-transaction state are in here.
    pub plan: Plan,
    /// Package files for `pacman -U` (install/downgrade inverses); empty
    /// for a removal.
    pub files: Vec<PathBuf>,
    pub skipped: Vec<SkippedPackage>,
}

/// Normalizes user input like `tx-12` (anything else is rejected).
pub fn parse_tx_id(input: &str) -> Result<String> {
    match input.strip_prefix("tx-") {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(input.to_string()),
        _ => Err(Error::InvalidTransactionId(input.to_string())),
    }
}

/// The record that reversed `id`, if one committed.
pub fn rolled_back_by<'a>(
    history: &'a [TransactionRecord],
    id: &str,
) -> Option<&'a TransactionRecord> {
    history
        .iter()
        .find(|r| r.rollback_of.as_deref() == Some(id) && r.status == TxStatus::Committed)
}

pub fn find_record<'a>(
    history: &'a [TransactionRecord],
    id: &str,
) -> Result<&'a TransactionRecord> {
    history
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| Error::UnknownTransaction(id.to_string()))
}

/// Records `undo`/a bare `rollback` may pick, newest first: not themselves
/// rollbacks, and not already rolled back. `history` is oldest first.
pub fn candidates(history: &[TransactionRecord]) -> Vec<&TransactionRecord> {
    history
        .iter()
        .rev()
        .filter(|r| r.rollback_of.is_none() && rolled_back_by(history, &r.id).is_none())
        .collect()
}

/// Builds the inverse of `record`. `history` (oldest first) is only used to
/// refuse repeats. `installed` returns a package's current version;
/// `find_file` locates a cached package file for an exact name + version.
pub fn plan_rollback(
    record: &TransactionRecord,
    history: &[TransactionRecord],
    installed: &dyn Fn(&str) -> Option<String>,
    find_file: &dyn Fn(&str, &str) -> Option<PathBuf>,
) -> Result<RollbackPlan> {
    let refuse = |why: String| Error::NotRollbackable {
        id: record.id.clone(),
        why,
    };

    if let Some(original) = &record.rollback_of {
        return Err(refuse(format!(
            "it's the rollback of {original}, and redoing isn't supported yet"
        )));
    }
    if let Some(by) = rolled_back_by(history, &record.id) {
        return Err(refuse(format!("it was already rolled back by {}", by.id)));
    }
    if record.action == PlanAction::Downgrade {
        return Err(refuse(
            "downgrades are only ever written as rollbacks".into(),
        ));
    }
    if record.action == PlanAction::Upgrade
        && record.packages.iter().any(|p| p.previous_version.is_none())
    {
        return Err(refuse(
            "it was recorded before rollback support, so the versions to go back to are unknown"
                .into(),
        ));
    }

    let inverse = match record.action {
        PlanAction::Install => PlanAction::Remove,
        PlanAction::Remove => PlanAction::Install,
        PlanAction::Upgrade => PlanAction::Downgrade,
        PlanAction::Downgrade => unreachable!("refused above"),
    };

    let mut packages = Vec::new();
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut missing = Vec::new();

    for pkg in &record.packages {
        let current = installed(&pkg.name);
        if let Err(reason) = still_in_post_state(record.action, pkg, current.as_deref()) {
            skipped.push(SkippedPackage {
                name: pkg.name.clone(),
                reason,
            });
            continue;
        }

        let (version, previous_version) = match record.action {
            PlanAction::Install => (current.clone().unwrap_or_else(|| pkg.version.clone()), None),
            PlanAction::Remove => (pkg.version.clone(), None),
            PlanAction::Upgrade => (
                pkg.previous_version.clone().expect("checked above"),
                current.clone(),
            ),
            PlanAction::Downgrade => unreachable!("refused above"),
        };

        if inverse != PlanAction::Remove {
            match find_file(&pkg.name, &version) {
                Some(path) => files.push(path),
                None => missing.push(format!("{} {version}", pkg.name)),
            }
        }

        packages.push(PlannedPackage {
            name: pkg.name.clone(),
            version,
            repo: pkg.repo.clone(),
            explicit: pkg.explicit,
            previous_version,
        });
    }

    if !missing.is_empty() {
        return Err(Error::RollbackFilesMissing(missing.join(", ")));
    }

    Ok(RollbackPlan {
        target: record.id.clone(),
        plan: Plan {
            action: inverse,
            packages,
            total_download_size: 0,
            total_installed_size_delta: 0,
        },
        files,
        skipped,
    })
}

/// Is `pkg` still as `action` left it? `Err` carries the reason it isn't.
///
/// AUR versions come from RPC metadata, which for VCS packages (`-git`)
/// differs from the version the build actually produced, so for those the
/// version is not compared — only that something is installed.
fn still_in_post_state(
    action: PlanAction,
    pkg: &PlannedPackage,
    current: Option<&str>,
) -> std::result::Result<(), String> {
    match action {
        PlanAction::Install | PlanAction::Upgrade => {
            let Some(current) = current else {
                return Err("no longer installed".into());
            };
            if pkg.repo == "aur" {
                if action == PlanAction::Upgrade && pkg.previous_version.as_deref() == Some(current)
                {
                    return Err(format!("already back at {current}"));
                }
                Ok(())
            } else if current == pkg.version {
                Ok(())
            } else {
                Err(format!("now at {current}, not {}", pkg.version))
            }
        }
        PlanAction::Remove => match current {
            Some(current) => Err(format!("installed again (at {current})")),
            None => Ok(()),
        },
        PlanAction::Downgrade => Err("not rollbackable".into()),
    }
}

/// Finds `<name>-<version>-<arch>.pkg.tar[.<compression>]` in `dirs` (not
/// recursive). The shape is matched exactly, so `foo` never matches a
/// `foo-bar` package, and `.sig` files are ignored.
pub fn find_package_file(dirs: &[PathBuf], name: &str, version: &str) -> Option<PathBuf> {
    let prefix = format!("{name}-{version}-");
    let mut found: Vec<PathBuf> = Vec::new();

    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if is_package_file(file_name, &prefix) && entry.path().is_file() {
                found.push(entry.path());
            }
        }
    }

    found.sort();
    found.into_iter().next()
}

fn is_package_file(file_name: &str, prefix: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(prefix) else {
        return false;
    };
    let Some((arch, ext)) = rest.split_once(".pkg.tar") else {
        return false;
    };
    // The compressions makepkg can produce; anything else (`.sig`,
    // `.part` from an interrupted download, ...) isn't an installable file.
    const EXTENSIONS: [&str; 9] = [
        "", ".zst", ".xz", ".gz", ".bz2", ".lz4", ".lrz", ".lzo", ".Z",
    ];
    !arch.is_empty() && !arch.contains('-') && EXTENSIONS.contains(&ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;

    fn pkg(name: &str, version: &str, prev: Option<&str>) -> PlannedPackage {
        PlannedPackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            explicit: true,
            previous_version: prev.map(str::to_string),
        }
    }

    fn rec(id: &str, action: PlanAction, packages: Vec<PlannedPackage>) -> TransactionRecord {
        TransactionRecord {
            id: id.into(),
            timestamp: 0,
            action,
            packages,
            status: TxStatus::Committed,
            error: None,
            rollback_of: None,
        }
    }

    struct World {
        installed: HashMap<String, String>,
        files: HashMap<(String, String), PathBuf>,
    }

    impl World {
        fn new(installed: &[(&str, &str)], files: &[(&str, &str)]) -> Self {
            World {
                installed: installed
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .collect(),
                files: files
                    .iter()
                    .map(|(n, v)| {
                        (
                            (n.to_string(), v.to_string()),
                            PathBuf::from(format!("/cache/{n}-{v}-x86_64.pkg.tar.zst")),
                        )
                    })
                    .collect(),
            }
        }

        fn plan(
            &self,
            record: &TransactionRecord,
            history: &[TransactionRecord],
        ) -> Result<RollbackPlan> {
            plan_rollback(
                record,
                history,
                &|n| self.installed.get(n).cloned(),
                &|n, v| self.files.get(&(n.to_string(), v.to_string())).cloned(),
            )
        }
    }

    fn names(p: &RollbackPlan) -> Vec<&str> {
        p.plan.packages.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn install_is_undone_by_removal_without_needing_files() {
        let r = rec(
            "tx-1",
            PlanAction::Install,
            vec![pkg("a", "1-1", None), pkg("dep", "2-1", None)],
        );
        let w = World::new(&[("a", "1-1"), ("dep", "2-1")], &[]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(p.plan.action, PlanAction::Remove);
        assert_eq!(names(&p), ["a", "dep"]);
        assert!(p.files.is_empty());
        assert!(p.skipped.is_empty());
        assert_eq!(p.target, "tx-1");
    }

    #[test]
    fn remove_is_undone_by_reinstalling_the_recorded_version_from_files() {
        let r = rec("tx-1", PlanAction::Remove, vec![pkg("a", "1-1", None)]);
        let w = World::new(&[], &[("a", "1-1")]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(p.plan.action, PlanAction::Install);
        assert_eq!(p.plan.packages[0].version, "1-1");
        assert_eq!(p.files, [PathBuf::from("/cache/a-1-1-x86_64.pkg.tar.zst")]);
    }

    #[test]
    fn upgrade_is_undone_by_downgrading_to_the_previous_version() {
        let r = rec(
            "tx-1",
            PlanAction::Upgrade,
            vec![pkg("a", "2-1", Some("1-1"))],
        );
        let w = World::new(&[("a", "2-1")], &[("a", "1-1")]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(p.plan.action, PlanAction::Downgrade);
        assert_eq!(p.plan.packages[0].version, "1-1");
        assert_eq!(p.plan.packages[0].previous_version.as_deref(), Some("2-1"));
        assert_eq!(p.files.len(), 1);
    }

    #[test]
    fn packages_no_longer_in_post_state_are_skipped_with_a_reason() {
        let r = rec(
            "tx-1",
            PlanAction::Upgrade,
            vec![
                pkg("moved", "2-1", Some("1-1")),
                pkg("gone", "2-1", Some("1-1")),
                pkg("ok", "2-1", Some("1-1")),
            ],
        );
        let w = World::new(&[("moved", "3-1"), ("ok", "2-1")], &[("ok", "1-1")]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(names(&p), ["ok"]);
        assert_eq!(p.skipped.len(), 2);
        assert_eq!(p.skipped[0].name, "moved");
        assert!(p.skipped[0].reason.contains("now at 3-1"));
        assert_eq!(p.skipped[1].reason, "no longer installed");
    }

    #[test]
    fn remove_record_skips_packages_that_are_installed_again() {
        let r = rec(
            "tx-1",
            PlanAction::Remove,
            vec![pkg("a", "1-1", None), pkg("b", "1-1", None)],
        );
        let w = World::new(&[("a", "1-1")], &[("b", "1-1")]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(names(&p), ["b"]);
        assert!(p.skipped[0].reason.contains("installed again"));
    }

    #[test]
    fn failed_transaction_reverses_only_what_landed() {
        let mut r = rec(
            "tx-1",
            PlanAction::Install,
            vec![pkg("landed", "1-1", None), pkg("never", "1-1", None)],
        );
        r.status = TxStatus::Failed;
        let w = World::new(&[("landed", "1-1")], &[]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(names(&p), ["landed"]);
        assert_eq!(p.skipped[0].name, "never");
    }

    #[test]
    fn nothing_applicable_gives_an_empty_plan_not_an_error() {
        let r = rec("tx-1", PlanAction::Install, vec![pkg("a", "1-1", None)]);
        let w = World::new(&[], &[]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert!(p.plan.is_empty());
        assert_eq!(p.skipped.len(), 1);
    }

    #[test]
    fn aur_versions_are_not_compared_since_rpc_metadata_can_differ_from_the_build() {
        let mut a = pkg("tool-git", "r10-1", None);
        a.repo = "aur".into();
        let r = rec("tx-1", PlanAction::Install, vec![a]);
        let w = World::new(&[("tool-git", "r12.abc-1")], &[]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert_eq!(names(&p), ["tool-git"]);
        // ...and the removal plan reports the version actually installed.
        assert_eq!(p.plan.packages[0].version, "r12.abc-1");

        let mut a = pkg("tool", "2-1", Some("1-1"));
        a.repo = "aur".into();
        let r = rec("tx-2", PlanAction::Upgrade, vec![a]);
        let w = World::new(&[("tool", "1-1")], &[("tool", "1-1")]);
        let p = w.plan(&r, std::slice::from_ref(&r)).unwrap();
        assert!(p.plan.is_empty());
        assert!(p.skipped[0].reason.contains("already back at 1-1"));
    }

    #[test]
    fn missing_files_abort_the_whole_rollback_and_are_all_listed() {
        let r = rec(
            "tx-1",
            PlanAction::Upgrade,
            vec![
                pkg("a", "2-1", Some("1-1")),
                pkg("b", "2-1", Some("1-1")),
                pkg("c", "2-1", Some("1-1")),
            ],
        );
        let w = World::new(&[("a", "2-1"), ("b", "2-1"), ("c", "2-1")], &[("b", "1-1")]);
        match w.plan(&r, std::slice::from_ref(&r)) {
            Err(Error::RollbackFilesMissing(list)) => assert_eq!(list, "a 1-1, c 1-1"),
            other => panic!("expected RollbackFilesMissing, got {other:?}"),
        }
    }

    #[test]
    fn old_upgrade_records_without_previous_versions_are_refused() {
        let r = rec("tx-1", PlanAction::Upgrade, vec![pkg("a", "2-1", None)]);
        let w = World::new(&[("a", "2-1")], &[]);
        assert!(matches!(
            w.plan(&r, std::slice::from_ref(&r)),
            Err(Error::NotRollbackable { .. })
        ));
        // Old install/remove records are fine.
        let r = rec("tx-2", PlanAction::Install, vec![pkg("a", "2-1", None)]);
        assert!(w.plan(&r, std::slice::from_ref(&r)).is_ok());
    }

    #[test]
    fn repeats_and_rollbacks_of_rollbacks_are_refused() {
        let target = rec("tx-1", PlanAction::Install, vec![pkg("a", "1-1", None)]);
        let mut undo = rec("tx-2", PlanAction::Remove, vec![pkg("a", "1-1", None)]);
        undo.rollback_of = Some("tx-1".into());
        let history = vec![target.clone(), undo.clone()];
        let w = World::new(&[("a", "1-1")], &[]);

        match w.plan(&target, &history) {
            Err(Error::NotRollbackable { why, .. }) => assert!(why.contains("tx-2")),
            other => panic!("expected refusal, got {other:?}"),
        }
        assert!(matches!(
            w.plan(&undo, &history),
            Err(Error::NotRollbackable { .. })
        ));
    }

    #[test]
    fn a_failed_rollback_does_not_count_as_rolled_back() {
        let target = rec("tx-1", PlanAction::Install, vec![pkg("a", "1-1", None)]);
        let mut attempt = rec("tx-2", PlanAction::Remove, vec![pkg("a", "1-1", None)]);
        attempt.rollback_of = Some("tx-1".into());
        attempt.status = TxStatus::Failed;
        let history = vec![target.clone(), attempt];
        let w = World::new(&[("a", "1-1")], &[]);
        assert!(w.plan(&target, &history).is_ok());
        assert_eq!(candidates(&history).len(), 1);
    }

    #[test]
    fn candidates_are_newest_first_and_skip_rollbacks_and_rolled_back() {
        let t1 = rec("tx-1", PlanAction::Install, vec![]);
        let t2 = rec("tx-2", PlanAction::Install, vec![]);
        let mut u2 = rec("tx-3", PlanAction::Remove, vec![]);
        u2.rollback_of = Some("tx-2".into());
        let t4 = rec("tx-4", PlanAction::Remove, vec![]);
        let history = vec![t1, t2, u2, t4];
        let ids: Vec<_> = candidates(&history).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["tx-4", "tx-1"]);
    }

    #[test]
    fn tx_ids_are_validated() {
        assert_eq!(parse_tx_id("tx-12").unwrap(), "tx-12");
        for bad in ["", "tx-", "tx-1a", "12", "TX-1", "tx--1", "tx-1 "] {
            assert!(
                matches!(parse_tx_id(bad), Err(Error::InvalidTransactionId(_))),
                "{bad:?}"
            );
        }
        let history = vec![rec("tx-1", PlanAction::Install, vec![])];
        assert!(find_record(&history, "tx-1").is_ok());
        assert!(matches!(
            find_record(&history, "tx-9"),
            Err(Error::UnknownTransaction(_))
        ));
    }

    #[test]
    fn package_file_matching_is_exact() {
        let dir = env::temp_dir().join(format!("emrac-rollback-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in [
            "foo-1.0-1-x86_64.pkg.tar.zst",
            "foo-1.0-1-x86_64.pkg.tar.zst.sig",
            "foo-bar-1.0-1-x86_64.pkg.tar.zst",
            "foo-1.0-10-x86_64.pkg.tar.zst",
            "foo-1:2.0-1-any.pkg.tar.xz",
            "foo-3.0-1-x86_64.pkg.tar.zst.part",
        ] {
            fs::write(dir.join(f), "").unwrap();
        }
        let dirs = [dir.join("nope"), dir.clone()];

        let hit = find_package_file(&dirs, "foo", "1.0-1").unwrap();
        assert_eq!(hit.file_name().unwrap(), "foo-1.0-1-x86_64.pkg.tar.zst");
        let hit = find_package_file(&dirs, "foo", "1:2.0-1").unwrap();
        assert_eq!(hit.file_name().unwrap(), "foo-1:2.0-1-any.pkg.tar.xz");
        assert!(find_package_file(&dirs, "foo", "3.0-1").is_none());
        assert!(find_package_file(&dirs, "fo", "1.0-1").is_none());
        assert!(find_package_file(&dirs, "bar", "1.0-1").is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}

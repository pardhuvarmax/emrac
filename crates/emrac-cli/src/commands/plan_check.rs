//! Plan integrity (u18): what was previewed is what runs. A plan has a digest
//! (`Plan::digest`); `--expect-plan` pins a run to one, and after a person has
//! had time to decide, the plan is made again just before the commit and must
//! be the same. See `docs/plan/r2/u18-plan-integrity.md`.

use std::time::{Duration, Instant};

use emrac_core::{Error, Plan, Result};

/// A plan the user may have been looking at for a while. Below this the plan
/// is not made a second time (a run with `--yes` goes straight through).
pub const RECHECK_AFTER: Duration = Duration::from_secs(30);

/// A plan that was shown, and when.
pub struct Shown {
    digest: String,
    at: Instant,
}

impl Shown {
    pub fn new(plan: &Plan) -> Shown {
        Shown {
            digest: plan.digest(),
            at: Instant::now(),
        }
    }

    #[cfg(test)]
    fn at(plan: &Plan, at: Instant) -> Shown {
        Shown {
            digest: plan.digest(),
            at,
        }
    }
}

/// `--expect-plan <digest>`: the plan about to run must start with it.
pub fn expect(plan: &Plan, expected: Option<&str>) -> Result<()> {
    let Some(want) = expected else {
        return Ok(());
    };
    let want = want.trim().to_ascii_lowercase();
    let have = plan.digest();
    if want.len() < 8 || !want.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::PlanChanged(format!(
            "'{want}' is not a plan id: they are at least 8 hex digits, printed under every plan and in `--json` as `digest`"
        )));
    }
    if have.starts_with(&want) {
        Ok(())
    } else {
        Err(Error::PlanChanged(format!(
            "this is not the plan you expected: it is {}, you asked for {want}. Nothing was changed. Run the command without --expect-plan to see the plan as it is now.",
            plan.short_digest()
        )))
    }
}

/// Just before the commit: if the plan was on screen for a while, make it
/// again and refuse to go on when it is not the one that was approved.
pub fn verify_before_commit(
    shown: &Shown,
    plan: &Plan,
    again: &dyn Fn() -> Result<Plan>,
) -> Result<()> {
    if shown.at.elapsed() < RECHECK_AFTER {
        return Ok(());
    }
    let now = again()?;
    if now.digest() == shown.digest {
        return Ok(());
    }
    let changes = plan.changes_to(&now);
    Err(Error::PlanChanged(format!(
        "the plan changed while you were deciding (it was {}, it would now be {}):\n  {}\nNothing was changed. Run the command again to see the new plan.",
        &shown.digest[..emrac_core::SHORT_DIGEST],
        now.short_digest(),
        if changes.is_empty() {
            "the details differ".to_string()
        } else {
            changes.join("\n  ")
        }
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::{PlanAction, PlannedPackage};

    fn plan(version: &str) -> Plan {
        Plan {
            action: PlanAction::Install,
            packages: vec![PlannedPackage {
                name: "a".into(),
                version: version.into(),
                repo: "extra".into(),
                explicit: true,
                previous_version: None,
            }],
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: vec![],
            provider_choices: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn an_expected_plan_id_must_match_by_prefix_and_look_like_one() {
        let p = plan("1-1");
        assert!(expect(&p, None).is_ok());
        assert!(expect(&p, Some(&p.digest())).is_ok());
        assert!(expect(&p, Some(&p.short_digest().to_uppercase())).is_ok());
        let other = plan("1-2");
        let e = expect(&p, Some(&other.short_digest()))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("not the plan you expected") && e.contains("Nothing was changed"),
            "{e}"
        );
        for bad in ["abc", "zzzzzzzzzz", ""] {
            assert!(
                expect(&p, Some(bad))
                    .unwrap_err()
                    .to_string()
                    .contains("not a plan id"),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_plan_that_was_just_shown_is_not_made_again_and_an_old_one_is() {
        let p = plan("1-1");
        let calls = std::cell::Cell::new(0);
        let same = || {
            calls.set(calls.get() + 1);
            Ok(plan("1-1"))
        };
        let fresh = Shown::new(&p);
        verify_before_commit(&fresh, &p, &same).unwrap();
        assert_eq!(calls.get(), 0, "a quick run goes straight through");
        let old = Shown::at(&p, Instant::now() - Duration::from_secs(120));
        verify_before_commit(&old, &p, &same).unwrap();
        assert_eq!(calls.get(), 1);
        let drifted = || Ok(plan("1-2"));
        let e = verify_before_commit(&old, &p, &drifted)
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("changed while you were deciding") && e.contains("changed: a 1-1 → 1-2"),
            "{e}"
        );
        assert!(e.contains("Nothing was changed"));
        let failing = || Err(Error::PlanChanged("boom".into()));
        assert!(
            verify_before_commit(&old, &p, &failing).is_err(),
            "a plan that can't be made again stops the run"
        );
    }
}

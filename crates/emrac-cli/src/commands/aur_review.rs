use emrac_core::{AurSyncOutcome, Error, Result, Sources, Verdict};

use crate::output;
use crate::prompt::{confirm, confirm_default_no};

/// Fetches the AUR package's current state and, unless `skip_pkgbuild`,
/// shows the PKGBUILD (first build) or what changed since last time
/// (rebuild), gated behind its own confirmation. Returns `false` if the
/// user declined — the caller should abort the whole operation, not just
/// skip this one package. Shared by `install` and `upgrade`, since
/// "upgrade an AUR package" and "build it for the first time" go through
/// the exact same review.
pub fn review_aur_package(
    sources: &Sources,
    name: &str,
    skip_pkgbuild: bool,
    skip_diff: bool,
    yes: bool,
    accept_risks: bool,
) -> Result<bool> {
    let outcome = sources.aur_sync(name)?;

    // The audit covers exactly what is about to be built, and runs whatever
    // the review flags say: it is what keeps `--skip-pkgbuild` from being a
    // way to build something dangerous unread.
    let report = match &outcome {
        AurSyncOutcome::Changed { new_pkgbuild, .. } => {
            sources.audit_aur_text(name, new_pkgbuild, Some("FETCH_HEAD"))
        }
        _ => sources.audit_aur_text(name, &sources.read_pkgbuild(name)?, None),
    };
    output::print_audit(&report, true);
    match report.verdict() {
        Verdict::Block => {
            return Err(Error::AuditBlocked {
                package: name.to_string(),
                findings: output::critical_summary(&report),
            });
        }
        // Deliberately not answered by `--yes`; the default is no.
        Verdict::Confirm
            if !accept_risks
                && !confirm_default_no(&format!(
                    "The audit found HIGH-risk problems in '{name}'. Build it anyway?"
                )) =>
        {
            return Ok(false);
        }
        _ => {}
    }

    match outcome {
        AurSyncOutcome::UpToDate => Ok(true),

        AurSyncOutcome::FirstClone => {
            if skip_pkgbuild {
                return Ok(true);
            }

            let pkgbuild = sources.read_pkgbuild(name)?;
            output::print_pkgbuild(name, &pkgbuild);

            if !yes
                && !confirm(&format!(
                    "Reviewed the PKGBUILD for '{name}'? Proceed with building it?"
                ))
            {
                return Ok(false);
            }
            Ok(true)
        }

        AurSyncOutcome::Changed { diff, new_pkgbuild } => {
            if !skip_pkgbuild {
                if skip_diff {
                    output::print_pkgbuild(name, &new_pkgbuild);
                } else {
                    output::print_pkgbuild_diff(name, &diff);
                }

                if !yes
                    && !confirm(&format!(
                        "Reviewed the PKGBUILD changes for '{name}'? Proceed with building it?"
                    ))
                {
                    return Ok(false);
                }
            }

            sources.aur_advance(name)?;
            Ok(true)
        }
    }
}

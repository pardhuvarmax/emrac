use emrac_core::{AurSyncOutcome, Result, Sources};

use crate::output;
use crate::prompt::confirm;

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
) -> Result<bool> {
    match sources.aur_sync(name)? {
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

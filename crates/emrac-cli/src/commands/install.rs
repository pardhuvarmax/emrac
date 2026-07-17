use emrac_core::{AurSyncOutcome, Result, Sources};

use crate::output;
use crate::prompt::confirm;

#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    pkgs: &[String],
    dry_run: bool,
    yes: bool,
    offline: bool,
    skip_pkgbuild: bool,
    skip_diff: bool,
    json: bool,
) -> Result<()> {
    let plan = sources.plan_install(pkgs, offline)?;
    output::print_plan(&plan, json);

    if plan.is_empty() || dry_run {
        return Ok(());
    }

    let (official, aur) = sources.classify_install(pkgs, offline)?;

    for details in &aur {
        if !review_aur_package(sources, &details.name, skip_pkgbuild, skip_diff, yes)? {
            println!("emrac notes: okay, nothing installed.");
            return Ok(());
        }
    }

    if !yes && !confirm("Proceed with installation?") {
        println!("emrac notes: okay, nothing installed.");
        return Ok(());
    }

    if !official.is_empty() {
        sources.install_official(&official)?;
    }
    for details in &aur {
        sources.build_aur(&details.name)?;
    }

    Ok(())
}

/// Fetches the AUR package's current state and, unless `skip_pkgbuild`,
/// shows the PKGBUILD (first build) or what changed since last time
/// (rebuild), gated behind its own confirmation. Returns `false` if the
/// user declined — the caller should abort the whole install, not just
/// skip this one package.
fn review_aur_package(
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

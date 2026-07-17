use emrac_core::{Result, Sources};

use crate::commands::aur_review::review_aur_package;
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

use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::aur_install::BuildChoice;
use crate::commands::aur_review::review_aur_package;
use crate::commands::history;
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
    choice: BuildChoice,
    inspect: bool,
    json: bool,
    allow_partial: bool,
    expect_plan: Option<&str>,
) -> Result<()> {
    let plan = sources.plan_install(pkgs, offline)?;
    output::print_plan_with_preview(&plan, &sources.preview(&plan), json);
    crate::commands::plan_check::expect(&plan, expect_plan)?;
    let shown = crate::commands::plan_check::Shown::new(&plan);

    if !plan.is_empty() {
        crate::commands::preflight::run(
            sources,
            &plan,
            &crate::commands::preflight::Options {
                dry_run,
                offline,
                json,
                allow_partial,
                check_partial: true,
                aur_builds: plan.packages.iter().any(|p| p.repo == "aur"),
            },
        )?;
    }

    if plan.is_empty() || dry_run {
        return Ok(());
    }

    let (official, aur) = sources.classify_install(pkgs, offline)?;

    for details in &aur {
        if !review_aur_package(
            sources,
            &details.name,
            skip_pkgbuild,
            skip_diff,
            yes,
            choice.accept_risks,
        )? {
            say!(Voice::Notes, "okay, nothing installed.");
            return Ok(());
        }
    }

    if !yes && !confirm("Proceed with installation?") {
        say!(Voice::Notes, "okay, nothing installed.");
        return Ok(());
    }

    crate::commands::plan_check::verify_before_commit(&shown, &plan, &|s| {
        s.plan_install(pkgs, offline)
    })?;
    history::begin(&plan)?;
    let result = (|| {
        if !official.is_empty() {
            if inspect {
                sources.download_official(&official)?;
                crate::commands::official_inspect::show(sources, &plan)?;
            }
            sources.install_official(&official)?;
        }
        for details in &aur {
            crate::commands::aur_install::build_and_install(sources, &details.name, choice)?;
        }
        Ok(())
    })();
    history::record(&plan, &result);
    result
}

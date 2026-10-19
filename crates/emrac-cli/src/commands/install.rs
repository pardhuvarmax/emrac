use emrac_core::style::Activity;
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
    mode: emrac_core::SourceMode,
) -> Result<()> {
    // package files on this machine: installed the way `pacman -U` does
    let files = pkgs
        .iter()
        .filter(|p| emrac_core::is_package_file_name(p))
        .count();
    if files > 0 {
        if files != pkgs.len() {
            return Err(emrac_core::Error::Help(
                "name package files or package names, not both in one command".into(),
            ));
        }
        return crate::commands::install_file::run(
            sources,
            pkgs,
            dry_run,
            yes,
            choice.accept_risks,
            json,
            allow_partial,
            expect_plan,
        );
    }
    let activity = Activity::spinner("working out the plan");
    let planned = sources.plan_install_with(pkgs, offline, mode);
    let by_source = planned.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    let planned = planned.map(|(plan, _)| plan);
    let preview = planned.as_ref().ok().map(|p| sources.preview(p));
    match &planned {
        Ok(_) => activity.finish("worked out the plan"),
        Err(_) => activity.fail("could not work out the plan"),
    }
    let plan = planned?;
    output::print_plan_with_preview(&plan, &preview.unwrap_or_default(), json);
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
                aur_builds: plan
                    .packages
                    .iter()
                    .any(|p| p.repo == "aur" || emrac_core::install_mode::is_source_repo(&p.repo)),
            },
        )?;
    }

    if plan.is_empty() || dry_run {
        return Ok(());
    }

    let (official, aur) = sources.classify_install(pkgs, offline)?;
    let (source_built, official): (Vec<String>, Vec<String>) =
        official.into_iter().partition(|n| by_source.contains(n));

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
        s.plan_install_with(pkgs, offline, mode)
            .map(|(plan, _)| plan)
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
        for name in &source_built {
            crate::commands::source_install::build_and_install(sources, name, choice)?;
        }
        for details in &aur {
            crate::commands::aur_install::build_and_install(sources, &details.name, choice)?;
        }
        Ok(())
    })();
    history::record(&plan, &result);
    if result.is_ok() && mode == emrac_core::SourceMode::Binary {
        // the binary was asked for explicitly: the source policy ends
        let names: Vec<String> = plan.packages.iter().map(|p| p.name.clone()).collect();
        let ended = sources.end_source_policy(&names).unwrap_or_else(|err| {
            say!(Voice::Warns, "{err}");
            false
        });
        if ended {
            say!(
                Voice::Notes,
                "the source-build policy of those packages ended: they follow the repository's binary."
            );
        }
    }
    result
}

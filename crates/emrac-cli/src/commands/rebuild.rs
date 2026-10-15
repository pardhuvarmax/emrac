use emrac_core::stale_builds::{is_binary_package, BINARY_NOTICE};
use emrac_core::{say, Error, PlanAction, Result, Sources, Voice};

use crate::commands::aur_install::BuildChoice;
use crate::commands::aur_review::review_aur_package;
use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

/// `emrac rebuild <pkg>...`: build installed AUR packages again from source.
#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    pkgs: &[String],
    check: bool,
    stale: bool,
    dry_run: bool,
    yes: bool,
    offline: bool,
    skip_pkgbuild: bool,
    skip_diff: bool,
    choice: BuildChoice,
    json: bool,
) -> Result<()> {
    if check {
        output::print_stale(&sources.stale_builds(), json);
        return Ok(());
    }
    let names: Vec<String> = if stale {
        let all = sources.stale_builds();
        let skipped: Vec<&str> = all
            .iter()
            .filter(|s| !s.broken.is_empty() && s.binary)
            .map(|s| s.name.as_str())
            .collect();
        if !skipped.is_empty() && !json {
            say!(
                Voice::Notes,
                "left out (binary packages, a rebuild would not fix them): {}. {}",
                skipped.join(", "),
                BINARY_NOTICE
            );
        }
        let broken: Vec<String> = all
            .into_iter()
            .filter(|s| !s.broken.is_empty() && !s.binary)
            .map(|s| s.name)
            .collect();
        if broken.is_empty() {
            say!(
                Voice::Notes,
                "no installed AUR package is broken right now; `emrac rebuild --check` also lists the ones that may break."
            );
            return Ok(());
        }
        broken
    } else if pkgs.is_empty() {
        return Err(Error::Rebuild(
            "name the packages to rebuild, or use --check to see which ones need it, or --stale to rebuild the broken ones".into(),
        ));
    } else {
        pkgs.to_vec()
    };
    let binary: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| is_binary_package(n))
        .collect();
    if !binary.is_empty() && !json {
        say!(
            Voice::Warns,
            "{} {} a binary package. {}",
            binary.join(", "),
            if binary.len() == 1 { "is" } else { "are" },
            BINARY_NOTICE
        );
    }
    let mut plan = sources.plan_rebuild(&names)?;
    output::print_rebuild(&plan, json);
    if offline && !dry_run {
        return Err(Error::Rebuild(
            "a rebuild fetches the sources again, and --offline is set".into(),
        ));
    }

    crate::commands::preflight::run(
        sources,
        &plan,
        &crate::commands::preflight::Options {
            dry_run,
            offline,
            json,
            allow_partial: false,
            check_partial: false,
            aur_builds: true,
        },
    )?;
    if dry_run {
        return Ok(());
    }

    for p in &plan.packages {
        if !review_aur_package(
            sources,
            &p.name,
            skip_pkgbuild,
            skip_diff,
            yes,
            choice.accept_risks,
        )? {
            say!(Voice::Notes, "okay, nothing rebuilt.");
            return Ok(());
        }
    }
    if !yes && !confirm("Proceed with the rebuild?") {
        say!(Voice::Notes, "okay, nothing rebuilt.");
        return Ok(());
    }

    history::begin(&plan)?;
    let result = (|| {
        for p in &plan.packages {
            crate::commands::aur_install::build_and_install(sources, &p.name, choice)?;
        }
        Ok(())
    })();
    // A rebuild of a VCS package, or of one the AUR has moved on, installs a
    // different version: record what is installed now, and call it an upgrade.
    if result.is_ok() {
        let mut changed = false;
        for p in &mut plan.packages {
            if let Some(now) = sources.installed_version(&p.name) {
                changed |= Some(&now) != p.previous_version.as_ref();
                p.version = now;
            }
        }
        if changed {
            plan.action = PlanAction::Upgrade;
        }
    }
    history::record(&plan, &result);
    result
}

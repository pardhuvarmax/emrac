use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    pkgs: &[String],
    cascade: bool,
    recursive: bool,
    dry_run: bool,
    yes: bool,
    json: bool,
    expect_plan: Option<&str>,
) -> Result<()> {
    let purge = sources.plan_purge(pkgs, cascade, recursive)?;
    output::print_purge(&purge, json);
    crate::commands::plan_check::expect(&purge.plan, expect_plan)?;
    let shown = crate::commands::plan_check::Shown::new(&purge.plan);

    if !purge.plan.is_empty() {
        crate::commands::preflight::run(
            sources,
            &purge.plan,
            &crate::commands::preflight::Options {
                dry_run,
                offline: false,
                json,
                allow_partial: false,
                check_partial: false,
                aur_builds: false,
            },
        )?;
    }
    if purge.plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the purge?") {
        say!(Voice::Notes, "okay, nothing purged.");
        return Ok(());
    }

    // `remove` would have kept these as .pacsave; purge destroys them, so
    // that gets its own confirmation, same idea as the AUR PKGBUILD review.
    let at_risk = purge.at_risk().len();
    if at_risk > 0 && !yes {
        let question = format!(
            "{} will be deleted for good instead of saved as .pacsave. Delete {}?",
            plural_files(at_risk),
            if at_risk == 1 { "it" } else { "them" }
        );
        if !confirm(&question) {
            say!(
                Voice::Notes,
                "okay, nothing purged. (`emrac remove` would keep them as .pacsave.)"
            );
            return Ok(());
        }
    }

    crate::commands::plan_check::verify_before_commit(&shown, &purge.plan, &|| {
        sources.plan_purge(pkgs, cascade, recursive).map(|p| p.plan)
    })?;
    history::begin(&purge.plan)?;
    let result = sources.purge(pkgs, cascade, recursive);
    history::record(&purge.plan, &result);
    result
}

fn plural_files(n: usize) -> String {
    if n == 1 {
        "1 config file with local changes (or that couldn't be checked)".to_string()
    } else {
        format!("{n} config files with local changes (or that couldn't be checked)")
    }
}

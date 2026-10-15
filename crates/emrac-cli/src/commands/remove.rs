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
    let plan = sources.plan_remove(pkgs, cascade, recursive)?;
    output::print_plan(&plan, json);
    crate::commands::plan_check::expect(&plan, expect_plan)?;
    let shown = crate::commands::plan_check::Shown::new(&plan);

    if !plan.is_empty() {
        crate::commands::preflight::run(
            sources,
            &plan,
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
    if plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with removal?") {
        say!(Voice::Notes, "okay, nothing removed.");
        return Ok(());
    }

    crate::commands::plan_check::verify_before_commit(&shown, &plan, &|s| {
        s.plan_remove(pkgs, cascade, recursive)
    })?;
    history::begin(&plan)?;
    let result = sources.remove(pkgs, cascade, recursive);
    history::record(&plan, &result);
    result
}

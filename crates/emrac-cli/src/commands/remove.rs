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
) -> Result<()> {
    let plan = sources.plan_remove(pkgs, cascade, recursive)?;
    output::print_plan(&plan, json);

    if plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with removal?") {
        say!(Voice::Notes, "okay, nothing removed.");
        return Ok(());
    }

    history::begin(&plan)?;
    let result = sources.remove(pkgs, cascade, recursive);
    history::record(&plan, &result);
    result
}

use emrac_core::{Result, Sources};

use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    pkgs: &[String],
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    let plan = sources.plan_install(pkgs)?;
    output::print_plan(&plan, json);

    if plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with installation?") {
        println!("emrac notes: okay, nothing installed.");
        return Ok(());
    }

    sources.install(pkgs)?;
    Ok(())
}

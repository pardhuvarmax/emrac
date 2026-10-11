use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    tx: Option<&str>,
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    let rollback = sources.plan_rollback(tx)?;
    output::print_rollback(&rollback, json);

    if rollback.plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the rollback?") {
        println!("emrac notes: okay, nothing rolled back.");
        return Ok(());
    }

    let result = sources.execute_rollback(&rollback);
    history::record_rollback(&rollback, &result);
    result
}

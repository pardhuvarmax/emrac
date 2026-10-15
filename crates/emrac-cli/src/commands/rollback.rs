use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

/// What to reverse: a named or the latest transaction, or the latest rollback.
pub enum Which<'a> {
    Rollback(Option<&'a str>),
    Redo,
}

pub fn run(
    sources: &Sources,
    which: Which,
    dry_run: bool,
    yes: bool,
    offline: bool,
    json: bool,
) -> Result<()> {
    let rollback = match which {
        Which::Rollback(tx) => sources.plan_rollback(tx, offline)?,
        Which::Redo => sources.plan_redo(offline)?,
    };
    output::print_rollback(&rollback, json);

    if rollback.plan.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the rollback?") {
        say!(Voice::Notes, "okay, nothing rolled back.");
        return Ok(());
    }

    history::begin(&rollback.plan)?;
    let result = sources.execute_rollback(&rollback);
    history::record_rollback(&rollback, &result);
    result
}

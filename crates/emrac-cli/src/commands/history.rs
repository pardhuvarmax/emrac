use emrac_core::{parse_date, History, Plan, Result};

use crate::output;

pub fn run(limit: Option<usize>, since: Option<&str>, json: bool) -> Result<()> {
    let since = since.map(parse_date).transpose()?;
    let records = History::list(limit, since)?;
    output::print_history(&records, json);
    Ok(())
}

/// Logs an executed transaction. Never fails the command it follows: the
/// transaction's own result is what the user cares about, so a history
/// write problem is only a warning.
pub fn record(plan: &Plan, outcome: &Result<()>) {
    if let Err(err) = History::record(plan, outcome) {
        eprintln!("emrac warns: couldn't record this in your history — {err}");
    }
}

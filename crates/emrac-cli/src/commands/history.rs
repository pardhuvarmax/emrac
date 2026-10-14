use emrac_core::{say, Voice};
use std::collections::HashMap;

use emrac_core::{parse_date, rolled_back_by, History, Plan, Result, RollbackPlan};

use crate::output;

pub fn run(limit: Option<usize>, since: Option<&str>, json: bool) -> Result<()> {
    let since = since.map(parse_date).transpose()?;
    let records = History::list(limit, since)?;
    // Markers need the whole log: the rollback that reversed a record may be
    // newer than the `--since` window or past `--limit`.
    let all = History::all()?;
    let rolled_back: HashMap<String, String> = records
        .iter()
        .filter_map(|r| rolled_back_by(&all, &r.id).map(|by| (r.id.clone(), by.id.clone())))
        .collect();
    output::print_history(&records, &rolled_back, json);
    Ok(())
}

/// Logs an executed transaction. Never fails the command it follows: the
/// transaction's own result is what the user cares about, so a history
/// write problem is only a warning.
pub fn record(plan: &Plan, outcome: &Result<()>) {
    log(plan, outcome, None);
}

/// Same as `record`, for a rollback: also notes which transaction it reversed.
pub fn record_rollback(rollback: &RollbackPlan, outcome: &Result<()>) {
    log(&rollback.plan, outcome, Some(&rollback.target));
}

fn log(plan: &Plan, outcome: &Result<()>, rollback_of: Option<&str>) {
    if let Err(err) = History::record(plan, outcome, rollback_of) {
        say!(Voice::Warns, "couldn't record this in your history — {err}");
    }
}

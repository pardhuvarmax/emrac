use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    name: &str,
    prune: bool,
    dry_run: bool,
    yes: bool,
    offline: bool,
    json: bool,
) -> Result<()> {
    let restore = sources.plan_restore(name, prune, offline)?;
    output::print_restore(&restore, json);

    if restore.is_empty() || dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the restore?") {
        say!(Voice::Notes, "okay, nothing restored.");
        return Ok(());
    }

    sources.execute_restore(&restore, &mut |plan, result| history::record(plan, result))
}

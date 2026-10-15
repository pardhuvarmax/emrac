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

    // One journal for the whole restore; each step is recorded as it ends.
    let mut all = emrac_core::Plan {
        action: emrac_core::PlanAction::Upgrade,
        packages: Vec::new(),
        total_download_size: 0,
        total_installed_size_delta: 0,
        removals: Vec::new(),
        provider_choices: Vec::new(),
        warnings: Vec::new(),
    };
    for e in &restore.entries {
        all.packages.push(emrac_core::PlannedPackage {
            name: e.item.name.clone(),
            version: e.item.to.clone(),
            repo: if e.item.foreign { "aur" } else { "restore" }.to_string(),
            explicit: true,
            previous_version: e.item.from.clone(),
        });
    }
    history::begin(&all)?;
    let result = sources.execute_restore(&restore, &mut |plan, result| {
        history::record_step(plan, result)
    });
    emrac_core::journal::finish();
    result
}

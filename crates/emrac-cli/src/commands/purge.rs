use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    pkgs: &[String],
    cascade: bool,
    recursive: bool,
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    let purge = sources.plan_purge(pkgs, cascade, recursive)?;
    output::print_purge(&purge, json);

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

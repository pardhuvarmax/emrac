use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    pkg: &str,
    version: Option<&str>,
    dry_run: bool,
    yes: bool,
    offline: bool,
    json: bool,
) -> Result<()> {
    let downgrade = sources.plan_downgrade(pkg, version, offline)?;
    output::print_downgrade(&downgrade, json);

    if dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the downgrade?") {
        say!(Voice::Notes, "okay, nothing downgraded.");
        return Ok(());
    }

    history::begin(&downgrade.plan)?;
    let result = sources.execute_downgrade(&downgrade);
    history::record(&downgrade.plan, &result);
    result?;

    if !json {
        say!(
            Voice::Suggests,
            "the next `emrac upgrade` will bring {pkg} forward again — add it to IgnorePkg in /etc/pacman.conf to hold it here."
        );
    }
    Ok(())
}

pub fn list(sources: &Sources, pkg: &str, offline: bool, json: bool) -> Result<()> {
    let versions = sources.list_versions(pkg, offline)?;
    output::print_versions(&versions, json);
    Ok(())
}

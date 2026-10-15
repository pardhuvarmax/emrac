use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::aur_install::BuildChoice;
use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    pkgs: &[String],
    dry_run: bool,
    yes: bool,
    offline: bool,
    choice: BuildChoice,
    json: bool,
) -> Result<()> {
    let reinstall = sources.plan_reinstall(pkgs, offline)?;
    output::print_reinstall(&reinstall, json);

    if dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the reinstall?") {
        say!(Voice::Notes, "okay, nothing reinstalled.");
        return Ok(());
    }

    history::begin(&reinstall.plan)?;
    let result = sources.execute_reinstall(&reinstall).and_then(|()| {
        for name in &reinstall.rebuild {
            crate::commands::aur_install::build_and_install(sources, name, choice)?;
        }
        Ok(())
    });
    history::record(&reinstall.plan, &result);
    result
}

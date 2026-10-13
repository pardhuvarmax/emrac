use emrac_core::{Result, Sources};

use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

pub fn run(
    sources: &Sources,
    pkgs: &[String],
    dry_run: bool,
    yes: bool,
    offline: bool,
    accept_risks: bool,
    json: bool,
) -> Result<()> {
    let reinstall = sources.plan_reinstall(pkgs, offline)?;
    output::print_reinstall(&reinstall, json);

    if dry_run {
        return Ok(());
    }

    if !yes && !confirm("Proceed with the reinstall?") {
        println!("emrac notes: okay, nothing reinstalled.");
        return Ok(());
    }

    let result = sources.execute_reinstall(&reinstall).and_then(|()| {
        for name in &reinstall.rebuild {
            crate::commands::aur_install::build_and_install(sources, name, accept_risks)?;
        }
        Ok(())
    });
    history::record(&reinstall.plan, &result);
    result
}

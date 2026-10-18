use emrac_core::style::Activity;
use emrac_core::{Result, Sources};

use crate::output;

pub fn run(sources: &Sources, pkg: &str, offline: bool, json: bool) -> Result<()> {
    let activity = Activity::spinner(&format!("looking up {pkg}"));
    let looked = sources.info(pkg, offline);
    match &looked {
        Ok(_) => activity.finish(&format!("found {pkg}")),
        Err(_) => activity.fail(&format!("could not look up {pkg}")),
    }
    let details = looked?;
    output::print_package_details(&details, json);
    if details.repo == "aur" && !json {
        println!();
        let signals = sources.aur_trust_for_info(&details);
        if signals.is_empty() {
            println!("Trust signals   : none (the AUR's data and the name raise no flag)");
        } else {
            output::print_trust(&details.name, &signals);
        }
    }
    Ok(())
}

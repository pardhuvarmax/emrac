use emrac_core::{Result, Sources};

use crate::output;

pub fn run(sources: &Sources, pkg: &str, offline: bool, json: bool) -> Result<()> {
    let details = sources.info(pkg, offline)?;
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

use anyhow::Result;
use emrac_core::Sources;

use crate::output;

pub fn run(sources: &Sources, pkg: &str, offline: bool, json: bool) -> Result<()> {
    let details = sources.info(pkg, offline)?;
    output::print_package_details(&details, json);
    Ok(())
}

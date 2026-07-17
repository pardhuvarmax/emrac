use anyhow::Result;
use emrac_core::AlpmBackend;

use crate::output;

pub fn run(backend: &AlpmBackend, pkg: &str, json: bool) -> Result<()> {
    let details = backend.info(pkg)?;
    output::print_package_details(&details, json);
    Ok(())
}

use emrac_core::{Error, Result, Sources};

use crate::output;

pub fn run(sources: &Sources, pkgs: &[String], json: bool) -> Result<()> {
    let report = sources.verify(pkgs)?;
    output::print_verify(&report, json);

    // Fail after reporting, so scripts can use the exit status. Edited
    // config files and unreadable files are not problems.
    let broken = report.broken();
    if broken.is_empty() {
        return Ok(());
    }
    Err(Error::VerificationFailed {
        count: broken.len(),
        names: broken
            .iter()
            .map(|p| p.package.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    })
}

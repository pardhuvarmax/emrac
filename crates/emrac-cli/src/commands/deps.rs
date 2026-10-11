use emrac_core::{Result, Sources};

use crate::output;

pub fn deps(sources: &Sources, pkg: &str, recursive: bool, json: bool) -> Result<()> {
    output::print_deps(&sources.deps(pkg, recursive)?, json);
    Ok(())
}

pub fn rdeps(sources: &Sources, pkg: &str, json: bool) -> Result<()> {
    output::print_rdeps(&sources.rdeps(pkg)?, json);
    Ok(())
}

pub fn why(sources: &Sources, pkg: &str, json: bool) -> Result<()> {
    output::print_why(&sources.why(pkg)?, json);
    Ok(())
}

pub fn orphans(sources: &Sources, json: bool) -> Result<()> {
    output::print_orphans(&sources.orphans(), json);
    Ok(())
}

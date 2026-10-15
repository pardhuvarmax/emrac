use emrac_core::{Result, Sources};

use crate::cli::GraphFormat;

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

pub fn tree(
    sources: &Sources,
    pkg: &str,
    depth: Option<usize>,
    optional: bool,
    json: bool,
) -> Result<()> {
    output::print_tree(&sources.tree(pkg, depth, optional)?, json);
    Ok(())
}

pub fn graph(
    sources: &Sources,
    pkg: &str,
    depth: Option<usize>,
    optional: bool,
    format: GraphFormat,
    json: bool,
) -> Result<()> {
    let report = sources.graph(pkg, depth, optional)?;
    match (format, json) {
        (GraphFormat::Dot, false) => print!("{}", report.to_dot()),
        (GraphFormat::Json, _) | (_, true) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap())
        }
        (GraphFormat::Text, false) => output::print_graph(&report),
    }
    Ok(())
}

pub fn cycles(sources: &Sources, json: bool) -> Result<()> {
    output::print_cycles(&sources.cycles(), json);
    Ok(())
}

/// `emrac impact <pkg>`: removal by default; `--install` and `--upgrade` show
/// the plan the real command would make.
pub fn impact(
    sources: &Sources,
    pkg: &str,
    install: bool,
    upgrade: bool,
    offline: bool,
    json: bool,
) -> Result<()> {
    if install {
        let plan = sources.plan_install(&[pkg.to_string()], offline)?;
        output::print_plan(&plan, json);
    } else if upgrade {
        let plan = sources.plan_upgrade(&[pkg.to_string()], offline)?;
        output::print_plan(&plan.plan, json);
    } else {
        output::print_impact_remove(&sources.impact_remove(pkg)?, json);
    }
    Ok(())
}

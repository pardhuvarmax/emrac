use emrac_core::{Error, Result, Sources};

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
        (GraphFormat::Boxes, false) => {
            let drawing = emrac_core::graph_render::render(&report).map_err(Error::Graph)?;
            print!("{drawing}");
        }
        (GraphFormat::Easy, false) => print!("{}", through_graph_easy(&report.to_dot())?),
    }
    Ok(())
}

/// The dot form drawn by `graph-easy`, which emrac does not ship.
fn through_graph_easy(dot: &str) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // The Arch package keeps it in /usr/bin/vendor_perl, which is on PATH only
    // in a login shell.
    let program = ["graph-easy", "/usr/bin/vendor_perl/graph-easy"]
        .into_iter()
        .find(|p| {
            Command::new(p)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        })
        .unwrap_or("graph-easy");
    let mut child = Command::new(program)
        .args(["--from=dot", "--as=boxart"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Graph(
                    "graph-easy is not installed (the AUR package perl-graph-easy); \
                     `--format boxes` draws the same thing without it"
                        .into(),
                )
            } else {
                Error::Graph(format!("couldn't run graph-easy: {e}"))
            }
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(dot.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Error::Graph(format!("graph-easy: {e}")))?;
    if !out.status.success() {
        return Err(Error::Graph(format!(
            "graph-easy failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

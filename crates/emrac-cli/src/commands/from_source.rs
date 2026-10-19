//! `emrac from-source`: the packages you build from source (`install --source`),
//! as a policy that an upgrade carries out, kept apart from pins.

use emrac_core::source_built::{installed, state_of, BuiltState};
use emrac_core::{say, Error, Result, Sources, Voice};

use crate::cli::FromSourceCommand;
use crate::commands::aur_install::BuildChoice;

pub fn run(
    sources: &Sources,
    command: Option<&FromSourceCommand>,
    choice: BuildChoice,
    json: bool,
) -> Result<()> {
    match command.cloned().unwrap_or(FromSourceCommand::List) {
        FromSourceCommand::List => list(sources, json),
        FromSourceCommand::Add { pkgs } => {
            for name in &pkgs {
                if sources.installed_version(name).is_none() {
                    return Err(Error::PackageNotInstalled(name.clone()));
                }
                sources.arch_source(name)?;
            }
            sources.declare_source_policy(&pkgs)?;
            say!(
                Voice::Confirms,
                "{} will be built from source: by the next upgrade, or now with `emrac from-source rebuild`.",
                pkgs.join(", ")
            );
            Ok(())
        }
        FromSourceCommand::Drop { pkgs } => {
            if sources.end_source_policy(&pkgs)? {
                say!(
                    Voice::Confirms,
                    "{} follow the repository's binary again; what is installed is left as it is.",
                    pkgs.join(", ")
                );
            } else {
                say!(Voice::Notes, "none of those had a source-build policy.");
            }
            Ok(())
        }
        FromSourceCommand::Rebuild { pkgs, accept_risks } => {
            let mut choice = choice;
            choice.accept_risks |= accept_risks;
            let policy = sources.source_policy()?;
            let targets: Vec<String> = if pkgs.is_empty() {
                policy
                    .iter()
                    .filter(|e| {
                        matches!(
                            state_of(e, installed(&e.name).as_ref()),
                            BuiltState::Pending
                                | BuiltState::Mismatch { .. }
                                | BuiltState::RepositoryDetected
                        )
                    })
                    .map(|e| e.name.clone())
                    .collect()
            } else {
                pkgs
            };
            if targets.is_empty() {
                say!(Voice::Notes, "nothing waits to be built from source.");
                return Ok(());
            }
            for name in &targets {
                if !policy.iter().any(|e| &e.name == name) {
                    sources.declare_source_policy(std::slice::from_ref(name))?;
                }
                // a transaction like any other: the lock, the journal, the history
                let src = sources.arch_source(name)?;
                let plan = sources.source_rebuild_plan(
                    name,
                    &src.version,
                    &format!("{}{}", src.repo, emrac_core::install_mode::SOURCE_SUFFIX),
                );
                crate::commands::history::begin(&plan)?;
                let built =
                    crate::commands::source_install::build_and_install(sources, name, choice);
                crate::commands::history::record(&plan, &built);
                built?;
            }
            Ok(())
        }
    }
}

fn list(sources: &Sources, json: bool) -> Result<()> {
    let policy = sources.source_policy()?;
    let rows: Vec<(String, String, &'static str, String)> = policy
        .iter()
        .map(|e| {
            let now = installed(&e.name);
            let (key, text) = match state_of(e, now.as_ref()) {
                BuiltState::SourceVerified => (
                    "source-verified",
                    format!("source verified: built here, {} (the package database agrees)", e.version),
                ),
                BuiltState::RepositoryDetected => (
                    "repository-detected",
                    format!(
                        "repository file detected: {} is installed but its build is not the one made here; `emrac from-source rebuild {}`",
                        e.version, e.name
                    ),
                ),
                BuiltState::OriginUnverified => (
                    "origin-unverified",
                    "origin unverified: the record has no build evidence (rebuild to establish it)"
                        .to_string(),
                ),
                BuiltState::Pending => (
                    "rebuild-pending",
                    "rebuild pending: the repository's binary is installed".to_string(),
                ),
                BuiltState::Mismatch { installed } => (
                    "policy-mismatch",
                    format!(
                        "policy mismatch: {installed} is installed, the one built here was {}",
                        if e.version.is_empty() { "never" } else { &e.version }
                    ),
                ),
                BuiltState::NotInstalled => ("not-installed", "not installed".to_string()),
            };
            (
                e.name.clone(),
                now.map(|i| i.version).unwrap_or_default(),
                key,
                text,
            )
        })
        .collect();
    if json {
        let v: Vec<_> = rows
            .iter()
            .zip(&policy)
            .map(|((n, i, k, t), e)| {
                serde_json::json!({"name": n, "installed": i, "status": k, "state": t,
                    "built_version": e.version, "recipe": e.recipe, "pending": e.pending,
                    "build_date": e.build_date, "packager": e.packager, "at": e.at})
            })
            .collect();
        println!("{}", serde_json::Value::Array(v));
        return Ok(());
    }
    if rows.is_empty() {
        say!(
            Voice::Notes,
            "no package is set to be built from source (`emrac install --source <pkg>`)."
        );
        return Ok(());
    }
    for (name, installed, _, text) in rows {
        println!("{name:<24} {installed:<16} {text}");
    }
    Ok(())
}

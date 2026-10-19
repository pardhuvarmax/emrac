//! `emrac from-source`: the packages you build from source (`install --source`),
//! as a policy that an upgrade carries out, kept apart from pins.

use emrac_core::source_built::{state_of, BuiltState};
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
            let policy = sources.source_policy();
            let targets: Vec<String> = if pkgs.is_empty() {
                policy
                    .iter()
                    .filter(|e| {
                        matches!(
                            state_of(e, sources.installed_version(&e.name).as_deref()),
                            BuiltState::Pending | BuiltState::Unknown { .. }
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
                crate::commands::source_install::build_and_install(sources, name, choice)?;
            }
            Ok(())
        }
    }
}

fn list(sources: &Sources, json: bool) -> Result<()> {
    let policy = sources.source_policy();
    let rows: Vec<(String, String, String)> =
        policy
            .iter()
            .map(|e| {
                let installed = sources.installed_version(&e.name);
                let state =
                    match state_of(e, installed.as_deref()) {
                        BuiltState::BuiltHere => format!("built here ({})", e.version),
                        BuiltState::Pending => {
                            "rebuild pending (the repository's binary is installed)".into()
                        }
                        BuiltState::Unknown { installed } => format!(
                    "unknown build intent: {installed} is installed, the one built here was {}",
                    if e.version.is_empty() { "never" } else { &e.version }
                ),
                        BuiltState::NotInstalled => "not installed".into(),
                    };
                (e.name.clone(), installed.unwrap_or_default(), state)
            })
            .collect();
    if json {
        let v: Vec<_> = rows
            .iter()
            .zip(&policy)
            .map(|((n, i, s), e)| {
                serde_json::json!({"name": n, "installed": i, "state": s,
                    "built_version": e.version, "recipe": e.recipe, "pending": e.pending, "at": e.at})
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
    for (name, installed, state) in rows {
        println!("{name:<24} {installed:<16} {state}");
    }
    Ok(())
}

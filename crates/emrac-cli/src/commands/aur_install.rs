use emrac_core::{
    inspect_package, BuildConfig, BuildOptions, Error, Limits, Result, Sources, Verdict,
};
use emrac_core::{say, Voice};

use crate::cli::AurReviewFlags;
use crate::output;
use crate::prompt::confirm_default_no;

/// What to do with the packages a build installed only to build.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum RemoveTools {
    /// List them and ask (never answered by `--yes`).
    #[default]
    Ask,
    Always,
    Never,
}

impl RemoveTools {
    pub fn from_config(build: &BuildConfig) -> Self {
        match build.remove_make_deps.as_str() {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Ask,
        }
    }
}

/// The safety switches that apply when an AUR package is built and installed.
#[derive(Clone, Copy, Default)]
pub struct BuildChoice {
    /// Memory, process, size and time limits for the sandboxed steps.
    pub limits: Limits,
    /// Proceed past high-risk audit/manifest findings without asking.
    pub accept_risks: bool,
    /// Build without the sandbox (explicit consent; never a config key).
    pub no_sandbox: bool,
    /// Give the sandboxed build network access without asking.
    pub allow_network: bool,
    /// Let pacman ask its own questions (no `--yes`).
    pub interactive: bool,
    /// Build again although a package file for this version exists.
    pub force: bool,
    /// The build tools it leaves behind: ask, remove, or leave.
    pub remove_tools: RemoveTools,
}

impl BuildChoice {
    pub fn from_flags(flags: &AurReviewFlags, build: &BuildConfig, interactive: bool) -> Self {
        Self {
            interactive,
            limits: Limits::from_config(build),
            accept_risks: flags.accept_risks,
            no_sandbox: flags.no_sandbox,
            allow_network: flags.allow_network,
            force: false,
            remove_tools: RemoveTools::from_config(build),
        }
    }
}

/// How to run the build: sandboxed unless `--no-sandbox`, and with the
/// network only if allowed. A package the audit expects to use the network
/// gets a warn-and-ask (default no; `--yes` does not answer it).
pub fn build_options(
    sources: &Sources,
    name: &str,
    predicts_network: bool,
    choice: BuildChoice,
) -> Result<BuildOptions> {
    if choice.no_sandbox {
        say!(
            Voice::Warns,
            "building '{name}' WITHOUT the sandbox (--no-sandbox): the build runs with your full access."
        );
        return Ok(BuildOptions {
            sandbox: false,
            allow_network: true,
            limits: choice.limits,
            interactive: choice.interactive,
            force: choice.force,
        });
    }
    sources.sandbox_check()?;
    let allow_network = choice.allow_network
        || (predicts_network
            && confirm_default_no(&format!(
                "'{name}' looks like it uses the network while building (see the audit). The sandbox blocks that. Allow network access for this build?"
            )));
    Ok(BuildOptions {
        sandbox: true,
        allow_network,
        limits: choice.limits,
        interactive: choice.interactive,
        force: choice.force,
    })
}

/// Builds an AUR package. If the sandboxed build fails in a way the missing
/// network may explain, asks (default no; `--yes` does not answer) whether
/// to retry with the network on. A dependency conflict that a swap of
/// packages would settle is offered the same way.
pub fn build_with_retry(
    sources: &Sources,
    name: &str,
    opts: BuildOptions,
) -> Result<Vec<std::path::PathBuf>> {
    match sources.build_aur_package(name, opts) {
        // The replacement provides what the old package did: offer the swap
        // (default no; `--yes` does not answer it) and carry on building.
        Err(Error::DependencyConflict(conflict)) if conflict.likely_safe() => {
            say!(Voice::Thinks, "{conflict}");
            if !confirm_default_no(&format!(
                "Swap '{}' for '{}' now, then continue building '{name}'?",
                conflict.removed, conflict.by
            )) {
                return Err(Error::DependencySwapDeclined {
                    package: name.to_string(),
                });
            }
            sources.apply_dependency_swap(&conflict)?;
            sources.build_aur_package(name, opts)
        }
        Err(err)
            if opts.sandbox
                && !opts.allow_network
                && err.may_need_network()
                && {
                    say!(
                        Voice::Suspects,
                        "the build of '{name}' failed because the sandbox has no network (its output mentions a host it could not resolve)."
                    );
                    confirm_default_no("Retry with network access for this build?")
                } =>
        {
            sources.build_aur_package(
                name,
                BuildOptions {
                    allow_network: true,
                    ..opts
                },
            )
        }
        result => result,
    }
}

/// Builds an AUR package, shows what the built package would change, and
/// installs it — unless the manifest says not to. This is the only path by
/// which an AUR package reaches `pacman -U`.
///
/// Critical findings in the built package: refused, no override. High ones:
/// the default-no prompt that `--yes` does not answer (`--accept-risks`
/// does). Anything else installs without a further question: the user
/// already confirmed the install.
pub fn build_and_install(sources: &Sources, name: &str, choice: BuildChoice) -> Result<()> {
    let accept_risks = choice.accept_risks;
    // What is installed before the build, to tell afterwards what the build added.
    let before = if choice.remove_tools == RemoveTools::Never {
        std::collections::HashSet::new()
    } else {
        sources.installed_names_set()
    };
    let opts = build_options(sources, name, sources.predicts_network(name), choice)?;
    let files = build_with_retry(sources, name, opts)?;

    let mut worst = Verdict::Allow;
    let mut blocked: Option<(String, String, String)> = None;
    let mut risky: Option<(String, String)> = None;
    for file in &files {
        let manifest = inspect_package(file)?;
        output::print_manifest(&manifest, true);
        match manifest.report.verdict() {
            Verdict::Block if blocked.is_none() => {
                blocked = Some((
                    manifest.package.clone(),
                    output::critical_summary(&manifest.report),
                    file.display().to_string(),
                ));
                worst = Verdict::Block;
            }
            Verdict::Confirm if risky.is_none() => {
                risky = Some((manifest.package.clone(), file.display().to_string()));
                if worst == Verdict::Allow {
                    worst = Verdict::Confirm;
                }
            }
            _ => {}
        }
    }

    if let Some((package, findings, file)) = blocked {
        return Err(Error::ManifestBlocked {
            package,
            findings,
            file,
        });
    }
    if let (Verdict::Confirm, Some((package, file))) = (worst, risky) {
        if !accept_risks
            && !confirm_default_no(&format!(
                "The built package '{package}' makes HIGH-risk changes (see above). Install it anyway?"
            ))
        {
            return Err(Error::InstallDeclined { package, file });
        }
    }

    sources.install_built(&files)?;
    sources.aur_trust_commit(name);
    note_build(name, before, choice);
    Ok(())
}

/// What the builds of this run added, kept until the run's transactions are
/// finished: the removal is a transaction of its own and cannot start inside
/// another one (the journal allows one at a time).
struct PendingTools {
    /// Installed before the first build of this run.
    before: std::collections::HashSet<String>,
    built: Vec<String>,
    mode: RemoveTools,
    interactive: bool,
}

static PENDING_TOOLS: std::sync::Mutex<Option<PendingTools>> = std::sync::Mutex::new(None);

/// Remembers that `name` was just built and installed.
fn note_build(name: &str, before: std::collections::HashSet<String>, choice: BuildChoice) {
    if choice.remove_tools == RemoveTools::Never {
        return;
    }
    let mut pending = PENDING_TOOLS.lock().unwrap();
    match pending.as_mut() {
        // Several packages in one run: the earliest snapshot covers all of them.
        Some(p) => p.built.push(name.to_string()),
        None => {
            *pending = Some(PendingTools {
                before,
                built: vec![name.to_string()],
                mode: choice.remove_tools,
                interactive: choice.interactive,
            })
        }
    }
}

/// At the end of a run that built AUR packages: the packages the builds added
/// as dependencies that nothing needs any more (compilers, `cmake`, ...). Asks,
/// removes or says nothing, as `build.remove_make_deps` says. The removal is an
/// ordinary `emrac remove` (plan, pre-flight, journal, history), and a problem
/// with it never fails the run, which already worked.
pub fn offer_build_tool_removal() {
    let Some(pending) = PENDING_TOOLS.lock().unwrap().take() else {
        return;
    };
    // A fresh `Sources`: the one the run used read pacman's database before
    // the builds and does not know the packages they installed.
    let Ok(sources) = Sources::init() else {
        return;
    };
    let tools = sources.build_tools_since(&pending.before);
    if tools.is_empty() {
        return;
    }
    let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    let size: u64 = tools.iter().map(|t| t.size).sum();
    let built = pending
        .built
        .iter()
        .map(|b| format!("'{b}'"))
        .collect::<Vec<_>>()
        .join(", ");
    say!(
        Voice::Notes,
        "{} installed only to build {built}, and nothing needs {} now: {} ({}).",
        if names.len() == 1 {
            "1 package was".to_string()
        } else {
            format!("{} packages were", names.len())
        },
        if names.len() == 1 { "it" } else { "them" },
        names.join(", "),
        output::human_size(size)
    );
    let remove = match pending.mode {
        RemoveTools::Always => true,
        RemoveTools::Ask => pending.interactive && confirm_default_no("Remove them now?"),
        RemoveTools::Never => false,
    };
    if !remove {
        say!(
            Voice::Suggests,
            "to remove them: emrac remove {}   (`emrac config set build.remove_make_deps always` does it after every build)",
            names.join(" ")
        );
        return;
    }
    if let Err(err) =
        crate::commands::remove::run(&sources, &names, false, false, false, true, false, None)
    {
        say!(Voice::Warns, "couldn't remove the build tools: {err}");
    }
}

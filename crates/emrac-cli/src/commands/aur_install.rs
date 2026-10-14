use emrac_core::{
    inspect_package, BuildConfig, BuildOptions, Error, Limits, Result, Sources, Verdict,
};

use crate::cli::AurReviewFlags;
use crate::output;
use crate::prompt::confirm_default_no;

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
}

impl BuildChoice {
    pub fn from_flags(flags: &AurReviewFlags, build: &BuildConfig, interactive: bool) -> Self {
        Self {
            interactive,
            limits: Limits::from_config(build),
            accept_risks: flags.accept_risks,
            no_sandbox: flags.no_sandbox,
            allow_network: flags.allow_network,
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
        eprintln!(
            "emrac warns: building '{name}' WITHOUT the sandbox (--no-sandbox): the build runs with your full access."
        );
        return Ok(BuildOptions {
            sandbox: false,
            allow_network: true,
            limits: choice.limits,
            interactive: choice.interactive,
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
            eprintln!("emrac found: {conflict}");
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
                && confirm_default_no(&format!(
                    "The build of '{name}' failed with the sandbox's network off, which may be why. Retry with network access for this build?"
                )) =>
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

    sources.install_built(&files)
}

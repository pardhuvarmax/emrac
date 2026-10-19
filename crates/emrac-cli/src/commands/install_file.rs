//! `emrac install ./name-1.0-1-x86_64.pkg.tar.zst`: a package file on this
//! machine (one you built, or downloaded), installed the way `pacman -U` does,
//! through the same plan, audit of what is inside, pre-flight, confirmation,
//! journal and history as any other install.

use emrac_core::{inspect_package, say, Error, Result, Sources, Verdict, Voice};

use crate::commands::history;
use crate::output;
use crate::prompt::{confirm, confirm_default_no};

#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    args: &[String],
    dry_run: bool,
    yes: bool,
    accept_risks: bool,
    json: bool,
    allow_partial: bool,
    expect_plan: Option<&str>,
) -> Result<()> {
    let local = sources.plan_install_files(args)?;
    let plan = &local.plan;
    output::print_plan_with_preview(plan, &Default::default(), json);
    crate::commands::plan_check::expect(plan, expect_plan)?;
    let shown = crate::commands::plan_check::Shown::new(plan);

    // What is inside: nothing is extracted or run. Critical findings are
    // refused, high ones ask (a question `--yes` does not answer).
    let mut risky: Option<(String, String)> = None;
    for file in &local.files {
        let manifest = inspect_package(file)?;
        output::print_manifest(&manifest, !json);
        match manifest.report.verdict() {
            Verdict::Block => {
                return Err(Error::ManifestBlocked {
                    package: manifest.package.clone(),
                    findings: output::critical_summary(&manifest.report),
                    file: file.display().to_string(),
                });
            }
            Verdict::Confirm if risky.is_none() => {
                risky = Some((manifest.package.clone(), file.display().to_string()));
            }
            _ => {}
        }
    }

    crate::commands::preflight::run(
        sources,
        plan,
        &crate::commands::preflight::Options {
            dry_run,
            offline: true,
            json,
            allow_partial,
            check_partial: false,
            aur_builds: false,
        },
    )?;
    if dry_run {
        return Ok(());
    }

    if let Some((package, file)) = risky {
        if !accept_risks
            && !confirm_default_no(&format!(
                "'{package}' makes HIGH-risk changes (see above). Install it anyway?"
            ))
        {
            return Err(Error::InstallDeclined { package, file });
        }
    }
    if !yes && !confirm("Proceed with installation?") {
        say!(Voice::Notes, "okay, nothing installed.");
        return Ok(());
    }

    crate::commands::plan_check::verify_before_commit(&shown, plan, &|s| {
        s.plan_install_files(args).map(|l| l.plan)
    })?;
    history::begin(plan)?;
    let result = sources.install_built(&local.files);
    history::record(plan, &result);
    result
}

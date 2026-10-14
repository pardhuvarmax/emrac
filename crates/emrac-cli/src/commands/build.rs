use std::path::Path;

use emrac_core::{audit_path, inspect_package, Error, Result, Severity, Sources, Verdict};

use crate::commands::aur_install::{build_options, build_with_retry, BuildChoice};
use crate::commands::aur_review::review_aur_package;
use crate::output;
use crate::prompt::confirm_default_no;

/// `emrac build`: builds each target in the sandbox and shows what the
/// resulting package would change. Nothing is installed.
///
/// A target is a directory with a PKGBUILD (audited, then built in place) or
/// an AUR package name (same review and audit as `install`).
pub fn run(
    sources: &Sources,
    targets: &[String],
    skip_pkgbuild: bool,
    skip_diff: bool,
    yes: bool,
    choice: BuildChoice,
    json: bool,
) -> Result<()> {
    let mut built = Vec::new();
    for target in targets {
        let path = Path::new(target);
        let files = if path.is_dir() || target.contains('/') {
            build_directory(sources, path, choice)?
        } else {
            if !review_aur_package(
                sources,
                target,
                skip_pkgbuild,
                skip_diff,
                yes,
                choice.accept_risks,
            )? {
                println!("emrac notes: okay, '{target}' not built.");
                continue;
            }
            let opts = build_options(sources, target, sources.predicts_network(target), choice)?;
            build_with_retry(sources, target, opts)?
        };
        for file in files {
            let manifest = inspect_package(&file)?;
            if !json {
                output::print_manifest(&manifest, false);
                match manifest.report.verdict() {
                    Verdict::Block => println!("emrac would refuse to install this."),
                    Verdict::Confirm => {
                        println!("emrac would stop and ask before installing this.")
                    }
                    Verdict::Allow => {}
                }
                println!("emrac built: {}\n", file.display());
            }
            built.push((file, manifest));
        }
    }
    if json {
        let values: Vec<_> = built
            .iter()
            .map(|(file, m)| serde_json::json!({ "file": file, "manifest": m }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&values).unwrap());
    }
    Ok(())
}

/// A local PKGBUILD directory: audited first (critical refuses, high asks),
/// then built in place.
fn build_directory(
    sources: &Sources,
    dir: &Path,
    choice: BuildChoice,
) -> Result<Vec<std::path::PathBuf>> {
    let report = audit_path(dir)?;
    output::print_audit(&report, true);
    match report.verdict() {
        Verdict::Block => {
            return Err(Error::AuditBlocked {
                package: dir.display().to_string(),
                findings: output::critical_summary(&report),
            });
        }
        Verdict::Confirm
            if !choice.accept_risks
                && !confirm_default_no(&format!(
                    "The audit found HIGH-risk problems in '{}'. Build it anyway?",
                    dir.display()
                )) =>
        {
            return Err(Error::AuditFlagged(dir.display().to_string()));
        }
        _ => {}
    }
    let predicts_network = report
        .findings
        .iter()
        .any(|f| f.severity >= Severity::Info && f.rule.starts_with("network-in-"));
    let opts = build_options(
        sources,
        &dir.display().to_string(),
        predicts_network,
        choice,
    )?;
    sources.build_dir_package(dir, opts)
}

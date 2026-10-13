use emrac_core::{Error, Plan, Result, Sources, Verdict};

use crate::output;

/// Shows what the (already downloaded) official packages in `plan` would
/// change. Deliberately lighter than for AUR builds: official packages are
/// signature-verified and legitimately ship setuid helpers and hooks, so
/// findings are *shown* and only a **critical** one stops the install.
pub fn show(sources: &Sources, plan: &Plan) -> Result<()> {
    let inspection = sources.inspect_official(plan)?;
    let manifests: Vec<_> = inspection.found.iter().map(|(_, m)| m).collect();
    output::print_official_manifests(&manifests, &inspection.missing);

    for (path, manifest) in &inspection.found {
        if manifest.report.verdict() == Verdict::Block {
            return Err(Error::ManifestBlocked {
                package: manifest.package.clone(),
                findings: output::critical_summary(&manifest.report),
                file: path.display().to_string(),
            });
        }
    }
    Ok(())
}

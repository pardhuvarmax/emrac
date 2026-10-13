use emrac_core::{inspect_package, Error, Result, Sources, Verdict};

use crate::output;
use crate::prompt::confirm_default_no;

/// Builds an AUR package, shows what the built package would change, and
/// installs it — unless the manifest says not to. This is the only path by
/// which an AUR package reaches `pacman -U`.
///
/// Critical findings in the built package: refused, no override. High ones:
/// the default-no prompt that `--yes` does not answer (`--accept-risks`
/// does). Anything else installs without a further question: the user
/// already confirmed the install.
pub fn build_and_install(sources: &Sources, name: &str, accept_risks: bool) -> Result<()> {
    let files = sources.build_aur_package(name)?;

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

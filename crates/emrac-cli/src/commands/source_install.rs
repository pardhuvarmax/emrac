//! An official package built from source here (u23, `emrac install --source`):
//! Arch's packaging source at the tag of the repository's version, audited,
//! built in the sandbox like an AUR package, the result inspected before it is
//! installed with `pacman -U`.

use emrac_core::{inspect_package, say, Error, Result, Sources, Verdict, Voice};

use crate::commands::aur_install::{
    build_options, build_with_retry, note_build, BuildChoice, RemoveTools,
};
use crate::output;
use crate::prompt::confirm_default_no;

pub fn build_and_install(sources: &Sources, name: &str, choice: BuildChoice) -> Result<()> {
    let src = sources.arch_source(name)?;
    say!(
        Voice::Plans,
        "building '{name}' {} from Arch's packaging source ({}, tag {})",
        src.version,
        src.base,
        src.version
    );
    let before = if choice.remove_tools == RemoveTools::Never {
        std::collections::HashSet::new()
    } else {
        sources.installed_names_set()
    };
    let key = sources.arch_source_sync(&src)?;

    // The same audit as for the AUR: what is about to be built is read first.
    let pkgbuild = sources.read_pkgbuild(&key)?;
    let report = sources.audit_aur_text(&key, &pkgbuild, None);
    output::print_audit(&report, true);
    match report.verdict() {
        Verdict::Block => {
            return Err(Error::AuditBlocked {
                package: name.to_string(),
                findings: output::critical_summary(&report),
            });
        }
        Verdict::Confirm
            if !choice.accept_risks
                && !confirm_default_no(&format!(
                    "The audit found HIGH-risk problems in the packaging source of '{name}'. Build it anyway?"
                )) =>
        {
            return Err(Error::InstallDeclined {
                package: name.to_string(),
                file: String::new(),
            });
        }
        _ => {}
    }

    let opts = build_options(sources, &key, sources.predicts_network(&key), choice)?;
    let built = build_with_retry(sources, &key, opts)?;
    // a base builds all its packages; only the one asked for is installed
    let files: Vec<std::path::PathBuf> = built
        .into_iter()
        .filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .and_then(emrac_core::install_mode::package_of_file)
                == Some(name)
        })
        .collect();
    if files.is_empty() {
        return Err(Error::NoBuiltPackage(name.to_string()));
    }

    let mut risky = false;
    for file in &files {
        let manifest = inspect_package(file)?;
        output::print_manifest(&manifest, true);
        if manifest.version != src.version {
            say!(
                Voice::Warns,
                "the build made {} {}, but the repository has {}: the packaging source is not what the binary was built from",
                manifest.package,
                manifest.version,
                src.version
            );
        }
        match manifest.report.verdict() {
            Verdict::Block => {
                return Err(Error::ManifestBlocked {
                    package: manifest.package.clone(),
                    findings: output::critical_summary(&manifest.report),
                    file: file.display().to_string(),
                });
            }
            Verdict::Confirm => risky = true,
            Verdict::Allow => {}
        }
    }
    if risky
        && !choice.accept_risks
        && !confirm_default_no(&format!(
            "The built package '{name}' makes HIGH-risk changes (see above). Install it anyway?"
        ))
    {
        return Err(Error::InstallDeclined {
            package: name.to_string(),
            file: files[0].display().to_string(),
        });
    }

    sources.install_built(&files)?;
    let _ = sources.record_source_built(name, &src.version);
    note_build(name, before, choice);
    Ok(())
}

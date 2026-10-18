//! `emrac bundle create | verify | apply` (u14).

use std::path::{Path, PathBuf};

use emrac_core::bundle::{self, Change, ChangeKind, Manifest};
use emrac_core::{
    say, version_is_newer, Error, Plan, PlanAction, PlannedPackage, Result, Sources, Voice,
};

use crate::commands::history;
use crate::output::human_size;
use crate::prompt::confirm;

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::Bundle(format!("{}: {e}", path.display()))
}

pub fn create(
    sources: &Sources,
    dir: &str,
    pkgs: &[String],
    target_file: Option<&str>,
    json: bool,
) -> Result<()> {
    let target = match target_file {
        Some(file) => {
            let text = std::fs::read_to_string(file).map_err(|e| io_err(Path::new(file), e))?;
            Some(emrac_core::exchange::parse(&text)?)
        }
        None => None,
    };
    let manifest = sources.bundle_create(Path::new(dir), pkgs, target.as_ref())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
        return Ok(());
    }
    let total: u64 = manifest.package.iter().map(|p| p.size).sum();
    println!(
        "bundle in {dir}: {} packages, {}",
        manifest.package.len(),
        human_size(total)
    );
    for p in &manifest.package {
        println!("  {} {}  ({})", p.name, p.version, p.repo);
    }
    for s in &manifest.skipped {
        println!("  not included  {}: {}", s.name, s.why);
    }
    println!();
    println!("Check it on the other machine with `emrac bundle verify {dir}`, install with `emrac bundle apply {dir}`.");
    Ok(())
}

/// The manifest of the bundle in `dir` and everything wrong with its files.
fn load(dir: &Path) -> Result<(Manifest, Vec<String>)> {
    let path = dir.join("manifest.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| io_err(&path, e))?;
    let manifest = bundle::parse(&text)?;
    let problems = bundle::verify_dir(dir, &manifest);
    Ok((manifest, problems))
}

pub fn verify(dir: &str, json: bool) -> Result<()> {
    let (manifest, problems) = load(Path::new(dir))?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "packages": manifest.package.len(), "problems": problems })
        );
    } else if problems.is_empty() {
        say!(
            Voice::Confirms,
            "the bundle is complete: {} packages, every file matches its checksum and has a signature file. pacman checks the signatures themselves when it is applied.",
            manifest.package.len()
        );
    } else {
        for p in &problems {
            say!(Voice::Warns, "{p}");
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::Bundle(format!(
            "the bundle has {} problem(s); do not apply it",
            problems.len()
        )))
    }
}

fn plan_of(changes: &[&Change], requested: &[String], action: PlanAction) -> Plan {
    Plan {
        action,
        packages: changes
            .iter()
            .map(|c| PlannedPackage {
                name: c.name.clone(),
                version: c.to.clone(),
                repo: "bundle".to_string(),
                explicit: requested.contains(&c.name),
                previous_version: c.from.clone(),
            })
            .collect(),
        total_download_size: 0,
        total_installed_size_delta: 0,
        removals: Vec::new(),
        provider_choices: Vec::new(),
        warnings: Vec::new(),
    }
}

pub fn apply(sources: &Sources, dir: &str, dry_run: bool, yes: bool, json: bool) -> Result<()> {
    let dir_path = std::fs::canonicalize(dir).map_err(|e| io_err(Path::new(dir), e))?;
    let (manifest, problems) = load(&dir_path)?;
    if !problems.is_empty() {
        for p in &problems {
            say!(Voice::Warns, "{p}");
        }
        return Err(Error::Bundle(format!(
            "the bundle has {} problem(s); nothing was installed",
            problems.len()
        )));
    }
    if manifest.meta.arch != std::env::consts::ARCH {
        return Err(Error::Bundle(format!(
            "this bundle is for {}, this machine is {}",
            manifest.meta.arch,
            std::env::consts::ARCH
        )));
    }
    let changes = bundle::compare(&manifest, &sources.installed_pairs(), &|a, b| {
        version_is_newer(b, a)
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&changes).unwrap());
        if dry_run {
            return Ok(());
        }
    } else {
        println!(
            "Bundle from {} (emrac {})",
            manifest.meta.created, manifest.meta.emrac
        );
        for c in &changes {
            match c.kind {
                ChangeKind::Install => println!("  install   {} {}", c.name, c.to),
                ChangeKind::Upgrade => println!(
                    "  upgrade   {} {} -> {}",
                    c.name,
                    c.from.as_deref().unwrap_or("?"),
                    c.to
                ),
                ChangeKind::Same => println!(
                    "  same      {} {} (already installed, skipped)",
                    c.name, c.to
                ),
                ChangeKind::Older => println!(
                    "  refused   {} {}: {} is installed, which is newer (never downgraded)",
                    c.name,
                    c.to,
                    c.from.as_deref().unwrap_or("?")
                ),
            }
        }
    }
    let todo: Vec<&Change> = changes
        .iter()
        .filter(|c| matches!(c.kind, ChangeKind::Install | ChangeKind::Upgrade))
        .collect();
    if todo.is_empty() {
        say!(Voice::Notes, "nothing to install from this bundle.");
        return Ok(());
    }
    let installs: Vec<&Change> = todo
        .iter()
        .copied()
        .filter(|c| c.kind == ChangeKind::Install)
        .collect();
    let upgrades: Vec<&Change> = todo
        .iter()
        .copied()
        .filter(|c| c.kind == ChangeKind::Upgrade)
        .collect();
    let all = plan_of(
        &todo,
        &manifest.requested,
        if upgrades.is_empty() {
            PlanAction::Install
        } else {
            PlanAction::Upgrade
        },
    );
    crate::commands::preflight::run(
        sources,
        &all,
        &crate::commands::preflight::Options {
            dry_run,
            offline: true,
            json,
            allow_partial: false,
            check_partial: false,
            aur_builds: false,
        },
    )?;
    if dry_run {
        return Ok(());
    }
    if !yes
        && !confirm("Install these packages from the bundle? pacman will check every signature.")
    {
        say!(Voice::Notes, "okay, nothing installed.");
        return Ok(());
    }
    let files: Vec<PathBuf> = todo
        .iter()
        .filter_map(|c| manifest.package.iter().find(|p| p.name == c.name))
        .map(|p| dir_path.join("pkg").join(&p.file))
        .collect();
    history::begin(&all)?;
    let result = sources.execute_bundle(&files);
    for (changes, action) in [
        (&installs, PlanAction::Install),
        (&upgrades, PlanAction::Upgrade),
    ] {
        if !changes.is_empty() {
            history::record_step(&plan_of(changes, &manifest.requested, action), &result);
        }
    }
    emrac_core::journal::finish();
    if result.is_err() {
        say!(
            Voice::Notes,
            "if pacman said \"invalid or corrupted package (PGP signature)\", suspect this machine's keyring before the bundle: a machine that has been offline for months may not know the keys new packages are signed with. Apply a bundle of just `archlinux-keyring` first (`emrac bundle apply --help` has the steps)."
        );
    }
    result
}

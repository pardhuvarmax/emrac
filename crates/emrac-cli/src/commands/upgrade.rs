use emrac_core::{Result, Sources};

use crate::commands::aur_review::review_aur_package;
use crate::commands::history;
use crate::output;
use crate::prompt::confirm;

#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    pkgs: &[String],
    dry_run: bool,
    yes: bool,
    offline: bool,
    skip_pkgbuild: bool,
    skip_diff: bool,
    accept_risks: bool,
    json: bool,
    quiet: bool,
    snapshot_first: bool,
) -> Result<()> {
    let upgrade = sources.plan_upgrade(pkgs, offline)?;

    if let Some(warning) = &upgrade.aur_warning {
        if !quiet {
            eprintln!("emrac warns: {warning}");
        }
    }

    output::print_plan(&upgrade.plan, json);
    output::print_pin_notes(&upgrade.held, &upgrade.expired_pins, quiet);

    if upgrade.plan.is_empty() || dry_run {
        return Ok(());
    }

    // Exactly the AUR packages the plan says need upgrading.
    let aur_names: Vec<String> = upgrade
        .plan
        .packages
        .iter()
        .filter(|p| p.repo == "aur")
        .map(|p| p.name.clone())
        .collect();

    for name in &aur_names {
        if !review_aur_package(sources, name, skip_pkgbuild, skip_diff, yes, accept_risks)? {
            println!("emrac notes: okay, nothing upgraded.");
            return Ok(());
        }
    }

    if !yes && !confirm("Proceed with the upgrade?") {
        println!("emrac notes: okay, nothing upgraded.");
        return Ok(());
    }

    if snapshot_first {
        match sources.auto_snapshot() {
            Ok(snapshot) => eprintln!(
                "emrac notes: saved snapshot '{0}' — `emrac restore {0}` returns to it.",
                snapshot.name
            ),
            Err(err) => eprintln!("emrac warns: couldn't save a snapshot before upgrading — {err}"),
        }
    }

    // Official side: a real full `-Syu` when nothing was explicitly named
    // (skipped if the plan shows no official packages behind, to avoid an
    // unnecessary sudo prompt when only AUR packages needed upgrading), or
    // just the named official targets otherwise.
    let official_targets = if pkgs.is_empty() {
        Vec::new()
    } else {
        sources.classify_upgrade(pkgs)?.0
    };
    let has_official_targets = if pkgs.is_empty() {
        upgrade.plan.packages.iter().any(|p| p.repo != "aur")
    } else {
        !official_targets.is_empty()
    };

    let result = (|| {
        if has_official_targets {
            sources.upgrade_official(&official_targets)?;
        }
        for name in &aur_names {
            sources.build_aur(name)?;
        }
        Ok(())
    })();
    history::record(&upgrade.plan, &result);
    result
}

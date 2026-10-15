use emrac_core::{say, Voice};
use emrac_core::{Result, Sources};

use crate::commands::aur_install::BuildChoice;
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
    choice: BuildChoice,
    inspect: bool,
    json: bool,
    quiet: bool,
    snapshot_first: bool,
    allow_partial: bool,
    expect_plan: Option<&str>,
    ack_news: bool,
) -> Result<()> {
    let upgrade = sources.plan_upgrade(pkgs, offline)?;

    if let Some(warning) = &upgrade.aur_warning {
        if !quiet {
            say!(Voice::Warns, "{warning}");
        }
    }

    // What the plan is based on. An empty plan from an old package list says
    // nothing about what is available now, so it must not be reported as
    // "already up to date".
    if let Some(note) = output::stale_list_note(&upgrade.package_list) {
        say!(Voice::Warns, "{note}");
        if upgrade.plan.is_empty() {
            if !json {
                say!(
                    Voice::Warns,
                    "nothing to upgrade in that list — which says nothing about what is available now. Run `emrac upgrade` with the network to see the real plan."
                );
            }
            output::print_pin_notes(&upgrade.held, &upgrade.expired_pins, quiet);
            return Ok(());
        }
    }
    output::print_plan(&upgrade.plan, json);
    crate::commands::plan_check::expect(&upgrade.plan, expect_plan)?;
    let shown = crate::commands::plan_check::Shown::new(&upgrade.plan);
    output::print_pin_notes(&upgrade.held, &upgrade.expired_pins, quiet);

    let news_gate = if upgrade.plan.is_empty() {
        None
    } else {
        crate::commands::news::gate(offline, dry_run, ack_news, json)?
    };
    if !upgrade.plan.is_empty() && !json {
        let aur = upgrade
            .plan
            .packages
            .iter()
            .filter(|p| p.repo == "aur")
            .count();
        crate::commands::report::print_risk(
            &crate::commands::report::changes_of(&upgrade.plan.packages),
            aur,
            news_gate.as_ref().map_or(0, |g| g.critical),
        );
    }
    if !upgrade.plan.is_empty() {
        crate::commands::preflight::run(
            sources,
            &upgrade.plan,
            &crate::commands::preflight::Options {
                dry_run,
                offline,
                json,
                allow_partial,
                check_partial: true,
                aur_builds: upgrade.plan.packages.iter().any(|p| p.repo == "aur"),
            },
        )?;
    }

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
        if !review_aur_package(
            sources,
            name,
            skip_pkgbuild,
            skip_diff,
            yes,
            choice.accept_risks,
        )? {
            say!(Voice::Notes, "okay, nothing upgraded.");
            return Ok(());
        }
    }

    if !yes && !confirm("Proceed with the upgrade?") {
        say!(Voice::Notes, "okay, nothing upgraded.");
        return Ok(());
    }

    if snapshot_first {
        match sources.auto_snapshot() {
            Ok(snapshot) => say!(
                Voice::Confirms,
                "saved snapshot '{0}' — `emrac restore {0}` returns to it.",
                snapshot.name
            ),
            Err(err) => say!(
                Voice::Warns,
                "couldn't save a snapshot before upgrading — {err}"
            ),
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

    crate::commands::plan_check::verify_before_commit(&shown, &upgrade.plan, &|s| {
        s.plan_upgrade(pkgs, offline).map(|u| u.plan)
    })?;
    let started = emrac_core::journal::unix_now();
    let failed_before = sources.failed_units_now();
    history::begin(&upgrade.plan)?;
    let result = (|| {
        if has_official_targets {
            if inspect {
                if pkgs.is_empty() {
                    sources.download_official_upgrades()?;
                } else {
                    sources.download_official(&official_targets)?;
                }
                crate::commands::official_inspect::show(sources, &upgrade.plan)?;
            }
            sources.upgrade_official(&official_targets)?;
        }
        for name in &aur_names {
            crate::commands::aur_install::build_and_install(sources, name, choice)?;
        }
        Ok(())
    })();
    history::record(&upgrade.plan, &result);
    if let (Ok(()), Some(gate)) = (&result, &news_gate) {
        gate.done();
    }
    if result.is_ok() && has_official_targets && !json && !quiet {
        crate::commands::report::after_upgrade(
            crate::commands::report::changes_of(&upgrade.plan.packages),
            started,
            failed_before,
        );
    }
    result
}

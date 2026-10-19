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
    skip_source_builds: bool,
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
    output::print_plan_with_preview(&upgrade.plan, &sources.preview(&upgrade.plan), json);
    if upgrade.plan.is_empty() && !json {
        // an empty plan prints no warnings; a pending rebuild is still said
        for w in &upgrade.plan.warnings {
            say!(Voice::Warns, "{w}");
        }
    }
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
            snapshot_first,
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

    // The packages whose policy is to build from source, with the version the
    // upgrade brings.
    let source_names: Vec<(String, String, String)> = upgrade
        .plan
        .packages
        .iter()
        .filter(|p| emrac_core::install_mode::is_source_repo(&p.repo))
        .map(|p| (p.name.clone(), p.version.clone(), p.repo.clone()))
        .collect();

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
    // The upgrade is one step, the source rebuilds another: what the rebuilds
    // come to is reported after the upgrade has been, but still decides the exit.
    let rebuilt = if result.is_ok() && !source_names.is_empty() {
        rebuild_policy(sources, &source_names, skip_source_builds, choice)
    } else {
        Ok(())
    };
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
    result?;
    rebuilt
}

/// Carries out the source-build policy after the repository's binaries are in:
/// each package is rebuilt from Arch's packaging source at the new version. A
/// rebuild that fails, or is skipped, leaves the binary installed, the policy
/// kept and the rebuild marked pending; it is never papered over.
fn rebuild_policy(
    sources: &Sources,
    packages: &[(String, String, String)],
    skip: bool,
    choice: BuildChoice,
) -> Result<()> {
    let names: Vec<String> = packages.iter().map(|(n, _, _)| n.clone()).collect();
    if skip {
        sources.mark_source_pending(&names);
        say!(
            Voice::Warns,
            "the repository's binary of {} is installed; the source builds you asked for were skipped (--skip-source-builds) and are pending: `emrac from-source rebuild`.",
            names.join(", ")
        );
        return Ok(());
    }
    let mut failed = Vec::new();
    for (name, version, repo) in packages {
        say!(
            Voice::Notes,
            "source-build policy: rebuilding '{name}' {version} from Arch's packaging source."
        );
        let plan = sources.source_rebuild_plan(name, version, repo);
        // Without the transaction lock nothing is built or installed: another
        // emrac may be changing packages, or an interrupted run is unresolved.
        let result = history::begin(&plan).and_then(|()| {
            let built = crate::commands::source_install::build_and_install_at(
                sources,
                name,
                Some(version),
                choice,
            );
            history::record(&plan, &built);
            built
        });
        if let Err(err) = result {
            say!(
                Voice::Warns,
                "couldn't rebuild '{name}' from source — {err}"
            );
            failed.push(name.clone());
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    sources.mark_source_pending(&failed);
    say!(
        Voice::Warns,
        "the repository's binary of {} stays installed (the system is consistent), your source-build policy is kept and the rebuild is pending.",
        failed.join(", ")
    );
    say!(
        Voice::Suggests,
        "retry: emrac from-source rebuild  ·  switch to the binary for good: emrac from-source drop {}",
        failed.join(" ")
    );
    Err(emrac_core::Error::SourceRebuildPending(format!(
        "the upgrade is done, but {} could not be rebuilt from source and is pending",
        failed.join(", ")
    )))
}

use emrac_core::self_check;
use emrac_core::self_meta;
use emrac_core::self_upgrade::{self, ExecOptions, Outcome, Plan};
use emrac_core::{say, Config, Voice};
use emrac_core::{DoctorStatus, Error, Result};

use crate::output;

/// `emrac self check`. Exits 1 when a check found a *problem*, like `doctor`.
pub fn check(json: bool) -> Result<()> {
    let report = self_check::evaluate(&self_check::gather());
    output::print_doctor(&report, json);
    let problems = report.count(DoctorStatus::Problem);
    if problems > 0 {
        return Err(Error::DoctorProblems(if problems == 1 {
            "1 problem".to_string()
        } else {
            format!("{problems} problems")
        }));
    }
    Ok(())
}

/// `emrac self diagnose`. The same exit rule as `check`.
pub fn diagnose(online: bool, offline: bool, json: bool) -> Result<()> {
    if online && offline {
        return Err(Error::SelfService(
            "--online asks the network and --offline forbids it: pick one".into(),
        ));
    }
    let mut report =
        self_check::evaluate_deep(&self_check::gather(), &self_check::gather_deep(online));
    self_check::redact_home(&mut report, &std::env::var("HOME").unwrap_or_default());
    output::print_doctor(&report, json);
    let problems = report.count(DoctorStatus::Problem);
    if problems > 0 {
        return Err(Error::DoctorProblems(if problems == 1 {
            "1 problem".to_string()
        } else {
            format!("{problems} problems")
        }));
    }
    Ok(())
}

fn now() -> u64 {
    emrac_core::journal::unix_now()
}

/// `emrac self sync`.
pub fn sync(dry_run: bool, offline: bool, json: bool) -> Result<()> {
    let cfg = Config::load()?.self_service;
    let stored = self_meta::load()?;
    let plan = self_meta::plan_sync(&cfg, &stored);
    if dry_run {
        if json {
            println!("{}", serde_json::to_string_pretty(&plan).unwrap());
            return Ok(());
        }
        println!("Synchronisation plan");
        println!("  Source:      {}", plan.source);
        println!("  Channel:     {}", plan.channel);
        println!(
            "  Network:     {}",
            if plan.network {
                "required"
            } else {
                "not needed (a file)"
            }
        );
        println!("  Signature:   {}", plan.signature);
        match (plan.stored_releases, plan.last_ok) {
            (Some(n), Some(t)) => println!(
                "  Stored now:  {n} releases, {}",
                self_meta::age_label(Some(t), now()).0
            ),
            _ => println!("  Stored now:  nothing (never synced)"),
        }
        println!("  Planned:");
        for a in &plan.actions {
            println!("    - {a}");
        }
        println!("  Changes applied: none (dry run). What the fetch finds can't be known before it is made.");
        return Ok(());
    }
    if offline && plan.network {
        return Err(Error::SelfService(
            "--offline is set and the release index is on the network; nothing was fetched".into(),
        ));
    }
    let outcome = self_meta::sync(&cfg, now())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
        return Ok(());
    }
    if outcome.unchanged {
        say!(
            Voice::Confirms,
            "release index is current ({} releases; signature: {}). Nothing changed.",
            outcome.releases,
            outcome.signature
        );
    } else {
        say!(
            Voice::Confirms,
            "release index updated: {} release(s) known, {} new{}, {} new news item(s); signature: {}.",
            outcome.releases,
            outcome.new_releases.len(),
            if outcome.new_releases.is_empty() {
                String::new()
            } else {
                format!(" ({})", outcome.new_releases.join(", "))
            },
            outcome.new_news,
            outcome.signature
        );
    }
    let stored = self_meta::load()?;
    if let Some(index) = &stored.index {
        if let Some(r) = self_meta::newer_than(index, &cfg.channel, env!("CARGO_PKG_VERSION")) {
            say!(
                Voice::Notes,
                "a newer release is out: {} ({}, {}). `emrac self news` says what is in it.",
                r.tag,
                r.version,
                r.date
            );
        }
    }
    Ok(())
}

/// `emrac self news`: from the cache only.
pub fn news(all: bool, json: bool) -> Result<()> {
    let stored = self_meta::load()?;
    let installed = env!("CARGO_PKG_VERSION");
    let last_ok = stored.state.as_ref().and_then(|s| s.last_ok);
    let (age, stale) = self_meta::age_label(last_ok, now());
    let Some(index) = &stored.index else {
        if json {
            println!("{}", serde_json::json!({ "synced": false, "items": [] }));
        } else {
            say!(
                Voice::Notes,
                "no release news yet: emrac hasn't fetched any ({}). `emrac self sync` does.",
                stored.index_problem.as_deref().unwrap_or(&age)
            );
        }
        return Ok(());
    };
    let items = self_meta::news_items(index, if all { None } else { Some(10) });
    if json {
        let list: Vec<_> = items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "date": i.date, "category": i.category, "title": i.title,
                    "summary": i.summary, "version": i.version, "url": i.url,
                    "newer_than_installed": self_meta::is_newer(i, installed),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "synced": true, "age": age, "stale": stale, "items": list,
            }))
            .unwrap()
        );
        return Ok(());
    }
    if stale {
        say!(
            Voice::Warns,
            "this is cached news and may be out of date ({age}). `emrac self sync` refreshes it."
        );
    }
    if items.is_empty() {
        say!(Voice::Notes, "no news items ({age}).");
        return Ok(());
    }
    println!("Release news ({age})\n");
    for i in &items {
        let version = i
            .version
            .as_deref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        let newer = if self_meta::is_newer(i, installed) {
            "  [newer than yours]"
        } else {
            ""
        };
        println!("[{}] {}{version}{newer}", i.date, i.title);
        println!("  Category: {}", i.category);
        if !i.summary.is_empty() {
            println!("  Summary:  {}", i.summary);
        }
        if let Some(u) = &i.url {
            println!("  Source:   {u}");
        }
        println!();
    }
    println!("No installation changes were performed.");
    Ok(())
}

/// Prints a plan for `upgrade` or `downgrade`.
pub fn print_plan(plan: &Plan, dry_run: bool, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(plan).unwrap());
        return;
    }
    println!("Emrac {} plan", plan.op.word());
    println!("  Installed:  {}", plan.installed);
    match &plan.outcome {
        Outcome::Nothing { reason } => println!("  Result:     {reason}"),
        Outcome::Refused { reasons } => {
            println!("  Result:     cannot be done");
            for r in reasons {
                println!("    - {r}");
            }
        }
        Outcome::Ready {
            target,
            steps,
            warnings,
        } => {
            println!(
                "  Target:     {} ({}, {}, channel {})",
                target.version, target.tag, target.date, target.channel
            );
            println!("  Source:     {}", target.tarball);
            println!(
                "  Verified:   SHA-256 from the release index ({})",
                &target.sha256[..16]
            );
            if !warnings.is_empty() {
                println!("  Worth knowing:");
                for w in warnings {
                    println!("    - {w}");
                }
            }
            println!("  Steps:");
            for (i, s) in steps.iter().enumerate() {
                println!("    {}. {s}", i + 1);
            }
            if dry_run {
                println!("  Changes applied: none (dry run).");
            }
        }
    }
}

/// Fetches a fresh index first when the stored one is more than a day old.
fn refresh_if_old(cfg: &emrac_core::SelfConfig, offline: bool) {
    let stored = match self_meta::load() {
        Ok(s) => s,
        Err(_) => return,
    };
    let old = stored
        .state
        .as_ref()
        .and_then(|s| s.last_ok)
        .is_none_or(|t| now().saturating_sub(t) > 86_400);
    if !old {
        return;
    }
    if offline && !cfg.source.starts_with("file://") {
        say!(
            Voice::Warns,
            "--offline is set: using the release index as it is."
        );
        return;
    }
    say!(
        Voice::Notes,
        "the release index is more than a day old: fetching a fresh one."
    );
    if let Err(e) = self_meta::sync(cfg, now()) {
        say!(
            Voice::Warns,
            "couldn't refresh the release index ({e}); using the one stored."
        );
    }
}

/// `emrac self upgrade`.
pub fn upgrade(
    dry_run: bool,
    yes: bool,
    offline: bool,
    skip_tests: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?.self_service;
    if !dry_run {
        refresh_if_old(&cfg, offline);
    }
    let ctx = self_upgrade::gather_context();
    let plan = self_upgrade::plan_upgrade(&ctx);
    print_plan(&plan, dry_run, json);
    run_plan(&plan, dry_run, yes, skip_tests, json)
}

/// `emrac self downgrade`.
pub fn downgrade(
    version: Option<&str>,
    tag: Option<&str>,
    dry_run: bool,
    yes: bool,
    offline: bool,
    skip_tests: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?.self_service;
    if !dry_run {
        refresh_if_old(&cfg, offline);
    }
    let ctx = self_upgrade::gather_context();
    let plan = self_upgrade::plan_downgrade(&ctx, version, tag);
    print_plan(&plan, dry_run, json);
    run_plan(&plan, dry_run, yes, skip_tests, json)
}

/// What both `upgrade` and `downgrade` do with a plan.
pub fn run_plan(plan: &Plan, dry_run: bool, yes: bool, skip_tests: bool, json: bool) -> Result<()> {
    let Outcome::Ready { target, .. } = &plan.outcome else {
        return match &plan.outcome {
            Outcome::Refused { reasons } => Err(Error::SelfService(format!(
                "not done: {}",
                reasons.first().cloned().unwrap_or_default()
            ))),
            _ => Ok(()),
        };
    };
    if dry_run {
        return Ok(());
    }
    if !yes && !crate::prompt::confirm(&format!("Proceed with the {}?", plan.op.word())) {
        say!(Voice::Notes, "okay, nothing changed.");
        return Ok(());
    }
    let mut say_phase = |s: &str| {
        if !json {
            say!(Voice::Notes, "{s}");
        }
    };
    let report = self_upgrade::execute(
        plan.op,
        target,
        &plan.installed,
        &ExecOptions { yes, skip_tests },
        &mut say_phase,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else if report.version_ok && report.check_ok {
        say!(
            Voice::Confirms,
            "emrac is now {} ({}). The new version reports itself correctly and `emrac self check` passes. A copy of your configuration and data is in {}.",
            report.to,
            report.tag,
            report.backup
        );
    } else {
        say!(
            Voice::Warns,
            "emrac {} was installed, but it did not pass the checks: {}. `emrac self diagnose` says more; `emrac self downgrade` goes back. Your backup is in {}.",
            report.to,
            report.notes.join("; "),
            report.backup
        );
    }
    Ok(())
}

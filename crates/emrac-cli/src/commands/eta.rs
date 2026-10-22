//! Time estimates (u26): under a plan, as `emrac eta`, and the record that keeps
//! them honest (`docs/plan/r3/u26-eta.md`).

use std::sync::Mutex;

use emrac_core::eta::{self, Estimate};
use emrac_core::{say, Plan, Result, Sources, Voice};

/// The estimate shown for the plan that is about to run (seconds), so that the
/// real time can be compared with it afterwards.
static SHOWN: Mutex<Option<u64>> = Mutex::new(None);

fn lines(est: &Estimate) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((secs, low, high, conf)) = est.total() {
        out.push(format!(
            "Estimated time   : about {} (between {} and {}), {} confidence",
            eta::words(secs),
            eta::words(low),
            eta::words(high),
            conf.word()
        ));
        if est.parts.len() > 1 {
            out.push(format!(
                "                   {}",
                est.parts
                    .iter()
                    .map(|p| format!("{} {}", p.label, eta::words(p.secs)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    } else if !est.unknown.is_empty() {
        out.push("Estimated time   : cannot say".to_string());
    }
    if !est.unknown.is_empty() {
        out.push(format!(
            "                   no estimate for: {}",
            est.unknown.join("; ")
        ));
    }
    out
}

/// Prints the estimate under a plan, and remembers it for the accuracy record.
pub fn print(sources: &Sources, plan: &Plan, json: bool) {
    if json || plan.is_empty() {
        return;
    }
    let est = sources.estimate(plan);
    let lines = lines(&est);
    for l in &lines {
        println!("{l}");
    }
    if let Some(note) = eta::accuracy_summary(&eta::read_accuracy()) {
        if !lines.is_empty() {
            println!("                   ({note})");
        }
    }
    *SHOWN.lock().unwrap() = est.total().map(|t| t.0);
}

/// A transaction ended: what an official one took teaches the next estimate, and
/// the estimate that was shown is compared with the real time.
pub fn finished(plan: &Plan, ok: bool, secs: u64) {
    if !ok {
        return;
    }
    let official = plan
        .packages
        .iter()
        .all(|p| p.repo != "aur" && !emrac_core::install_mode::is_source_repo(&p.repo));
    if official && !plan.packages.is_empty() && plan.total_download_size > 0 {
        eta::record_op(plan.total_download_size, plan.packages.len() as u32, secs);
    }
    if let Some(estimated) = SHOWN.lock().unwrap().take() {
        eta::record_accuracy(estimated, secs);
    }
}

/// `emrac eta [package...]`: how long installing these (or the upgrade that is
/// waiting) would take, with what it is based on.
pub fn run(sources: &Sources, pkgs: &[String], offline: bool, json: bool) -> Result<()> {
    let plan = if pkgs.is_empty() {
        sources.plan_upgrade(&[], offline)?.plan
    } else {
        sources
            .plan_install_with(pkgs, offline, emrac_core::SourceMode::Auto)?
            .0
    };
    let est = sources.estimate(&plan);
    if json {
        println!("{}", serde_json::to_string_pretty(&est).unwrap_or_default());
        return Ok(());
    }
    if plan.is_empty() {
        say!(
            Voice::Confirms,
            "there is nothing to do, so nothing takes time."
        );
        return Ok(());
    }
    for l in lines(&est) {
        println!("{l}");
    }
    for p in &est.parts {
        println!(
            "  {:<28} {:>10}  ({}\u{2013}{})  {} confidence \u{2014} {}",
            p.label,
            eta::words(p.secs),
            eta::words(p.low),
            eta::words(p.high),
            p.confidence.word(),
            p.basis
        );
    }
    match eta::accuracy_summary(&eta::read_accuracy()) {
        Some(note) => say!(Voice::Notes, "{note}."),
        None => say!(
            Voice::Notes,
            "no record yet of how close estimates came; it builds up as transactions finish."
        ),
    }
    Ok(())
}

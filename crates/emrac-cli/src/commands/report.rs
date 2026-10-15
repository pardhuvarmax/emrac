//! The update risk before an upgrade and the report after it (u20, part 5).

use emrac_core::upgrade_report::{self, Change, Level, Report};
use emrac_core::{say, Error, PlanAction, Result, Sources, TxStatus, Voice};

/// What an upgrade plan changes, as the report counts it.
pub fn changes_of(packages: &[emrac_core::PlannedPackage]) -> Vec<Change> {
    packages
        .iter()
        .map(|p| Change {
            name: p.name.clone(),
            from: p.previous_version.clone(),
            to: p.version.clone(),
        })
        .collect()
}

/// Under the plan of an upgrade: how risky it is and why.
pub fn print_risk(changes: &[Change], aur_rebuilds: usize, critical_news: usize) {
    let r = upgrade_report::risk(changes, aur_rebuilds, critical_news);
    let line = if r.reasons.is_empty() {
        "nothing in it touches the kernel, the C library, the bootloader or the boot".to_string()
    } else {
        r.reasons.join("; ")
    };
    println!("Update risk      : {} ({line})", r.level.word());
    if r.level == Level::High {
        println!(
            "                   a snapshot first is cheap: `emrac snapshot`, or `emrac config set snapshot.before_upgrade true`"
        );
    }
}

fn newer(a: &str, b: &str) -> bool {
    emrac_core::version_is_newer(a, b)
}

/// Prints the report. `quiet_when_clear` makes an all-clear a single line.
pub fn print_report(r: &Report, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap());
        return;
    }
    if !r.notable.is_empty() {
        println!("Notable changes:");
        for n in &r.notable {
            println!("  {n}");
        }
    }
    if !r.reboot.is_empty() {
        say!(Voice::Suggests, "a reboot is advised:");
        for why in &r.reboot {
            println!("  - {why}");
        }
    }
    if !r.restart.is_empty() {
        say!(
            Voice::Suggests,
            "these services started before the upgrade and still run the old files:"
        );
        for (unit, package) in &r.restart {
            println!("  - {unit}  (from {package})");
        }
        println!(
            "  restart them: sudo systemctl restart {}",
            r.restart
                .iter()
                .map(|(u, _)| u.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    if !r.failed.is_empty() {
        if r.failed_new {
            say!(
                Voice::Warns,
                "units that failed during or after the upgrade: {}",
                r.failed.join(", ")
            );
            println!(
                "  `systemctl status <unit>` says why; `emrac undo` goes back to the versions before."
            );
        } else {
            say!(
                Voice::Notes,
                "units failing right now: {} (this can't tell whether the upgrade caused it)",
                r.failed.join(", ")
            );
            println!("  `systemctl status <unit>` says why.");
        }
    }
    if !r.stale.is_empty() {
        say!(
            Voice::Suspects,
            "AUR packages the new libraries broke: {}. `emrac rebuild --check` says why, `emrac rebuild --stale` builds them again.",
            r.stale.join(", ")
        );
    }
    if r.pacnew_new > 0 {
        say!(
            Voice::Suggests,
            "{} new .pacnew/.pacsave file(s) from this upgrade: `emrac pacnew` lists them.",
            r.pacnew_new
        );
    }
    if r.all_clear() {
        say!(
            Voice::Confirms,
            "after the upgrade: nothing to restart, no new failures, nothing broken. (As a normal user only your own view of systemd is checked.)"
        );
    }
}

/// After an upgrade went through: the report, from a new `Sources` since the
/// one the command started with read the database before the change.
pub fn after_upgrade(changes: Vec<Change>, since: u64, failed_before: Vec<String>) {
    let Ok(fresh) = Sources::init() else { return };
    let facts = fresh.upgrade_facts(changes, since, Some(failed_before));
    let report = upgrade_report::assess(&facts, &newer);
    println!();
    print_report(&report, false);
}

/// `emrac report`: the report for the last upgrade in the history.
pub fn run(sources: &Sources, json: bool) -> Result<()> {
    let records = emrac_core::History::all()?;
    let Some(last) = records
        .iter()
        .rev()
        .find(|r| r.action == PlanAction::Upgrade && r.status == TxStatus::Committed)
    else {
        return Err(Error::History(
            "there is no upgrade in your history to report on".into(),
        ));
    };
    let log = std::fs::read_to_string("/var/log/pacman.log").unwrap_or_default();
    let since = emrac_core::stale_builds::transaction_starts(&log)
        .into_iter()
        .filter(|t| *t <= last.timestamp)
        .max()
        .unwrap_or(last.timestamp);
    let facts = sources.upgrade_facts(changes_of(&last.packages), since, None);
    let report = upgrade_report::assess(&facts, &newer);
    if !json {
        say!(
            Voice::Found,
            "report for {} ({}, {} package(s))\n",
            last.id,
            last.timestamp_utc(),
            last.packages.len()
        );
    }
    print_report(&report, json);
    Ok(())
}

//! `emrac vulns` (u19): installed packages against the Arch Security Tracker.

use emrac_core::security_tracker as tracker;
use emrac_core::style::Activity;
use emrac_core::{Error, Result, Sources};

use crate::cli::VulnsCommand;
use crate::view;

pub struct Options {
    pub all: bool,
    pub fixable: bool,
    pub check: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn run(
    sources: &Sources,
    command: Option<&VulnsCommand>,
    opts: Options,
    offline: bool,
    json: bool,
) -> Result<()> {
    match command {
        Some(VulnsCommand::Ignore { id, reason }) => ignore(id, reason.as_deref(), offline, json),
        Some(VulnsCommand::Unignore { id }) => unignore(id, json),
        Some(VulnsCommand::Ignored) => ignored(json),
        None => list(sources, opts, offline, json),
    }
}

fn list(sources: &Sources, opts: Options, offline: bool, json: bool) -> Result<()> {
    let activity =
        Activity::spinner("checking installed packages against the Arch Security Tracker");
    let report = sources.vuln_report(offline);
    match &report {
        Ok(_) => activity.finish("checked against the Arch Security Tracker"),
        Err(_) => activity.fail("could not check against the Arch Security Tracker"),
    }
    let report = report?;
    let hard = report.hard();
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        let st = emrac_core::style::Style::stdout();
        print!("{}", view::vulns_view(&st, &report, opts.all, opts.fixable));
    }
    if opts.check && hard > 0 {
        return Err(Error::DoctorProblems(format!(
            "{hard} confirmed High or Critical {}",
            if hard == 1 { "entry" } else { "entries" }
        )));
    }
    Ok(())
}

fn ignore(id: &str, reason: Option<&str>, offline: bool, json: bool) -> Result<()> {
    let list = tracker::load(offline, now())?;
    let path = tracker::ignore_path()?;
    let entry = tracker::add_ignore_to(&path, &list.groups, id, reason, now())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entry).unwrap());
    } else {
        emrac_core::say!(
            emrac_core::Voice::Confirms,
            "{id} is set aside{}",
            match &entry.fixed_at_ignore {
                Some(f) => format!(
                    " (it records a fix, {f}, so this lapses at once for versions older than that)"
                ),
                None => "; it lapses if the entry later records a fix".to_string(),
            }
        );
    }
    Ok(())
}

fn unignore(id: &str, json: bool) -> Result<()> {
    tracker::remove_ignore_from(&tracker::ignore_path()?, id)?;
    if json {
        println!("{{\"unignored\": \"{id}\"}}");
    } else {
        emrac_core::say!(emrac_core::Voice::Confirms, "{id} counts again");
    }
    Ok(())
}

fn ignored(json: bool) -> Result<()> {
    let list = tracker::read_ignores();
    if json {
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return Ok(());
    }
    if list.is_empty() {
        emrac_core::say!(emrac_core::Voice::Notes, "nothing is ignored");
    }
    for i in &list {
        println!(
            "{}  {}",
            i.id,
            i.reason.as_deref().unwrap_or("(no reason given)")
        );
    }
    Ok(())
}

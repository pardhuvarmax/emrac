//! `emrac bisect` (u24): the transaction that broke something, found by putting
//! the system back into states it really had (`docs/plan/r3/u24-bisect.md`).

use std::io::{BufRead, IsTerminal, Write};

use emrac_core::bisect::{self, Outcome, Point, Reference, Search, Timeline, Verdict};
use emrac_core::{say, Error, Result, Snapshot, Sources, Voice};

use crate::prompt::confirm;

pub struct Args<'a> {
    pub good: &'a str,
    pub bad: Option<&'a str>,
    pub test: Option<&'a str>,
    pub dry_run: bool,
    pub analyze: bool,
    pub yes: bool,
    pub offline: bool,
}

fn snapshot_of(point: &Point) -> Snapshot {
    Snapshot {
        name: point.label.clone(),
        timestamp: point.timestamp,
        note: None,
        packages: point.packages.clone(),
    }
}

fn describe(change: &(String, Option<String>, Option<String>)) -> String {
    match change {
        (n, Some(a), Some(b)) => format!("{n} {a} \u{2192} {b}"),
        (n, None, Some(b)) => format!("{n} {b} (installed)"),
        (n, Some(a), None) => format!("{n} {a} (removed)"),
        (n, None, None) => n.clone(),
    }
}

fn summary(timeline: &Timeline, i: usize) -> String {
    let changes = bisect::changed(&timeline.points[i - 1], &timeline.points[i]);
    let shown: Vec<String> = changes.iter().take(4).map(describe).collect();
    let more = changes.len().saturating_sub(4);
    format!(
        "{}{}",
        shown.join(", "),
        if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        }
    )
}

/// Puts the system into the state of `point`: the machinery of `restore --prune`.
///
/// Every step reads the installed packages afresh (the handle a command starts
/// with was opened before the steps before it changed them) and works out what
/// pacman would really do right now: the packages the plan changes, the ones it
/// removes, and what installing the files would remove by conflict or replacement.
/// If any of those is protected (`emrac_core::protected`), nothing is changed.
fn go_to(point: &Point, offline: bool, extra: &[String]) -> Result<()> {
    let sources = Sources::init()?;
    let plan = sources.plan_restore_snapshot(&snapshot_of(point), true, offline)?;
    if plan.is_empty() {
        return Ok(());
    }
    let mut touched: Vec<String> = plan.entries.iter().map(|e| e.item.name.clone()).collect();
    touched.extend(plan.extras.iter().map(|(n, _)| n.clone()));
    touched.extend(sources.restore_side_removals(&plan)?);
    let blocked = emrac_core::protected::blocked(&touched, extra);
    if !blocked.is_empty() {
        return Err(Error::Help(format!(
            "bisect stopped before changing anything: reaching '{}' would change {}. Protected packages are never changed on a live host.",
            point.label,
            emrac_core::protected::describe(&blocked)
        )));
    }
    crate::commands::restore::apply(&sources, &plan)
}

/// `(name, version)` of everything installed, read afresh.
fn installed_now() -> Vec<(String, String)> {
    Sources::init()
        .map(|s| s.installed_pairs())
        .unwrap_or_default()
}

/// 0 good, 125 cannot tell, anything else bad.
fn run_test(command: &str) -> Result<Verdict> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .status()
        .map_err(|e| Error::CommandSpawn("sh".into(), e))?;
    Ok(match status.code() {
        Some(0) => Verdict::Good,
        Some(125) => Verdict::Skip,
        _ => Verdict::Bad,
    })
}

/// `None`: the person quit.
fn ask() -> Option<Verdict> {
    let stdin = std::io::stdin();
    loop {
        print!("Is it still broken?  [g]ood (works) / [b]ad (broken) / [s]kip / [q]uit: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).ok()? == 0 {
            return None;
        }
        match line.trim().to_lowercase().as_str() {
            "g" | "good" => return Some(Verdict::Good),
            "b" | "bad" => return Some(Verdict::Bad),
            "s" | "skip" => return Some(Verdict::Skip),
            "q" | "quit" => return None,
            _ => {}
        }
    }
}

pub fn run(sources: &Sources, args: &Args) -> Result<()> {
    let config = emrac_core::Config::load()?;
    let extra = config.bisect.extra();
    let analysis = args.analyze || args.dry_run || config.bisect.mode == "analysis";

    let timeline = sources.bisect_timeline()?;
    if timeline.points.len() < 2 {
        return Err(Error::Help(
            "there are no committed transactions in the history to search through".into(),
        ));
    }
    let now = emrac_core::journal::unix_now();
    let reference = |text: &str, what: &str| -> Result<usize> {
        let parsed = Reference::parse(text, now).ok_or_else(|| {
            Error::Help(format!(
                "'{text}' is not a transaction id (tx-12), a date (2026-10-10) or a time back (7d, 12h): {what}"
            ))
        })?;
        bisect::locate(&timeline.points, &parsed).ok_or_else(|| {
            Error::Help(format!(
                "no state in the history matches '{text}': {what}. `emrac history` lists the transactions."
            ))
        })
    };
    let good = reference(args.good, "--good")?;
    let last = timeline.points.len() - 1;
    let bad = match args.bad {
        Some(t) => reference(t, "--bad")?,
        None => last,
    };
    if good >= bad {
        return Err(Error::Help(
            "the good state must come before the bad one; there is nothing to search between them"
                .into(),
        ));
    }

    // The boundary: a state that needs a protected package changed cannot be visited.
    let unreachable: Vec<usize> = ((good + 1)..bad)
        .filter(|&i| !bisect::reachable(&timeline.points, i, &extra))
        .collect();
    let behind =
        |i: usize| bisect::protected_changes(&timeline.points[i - 1], &timeline.points[i], &extra);
    say!(
        Voice::Plans,
        "bisect: {} transaction{} between good ({}) and bad ({}).",
        bad - good,
        if bad - good == 1 { "" } else { "s" },
        timeline.points[good].label,
        timeline.points[bad].label,
    );
    for i in (good + 1)..=bad {
        let protected = behind(i);
        println!(
            "  {:<8} {}  {}{}{}",
            timeline.points[i].tx.as_deref().unwrap_or("?"),
            emrac_core::format_utc(timeline.points[i].timestamp),
            summary(&timeline, i),
            if protected.is_empty() {
                String::new()
            } else {
                format!(
                    "   [protected: {}]",
                    emrac_core::protected::describe(&protected)
                )
            },
            if unreachable.contains(&i) {
                "   [not visitable on this host]"
            } else {
                ""
            }
        );
    }
    if !timeline.unexplained.is_empty() {
        say!(
            Voice::Warns,
            "the history does not explain the version of {}: they stay at the installed version in every state (changed with pacman directly?).",
            timeline.unexplained.join(", ")
        );
    }
    let candidates = (bad - good - 1).saturating_sub(unreachable.len());
    if !unreachable.is_empty() {
        say!(
            Voice::Warns,
            "{} of the {} states in between need protected packages changed ({}) and are never visited on a live host.",
            unreachable.len(),
            bad - good - 1,
            emrac_core::protected::describe(&bisect::protected_changes(
                &timeline.points[good],
                &timeline.points[last],
                &extra
            ))
        );
    }
    if bad - good > 1 && candidates == 0 {
        return Err(Error::Help(format!(
            "bisection stopped: every state between {} and {} needs protected packages changed ({}), which is never done on a live host. No package changes were made. The regression is in one of these transactions: test them in an isolated environment (a disposable VM) — that mode is planned, not built.",
            timeline.points[good].label,
            timeline.points[bad].label,
            emrac_core::protected::describe(&bisect::protected_changes(
                &timeline.points[good],
                &timeline.points[last],
                &extra
            ))
        )));
    }
    let steps = Search::steps(good, bad);
    say!(
        Voice::Plans,
        "at most {steps} state{} to look at (protection policy {}).",
        if steps == 1 { "" } else { "s" },
        emrac_core::protected::POLICY_VERSION
    );

    let missing = sources.bisect_missing(&timeline, good, args.offline)?;
    if !missing.is_empty() {
        return Err(Error::RestoreFilesMissing(missing.join(", ")));
    }
    say!(
        Voice::Confirms,
        "every package version the states need can be found."
    );
    if analysis {
        say!(
            Voice::Notes,
            "{}: nothing was changed.",
            if config.bisect.mode == "analysis" {
                "bisect.mode is analysis"
            } else {
                "this is an analysis (no --test is run, no package touched)"
            }
        );
        return Ok(());
    }
    if args.test.is_none() && !std::io::stdin().is_terminal() {
        return Err(Error::Help(
            "without a terminal there is nobody to ask: give the check as --test '<command>' (exit 0 works, 125 cannot tell, anything else broken)".into(),
        ));
    }
    if !args.yes
        && !confirm(
            "This puts the system back into earlier states, one after another (ordinary packages only), and back to now at the end. Continue?",
        )
    {
        say!(Voice::Notes, "okay, nothing was changed.");
        return Ok(());
    }

    // The starting point, exactly: what has to be true again at the end.
    let original = installed_now();
    let stamp = emrac_core::journal::unix_now();
    let safety = sources.create_snapshot(
        Some(&format!("bisect-{stamp}")),
        Some("before emrac bisect"),
    )?;
    say!(
        Voice::Confirms,
        "saved snapshot '{0}' of the state now: `emrac restore {0}` returns to it.",
        safety.name
    );

    let mut looked = 0u32;
    let mut stopped: Option<String> = None;
    // one state: reach it, then judge it
    let mut judge = |i: usize, stopped: &mut Option<String>| -> Option<Verdict> {
        looked += 1;
        say!(
            Voice::Plans,
            "state {looked}: {} ({})",
            timeline.points[i].label,
            emrac_core::format_utc(timeline.points[i].timestamp)
        );
        if let Err(err) = go_to(&timeline.points[i], args.offline, &extra) {
            *stopped = Some(format!("couldn't put the system into that state — {err}"));
            return None;
        }
        let verdict = match args.test {
            Some(cmd) => match run_test(cmd) {
                Ok(v) => v,
                Err(err) => {
                    *stopped = Some(err.to_string());
                    return None;
                }
            },
            None => match ask() {
                Some(v) => v,
                None => {
                    *stopped = Some("you stopped the search".into());
                    return None;
                }
            },
        };
        say!(
            Voice::Notes,
            "{}: {}",
            timeline.points[i].label,
            match verdict {
                Verdict::Good => "works",
                Verdict::Bad => "broken",
                Verdict::Skip => "cannot tell",
            }
        );
        Some(verdict)
    };

    let outcome = {
        let mut search = Search::with_skipped(good, bad, unreachable.clone());
        loop {
            match search.state() {
                Outcome::Next(i) => match judge(i, &mut stopped) {
                    Some(v) => search.mark(i, v),
                    None => break None,
                },
                other => break Some(other),
            }
        }
    };

    // Whatever happened, the last thing done is to go back to where it started —
    // and to check it against the exact list taken at the start.
    say!(Voice::Plans, "putting the system back to where it was.");
    let back = go_to(&timeline.points[last], args.offline, &extra);
    let left = bisect::manifest_diff(&original, &installed_now());
    let mut incomplete = false;
    if left.is_empty() {
        say!(
            Voice::Confirms,
            "the system is back as it was: every package is at the version it had."
        );
    } else {
        incomplete = true;
        say!(
            Voice::Warns,
            "the system is NOT back as it was ({} difference{}):",
            left.len(),
            if left.len() == 1 { "" } else { "s" }
        );
        for line in left.iter().take(12) {
            println!("  {line}");
        }
        if let Err(err) = &back {
            say!(Voice::Warns, "putting it back failed — {err}");
        }
        say!(
            Voice::Suggests,
            "no more steps will be taken. `emrac restore {} --prune` returns to the saved state (the package files it needs are in the cache); `emrac doctor` and `emrac verify` check the result.",
            safety.name
        );
    }

    if incomplete {
        return Err(Error::Help(
            "bisection ended with the system not back where it started (see above)".into(),
        ));
    }
    if let Some(why) = stopped {
        return Err(Error::Help(format!("the search stopped: {why}")));
    }
    match outcome {
        Some(Outcome::Found(i)) => {
            let tx = timeline.points[i].tx.clone().unwrap_or_default();
            say!(
                Voice::Found,
                "the first state where it is broken is {} ({}), after {looked} state{} looked at.",
                timeline.points[i].label,
                emrac_core::format_utc(timeline.points[i].timestamp),
                if looked == 1 { "" } else { "s" }
            );
            for change in bisect::changed(&timeline.points[i - 1], &timeline.points[i]) {
                println!("  {}", describe(&change));
            }
            say!(
                Voice::Suggests,
                "`emrac history` shows {tx}. To go back with one package: `emrac downgrade <package>`; `emrac pin <package>` keeps it there (and holds it back from upgrades)."
            );
        }
        Some(Outcome::Inconclusive { after, up_to }) => {
            let blocked =
                bisect::protected_changes(&timeline.points[after], &timeline.points[up_to], &extra);
            if blocked.is_empty() {
                say!(
                    Voice::Warns,
                    "the states in between could not be judged: it broke somewhere in {} \u{2026} {}.",
                    timeline.points[after + 1].label,
                    timeline.points[up_to].label
                );
            } else {
                return Err(Error::Help(format!(
                    "bisection stopped: it broke somewhere in {} \u{2026} {}, and finding which transaction there requires changing protected package(s): {}. No further package changes were made. Continue in an isolated environment (a disposable VM) to test those candidates — that mode is planned, not built. Not reproduced is not ruled out.",
                    timeline.points[after + 1].label,
                    timeline.points[up_to].label,
                    emrac_core::protected::describe(&blocked)
                )));
            }
        }
        _ => {}
    }
    back
}

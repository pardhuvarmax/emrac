use emrac_core::{say, Voice};
use std::collections::HashMap;

use emrac_core::{
    journal, ledger, ledger_watch, parse_date, parse_duration, parse_tx_id, rolled_back_by, Config,
    Error, History, Plan, Result, RollbackPlan, RECENT_KEPT,
};

use crate::{output, prompt};

pub fn run(limit: Option<usize>, since: Option<&str>, archive: bool, json: bool) -> Result<()> {
    let since = since.map(parse_date).transpose()?;
    let records = if archive {
        History::list_with_archive(limit, since)?
    } else {
        History::list(limit, since)?
    };
    // Markers need the whole log: the rollback that reversed a record may be
    // newer than the `--since` window or past `--limit`.
    let all = if archive {
        History::all_with_archive()?
    } else {
        History::all()?
    };
    let rolled_back: HashMap<String, String> = records
        .iter()
        .filter_map(|r| rolled_back_by(&all, &r.id).map(|by| (r.id.clone(), by.id.clone())))
        .collect();
    let acknowledged = History::acknowledged(archive).unwrap_or_default();
    let archives = if archive {
        Vec::new()
    } else {
        History::archives().unwrap_or_default()
    };
    output::print_history(&records, &rolled_back, &acknowledged, &archives, json);
    Ok(())
}

/// `history archive --older-than AGE`.
pub fn archive(older_than: &str, dry_run: bool, json: bool) -> Result<()> {
    let secs = parse_duration(older_than)?;
    let keep_days = Config::load()?.cache.keep_days;
    if secs < u64::from(keep_days) * 86_400 {
        return Err(Error::History(format!(
            "--older-than {older_than} is shorter than cache.keep_days ({keep_days} days): \
             the cache policy still needs those records to know what `undo` may need. \
             Use {keep_days}d or more, or lower cache.keep_days first."
        )));
    }
    let Some(plan) = History::archive_plan(secs)? else {
        if json {
            println!("{}", serde_json::json!({ "archived": 0 }));
        } else {
            say!(
                Voice::Notes,
                "nothing to archive: no transaction is older than {older_than} outside the newest {RECENT_KEPT} (which undo and the cache policy read)."
            );
        }
        return Ok(());
    };
    let dir = History::path()?
        .parent()
        .map(|p| p.join("history-archive").join(&plan.file));
    let where_to = dir.map_or(plan.file.clone(), |p| p.display().to_string());
    if !json {
        say!(
            Voice::Plans,
            "{} would move to {where_to}: {} to {} ({} to {} UTC); {} stay in the live history.",
            plural_tx(plan.records),
            plan.first_id,
            plan.last_id,
            format_date(plan.first_time),
            format_date(plan.last_time),
            plan.kept_records
        );
    }
    if dry_run {
        if json {
            println!("{}", serde_json::to_string_pretty(&plan).unwrap());
        } else {
            say!(Voice::Notes, "dry run: nothing was moved.");
        }
        return Ok(());
    }
    // Rewriting the history is deliberate: --yes does not answer this.
    if !prompt::confirm_default_no("Move them to the archive?") {
        say!(Voice::Notes, "nothing was moved.");
        return Ok(());
    }
    let done = History::archive(secs)?.unwrap_or(plan);
    super::ledger::sync_after();
    if json {
        println!("{}", serde_json::to_string_pretty(&done).unwrap());
    } else {
        say!(
            Voice::Confirms,
            "archived {} ({} to {}). `emrac history --archive` still shows them, and `emrac history verify` checks them.",
            plural_tx(done.records),
            done.first_id,
            done.last_id
        );
    }
    Ok(())
}

/// `history verify`: the hash chain over the log and its archives, and
/// the comparison with the ledger.
pub fn verify(repair: bool, against: Option<&str>, json: bool) -> Result<()> {
    let report = History::verify()?;
    let history_path = History::path()?;
    let ledger_path = ledger::path(ledger::running_as_root());
    let ledger = ledger::read(&ledger_path)?;
    let comparison = match &ledger {
        Some(l) if l.problem.is_none() => Some(ledger::compare(l, &history_path)?),
        _ => None,
    };
    let ledger_trouble = ledger.as_ref().is_some_and(|l| l.problem.is_some())
        || comparison.as_ref().is_some_and(|c| !c.is_clean());

    if json {
        let mut value = serde_json::to_value(&report).unwrap();
        value["ledger"] = match &ledger {
            None => serde_json::Value::Null,
            Some(l) => serde_json::json!({
                "entries": l.count,
                "break": l.problem,
                "comparison": comparison,
                "protection": ledger::protection(&ledger_path),
            }),
        };
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        print_chain(&report);
        print_ledger(&ledger, comparison.as_ref(), &report);
    }

    if let Some(code) = against {
        check_against(code, json)?;
    }
    if !json && ledger.is_some() {
        let watch = ledger_watch::facts(&super::ledger::trusted_exes());
        if !watch.foreign.is_empty() || !watch.unwitnessed.is_empty() {
            say!(
                Voice::Warns,
                "{} event(s) in the audit log touched the ledger and were not emrac's own appends; `emrac ledger events` lists them.",
                watch.foreign.len() + watch.unwitnessed.len()
            );
        }
    }
    let chain_broken = report.problem.is_some();
    if repair && (chain_broken || ledger_trouble) {
        return repair_from_ledger(ledger.as_ref(), &history_path);
    }
    if let Some(problem) = &report.problem {
        return Err(Error::History(format!(
            "the history does not verify — {}{}",
            problem.describe(),
            if ledger.is_some() {
                ". `emrac history verify --repair` rebuilds it from the ledger."
            } else {
                ""
            }
        )));
    }
    if let Some(l) = &ledger {
        if let Some(b) = &l.problem {
            return Err(Error::History(format!(
                "the ledger does not verify — {}",
                b.describe()
            )));
        }
    }
    if let Some(c) = &comparison {
        if !c.is_clean() {
            return Err(Error::History(format!(
                "the history disagrees with the ledger — {}. `emrac history verify --repair` rebuilds it from the ledger.",
                describe_differences(c)
            )));
        }
    }
    Ok(())
}

/// `history verify --against CODE`.
fn check_against(code: &str, json: bool) -> Result<()> {
    use emrac_core::AnchorCheck;
    let result = History::check_anchor(code)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    }
    match result {
        AnchorCheck::Matches => {
            if !json {
                say!(
                    Voice::Confirms,
                    "the history still matches the anchor {code}: nothing up to that point was rewritten."
                );
            }
            Ok(())
        }
        AnchorCheck::Differs { found } => Err(Error::History(format!(
            "the history does not match the anchor {code}: that line now hashes to {found}…. Something up to that point was rewritten, even if the chain still holds."
        ))),
        AnchorCheck::Shorter { head_seq } => Err(Error::History(format!(
            "the history ends at line #{head_seq}, before the anchor {code}: lines were cut off the end."
        ))),
        AnchorCheck::Broken { at } => Err(Error::History(format!(
            "the history cannot vouch for the anchor {code}: its chain breaks first — {}",
            at.describe()
        ))),
    }
}

/// `history anchor`: the code to keep somewhere else.
pub fn anchor(json: bool) -> Result<()> {
    let Some(code) = History::anchor()? else {
        return Err(Error::History(
            "nothing in the history is chained yet, so there is nothing to anchor (the next transaction starts the chain)".into(),
        ));
    };
    if json {
        println!("{}", serde_json::json!({ "anchor": code }));
    } else {
        println!("{code}");
        say!(
            Voice::Notes,
            "keep that where this machine cannot change it (a note, a message to yourself, a repository). `emrac history verify --against {code}` later shows whether anything up to that line was rewritten or cut off."
        );
    }
    Ok(())
}

fn describe_differences(c: &ledger::Comparison) -> String {
    let mut parts = Vec::new();
    if !c.missing.is_empty() {
        parts.push(format!(
            "missing from your history: {}",
            c.missing.join(", ")
        ));
    }
    if !c.changed.is_empty() {
        parts.push(format!(
            "changed since they were recorded: {}",
            c.changed.join(", ")
        ));
    }
    if !c.archives.is_empty() {
        parts.push(format!(
            "archive files that differ: {}",
            c.archives.join(", ")
        ));
    }
    parts.join("; ")
}

fn print_chain(report: &emrac_core::HistoryVerify) {
    if report.problem.is_none() {
        if report.chained + report.legacy == 0 {
            say!(
                Voice::Notes,
                "there is no history yet, so nothing to verify."
            );
            return;
        }
        say!(
            Voice::Confirms,
            "the history verifies: {} chained{}{}.",
            plural_tx(report.chained),
            if report.archives > 0 {
                format!(", {} archive file(s) checked", report.archives)
            } else {
                String::new()
            },
            if report.legacy > 0 {
                format!(
                    "; {} earlier lines predate the chain and are not covered by it",
                    report.legacy
                )
            } else {
                String::new()
            }
        );
    }
}

fn print_ledger(
    ledger: &Option<ledger::Ledger>,
    comparison: Option<&ledger::Comparison>,
    report: &emrac_core::HistoryVerify,
) {
    let Some(l) = ledger else {
        if report.chained + report.legacy > 0 {
            say!(
                Voice::Notes,
                "there is no ledger yet: nothing outside your home holds a second copy. The next transaction starts it (it needs sudo); `emrac history sync` does it now."
            );
            if let (Some(seq), Some(hash)) = (report.head_seq, &report.head_hash) {
                say!(
                    Voice::Notes,
                    "newest line: #{seq}, {}…. Keep that somewhere the history file can't reach: a copy cut off the end of the log shows up only by comparing it.",
                    &hash[..16]
                );
            }
        }
        return;
    };
    if let Some(b) = &l.problem {
        say!(
            Voice::Warns,
            "the ledger's own chain is broken — {}",
            b.describe()
        );
        return;
    }
    let Some(c) = comparison else { return };
    if c.is_clean() {
        say!(
            Voice::Confirms,
            "the ledger agrees: {} entries, its own chain holds, every record matches.",
            l.count
        );
    } else {
        say!(
            Voice::Warns,
            "your history disagrees with the ledger — {}.",
            describe_differences(c)
        );
    }
    if c.pending > 0 {
        say!(
            Voice::Notes,
            "{} line(s) are not in the ledger yet; `emrac history sync` sends them.",
            c.pending
        );
    }
}

fn repair_from_ledger(
    ledger: Option<&ledger::Ledger>,
    history_path: &std::path::Path,
) -> Result<()> {
    let Some(l) = ledger.filter(|l| l.problem.is_none()) else {
        return Err(Error::History(
            "there is no intact ledger to rebuild from".into(),
        ));
    };
    let rebuilt = ledger::rebuild(l, history_path);
    say!(
        Voice::Plans,
        "rebuild your history from the ledger: {} transactions in the live history (older ones stay in their archive files). The current file is kept as history.jsonl.broken.",
        rebuilt.records
    );
    if !prompt::confirm_default_no("Rebuild it?") {
        say!(Voice::Notes, "nothing was changed.");
        return Ok(());
    }
    ledger::write_rebuilt(history_path, &rebuilt)?;
    let report = History::verify()?;
    match report.problem {
        None => say!(Voice::Confirms, "rebuilt; the history verifies again."),
        Some(b) => say!(
            Voice::Warns,
            "rebuilt, but it still does not verify — {}",
            b.describe()
        ),
    }
    Ok(())
}

/// `history ack`: marks failures as seen without deleting them.
pub fn ack(ids: &[String], failed: bool, json: bool) -> Result<()> {
    let ids: Vec<String> = if failed {
        Vec::new()
    } else {
        ids.iter().map(|i| parse_tx_id(i)).collect::<Result<_>>()?
    };
    let done = History::acknowledge(&ids)?;
    if !done.is_empty() {
        super::ledger::sync_after();
    }
    if json {
        println!("{}", serde_json::json!({ "acknowledged": done }));
    } else if done.is_empty() {
        say!(Voice::Notes, "nothing new to acknowledge.");
    } else {
        say!(
            Voice::Confirms,
            "acknowledged {}. They stay in your history (marked) and `doctor` stops counting them.",
            done.join(", ")
        );
    }
    Ok(())
}

fn plural_tx(n: usize) -> String {
    if n == 1 {
        "1 transaction".into()
    } else {
        format!("{n} transactions")
    }
}

fn format_date(ts: u64) -> String {
    emrac_core::format_utc_date(ts)
}

/// Logs an executed transaction. Never fails the command it follows: the
/// transaction's own result is what the user cares about, so a history
/// write problem is only a warning.
pub fn record(plan: &Plan, outcome: &Result<()>) {
    log(plan, outcome, None);
}

/// Same as `record`, for a rollback: also notes which transaction it reversed.
pub fn record_rollback(rollback: &RollbackPlan, outcome: &Result<()>) {
    log(&rollback.plan, outcome, Some(&rollback.target));
}

/// One step of a longer transaction (a restore runs several plans): recorded,
/// but the journal stays until the whole transaction ends.
pub fn record_step(plan: &Plan, outcome: &Result<()>) {
    log_step(plan, outcome, None);
}

fn log(plan: &Plan, outcome: &Result<()>, rollback_of: Option<&str>) {
    log_step(plan, outcome, rollback_of);
    journal::finish();
}

fn log_step(plan: &Plan, outcome: &Result<()>, rollback_of: Option<&str>) {
    match History::record(plan, outcome, rollback_of) {
        Ok(_) => super::ledger::sync_after(),
        Err(err) => say!(Voice::Warns, "couldn't record this in your history — {err}"),
    }
}

/// Starts the journal for a transaction that is about to run. A cut-off
/// transaction left by an earlier run is put on record first; another emrac
/// changing packages right now is a refusal.
pub fn begin(plan: &Plan) -> Result<()> {
    match journal::state()? {
        journal::State::Nothing => {}
        journal::State::Running => {
            return Err(Error::Journal(
                "another emrac is changing packages right now: wait for it to finish".into(),
            ))
        }
        journal::State::Interrupted(found) => {
            recover_found(*found, false, false)?;
        }
    }
    if let Err(err) = journal::begin(plan, journal::unix_now()) {
        // The journal protects against a crash; failing to write it must not
        // block the transaction, but the user is told.
        if matches!(err, Error::Journal(ref m) if m.starts_with("another emrac")) {
            return Err(err);
        }
        say!(
            Voice::Warns,
            "couldn't write the transaction journal ({err}); if this run is cut off, emrac won't know what it did."
        );
    }
    Ok(())
}

/// `history recover`: finds a transaction that was cut off, says what is
/// known about it, and puts it on record.
pub fn recover(dry_run: bool, json: bool) -> Result<()> {
    match journal::state()? {
        journal::State::Nothing => {
            if json {
                println!("{}", serde_json::json!({ "interrupted": false }));
            } else {
                say!(
                    Voice::Notes,
                    "no interrupted transaction: nothing to recover."
                );
            }
            Ok(())
        }
        journal::State::Running => {
            say!(
                Voice::Notes,
                "another emrac is changing packages right now; nothing is interrupted."
            );
            Ok(())
        }
        journal::State::Interrupted(found) => recover_found(*found, dry_run, json),
    }
}

fn recover_found(found: journal::Interrupted, dry_run: bool, json: bool) -> Result<()> {
    let Some(entry) = found.entry.clone() else {
        if dry_run {
            say!(
                Voice::Suspects,
                "a transaction was cut off while its journal was being written, so nothing is known about it."
            );
            return Ok(());
        }
        say!(
            Voice::Suspects,
            "a transaction was cut off before its journal was complete: nothing is known about what it did. Run `emrac verify` on what you were changing, and check `emrac doctor`."
        );
        return found.clear();
    };
    let range = found.log_range();
    let sources = emrac_core::Sources::init()?;
    let assessment = journal::assess(&entry, &range, &|n| sources.installed_version(n));
    let summary = assessment.summary(&entry);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "interrupted": true,
                "recorded": !dry_run,
                "started": entry.started,
                "action": entry.action,
                "done": assessment.done,
                "not_reached": assessment.not_reached,
                "changed_otherwise": assessment.other,
                "pacman_unfinished": assessment.pacman_unfinished,
            })
        );
    } else {
        say!(Voice::Suspects, "{summary}");
        if assessment.pacman_unfinished {
            say!(
                Voice::Suggests,
                "pacman may have stopped in the middle of a package: `emrac verify` on the packages above will tell."
            );
        }
        if std::path::Path::new("/var/lib/pacman/db.lck").exists() {
            say!(
                Voice::Suggests,
                "pacman's lock /var/lib/pacman/db.lck is still there; once no pacman is running, `sudo rm /var/lib/pacman/db.lck` frees it."
            );
        }
    }
    if dry_run {
        return Ok(());
    }
    match History::record_interrupted(&entry, &assessment) {
        Ok(record) => {
            super::ledger::sync_after();
            if !json {
                say!(
                    Voice::Remembers,
                    "recorded as {} in your history (failed; `emrac history ack {}` once you have dealt with it).",
                    record.id,
                    record.id
                );
            }
            found.clear()
        }
        Err(err) => {
            say!(
                Voice::Warns,
                "couldn't record the interrupted transaction ({err}); the journal stays so you can try again."
            );
            Ok(())
        }
    }
}

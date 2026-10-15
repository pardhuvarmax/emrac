use emrac_core::{say, Voice};
use std::collections::HashMap;

use emrac_core::{
    parse_date, parse_duration, parse_tx_id, rolled_back_by, Config, Error, History, Plan, Result,
    RollbackPlan, RECENT_KEPT,
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

/// `history verify`: the hash chain over the log and its archives.
pub fn verify(json: bool) -> Result<()> {
    let report = History::verify()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else if report.problem.is_none() {
        if report.chained + report.legacy == 0 {
            say!(
                Voice::Notes,
                "there is no history yet, so nothing to verify."
            );
            return Ok(());
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
        if let (Some(seq), Some(hash)) = (report.head_seq, &report.head_hash) {
            say!(
                Voice::Notes,
                "newest line: #{seq}, {}…. Keep that somewhere the history file can't reach: a copy cut off the end of the log shows up only by comparing it.",
                &hash[..16]
            );
        }
    }
    match report.problem {
        Some(problem) => Err(Error::History(format!(
            "the history does not verify — {}",
            problem.describe()
        ))),
        None => Ok(()),
    }
}

/// `history ack`: marks failures as seen without deleting them.
pub fn ack(ids: &[String], failed: bool, json: bool) -> Result<()> {
    let ids: Vec<String> = if failed {
        Vec::new()
    } else {
        ids.iter().map(|i| parse_tx_id(i)).collect::<Result<_>>()?
    };
    let done = History::acknowledge(&ids)?;
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

fn log(plan: &Plan, outcome: &Result<()>, rollback_of: Option<&str>) {
    if let Err(err) = History::record(plan, outcome, rollback_of) {
        say!(Voice::Warns, "couldn't record this in your history — {err}");
    }
}

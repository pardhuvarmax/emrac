//! `emrac news` and the news gate in `upgrade` (u20, part 2).

use emrac_core::arch_news::{self, Item, Loaded, Reason};
use emrac_core::{say, Config, Error, Result, Voice};

fn now() -> u64 {
    emrac_core::journal::unix_now()
}

/// When the last upgrade ran (unix seconds), from the history.
fn last_upgrade() -> Option<u64> {
    emrac_core::History::all()
        .ok()?
        .iter()
        .filter(|r| {
            r.action == emrac_core::PlanAction::Upgrade
                && r.status == emrac_core::TxStatus::Committed
        })
        .map(|r| r.timestamp)
        .max()
}

fn reasons_text(item: &Item) -> String {
    let r: Vec<&str> = arch_news::reasons(item)
        .into_iter()
        .map(Reason::phrase)
        .collect();
    r.join("; ")
}

fn print_item(item: &Item, unread: bool) {
    let critical = arch_news::is_critical(item);
    println!(
        "[{}] {}{}{}",
        emrac_core::format_utc_date(item.published),
        item.title,
        if unread { "  [unread]" } else { "" },
        if critical { "  [CRITICAL]" } else { "" }
    );
    if critical {
        println!("  Why:    {}", reasons_text(item));
    }
    println!("  Source: {}", item.link);
}

/// `emrac news`.
pub fn show(all: bool, offline: bool, json: bool) -> Result<()> {
    let loaded = arch_news::load_fresh(offline, now());
    let seen = arch_news::effective_seen(&loaded.state, last_upgrade(), now());
    let (age, stale) = arch_news::age_label(loaded.state.fetched_at, now());
    if json {
        let items: Vec<_> = loaded
            .items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "title": i.title, "link": i.link, "published": i.published,
                    "unread": i.published > seen,
                    "critical": arch_news::is_critical(i),
                    "reasons": arch_news::reasons(i).iter().map(|r| r.phrase()).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "age": age, "stale": stale, "problem": loaded.problem, "items": items,
            }))
            .unwrap()
        );
        return Ok(());
    }
    if let Some(why) = &loaded.problem {
        say!(
            Voice::Warns,
            "couldn't read the Arch news ({why}); showing what was cached ({age})."
        );
    }
    if loaded.items.is_empty() {
        say!(
            Voice::Notes,
            "no Arch news cached yet: run `emrac news` with the network."
        );
        return Ok(());
    }
    let shown: Vec<&Item> = loaded
        .items
        .iter()
        .enumerate()
        .filter(|(n, i)| all || *n < 10 || i.published > seen)
        .map(|(_, i)| i)
        .collect();
    let unread = arch_news::unread(&loaded.items, seen);
    let critical = unread.iter().filter(|i| arch_news::is_critical(i)).count();
    say!(
        Voice::Found,
        "{} Arch news item(s), {} unread, {} critical among them ({age})\n",
        loaded.items.len(),
        unread.len(),
        critical
    );
    for item in shown {
        print_item(item, item.published > seen);
        println!();
    }
    if !unread.is_empty() {
        println!("`emrac news ack` marks them as seen.");
    }
    Ok(())
}

/// `emrac news ack`.
pub fn ack(offline: bool, json: bool) -> Result<()> {
    let loaded = arch_news::load_fresh(offline, now());
    if loaded.items.is_empty() {
        return Err(Error::News(format!(
            "there is no news to mark as seen ({})",
            loaded.problem.unwrap_or_else(|| "the feed is empty".into())
        )));
    }
    let seen = arch_news::effective_seen(&loaded.state, last_upgrade(), now());
    let before = arch_news::unread(&loaded.items, seen).len();
    arch_news::acknowledge(&loaded.items)?;
    if json {
        println!("{}", serde_json::json!({ "acknowledged": before }));
    } else {
        say!(Voice::Confirms, "{before} news item(s) marked as seen.");
    }
    Ok(())
}

/// What the gate found for an upgrade.
pub struct Gate {
    loaded: Loaded,
}

impl Gate {
    /// Marks everything shown as seen, after the upgrade went through.
    pub fn done(&self) {
        let _ = arch_news::acknowledge(&self.loaded.items);
    }
}

/// The news gate for `upgrade`: shows unread news and stops for a critical
/// item unless acknowledged. `Ok(None)` when the gate is off.
pub fn gate(offline: bool, dry_run: bool, ack_news: bool, json: bool) -> Result<Option<Gate>> {
    let mode = Config::load()?.safety.arch_news;
    if mode == "off" {
        return Ok(None);
    }
    let loaded = arch_news::load_fresh(offline, now());
    let seen = arch_news::effective_seen(&loaded.state, last_upgrade(), now());
    let unread = arch_news::unread(&loaded.items, seen);
    let critical: Vec<&&Item> = unread
        .iter()
        .filter(|i| arch_news::is_critical(i))
        .collect();
    if !json {
        if let Some(why) = &loaded.problem {
            if !offline {
                say!(
                    Voice::Warns,
                    "couldn't read the Arch news ({why}); checking what was cached."
                );
            }
        }
        if !unread.is_empty() {
            println!("\nArch news you haven't seen:");
            for item in &unread {
                print_item(item, true);
            }
            println!();
        }
    }
    let blocking = mode == "gate" && !critical.is_empty() && !ack_news;
    if blocking && !dry_run {
        return Err(Error::News(format!(
            "{} unread Arch news item(s) are critical ({}). Read {}, then run `emrac news ack`, or pass --ack-news to go ahead now. Nothing was changed.",
            critical.len(),
            critical
                .iter()
                .flat_map(|i| arch_news::reasons(i))
                .map(Reason::phrase)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("; "),
            if critical.len() == 1 { "it" } else { "them" }
        )));
    }
    if blocking && dry_run && !json {
        say!(
            Voice::Notes,
            "this is a dry run, so nothing was stopped; the real run would refuse until the critical news is acknowledged."
        );
    }
    Ok(Some(Gate { loaded }))
}

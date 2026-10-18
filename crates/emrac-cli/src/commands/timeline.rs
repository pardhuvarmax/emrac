//! `emrac timeline` (u10): what happened to the system, day by day.

use emrac_core::build_stats::parse_since;
use emrac_core::timeline::{self, Filter};
use emrac_core::{parse_date, Error, Result, Sources};

/// `7d`, `12h`, `8w` back from `now`, or a `YYYY-MM-DD` date.
fn since_time(text: &str, now: u64) -> Result<u64> {
    if let Some(secs) = parse_since(text) {
        return Ok(now.saturating_sub(secs));
    }
    parse_date(text).map_err(|_| {
        Error::Benchmark(format!(
            "'{text}' is neither a period (30d, 12h, 8w) nor a date (YYYY-MM-DD)"
        ))
    })
}

pub fn run(sources: &Sources, since: Option<&str>, pkg: Option<&str>, json: bool) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let since_text = since.unwrap_or("7d");
    let filter = Filter {
        since: since_time(since_text, now)?,
        package: pkg.map(str::to_string),
    };
    let events = sources.timeline(&filter);
    if json {
        println!("{}", serde_json::to_string_pretty(&events).unwrap());
        return Ok(());
    }
    if events.is_empty() {
        match pkg {
            Some(p) => println!("Nothing recorded for {p} in the period ({since_text})."),
            None => println!("Nothing recorded in the period ({since_text})."),
        }
        return Ok(());
    }
    for (label, day) in timeline::group(events, now) {
        println!("{label}");
        for e in day {
            println!("  {}  {}", timeline::clock(e.time), e.text);
        }
    }
    println!();
    println!("Times are UTC. From emrac's own records: transactions, builds, snapshots and config file actions.");
    Ok(())
}

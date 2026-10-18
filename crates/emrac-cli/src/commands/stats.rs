//! `emrac stats` (u9): what emrac has built and installed here.

use emrac_core::build_stats::{self, Stats};
use emrac_core::{format_utc_date, Error, Result, Sources};

fn duration(secs: u64) -> String {
    if secs < 90 {
        format!("{secs} s")
    } else if secs < 5400 {
        format!("{} min", (secs + 30) / 60)
    } else {
        format!("{:.1} h", secs as f64 / 3600.0)
    }
}

pub fn run(sources: &Sources, since: Option<&str>, json: bool) -> Result<()> {
    let since_secs = match since {
        Some(text) => Some(build_stats::parse_since(text).ok_or_else(|| {
            Error::Benchmark(format!(
                "'{text}' is not a period; use a number with m, h, d or w, like 30d"
            ))
        })?),
        None => None,
    };
    let stats = sources.stats(since_secs);
    if json {
        println!("{}", serde_json::to_string_pretty(&stats).unwrap());
    } else {
        print_stats(&stats, since);
    }
    Ok(())
}

fn print_stats(s: &Stats, since: Option<&str>) {
    match (since, s.first) {
        (Some(p), _) => println!("Period       : the last {p}"),
        (None, Some(first)) => println!(
            "Period       : since {} (all that is kept)",
            format_utc_date(first)
        ),
        (None, None) => {}
    }
    let b = &s.builds;
    let t = &s.transactions;
    if b.total == 0 && t.committed + t.failed == 0 {
        println!("Nothing recorded for this period yet.");
        return;
    }
    println!();
    println!("Builds");
    if b.total == 0 {
        println!("  none recorded");
    } else {
        println!(
            "  {} built ({} failed), {} different packages",
            b.total, b.failed, b.packages
        );
        if b.total > b.failed {
            println!(
                "  time spent   : {} in all, {} on average",
                duration(b.total_secs),
                duration(b.average_secs)
            );
        }
        if let Some((name, secs)) = &b.longest {
            println!("  longest      : {name}, {}", duration(*secs));
        }
        if !b.most_built.is_empty() {
            println!("  built most   :");
            for p in &b.most_built {
                println!(
                    "    {:<28} {:>3}x  {} on average, last {}",
                    p.name,
                    p.builds,
                    duration(p.average_secs),
                    duration(p.last_secs)
                );
            }
        }
    }
    println!();
    println!("Transactions");
    println!(
        "  {} done ({} installs, {} upgrades, {} removals, {} other), {} failed",
        t.committed, t.installs, t.upgrades, t.removals, t.other, t.failed
    );
    let pkgs = t.aur_packages + t.repo_packages;
    if pkgs > 0 {
        println!(
            "  packages put on the system: {pkgs}, {} from the AUR, {} from the official repositories",
            t.aur_packages, t.repo_packages
        );
    }
    println!();
    println!(
        "Not shown: hours or storage saved by installing binaries instead of building. emrac measures \
         builds only, so a figure for the other side would be a guess."
    );
}

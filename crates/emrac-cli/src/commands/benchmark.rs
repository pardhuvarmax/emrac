//! `emrac benchmark` (u9): what an installed package costs to start.

use emrac_core::benchmark::{self, BenchRecord, Measurement};
use emrac_core::{format_utc_date, Error, Result, Sources};

use crate::prompt;

fn micros(us: u64) -> String {
    if us < 1_000 {
        format!("{us} \u{b5}s")
    } else if us < 1_000_000 {
        format!("{:.1} ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2} s", us as f64 / 1_000_000.0)
    }
}

fn kib(k: u64) -> String {
    if k < 1024 {
        format!("{k} KiB")
    } else {
        format!("{:.1} MiB", k as f64 / 1024.0)
    }
}

fn line(m: &Measurement) -> String {
    let mut s = format!(
        "  {:<28} size {:<9}",
        m.exe,
        crate::output::human_size(m.size_bytes)
    );
    match (m.first_micros, m.warm_micros) {
        (Some(f), Some(w)) => s.push_str(&format!(
            " first run {:<9} warm {:<9}",
            micros(f),
            micros(w)
        )),
        (Some(f), None) => s.push_str(&format!(" first run {:<9}", micros(f))),
        _ => s.push_str(" did not finish"),
    }
    match m.real_rss_kib() {
        Some(k) => s.push_str(&format!(" memory {}", kib(k))),
        None if m.max_rss_kib > 0 => s.push_str(" memory small"),
        None => {}
    }
    if m.timeouts > 0 && m.first_micros.is_some() {
        s.push_str(&format!(" ({} run(s) timed out)", m.timeouts));
    }
    if m.nonzero_exit {
        s.push_str(" [exit status not 0]");
    }
    s
}

pub fn run(
    sources: &Sources,
    pkg: &str,
    exe: Option<&str>,
    runs: u32,
    show_history: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    if show_history {
        let records = benchmark::history(pkg);
        if json {
            println!("{}", serde_json::to_string_pretty(&records).unwrap());
            return Ok(());
        }
        if records.is_empty() {
            println!("No benchmark of {pkg} yet. Run `emrac benchmark {pkg}`.");
        }
        for r in &records {
            println!(
                "{} {} {}",
                format_utc_date(r.time),
                r.version,
                line(&r.measurement).trim_start()
            );
        }
        return Ok(());
    }
    if runs == 0 || runs > benchmark::MAX_RUNS {
        return Err(Error::Benchmark(format!(
            "--runs must be between 1 and {}",
            benchmark::MAX_RUNS
        )));
    }
    let targets = sources.benchmark_targets(pkg)?;
    let mut chosen = targets.executables.clone();
    if let Some(name) = exe {
        chosen.retain(|p| p.file_name().is_some_and(|f| f == name));
        if chosen.is_empty() {
            return Err(Error::Benchmark(format!(
                "{pkg} has no executable called '{name}' in /usr/bin or /usr/sbin"
            )));
        }
    }
    if chosen.is_empty() {
        return Err(Error::Benchmark(format!(
            "{pkg} installs no program in /usr/bin or /usr/sbin, so there is nothing to start"
        )));
    }
    if !json {
        println!(
            "This runs `<program> --version` {} times for each of these, as you:",
            runs + 1
        );
        for p in &chosen {
            println!("  {}", p.display());
        }
        println!("Running a package's program executes its code.");
    }
    if !yes && !prompt::confirm_default_no("Run them?") {
        println!("Nothing was run.");
        return Ok(());
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let mut records = Vec::new();
    for path in &chosen {
        let measurement = benchmark::measure(path, runs, benchmark::RUN_LIMIT)?;
        let record = BenchRecord {
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            package: pkg.to_string(),
            version: targets.version.clone(),
            installed_size: targets.installed_size,
            cores,
            measurement,
        };
        benchmark::record(&record)?;
        records.push(record);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&records).unwrap());
        return Ok(());
    }
    println!();
    println!(
        "{pkg} {} (installed size {})",
        targets.version,
        crate::output::human_size(targets.installed_size)
    );
    for r in &records {
        println!("{}", line(&r.measurement));
    }
    println!();
    println!(
        "First run is the first start after this command began, not an empty cache; warm is the median of the \
         later runs. {} cores. \"Memory small\" means the peak is not above what any program started this way \
         shows (the size of emrac itself, about {} KiB), so it is not measured more finely. This is the cost of starting the program with `--version`, not of a workload.",
        cores,
        records.first().map_or(0, |r| r.measurement.floor_rss_kib)
    );
    Ok(())
}

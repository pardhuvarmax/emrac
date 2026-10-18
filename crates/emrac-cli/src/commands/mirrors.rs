//! `emrac mirrors`, `mirrors test`, `mirrors rank` (u16).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use emrac_core::mirrors::{self, Group, Ranked, Sample, Stats};
use emrac_core::style::{Activity, Style};
use emrac_core::{Error, Result};

use crate::prompt;

pub const DEFAULT_LIST: &str = "/etc/pacman.d/mirrorlist";
const TIMEOUT: Duration = Duration::from_secs(10);

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn bad(why: impl Into<String>) -> Error {
    Error::Mirrors(why.into())
}

fn read_list(file: &str) -> Result<(String, Vec<String>)> {
    let text = std::fs::read_to_string(file).map_err(|e| bad(format!("{file}: {e}")))?;
    let active: Vec<String> = mirrors::parse(&text)
        .into_iter()
        .filter(|e| e.active)
        .map(|e| e.url)
        .collect();
    if active.is_empty() {
        return Err(bad(format!("{file} has no active Server line")));
    }
    Ok((text, active))
}

fn age(secs: u64) -> String {
    if secs < 5400 {
        format!("{} min", (secs + 30) / 60)
    } else if secs < 2 * 86_400 {
        format!("{:.1} h", secs as f64 / 3600.0)
    } else {
        format!("{} d", secs / 86_400)
    }
}

fn short(url: &str) -> String {
    let u = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let cut = u.find("/$repo").unwrap_or(u.len());
    let host = &u[..cut];
    let mut s: String = host.chars().take(46).collect();
    if host.chars().count() > 46 {
        s.push('\u{2026}');
    }
    s
}

fn row(n: usize, url: &str, group: Group, stats: Option<&Stats>) -> String {
    let (ok, median, speed, sync) = match stats {
        Some(s) if s.attempts > 0 => (
            format!("{}/{}", s.recent_successes, s.recent_attempts),
            s.median_total_ms.map_or("-".into(), |m| format!("{m} ms")),
            s.kib_per_s.map_or("-".into(), |k| format!("{k} KiB/s")),
            s.sync_age_s.map_or("?".into(), age),
        ),
        _ => ("-".into(), "-".into(), "-".into(), "-".into()),
    };
    format!(
        "{n:>3}  {:<47} {:<11} {ok:<6} {median:<9} {speed:<11} {sync}",
        short(url),
        group.word()
    )
}

const HEADER: &str = "  #  mirror                                          group       last10 median    speed       behind Arch";

fn print_table(ranked: &[Ranked]) {
    println!("{HEADER}");
    for (i, r) in ranked.iter().enumerate() {
        println!("{}", row(i + 1, &r.url, r.group, r.stats.as_ref()));
    }
}

pub fn list(file: &str, json: bool) -> Result<()> {
    let (_, active) = read_list(file)?;
    let history = mirrors::read_history();
    let stats = mirrors::summarize(&history);
    let in_file_order: Vec<Ranked> = active
        .iter()
        .map(|u| Ranked {
            url: u.clone(),
            group: mirrors::group_of(stats.get(u)),
            stats: stats.get(u).cloned(),
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&in_file_order).unwrap());
        return Ok(());
    }
    println!(
        "{} active mirrors in {file}; {} measurements in the history",
        active.len(),
        history.len()
    );
    println!();
    let st = Style::stdout();
    if st.fancy {
        print!("{}", crate::view::mirrors_view(&st, &in_file_order));
    } else {
        print_table(&in_file_order);
    }
    println!();
    if history.is_empty() {
        println!("Nothing measured yet: `emrac mirrors test` measures them, `emrac mirrors rank` orders them.");
    } else {
        println!("`emrac mirrors rank` shows the order the history supports.");
    }
    Ok(())
}

/// Measures `urls` with up to `jobs` at a time; results in the order given.
fn measure_all(urls: &[String], jobs: usize, activity: &Activity) -> Vec<Sample> {
    let t = now();
    let arch = std::env::consts::ARCH;
    let mut out: Vec<Option<Sample>> = vec![None; urls.len()];
    let chunk = jobs.max(1);
    for (ci, batch) in urls.chunks(chunk).enumerate() {
        let results: Vec<Sample> = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|u| scope.spawn(move || mirrors::measure(u, "core", arch, t, TIMEOUT)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (i, r) in results.into_iter().enumerate() {
            out[ci * chunk + i] = Some(r);
        }
        let done = ((ci + 1) * chunk).min(urls.len());
        if activity.is_active() {
            activity.set(
                done,
                &crate::view::short_url(batch.last().map_or("", |u| u.as_str())),
            );
        } else {
            eprintln!("measured {done} of {}", urls.len());
        }
    }
    out.into_iter().flatten().collect()
}

pub fn test(file: &str, limit: usize, jobs: usize, json: bool) -> Result<()> {
    let (_, active) = read_list(file)?;
    let urls: Vec<String> = active.into_iter().take(limit).collect();
    let activity = Activity::progress("measuring mirrors", urls.len());
    let samples = measure_all(&urls, jobs, &activity);
    activity.finish(&format!("measured {} mirrors", samples.len()));
    mirrors::record(&samples)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&samples).unwrap());
        return Ok(());
    }
    let st = Style::stdout();
    if st.fancy {
        print!("{}", crate::view::mirror_test_view(&st, &samples));
        let ok = samples.iter().filter(|s| s.ok).count();
        println!();
        println!(
            "{ok} of {} answered. Recorded in the history; `emrac mirrors rank` uses it.",
            samples.len()
        );
        return Ok(());
    }
    println!(
        "{:<48} {:<7} {:<9} {:<9} behind Arch",
        "mirror", "result", "headers", "total"
    );
    for s in &samples {
        println!(
            "{:<48} {:<7} {:<9} {:<9} {}",
            short(&s.url),
            if s.ok { "ok" } else { "FAILED" },
            if s.ok {
                format!("{} ms", s.ttfb_ms)
            } else {
                "-".into()
            },
            if s.ok {
                format!("{} ms", s.total_ms)
            } else {
                "-".into()
            },
            if s.ok {
                s.sync_age_s.map_or("unknown".into(), age)
            } else {
                s.error.clone().unwrap_or_default()
            }
        );
    }
    let ok = samples.iter().filter(|s| s.ok).count();
    println!();
    println!(
        "{ok} of {} answered. Recorded in the history; `emrac mirrors rank` uses it.",
        samples.len()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn rank(
    file: &str,
    keep: Option<usize>,
    write: Option<&str>,
    apply: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    let (original, active) = read_list(file)?;
    let stats = mirrors::summarize(&mirrors::read_history());
    if !active.iter().any(|u| stats.contains_key(u)) {
        return Err(bad(
            "nothing has been measured yet: run `emrac mirrors test` first",
        ));
    }
    let ranked = mirrors::rank(&active, &stats);
    let order: Vec<String> = ranked.iter().map(|r| r.url.clone()).collect();
    let proposal = mirrors::render_mirrorlist(&original, &order, keep)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&ranked).unwrap());
    } else {
        let st = Style::stdout();
        if st.fancy {
            print!("{}", crate::view::mirrors_view(&st, &ranked));
        } else {
            print_table(&ranked);
        }
        println!();
        println!("Order: good (fastest first), stale (more than a day behind Arch), unreliable (fewer than 8 of the last 10 worked, or the last failed), untested.");
        if order == active && keep.is_none_or(|k| k >= order.len()) {
            println!("{file} is already in this order.");
        }
    }
    if let Some(path) = write {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| bad(format!("{path}: {e}")))?;
        f.write_all(proposal.as_bytes())
            .map_err(|e| bad(format!("{path}: {e}")))?;
        eprintln!("wrote {path}");
    }
    if apply {
        apply_list(file, &original, &proposal, yes)?;
    } else if write.is_none() && !json {
        println!("Nothing was changed. `--write FILE` saves this order to a file, `--apply` puts it in {file}.");
    }
    Ok(())
}

fn apply_list(file: &str, original: &str, proposal: &str, yes: bool) -> Result<()> {
    if proposal == original {
        println!("Nothing to change.");
        return Ok(());
    }
    println!();
    println!("This replaces {file}. The mirrors stay the same ones; only the order (and, with --keep, which are active) changes.");
    println!("pacman's databases are not refreshed by it: run `sudo pacman -Sy` or `emrac upgrade` after.");
    if !yes && !prompt::confirm_default_no("Replace the mirror list?") {
        println!("okay, nothing changed.");
        return Ok(());
    }
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    let dir = PathBuf::from(home).join(".local/share/emrac/mirrorlist-backups");
    std::fs::create_dir_all(&dir).map_err(|e| bad(format!("{}: {e}", dir.display())))?;
    let backup = dir.join(format!("mirrorlist-{}", now()));
    std::fs::write(&backup, original).map_err(|e| bad(format!("{}: {e}", backup.display())))?;
    let tmp = std::env::temp_dir().join(format!("emrac-mirrorlist-{}", std::process::id()));
    std::fs::write(&tmp, proposal).map_err(|e| bad(format!("{}: {e}", tmp.display())))?;
    let status = Command::new("sudo")
        .args(["install", "-m", "644"])
        .arg(&tmp)
        .arg(file)
        .status();
    let _ = std::fs::remove_file(&tmp);
    match status {
        Ok(s) if s.success() => {
            println!(
                "done. The old list is saved: put it back with `sudo cp {} {file}`",
                backup.display()
            );
            Ok(())
        }
        Ok(s) => Err(bad(format!(
            "sudo install failed ({s}); {file} was not changed"
        ))),
        Err(e) => Err(bad(format!(
            "couldn't run sudo: {e}; {file} was not changed"
        ))),
    }
}

pub fn default_file() -> &'static str {
    DEFAULT_LIST
}

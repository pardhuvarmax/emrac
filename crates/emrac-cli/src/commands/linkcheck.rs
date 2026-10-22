//! `emrac linkcheck` (u25): programs that no package owns (`/usr/local`,
//! `~/.local`, unowned files under `/opt`) and the libraries they need. Without
//! a flag: which of them cannot find a library now. With `--upgrade`: which
//! would lose one to the upgrade that is waiting.

use emrac_core::{say, Result, Sources, Voice};

pub fn run(sources: &Sources, upgrade: bool, offline: bool, json: bool) -> Result<()> {
    if upgrade {
        let plan = sources.plan_upgrade(&[], offline)?.plan;
        let lines: Vec<&String> = plan
            .warnings
            .iter()
            .filter(|w| {
                w.contains("is not from a package") || w.contains("outside the package manager")
            })
            .collect();
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&lines).unwrap_or_default()
            );
            return Ok(());
        }
        if lines.is_empty() {
            say!(
                Voice::Confirms,
                "the upgrade that is waiting ({} package{}) takes no library away from a program outside the package manager.",
                plan.packages.len(),
                if plan.packages.len() == 1 { "" } else { "s" }
            );
        } else {
            for l in lines {
                say!(Voice::Warns, "{l}");
            }
        }
        return Ok(());
    }
    let binaries = sources.unpackaged_binaries();
    let system = emrac_core::stale_builds::system_libs();
    let broken = emrac_core::linkcheck::broken_now(&binaries, &system, &|p| p.exists());
    if json {
        let v: Vec<_> = broken
            .iter()
            .map(|(b, m)| serde_json::json!({"path": b.path, "missing": m}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    say!(
        Voice::Notes,
        "{} program{} outside the package manager looked at (/usr/local, ~/.local, unowned files under /opt).",
        binaries.len(),
        if binaries.len() == 1 { "" } else { "s" }
    );
    if broken.is_empty() {
        say!(Voice::Confirms, "none of them is missing a library now.");
    }
    for (b, missing) in &broken {
        say!(
            Voice::Warns,
            "{} cannot find {} (nothing installed provides it).",
            b.path,
            missing.join(", ")
        );
    }
    if !broken.is_empty() {
        say!(
            Voice::Suggests,
            "build it again against the libraries that are installed now; `emrac strays` lists what else no package owns."
        );
    }
    Ok(())
}

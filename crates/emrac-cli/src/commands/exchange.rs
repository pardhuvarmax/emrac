//! `emrac export` and `emrac import` (u12).

use std::io::Write;

use emrac_core::exchange::{self, ImportReport, Scope};
use emrac_core::{parse_date, Config, Error, PinStore, Result, Sources};

use crate::commands::aur_install::BuildChoice;
use crate::prompt;

/// `--packages --config --repos --pins`; none given means all of them.
pub fn scope_from(packages: bool, config: bool, repos: bool, pins: bool) -> Scope {
    if !(packages || config || repos || pins) {
        return Scope::ALL;
    }
    Scope {
        packages,
        config,
        repos,
        pins,
    }
}

pub fn export(
    sources: &Sources,
    config: &Config,
    scope: Scope,
    output: Option<&str>,
) -> Result<()> {
    let text = exchange::render(&sources.export(scope, config)?);
    match output {
        None => print!("{text}"),
        Some(path) => {
            // create_new: an existing file is never overwritten.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| Error::Exchange(format!("{path}: {e}")))?;
            file.write_all(text.as_bytes())
                .map_err(|e| Error::Exchange(format!("{path}: {e}")))?;
            eprintln!("wrote {path}");
        }
    }
    Ok(())
}

fn print_report(r: &ImportReport, scope: Scope) {
    if scope.packages {
        println!("Packages");
        println!(
            "  {} to install, {} already installed, {} left to come along as dependencies",
            r.to_install.len(),
            r.already_installed,
            r.dependencies_left
        );
        for p in &r.to_install {
            println!(
                "    install  {}{}",
                p.name,
                if p.foreign { "  (AUR)" } else { "" }
            );
        }
        for (name, theirs, ours) in r.version_differs.iter().take(10) {
            println!(
                "    differs  {name}: the file has {theirs}, this machine {ours} (left as it is)"
            );
        }
        if r.version_differs.len() > 10 {
            println!(
                "    and {} more at another version (left as they are)",
                r.version_differs.len() - 10
            );
        }
    }
    if scope.config {
        println!("Settings");
        if r.config_changes.is_empty() {
            println!("  nothing to change");
        }
        for c in &r.config_changes {
            println!("    {}: {} -> {}", c.key, c.from, c.to);
        }
        for (key, why) in &r.config_refused {
            println!("    not applied  {key}: {why}");
        }
    }
    if scope.pins {
        println!("Holds (pins)");
        if r.new_pins.is_empty() {
            println!("  nothing new");
        }
        for p in &r.new_pins {
            println!(
                "    {}{}{}",
                p.name,
                p.until
                    .as_ref()
                    .map_or(String::from("  (no end date)"), |u| format!("  until {u}")),
                p.reason
                    .as_ref()
                    .map_or(String::new(), |why| format!("  ({why})"))
            );
        }
    }
    if scope.repos {
        println!("Repositories");
        if r.repos_missing.is_empty() {
            println!("  all enabled here");
        } else {
            println!(
                "  not enabled here and never applied by import: {} (add them to pacman.conf yourself, if you trust them)",
                r.repos_missing.join(", ")
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn import(
    sources: &Sources,
    config: &Config,
    file: &str,
    scope: Scope,
    dry_run: bool,
    yes: bool,
    offline: bool,
    json: bool,
) -> Result<()> {
    let size = std::fs::metadata(file)
        .map_err(|e| Error::Exchange(format!("{file}: {e}")))?
        .len();
    if size as usize > exchange::MAX_BYTES {
        return Err(Error::Exchange(format!(
            "{file} is larger than {} MiB",
            exchange::MAX_BYTES >> 20
        )));
    }
    let text =
        std::fs::read_to_string(file).map_err(|e| Error::Exchange(format!("{file}: {e}")))?;
    let ex = exchange::parse(&text)?;
    let report = sources.import_report(&ex, scope, config)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }
    println!(
        "{file}: exported {} by emrac {}",
        ex.meta.exported, ex.meta.emrac
    );
    print_report(&report, scope);
    if dry_run {
        println!("\n(--dry-run: nothing was changed)");
        return Ok(());
    }

    if scope.config && !report.config_changes.is_empty() {
        println!();
        if yes
            || prompt::confirm_default_no(&format!(
                "Apply these {} settings?",
                report.config_changes.len()
            ))
        {
            let mut fresh = Config::load()?;
            exchange::apply_config(&mut fresh, &report.config_changes)?;
            fresh.save()?;
            println!("settings applied.");
        } else {
            println!("settings left as they are.");
        }
    }

    if scope.pins && !report.new_pins.is_empty() {
        println!();
        println!(
            "A hold keeps a package from getting updates, security fixes included. `--yes` does not answer this."
        );
        if prompt::confirm_default_no(&format!("Hold these {} packages?", report.new_pins.len())) {
            for p in &report.new_pins {
                let until = p.until.as_deref().map(parse_date).transpose()?;
                PinStore::pin(std::slice::from_ref(&p.name), p.reason.as_deref(), until)?;
            }
            println!("holds added.");
        } else {
            println!("no holds added.");
        }
    }

    if scope.packages && !report.to_install.is_empty() {
        println!();
        println!(
            "Installing {} packages through `emrac install` (the usual plan, checks and questions):",
            report.to_install.len()
        );
        let names: Vec<String> = report.to_install.iter().map(|p| p.name.clone()).collect();
        let choice = BuildChoice {
            interactive: !yes,
            limits: emrac_core::Limits::from_config(&config.build),
            remove_tools: crate::commands::aur_install::RemoveTools::from_config(&config.build),
            ..BuildChoice::default()
        };
        crate::commands::install::run(
            sources,
            &names,
            false,
            yes,
            offline,
            config.review.skip_pkgbuild,
            config.review.skip_diff,
            choice,
            config.review.inspect_official,
            false,
            false,
            None,
        )?;
    }
    Ok(())
}

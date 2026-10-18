//! `man`, `docs`, `license`, `credits`, `support`, `feedback`, `bug-report` (u13).

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

use clap::CommandFactory;
use emrac_core::{Error, Result, Sources};

use crate::cli::Cli;

pub const SITE: &str = "https://pardhuvarmax.github.io/emrac/";
pub const REPO: &str = "https://github.com/pardhuvarmax/emrac";
const LICENSE_TEXT: &str = include_str!("../../../../LICENSE");

/// Libraries the program is built with: (name, licence). Kept true to
/// `Cargo.toml` by a test.
pub const LIBRARIES: &[(&str, &str)] = &[
    ("alpm", "GPL-3.0"),
    ("clap", "MIT OR Apache-2.0"),
    ("clap_complete", "MIT OR Apache-2.0"),
    ("clap_mangen", "MIT OR Apache-2.0"),
    ("serde", "MIT OR Apache-2.0"),
    ("serde_json", "MIT OR Apache-2.0"),
    ("toml", "MIT OR Apache-2.0"),
    ("ureq", "MIT OR Apache-2.0"),
    ("flate2", "MIT OR Apache-2.0"),
    ("sha2", "MIT OR Apache-2.0"),
    ("thiserror", "MIT OR Apache-2.0"),
    ("libc", "MIT OR Apache-2.0"),
];

/// Programs it runs.
const PROGRAMS: &[&str] = &[
    "pacman and libalpm",
    "makepkg",
    "bubblewrap (the build sandbox)",
    "git",
];

/// Opens an address with `xdg-open`; says so when that is not possible.
fn open(url: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| Error::Help(format!("couldn't run xdg-open: {e}; open {url} yourself")))
}

fn find<'a>(root: &'a clap::Command, path: &[String]) -> Result<&'a clap::Command> {
    let mut cmd = root;
    for word in path {
        cmd = cmd.find_subcommand(word).ok_or_else(|| {
            let known: Vec<&str> = cmd
                .get_subcommands()
                .filter(|c| !c.is_hide_set())
                .map(clap::Command::get_name)
                .collect();
            Error::Help(format!(
                "no command '{word}' here; the commands are: {}",
                known.join(", ")
            ))
        })?;
    }
    Ok(cmd)
}

/// The command to document: `emrac` itself, or a (sub)command.
fn command_for(path: &[String]) -> Result<clap::Command> {
    let mut root = Cli::command();
    root.build();
    let path: Vec<String> = if path.first().is_some_and(|w| w == "emrac") {
        path[1..].to_vec()
    } else {
        path.to_vec()
    };
    Ok(find(&root, &path)?.clone())
}

pub fn man(path: &[String], roff: bool) -> Result<()> {
    let mut cmd = command_for(path)?;
    if roff || std::io::stdout().is_terminal() {
        let mut page = Vec::new();
        clap_mangen::Man::new(cmd.clone())
            .render(&mut page)
            .map_err(|e| Error::Help(format!("couldn't write the page: {e}")))?;
        if roff {
            std::io::stdout()
                .write_all(&page)
                .map_err(|e| Error::Help(e.to_string()))?;
            return Ok(());
        }
        if let Ok(mut child) = Command::new("man")
            .args(["-l", "-"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&page);
            }
            let _ = child.wait();
            return Ok(());
        }
    }
    println!("{}", cmd.render_long_help());
    Ok(())
}

pub fn docs(open_it: bool) -> Result<()> {
    println!("Documentation   {SITE}");
    println!("Source          {REPO}");
    println!("Changelog       {REPO}/blob/main/CHANGELOG.md");
    println!("What is built   {REPO}/blob/main/FEATURE_STATUS.rst");
    println!("Specification   {REPO}/blob/main/docs/feats/SPEC.md");
    println!();
    println!("In the terminal: `emrac help <command>`, `emrac man <command>`.");
    if open_it {
        open(SITE)?;
    }
    Ok(())
}

pub fn license(full: bool) {
    println!(
        "emrac is free software: you can redistribute it and/or modify it under the terms of the"
    );
    println!(
        "GNU General Public License as published by the Free Software Foundation, either version 3"
    );
    println!("of the License, or (at your option) any later version. It comes with NO WARRANTY.");
    println!();
    if full {
        println!("{LICENSE_TEXT}");
    } else {
        println!(
            "The whole text: `emrac license --full`, or https://www.gnu.org/licenses/gpl-3.0.html"
        );
    }
}

pub fn credits() {
    println!("emrac {}", env!("CARGO_PKG_VERSION"));
    println!("Made by pardhuvarmax  {REPO}");
    println!("Licence: GPL-3.0-or-later");
    println!();
    println!("Made for Arch Linux. It runs, and depends on:");
    for p in PROGRAMS {
        println!("  {p}");
    }
    println!();
    println!("Built with these Rust libraries:");
    for (name, licence) in LIBRARIES {
        println!("  {name:<14} {licence}");
    }
}

pub fn support(open_it: bool) -> Result<()> {
    println!("Something not right?");
    println!("  1. emrac doctor         checks this system for common problems");
    println!("  2. emrac help <cmd>     what a command does");
    println!("  3. {SITE}   the documentation");
    println!("  4. emrac bug-report     a report you can read, then paste into an issue");
    println!("  5. {REPO}/issues   ask, or report");
    if open_it {
        open(&format!("{REPO}/issues"))?;
    }
    Ok(())
}

pub fn feedback(open_it: bool) -> Result<()> {
    println!("An idea, or something that annoys you? Say so:");
    println!("  {REPO}/issues/new");
    println!(
        "Say what you were doing, what you expected, and what happened. Nothing is sent from here."
    );
    if open_it {
        open(&format!("{REPO}/issues/new"))?;
    }
    Ok(())
}

pub fn bug_report(sources: &Sources, open_it: bool) -> Result<()> {
    print!("{}", sources.bug_report()?);
    println!();
    println!("---");
    println!("This was only printed. Read it, then paste it into an issue: {REPO}/issues/new");
    if open_it {
        open(&format!("{REPO}/issues/new?title=Bug%3A%20"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_and_a_subcommand_are_found() {
        assert_eq!(
            command_for(&["install".into()]).unwrap().get_name(),
            "install"
        );
        assert_eq!(
            command_for(&["snapshot".into(), "create".into()])
                .unwrap()
                .get_name(),
            "create"
        );
        assert_eq!(command_for(&["emrac".into()]).unwrap().get_name(), "emrac");
    }

    #[test]
    fn an_unknown_command_lists_the_known_ones() {
        let e = command_for(&["nope".into()]).unwrap_err().to_string();
        assert!(
            e.contains("no command 'nope'") && e.contains("install"),
            "{e}"
        );
    }

    #[test]
    fn every_command_renders_as_a_man_page() {
        let mut root = Cli::command();
        root.build();
        for c in root.get_subcommands() {
            let mut page = Vec::new();
            clap_mangen::Man::new(c.clone()).render(&mut page).unwrap();
            assert!(
                String::from_utf8_lossy(&page).contains(".TH"),
                "{}",
                c.get_name()
            );
        }
    }

    #[test]
    fn the_libraries_named_are_the_ones_in_cargo_toml() {
        let manifests = [
            include_str!("../../Cargo.toml"),
            include_str!("../../../emrac-core/Cargo.toml"),
        ]
        .concat();
        for (name, _) in LIBRARIES {
            assert!(
                manifests
                    .lines()
                    .any(|l| l.starts_with(&format!("{name} ="))
                        || l.starts_with(&format!("{name}="))),
                "{name} is in neither Cargo.toml"
            );
        }
    }

    #[test]
    fn the_licence_is_the_gpl_3() {
        assert!(
            LICENSE_TEXT.contains("GNU GENERAL PUBLIC LICENSE")
                && LICENSE_TEXT.contains("Version 3")
        );
    }
}

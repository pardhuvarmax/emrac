//! `man`, `docs`, `license`, `credits`, `support`, `feedback`, `bug-report` (u13).

use std::io::{IsTerminal, Write};
use std::path::Path;
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

/// Roff text for one line: backslashes and hyphens escaped, and a leading
/// `.` or `'` kept from being read as a request.
fn esc(text: &str) -> String {
    let t = text.replace('\\', "\\\\").replace('-', "\\-");
    if t.starts_with(['.', '\'']) {
        format!("\\&{t}")
    } else {
        t
    }
}

fn section(out: &mut Vec<u8>, title: &str, body: &[&str]) {
    out.extend_from_slice(format!(".SH {title}\n").as_bytes());
    for line in body {
        if *line == ".PP" {
            out.extend_from_slice(b".PP");
        } else {
            out.extend_from_slice(esc(line).as_bytes());
        }
        out.push(b'\n');
    }
}

/// `.TP` entries: (term, description).
fn entries(out: &mut Vec<u8>, title: &str, items: &[(&str, &str)]) {
    out.extend_from_slice(format!(".SH {title}\n").as_bytes());
    for (term, text) in items {
        out.extend_from_slice(format!(".TP\n\\fB{}\\fR\n{}\n", esc(term), esc(text)).as_bytes());
    }
}

/// The manual page of one (sub)command. The top one also gets the long
/// description, the files emrac keeps, the exit codes and the related pages.
fn page(cmd: &clap::Command, top: bool, top_level: &[String]) -> Result<Vec<u8>> {
    let man = clap_mangen::Man::new(cmd.clone())
        .source(concat!("emrac ", env!("CARGO_PKG_VERSION")))
        .manual("emrac Manual");
    let io = |e: std::io::Error| Error::Help(format!("couldn't write the page: {e}"));
    let mut out = Vec::new();
    man.render_title(&mut out).map_err(io)?;
    man.render_name_section(&mut out).map_err(io)?;
    man.render_synopsis_section(&mut out).map_err(io)?;
    if top {
        section(
            &mut out,
            "DESCRIPTION",
            &[
                "emrac is a package manager front end for Arch Linux. It puts the official repositories and the AUR behind one command line, shows what a change will do before it makes it, and keeps a history that can be rolled back.",
                ".PP",
                "It reads package data through libalpm and changes the system only by running pacman (through sudo) or makepkg. AUR packages are audited before they are built and are built in a bubblewrap sandbox; the packages that were built are audited again before they are installed.",
                ".PP",
                "Every command has its own page, named emrac-COMMAND(1), for example emrac-install(1). A subcommand is joined with a hyphen: emrac-snapshot-create(1).",
            ],
        );
    } else {
        man.render_description_section(&mut out).map_err(io)?;
    }
    man.render_options_section(&mut out).map_err(io)?;
    if cmd.get_subcommands().any(|c| !c.is_hide_set()) {
        man.render_subcommands_section(&mut out).map_err(io)?;
    }
    if top {
        entries(
            &mut out,
            "FILES",
            &[
                (
                    "~/.config/emrac/config.toml",
                    "The settings (emrac config).",
                ),
                (
                    "~/.local/share/emrac/history.jsonl",
                    "The transaction history; history-archive/ holds archived parts.",
                ),
                (
                    "~/.local/share/emrac/snapshots/",
                    "Snapshots of the installed set (emrac snapshot).",
                ),
                ("~/.local/share/emrac/pins.json", "Holds (emrac pin)."),
                (
                    "~/.local/share/emrac/build-times.jsonl",
                    "How long builds took (emrac stats, emrac recommend).",
                ),
                (
                    "~/.local/share/emrac/benchmarks.jsonl",
                    "Results of emrac benchmark.",
                ),
                (
                    "~/.local/share/emrac/pacnew-log.jsonl",
                    "What was done to .pacnew files (emrac pacnew).",
                ),
                (
                    "~/.local/share/emrac/pacman-db/",
                    "Backups of pacman's own database, made before each transaction.",
                ),
                ("~/.cache/emrac/", "AUR checkouts and built packages."),
                (
                    "/etc/emrac/policy.toml",
                    "Overrides and diversions (emrac override, emrac divert).",
                ),
            ],
        );
        entries(
            &mut out,
            "EXIT STATUS",
            &[
                ("0", "Success, including nothing to do and a declined confirmation."),
                ("1", "emrac reported an error, including verify finding damaged files and doctor finding a problem."),
                ("2", "A command line usage error: an unknown command, a missing argument, a bad value."),
            ],
        );
        entries(
            &mut out,
            "ENVIRONMENT",
            &[(
                "HOME",
                "Where the settings, the history and the caches are kept.",
            )],
        );
    }
    let mut also: Vec<String> = Vec::new();
    if top {
        also.extend(top_level.iter().map(|n| format!("emrac-{n}(1)")));
        also.extend(
            [
                "pacman(8)",
                "makepkg(8)",
                "pacman.conf(5)",
                "PKGBUILD(5)",
                "bwrap(1)",
            ]
            .iter()
            .map(|s| (*s).to_string()),
        );
    } else {
        let name = cmd
            .get_display_name()
            .unwrap_or_else(|| cmd.get_name())
            .to_string();
        let mut parts: Vec<&str> = name.split('-').collect();
        parts.pop();
        let parent = if parts.is_empty() {
            "emrac".to_string()
        } else {
            parts.join("-")
        };
        also.push(format!("{parent}(1)"));
        if parent != "emrac" {
            also.push("emrac(1)".to_string());
        }
    }
    out.extend_from_slice(b".SH SEE ALSO\n");
    let mut line = String::new();
    for (i, a) in also.iter().enumerate() {
        if i > 0 {
            line.push_str(",\n");
        }
        let (n, sec) = a.split_once('(').unwrap_or((a, "1)"));
        line.push_str(&format!("\\fB{}\\fR({}", esc(n), sec));
    }
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
    if top {
        section(
            &mut out,
            "BUGS",
            &[
                "Report them at https://github.com/pardhuvarmax/emrac/issues. emrac bug-report prints a report to paste into an issue; it is never sent anywhere.",
            ],
        );
        section(
            &mut out,
            "VERSION",
            &[concat!("emrac ", env!("CARGO_PKG_VERSION"))],
        );
        section(
            &mut out,
            "AUTHOR",
            &["pardhuvarmax. Sic Mundus Creatus Est."],
        );
        section(
            &mut out,
            "COPYRIGHT",
            &["emrac is free software, licensed GPL-3.0-or-later. See emrac license."],
        );
    }
    // Typographic characters the viewers may not all handle.
    let text = String::from_utf8_lossy(&out)
        .replace('\u{2014}', "\\(em")
        .replace('\u{2013}', "\\(en")
        .replace('\u{2026}', "...");
    Ok(text.into_bytes())
}

/// Names of the top-level commands that have a page.
fn top_level_names(root: &clap::Command) -> Vec<String> {
    root.get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .map(|c| c.get_name().to_string())
        .collect()
}

/// `emrac.1` and one page per (sub)command in `dir`; returns the file names.
pub fn generate(dir: &Path) -> Result<Vec<String>> {
    let mut root = Cli::command().disable_help_subcommand(true);
    root.build();
    let top = top_level_names(&root);
    std::fs::create_dir_all(dir).map_err(|e| Error::Help(format!("{}: {e}", dir.display())))?;
    let mut written = Vec::new();
    fn walk(
        cmd: &clap::Command,
        is_top: bool,
        top: &[String],
        dir: &Path,
        written: &mut Vec<String>,
    ) -> Result<()> {
        let name = cmd.get_display_name().unwrap_or_else(|| cmd.get_name());
        let file = format!("{name}.1");
        let bytes = page(cmd, is_top, top)?;
        std::fs::write(dir.join(&file), bytes)
            .map_err(|e| Error::Help(format!("{}: {e}", dir.join(&file).display())))?;
        written.push(file);
        for sub in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
            walk(sub, false, top, dir, written)?;
        }
        Ok(())
    }
    walk(&root, true, &top, dir, &mut written)?;
    Ok(written)
}

pub fn man(path: &[String], roff: bool, generate_to: Option<&str>) -> Result<()> {
    if let Some(dir) = generate_to {
        let written = generate(Path::new(dir))?;
        eprintln!("wrote {} manual pages to {dir}", written.len());
        return Ok(());
    }
    let mut root = Cli::command();
    root.build();
    let top = top_level_names(&root);
    let words: Vec<String> = if path.first().is_some_and(|w| w == "emrac") {
        path[1..].to_vec()
    } else {
        path.to_vec()
    };
    let cmd = find(&root, &words)?.clone();
    if roff || std::io::stdout().is_terminal() {
        let bytes = page(&cmd, words.is_empty(), &top)?;
        if roff {
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| Error::Help(e.to_string()))?;
            return Ok(());
        }
        if let Ok(mut child) = Command::new("man")
            .args(["-l", "-"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&bytes);
            }
            let _ = child.wait();
            return Ok(());
        }
    }
    let mut cmd = cmd;
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

    fn built() -> clap::Command {
        let mut root = Cli::command().disable_help_subcommand(true);
        root.build();
        root
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn a_command_and_a_subcommand_are_found() {
        let root = built();
        assert_eq!(
            find(&root, &["install".into()]).unwrap().get_name(),
            "install"
        );
        assert_eq!(
            find(&root, &["snapshot".into(), "create".into()])
                .unwrap()
                .get_name(),
            "create"
        );
        assert_eq!(find(&root, &[]).unwrap().get_name(), "emrac");
    }

    #[test]
    fn an_unknown_command_lists_the_known_ones() {
        let root = built();
        let e = find(&root, &["nope".into()]).unwrap_err().to_string();
        assert!(
            e.contains("no command 'nope'") && e.contains("install"),
            "{e}"
        );
    }

    #[test]
    fn the_main_page_has_the_sections_a_man_page_has() {
        let root = built();
        let top = top_level_names(&root);
        let page = text(&page(&root, true, &top).unwrap());
        assert!(
            page.starts_with(".ie") || page.contains(".TH emrac 1"),
            "{page}"
        );
        for section in [
            ".SH NAME",
            ".SH SYNOPSIS",
            ".SH DESCRIPTION",
            ".SH OPTIONS",
            ".SH SUBCOMMANDS",
            ".SH FILES",
            ".SH EXIT STATUS",
            ".SH ENVIRONMENT",
            ".SH SEE ALSO",
            ".SH BUGS",
            ".SH VERSION",
            ".SH AUTHOR",
            ".SH COPYRIGHT",
        ] {
            assert!(page.contains(section), "{section} is missing");
        }
        assert!(page.contains("config.toml") && page.contains("emrac\\-install\\fR(1)"));
        assert!(page.contains("pacman\\fR(8)"));
        // hyphens are escaped, so they can be searched for and copied
        assert!(!page.contains("emrac-install(1)"));
    }

    #[test]
    fn a_command_page_points_back_to_its_parent() {
        let root = built();
        let top = top_level_names(&root);
        let snapshot = find(&root, &["snapshot".into()]).unwrap();
        let create = find(&root, &["snapshot".into(), "create".into()]).unwrap();
        let install = find(&root, &["install".into()]).unwrap();
        let p = text(&page(install, false, &top).unwrap());
        assert!(
            p.contains(".SH SEE ALSO") && p.contains("emrac\\fR(1)"),
            "{p}"
        );
        assert!(
            !p.contains(".SH FILES"),
            "only the main page lists the files"
        );
        let c = text(&page(create, false, &top).unwrap());
        assert!(
            c.contains("emrac\\-snapshot\\fR(1)") && c.contains("emrac\\fR(1)"),
            "{c}"
        );
        assert!(text(&page(snapshot, false, &top).unwrap()).contains(".SH SUBCOMMANDS"));
    }

    #[test]
    fn a_page_is_written_for_every_command_and_subcommand() {
        let dir = std::env::temp_dir().join(format!("emrac-man-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let written = generate(&dir).unwrap();
        assert!(written.contains(&"emrac.1".to_string()));
        assert!(written.contains(&"emrac-install.1".to_string()));
        assert!(written.contains(&"emrac-snapshot-create.1".to_string()));
        assert!(written.contains(&"emrac-bundle-apply.1".to_string()));
        assert!(
            !written.iter().any(|f| f.contains("help")),
            "no page for the help subcommand"
        );
        for f in &written {
            let body = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(body.contains(".TH ") && body.contains(".SH NAME"), "{f}");
        }
        // hidden commands (the root helper) have no page
        assert!(!written
            .iter()
            .any(|f| f.contains("policy-helper") || f.contains("ledger-append")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_in_roff_is_escaped() {
        assert_eq!(esc("a-b"), "a\\-b");
        assert_eq!(esc(".TP"), r"\&.TP");
        assert_eq!(esc("back\\slash"), "back\\\\slash");
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

use emrac_core::{
    unified_diff, PacnewAction, PacnewClass, PacnewFile, PacnewMerge, Result, Sources,
};

use crate::cli::PacnewCommand;
use crate::prompt::{choose, confirm_typed};

pub fn run(
    sources: &Sources,
    fetch: bool,
    command: Option<&PacnewCommand>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let files = sources.pacnew_scan(fetch)?;
    match command {
        None => list(&files, json),
        Some(PacnewCommand::Diff { file }) => diff(&files, file, json),
        Some(PacnewCommand::Resolve { files: wanted }) => {
            resolve(sources, &files, wanted, dry_run, json)
        }
    }
}

fn list(files: &[PacnewFile], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(files).unwrap());
        return Ok(());
    }
    if files.is_empty() {
        println!("emrac notes: no .pacnew or .pacsave files are waiting.");
        return Ok(());
    }
    println!(
        "emrac found: {} waiting\n",
        plural(files.len(), "config file")
    );
    for f in files {
        let age = f
            .age_days
            .map(|d| format!(", {d} days old"))
            .unwrap_or_default();
        let owner = f.package.as_deref().unwrap_or("no package owns it");
        let lock = if f.critical {
            "  [can lock you out]"
        } else {
            ""
        };
        println!("  {}  — {} ({owner}{age}){lock}", f.live, f.class.label());
        println!("      {}", f.reason);
        if let Some(merge) = &f.merge {
            println!("      a three-way merge comes out {merge}");
        }
    }
    println!(
        "\n`emrac pacnew diff <file>` shows the changes; `emrac pacnew resolve` goes through them."
    );
    Ok(())
}

fn find<'a>(files: &'a [PacnewFile], name: &str) -> Option<&'a PacnewFile> {
    files.iter().find(|f| f.live == name || f.pending == name)
}

fn diff(files: &[PacnewFile], name: &str, json: bool) -> Result<()> {
    let Some(f) = find(files, name) else {
        println!("emrac notes: '{name}' has no .pacnew or .pacsave waiting.");
        return Ok(());
    };
    if json {
        println!("{}", serde_json::to_string_pretty(f).unwrap());
        return Ok(());
    }
    println!("{} — {}\n  {}\n", f.live, f.class.label(), f.reason);
    let (Some(ours), Some(theirs)) = (&f.texts.ours, &f.texts.theirs) else {
        println!("(the file can't be shown here)");
        return Ok(());
    };
    match &f.texts.base {
        Some(base) => {
            let version = f.base_version.as_deref().unwrap_or("?");
            section(
                "What you changed (the old package's file → your live file)",
                &unified_diff(
                    &format!("shipped in {version}"),
                    base,
                    "your live file",
                    ours,
                )?,
            );
            section(
                "What upstream changed (the old package's file → the new one)",
                &unified_diff(
                    &format!("shipped in {version}"),
                    base,
                    "the new file",
                    theirs,
                )?,
            );
        }
        None => section(
            "Your live file → the new file (your edits and upstream's are mixed)",
            &unified_diff("your live file", ours, "the new file", theirs)?,
        ),
    }
    match &f.merged {
        Some(PacnewMerge::Clean(merged)) => section(
            "A merge of both has no conflicts; it would change your live file like this",
            &unified_diff("your live file", ours, "merged", merged)?,
        ),
        Some(PacnewMerge::Conflicts(n)) => println!(
            "A merge would leave {} to settle by hand, so emrac won't offer one.",
            plural(*n as usize, "conflict")
        ),
        None => {}
    }
    Ok(())
}

fn section(title: &str, diff: &str) {
    println!("{title}:");
    if diff.is_empty() {
        println!("  (no changes)\n");
    } else {
        println!("{diff}");
    }
}

fn resolve(
    sources: &Sources,
    all: &[PacnewFile],
    wanted: &[String],
    dry_run: bool,
    json: bool,
) -> Result<()> {
    // Nothing is asked under --json: the findings are the output.
    if json {
        return list(all, true);
    }
    let chosen: Vec<&PacnewFile> = if wanted.is_empty() {
        all.iter().collect()
    } else {
        wanted.iter().filter_map(|w| find(all, w)).collect()
    };
    if chosen.is_empty() {
        println!("emrac notes: nothing to resolve.");
        return Ok(());
    }
    for f in chosen {
        println!("\n{} — {}\n  {}", f.live, f.class.label(), f.reason);
        if f.critical {
            println!("  This file can lock you out if it is wrong; taking or merging it needs you to type its name.");
        }
        let mut options = String::from("ks");
        let mut menu = vec!["[k]eep yours and drop the new one"];
        if !f.is_pacsave() {
            options.push('t');
            menu.push("[t]ake the new file");
        }
        if matches!(f.merged, Some(PacnewMerge::Clean(_))) {
            options.push('m');
            menu.push("[m]erge both (no conflicts)");
        }
        menu.push("[s]kip");
        let suggestion = match f.class {
            PacnewClass::Untouched => "take",
            PacnewClass::MineOnly | PacnewClass::Identical | PacnewClass::Pacsave => "keep",
            PacnewClass::BothChanged if options.contains('m') => "merge",
            _ => "keep, or look at `emrac pacnew diff` first",
        };
        println!("  Suggested: {suggestion}. Enter alone skips.");
        let Some(pick) = choose(&format!("{}?", menu.join(" / ")), &options) else {
            println!("emrac notes: skipped {}.", f.live);
            continue;
        };
        let action = match pick {
            'k' => PacnewAction::Keep,
            't' => PacnewAction::Take,
            'm' => PacnewAction::Merge,
            _ => {
                println!("emrac notes: skipped {}.", f.live);
                continue;
            }
        };
        if f.critical
            && action != PacnewAction::Keep
            && !confirm_typed(&format!("Type {} to go on:", f.live), &f.live)
        {
            println!("emrac notes: skipped {}.", f.live);
            continue;
        }
        if dry_run {
            println!(
                "emrac plans: {action:?} {} (dry run, nothing changed)",
                f.live
            );
            continue;
        }
        match sources.pacnew_apply(f, action)? {
            Some(backup) => println!(
                "emrac notes: done. Your previous file is saved at {}.",
                backup.display()
            ),
            None => println!("emrac notes: done."),
        }
        if action != PacnewAction::Keep {
            if let Some(next) = follow_up(&f.live) {
                println!("emrac notes: {next}");
            }
        }
    }
    Ok(())
}

/// What to do after a config file changed, for the few that need it.
fn follow_up(live: &str) -> Option<&'static str> {
    match live {
        "/etc/mkinitcpio.conf" => Some("rebuild the initramfs to apply it: `sudo mkinitcpio -P`."),
        "/etc/locale.gen" => Some("generate the locales: `sudo locale-gen`."),
        "/etc/ssh/sshd_config" => Some("the SSH server reads it again when restarted."),
        _ => None,
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

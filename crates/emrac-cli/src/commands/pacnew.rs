use emrac_core::{
    has_conflict_markers, merge3_markers, unified_diff, Error, PacnewAction, PacnewClass,
    PacnewFile, PacnewMerge, Result, Sources,
};

use crate::cli::PacnewCommand;
use crate::prompt::{choose, confirm_default_no, confirm_typed};

pub fn run(
    sources: &Sources,
    fetch: bool,
    command: Option<&PacnewCommand>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    match command {
        Some(PacnewCommand::Revert { file }) => return revert(sources, file.as_deref(), json),
        Some(PacnewCommand::Status) => return status(sources, json),
        _ => {}
    }
    let files = sources.pacnew_scan(fetch)?;
    match command {
        None => list(&files, json),
        Some(PacnewCommand::Diff { file }) => diff(&files, file, json),
        Some(PacnewCommand::Resolve { files: wanted }) => {
            resolve(sources, &files, wanted, dry_run, json)
        }
        Some(PacnewCommand::Revert { .. } | PacnewCommand::Status) => unreachable!("handled above"),
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
        if f.texts.base.is_some() && !f.is_pacsave() {
            options.push('e');
            menu.push("[e]dit the merge by hand");
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
        let mut action = match pick {
            'k' => PacnewAction::Keep,
            't' => PacnewAction::Take,
            'm' => PacnewAction::Merge,
            'e' => PacnewAction::Edit(String::new()),
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
        if let PacnewAction::Edit(_) = action {
            match edit_by_hand(f) {
                Ok(Some(text)) => action = PacnewAction::Edit(text),
                Ok(None) => {
                    println!("emrac notes: skipped {}.", f.live);
                    continue;
                }
                Err(err) => {
                    eprintln!("emrac {}: {err}", err.voice());
                    continue;
                }
            }
        }
        if dry_run {
            println!(
                "emrac plans: {} {} (dry run, nothing changed)",
                match &action {
                    PacnewAction::Keep => "keep",
                    PacnewAction::Take => "take",
                    PacnewAction::Merge => "merge",
                    PacnewAction::Edit(_) => "edit",
                },
                f.live
            );
            continue;
        }
        let applied = match sources.pacnew_apply(f, action.clone()) {
            Ok(applied) => applied,
            // A refused candidate leaves this file alone; the rest go on.
            Err(err @ (Error::PacnewInvalid { .. } | Error::PacnewMarkers(_))) => {
                eprintln!("emrac {}: {err}", err.voice());
                continue;
            }
            Err(err) => return Err(err),
        };
        if let Some(what) = &applied.checked {
            println!("emrac notes: checked first — {what}.");
        }
        match &applied.backup_dir {
            Some(dir) => println!(
                "emrac notes: done ({}). Your previous file is saved under {}; `emrac pacnew revert {}` puts it back.",
                applied.id,
                dir.display(),
                f.live
            ),
            None => println!("emrac notes: done ({}).", applied.id),
        }
        if action != PacnewAction::Keep {
            if let Some(next) = follow_up(&f.live) {
                println!("emrac notes: {next}");
            }
        }
    }
    Ok(())
}

/// Opens the three-way result, with conflict markers where the two sides
/// disagree, in `$VISUAL` / `$EDITOR`. `None` if the user ends up not wanting it.
fn edit_by_hand(f: &PacnewFile) -> Result<Option<String>> {
    use std::os::unix::fs::DirBuilderExt;
    let (Some(base), Some(ours), Some(theirs)) = (&f.texts.base, &f.texts.ours, &f.texts.theirs)
    else {
        return Ok(None);
    };
    let (seed, conflicts) = merge3_markers(base, ours, theirs)?;
    let dir = std::env::temp_dir().join(format!(
        "emrac-edit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let io = |what: &str, e: std::io::Error| Error::Io {
        context: what.to_string(),
        source: e,
    };
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| io("couldn't create a folder to edit in", e))?;
    let name = std::path::Path::new(&f.live)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    let path = dir.join(name);
    let result = (|| -> Result<Option<String>> {
        std::fs::write(&path, &seed).map_err(|e| io("couldn't write the file to edit", e))?;
        println!(
            "  Opening it in your editor: {}; remove every <<<<<<<, ||||||| and >>>>>>> line, then save and quit.",
            if conflicts == 0 {
                "no conflicts, adjust as you like".to_string()
            } else {
                format!("{} to settle", plural(conflicts as usize, "conflict"))
            }
        );
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());
        let mut words = editor.split_whitespace();
        let Some(program) = words.next() else {
            return Ok(None);
        };
        let status = std::process::Command::new(program)
            .args(words)
            .arg(&path)
            .status()
            .map_err(|e| Error::CommandSpawn(program.to_string(), e))?;
        if !status.success() {
            println!("emrac notes: the editor exited with {status}; nothing changed.");
            return Ok(None);
        }
        let edited =
            std::fs::read_to_string(&path).map_err(|e| io("couldn't read the edited file", e))?;
        if has_conflict_markers(&edited) {
            return Err(Error::PacnewMarkers(f.live.clone()));
        }
        let diff = unified_diff("your live file", ours, "your edited result", &edited)?;
        if diff.is_empty() {
            println!("emrac notes: the result is the same as your live file.");
            return Ok(None);
        }
        println!("\nThis would change your live file like this:\n{diff}");
        Ok(confirm_default_no("Put this in place?").then_some(edited))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn revert(sources: &Sources, file: Option<&str>, json: bool) -> Result<()> {
    if json {
        return status(sources, true);
    }
    let log = sources.pacnew_log()?;
    let target = log
        .iter()
        .rev()
        .find(|(r, standing)| {
            r.action != "revert"
                && r.state == emrac_core::PacnewState::Done
                && *standing != emrac_core::PacnewStanding::Reverted
                && file.is_none_or(|f| r.file == f || r.pending == f)
        })
        .map(|(r, _)| r.clone());
    let Some(target) = target else {
        return Err(Error::PacnewNothingToRevert(
            file.map(|f| format!(" for {f}")).unwrap_or_default(),
        ));
    };
    println!(
        "emrac plans: revert {} — {} of {} ({})",
        target.id,
        target.action,
        target.file,
        target.time_utc()
    );
    if !confirm_default_no("Put the file back as it was before that change?") {
        println!("emrac notes: okay, nothing changed.");
        return Ok(());
    }
    let done = match sources.pacnew_revert(Some(&target.file), false) {
        Err(Error::PacnewDrift { file }) => {
            println!(
                "emrac found: {file} was changed after emrac's change; reverting overwrites those edits."
            );
            if !confirm_typed(&format!("Type {file} to revert anyway:"), &file) {
                println!("emrac notes: okay, nothing changed.");
                return Ok(());
            }
            sources.pacnew_revert(Some(&file), true)?
        }
        other => other?,
    };
    println!(
        "emrac notes: reverted {} ({}){}.",
        done.id,
        done.file,
        if done.restored_pending {
            "; the new file is pending again"
        } else {
            ""
        }
    );
    Ok(())
}

fn status(sources: &Sources, json: bool) -> Result<()> {
    let log = sources.pacnew_log()?;
    if json {
        let entries: Vec<_> = log
            .iter()
            .map(|(r, s)| serde_json::json!({ "record": r, "standing": s }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
        return Ok(());
    }
    let pending = sources.pacnew_scan(false)?;
    println!(
        "emrac found: {} waiting (`emrac pacnew` lists them).",
        plural(pending.len(), "config file")
    );
    if log.is_empty() {
        println!("emrac notes: emrac has not changed any config file yet.");
        return Ok(());
    }
    println!("\nChanges (times are UTC):");
    for (r, standing) in &log {
        println!(
            "  {:<7} {}  {:<7} {}  — {}",
            r.id,
            r.time_utc(),
            r.action,
            r.file,
            match (&r.reverts, r.action.as_str()) {
                (Some(undone), "revert") => format!("undoes {undone}"),
                _ => standing.label().to_string(),
            }
        );
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

//! `emrac override`, `emrac divert`, `emrac policy` and the hidden helper they
//! (and the pacman hook) run as root. See `docs/plan/r3/u8-override-divert.md`.

use std::process::Command;

use emrac_core::ledger::running_as_root;
use emrac_core::policy::{self, EventKind, Override, POLICY_FILE};
use emrac_core::policy_ops::{self, LineState, Outcome, Paths, Status};
use emrac_core::{say, Error, Result, Voice};

use crate::cli::{DivertCommand, OverrideCommand, PolicyCommand, PolicyHelperVerb};
use crate::prompt::confirm_default_no;

// ------------------------------------------------------- the user's side

/// Runs the helper as root (through `sudo` unless already root), with its
/// output going straight to the terminal. A refusal ends the process with the
/// helper's own exit status: the helper has already said why.
fn as_root(args: &[String]) -> Result<()> {
    let exe =
        std::env::current_exe().map_err(|e| Error::Policy(format!("couldn't find emrac: {e}")))?;
    let mut command = if running_as_root() {
        Command::new(&exe)
    } else {
        let mut c = Command::new("sudo");
        c.arg(&exe);
        c
    };
    let status = command
        .arg("policy-helper")
        .args(args)
        .status()
        .map_err(|e| Error::CommandSpawn("sudo".into(), e))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn strs(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

pub fn override_cmd(command: &OverrideCommand, json: bool) -> Result<()> {
    match command {
        OverrideCommand::Set {
            path,
            mode,
            owner,
            group,
            note,
            allow_setuid,
        } => {
            let o = Override {
                path: path.clone(),
                mode: mode.clone(),
                owner: owner.clone(),
                group: group.clone(),
                note: note.clone(),
            };
            o.validate().map_err(Error::Policy)?;
            if let Some(user) = owner {
                if !policy_ops::user_exists(user) {
                    return Err(Error::Policy(format!("there is no user '{user}'")));
                }
            }
            if let Some(g) = group {
                if !policy_ops::group_exists(g) {
                    return Err(Error::Policy(format!("there is no group '{g}'")));
                }
            }
            let privileged = mode
                .as_deref()
                .and_then(|m| policy::parse_mode(m).ok())
                .is_some_and(policy::is_privileged_mode);
            let mut consented = *allow_setuid;
            if privileged && !consented {
                say!(
                    Voice::Warns,
                    "mode {} makes {path} run with the rights of its owner or group, for whoever runs it.",
                    mode.as_deref().unwrap_or("")
                );
                consented = confirm_default_no("Set it anyway?");
                if !consented {
                    say!(Voice::Notes, "okay, nothing changed.");
                    return Ok(());
                }
            }
            let mut args = strs(&["set-override", path]);
            for (flag, value) in [
                ("--mode", mode),
                ("--owner", owner),
                ("--group", group),
                ("--note", note),
            ] {
                if let Some(v) = value {
                    args.push(flag.into());
                    args.push(v.clone());
                }
            }
            if consented {
                args.push("--allow-setuid".into());
            }
            as_root(&args)
        }
        OverrideCommand::Remove { path } => as_root(&strs(&["remove-override", path])),
        OverrideCommand::List => list("override", json),
    }
}

pub fn divert_cmd(command: &DivertCommand, json: bool) -> Result<()> {
    match command {
        DivertCommand::Add {
            path,
            note,
            allow_stale,
        } => {
            // What diverting costs, said before anything is changed, and a
            // default-no question that `--yes` does not answer.
            let mut consented = *allow_stale;
            if !consented {
                let head = std::fs::read(path).map(|b| b.into_iter().take(4).collect::<Vec<u8>>());
                let mode = emrac_core::policy_ops::mode_and_owner(std::path::Path::new(path))
                    .map_or(0, |m| m.0);
                say!(
                    Voice::Warns,
                    "diverting {path} freezes it: when its package ships a new version of this file, yours stays, so the package's changes to it, security fixes included, will NOT take effect, and pacman will report a normal, successful upgrade."
                );
                for risk in policy::divert_risks(mode, head.as_deref().unwrap_or(&[])) {
                    say!(Voice::Warns, "take extra care: {risk}.");
                }
                say!(
                    Voice::Notes,
                    "emrac will warn after every transaction that changes it; `emrac divert diff {path}` shows what you are missing."
                );
                consented = confirm_default_no("Divert it anyway?");
                if !consented {
                    say!(Voice::Notes, "okay, nothing changed.");
                    return Ok(());
                }
            }
            let mut args = strs(&["add-divert", path, "--allow-stale"]);
            if let Some(n) = note {
                args.push("--note".into());
                args.push(n.clone());
            }
            as_root(&args)
        }
        DivertCommand::Reviewed { path } => as_root(&strs(&["reviewed", path])),
        DivertCommand::Remove { path } => as_root(&strs(&["remove-divert", path])),
        DivertCommand::Diff { path } => as_root(&strs(&["diff", path])),
        DivertCommand::List => list("divert", json),
    }
}

pub fn policy_cmd(command: &PolicyCommand, json: bool) -> Result<()> {
    match command {
        PolicyCommand::Apply => as_root(&strs(&["apply"])),
        PolicyCommand::Status => {
            let status = policy_ops::status(&Paths::system()?)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status).unwrap());
            } else {
                print_status(&status, None);
            }
            Ok(())
        }
    }
}

fn state_word(state: LineState) -> &'static str {
    match state {
        LineState::InEffect => "in effect",
        LineState::Stale => "STALE",
        LineState::Drift => "NOT IN EFFECT",
        LineState::Absent => "not installed",
        LineState::Unknown => "unknown",
    }
}

fn print_status(status: &Status, only: Option<&str>) {
    let policy = policy::load(std::path::Path::new(POLICY_FILE)).unwrap_or_default();
    let mut shown = 0;
    for line in status
        .lines
        .iter()
        .filter(|l| only.is_none_or(|k| l.kind == k))
    {
        shown += 1;
        let detail = match line.kind {
            "override" => policy
                .override_of(&line.path)
                .map(|o| {
                    let mut p = Vec::new();
                    if let Some(m) = &o.mode {
                        p.push(format!("mode {m}"));
                    }
                    if let Some(u) = &o.owner {
                        p.push(format!("owner {u}"));
                    }
                    if let Some(g) = &o.group {
                        p.push(format!("group {g}"));
                    }
                    if let Some(n) = &o.note {
                        p.push(format!("({n})"));
                    }
                    p.join(" ")
                })
                .unwrap_or_default(),
            _ => policy
                .diverts
                .iter()
                .find(|d| d.path == line.path)
                .and_then(|d| d.note.clone())
                .map(|n| format!("({n})"))
                .unwrap_or_default(),
        };
        println!("  {:<10} {}  {detail}", state_word(line.state), line.path);
        if line.state != LineState::InEffect {
            println!("             {}", line.text);
        }
    }
    if shown == 0 {
        match only {
            Some(kind) => say!(Voice::Notes, "no {kind}s are set."),
            None => say!(Voice::Notes, "no local file policy is set (`emrac override set` and `emrac divert add` start one)."),
        }
    }
    if let Some(note) = &status.hook_note {
        say!(Voice::Warns, "{note} (`emrac policy apply` sets it right)");
    }
}

fn list(kind: &str, json: bool) -> Result<()> {
    let status = policy_ops::status(&Paths::system()?)?;
    if json {
        let lines: Vec<_> = status.lines.iter().filter(|l| l.kind == kind).collect();
        println!("{}", serde_json::to_string_pretty(&lines).unwrap());
    } else {
        print_status(&status, Some(kind));
    }
    Ok(())
}

// ------------------------------------------------------ the root side

fn report(out: &Outcome) {
    for note in &out.notes {
        say!(Voice::Notes, "{note}");
    }
    for event in &out.events {
        match event.kind {
            EventKind::Already => {}
            EventKind::Applied => say!(Voice::Remembers, "{}: {}", event.path, event.text),
            EventKind::Stale => say!(Voice::Warns, "{}: {}", event.path, event.text),
            EventKind::Skipped => say!(Voice::Notes, "{}: {}", event.path, event.text),
            EventKind::Failed => say!(Voice::Warns, "{}: {}", event.path, event.text),
        }
    }
}

/// `emrac policy-helper …`: the only code that changes the policy, the store
/// or the hook. It trusts nothing it is given: every argument is checked again.
pub fn helper(verb: &PolicyHelperVerb) -> Result<()> {
    if !running_as_root() {
        return Err(Error::Policy(
            "policy-helper changes system files and must run as root (emrac runs it through sudo itself)".into(),
        ));
    }
    let paths = Paths::system()?;
    let out = match verb {
        PolicyHelperVerb::Apply => policy_ops::apply_policy(&paths)?,
        // Before a transaction: never stops it, whatever happens.
        PolicyHelperVerb::Capture => {
            match policy_ops::capture_before(&paths) {
                Ok(out) => report(&out),
                Err(err) => say!(
                    Voice::Warns,
                    "couldn't save your edits of diverted files: {err}"
                ),
            }
            return Ok(());
        }
        PolicyHelperVerb::SetOverride {
            path,
            mode,
            owner,
            group,
            note,
            allow_setuid,
        } => policy_ops::set_override(
            &paths,
            Override {
                path: path.clone(),
                mode: mode.clone(),
                owner: owner.clone(),
                group: group.clone(),
                note: note.clone(),
            },
            *allow_setuid,
        )?,
        PolicyHelperVerb::RemoveOverride { path } => policy_ops::remove_override(&paths, path)?,
        PolicyHelperVerb::AddDivert {
            path,
            note,
            allow_stale,
        } => policy_ops::add_divert(&paths, path, note.clone(), *allow_stale)?,
        PolicyHelperVerb::Reviewed { path } => policy_ops::reviewed(&paths, path)?,
        PolicyHelperVerb::RemoveDivert { path } => policy_ops::remove_divert(&paths, path)?,
        PolicyHelperVerb::Diff { path } => {
            let diff = policy_ops::diff_diverted(&paths, path)?;
            if diff.is_empty() {
                say!(Voice::Notes, "the package's version is the same as yours.");
            } else {
                print!("{diff}");
            }
            return Ok(());
        }
    };
    report(&out);
    if out.failed() {
        std::process::exit(1);
    }
    Ok(())
}

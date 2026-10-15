//! The user's side of the ledger (see `docs/plan/u8-ledger.md`): sending
//! history lines to the root-owned file through `sudo emrac ledger-append`,
//! and that hidden helper itself.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use emrac_core::ledger_watch::{self, Verdict};
use emrac_core::{ledger, say, Error, History, Result, Voice};

/// The most the helper reads: far more than any real history.
const MAX_INPUT: u64 = 8 * 1024 * 1024;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `emrac ledger-append`: the one thing that writes the ledger. It reads
/// history lines from standard input, checks them, and appends the new ones.
pub fn append_from_stdin() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_string(&mut input)
        .map_err(|e| Error::History(format!("ledger refused: couldn't read its input: {e}")))?;
    if input.len() as u64 > MAX_INPUT {
        return Err(Error::History("ledger refused: too much input".into()));
    }
    let lines: Vec<String> = input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let path = ledger::path(ledger::running_as_root());
    let done = ledger::append_detailed(&path, &lines, now(), true)?;
    if done.added > 0 {
        witness(&done);
    }
    println!("{}", done.added);
    Ok(())
}

/// The journal line that says emrac did this append (u9): the audit events
/// about the ledger are matched against it. Best effort.
fn witness(done: &ledger::Appended) {
    let message = ledger_watch::witness_message(
        done.added,
        &done.first,
        &done.last,
        &done.head,
        done.created,
    );
    let by = std::env::var("SUDO_USER").unwrap_or_default();
    let _ = Command::new("/usr/bin/logger")
        .args(["-t", ledger_watch::WITNESS_TAG, "-p", "authpriv.notice"])
        .arg(format!("{message} by={by}"))
        .stdin(Stdio::null())
        .output();
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// After a transaction: never asks for a password.
    Quiet,
    /// `history sync`: sudo may ask.
    Interactive,
}

/// An `EMRAC_LEDGER` that points somewhere else is written directly (it is
/// the user's own file, for tests and for a copy); the real ledger is
/// only ever written by root.
fn override_path() -> Option<std::path::PathBuf> {
    if ledger::running_as_root() {
        return None;
    }
    std::env::var_os("EMRAC_LEDGER")
        .filter(|p| !p.is_empty())
        .map(Into::into)
}

/// The binary run as root has to be one the user cannot have replaced.
fn trusted_exe() -> std::result::Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find itself: {e}"))?;
    let meta = fs::metadata(&exe).map_err(|e| format!("can't look at {}: {e}", exe.display()))?;
    if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{} is not a root-owned install, so emrac will not run it as root",
            exe.display()
        ));
    }
    Ok(exe)
}

fn send(lines: &[String], mode: Mode) -> std::result::Result<usize, String> {
    if let Some(path) = override_path() {
        return ledger::append(&path, lines, now(), false).map_err(|e| e.to_string());
    }
    let exe = trusted_exe()?;
    let mut cmd = Command::new("sudo");
    if mode == Mode::Quiet {
        cmd.arg("-n");
    }
    cmd.arg("--").arg(&exe).arg("ledger-append");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    cmd.stderr(if mode == Mode::Quiet {
        Stdio::null()
    } else {
        Stdio::inherit()
    });
    let mut child = cmd.spawn().map_err(|e| format!("couldn't run sudo: {e}"))?;
    let mut input = lines.join("\n");
    input.push('\n');
    if let Some(mut stdin) = child.stdin.take() {
        // A helper that exits early (sudo said no) closes the pipe: that is
        // reported through its status below.
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("couldn't wait for sudo: {e}"))?;
    if !out.status.success() {
        return Err(if mode == Mode::Quiet {
            "sudo would have asked for a password, or does not allow it".into()
        } else {
            "sudo or the ledger helper said no".into()
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0))
}

fn sync(mode: Mode) -> std::result::Result<usize, String> {
    let history = History::path().map_err(|e| e.to_string())?;
    let path = override_path().unwrap_or_else(|| ledger::path(ledger::running_as_root()));
    let current = ledger::read(&path).map_err(|e| e.to_string())?;
    if current.as_ref().is_some_and(|l| l.problem.is_some()) {
        return Err("the ledger's own chain is broken (see `emrac history verify`)".into());
    }
    let pending = ledger::pending_lines(current.as_ref(), &history).map_err(|e| e.to_string())?;
    if pending.is_empty() {
        return Ok(0);
    }
    send(&pending, mode)
}

/// After the history changed: bring the ledger up to date if sudo allows it
/// without a question. Never fails the command, says so once per run.
pub fn sync_after() {
    static SAID: AtomicBool = AtomicBool::new(false);
    if let Err(why) = sync(Mode::Quiet) {
        if !SAID.swap(true, Ordering::Relaxed) {
            say!(
                Voice::Warns,
                "the ledger wasn't updated ({why}). Your history is safe; `emrac history sync` catches the ledger up."
            );
        }
    }
}

/// `emrac history sync`.
pub fn sync_interactive(json: bool) -> Result<()> {
    let added = sync(Mode::Interactive)
        .map_err(|why| Error::History(format!("the ledger wasn't updated: {why}")))?;
    if json {
        println!("{}", serde_json::json!({ "sent": added }));
    } else if added == 0 {
        say!(
            Voice::Notes,
            "the ledger already holds everything in your history."
        );
    } else {
        say!(
            Voice::Confirms,
            "sent {added} line{} to the ledger.",
            if added == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// The paths this emrac program may be audited as.
pub(crate) fn trusted_exes() -> Vec<String> {
    let mut v = vec!["/usr/bin/emrac".to_string()];
    if let Ok(p) = std::env::current_exe() {
        v.push(p.display().to_string());
    }
    v
}

/// `emrac ledger events`.
pub fn events(ack: Option<&[String]>, json: bool) -> Result<()> {
    let read = ledger_watch::read(&trusted_exes())?;
    if let Some(ids) = ack {
        return ack_events(&read, ids, json);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "events": read.events,
                "emrac_appends_logged": read.witnesses,
                "rules_installed": ledger_watch::rules_installed(),
            }))
            .unwrap()
        );
        return Ok(());
    }
    if let Some(why) = &read.unreadable {
        say!(Voice::Suspects, "this may be incomplete: {why}");
    }
    if read.events.is_empty() {
        say!(
            Voice::Notes,
            "no audit events about the ledger in the last 30 days ({} appends logged by emrac).{}",
            read.witnesses,
            if ledger_watch::rules_installed() {
                ""
            } else {
                " The watch is not installed: `sudo emrac ledger watch install`."
            }
        );
        return Ok(());
    }
    say!(
        Voice::Found,
        "{} audit event(s) about the ledger in the last 30 days (times are UTC):\n",
        read.events.len()
    );
    for e in &read.events {
        let mark = match e.verdict {
            Verdict::Emrac => "emrac     ",
            Verdict::Unwitnessed => "UNLOGGED  ",
            Verdict::Foreign => "FOREIGN   ",
        };
        let seen = if e.acknowledged {
            " [acknowledged]"
        } else {
            ""
        };
        println!("  {mark}{} {}{seen}", e.id, e.describe());
    }
    Ok(())
}

/// `emrac ledger events --ack [ID…]`: other programs' events, marked as seen.
fn ack_events(read: &ledger_watch::WatchRead, ids: &[String], json: bool) -> Result<()> {
    let open: Vec<&ledger_watch::WatchEvent> = read
        .events
        .iter()
        .filter(|e| e.verdict != Verdict::Emrac && !e.acknowledged)
        .collect();
    let chosen: Vec<String> = if ids.is_empty() {
        open.iter().map(|e| e.id.clone()).collect()
    } else {
        for id in ids {
            if !read.events.iter().any(|e| &e.id == id) {
                return Err(Error::History(format!(
                    "no ledger event {id} in the last 30 days (`emrac ledger events` lists them with their ids)"
                )));
            }
            if read
                .events
                .iter()
                .any(|e| &e.id == id && e.verdict == Verdict::Emrac)
            {
                return Err(Error::History(format!(
                    "{id} is emrac's own append; there is nothing to acknowledge"
                )));
            }
        }
        ids.to_vec()
    };
    let done = History::acknowledge_events(&chosen)?;
    if !done.is_empty() {
        sync_after();
    }
    if json {
        println!("{}", serde_json::json!({ "acknowledged": done }));
    } else if done.is_empty() {
        say!(Voice::Notes, "nothing new to acknowledge.");
    } else {
        say!(
            Voice::Confirms,
            "acknowledged {} event(s). They stay listed (marked) and in your history and ledger; `doctor` stops counting them.",
            done.len()
        );
    }
    Ok(())
}

/// `sudo emrac ledger watch install`.
pub fn watch_install(dry_run: bool) -> Result<()> {
    use std::path::Path;
    if !ledger::running_as_root() {
        return Err(Error::History(
            "installing audit rules needs root: run `sudo emrac ledger watch install`".into(),
        ));
    }
    let dir = Path::new("/etc/audit/rules.d");
    let augenrules = ["/usr/bin/augenrules", "/usr/sbin/augenrules"]
        .into_iter()
        .find(|p| Path::new(p).exists());
    if !dir.is_dir() || augenrules.is_none() {
        return Err(Error::History(
            "the audit package is not installed (no /etc/audit/rules.d or augenrules): `sudo pacman -S audit`, then run this again".into(),
        ));
    }
    let target = dir.join(ledger_watch::RULES_NAME);
    let loaded = || {
        Command::new(
            ["/usr/bin/auditctl", "/usr/sbin/auditctl"]
                .into_iter()
                .find(|p| Path::new(p).exists())
                .unwrap_or("auditctl"),
        )
        .arg("-l")
        .output()
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains(ledger_watch::KEY) && text.contains(ledger_watch::KEY_ATTR)
        })
        .unwrap_or(false)
    };
    let in_place = fs::read_to_string(&target).is_ok_and(|t| t == ledger_watch::RULES);
    if in_place && loaded() {
        say!(
            Voice::Notes,
            "the audit watch on the ledger is already installed and loaded ({}).",
            target.display()
        );
        return Ok(());
    }
    say!(
        Voice::Plans,
        "write {} and load it with augenrules. It watches every write, rename, delete or permission change of /var/lib/emrac/ and every run of chattr, and logs them; it blocks nothing.",
        target.display()
    );
    println!("{}", ledger_watch::RULES.trim_end());
    if dry_run {
        say!(Voice::Notes, "dry run: nothing was written.");
        return Ok(());
    }
    if !crate::prompt::confirm_default_no("Install it?") {
        say!(Voice::Notes, "nothing was changed.");
        return Ok(());
    }
    if !in_place {
        fs::write(&target, ledger_watch::RULES)
            .map_err(|e| Error::History(format!("couldn't write {}: {e}", target.display())))?;
    }
    let status = Command::new(augenrules.expect("checked above"))
        .arg("--load")
        .status()
        .map_err(|e| Error::History(format!("couldn't run augenrules: {e}")))?;
    // Loading twice makes the kernel say "Rule exists" and augenrules fail;
    // what matters is that the rules are in.
    if !status.success() && !loaded() {
        return Err(Error::History(
            "augenrules --load failed; the rules file is in place and loads at the next start of auditd".into(),
        ));
    }
    say!(
        Voice::Confirms,
        "the audit watch on the ledger is installed and loaded."
    );
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    if !cmdline.split_whitespace().any(|a| a == "audit=1") {
        say!(
            Voice::Suspects,
            "the kernel was not started with audit=1, so programs started before the rules loaded are not covered; add it to the kernel command line."
        );
    }
    say!(
        Voice::Suggests,
        "make sure auditd is enabled (`systemctl enable --now auditd`); `emrac ledger events` and `emrac doctor` read what it saw."
    );
    Ok(())
}

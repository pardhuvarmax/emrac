//! Reading back what the kernel's audit system saw happen to the ledger
//! (see `docs/plan/u9-ledger-watch.md`). The rules that make it watch are
//! `data/50-emrac-ledger.rules`; emrac's own helper leaves a line in the
//! journal for each append (`emrac-ledger`), so a write that has no such
//! line, or was made by another program, stands out.
//!
//! The parser follows the documented audit record format
//! (`type=SYSCALL msg=audit(1700000000.123:42): … exe="/usr/bin/rm" key="emrac-ledger"`,
//! or journald's `SYSCALL … exe=…`). It has been tested on fixtures written
//! from that format, not on a live audit daemon.

use std::collections::BTreeMap;
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};

/// The rules file `emrac ledger watch install` writes.
pub const RULES: &str = include_str!("../data/50-emrac-ledger.rules");
pub const RULES_NAME: &str = "50-emrac-ledger.rules";
pub const KEY: &str = "emrac-ledger";
pub const KEY_ATTR: &str = "emrac-ledger-attr";
pub const WITNESS_TAG: &str = "emrac-ledger";

/// How far apart (seconds) an audit event and emrac's journal line can be
/// and still be the same append.
const WINDOW: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// emrac's own helper, with its journal line.
    Emrac,
    /// emrac's program name, but no journal line: it did not log the append.
    Unwitnessed,
    /// Anything else.
    Foreign,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchEvent {
    /// Unix seconds.
    pub time: u64,
    pub key: String,
    /// What was done: `openat`, `unlinkat`, … or the syscall number.
    pub action: String,
    pub exe: String,
    pub comm: String,
    pub uid: Option<u32>,
    /// The user who logged in, whatever they became since (`None`: unset).
    pub auid: Option<u32>,
    pub success: bool,
    pub paths: Vec<String>,
    pub verdict: Verdict,
}

impl WatchEvent {
    /// One line for a person.
    pub fn describe(&self) -> String {
        let who = match (self.uid, self.auid) {
            (Some(u), Some(a)) if u != a => {
                format!("user {} (logged in as {})", name_of(u), name_of(a))
            }
            (Some(u), _) => format!("user {}", name_of(u)),
            _ => "an unknown user".into(),
        };
        let on = if self.paths.is_empty() {
            String::new()
        } else {
            format!(" on {}", self.paths.join(", "))
        };
        let program = if self.exe.is_empty() {
            &self.comm
        } else {
            &self.exe
        };
        format!(
            "{} {}: {} by {program}, {who}{on}{}",
            crate::history::format_utc(self.time),
            if self.key == KEY_ATTR {
                "chattr was run"
            } else {
                "ledger touched"
            },
            self.action,
            if self.success { "" } else { " (denied)" },
        )
    }
}

/// A line emrac's helper left in the journal.
#[derive(Debug, Clone)]
pub struct Witness {
    pub time: u64,
    pub created: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WatchRead {
    pub events: Vec<WatchEvent>,
    pub witnesses: usize,
}

fn name_of(uid: u32) -> String {
    if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
        for line in passwd.lines() {
            let mut f = line.split(':');
            if let (Some(name), _, Some(id)) = (f.next(), f.next(), f.next()) {
                if id.parse::<u32>().ok() == Some(uid) {
                    return name.to_string();
                }
            }
        }
    }
    format!("uid {uid}")
}

/// `key=value` pairs of an audit record body; values may be quoted.
pub(crate) fn fields(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i] != b'=' && b[i] != b' ' {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        let key = body[start..i].to_string();
        i += 1;
        let value = if i < b.len() && b[i] == b'"' {
            i += 1;
            let v = i;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            let s = body[v..i].to_string();
            i += 1;
            s
        } else {
            let v = i;
            while i < b.len() && b[i] != b' ' {
                i += 1;
            }
            body[v..i].to_string()
        };
        out.insert(key, value);
    }
    out
}

/// Splits `type=SYSCALL msg=audit(123.456:42): rest` (or journald's
/// `SYSCALL rest`) into the record type, the time and the fields.
pub(crate) fn split_record(
    message: &str,
) -> Option<(String, Option<u64>, BTreeMap<String, String>)> {
    let message = message.trim();
    if let Some(rest) = message.strip_prefix("type=") {
        let (kind, rest) = rest.split_once(' ')?;
        let mut time = None;
        let body = match rest.strip_prefix("msg=audit(") {
            Some(r) => {
                let (stamp, body) = r.split_once("): ")?;
                time = stamp.split('.').next().and_then(|s| s.parse().ok());
                body
            }
            None => rest,
        };
        Some((kind.to_string(), time, fields(body)))
    } else {
        let (kind, body) = message.split_once(' ')?;
        kind.chars()
            .all(|c| c.is_ascii_uppercase() || c == '_')
            .then(|| (kind.to_string(), None, fields(body)))
    }
}

fn syscall_name(n: &str) -> String {
    let name = match n {
        "2" => "open",
        "76" => "truncate",
        "77" => "ftruncate",
        "82" => "rename",
        "87" => "unlink",
        "90" => "chmod",
        "91" => "fchmod",
        "92" => "chown",
        "257" => "openat",
        "260" => "fchownat",
        "263" => "unlinkat",
        "264" => "renameat",
        "268" => "fchmodat",
        "316" => "renameat2",
        "59" | "322" => "execve",
        _ => return format!("syscall {n}"),
    };
    name.to_string()
}

#[derive(Default)]
struct Builder {
    time: u64,
    key: String,
    action: String,
    exe: String,
    comm: String,
    uid: Option<u32>,
    auid: Option<u32>,
    success: bool,
    paths: Vec<String>,
}

fn id_field(f: &BTreeMap<String, String>, k: &str) -> Option<u32> {
    f.get(k)
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v != u32::MAX)
}

/// Audit events from `journalctl -o json` output (one JSON object per line).
pub fn parse_journal(text: &str, trusted: &[String]) -> (Vec<WatchEvent>, Vec<Witness>) {
    let mut builders: BTreeMap<String, Builder> = BTreeMap::new();
    let mut witnesses = Vec::new();
    for line in text.lines() {
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let s = |k: &str| entry.get(k).and_then(Value::as_str);
        let micros: u64 = s("__REALTIME_TIMESTAMP")
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);
        let Some(message) = s("MESSAGE") else {
            continue;
        };
        if s("SYSLOG_IDENTIFIER") == Some(WITNESS_TAG) {
            witnesses.push(Witness {
                time: micros / 1_000_000,
                created: message.contains("created=1"),
            });
            continue;
        }
        if s("_TRANSPORT") != Some("audit") {
            continue;
        }
        let Some((kind, time, f)) = split_record(message) else {
            continue;
        };
        let id = s("_AUDIT_ID").map(str::to_string).unwrap_or_else(|| {
            // Raw lines carry the id after the colon; journald has the field.
            "?".into()
        });
        let b = builders.entry(id).or_default();
        b.time = time.unwrap_or(micros / 1_000_000);
        match kind.as_str() {
            "SYSCALL" => {
                b.key = f.get("key").cloned().unwrap_or_default();
                b.action = f
                    .get("syscall")
                    .map_or_else(String::new, |n| syscall_name(n));
                b.exe = f.get("exe").cloned().unwrap_or_default();
                b.comm = f.get("comm").cloned().unwrap_or_default();
                b.uid = id_field(&f, "uid");
                b.auid = id_field(&f, "auid");
                b.success = f.get("success").is_none_or(|v| v == "yes");
            }
            "PATH" => {
                if let Some(n) = f.get("name") {
                    b.paths.push(n.clone());
                }
            }
            _ => {}
        }
    }
    let events = builders
        .into_values()
        .filter(|b| b.key.starts_with(KEY))
        .map(|b| WatchEvent {
            verdict: Verdict::Foreign,
            time: b.time,
            key: b.key,
            action: b.action,
            exe: b.exe,
            comm: b.comm,
            uid: b.uid,
            auid: b.auid,
            success: b.success,
            paths: b.paths,
        })
        .collect();
    let mut events: Vec<WatchEvent> = events;
    classify(&mut events, &witnesses, trusted);
    events.sort_by_key(|e| e.time);
    (events, witnesses)
}

/// Sets each event's verdict: emrac's own only with its program and its
/// journal line close in time.
pub fn classify(events: &mut [WatchEvent], witnesses: &[Witness], trusted: &[String]) {
    let near = |t: u64, only_created: bool| {
        witnesses
            .iter()
            .any(|w| w.time.abs_diff(t) <= WINDOW && (!only_created || w.created))
    };
    for e in events {
        let trusted_exe = trusted.contains(&e.exe);
        e.verdict = if e.key == KEY_ATTR {
            // emrac's helper runs chattr once, when it creates the ledger.
            if near(e.time, true) && e.uid == Some(0) {
                Verdict::Emrac
            } else {
                Verdict::Foreign
            }
        } else if trusted_exe && near(e.time, false) {
            Verdict::Emrac
        } else if trusted_exe {
            Verdict::Unwitnessed
        } else {
            Verdict::Foreign
        };
    }
}

fn journal(args: &[&str]) -> Result<String> {
    let out = Command::new("journalctl")
        .args(["-o", "json", "--no-pager", "--since", "30 days ago"])
        .args(args)
        .output()
        .map_err(|e| Error::History(format!("couldn't run journalctl: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() || stderr.contains("insufficient permissions") {
        return Err(Error::History(
            "the journal can't be read as this user (join the systemd-journal group, or use `sudo emrac ledger events`)"
                .into(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The audit events about the ledger from the last 30 days, and how many
/// appends emrac logged itself.
pub fn read(trusted: &[String]) -> Result<WatchRead> {
    let audit = journal(&["_TRANSPORT=audit"])?;
    let mine = journal(&["-t", WITNESS_TAG])?;
    let (_, mut witnesses) = parse_journal(&mine, trusted);
    let (events, w2) = parse_journal(&audit, trusted);
    witnesses.extend(w2);
    let mut events = events;
    classify(&mut events, &witnesses, trusted);
    Ok(WatchRead {
        witnesses: witnesses.len(),
        events,
    })
}

/// What the doctor (and `history verify`) need to know about the watch.
#[derive(Debug, Clone, Default)]
pub struct WatchFacts {
    pub rules_installed: bool,
    /// Why the events could not be read.
    pub unreadable: Option<String>,
    pub events: usize,
    /// Appends emrac logged in the journal.
    pub witnesses: usize,
    /// Other programs' events, one line each.
    pub foreign: Vec<String>,
    /// emrac's program with no journal line.
    pub unwitnessed: Vec<String>,
}

pub fn facts(trusted: &[String]) -> WatchFacts {
    let mut f = WatchFacts {
        rules_installed: rules_installed(),
        ..WatchFacts::default()
    };
    match read(trusted) {
        Ok(r) => {
            f.events = r.events.len();
            f.witnesses = r.witnesses;
            for e in r.events {
                match e.verdict {
                    Verdict::Emrac => {}
                    Verdict::Unwitnessed => f.unwitnessed.push(e.describe()),
                    Verdict::Foreign => f.foreign.push(e.describe()),
                }
            }
        }
        Err(e) => f.unreadable = Some(e.to_string()),
    }
    f
}

/// Whether the rules file is in place for `auditd` to load at boot.
pub fn rules_installed() -> bool {
    std::path::Path::new("/etc/audit/rules.d")
        .join(RULES_NAME)
        .exists()
}

/// The journal line the helper writes after an append.
pub fn witness_message(
    added: usize,
    first: &str,
    last: &str,
    head_sha: &str,
    created: bool,
) -> String {
    format!(
        "appended={added} first={first} last={last} head={head_sha} created={}",
        u8::from(created)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMRAC: &str = "/usr/bin/emrac";

    fn audit_line(id: u32, secs: u64, record: &str) -> String {
        let message = match record {
            r if r.starts_with("PATH") => r.to_string(),
            r => r.to_string(),
        };
        serde_json::json!({
            "_TRANSPORT": "audit",
            "_AUDIT_ID": id.to_string(),
            "__REALTIME_TIMESTAMP": (secs * 1_000_000).to_string(),
            "MESSAGE": message,
        })
        .to_string()
    }

    fn syscall(exe: &str, comm: &str, uid: u32, auid: u32, key: &str, sys: u32) -> String {
        format!(
            "SYSCALL arch=c000003e syscall={sys} success=yes exit=3 a0=ffffff9c items=2 ppid=100 pid=200 auid={auid} uid={uid} gid=0 euid={uid} tty=pts0 ses=2 comm=\\\"{comm}\\\" exe=\\\"{exe}\\\" key=\\\"{key}\\\""
        )
        .replace("\\\"", "\"")
    }

    fn witness(secs: u64, created: bool) -> String {
        serde_json::json!({
            "SYSLOG_IDENTIFIER": WITNESS_TAG,
            "__REALTIME_TIMESTAMP": (secs * 1_000_000).to_string(),
            "MESSAGE": witness_message(2, "tx-1", "tx-2", "abc", created),
        })
        .to_string()
    }

    fn lines(parts: &[String]) -> String {
        parts.join("\n")
    }

    fn read(parts: &[String]) -> Vec<WatchEvent> {
        parse_journal(&lines(parts), &[EMRAC.to_string()]).0
    }

    #[test]
    fn fields_are_read_with_and_without_quotes() {
        let f = fields(
            r#"arch=c000003e success=yes comm="my prog" exe="/usr/bin/rm" key="emrac-ledger""#,
        );
        assert_eq!(f["success"], "yes");
        assert_eq!(f["comm"], "my prog");
        assert_eq!(f["key"], "emrac-ledger");
        assert_eq!(f["exe"], "/usr/bin/rm");
    }

    #[test]
    fn both_the_raw_and_the_journald_forms_of_a_record_split() {
        let (kind, time, f) = split_record(
            r#"type=SYSCALL msg=audit(1700000000.123:42): syscall=263 exe="/usr/bin/rm" key="emrac-ledger""#,
        )
        .unwrap();
        assert_eq!(
            (kind.as_str(), time, f["syscall"].as_str()),
            ("SYSCALL", Some(1_700_000_000), "263")
        );
        let (kind, time, f) =
            split_record(r#"PATH item=0 name="/var/lib/emrac/ledger.jsonl""#).unwrap();
        assert_eq!((kind.as_str(), time), ("PATH", None));
        assert_eq!(f["name"], "/var/lib/emrac/ledger.jsonl");
        assert!(split_record("some ordinary message here").is_none());
    }

    #[test]
    fn emracs_own_append_with_its_journal_line_is_not_flagged() {
        let ev = read(&[
            audit_line(1, 1000, &syscall(EMRAC, "emrac", 0, 1000, KEY, 257)),
            audit_line(
                1,
                1000,
                r#"PATH item=1 name="/var/lib/emrac/ledger.jsonl" nametype=NORMAL"#,
            ),
            witness(1001, false),
        ]);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].verdict, Verdict::Emrac);
        assert_eq!(ev[0].paths, ["/var/lib/emrac/ledger.jsonl"]);
        assert_eq!(ev[0].action, "openat");
        assert_eq!((ev[0].uid, ev[0].auid), (Some(0), Some(1000)));
    }

    #[test]
    fn other_programs_are_named_with_user_and_login_user() {
        let ev = read(&[
            audit_line(7, 5000, &syscall("/usr/bin/rm", "rm", 0, 1000, KEY, 263)),
            audit_line(
                8,
                5002,
                &syscall("/usr/bin/vim", "vim", 0, 4294967295, KEY, 257),
            ),
            witness(5001, false),
        ]);
        assert_eq!(ev.len(), 2);
        assert!(
            ev.iter().all(|e| e.verdict == Verdict::Foreign),
            "a witness doesn't excuse another program"
        );
        let text = ev[0].describe();
        assert!(
            text.contains("/usr/bin/rm") && text.contains("unlinkat"),
            "{text}"
        );
        assert_eq!(ev[1].auid, None, "an unset login user is not a user");
    }

    #[test]
    fn emrac_without_a_journal_line_is_unwitnessed_and_a_late_line_does_not_count() {
        let ev = read(&[
            audit_line(1, 1000, &syscall(EMRAC, "emrac", 0, 1000, KEY, 257)),
            witness(1000 + WINDOW + 1, false),
        ]);
        assert_eq!(ev[0].verdict, Verdict::Unwitnessed);
    }

    #[test]
    fn chattr_is_foreign_except_when_emrac_just_created_the_ledger() {
        let chattr = syscall("/usr/bin/chattr", "chattr", 0, 1000, KEY_ATTR, 59);
        let foreign = read(&[audit_line(3, 2000, &chattr), witness(2001, false)]);
        assert_eq!(
            foreign[0].verdict,
            Verdict::Foreign,
            "a plain append is no reason to run chattr"
        );
        let own = read(&[audit_line(3, 2000, &chattr), witness(2001, true)]);
        assert_eq!(own[0].verdict, Verdict::Emrac);
        assert_eq!(own[0].action, "execve");
    }

    #[test]
    fn events_with_other_keys_and_unrelated_journal_lines_are_ignored() {
        let ev = read(&[
            audit_line(
                1,
                1,
                &syscall("/usr/bin/rm", "rm", 0, 0, "something-else", 263),
            ),
            audit_line(2, 2, "USER_LOGIN pid=1 uid=0"),
            "not json at all".to_string(),
            serde_json::json!({"MESSAGE": "hello"}).to_string(),
        ]);
        assert!(ev.is_empty());
    }

    #[test]
    fn a_denied_write_is_still_reported_as_denied() {
        let mut s = syscall("/usr/bin/sh", "sh", 1000, 1000, KEY, 257);
        s = s.replace("success=yes", "success=no");
        let ev = read(&[audit_line(1, 10, &s)]);
        assert!(!ev[0].success);
        assert!(ev[0].describe().contains("(denied)"));
    }

    #[test]
    fn the_shipped_rules_watch_the_ledger_folder_and_chattr() {
        let rules: Vec<&str> = RULES
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect();
        assert_eq!(
            rules,
            [
                "-w /var/lib/emrac/ -p wa -k emrac-ledger",
                "-w /usr/bin/chattr -p x -k emrac-ledger-attr",
            ]
        );
        assert!(
            !RULES.contains("-e 2"),
            "locking the rules is the administrator's choice"
        );
    }
}

//! `.pacnew` / `.pacsave` files: finding the old shipped version of a config
//! file (the "base"), telling what the user changed from what upstream changed,
//! and merging. Pure functions and small file helpers; the lookups that need
//! libalpm, the pacman cache or the Archive live in `sources.rs`. See
//! `docs/plan/u27-pacnew.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::error::{Error, Result};

/// What a pending file is, compared three ways: the version the old package
/// shipped (base), the live file (ours) and the new one (theirs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    /// Live file is what the old package shipped: only upstream changed.
    Untouched,
    /// The `.pacnew` is what the old package shipped: only the user changed it.
    MineOnly,
    /// Both changed it.
    BothChanged,
    /// Live file and `.pacnew` are the same.
    Identical,
    /// No base to compare with (or the files can't be read as text).
    Unknown,
    /// A `.pacsave`: made when a package with edited config was removed.
    Pacsave,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Untouched => "untouched",
            Class::MineOnly => "mine-only",
            Class::BothChanged => "both-changed",
            Class::Identical => "identical",
            Class::Unknown => "unknown",
            Class::Pacsave => "pacsave",
        }
    }
}

/// How the three-way merge of a `both-changed` file came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    /// No overlapping changes: this is the merged file.
    Clean(String),
    /// This many places changed on both sides in different ways.
    Conflicts(u32),
}

/// The three texts a pending file is judged by.
#[derive(Debug, Clone, Default)]
pub struct Texts {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pending {
    /// The live file, e.g. `/etc/services`.
    pub live: String,
    /// The `.pacnew` / `.pacsave` beside it.
    pub pending: String,
    pub package: Option<String>,
    pub class: Class,
    /// One line saying why it was put in that class.
    pub reason: String,
    /// The old package version the base was taken from.
    pub base_version: Option<String>,
    pub age_days: Option<u64>,
    /// Changing it wrongly can lock the user out of the machine.
    pub critical: bool,
    /// `clean`, `conflicts` or absent.
    pub merge: Option<String>,
    #[serde(skip)]
    pub texts: Texts,
    #[serde(skip)]
    pub merged: Option<Merge>,
}

impl Pending {
    pub fn is_pacsave(&self) -> bool {
        self.class == Class::Pacsave
    }
}

/// One thing pacman.log says that matters here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    Upgraded { pkg: String, old: String },
}

/// `upgraded <pkg> (<old> -> <new>)` entries, oldest first.
pub fn parse_log(log: &str) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for line in log.lines() {
        let Some(rest) = line.split("[ALPM] upgraded ").nth(1) else {
            continue;
        };
        let Some((pkg, versions)) = rest.split_once(" (") else {
            continue;
        };
        let Some((old, _new)) = versions.trim_end_matches(')').split_once(" -> ") else {
            continue;
        };
        out.push(LogEvent::Upgraded {
            pkg: pkg.to_string(),
            old: old.to_string(),
        });
    }
    out
}

/// The versions `pkg` was upgraded *from*, newest first, without repeats. A
/// base is the most recent of them whose copy of the file differs from the
/// current one.
pub fn earlier_versions(events: &[LogEvent], pkg: &str, limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for LogEvent::Upgraded { pkg: p, old } in events.iter().rev() {
        if p == pkg && !out.contains(old) {
            out.push(old.clone());
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Which of the three texts agree decides the class.
pub fn classify(base: Option<&str>, ours: &str, theirs: &str) -> (Class, &'static str) {
    if ours == theirs {
        return (
            Class::Identical,
            "the live file and the new one are the same",
        );
    }
    let Some(base) = base else {
        return (
            Class::Unknown,
            "no earlier version of the file to compare with, so edits can't be told from upstream's changes",
        );
    };
    if ours == base {
        (
            Class::Untouched,
            "the live file is what the old package shipped; only upstream changed it",
        )
    } else if theirs == base {
        (
            Class::MineOnly,
            "the new file is what the old package shipped; only you changed the live one",
        )
    } else {
        (
            Class::BothChanged,
            "both the live file and upstream's file changed",
        )
    }
}

/// Config files whose wrong change can lock the user out.
pub fn is_critical(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "/etc/sudoers",
        "/etc/passwd",
        "/etc/shadow",
        "/etc/group",
        "/etc/gshadow",
        "/etc/fstab",
        "/etc/crypttab",
        "/etc/ssh/sshd_config",
        "/etc/doas.conf",
    ];
    const PREFIXES: &[&str] = &["/etc/sudoers.d/", "/etc/pam.d/", "/etc/ssh/sshd_config.d/"];
    EXACT.contains(&path) || PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Three-way merge with `git merge-file -p` (git is already needed for AUR
/// builds). A merge that fails to run is an error, not a conflict.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Result<Merge> {
    let dir = private_temp_dir("merge")?;
    let result = (|| {
        let write = |name: &str, text: &str| -> Result<PathBuf> {
            let path = dir.join(name);
            std::fs::write(&path, text).map_err(|source| Error::Io {
                context: format!("couldn't write {}", path.display()),
                source,
            })?;
            Ok(path)
        };
        let (o, b, t) = (
            write("ours", ours)?,
            write("base", base)?,
            write("theirs", theirs)?,
        );
        let output = Command::new("git")
            .args(["merge-file", "-p"])
            .arg(&o)
            .arg(&b)
            .arg(&t)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("git".to_string(), err))?;
        match output.status.code() {
            Some(0) => Ok(Merge::Clean(
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )),
            Some(n) if (1..=127).contains(&n) => Ok(Merge::Conflicts(n as u32)),
            _ => Err(Error::CommandFailed {
                command: "git merge-file".to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            }),
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The three-way result as text, with `diff3`-style conflict markers where the
/// two sides disagree, and how many conflicts there are.
pub fn merge3_markers(base: &str, ours: &str, theirs: &str) -> Result<(String, u32)> {
    let dir = private_temp_dir("markers")?;
    let result = (|| {
        let mut paths = Vec::new();
        for (name, text) in [("ours", ours), ("base", base), ("theirs", theirs)] {
            let path = dir.join(name);
            std::fs::write(&path, text).map_err(|source| Error::Io {
                context: format!("couldn't write {}", path.display()),
                source,
            })?;
            paths.push(path);
        }
        let output = Command::new("git")
            .args(["merge-file", "-p", "--diff3"])
            .args([
                "-L",
                "your file",
                "-L",
                "the old package's file",
                "-L",
                "the new file",
            ])
            .args(&paths)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("git".to_string(), err))?;
        match output.status.code() {
            Some(n) if (0..=127).contains(&n) => Ok((
                String::from_utf8_lossy(&output.stdout).into_owned(),
                n as u32,
            )),
            _ => Err(Error::CommandFailed {
                command: "git merge-file".to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            }),
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Does `text` still have a merge conflict marker line?
pub fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|l| {
        ["<<<<<<<", ">>>>>>>", "|||||||"]
            .iter()
            .any(|m| l == *m || l.starts_with(&format!("{m} ")))
    })
}

/// What checking a candidate file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// Checked, and fine: what was checked.
    Valid(String),
    /// Nothing here knows how to check this file.
    NoValidator,
    /// Checked, and wrong: why.
    Invalid(String),
}

/// Files `bash -n` can check because they are sourced as shell.
const SHELL_SOURCED: &[&str] = &[
    "/etc/mkinitcpio.conf",
    "/etc/makepkg.conf",
    "/etc/default/grub",
];

/// Checks `candidate`, the text that would become `live`, with whatever can
/// check that kind of file (`docs/plan/u30-pacnew-revert.md`).
pub fn validate(live: &str, candidate: &str) -> Result<Validation> {
    if live == "/etc/pacman.d/mirrorlist" {
        let has_server = candidate.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with("Server") && l[6..].trim_start().starts_with('=')
        });
        return Ok(if has_server {
            Validation::Valid("it has a Server line".into())
        } else {
            Validation::Invalid(
                "no uncommented Server line: pacman would have no mirrors to use".into(),
            )
        });
    }
    if live.ends_with(".json") {
        return Ok(match serde_json::from_str::<serde_json::Value>(candidate) {
            Ok(_) => Validation::Valid("it parses as JSON".into()),
            Err(e) => Validation::Invalid(format!("not valid JSON: {e}")),
        });
    }
    // (tool, arguments before the file, needs root, exit codes that pass)
    let tool: Option<(&str, Vec<&str>, bool, &[i32])> =
        if live == "/etc/sudoers" || live.starts_with("/etc/sudoers.d/") {
            Some(("visudo", vec!["-cf"], true, &[0]))
        } else if live == "/etc/ssh/sshd_config" {
            Some(("sshd", vec!["-t", "-f"], true, &[0]))
        } else if live == "/etc/pacman.conf" {
            Some(("pacman-conf", vec!["--config"], false, &[0]))
        } else if live == "/etc/fstab" {
            Some(("findmnt", vec!["--verify", "--tab-file"], false, &[0, 64]))
        } else if SHELL_SOURCED.contains(&live) {
            Some(("bash", vec!["-n"], false, &[0]))
        } else {
            None
        };
    let Some((program, args, root, ok_codes)) = tool else {
        return Ok(Validation::NoValidator);
    };
    let dir = private_temp_dir("validate")?;
    let result = (|| {
        let path = dir.join("candidate");
        std::fs::write(&path, candidate).map_err(|source| Error::Io {
            context: format!("couldn't write {}", path.display()),
            source,
        })?;
        let mut cmd = if root {
            let mut c = Command::new("sudo");
            c.arg(program);
            c
        } else {
            Command::new(program)
        };
        cmd.args(&args).arg(&path);
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Validation::NoValidator)
            }
            Err(e) => return Err(Error::CommandSpawn(program.to_string(), e)),
        };
        let code = output.status.code().unwrap_or(-1);
        if ok_codes.contains(&code) {
            return Ok(Validation::Valid(format!("{program} accepts it")));
        }
        let mut said = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if said.is_empty() {
            said = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        let said: String = said.chars().take(400).collect();
        Ok(Validation::Invalid(format!("{program} says: {said}")))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// `diff -u` of two texts, empty when they are the same.
pub fn unified_diff(a_label: &str, a: &str, b_label: &str, b: &str) -> Result<String> {
    if a == b {
        return Ok(String::new());
    }
    let dir = private_temp_dir("diff")?;
    let result = (|| {
        let (pa, pb) = (dir.join("a"), dir.join("b"));
        for (path, text) in [(&pa, a), (&pb, b)] {
            std::fs::write(path, text).map_err(|source| Error::Io {
                context: format!("couldn't write {}", path.display()),
                source,
            })?;
        }
        let output = Command::new("diff")
            .args(["-u", "--label", a_label, "--label", b_label])
            .arg(&pa)
            .arg(&pb)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("diff".to_string(), err))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn private_temp_dir(what: &str) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("emrac-{what}-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|source| Error::Io {
            context: format!("couldn't create {}", dir.display()),
            source,
        })?;
    Ok(dir)
}

/// Where the live file is copied before it is changed:
/// `~/.local/share/emrac/config-backups/<stamp>/<path without the leading />`.
pub fn backup_path(home: &Path, stamp: &str, live: &str) -> PathBuf {
    home.join(".local/share/emrac/config-backups")
        .join(stamp)
        .join(live.trim_start_matches('/'))
}

/// The live path for a `.pacnew` / `.pacsave` path.
pub fn live_path(pending: &str) -> Option<&str> {
    pending
        .strip_suffix(".pacnew")
        .or_else(|| pending.strip_suffix(".pacsave"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
[2026-05-05T17:59:16+0530] [ALPM] warning: /etc/mkinitcpio.conf installed as /etc/mkinitcpio.conf.pacnew
[2026-05-05T17:59:16+0530] [ALPM] upgraded mkinitcpio (40-4 -> 41-3)
[2026-05-05T17:59:17+0530] [ALPM] upgraded linux (6.19.14.arch1-1 -> 7.0.3.arch1-2)
[2026-08-21T21:21:27+0530] [ALPM] upgraded mkinitcpio (41-3 -> 41.1-1)
[2026-10-03T20:43:37+0530] [ALPM] upgraded mkinitcpio (41.1-1 -> 42.2-1)
[2026-10-03T20:43:38+0530] [ALPM] installed foo (1-1)
[2026-10-04T10:00:00+0530] [ALPM] upgraded mkinitcpio (41.1-1 -> 42.2-1)
garbage line
";

    #[test]
    fn upgrades_are_read_from_the_log_and_earlier_versions_come_newest_first() {
        let events = parse_log(LOG);
        assert_eq!(events.len(), 5);
        assert_eq!(
            earlier_versions(&events, "mkinitcpio", 10),
            ["41.1-1", "41-3", "40-4"]
        );
        assert_eq!(
            earlier_versions(&events, "mkinitcpio", 2),
            ["41.1-1", "41-3"]
        );
        assert!(earlier_versions(&events, "nothing", 3).is_empty());
    }

    #[test]
    fn the_class_follows_which_texts_agree() {
        assert_eq!(classify(Some("a"), "b", "b").0, Class::Identical);
        assert_eq!(classify(Some("a"), "a", "c").0, Class::Untouched);
        assert_eq!(classify(Some("a"), "b", "a").0, Class::MineOnly);
        assert_eq!(classify(Some("a"), "b", "c").0, Class::BothChanged);
        assert_eq!(classify(None, "b", "c").0, Class::Unknown);
        assert_eq!(
            classify(None, "b", "b").0,
            Class::Identical,
            "equal needs no base"
        );
    }

    #[test]
    fn files_that_can_lock_the_user_out_are_marked() {
        for p in [
            "/etc/sudoers",
            "/etc/sudoers.d/wheel",
            "/etc/pam.d/system-auth",
            "/etc/fstab",
            "/etc/ssh/sshd_config",
        ] {
            assert!(is_critical(p), "{p}");
        }
        for p in ["/etc/services", "/etc/pacman.d/mirrorlist", "/etc/pamx"] {
            assert!(!is_critical(p), "{p}");
        }
    }

    #[test]
    fn the_backup_keeps_the_path_under_a_stamped_folder() {
        assert_eq!(
            backup_path(Path::new("/home/u"), "20261014-101500", "/etc/pacman.conf"),
            Path::new("/home/u/.local/share/emrac/config-backups/20261014-101500/etc/pacman.conf")
        );
        assert_eq!(live_path("/etc/x.pacnew"), Some("/etc/x"));
        assert_eq!(live_path("/etc/x.pacsave"), Some("/etc/x"));
        assert_eq!(live_path("/etc/x"), None);
    }

    #[test]
    fn a_merge_is_clean_when_the_changes_do_not_overlap_and_a_conflict_otherwise() {
        let base = "a\nb\nc\nd\ne\n";
        match merge3(base, "a\nB\nc\nd\ne\n", "a\nb\nc\nd\nE\n").unwrap() {
            Merge::Clean(text) => assert_eq!(text, "a\nB\nc\nd\nE\n"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            merge3(base, "a\nB\nc\nd\ne\n", "a\nX\nc\nd\ne\n").unwrap(),
            Merge::Conflicts(1)
        );
    }

    #[test]
    fn markers_are_written_where_both_sides_differ_and_found_again() {
        let base = "a\nb\nc\n";
        let (text, n) = merge3_markers(base, "a\nB\nc\n", "a\nX\nc\n").unwrap();
        assert_eq!(n, 1);
        assert!(text.contains("<<<<<<< your file") && text.contains("B"));
        assert!(text.contains("||||||| the old package's file"));
        assert!(text.contains(">>>>>>> the new file"));
        assert!(has_conflict_markers(&text));
        let (clean, n) = merge3_markers(base, "a\nB\nc\n", "a\nb\nc\nd\n").unwrap();
        assert_eq!(n, 0);
        assert_eq!(clean, "a\nB\nc\nd\n");
        assert!(!has_conflict_markers(&clean));
        assert!(!has_conflict_markers("# ======= a banner\nx <<<<<<< y\n"));
    }

    #[test]
    fn candidates_are_checked_by_what_can_check_them() {
        let mirrors = "/etc/pacman.d/mirrorlist";
        let none = "## Arch Linux repository mirrorlist\n#Server = https://a/$repo/os/$arch\n";
        assert!(matches!(
            validate(mirrors, none).unwrap(),
            Validation::Invalid(_)
        ));
        assert!(matches!(
            validate(mirrors, "#x\nServer = https://a/$repo/os/$arch\n").unwrap(),
            Validation::Valid(_)
        ));
        assert!(matches!(
            validate("/etc/tpm2-tss/p.json", "{\"a\": 1}").unwrap(),
            Validation::Valid(_)
        ));
        assert!(matches!(
            validate("/etc/tpm2-tss/p.json", "{\"a\": ").unwrap(),
            Validation::Invalid(_)
        ));
        assert_eq!(
            validate("/etc/services", "x\n").unwrap(),
            Validation::NoValidator
        );
        let conf = "/etc/mkinitcpio.conf";
        assert!(matches!(
            validate(conf, "HOOKS=(base udev)\n").unwrap(),
            Validation::Valid(_)
        ));
        match validate(conf, "HOOKS=(base udev\n").unwrap() {
            Validation::Invalid(why) => assert!(why.starts_with("bash says"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diff_is_empty_for_equal_texts_and_labelled_otherwise() {
        assert_eq!(unified_diff("x", "a\n", "y", "a\n").unwrap(), "");
        let d = unified_diff("old", "a\nb\n", "new", "a\nc\n").unwrap();
        assert!(d.contains("--- old") && d.contains("+++ new"), "{d}");
        assert!(d.contains("-b") && d.contains("+c"), "{d}");
    }
}

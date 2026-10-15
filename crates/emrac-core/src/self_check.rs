//! `emrac self check` (u11): a quick, read-only look at emrac's own
//! installation. It needs no libalpm, no network and no root, so it still
//! works on the day pacman does not. Facts are gathered by `gather` and turned
//! into a report by `evaluate`, a pure function that the tests drive.
//! See `docs/plan/r2/u11-self-check.md`.

use std::path::Path;
use std::process::Command;

use crate::doctor::{Check, DoctorReport, Status};

/// How this copy of emrac got onto the machine, as far as can be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// A pacman package owns the binary.
    Package {
        name: String,
        version: String,
        /// Not in any sync repository: built locally or from the AUR.
        foreign: bool,
    },
    /// `cargo install` (the binary is under `~/.cargo/bin`).
    Cargo,
    /// A copy someone put in `/usr/local` or `~/.local/bin`.
    Manual(String),
    /// Run from a cargo `target/` directory.
    Development,
    /// Could not be told. Never guessed.
    Unknown(String),
}

/// Looks at where the binary is and classifies it when no package owns it.
pub fn classify_path(exe: &str, home: &str) -> Owner {
    if exe.contains("/target/debug/") || exe.contains("/target/release/") {
        Owner::Development
    } else if !home.is_empty() && exe.starts_with(&format!("{home}/.cargo/bin/")) {
        Owner::Cargo
    } else if exe.starts_with("/usr/local/")
        || (!home.is_empty() && exe.starts_with(&format!("{home}/.local/bin/")))
    {
        Owner::Manual(exe.to_string())
    } else {
        Owner::Unknown(format!(
            "no package owns {exe} and it is in no place emrac knows"
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ExeFacts {
    pub path: String,
    /// The file was replaced or removed while this process runs.
    pub replaced: bool,
    pub readable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HelperFacts {
    /// Where the binary the sudo helper runs lives (`/usr/bin/emrac`).
    pub path: String,
    pub exists: bool,
    pub root_owned: bool,
    /// Group- or world-writable, so not safe to run as root.
    pub writable_by_others: bool,
}

#[derive(Debug, Clone)]
pub enum Journal {
    Clean,
    Running,
    /// A transaction was cut off: a word about it.
    Interrupted(String),
}

#[derive(Debug, Clone)]
pub struct SelfFacts {
    pub exe: Result<ExeFacts, String>,
    pub version: String,
    pub owner: Owner,
    pub config_error: Option<String>,
    pub config_world_writable: bool,
    /// `Err`: the history could not be read.
    pub history: Result<HistoryState, String>,
    pub journal: Journal,
    /// Why emrac's data folder can't be written, if it can't.
    pub data_dir_problem: Option<String>,
    pub helper: HelperFacts,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryState {
    pub records: usize,
    /// Where the chain breaks, worded.
    pub problem: Option<String>,
}

fn check(
    id: &'static str,
    title: &'static str,
    status: Status,
    summary: impl Into<String>,
) -> Check {
    Check {
        id,
        title,
        status,
        summary: summary.into(),
        detail: Vec::new(),
        fix: None,
    }
}

/// `x.y.z` with numbers only (an optional `-suffix` is allowed).
pub fn version_is_well_formed(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

pub fn evaluate(f: &SelfFacts) -> DoctorReport {
    let mut checks = Vec::new();

    // The executable.
    checks.push(match &f.exe {
        Err(why) => check(
            "exe",
            "Executable",
            Status::Problem,
            format!("can't tell which file is running: {why}"),
        ),
        Ok(e) if !e.readable => check(
            "exe",
            "Executable",
            Status::Problem,
            format!("{} can't be read", e.path),
        ),
        Ok(e) if e.replaced => {
            let mut c = check(
                "exe",
                "Executable",
                Status::Warning,
                format!(
                    "{} was replaced or removed while this ran (an upgrade of emrac?)",
                    e.path
                ),
            );
            c.fix = Some("run the command again to use the new binary".into());
            c
        }
        Ok(e) => check("exe", "Executable", Status::Ok, e.path.clone()),
    });

    // The version.
    checks.push(if version_is_well_formed(&f.version) {
        check(
            "version",
            "Version",
            Status::Ok,
            format!("{} (build of this binary)", f.version),
        )
    } else {
        check(
            "version",
            "Version",
            Status::Warning,
            format!("'{}' is not a version number of the form x.y.z", f.version),
        )
    });

    // How it was installed.
    checks.push(match &f.owner {
        Owner::Package {
            name,
            version,
            foreign,
        } => {
            let mut c = check(
                "owner",
                "Installation",
                Status::Ok,
                format!(
                    "package {name} {version}{}",
                    if *foreign {
                        " (built locally or from the AUR)"
                    } else {
                        ""
                    }
                ),
            );
            if *version != f.version && !version.starts_with(&format!("{}-", f.version)) {
                c.status = Status::Note;
                c.detail.push(format!(
                    "the package says {version}, the binary says {}",
                    f.version
                ));
            }
            c
        }
        Owner::Cargo => check(
            "owner",
            "Installation",
            Status::Note,
            "installed with `cargo install`: pacman does not know this copy",
        ),
        Owner::Manual(p) => check(
            "owner",
            "Installation",
            Status::Note,
            format!("a copy put in place by hand ({p}): pacman does not know it"),
        ),
        Owner::Development => check(
            "owner",
            "Installation",
            Status::Note,
            "a development build run from a cargo target folder",
        ),
        Owner::Unknown(why) => {
            let mut c = check(
                "owner",
                "Installation",
                Status::Note,
                "UNKNOWN: emrac can't tell how it was installed",
            );
            c.detail.push(why.clone());
            c
        }
    });

    // Updating.
    checks.push(match &f.owner {
        Owner::Package { foreign: true, .. } => check(
            "update",
            "Updating",
            Status::Note,
            "it comes through your AUR helper or a rebuild of the package; emrac does not update itself yet",
        ),
        Owner::Package { .. } => check("update", "Updating", Status::Note, "through `pacman -Syu`; emrac does not update itself yet"),
        _ => check(
            "update",
            "Updating",
            Status::Note,
            "no package manager owns this copy, so it has to be replaced by hand; emrac does not update itself yet",
        ),
    });

    // Configuration.
    checks.push(if let Some(err) = &f.config_error {
        let mut c = check(
            "config",
            "Configuration",
            Status::Problem,
            "config.toml can't be read",
        );
        c.detail.push(err.clone());
        c.fix = Some("emrac config validate  (or fix the file named above)".into());
        c
    } else if f.config_world_writable {
        let mut c = check(
            "config",
            "Configuration",
            Status::Warning,
            "config.toml is writable by everyone",
        );
        c.fix = Some("chmod o-w ~/.config/emrac/config.toml".into());
        c
    } else {
        check(
            "config",
            "Configuration",
            Status::Ok,
            "config.toml reads cleanly",
        )
    });

    // Local state.
    checks.push(match &f.history {
        Err(why) => {
            let mut c = check(
                "state",
                "Local state",
                Status::Problem,
                "the history can't be read",
            );
            c.detail.push(why.clone());
            c
        }
        Ok(h) => match &h.problem {
            Some(p) => {
                let mut c = check(
                    "state",
                    "Local state",
                    Status::Problem,
                    "the history chain is broken",
                );
                c.detail.push(p.clone());
                c.fix = Some("emrac history verify".into());
                c
            }
            None => check(
                "state",
                "Local state",
                Status::Ok,
                format!(
                    "history readable and chained ({} record{})",
                    h.records,
                    if h.records == 1 { "" } else { "s" }
                ),
            ),
        },
    });

    // The data folder.
    checks.push(match &f.data_dir_problem {
        Some(why) => {
            let mut c = check(
                "data-dir",
                "Data folder",
                Status::Problem,
                "~/.local/share/emrac can't be written",
            );
            c.detail.push(why.clone());
            c
        }
        None => check(
            "data-dir",
            "Data folder",
            Status::Ok,
            "~/.local/share/emrac is writable",
        ),
    });

    // The operation journal.
    checks.push(match &f.journal {
        Journal::Clean => check(
            "journal",
            "Operation journal",
            Status::Ok,
            "no unfinished transaction",
        ),
        Journal::Running => check(
            "journal",
            "Operation journal",
            Status::Note,
            "another emrac is changing packages right now",
        ),
        Journal::Interrupted(what) => {
            let mut c = check(
                "journal",
                "Operation journal",
                Status::Warning,
                format!("a transaction was cut off before it was recorded: {what}"),
            );
            c.fix = Some("emrac history recover".into());
            c
        }
    });

    // The binary that runs as root for the ledger.
    let h = &f.helper;
    checks.push(if !h.exists {
        check(
            "helper",
            "Ledger helper",
            Status::Note,
            format!("{} does not exist, so `sudo emrac ledger-append` can't run (the history is then kept without the ledger)", h.path),
        )
    } else if !h.root_owned || h.writable_by_others {
        let mut c = check(
            "helper",
            "Ledger helper",
            Status::Problem,
            format!("{} is not safe to run as root: it is {}", h.path,
                if !h.root_owned { "not owned by root" } else { "writable by others" }),
        );
        c.fix = Some(format!("sudo chown root:root {p} && sudo chmod 755 {p}", p = h.path));
        c
    } else {
        check("helper", "Ledger helper", Status::Ok, format!("{} is owned by root and not writable by others", h.path))
    });

    checks.sort_by_key(|c| std::cmp::Reverse(c.status));
    DoctorReport { checks }
}

/// Collects the facts. Reads only; the one process it starts is `pacman -Qo`
/// (a database query).
pub fn gather() -> SelfFacts {
    let home = std::env::var("HOME").unwrap_or_default();
    let exe = std::env::current_exe().map_err(|e| e.to_string()).map(|p| {
        let shown = p.display().to_string();
        let replaced = shown.ends_with(" (deleted)");
        let path = shown.trim_end_matches(" (deleted)").to_string();
        ExeFacts {
            readable: replaced || std::fs::File::open(&path).is_ok(),
            path,
            replaced,
        }
    });
    let owner = match &exe {
        Ok(e) => owner_of(&e.path, &home),
        Err(why) => Owner::Unknown(why.clone()),
    };
    let (config_error, config_world_writable) = match crate::Config::path() {
        Ok(path) => (
            crate::Config::load().err().map(|e| e.to_string()),
            std::fs::metadata(&path)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o002 != 0
                })
                .unwrap_or(false),
        ),
        Err(_) => (None, false),
    };
    let history = crate::History::verify()
        .map(|v| HistoryState {
            records: v.chained + v.legacy,
            problem: v.problem.as_ref().map(|b| b.describe()),
        })
        .map_err(|e| e.to_string());
    let journal = match crate::journal::state() {
        Ok(crate::journal::State::Interrupted(found)) => Journal::Interrupted(match &found.entry {
            Some(e) => format!(
                "{} packages, started {} UTC",
                e.packages.len(),
                crate::history::format_utc(e.started)
            ),
            None => "its journal is incomplete".to_string(),
        }),
        Ok(crate::journal::State::Running) => Journal::Running,
        _ => Journal::Clean,
    };
    SelfFacts {
        exe,
        version: env!("CARGO_PKG_VERSION").to_string(),
        owner,
        config_error,
        config_world_writable,
        history,
        journal,
        data_dir_problem: data_dir_problem(&home),
        helper: helper_facts(),
    }
}

fn owner_of(exe: &str, home: &str) -> Owner {
    let query = |args: &[&str]| {
        Command::new("pacman")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    match query(&["-Qqo", exe]) {
        Some(name) if !name.is_empty() => {
            let version = query(&["-Q", &name])
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
                .unwrap_or_default();
            let foreign = query(&["-Qqm", &name]).is_some_and(|m| m == name);
            Owner::Package {
                name,
                version,
                foreign,
            }
        }
        _ => classify_path(exe, home),
    }
}

fn data_dir_problem(home: &str) -> Option<String> {
    let dir = Path::new(home).join(".local/share/emrac");
    if !dir.exists() {
        // Created on first use; fine as long as the parent can take it.
        return None;
    }
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(e) => Some(format!("{}: {e}", dir.display())),
    }
}

fn helper_facts() -> HelperFacts {
    use std::os::unix::fs::MetadataExt;
    let path = "/usr/bin/emrac".to_string();
    match std::fs::metadata(&path) {
        Ok(m) => HelperFacts {
            path,
            exists: true,
            root_owned: m.uid() == 0,
            writable_by_others: m.mode() & 0o022 != 0,
        },
        Err(_) => HelperFacts {
            path,
            ..HelperFacts::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> SelfFacts {
        SelfFacts {
            exe: Ok(ExeFacts {
                path: "/usr/bin/emrac".into(),
                replaced: false,
                readable: true,
            }),
            version: "0.2.0".into(),
            owner: Owner::Package {
                name: "emrac".into(),
                version: "0.2.0-10".into(),
                foreign: true,
            },
            config_error: None,
            config_world_writable: false,
            history: Ok(HistoryState {
                records: 5,
                problem: None,
            }),
            journal: Journal::Clean,
            data_dir_problem: None,
            helper: HelperFacts {
                path: "/usr/bin/emrac".into(),
                exists: true,
                root_owned: true,
                writable_by_others: false,
            },
        }
    }

    fn status(r: &DoctorReport, id: &str) -> Status {
        r.checks.iter().find(|c| c.id == id).unwrap().status
    }

    #[test]
    fn a_healthy_install_has_nothing_above_a_note() {
        let r = evaluate(&healthy());
        assert!(r.worst() <= Status::Note, "{:?}", r.checks);
        assert_eq!(status(&r, "owner"), Status::Ok);
        assert_eq!(r.checks.len(), 9);
    }

    #[test]
    fn where_the_binary_lives_decides_the_owner_and_unknown_stays_unknown() {
        assert_eq!(
            classify_path("/home/u/proj/target/release/emrac", "/home/u"),
            Owner::Development
        );
        assert_eq!(
            classify_path("/home/u/.cargo/bin/emrac", "/home/u"),
            Owner::Cargo
        );
        assert!(matches!(
            classify_path("/usr/local/bin/emrac", "/home/u"),
            Owner::Manual(_)
        ));
        assert!(matches!(
            classify_path("/opt/odd/emrac", "/home/u"),
            Owner::Unknown(_)
        ));
        let mut f = healthy();
        f.owner = Owner::Unknown("x".into());
        let r = evaluate(&f);
        let c = r.checks.iter().find(|c| c.id == "owner").unwrap();
        assert!(c.summary.starts_with("UNKNOWN"));
        assert_eq!(c.status, Status::Note);
    }

    #[test]
    fn a_package_whose_version_differs_from_the_binary_is_noted() {
        let mut f = healthy();
        f.owner = Owner::Package {
            name: "emrac".into(),
            version: "0.3.0-1".into(),
            foreign: false,
        };
        assert_eq!(status(&evaluate(&f), "owner"), Status::Note);
    }

    #[test]
    fn version_strings_are_validated() {
        assert!(version_is_well_formed("0.2.0"));
        assert!(version_is_well_formed("1.12.3-rc1"));
        assert!(!version_is_well_formed("0.2"));
        assert!(!version_is_well_formed("a.b.c"));
        assert!(!version_is_well_formed(""));
    }

    #[test]
    fn each_failure_is_reported_with_its_own_status_and_fix() {
        let mut f = healthy();
        f.exe = Ok(ExeFacts {
            path: "/usr/bin/emrac".into(),
            replaced: true,
            readable: true,
        });
        f.config_error = Some("bad toml at line 3".into());
        f.history = Ok(HistoryState {
            records: 5,
            problem: Some("line 4 was changed".into()),
        });
        f.journal = Journal::Interrupted("2 packages, started now".into());
        f.data_dir_problem = Some("read-only".into());
        f.helper.writable_by_others = true;
        let r = evaluate(&f);
        assert_eq!(status(&r, "exe"), Status::Warning);
        assert_eq!(status(&r, "config"), Status::Problem);
        assert_eq!(status(&r, "state"), Status::Problem);
        assert_eq!(status(&r, "journal"), Status::Warning);
        assert_eq!(status(&r, "data-dir"), Status::Problem);
        assert_eq!(status(&r, "helper"), Status::Problem);
        assert!(r.checks[0].status == Status::Problem, "problems come first");
        let j = r.checks.iter().find(|c| c.id == "journal").unwrap();
        assert_eq!(j.fix.as_deref(), Some("emrac history recover"));
        let h = r.checks.iter().find(|c| c.id == "helper").unwrap();
        assert!(h.fix.as_deref().unwrap().contains("chmod 755"));
    }

    #[test]
    fn a_missing_helper_is_a_note_and_an_unreadable_history_a_problem() {
        let mut f = healthy();
        f.helper.exists = false;
        f.history = Err("permission denied".into());
        let r = evaluate(&f);
        assert_eq!(status(&r, "helper"), Status::Note);
        assert_eq!(status(&r, "state"), Status::Problem);
    }
}

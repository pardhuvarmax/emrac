//! `emrac doctor`: read-only health checks. Everything the checks look at is
//! gathered into `DoctorInputs` (plain facts), and `evaluate` turns that into
//! a report — so every status decision is a pure function that can be tested
//! without touching the system. See `docs/plan/r1/u21-doctor.md`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    /// Worth knowing; nothing to fix.
    Note,
    Warning,
    Problem,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Note => "note",
            Status::Warning => "warning",
            Status::Problem => "PROBLEM",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub title: &'static str,
    pub status: Status,
    pub summary: String,
    /// Specifics (at most a handful of lines).
    pub detail: Vec<String>,
    /// What to run, when there is something to do.
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    /// Problems first, then warnings, notes and ok, each group in check order.
    pub checks: Vec<Check>,
}

impl DoctorReport {
    pub fn worst(&self) -> Status {
        self.checks
            .iter()
            .map(|c| c.status)
            .max()
            .unwrap_or(Status::Ok)
    }

    pub fn count(&self, status: Status) -> usize {
        self.checks.iter().filter(|c| c.status == status).count()
    }
}

/// How `config.toml` looks.
#[derive(Debug, Clone, Default)]
pub struct ConfigFacts {
    pub parse_error: Option<String>,
    pub world_writable: bool,
}

/// What `emrac history verify` found, for the doctor.
#[derive(Debug, Clone, Default)]
pub struct HistoryFacts {
    /// Where the chain breaks, worded for the user.
    pub problem: Option<String>,
    /// Lines from before the chain existed.
    pub predate: usize,
    /// Lines the chain covers.
    pub chained: usize,
    pub archives: usize,
}

pub use crate::ledger_watch::WatchFacts;

/// What the ledger looks like, for the doctor.
#[derive(Debug, Clone, Default)]
pub struct LedgerFacts {
    pub entries: usize,
    /// The ledger's own chain, worded; a problem if set.
    pub chain_problem: Option<String>,
    /// Where the user's history and the ledger differ.
    pub differences: Vec<String>,
    /// History lines the ledger lacks.
    pub pending: usize,
    pub root_owned: bool,
    pub world_writable: bool,
    /// `None`: this system can't tell.
    pub append_only: Option<bool>,
}

/// A tool emrac shells out to.
#[derive(Debug, Clone)]
pub struct ToolFact {
    pub name: &'static str,
    /// The package that provides it.
    pub package: &'static str,
    pub present: bool,
    /// What stops working without it.
    pub needed_for: &'static str,
    /// AUR builds can't happen without it.
    pub required: bool,
}

/// Free and total bytes of a filesystem.
#[derive(Debug, Clone)]
pub struct DiskFact {
    pub label: String,
    pub available: u64,
    pub total: u64,
}

/// Everything `evaluate` needs, gathered by the caller.
#[derive(Debug, Clone, Default)]
pub struct DoctorInputs {
    /// `(package, dependency)` pairs nothing installed satisfies.
    pub unsatisfied: Vec<(String, String)>,
    pub lock_present: bool,
    pub pacman_running: bool,
    /// Age of the newest sync database, in days; `None` if there are none.
    pub sync_db_age_days: Option<u64>,
    /// Packages `pacman -Su` would upgrade; `None` if that couldn't be asked.
    pub pending_upgrades: Option<usize>,
    /// `pending_upgrades` was computed against freshly synced repositories
    /// (`--online`), not against the possibly old local list.
    pub pending_fresh: bool,
    /// Why `--online` couldn't refresh.
    pub pending_error: Option<String>,
    pub pacnew_files: Vec<String>,
    pub failed_system_units: Vec<String>,
    pub failed_user_units: Vec<String>,
    pub kernel_release: Option<String>,
    pub kernel_modules_present: bool,
    pub disks: Vec<DiskFact>,
    pub pacman_cache_bytes: Option<u64>,
    pub emrac_cache_bytes: Option<u64>,
    pub orphan_count: usize,
    pub orphan_bytes: u64,
    pub expired_pins: Vec<String>,
    pub recent_failed_transactions: Vec<String>,
    /// `None`: there is no history yet.
    pub history: Option<HistoryFacts>,
    /// `None`: there is no ledger file.
    pub ledger: Option<LedgerFacts>,
    /// The history has records (so a missing ledger is worth a word).
    pub history_has_records: bool,
    /// A transaction that was cut off (u10): when it started and what it was.
    pub interrupted: Option<String>,
    /// Set when there is a ledger: what the audit watch saw.
    pub ledger_watch: Option<WatchFacts>,
    pub config: ConfigFacts,
    pub tools: Vec<ToolFact>,
    /// `Err` carries bubblewrap's reason.
    pub sandbox: Option<Result<(), String>>,
    pub scope_available: Option<bool>,
}

const GIB: u64 = 1 << 30;
const STALE_DB_DAYS: u64 = 7;
const BIG_PACMAN_CACHE: u64 = 10 * GIB;
const MAX_DETAIL: usize = 8;

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// "today", "yesterday", "N days ago"; "an unknown time ago" without a date.
fn age_phrase(days: Option<u64>) -> String {
    match days {
        None => "an unknown time ago".into(),
        Some(0) => "today".into(),
        Some(1) => "yesterday".into(),
        Some(d) => format!("{d} days ago"),
    }
}

/// `first` entries, then "and N more".
fn listed(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = items.iter().take(MAX_DETAIL).cloned().collect();
    if items.len() > MAX_DETAIL {
        out.push(format!("and {} more", items.len() - MAX_DETAIL));
    }
    out
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Unit names from `systemctl --failed --no-legend --plain` output (the first
/// column; a leading `●` marker is dropped).
pub fn parse_failed_units(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|l| l.trim().trim_start_matches(['●', '*']).trim())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|u| u.contains('.'))
        .map(str::to_string)
        .collect()
}

/// `(available, total)` bytes from `df -Pk <path>` output.
pub fn parse_df(output: &str) -> Option<(u64, u64)> {
    let line = output.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = cols.get(1)?.parse().ok()?;
    let available: u64 = cols.get(3)?.parse().ok()?;
    Some((available * 1024, total * 1024))
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

impl Check {
    fn detail(mut self, detail: Vec<String>) -> Self {
        self.detail = detail;
        self
    }

    fn fix(mut self, fix: &str) -> Self {
        self.fix = Some(fix.to_string());
        self
    }
}

pub fn evaluate(i: &DoctorInputs) -> DoctorReport {
    let mut checks: Vec<Check> = Vec::new();

    // Dependencies.
    checks.push(if i.unsatisfied.is_empty() {
        check(
            "dependencies",
            "Dependencies",
            Status::Ok,
            "every installed package's dependencies are satisfied",
        )
    } else {
        let lines: Vec<String> = i
            .unsatisfied
            .iter()
            .map(|(p, d)| format!("{p} needs {d}"))
            .collect();
        check(
            "dependencies",
            "Dependencies",
            Status::Problem,
            format!(
                "{} — typical after a partial upgrade or a forced removal",
                plural(
                    i.unsatisfied.len(),
                    "unmet dependency",
                    "unmet dependencies"
                )
            ),
        )
        .detail(listed(&lines))
        .fix("emrac upgrade, then emrac reinstall the packages named above")
    });

    // Lock file.
    checks.push(match (i.lock_present, i.pacman_running) {
        (false, _) => check("lock", "Package database lock", Status::Ok, "no lock file"),
        (true, true) => check(
            "lock",
            "Package database lock",
            Status::Note,
            "pacman is running and holds the lock",
        ),
        (true, false) => check(
            "lock",
            "Package database lock",
            Status::Problem,
            "a lock file exists but no pacman is running — a crashed run left it behind, and nothing can install until it is removed",
        )
        .fix("sudo rm /var/lib/pacman/db.lck"),
    });

    // Sync databases.
    checks.push(match i.sync_db_age_days {
        None => check(
            "sync-db",
            "Sync databases",
            Status::Warning,
            "no sync databases found",
        )
        .fix("sudo pacman -Sy"),
        Some(days) if days > STALE_DB_DAYS => check(
            "sync-db",
            "Sync databases",
            Status::Warning,
            format!(
                "last refreshed {days} days ago — installing from a stale database is how partial upgrades start"
            ),
        )
        .fix("emrac upgrade (it refreshes the databases and upgrades everything together)"),
        Some(0) => check(
            "sync-db",
            "Sync databases",
            Status::Ok,
            "refreshed today",
        ),
        Some(days) => check(
            "sync-db",
            "Sync databases",
            Status::Ok,
            format!("refreshed {} ago", plural(days as usize, "day", "days")),
        ),
    });

    // Pending upgrades. Only ever as good as the package list it was computed
    // from: against an old local list it is never reported as "up to date"
    // (that once hid 347 pending updates); `--online` checks against freshly
    // synced repositories instead.
    const REFRESH: &str = "emrac doctor --online checks against freshly synced repositories; emrac upgrade previews against them, then refreshes and upgrades";
    if let Some(why) = &i.pending_error {
        checks.push(
            check(
                "upgrades",
                "Pending upgrades",
                Status::Note,
                format!("couldn't check against fresh repositories: {why}"),
            )
            .fix(REFRESH),
        );
    } else if let Some(n) = i.pending_upgrades {
        let fresh = i.pending_fresh || i.sync_db_age_days.is_some_and(|d| d <= 1);
        let as_of = if i.pending_fresh {
            "just now".to_string()
        } else {
            age_phrase(i.sync_db_age_days)
        };
        let count = plural(n, "package can be upgraded", "packages can be upgraded");
        checks.push(match (n, fresh, i.pending_fresh) {
            (0, true, true) => check(
                "upgrades",
                "Pending upgrades",
                Status::Ok,
                "none, checked against freshly synced repositories",
            ),
            (0, true, false) => check(
                "upgrades",
                "Pending upgrades",
                Status::Ok,
                format!("none, according to the database refreshed {as_of}"),
            ),
            (0, false, _) => check(
                "upgrades",
                "Pending upgrades",
                Status::Note,
                format!(
                    "can't tell — the package list was refreshed {as_of}, so \"nothing to upgrade\" only describes the repositories as they were then"
                ),
            )
            .fix(REFRESH),
            (_, true, true) => check(
                "upgrades",
                "Pending upgrades",
                Status::Note,
                format!("{count} (checked against freshly synced repositories)"),
            )
            .fix("emrac upgrade"),
            (_, true, false) => check(
                "upgrades",
                "Pending upgrades",
                Status::Note,
                format!("{count} (database refreshed {as_of})"),
            )
            .fix(REFRESH),
            (_, false, _) => check(
                "upgrades",
                "Pending upgrades",
                Status::Note,
                format!(
                    "{count} according to a package list refreshed {as_of}; there are probably more"
                ),
            )
            .fix(REFRESH),
        });
    }

    // Config files left by upgrades.
    checks.push(if i.pacnew_files.is_empty() {
        check(
            "pacnew",
            ".pacnew / .pacsave files",
            Status::Ok,
            "none left to merge",
        )
    } else {
        check(
            "pacnew",
            ".pacnew / .pacsave files",
            Status::Warning,
            format!(
                "{} waiting to be merged or removed",
                plural(i.pacnew_files.len(), "file is", "files are")
            ),
        )
        .detail(listed(&i.pacnew_files))
        .fix("`emrac pacnew` shows which ones hold your own edits and offers to keep, take or merge each")
    });

    // Failed units.
    let failed: Vec<String> = i
        .failed_system_units
        .iter()
        .map(|u| format!("{u} (system)"))
        .chain(i.failed_user_units.iter().map(|u| format!("{u} (user)")))
        .collect();
    checks.push(if failed.is_empty() {
        check("units", "systemd units", Status::Ok, "no failed units")
    } else {
        check(
            "units",
            "systemd units",
            Status::Warning,
            plural(failed.len(), "unit has failed", "units have failed"),
        )
        .detail(listed(&failed))
        .fix("systemctl status <unit> (or journalctl -u <unit>) to see why")
    });

    // Running kernel.
    if let Some(release) = &i.kernel_release {
        checks.push(if i.kernel_modules_present {
            check(
                "kernel",
                "Running kernel",
                Status::Ok,
                format!("{release}; its modules are installed"),
            )
        } else {
            check(
                "kernel",
                "Running kernel",
                Status::Warning,
                format!(
                    "{release} is running but /usr/lib/modules/{release} is gone — the kernel was upgraded under you, so new modules (USB storage, filesystems, …) can't load until you reboot"
                ),
            )
            .fix("reboot")
        });
    }

    // Disk space.
    for disk in &i.disks {
        let pct_free = disk
            .available
            .saturating_mul(100)
            .checked_div(disk.total)
            .unwrap_or(100);
        let (status, hint) = if disk.available < GIB {
            (Status::Problem, true)
        } else if disk.available < 5 * GIB || pct_free < 10 {
            (Status::Warning, true)
        } else {
            (Status::Ok, false)
        };
        let mut c = check(
            "disk",
            "Disk space",
            status,
            format!(
                "{}: {} free of {} ({pct_free}%)",
                disk.label,
                human(disk.available),
                human(disk.total)
            ),
        );
        if hint {
            c = c.fix("free some space: sudo paccache -r (pacman-contrib), emrac orphans, or clear ~/.cache");
        }
        checks.push(c);
    }

    // Caches.
    if let Some(bytes) = i.pacman_cache_bytes {
        checks.push(if bytes > BIG_PACMAN_CACHE {
            check(
                "cache",
                "Package cache",
                Status::Warning,
                format!("pacman's cache is {}", human(bytes)),
            )
            .fix("sudo paccache -r keeps the last 3 versions of each package")
        } else {
            check(
                "cache",
                "Package cache",
                Status::Ok,
                format!("pacman's cache is {}", human(bytes)),
            )
        });
    }
    if let Some(bytes) = i.emrac_cache_bytes {
        checks.push(check(
            "emrac-cache",
            "emrac caches",
            Status::Note,
            format!(
                "AUR checkouts, built packages and downloaded archive files use {}",
                human(bytes)
            ),
        ));
    }

    // Orphans.
    checks.push(if i.orphan_count == 0 {
        check("orphans", "Orphans", Status::Ok, "no orphaned dependencies")
    } else {
        check(
            "orphans",
            "Orphans",
            Status::Note,
            format!(
                "{} that nothing needs ({})",
                plural(i.orphan_count, "dependency", "dependencies"),
                human(i.orphan_bytes)
            ),
        )
        .fix("emrac orphans, then emrac remove what you don't want")
    });

    // Pins.
    if !i.expired_pins.is_empty() {
        checks.push(
            check(
                "pins",
                "Pins",
                Status::Warning,
                format!(
                    "{} expired and no longer hold anything back",
                    plural(i.expired_pins.len(), "pin has", "pins have")
                ),
            )
            .detail(listed(&i.expired_pins))
            .fix("emrac pin <pkg> --until … to renew, or emrac unpin <pkg>"),
        );
    }

    // Recent failures.
    if !i.recent_failed_transactions.is_empty() {
        checks.push(
            check(
                "history",
                "Recent transactions",
                Status::Note,
                format!(
                    "{} failed in the last 7 days",
                    plural(
                        i.recent_failed_transactions.len(),
                        "transaction",
                        "transactions"
                    )
                ),
            )
            .detail(listed(&i.recent_failed_transactions))
            .fix("emrac history"),
        );
    }

    // The history's hash chain.
    if let Some(h) = &i.history {
        let mut c = match &h.problem {
            Some(problem) => check(
                "history-integrity",
                "History integrity",
                Status::Warning,
                "the transaction history does not verify — something in it changed outside emrac"
                    .to_string(),
            )
            .detail(vec![problem.clone()])
            .fix("emrac history verify"),
            None => check(
                "history-integrity",
                "History integrity",
                Status::Ok,
                if h.chained == 0 {
                    "nothing is chained yet; the next transaction starts the chain".to_string()
                } else {
                    format!(
                        "{} chained, nothing out of place",
                        plural(h.chained, "record", "records")
                    )
                },
            ),
        };
        let mut notes = Vec::new();
        if h.predate > 0 {
            notes.push(format!(
                "{} earlier records predate the chain and are not covered by it",
                h.predate
            ));
        }
        if h.archives > 0 {
            notes.push(format!("{} archive file(s) checked", h.archives));
        }
        if !notes.is_empty() && h.problem.is_none() {
            c = c.detail(notes);
        }
        checks.push(c);
    }

    // A transaction that was cut off.
    if let Some(what) = &i.interrupted {
        checks.push(
            check(
                "interrupted",
                "Interrupted transaction",
                Status::Warning,
                format!("a transaction was cut off before it was recorded: {what}"),
            )
            .fix("emrac history recover"),
        );
    }

    // The ledger.
    match &i.ledger {
        None if i.history_has_records => checks.push(
            check(
                "ledger",
                "Ledger",
                Status::Note,
                "no ledger yet: nothing outside your home holds a second copy of the history"
                    .to_string(),
            )
            .fix("emrac history sync"),
        ),
        None => {}
        Some(l) => {
            let mut notes = Vec::new();
            let (status, summary) = if let Some(p) = &l.chain_problem {
                notes.push(p.clone());
                (
                    Status::Problem,
                    "the ledger's own chain is broken — it was changed after it was written"
                        .to_string(),
                )
            } else if !l.differences.is_empty() {
                notes.extend(l.differences.iter().cloned());
                (
                    Status::Warning,
                    "your history and the ledger disagree".to_string(),
                )
            } else if !l.root_owned || l.world_writable {
                (
                    Status::Warning,
                    "the ledger is not root-owned or is writable by others, so it protects nothing"
                        .to_string(),
                )
            } else if l.append_only != Some(true) {
                (
                    Status::Note,
                    "root-owned, but not append-only here (this filesystem has no such flag)"
                        .to_string(),
                )
            } else if l.pending > 0 {
                (
                    Status::Note,
                    format!(
                        "{} not in the ledger yet",
                        plural(l.pending, "history line is", "history lines are")
                    ),
                )
            } else {
                (
                    Status::Ok,
                    format!(
                        "{}, root-owned and append-only, matching your history",
                        plural(l.entries, "entry", "entries")
                    ),
                )
            };
            let mut c = check("ledger", "Ledger", status, summary).detail(notes);
            if l.chain_problem.is_some() {
                // --repair rebuilds the history *from* the ledger, so it cannot help here.
                c = c.fix("restore the ledger by hand as root: docs/design/ledger.md, 'Restoring a tampered ledger'");
            } else if !l.differences.is_empty() {
                c = c.fix("emrac history verify --repair");
            } else if l.pending > 0 {
                c = c.fix("emrac history sync");
            }
            checks.push(c);
        }
    }

    // The audit watch on the ledger.
    if let Some(w) = &i.ledger_watch {
        let c = if !w.foreign.is_empty() {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Warning,
                format!(
                    "{} touched the ledger that {} not emrac's own appends",
                    plural(w.foreign.len(), "event", "events"),
                    if w.foreign.len() == 1 { "was" } else { "were" }
                ),
            )
            .detail(w.foreign.clone())
            .fix("emrac ledger events")
        } else if !w.unwitnessed.is_empty() {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Warning,
                "emrac's program wrote to the ledger without leaving its journal line".to_string(),
            )
            .detail(w.unwitnessed.clone())
            .fix("emrac ledger events")
        } else if !w.rules_installed {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Note,
                "not set up: nothing records who else touches the ledger".to_string(),
            )
            .fix("sudo emrac ledger watch install")
        } else if let Some(why) = &w.unreadable {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Note,
                "the audit events can't be read from here".to_string(),
            )
            .detail(vec![why.clone()])
            .fix("sudo emrac ledger events")
        } else if w.events == 0 && w.witnesses_after_rules > 0 {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Warning,
                format!(
                    "not active: emrac appended to the ledger {} but the audit system saw nothing",
                    plural(w.witnesses_after_rules, "time", "times")
                ),
            )
            .detail(vec![
                "auditd may be stopped, the rules not loaded, or the kernel started without audit=1".to_string(),
            ])
            .fix("systemctl status auditd audit-rules; sudo auditctl -l (should list emrac-ledger)")
        } else {
            check(
                "ledger-watch",
                "Ledger watch",
                Status::Ok,
                format!(
                    "{} in the last 30 days, all emrac's own",
                    plural(w.events, "audit event", "audit events")
                ),
            )
        };
        checks.push(c);
    }

    // emrac's own config.
    checks.push(match (&i.config.parse_error, i.config.world_writable) {
        (Some(err), _) => check(
            "config",
            "emrac config",
            Status::Problem,
            format!("config.toml doesn't parse: {err}"),
        )
        .fix("emrac config edit"),
        (None, true) => check(
            "config",
            "emrac config",
            Status::Warning,
            "config.toml is writable by everyone",
        )
        .fix("chmod 600 ~/.config/emrac/config.toml"),
        (None, false) => check("config", "emrac config", Status::Ok, "config.toml is fine"),
    });

    // Tools.
    let missing: Vec<&ToolFact> = i.tools.iter().filter(|t| !t.present).collect();
    checks.push(if missing.is_empty() {
        check(
            "tools",
            "Tools",
            Status::Ok,
            "makepkg, git, bsdtar, fakeroot and sudo are all present",
        )
    } else {
        let status = if missing.iter().any(|t| t.required) {
            Status::Problem
        } else {
            Status::Warning
        };
        check(
            "tools",
            "Tools",
            status,
            format!("{} missing", plural(missing.len(), "tool is", "tools are")),
        )
        .detail(
            missing
                .iter()
                .map(|t| format!("{} — needed for {}", t.name, t.needed_for))
                .collect(),
        )
        .fix(&{
            let mut pkgs: Vec<&str> = missing.iter().map(|t| t.package).collect();
            pkgs.sort();
            pkgs.dedup();
            format!("sudo pacman -S {}", pkgs.join(" "))
        })
    });

    // Build sandbox.
    match &i.sandbox {
        Some(Ok(())) => {
            let limits = match i.scope_available {
                Some(true) => "; memory and process limits are enforced",
                Some(false) => {
                    "; memory and process limits aren't available (no systemd user session) — time and size limits only"
                }
                None => "",
            };
            checks.push(check(
                "sandbox",
                "Build sandbox",
                Status::Ok,
                format!("bubblewrap works{limits}"),
            ));
        }
        Some(Err(why)) => checks.push(
            check(
                "sandbox",
                "Build sandbox",
                Status::Warning,
                format!("AUR builds will be refused: {why}"),
            )
            .fix("sudo pacman -S bubblewrap, or build with --no-sandbox"),
        ),
        None => {}
    }

    // Problems first, then the rest; each group keeps check order.
    let mut indexed: Vec<(usize, Check)> = checks.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| b.1.status.cmp(&a.1.status).then(a.0.cmp(&b.0)));
    DoctorReport {
        checks: indexed.into_iter().map(|(_, c)| c).collect(),
    }
}

/// What to scrub from a pasted report.
pub struct Scrub<'a> {
    pub home: &'a str,
    pub user: &'a str,
    pub host: &'a str,
}

/// Replaces the home directory, user name and host name, so a report can be
/// shared. Longest first (the home path contains the user name).
pub fn scrub(text: &str, s: &Scrub) -> String {
    let mut out = text.to_string();
    if s.home.len() > 1 {
        out = out.replace(s.home, "~");
    }
    for (needle, with) in [(s.host, "<host>"), (s.user, "<user>")] {
        if needle.len() > 2 {
            out = out.replace(needle, with);
        }
    }
    out
}

/// The facts printed at the top of a report.
pub struct ReportEnv {
    pub emrac_version: String,
    pub pacman_version: String,
    pub kernel: String,
    pub installed: usize,
    pub foreign: usize,
}

/// A markdown block for a bug report; `scrub` it before sharing.
pub fn report_markdown(report: &DoctorReport, env: &ReportEnv) -> String {
    let mut out = String::new();
    out.push_str("### emrac doctor report\n\n");
    out.push_str(&format!(
        "- emrac {}\n- {}\n- kernel {}\n- {} installed packages, {} foreign\n\n",
        env.emrac_version, env.pacman_version, env.kernel, env.installed, env.foreign
    ));
    for c in &report.checks {
        out.push_str(&format!(
            "- **{}** — {}: {}\n",
            c.status.label(),
            c.title,
            c.summary
        ));
        for d in &c.detail {
            out.push_str(&format!("  - {d}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(r: &'a DoctorReport, id: &str) -> Vec<&'a Check> {
        r.checks.iter().filter(|c| c.id == id).collect()
    }

    fn one<'a>(r: &'a DoctorReport, id: &str) -> &'a Check {
        let v = find(r, id);
        assert_eq!(v.len(), 1, "{id}: {v:?}");
        v[0]
    }

    /// A healthy machine.
    fn healthy() -> DoctorInputs {
        DoctorInputs {
            sync_db_age_days: Some(0),
            pending_upgrades: Some(0),
            kernel_release: Some("6.1.0-arch1".into()),
            kernel_modules_present: true,
            disks: vec![DiskFact {
                label: "/".into(),
                available: 200 * GIB,
                total: 500 * GIB,
            }],
            pacman_cache_bytes: Some(2 * GIB),
            tools: vec![ToolFact {
                name: "makepkg",
                package: "base-devel",
                present: true,
                needed_for: "AUR builds",
                required: true,
            }],
            sandbox: Some(Ok(())),
            scope_available: Some(true),
            ..DoctorInputs::default()
        }
    }

    #[test]
    fn a_healthy_machine_has_nothing_above_a_note() {
        let r = evaluate(&healthy());
        assert_eq!(r.worst(), Status::Ok, "{:#?}", r.checks);
        assert_eq!(r.count(Status::Problem) + r.count(Status::Warning), 0);
    }

    #[test]
    fn unmet_dependencies_are_a_problem_with_the_pairs_listed_and_capped() {
        let mut i = healthy();
        i.unsatisfied = (0..12)
            .map(|n| (format!("pkg{n}"), "libfoo.so=2".into()))
            .collect();
        let r = evaluate(&i);
        let c = one(&r, "dependencies");
        assert_eq!(c.status, Status::Problem);
        assert!(
            c.summary.starts_with("12 unmet dependencies —"),
            "{}",
            c.summary
        );
        assert_eq!(
            c.detail.len(),
            MAX_DETAIL + 1,
            "capped, plus the 'and N more' line"
        );
        assert_eq!(c.detail[0], "pkg0 needs libfoo.so=2");
        assert_eq!(c.detail.last().unwrap(), "and 4 more");
        assert!(c.fix.is_some());
        assert_eq!(r.checks[0].id, "dependencies", "problems sort first");
    }

    #[test]
    fn a_lock_is_only_stale_when_no_pacman_is_running() {
        let mut i = healthy();
        i.lock_present = true;
        i.pacman_running = false;
        let r = evaluate(&i);
        assert_eq!(one(&r, "lock").status, Status::Problem);
        assert!(one(&r, "lock").fix.as_deref().unwrap().contains("db.lck"));
        i.pacman_running = true;
        assert_eq!(one(&evaluate(&i), "lock").status, Status::Note);
        i.lock_present = false;
        assert_eq!(one(&evaluate(&i), "lock").status, Status::Ok);
    }

    #[test]
    fn databases_older_than_a_week_warn() {
        let mut i = healthy();
        for (days, status) in [
            (0, Status::Ok),
            (3, Status::Ok),
            (7, Status::Ok),
            (8, Status::Warning),
            (40, Status::Warning),
        ] {
            i.sync_db_age_days = Some(days);
            assert_eq!(one(&evaluate(&i), "sync-db").status, status, "{days} days");
        }
        i.sync_db_age_days = None;
        assert_eq!(one(&evaluate(&i), "sync-db").status, Status::Warning);
    }

    #[test]
    fn pending_upgrades_are_a_note_and_unknown_is_skipped() {
        let mut i = healthy();
        i.pending_upgrades = Some(14);
        let r = evaluate(&i);
        assert_eq!(one(&r, "upgrades").status, Status::Note);
        assert!(one(&r, "upgrades").summary.starts_with("14 packages"));
        i.pending_upgrades = Some(1);
        assert!(one(&evaluate(&i), "upgrades")
            .summary
            .starts_with("1 package can"));
        i.pending_upgrades = None;
        assert!(find(&evaluate(&i), "upgrades").is_empty());
    }

    #[test]
    fn nothing_to_upgrade_is_only_reassuring_when_the_package_list_is_fresh() {
        let mut i = healthy();
        i.pending_upgrades = Some(0);
        for days in [0, 1] {
            i.sync_db_age_days = Some(days);
            let c = one(&evaluate(&i), "upgrades").clone();
            assert_eq!(c.status, Status::Ok, "{days} days");
            assert!(c.summary.starts_with("none, according to"), "{}", c.summary);
        }
        // A stale list says nothing about what is available now: never "ok",
        // never the words "up to date", and it says how old the list is.
        for days in [2, 10, 400] {
            i.sync_db_age_days = Some(days);
            let c = one(&evaluate(&i), "upgrades").clone();
            assert_eq!(c.status, Status::Note, "{days} days");
            assert!(
                c.summary.contains(&format!("{days} days ago")),
                "{}",
                c.summary
            );
            assert!(c.summary.starts_with("can't tell"), "{}", c.summary);
            assert!(!c.summary.contains("up to date"), "{}", c.summary);
            assert!(c.fix.as_deref().unwrap().contains("doctor --online"));
        }
        // No database date at all is stale too.
        i.sync_db_age_days = None;
        let c = one(&evaluate(&i), "upgrades").clone();
        assert_eq!(c.status, Status::Note);
        assert!(c.summary.contains("an unknown time ago"));
    }

    #[test]
    fn a_stale_list_with_upgrades_says_there_are_probably_more() {
        let mut i = healthy();
        i.pending_upgrades = Some(3);
        i.sync_db_age_days = Some(10);
        let c = one(&evaluate(&i), "upgrades").clone();
        assert!(
            c.summary.starts_with(
                "3 packages can be upgraded according to a package list refreshed 10 days ago"
            ),
            "{}",
            c.summary
        );
        assert!(c.summary.contains("probably more"));
        i.sync_db_age_days = Some(0);
        let c = one(&evaluate(&i), "upgrades").clone();
        assert!(
            c.summary.contains("database refreshed today"),
            "{}",
            c.summary
        );
        assert!(!c.summary.contains("probably more"));
    }

    #[test]
    fn online_results_are_trusted_regardless_of_the_local_lists_age() {
        let mut i = healthy();
        i.sync_db_age_days = Some(30);
        i.pending_fresh = true;
        i.pending_upgrades = Some(0);
        let c = one(&evaluate(&i), "upgrades").clone();
        assert_eq!(c.status, Status::Ok);
        assert_eq!(
            c.summary,
            "none, checked against freshly synced repositories"
        );
        i.pending_upgrades = Some(347);
        let c = one(&evaluate(&i), "upgrades").clone();
        assert_eq!(c.status, Status::Note);
        assert!(
            c.summary
                .starts_with("347 packages can be upgraded (checked against freshly"),
            "{}",
            c.summary
        );
        assert!(!c.summary.contains("probably more"));
        // The local list is still flagged as old by its own check.
        assert_eq!(one(&evaluate(&i), "sync-db").status, Status::Warning);
    }

    #[test]
    fn a_failed_online_check_says_so_instead_of_guessing() {
        let mut i = healthy();
        i.pending_error = Some("Could not resolve host".into());
        i.pending_upgrades = Some(0);
        let c = one(&evaluate(&i), "upgrades").clone();
        assert_eq!(c.status, Status::Note);
        assert!(
            c.summary
                .contains("couldn't check against fresh repositories: Could not resolve host"),
            "{}",
            c.summary
        );
        assert!(!c.summary.contains("none") && !c.summary.contains("up to date"));
    }

    #[test]
    fn no_report_claims_up_to_date_from_an_old_database() {
        // The whole report, not one check: nothing anywhere may say
        // "up to date" when the package list is old.
        let mut i = healthy();
        i.pending_upgrades = Some(0);
        i.sync_db_age_days = Some(11);
        let r = evaluate(&i);
        for c in &r.checks {
            assert!(!c.summary.contains("up to date"), "{}: {}", c.id, c.summary);
        }
        assert_eq!(
            one(&r, "sync-db").status,
            Status::Warning,
            "and the age is flagged"
        );
    }

    #[test]
    fn leftover_pacnew_files_and_failed_units_warn() {
        let mut i = healthy();
        i.pacnew_files = vec!["/etc/pacman.conf.pacnew".into()];
        i.failed_system_units = vec!["foo.service".into()];
        i.failed_user_units = vec!["bar.timer".into()];
        let r = evaluate(&i);
        assert_eq!(one(&r, "pacnew").status, Status::Warning);
        let units = one(&r, "units");
        assert_eq!(units.status, Status::Warning);
        assert_eq!(units.detail, ["foo.service (system)", "bar.timer (user)"]);
    }

    #[test]
    fn a_kernel_whose_modules_are_gone_says_reboot() {
        let mut i = healthy();
        i.kernel_modules_present = false;
        let c = &evaluate(&i).checks[0];
        assert_eq!(c.id, "kernel");
        assert_eq!(c.status, Status::Warning);
        assert_eq!(c.fix.as_deref(), Some("reboot"));
        assert!(c.summary.contains("6.1.0-arch1"));
        i.kernel_release = None;
        assert!(find(&evaluate(&i), "kernel").is_empty());
    }

    #[test]
    fn disk_space_thresholds() {
        let mut i = healthy();
        let case = |avail: u64, total: u64| DiskFact {
            label: "/".into(),
            available: avail,
            total,
        };
        for (disk, status) in [
            (case(200 * GIB, 500 * GIB), Status::Ok),
            (case(4 * GIB, 500 * GIB), Status::Warning),
            (case(40 * GIB, 500 * GIB), Status::Warning), // under 10 %
            (case(900 * 1024 * 1024, 500 * GIB), Status::Problem),
            (case(20 * GIB, 30 * GIB), Status::Ok),
        ] {
            i.disks = vec![disk.clone()];
            assert_eq!(
                one(&evaluate(&i), "disk").status,
                status,
                "{} free of {}",
                human(disk.available),
                human(disk.total)
            );
        }
        i.disks = vec![case(200 * GIB, 500 * GIB), case(2 * GIB, 100 * GIB)];
        assert_eq!(
            find(&evaluate(&i), "disk").len(),
            2,
            "one check per filesystem"
        );
    }

    #[test]
    fn caches_orphans_pins_history_are_reported_when_there_is_something_to_say() {
        let mut i = healthy();
        i.pacman_cache_bytes = Some(25 * GIB);
        i.emrac_cache_bytes = Some(300 * 1024 * 1024);
        i.orphan_count = 3;
        i.orphan_bytes = 5 * 1024 * 1024;
        i.expired_pins = vec!["linux".into()];
        i.recent_failed_transactions = vec!["tx-9".into()];
        let r = evaluate(&i);
        assert_eq!(one(&r, "cache").status, Status::Warning);
        assert_eq!(one(&r, "emrac-cache").status, Status::Note);
        assert_eq!(one(&r, "orphans").status, Status::Note);
        assert_eq!(one(&r, "pins").status, Status::Warning);
        assert_eq!(one(&r, "history").status, Status::Note);
        let h = healthy();
        let r = evaluate(&h);
        assert!(find(&r, "pins").is_empty() && find(&r, "history").is_empty());
    }

    #[test]
    fn config_problems() {
        let mut i = healthy();
        i.config.world_writable = true;
        assert_eq!(one(&evaluate(&i), "config").status, Status::Warning);
        i.config.parse_error = Some("expected `=`".into());
        let c = one(&evaluate(&i), "config").clone();
        assert_eq!(c.status, Status::Problem);
        assert!(c.summary.contains("expected `=`"));
    }

    #[test]
    fn missing_tools_are_a_problem_only_when_one_is_required() {
        let mut i = healthy();
        i.tools.push(ToolFact {
            name: "pacdiff",
            package: "pacman-contrib",
            present: false,
            needed_for: "merging .pacnew files",
            required: false,
        });
        let c = one(&evaluate(&i), "tools").clone();
        assert_eq!(c.status, Status::Warning);
        assert_eq!(c.detail, ["pacdiff — needed for merging .pacnew files"]);
        assert_eq!(
            c.fix.as_deref(),
            Some("sudo pacman -S pacman-contrib"),
            "its own package"
        );
        i.tools.push(ToolFact {
            name: "bsdtar",
            package: "libarchive",
            present: false,
            needed_for: "inspecting built packages",
            required: true,
        });
        let c = one(&evaluate(&i), "tools").clone();
        assert_eq!(c.status, Status::Problem);
        assert_eq!(
            c.fix.as_deref(),
            Some("sudo pacman -S libarchive pacman-contrib")
        );
    }

    #[test]
    fn the_sandbox_check_reflects_bwrap_and_the_limits() {
        let mut i = healthy();
        let ok = one(&evaluate(&i), "sandbox").clone();
        assert_eq!(ok.status, Status::Ok);
        assert!(ok.summary.contains("limits are enforced"));
        i.scope_available = Some(false);
        assert!(one(&evaluate(&i), "sandbox")
            .summary
            .contains("time and size limits only"));
        i.sandbox = Some(Err("Can't mount proc on /proc".into()));
        let bad = one(&evaluate(&i), "sandbox").clone();
        assert_eq!(bad.status, Status::Warning);
        assert!(bad.summary.contains("Can't mount proc"));
        assert!(bad.fix.unwrap().contains("--no-sandbox"));
    }

    #[test]
    fn problems_sort_before_warnings_before_notes_and_keep_check_order_within() {
        let mut i = healthy();
        i.lock_present = true; // problem
        i.pacnew_files = vec!["/etc/x.pacnew".into()]; // warning
        i.orphan_count = 1; // note
        i.pending_upgrades = Some(2); // note
        let order: Vec<_> = evaluate(&i)
            .checks
            .iter()
            .map(|c| (c.status, c.id))
            .collect();
        let statuses: Vec<_> = order.iter().map(|(s, _)| *s).collect();
        let mut sorted = statuses.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(statuses, sorted, "{order:?}");
        let notes: Vec<_> = order
            .iter()
            .filter(|(s, _)| *s == Status::Note)
            .map(|(_, id)| *id)
            .collect();
        assert_eq!(
            notes,
            ["upgrades", "orphans"],
            "check order kept within a group"
        );
    }

    #[test]
    fn systemctl_and_df_output_are_parsed() {
        let units = "● foo.service loaded failed failed Foo daemon\n  bar.timer   loaded failed failed Bar\n\n";
        assert_eq!(parse_failed_units(units), ["foo.service", "bar.timer"]);
        assert!(parse_failed_units("").is_empty());
        assert!(parse_failed_units("0 loaded units listed.\n").is_empty());

        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 400 600 40% /\n";
        assert_eq!(parse_df(df), Some((600 * 1024, 1000 * 1024)));
        assert_eq!(parse_df("garbage"), None);
    }

    #[test]
    fn scrubbing_removes_the_home_user_and_host_names() {
        let s = Scrub {
            home: "/home/emracus",
            user: "emracus",
            host: "archbox",
        };
        let t = "cache /home/emracus/.cache/emrac on archbox for emracus";
        let out = scrub(t, &s);
        assert_eq!(out, "cache ~/.cache/emrac on <host> for <user>");
        assert!(!out.contains("emracus") && !out.contains("archbox"));
        // Too-short names are left alone rather than mangling the text.
        let short = Scrub {
            home: "/",
            user: "al",
            host: "",
        };
        assert_eq!(scrub("all of /usr", &short), "all of /usr");
    }

    #[test]
    fn the_report_is_markdown_with_every_check_and_its_detail() {
        let mut i = healthy();
        i.pacnew_files = vec!["/etc/a.pacnew".into(), "/etc/b.pacnew".into()];
        let report = evaluate(&i);
        let env = ReportEnv {
            emrac_version: "0.1.0".into(),
            pacman_version: "Pacman v7.1.0".into(),
            kernel: "6.1.0-arch1".into(),
            installed: 1635,
            foreign: 12,
        };
        let md = report_markdown(&report, &env);
        assert!(md.starts_with("### emrac doctor report"));
        assert!(md.contains("emrac 0.1.0") && md.contains("1635 installed packages, 12 foreign"));
        assert!(md.contains("**warning** — .pacnew / .pacsave files:"));
        assert!(md.contains("  - /etc/a.pacnew"));
        assert_eq!(md.matches("\n- **").count(), report.checks.len());
    }

    #[test]
    fn history_integrity_is_ok_warns_on_a_break_and_is_absent_without_history() {
        assert!(find(&evaluate(&healthy()), "history-integrity").is_empty());

        let mut i = healthy();
        i.history = Some(HistoryFacts {
            chained: 12,
            predate: 3,
            ..HistoryFacts::default()
        });
        let r = evaluate(&i);
        let c = one(&r, "history-integrity");
        assert_eq!(c.status, Status::Ok);
        assert!(c.summary.contains("12 records"), "{}", c.summary);
        assert!(c
            .detail
            .iter()
            .any(|d| d.contains("3 earlier records predate")));

        i.history = Some(HistoryFacts {
            problem: Some("history.jsonl line 4: edited".into()),
            chained: 2,
            ..HistoryFacts::default()
        });
        let c = one(&evaluate(&i), "history-integrity").clone();
        assert_eq!(c.status, Status::Warning);
        assert_eq!(c.fix.as_deref(), Some("emrac history verify"));
        assert!(c.detail[0].contains("line 4"));
    }

    #[test]
    fn the_ledger_check_reports_each_state() {
        let mut i = healthy();
        assert!(
            find(&evaluate(&i), "ledger").is_empty(),
            "no history, no ledger: no word"
        );
        i.history_has_records = true;
        assert_eq!(one(&evaluate(&i), "ledger").status, Status::Note);

        let good = LedgerFacts {
            entries: 9,
            root_owned: true,
            append_only: Some(true),
            ..LedgerFacts::default()
        };
        i.ledger = Some(good.clone());
        let c = one(&evaluate(&i), "ledger").clone();
        assert_eq!(c.status, Status::Ok);
        assert!(c.summary.contains("9 entries"), "{}", c.summary);

        i.ledger = Some(LedgerFacts {
            append_only: Some(false),
            ..good.clone()
        });
        assert_eq!(one(&evaluate(&i), "ledger").status, Status::Note);
        i.ledger = Some(LedgerFacts {
            pending: 2,
            ..good.clone()
        });
        let c = one(&evaluate(&i), "ledger").clone();
        assert_eq!(
            (c.status, c.fix.as_deref()),
            (Status::Note, Some("emrac history sync"))
        );
        i.ledger = Some(LedgerFacts {
            root_owned: false,
            ..good.clone()
        });
        assert_eq!(one(&evaluate(&i), "ledger").status, Status::Warning);
        i.ledger = Some(LedgerFacts {
            differences: vec!["changed since recorded: tx-2".into()],
            ..good.clone()
        });
        let c = one(&evaluate(&i), "ledger").clone();
        assert_eq!(c.status, Status::Warning);
        assert!(c.detail[0].contains("tx-2"));
        i.ledger = Some(LedgerFacts {
            chain_problem: Some("ledger.jsonl line 3".into()),
            ..good
        });
        assert_eq!(one(&evaluate(&i), "ledger").status, Status::Problem);
    }

    #[test]
    fn a_cut_off_transaction_is_a_warning_with_the_recover_fix() {
        let mut i = healthy();
        i.interrupted = Some("install of 2 packages, started 2026-10-15 10:00 UTC".into());
        let r = evaluate(&i);
        let c = one(&r, "interrupted");
        assert_eq!(c.status, Status::Warning);
        assert_eq!(c.fix.as_deref(), Some("emrac history recover"));
        assert!(evaluate(&healthy())
            .checks
            .iter()
            .all(|c| c.id != "interrupted"));
    }

    #[test]
    fn the_ledger_watch_check_reports_each_state() {
        let watch = |w: WatchFacts| {
            let mut i = healthy();
            i.ledger_watch = Some(w);
            let r = evaluate(&i);
            one(&r, "ledger-watch").clone()
        };
        let installed = WatchFacts {
            rules_installed: true,
            ..WatchFacts::default()
        };
        assert!(find(&evaluate(&healthy()), "ledger-watch").is_empty());

        let c = watch(WatchFacts::default());
        assert_eq!(
            (c.status, c.fix.as_deref()),
            (Status::Note, Some("sudo emrac ledger watch install"))
        );
        let c = watch(WatchFacts {
            unreadable: Some("join systemd-journal".into()),
            ..installed.clone()
        });
        assert_eq!(c.status, Status::Note);
        assert!(c.detail[0].contains("systemd-journal"));
        // Appends from before the rules existed could not have been seen.
        let c = watch(WatchFacts {
            witnesses: 3,
            ..installed.clone()
        });
        assert_eq!(c.status, Status::Ok, "{}", c.summary);
        let c = watch(WatchFacts {
            witnesses: 3,
            witnesses_after_rules: 2,
            ..installed.clone()
        });
        assert_eq!(c.status, Status::Warning);
        assert!(c.summary.contains("not active"), "{}", c.summary);
        let c = watch(WatchFacts {
            events: 1,
            foreign: vec!["rm touched it".into()],
            ..installed.clone()
        });
        assert_eq!(c.status, Status::Warning);
        assert_eq!(c.detail, ["rm touched it"]);
        let c = watch(WatchFacts {
            events: 1,
            unwitnessed: vec!["emrac wrote".into()],
            ..installed.clone()
        });
        assert_eq!(c.status, Status::Warning);
        let c = watch(WatchFacts {
            events: 4,
            witnesses: 4,
            ..installed
        });
        assert_eq!(c.status, Status::Ok);
        assert!(c.summary.contains("4 audit events"), "{}", c.summary);
    }
}

//! What the `policy-helper` does, as root: change the policy, keep the pacman
//! hook that applies it in step, apply it. Every check is made before anything
//! is written. See `docs/plan/r3/u8-override-divert.md`.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::backend::AlpmBackend;
use crate::error::{Error, Result};
use crate::policy::{
    self, apply, check_path, check_trust, divert_action, divert_dir, hook_state, hooks_wanted,
    is_stale, lookup_id, override_action, parse_mode, refresh_before, replace_file, sha256_file,
    wanted_of, Divert, DivertAction, Env, Event, EventKind, HookState, Override, OverrideAction,
    Policy, HOOK_FILE, POLICY_FILE, PRE_HOOK_FILE, STORE_DIR, TRUSTED_EXES,
};

/// Where everything is. The real system's, or a temporary one in tests.
pub struct Paths {
    pub policy: PathBuf,
    pub store: PathBuf,
    pub hook_dir: PathBuf,
    pub root: PathBuf,
    pub passwd: PathBuf,
    pub group: PathBuf,
    /// The `emrac` that is running (what the hook would call).
    pub exe: PathBuf,
    /// The uid that must own `exe` for the hook to be allowed (root).
    pub trusted_owner: u32,
    pub trusted_exes: Vec<PathBuf>,
}

fn err(text: impl Into<String>) -> Error {
    Error::Policy(text.into())
}

fn io_err(context: &str, e: std::io::Error) -> Error {
    Error::Policy(format!("{context}: {e}"))
}

impl Paths {
    pub fn system() -> Result<Paths> {
        let output = Command::new("pacman-conf")
            .arg("HookDir")
            .output()
            .map_err(Error::PacmanConfSpawn)?;
        let hook_dir = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map_or_else(|| PathBuf::from("/etc/pacman.d/hooks"), PathBuf::from);
        let root = Command::new("pacman-conf")
            .arg("RootDir")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "/".to_string());
        Ok(Paths {
            policy: PathBuf::from(POLICY_FILE),
            store: PathBuf::from(STORE_DIR),
            hook_dir,
            root: PathBuf::from(root),
            passwd: PathBuf::from("/etc/passwd"),
            group: PathBuf::from("/etc/group"),
            exe: std::env::current_exe()
                .map_err(|e| io_err("couldn't find the running emrac", e))?,
            trusted_owner: 0,
            trusted_exes: TRUSTED_EXES.iter().map(PathBuf::from).collect(),
        })
    }

    fn read_hook(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.hook_dir.join(name)).ok()
    }

    fn exe_text(&self) -> String {
        self.exe.display().to_string()
    }

    fn trust(&self) -> std::result::Result<(), String> {
        let allowed: Vec<&Path> = self.trusted_exes.iter().map(PathBuf::as_path).collect();
        check_trust(&self.exe, self.trusted_owner, &allowed)
    }

    fn tables(&self) -> (String, String) {
        (
            fs::read_to_string(&self.passwd).unwrap_or_default(),
            fs::read_to_string(&self.group).unwrap_or_default(),
        )
    }

    fn load(&self) -> Result<Policy> {
        policy::load(&self.policy).map_err(err)
    }

    /// Writes `policy` (all or nothing) and sets the hook to match. The hook is
    /// checked first: a policy no hook will apply is not recorded.
    fn save(&self, policy: &Policy) -> Result<Vec<String>> {
        let mut notes = Vec::new();
        if !policy.is_empty() {
            self.trust().map_err(|why| {
                err(format!(
                    "not recorded: the hook that applies the policy runs emrac as root, and {why}. Install emrac as a package, or copy it into /usr/local/bin as root, and try again"
                ))
            })?;
        }
        if let Some(parent) = self.policy.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| io_err("couldn't create the policy directory", e))?;
        }
        write_atomic(&self.policy, policy.to_text().as_bytes(), 0o644)
            .map_err(|e| io_err("couldn't write the policy", e))?;
        let wanted = hooks_wanted(policy, &self.exe_text());
        if !wanted.is_empty() {
            fs::create_dir_all(&self.hook_dir)
                .map_err(|e| io_err("couldn't create the hook directory", e))?;
        }
        for (name, text) in &wanted {
            if self.read_hook(name).as_deref() != Some(text.as_str()) {
                write_atomic(&self.hook_dir.join(name), text.as_bytes(), 0o644)
                    .map_err(|e| io_err("couldn't write the hook", e))?;
                notes.push(format!(
                    "wrote the hook {}",
                    self.hook_dir.join(name).display()
                ));
            }
        }
        for name in [HOOK_FILE, PRE_HOOK_FILE] {
            if !wanted.iter().any(|(n, _)| *n == name) && self.hook_dir.join(name).exists() {
                fs::remove_file(self.hook_dir.join(name))
                    .map_err(|e| io_err("couldn't remove the hook", e))?;
                notes.push(format!(
                    "removed the hook {}",
                    self.hook_dir.join(name).display()
                ));
            }
        }
        Ok(notes)
    }

    fn env_with<R>(&self, f: impl FnOnce(&Env) -> R) -> R {
        let (passwd, group) = self.tables();
        let alpm = AlpmBackend::init().ok();
        let lookup = |rel: &str| {
            alpm.as_ref()
                .and_then(|a| a.file_record(rel))
                .and_then(|r| r.sha256)
        };
        let env = Env {
            owner_uid: self.trusted_owner,
            root: &self.root,
            store: &self.store,
            passwd: &passwd,
            group: &group,
            package_sha: &lookup,
        };
        f(&env)
    }
}

/// Writes a file next to its place and renames it over: never half written.
fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("emrac-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// What a helper call did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub notes: Vec<String>,
    pub events: Vec<Event>,
}

impl Outcome {
    pub fn failed(&self) -> bool {
        self.events.iter().any(|e| e.kind == EventKind::Failed)
    }
}

/// Applies the whole policy now, and puts the hook right.
pub fn apply_policy(paths: &Paths) -> Result<Outcome> {
    let policy = paths.load()?;
    let mut out = Outcome::default();
    match hook_state(&policy, &|n| paths.read_hook(n), &paths.exe_text()) {
        HookState::Ok | HookState::NotNeeded => {}
        _ => match paths.save(&policy) {
            Ok(notes) => out.notes.extend(notes),
            // An explicit apply still applies; the hook is what is not in place.
            Err(e) => out.notes.push(format!("the hook could not be set: {e}")),
        },
    }
    out.events = paths.env_with(|env| apply(&policy, env));
    Ok(out)
}

/// Before a transaction (the pacman hook): saves your edits of diverted files.
/// Never fails the transaction: problems are reported and the run goes on.
pub fn capture_before(paths: &Paths) -> Result<Outcome> {
    let policy = paths.load()?;
    Ok(Outcome {
        events: paths.env_with(|env| refresh_before(&policy, env)),
        ..Outcome::default()
    })
}

/// Adds or replaces the override of `o.path`.
pub fn set_override(paths: &Paths, o: Override, allow_privileged: bool) -> Result<Outcome> {
    o.validate().map_err(err)?;
    if let Some(mode) = o.mode.as_deref().map(parse_mode).transpose().map_err(err)? {
        if policy::is_privileged_mode(mode) && !allow_privileged {
            return Err(err(format!(
                "mode {mode:o} makes {} run with its owner's or group's rights; this needs the explicit consent of the person asking",
                o.path
            )));
        }
    }
    let (passwd, group) = paths.tables();
    wanted_of(&o, &passwd, &group).map_err(err)?;
    policy::check_location(
        &paths.root,
        &paths.root.join(o.path.trim_start_matches('/')),
        paths.trusted_owner,
    )
    .map_err(err)?;
    let mut policy = paths.load()?;
    let mut out = Outcome::default();
    match policy.overrides.iter_mut().find(|x| x.path == o.path) {
        Some(existing) => *existing = o.clone(),
        None => policy.overrides.push(o.clone()),
    }
    // Only a path a package owns is something pacman will reset.
    let rel = o.path.trim_start_matches('/');
    let owned = AlpmBackend::init().ok().is_some_and(|a| {
        a.file_record(rel).is_some() || a.file_record(&format!("{rel}/")).is_some()
    });
    if !owned {
        out.notes.push(format!(
            "no installed package owns {}: pacman will not change it, so this only sets it once",
            o.path
        ));
    }
    out.notes.extend(paths.save(&policy)?);
    out.events = paths.env_with(|env| {
        apply(
            &Policy {
                overrides: vec![o],
                diverts: vec![],
            },
            env,
        )
    });
    Ok(out)
}

pub fn remove_override(paths: &Paths, path: &str) -> Result<Outcome> {
    check_path(path).map_err(err)?;
    let mut policy = paths.load()?;
    if policy.override_of(path).is_none() {
        return Err(err(format!("{path} has no override")));
    }
    policy.overrides.retain(|o| o.path != path);
    let mut out = Outcome::default();
    out.notes.extend(paths.save(&policy)?);
    out.notes.push(format!(
        "{path} keeps its mode and owner as they are now; the next upgrade sets the package's back"
    ));
    Ok(out)
}

/// Starts diverting `path`: your file as it is now becomes the one that is kept.
pub fn add_divert(
    paths: &Paths,
    path: &str,
    note: Option<String>,
    consented: bool,
) -> Result<Outcome> {
    check_path(path).map_err(err)?;
    if !consented {
        return Err(err(format!(
            "diverting {path} freezes it: the package's later versions of it, security fixes included, will not take effect. This needs the explicit consent of the person asking"
        )));
    }
    let mut policy = paths.load()?;
    if policy.is_diverted(path) {
        return Err(err(format!("{path} is already diverted")));
    }
    let live = paths.root.join(path.trim_start_matches('/'));
    policy::check_location(&paths.root, &live, paths.trusted_owner).map_err(err)?;
    let meta = fs::symlink_metadata(&live).map_err(|e| io_err(path, e))?;
    if !meta.is_file() {
        return Err(err(format!(
            "{path} is not a regular file (links and directories cannot be diverted)"
        )));
    }
    let alpm = AlpmBackend::init()?;
    let record = alpm
        .file_record(path.trim_start_matches('/'))
        .ok_or_else(|| err(format!("no installed package owns {path}: nothing would overwrite it, so there is nothing to divert")))?;
    if record.backup {
        return Err(err(format!(
            "{path} is a backup= file of {}: pacman already keeps your edits and puts the new version beside it as .pacnew (`emrac pacnew` helps)",
            record.package
        )));
    }
    if !record.is_regular_file || record.sha256.is_none() {
        return Err(err(format!(
            "{} recorded no checksum for {path}, so your version could not be told from the package's",
            record.package
        )));
    }
    // Keep your file as it is now, in a directory only root can enter.
    let dir = divert_dir(&paths.store, path);
    fs::create_dir_all(&dir).map_err(|e| io_err("couldn't create the store", e))?;
    for d in [paths.store.as_path(), dir.as_path()] {
        fs::set_permissions(d, fs::Permissions::from_mode(0o700))
            .map_err(|e| io_err("couldn't protect the store", e))?;
    }
    replace_file(&live, &dir.join("local"), &meta)
        .map_err(|e| io_err("couldn't keep a copy of the file", e))?;
    policy.diverts.push(Divert {
        path: path.to_string(),
        note,
        added: Some(now_secs()),
        base: record.sha256.clone(),
        base_version: Some(record.version.clone()),
    });
    let mut out = Outcome::default();
    if let Err(e) = paths.save(&policy) {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    let mut notes = vec![format!(
        "{path} (package {} {}) is diverted: your version is kept in the store",
        record.package, record.version
    )];
    if sha256_file(&live).ok() == record.sha256 {
        notes.push("it is still identical to the package's: edit it now, and the next upgrade will keep your edit".into());
    }
    notes.push(format!(
        "a diversion freezes the file: when {} changes it, emrac warns you after every transaction, and `emrac divert diff {path}` shows what you are missing",
        record.package
    ));
    out.notes = notes;
    Ok(out)
}

/// You have looked at the package's changes to a diverted file and decided to
/// keep yours: what the package ships now becomes the version you diverted from.
pub fn reviewed(paths: &Paths, path: &str) -> Result<Outcome> {
    check_path(path).map_err(err)?;
    let mut policy = paths.load()?;
    let record = AlpmBackend::init()?
        .file_record(path.trim_start_matches('/'))
        .ok_or_else(|| err(format!("no installed package owns {path} any more")))?;
    let sha = record.sha256.clone().ok_or_else(|| {
        err(format!(
            "{} recorded no checksum for {path}",
            record.package
        ))
    })?;
    let divert = policy
        .diverts
        .iter_mut()
        .find(|d| d.path == path)
        .ok_or_else(|| err(format!("{path} is not diverted")))?;
    divert.base = Some(sha);
    divert.base_version = Some(record.version.clone());
    let mut out = Outcome::default();
    out.notes.extend(paths.save(&policy)?);
    out.notes.push(format!(
        "{path} is marked as reviewed against {} {}; emrac warns again only if the package changes it again",
        record.package, record.version
    ));
    Ok(out)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn remove_divert(paths: &Paths, path: &str) -> Result<Outcome> {
    check_path(path).map_err(err)?;
    let mut policy = paths.load()?;
    if !policy.is_diverted(path) {
        return Err(err(format!("{path} is not diverted")));
    }
    policy.diverts.retain(|d| d.path != path);
    let mut out = Outcome::default();
    out.notes.extend(paths.save(&policy)?);
    let _ = fs::remove_dir_all(divert_dir(&paths.store, path));
    out.notes.push(format!(
        "{path} is left as it is; the next upgrade of its package replaces it with the package's version"
    ));
    Ok(out)
}

// ------------------------------------------------------------------ status

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineState {
    InEffect,
    /// Your version is in place, but the package has changed the file since
    /// you diverted it or last reviewed it: you are not getting the change.
    Stale,
    /// Not as the policy says (pacman reset it, or the hook has not run).
    Drift,
    Absent,
    /// Could not be looked at.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusLine {
    /// `override` or `divert`.
    pub kind: &'static str,
    pub path: String,
    pub state: LineState,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub lines: Vec<StatusLine>,
    pub hook: HookState,
    /// Why the hook cannot be written or is not as it should be.
    pub hook_note: Option<String>,
}

impl Status {
    pub fn in_order(&self) -> bool {
        matches!(self.hook, HookState::Ok | HookState::NotNeeded)
            && self
                .lines
                .iter()
                .all(|l| matches!(l.state, LineState::InEffect | LineState::Absent))
    }
}

/// Where the policy stands, without changing anything. Works without root as
/// far as the files can be read; what it cannot read is `Unknown`.
pub fn status(paths: &Paths) -> Result<Status> {
    let policy = paths.load()?;
    let (passwd, group) = paths.tables();
    let alpm = AlpmBackend::init().ok();
    let mut lines = Vec::new();
    for o in &policy.overrides {
        let live = paths.root.join(o.path.trim_start_matches('/'));
        let meta = fs::symlink_metadata(&live).ok();
        let (state, text) = match wanted_of(o, &passwd, &group) {
            Err(e) => (LineState::Unknown, e),
            Ok(w) => match override_action(w, meta.as_ref()) {
                OverrideAction::InEffect => (LineState::InEffect, "as the override says".into()),
                OverrideAction::Absent => (LineState::Absent, "the file is not there".into()),
                OverrideAction::Symlink => (
                    LineState::Unknown,
                    "it is a symbolic link; overrides do not follow links".into(),
                ),
                OverrideAction::Special => (
                    LineState::Unknown,
                    "it is not a regular file or a directory".into(),
                ),
                OverrideAction::Set(_) => (
                    LineState::Drift,
                    "not as the override says (pacman reset it? `emrac policy apply`)".into(),
                ),
            },
        };
        lines.push(StatusLine {
            kind: "override",
            path: o.path.clone(),
            state,
            text,
        });
    }
    for d in &policy.diverts {
        let live = paths.root.join(d.path.trim_start_matches('/'));
        let dir = divert_dir(&paths.store, &d.path);
        let live_sha = fs::symlink_metadata(&live)
            .ok()
            .filter(|m| m.is_file())
            .and_then(|_| sha256_file(&live).ok());
        let local_file = dir.join("local");
        let local_sha = sha256_file(&local_file).ok();
        let pkg_sha = alpm
            .as_ref()
            .and_then(|a| a.file_record(d.path.trim_start_matches('/')))
            .and_then(|r| r.sha256);
        let unreadable = local_sha.is_none() && local_file.exists();
        let stale = is_stale(d.base.as_deref(), pkg_sha.as_deref()) == Some(true);
        let (state, text) = if unreadable {
            (
                LineState::Unknown,
                "your stored copy needs root to read".into(),
            )
        } else {
            match divert_action(pkg_sha.as_deref(), live_sha.as_deref(), local_sha.as_deref()) {
                DivertAction::InPlace if stale => (
                    LineState::Stale,
                    stale_text(d, alpm.as_ref().and_then(|a| a.file_record(d.path.trim_start_matches('/'))).map(|r| r.version)),
                ),
                DivertAction::InPlace => (LineState::InEffect, "your version is in place".into()),
                DivertAction::PutBack { set_aside: true } => (
                    LineState::Drift,
                    "pacman replaced it with the package's version (`emrac policy apply` puts yours back)".into(),
                ),
                DivertAction::Dormant => (
                    LineState::Absent,
                    "its package is not installed; your copy waits in the store".into(),
                ),
                DivertAction::PutBack { set_aside: false } => (
                    LineState::Drift,
                    "the file is gone (`emrac policy apply` puts your copy back)".into(),
                ),
                DivertAction::RefreshLocal => (
                    LineState::Drift,
                    "you changed it since the copy was kept (`emrac policy apply` updates the copy)".into(),
                ),
                DivertAction::Unknown(why) => (LineState::Unknown, why.into()),
                DivertAction::Broken(why) => (LineState::Drift, why.into()),
            }
        };
        lines.push(StatusLine {
            kind: "divert",
            path: d.path.clone(),
            state,
            text,
        });
    }
    let hook = hook_state(&policy, &|n| paths.read_hook(n), &paths.exe_text());
    let hook_note = match hook {
        HookState::Missing => Some(
            "the hook is missing: the policy would not be re-applied after a transaction"
                .to_string(),
        ),
        HookState::Stale => Some("the hook is not what the policy calls for".to_string()),
        HookState::Unwanted => Some("a hook is installed although the policy is empty".to_string()),
        _ => None,
    };
    let hook_note = hook_note.or_else(|| {
        (!policy.is_empty())
            .then(|| paths.trust().err())
            .flatten()
            .map(|why| format!("the hook would not be allowed to run: {why}"))
    });
    Ok(Status {
        lines,
        hook,
        hook_note,
    })
}

/// Your version of a diverted file against the package's version that an
/// upgrade set aside. Needs root (the store is closed to others).
pub fn diff_diverted(paths: &Paths, path: &str) -> Result<String> {
    check_path(path).map_err(err)?;
    let policy = paths.load()?;
    if !policy.is_diverted(path) {
        return Err(err(format!("{path} is not diverted")));
    }
    let dir = divert_dir(&paths.store, path);
    let read =
        |name: &str| fs::read(dir.join(name)).map(|b| String::from_utf8_lossy(&b).into_owned());
    let local = read("local").map_err(|e| io_err("couldn't read your kept copy", e))?;
    let package = match read("package") {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(err(format!(
                "no package version of {path} has been set aside yet: that happens when an upgrade replaces your file"
            )))
        }
        Err(e) => return Err(io_err("couldn't read the package's version", e)),
    };
    if local == package {
        return Ok(String::new());
    }
    crate::pacnew::unified_diff("yours", &local, "package", &package)
}

/// The sentence for a stale diversion: what changed and what to do.
fn stale_text(d: &Divert, now_version: Option<String>) -> String {
    format!(
        "the package changed this file since you diverted it ({} \u{2192} {}): its changes, security fixes included, are NOT in use. `emrac divert diff`, then merge them, `emrac divert reviewed`, or `emrac divert remove`",
        d.base_version.as_deref().unwrap_or("?"),
        now_version.as_deref().unwrap_or("?")
    )
}

/// How many diversions are stale, for `doctor` and the plan.
pub fn stale_diversions(status: &Status) -> Vec<&StatusLine> {
    status
        .lines
        .iter()
        .filter(|l| l.kind == "divert" && l.state == LineState::Stale)
        .collect()
}

/// A user name or id that exists on this system.
pub fn user_exists(name: &str) -> bool {
    lookup_id(&fs::read_to_string("/etc/passwd").unwrap_or_default(), name).is_some()
}

pub fn group_exists(name: &str) -> bool {
    lookup_id(&fs::read_to_string("/etc/group").unwrap_or_default(), name).is_some()
}

/// The numeric owner of a path, for `override list`.
pub fn mode_and_owner(path: &Path) -> Option<(u32, u32, u32)> {
    fs::symlink_metadata(path)
        .ok()
        .map(|m| (m.mode() & 0o7777, m.uid(), m.gid()))
}

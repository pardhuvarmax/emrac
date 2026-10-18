//! Local file policy: `override` (mode and owner of a packaged file) and
//! `divert` (your own version of a packaged file), kept across upgrades.
//!
//! pacman puts a package's files back exactly as shipped on every upgrade and
//! reinstall (`docs/plan/r3/u8-override-divert.md`). The policy lives in
//! `/etc/emrac/policy.toml`; a pacman hook runs `emrac policy-helper apply`
//! after each transaction that touches a policy path. Everything here is
//! plain functions over paths given by the caller, so it is tested on temporary
//! files; only the helper in the command-line crate passes the real ones.

use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const POLICY_FILE: &str = "/etc/emrac/policy.toml";
pub const STORE_DIR: &str = "/var/lib/emrac/divert";
pub const HOOK_FILE: &str = "zz-emrac-policy.hook";
/// The hook that runs before a transaction, to save an edit of a diverted file
/// before pacman overwrites it.
pub const PRE_HOOK_FILE: &str = "zz-emrac-policy-pre.hook";
/// The only places root will run `emrac` from in the hook.
pub const TRUSTED_EXES: [&str; 2] = ["/usr/bin/emrac", "/usr/local/bin/emrac"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default, rename = "override", skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    #[serde(default, rename = "divert", skip_serializing_if = "Vec::is_empty")]
    pub diverts: Vec<Divert>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub path: String,
    /// Octal, three or four digits: `755`, `4750`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// A user name or a number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Divert {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When it was diverted (seconds since the epoch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    /// The sha256 of the package's version of the file when it was diverted or
    /// last reviewed: if the package's file differs from this now, the package
    /// has changed it and you are not getting the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The package version `base` belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_version: Option<String>,
}

impl Divert {
    /// A diversion with nothing but a path (tests, hand-written policies).
    pub fn new(path: &str) -> Divert {
        Divert {
            path: path.to_string(),
            note: None,
            added: None,
            base: None,
            base_version: None,
        }
    }
}

/// Whether the package has changed a diverted file since it was diverted or
/// last reviewed. Unknown (`None`) when there is no base or no record.
pub fn is_stale(base: Option<&str>, package_now: Option<&str>) -> Option<bool> {
    Some(base? != package_now?)
}

/// After this many days a stale diversion is called overdue.
pub const OVERDUE_DAYS: u64 = 90;

/// Why diverting this file deserves extra care: the kinds of file whose
/// security fixes matter most. `head` is the file's first bytes.
pub fn divert_risks(mode: u32, head: &[u8]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if is_privileged_mode(mode) {
        out.push("it runs with the rights of its owner or group (setuid or setgid)");
    }
    if head.starts_with(b"\x7fELF") {
        out.push(
            "it is a compiled program or library, the kind of file security fixes are shipped in",
        );
    }
    out
}

// ---------------------------------------------------------------- validation

/// A path the policy may name: absolute and already normal, nothing that a
/// pacman glob or a text file would read as something else.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 4096 {
        return Err("the path is empty or too long".into());
    }
    if !path.starts_with('/') {
        return Err(format!("'{path}' is not an absolute path"));
    }
    if path == "/" {
        return Err("'/' cannot be named".into());
    }
    if path.chars().any(|c| c.is_control() || "*?[]\\".contains(c)) {
        return Err(format!(
            "'{path}' has a control character or one of * ? [ ] \\, which a pacman hook would read as a pattern"
        ));
    }
    if path.ends_with('/') || path.contains("//") {
        return Err(format!(
            "'{path}' is not in its normal form (no trailing or doubled '/')"
        ));
    }
    // The raw segments: `Path::components` would quietly drop a middle '.'.
    if path.split('/').skip(1).any(|c| c == "." || c == "..") {
        return Err(format!("'{path}' contains '.' or '..'"));
    }
    if path
        .split('/')
        .nth(1)
        .is_some_and(|first| first.starts_with('!'))
    {
        return Err("a path cannot start with '!'".into());
    }
    for virtual_fs in ["/proc", "/sys", "/dev", "/run"] {
        if path == virtual_fs || path.starts_with(&format!("{virtual_fs}/")) {
            return Err(format!("{virtual_fs} is not made of package files"));
        }
    }
    Ok(())
}

/// `4750` as the number `0o4750`.
pub fn parse_mode(text: &str) -> Result<u32, String> {
    if !(3..=4).contains(&text.len()) || !text.chars().all(|c| ('0'..='7').contains(&c)) {
        return Err(format!(
            "'{text}' is not a mode: three or four octal digits, like 755 or 4750"
        ));
    }
    u32::from_str_radix(text, 8).map_err(|e| e.to_string())
}

/// Whether a mode makes a program run with its owner's or group's rights.
pub fn is_privileged_mode(mode: u32) -> bool {
    mode & 0o6000 != 0
}

fn check_name(kind: &str, name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && (name.chars().all(|c| c.is_ascii_digit())
            || (name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.-$".contains(c))));
    if ok {
        Ok(())
    } else {
        Err(format!("'{name}' is not a valid {kind} name or number"))
    }
}

impl Override {
    pub fn validate(&self) -> Result<(), String> {
        check_path(&self.path)?;
        if self.mode.is_none() && self.owner.is_none() && self.group.is_none() {
            return Err(format!(
                "the override of {} sets nothing: give a mode, an owner or a group",
                self.path
            ));
        }
        if let Some(mode) = &self.mode {
            parse_mode(mode)?;
        }
        if let Some(owner) = &self.owner {
            check_name("user", owner)?;
        }
        if let Some(group) = &self.group {
            check_name("group", group)?;
        }
        Ok(())
    }
}

impl Policy {
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty() && self.diverts.is_empty()
    }

    /// Every entry valid and no path named twice in the same list.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for o in &self.overrides {
            o.validate()?;
            if !seen.insert(("override", o.path.as_str())) {
                return Err(format!("{} is overridden twice", o.path));
            }
        }
        for d in &self.diverts {
            check_path(&d.path)?;
            if !seen.insert(("divert", d.path.as_str())) {
                return Err(format!("{} is diverted twice", d.path));
            }
        }
        Ok(())
    }

    pub fn parse(text: &str) -> Result<Policy, String> {
        let policy: Policy = toml::from_str(text).map_err(|e| e.to_string())?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Local file policy, kept by `emrac override` and `emrac divert`.\n\
             # Edit with those commands (or by hand: `emrac policy apply` applies it).\n\n{}",
            toml::to_string_pretty(self).unwrap_or_default()
        )
    }

    /// Every path the policy names, once, in order.
    pub fn paths(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for p in self
            .overrides
            .iter()
            .map(|o| o.path.as_str())
            .chain(self.diverts.iter().map(|d| d.path.as_str()))
        {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    pub fn override_of(&self, path: &str) -> Option<&Override> {
        self.overrides.iter().find(|o| o.path == path)
    }

    pub fn is_diverted(&self, path: &str) -> bool {
        self.diverts.iter().any(|d| d.path == path)
    }
}

/// Loads the policy; a missing file is an empty policy.
pub fn load(path: &Path) -> Result<Policy, String> {
    match fs::read_to_string(path) {
        Ok(text) => Policy::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Policy::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// -------------------------------------------------------- users and groups

/// The id for `name` in the text of `/etc/passwd` or `/etc/group` (the third
/// field), or the number itself.
pub fn lookup_id(table: &str, name: &str) -> Option<u32> {
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    table.lines().find_map(|line| {
        let mut f = line.split(':');
        (f.next()? == name).then(|| f.nth(1)?.parse().ok())?
    })
}

// ------------------------------------------------------------ decisions

/// What a file should look like; only the parts the policy names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wanted {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideAction {
    InEffect,
    Absent,
    /// Not touched: a link's own mode means nothing and following it could
    /// reach somewhere else.
    Symlink,
    /// A device, socket or the like.
    Special,
    /// Change these parts (only the ones that differ).
    Set(Wanted),
}

pub fn override_action(wanted: Wanted, meta: Option<&fs::Metadata>) -> OverrideAction {
    let Some(meta) = meta else {
        return OverrideAction::Absent;
    };
    let ft = meta.file_type();
    if ft.is_symlink() {
        return OverrideAction::Symlink;
    }
    if !ft.is_file() && !ft.is_dir() {
        return OverrideAction::Special;
    }
    let differing = Wanted {
        mode: wanted.mode.filter(|m| meta.mode() & 0o7777 != *m),
        uid: wanted.uid.filter(|u| meta.uid() != *u),
        gid: wanted.gid.filter(|g| meta.gid() != *g),
    };
    if differing == Wanted::default() {
        OverrideAction::InEffect
    } else {
        OverrideAction::Set(differing)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivertAction {
    /// The file is yours already.
    InPlace,
    /// pacman replaced it (or removed it): put yours back; set the package's
    /// version aside first when there is one.
    PutBack { set_aside: bool },
    /// You changed the file since: your copy in the store is out of date.
    RefreshLocal,
    /// No package owns the file any more (it was removed): your copy waits in
    /// the store and comes back when the package is installed again. Putting it
    /// back now would make the package impossible to install (a file it ships
    /// would already exist).
    Dormant,
    /// Cannot tell yours from the package's.
    Unknown(&'static str),
    /// Cannot be put right.
    Broken(&'static str),
}

/// From three sha256 sums: the package's (pacman's record), the file now, and
/// your stored copy; `None` where there is no such file or sum.
pub fn divert_action(
    package: Option<&str>,
    live: Option<&str>,
    local: Option<&str>,
) -> DivertAction {
    match live {
        None => match (local, package) {
            (Some(_), Some(_)) => DivertAction::PutBack { set_aside: false },
            (Some(_), None) => DivertAction::Dormant,
            (None, _) => DivertAction::Broken("the file is gone and your copy is not in the store"),
        },
        Some(l) if Some(l) == local => DivertAction::InPlace,
        Some(l) if Some(l) == package => match local {
            Some(_) => DivertAction::PutBack { set_aside: true },
            None => {
                DivertAction::Broken("pacman replaced the file and your copy is not in the store")
            }
        },
        Some(_) if package.is_none() => DivertAction::Unknown(
            "the package recorded no checksum for it, so yours cannot be told from its",
        ),
        Some(_) => DivertAction::RefreshLocal,
    }
}

// ------------------------------------------------------------- hashing, I/O

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Where a diversion's files live inside the store.
pub fn divert_dir(store: &Path, path: &str) -> PathBuf {
    let id: String = Sha256::digest(path.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    store.join(id)
}

/// Writes `src`'s content to `dst` as a new file next to it and renames it into
/// place, with `meta`'s mode and owner: no moment where `dst` is half written.
pub fn replace_file(src: &Path, dst: &Path, meta: &fs::Metadata) -> std::io::Result<()> {
    let parent = dst
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent directory"))?;
    let name = dst.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{name}.emrac-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let result = (|| {
        let mut from = fs::File::open(src)?;
        let mut to = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        std::io::copy(&mut from, &mut to)?;
        std::os::unix::fs::chown(&tmp, Some(meta.uid()), Some(meta.gid()))?;
        to.set_permissions(fs::Permissions::from_mode(meta.mode() & 0o7777))?;
        to.sync_all()?;
        fs::rename(&tmp, dst)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

// ----------------------------------------------------------------- applying

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Something was changed.
    Applied,
    /// Already as the policy says.
    Already,
    /// Done, but the package has changed the file and you are not getting the change.
    Stale,
    /// Not touched, and why.
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Event {
    pub path: String,
    pub kind: EventKind,
    pub text: String,
}

/// What `apply` needs from the system, so tests can give it a temporary one.
pub struct Env<'a> {
    /// The uid that must own every directory above a file root touches (root).
    pub owner_uid: u32,
    /// `/` on a real system.
    pub root: &'a Path,
    pub store: &'a Path,
    pub passwd: &'a str,
    pub group: &'a str,
    /// The sha256 the owning package's `mtree` records for a path (no leading `/`).
    pub package_sha: &'a dyn Fn(&str) -> Option<String>,
}

fn event(path: &str, kind: EventKind, text: impl Into<String>) -> Event {
    Event {
        path: path.to_string(),
        kind,
        text: text.into(),
    }
}

fn live_path(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// The wanted mode and owner of an override, resolved against `passwd` and `group`.
pub fn wanted_of(o: &Override, passwd: &str, group: &str) -> Result<Wanted, String> {
    Ok(Wanted {
        mode: o.mode.as_deref().map(parse_mode).transpose()?,
        uid: match &o.owner {
            Some(n) => Some(lookup_id(passwd, n).ok_or_else(|| format!("no user '{n}'"))?),
            None => None,
        },
        gid: match &o.group {
            Some(n) => Some(lookup_id(group, n).ok_or_else(|| format!("no group '{n}'"))?),
            None => None,
        },
    })
}

fn describe(w: Wanted) -> String {
    let mut parts = Vec::new();
    if let Some(m) = w.mode {
        parts.push(format!("mode {m:o}"));
    }
    if let Some(u) = w.uid {
        parts.push(format!("owner {u}"));
    }
    if let Some(g) = w.gid {
        parts.push(format!("group {g}"));
    }
    parts.join(", ")
}

fn apply_override(o: &Override, env: &Env) -> Event {
    let wanted = match wanted_of(o, env.passwd, env.group) {
        Ok(w) => w,
        Err(e) => return event(&o.path, EventKind::Failed, e),
    };
    let path = live_path(env.root, &o.path);
    let meta = fs::symlink_metadata(&path).ok();
    match override_action(wanted, meta.as_ref()) {
        OverrideAction::InEffect => event(&o.path, EventKind::Already, "as the override says"),
        OverrideAction::Absent => event(&o.path, EventKind::Skipped, "the file is not there"),
        OverrideAction::Symlink => event(
            &o.path,
            EventKind::Skipped,
            "it is a symbolic link; overrides do not follow links",
        ),
        OverrideAction::Special => event(
            &o.path,
            EventKind::Skipped,
            "it is not a regular file or a directory",
        ),
        OverrideAction::Set(_) if check_location(env.root, &path, env.owner_uid).is_err() => event(
            &o.path,
            EventKind::Failed,
            check_location(env.root, &path, env.owner_uid).unwrap_err(),
        ),
        OverrideAction::Set(change) => {
            // Through one open descriptor, refusing a link that appeared since.
            let result = (|| -> std::io::Result<()> {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc_o_nofollow())
                    .open(&path)?;
                if change.uid.is_some() || change.gid.is_some() {
                    std::os::unix::fs::fchown(&file, change.uid, change.gid)?;
                }
                if let Some(mode) = change.mode {
                    file.set_permissions(fs::Permissions::from_mode(mode))?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => event(
                    &o.path,
                    EventKind::Applied,
                    format!("set {}", describe(change)),
                ),
                Err(e) => event(&o.path, EventKind::Failed, e.to_string()),
            }
        }
    }
}

/// `O_NOFOLLOW` on Linux, without a libc dependency.
fn libc_o_nofollow() -> i32 {
    0o400000
}

fn apply_divert(d: &Divert, env: &Env) -> Event {
    let path = live_path(env.root, &d.path);
    let dir = divert_dir(env.store, &d.path);
    let local = dir.join("local");
    let rel = d.path.trim_start_matches('/');
    let live_sha = if fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        sha256_file(&path).ok()
    } else {
        None
    };
    let local_sha = sha256_file(&local).ok();
    let package_sha = (env.package_sha)(rel);
    if let Err(why) = check_location(env.root, &path, env.owner_uid) {
        return event(&d.path, EventKind::Failed, why);
    }
    match divert_action(
        package_sha.as_deref(),
        live_sha.as_deref(),
        local_sha.as_deref(),
    ) {
        DivertAction::InPlace => event(&d.path, EventKind::Already, "your version is in place"),
        DivertAction::Dormant => event(
            &d.path,
            EventKind::Skipped,
            "its package is not installed; your copy waits in the store and comes back when it is installed again",
        ),
        DivertAction::Unknown(why) => event(&d.path, EventKind::Skipped, why),
        DivertAction::Broken(why) => event(&d.path, EventKind::Failed, why),
        DivertAction::RefreshLocal => {
            let result =
                fs::symlink_metadata(&path).and_then(|meta| replace_file(&path, &local, &meta));
            match result {
                Ok(()) => event(
                    &d.path,
                    EventKind::Applied,
                    "you changed it since; your stored copy is updated",
                ),
                Err(e) => event(&d.path, EventKind::Failed, e.to_string()),
            }
        }
        DivertAction::PutBack { set_aside } => {
            let result = (|| -> std::io::Result<()> {
                if set_aside {
                    let meta = fs::symlink_metadata(&path)?;
                    replace_file(&path, &dir.join("package"), &meta)?;
                }
                let meta = fs::metadata(&local)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                replace_file(&local, &path, &meta)
            })();
            let changed = is_stale(d.base.as_deref(), package_sha.as_deref()) == Some(true);
            match result {
                Ok(()) if set_aside && changed => event(
                    &d.path,
                    EventKind::Stale,
                    "the package changed this file and YOUR old version is back: the package's changes, security fixes included, are NOT in use. See them with `emrac divert diff`, then merge them or `emrac divert remove`; `emrac divert reviewed` once you have looked",
                ),
                Ok(()) if set_aside => event(
                    &d.path,
                    EventKind::Applied,
                    "pacman replaced it with a version identical to the one you diverted from; your version is back",
                ),
                Ok(()) => event(
                    &d.path,
                    EventKind::Applied,
                    "the file was gone although its package is installed; your copy is back",
                ),
                Err(e) => event(&d.path, EventKind::Failed, e.to_string()),
            }
        }
    }
}

/// Applies the whole policy: diversions first (they may recreate the file),
/// then overrides (so an override applies to your version).
pub fn apply(policy: &Policy, env: &Env) -> Vec<Event> {
    let mut out: Vec<Event> = policy
        .diverts
        .iter()
        .map(|d| apply_divert(d, env))
        .collect();
    out.extend(policy.overrides.iter().map(|o| apply_override(o, env)));
    out
}

/// Before a transaction: a diverted file you edited since its copy was kept is
/// saved now, because once pacman has overwritten it the edit is gone.
pub fn refresh_before(policy: &Policy, env: &Env) -> Vec<Event> {
    let mut out = Vec::new();
    for d in &policy.diverts {
        let path = live_path(env.root, &d.path);
        let rel = d.path.trim_start_matches('/');
        let Some(meta) = fs::symlink_metadata(&path).ok().filter(|m| m.is_file()) else {
            continue;
        };
        let (Ok(live), local) = (
            sha256_file(&path),
            sha256_file(&divert_dir(env.store, &d.path).join("local")).ok(),
        ) else {
            continue;
        };
        // Yours already, or still exactly what the package installed: nothing to save.
        if Some(&live) == local.as_ref() || Some(live.clone()) == (env.package_sha)(rel) {
            continue;
        }
        let local_path = divert_dir(env.store, &d.path).join("local");
        let result = local_path
            .parent()
            .map(fs::create_dir_all)
            .transpose()
            .and_then(|_| replace_file(&path, &local_path, &meta));
        out.push(match result {
            Ok(()) => event(
                &d.path,
                EventKind::Applied,
                "your edit is saved before pacman replaces the file",
            ),
            Err(e) => event(&d.path, EventKind::Failed, e.to_string()),
        });
    }
    out
}

// -------------------------------------------------------------------- hook

/// The pacman hook that calls the helper after a transaction touching a
/// policy path. Paths are written without the leading `/`, as hooks expect.
pub fn hook_text(policy: &Policy, exe: &str) -> String {
    let rel = |p: &str| p.trim_start_matches('/').to_string();
    let mut out = String::from(
        "# Written by emrac (`emrac override`, `emrac divert`); do not edit, `emrac policy apply` rewrites it.\n",
    );
    out.push_str("[Trigger]\nOperation = Install\nOperation = Upgrade\nType = Path\n");
    for p in policy.paths() {
        out.push_str(&format!("Target = {}\n", rel(p)));
    }
    // Removal only *reports* (a diverted file's copy waits in the store); apply
    // never puts a file back for a package that is no longer installed.
    if !policy.diverts.is_empty() {
        out.push_str("\n[Trigger]\nOperation = Remove\nType = Path\n");
        for d in &policy.diverts {
            out.push_str(&format!("Target = {}\n", rel(&d.path)));
        }
    }
    out.push_str(&format!(
        "\n[Action]\nDescription = Applying emrac's local file policy (overrides and diversions)\nWhen = PostTransaction\nExec = {exe} policy-helper apply\n"
    ));
    out
}

/// Whether root may act on `path`: every directory from `root` down to it is
/// owned by `owner_uid` (root) and writable by no one else. Otherwise someone
/// else could swap a directory or a link between a check and root's write.
pub fn check_location(root: &Path, path: &Path, owner_uid: u32) -> Result<(), String> {
    let mut cur = path.parent();
    while let Some(dir) = cur.filter(|d| d.starts_with(root)) {
        let meta = match fs::metadata(dir) {
            Ok(m) => m,
            // A directory that is not there yet is created by root itself.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                cur = dir.parent();
                continue;
            }
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        };
        if meta.uid() != owner_uid {
            return Err(format!(
                "{} is not owned by root, so someone else could change what a path under it points to",
                dir.display()
            ));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(format!(
                "{} is writable by someone other than root, so root will not act on files under it",
                dir.display()
            ));
        }
        cur = dir.parent();
    }
    Ok(())
}

/// Whether root may run `exe` from a hook: one of `allowed`, owned by
/// `owner_uid` (root), writable by nobody else, and so is every directory above
/// it. A copy anyone can write would be a way to run their code as root.
pub fn check_trust(exe: &Path, owner_uid: u32, allowed: &[&Path]) -> Result<(), String> {
    if !allowed.contains(&exe) {
        return Err(format!(
            "{} is not where root may run emrac from (only {})",
            exe.display(),
            allowed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" or ")
        ));
    }
    let mut cur = Some(exe);
    while let Some(p) = cur {
        let meta = fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if meta.uid() != owner_uid {
            return Err(format!("{} is not owned by root", p.display()));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(format!(
                "{} is writable by someone other than root",
                p.display()
            ));
        }
        cur = p.parent().filter(|q| !q.as_os_str().is_empty());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookState {
    /// Nothing is configured, so no hook is wanted, and none is there.
    NotNeeded,
    Ok,
    /// Wanted and not there.
    Missing,
    /// There, but not what the policy calls for.
    Stale,
    /// There although the policy calls for none.
    Unwanted,
}

/// The hook before a transaction (only for diversions): saves an edit of a
/// diverted file before pacman overwrites or removes it.
pub fn pre_hook_text(policy: &Policy, exe: &str) -> Option<String> {
    if policy.diverts.is_empty() {
        return None;
    }
    let mut out = String::from(
        "# Written by emrac (`emrac divert`); do not edit, `emrac policy apply` rewrites it.\n",
    );
    out.push_str(
        "[Trigger]\nOperation = Install\nOperation = Upgrade\nOperation = Remove\nType = Path\n",
    );
    for d in &policy.diverts {
        out.push_str(&format!("Target = {}\n", d.path.trim_start_matches('/')));
    }
    out.push_str(&format!(
        "\n[Action]\nDescription = Saving your edits of diverted files before they can be replaced\nWhen = PreTransaction\nExec = {exe} policy-helper capture\n"
    ));
    Some(out)
}

/// The hook files the policy calls for: `(file name, content)`.
pub fn hooks_wanted(policy: &Policy, exe: &str) -> Vec<(&'static str, String)> {
    if policy.is_empty() {
        return Vec::new();
    }
    let mut out = vec![(HOOK_FILE, hook_text(policy, exe))];
    if let Some(pre) = pre_hook_text(policy, exe) {
        out.push((PRE_HOOK_FILE, pre));
    }
    out
}

/// How the hook files in place compare with what the policy calls for.
/// `read(name)` gives a hook file's content, if it exists.
pub fn hook_state(policy: &Policy, read: &dyn Fn(&str) -> Option<String>, exe: &str) -> HookState {
    let wanted = hooks_wanted(policy, exe);
    if wanted.is_empty() {
        return if [HOOK_FILE, PRE_HOOK_FILE].iter().any(|n| read(n).is_some()) {
            HookState::Unwanted
        } else {
            HookState::NotNeeded
        };
    }
    let present = wanted.iter().filter(|(n, _)| read(n).is_some()).count();
    if present == 0 {
        return HookState::Missing;
    }
    if wanted
        .iter()
        .all(|(n, text)| read(n).as_deref() == Some(text.as_str()))
        && (policy.diverts.is_empty() == read(PRE_HOOK_FILE).is_none())
    {
        HookState::Ok
    } else {
        HookState::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("emrac-policy-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
        fn write(&self, rel: &str, content: &str, mode: u32) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, content).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
            p
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn me() -> (u32, u32) {
        let m = fs::metadata(std::env::temp_dir()).unwrap();
        let own = fs::metadata(std::env::current_exe().unwrap()).unwrap();
        let _ = m;
        (own.uid(), own.gid())
    }

    #[test]
    fn paths_are_absolute_normal_and_not_patterns() {
        for good in ["/usr/bin/x", "/etc/a b/c.conf", "/usr/lib/libfoo.so.1"] {
            assert!(check_path(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "x",
            "usr/bin/x",
            "/",
            "/usr/bin/",
            "/usr//bin",
            "/usr/../etc/shadow",
            "/usr/./x",
            "/usr/*/x",
            "/usr/b?n",
            "/usr/[ab]",
            "/usr/a\\b",
            "/usr/a\nb",
            "/!x",
            "/proc/1/mem",
            "/sys",
            "/dev/null",
            "/run/lock",
        ] {
            assert!(check_path(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn modes_are_three_or_four_octal_digits() {
        assert_eq!(parse_mode("755"), Ok(0o755));
        assert_eq!(parse_mode("4750"), Ok(0o4750));
        for bad in ["", "75", "77777", "758", "rwx", "-755", "0x1ed"] {
            assert!(parse_mode(bad).is_err(), "{bad}");
        }
        assert!(is_privileged_mode(0o4755) && is_privileged_mode(0o2755));
        assert!(!is_privileged_mode(0o1777) && !is_privileged_mode(0o755));
    }

    #[test]
    fn a_policy_is_read_written_and_checked() {
        let text = "[[override]]\npath = \"/usr/bin/x\"\nmode = \"4750\"\nowner = \"root\"\ngroup = \"wheel\"\n\n[[divert]]\npath = \"/usr/lib/y\"\nnote = \"patched\"\n";
        let p = Policy::parse(text).unwrap();
        assert_eq!(p.overrides[0].mode.as_deref(), Some("4750"));
        assert_eq!(p.diverts[0].note.as_deref(), Some("patched"));
        assert_eq!(Policy::parse(&p.to_text()).unwrap(), p, "round trip");
        assert_eq!(p.paths(), ["/usr/bin/x", "/usr/lib/y"]);
        assert!(Policy::parse("").unwrap().is_empty());
        // refusals
        let bad = |t: &str| Policy::parse(t).unwrap_err();
        assert!(bad("[[override]]\npath = \"/usr/bin/x\"\n").contains("sets nothing"));
        assert!(bad("[[override]]\npath = \"x\"\nmode = \"755\"\n").contains("absolute"));
        assert!(bad("[[override]]\npath = \"/a\"\nmode = \"9\"\n").contains("mode"));
        assert!(bad("[[override]]\npath = \"/a\"\nowner = \"a b\"\n").contains("user"));
        assert!(bad("[[override]]\npath = \"/a\"\nmode = \"755\"\n[[override]]\npath = \"/a\"\nmode = \"700\"\n").contains("twice"));
        assert!(
            bad("[[override]]\npath = \"/a\"\nmode = \"755\"\nsurprise = 1\n").contains("surprise")
        );
    }

    #[test]
    fn names_and_numbers_resolve_through_passwd_and_group() {
        let passwd =
            "root:x:0:0:root:/root:/bin/bash\nnobody:x:65534:65534:Nobody:/:/usr/bin/nologin\n";
        let group = "root:x:0:\nusers:x:984:\n";
        assert_eq!(lookup_id(passwd, "nobody"), Some(65534));
        assert_eq!(lookup_id(group, "users"), Some(984));
        assert_eq!(lookup_id(passwd, "1000"), Some(1000));
        assert_eq!(lookup_id(passwd, "nosuch"), None);
        assert_eq!(lookup_id(passwd, "nobod"), None, "whole names only");
    }

    #[test]
    fn an_override_changes_only_what_differs() {
        let t = Tmp::new("ovr-action");
        let f = t.write("f", "x", 0o644);
        let meta = fs::symlink_metadata(&f).unwrap();
        let (uid, gid) = me();
        let w = |mode| Wanted {
            mode,
            uid: Some(uid),
            gid: Some(gid),
        };
        assert_eq!(
            override_action(w(Some(0o644)), Some(&meta)),
            OverrideAction::InEffect
        );
        assert_eq!(
            override_action(w(Some(0o755)), Some(&meta)),
            OverrideAction::Set(Wanted {
                mode: Some(0o755),
                uid: None,
                gid: None
            })
        );
        assert_eq!(override_action(w(None), None), OverrideAction::Absent);
        let link = t.0.join("link");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        let lm = fs::symlink_metadata(&link).unwrap();
        assert_eq!(
            override_action(w(Some(0o755)), Some(&lm)),
            OverrideAction::Symlink
        );
    }

    #[test]
    fn the_three_sums_say_what_to_do_with_a_diverted_file() {
        use DivertAction::*;
        // live, package, local
        assert_eq!(divert_action(Some("p"), Some("l"), Some("l")), InPlace);
        assert_eq!(
            divert_action(Some("p"), Some("p"), Some("l")),
            PutBack { set_aside: true }
        );
        assert_eq!(
            divert_action(Some("p"), None, Some("l")),
            PutBack { set_aside: false }
        );
        assert_eq!(
            divert_action(Some("p"), Some("new"), Some("l")),
            RefreshLocal
        );
        assert_eq!(
            divert_action(Some("p"), Some("p"), Some("p")),
            InPlace,
            "diverted while still the package's"
        );
        assert!(matches!(
            divert_action(None, Some("l"), Some("old")),
            Unknown(_)
        ));
        assert!(matches!(
            divert_action(Some("p"), Some("p"), None),
            Broken(_)
        ));
        assert!(matches!(divert_action(Some("p"), None, None), Broken(_)));
        assert!(matches!(divert_action(None, None, None), Broken(_)));
        assert_eq!(
            divert_action(Some("p"), Some("l"), None),
            RefreshLocal,
            "the store was lost: rebuild it from yours"
        );
    }

    fn env<'a>(t: &'a Tmp, store: &'a Path, pkg: &'a dyn Fn(&str) -> Option<String>) -> Env<'a> {
        Env {
            owner_uid: me().0,
            root: &t.0,
            store,
            passwd: "",
            group: "",
            package_sha: pkg,
        }
    }

    #[test]
    fn applying_an_override_sets_the_mode_and_then_finds_it_in_effect() {
        let t = Tmp::new("ovr-apply");
        let f = t.write("usr/bin/tool", "x", 0o755);
        let store = t.0.join("store");
        let none = |_: &str| None;
        let (uid, gid) = me();
        let policy = Policy {
            overrides: vec![Override {
                path: "/usr/bin/tool".into(),
                mode: Some("750".into()),
                owner: Some(uid.to_string()),
                group: Some(gid.to_string()),
                note: None,
            }],
            diverts: vec![],
        };
        let first = apply(&policy, &env(&t, &store, &none));
        assert_eq!(first[0].kind, EventKind::Applied, "{first:?}");
        assert_eq!(fs::metadata(&f).unwrap().mode() & 0o7777, 0o750);
        let again = apply(&policy, &env(&t, &store, &none));
        assert_eq!(again[0].kind, EventKind::Already);
        // a file that is not there, and a name that does not exist
        let mut gone = policy.clone();
        gone.overrides[0].path = "/usr/bin/absent".into();
        assert_eq!(
            apply(&gone, &env(&t, &store, &none))[0].kind,
            EventKind::Skipped
        );
        let mut nobody = policy.clone();
        nobody.overrides[0].owner = Some("no-such-user".into());
        assert_eq!(
            apply(&nobody, &env(&t, &store, &none))[0].kind,
            EventKind::Failed
        );
    }

    #[test]
    fn a_diverted_file_is_put_back_after_pacman_replaces_it() {
        let t = Tmp::new("divert");
        let store = t.0.join("store");
        let live = t.write("usr/lib/y", "package v1\n", 0o644);
        let sha = |s: &str| {
            Sha256::digest(s.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        // you divert it after editing it
        fs::write(&live, "mine\n").unwrap();
        fs::set_permissions(&live, fs::Permissions::from_mode(0o640)).unwrap();
        let dir = divert_dir(&store, "/usr/lib/y");
        fs::create_dir_all(&dir).unwrap();
        replace_file(&live, &dir.join("local"), &fs::metadata(&live).unwrap()).unwrap();
        let policy = Policy {
            overrides: vec![],
            diverts: vec![Divert::new("/usr/lib/y")],
        };
        // the package's recorded sum is that of version 2, which pacman is about to write
        let v2 = sha("package v2\n");
        let pkg = |rel: &str| (rel == "usr/lib/y").then(|| v2.clone());
        // nothing happened yet: yours is in place
        assert_eq!(
            apply(&policy, &env(&t, &store, &pkg))[0].kind,
            EventKind::Already
        );
        // pacman upgrades the package
        fs::write(&live, "package v2\n").unwrap();
        fs::set_permissions(&live, fs::Permissions::from_mode(0o644)).unwrap();
        let ev = apply(&policy, &env(&t, &store, &pkg));
        assert_eq!(ev[0].kind, EventKind::Applied, "{ev:?}");
        assert_eq!(fs::read_to_string(&live).unwrap(), "mine\n");
        assert_eq!(
            fs::metadata(&live).unwrap().mode() & 0o7777,
            0o640,
            "your mode comes back too"
        );
        assert_eq!(
            fs::read_to_string(dir.join("package")).unwrap(),
            "package v2\n"
        );
        // you edit it again: the next run keeps the new edit as yours
        fs::write(&live, "mine, edited\n").unwrap();
        let ev = apply(&policy, &env(&t, &store, &pkg));
        assert_eq!(ev[0].kind, EventKind::Applied);
        assert_eq!(
            fs::read_to_string(dir.join("local")).unwrap(),
            "mine, edited\n"
        );
        // the package is removed (nothing owns the file): your copy waits in the store
        fs::remove_file(&live).unwrap();
        let gone = |_: &str| None;
        let ev = apply(&policy, &env(&t, &store, &gone));
        assert_eq!(ev[0].kind, EventKind::Skipped, "{ev:?}");
        assert!(ev[0].text.contains("waits in the store"));
        assert!(
            !live.exists(),
            "putting it back would make the package impossible to install again"
        );
        assert_eq!(
            fs::read_to_string(dir.join("local")).unwrap(),
            "mine, edited\n"
        );
        // the package is installed again and pacman writes its file: yours comes back
        fs::write(&live, "package v2\n").unwrap();
        let ev = apply(&policy, &env(&t, &store, &pkg));
        assert_eq!(ev[0].kind, EventKind::Applied, "{ev:?}");
        assert_eq!(fs::read_to_string(&live).unwrap(), "mine, edited\n");
        // no stray temporary file is left beside it
        let names: Vec<_> = fs::read_dir(live.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["y"]);
    }

    #[test]
    fn the_hook_names_every_path_without_the_leading_slash() {
        let p = Policy::parse(
            "[[override]]\npath = \"/usr/bin/x\"\nmode = \"755\"\n\n[[divert]]\npath = \"/usr/lib/y\"\n",
        )
        .unwrap();
        let hook = hook_text(&p, "/usr/bin/emrac");
        assert!(hook.contains("Target = usr/bin/x\n") && hook.contains("Target = usr/lib/y\n"));
        assert!(!hook.contains("Target = /"));
        // a diversion's removal is reported by the hook (apply puts nothing back for it)
        assert!(hook.contains("Operation = Remove"));
        assert!(hook.contains("When = PostTransaction"));
        assert!(hook.contains("Exec = /usr/bin/emrac policy-helper apply"));
        // and our own parser accepts what we write
        let parsed = crate::hooks::parse_hook(HOOK_FILE, &hook).unwrap();
        assert_eq!(parsed.triggers.len(), 2);
        // overrides alone have no Remove trigger
        let only = Policy::parse("[[override]]\npath = \"/usr/bin/x\"\nmode = \"755\"\n").unwrap();
        assert!(!hook_text(&only, "/usr/bin/emrac").contains("Remove"));
    }

    #[test]
    fn the_hook_state_tells_missing_stale_and_unwanted_apart() {
        use std::collections::HashMap;
        let exe = "/usr/bin/emrac";
        let read_from =
            |files: HashMap<&'static str, String>| move |name: &str| files.get(name).cloned();
        let only = Policy::parse("[[override]]\npath = \"/usr/bin/x\"\nmode = \"755\"\n").unwrap();
        let both = Policy::parse("[[override]]\npath = \"/usr/bin/x\"\nmode = \"755\"\n[[divert]]\npath = \"/usr/lib/y\"\n").unwrap();
        // an override needs only the post hook
        let want = hooks_wanted(&only, exe);
        assert_eq!(want.len(), 1);
        let files: HashMap<_, _> = want.iter().cloned().collect();
        assert_eq!(hook_state(&only, &read_from(files), exe), HookState::Ok);
        assert_eq!(
            hook_state(&only, &read_from(HashMap::new()), exe),
            HookState::Missing
        );
        assert_eq!(
            hook_state(
                &only,
                &read_from(HashMap::from([(HOOK_FILE, "old".to_string())])),
                exe
            ),
            HookState::Stale
        );
        // a diversion needs both
        let want = hooks_wanted(&both, exe);
        assert_eq!(want.len(), 2);
        assert_eq!(want[1].0, PRE_HOOK_FILE);
        let files: HashMap<_, _> = want.iter().cloned().collect();
        assert_eq!(
            hook_state(&both, &read_from(files.clone()), exe),
            HookState::Ok
        );
        let mut missing_pre = files.clone();
        missing_pre.remove(PRE_HOOK_FILE);
        assert_eq!(
            hook_state(&both, &read_from(missing_pre), exe),
            HookState::Stale
        );
        // a pre hook left behind after the last diversion went
        let post_only: HashMap<_, _> = hooks_wanted(&only, exe)
            .into_iter()
            .chain([(PRE_HOOK_FILE, "x".to_string())])
            .collect();
        assert_eq!(
            hook_state(&only, &read_from(post_only), exe),
            HookState::Stale
        );
        // another emrac path makes the hook stale
        let files: HashMap<_, _> = hooks_wanted(&only, exe).into_iter().collect();
        assert_eq!(
            hook_state(&only, &read_from(files), "/usr/local/bin/emrac"),
            HookState::Stale
        );
        // nothing wanted
        assert_eq!(
            hook_state(&Policy::default(), &read_from(HashMap::new()), exe),
            HookState::NotNeeded
        );
        assert_eq!(
            hook_state(
                &Policy::default(),
                &read_from(HashMap::from([(PRE_HOOK_FILE, "x".to_string())])),
                exe
            ),
            HookState::Unwanted
        );
    }

    #[test]
    fn the_pre_hook_covers_install_upgrade_and_remove_of_diverted_paths_only() {
        let p = Policy::parse("[[override]]\npath = \"/usr/bin/x\"\nmode = \"755\"\n[[divert]]\npath = \"/usr/lib/y\"\n").unwrap();
        let pre = pre_hook_text(&p, "/usr/bin/emrac").unwrap();
        assert!(pre.contains("When = PreTransaction"));
        assert!(pre.contains("Target = usr/lib/y\n") && !pre.contains("usr/bin/x"));
        assert!(pre.contains("Operation = Remove"));
        assert!(!pre.contains("AbortOnFail"), "never blocks a transaction");
        assert!(pre.contains("Exec = /usr/bin/emrac policy-helper capture"));
        crate::hooks::parse_hook(PRE_HOOK_FILE, &pre).unwrap();
        assert!(pre_hook_text(&Policy::default(), "/usr/bin/emrac").is_none());
    }

    #[test]
    fn an_edit_is_saved_before_pacman_can_overwrite_it() {
        let t = Tmp::new("before");
        let store = t.0.join("store");
        let live = t.write("usr/lib/y", "package v1\n", 0o644);
        let sha = |s: &str| {
            Sha256::digest(s.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let dir = divert_dir(&store, "/usr/lib/y");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("local"), "kept\n").unwrap();
        let policy = Policy {
            overrides: vec![],
            diverts: vec![Divert::new("/usr/lib/y")],
        };
        let v1 = sha("package v1\n");
        let pkg = |rel: &str| (rel == "usr/lib/y").then(|| v1.clone());
        // exactly what the package installed: nothing to save
        assert!(refresh_before(&policy, &env(&t, &store, &pkg)).is_empty());
        // you edited it: saved
        fs::write(&live, "edited since\n").unwrap();
        let ev = refresh_before(&policy, &env(&t, &store, &pkg));
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!(
            fs::read_to_string(dir.join("local")).unwrap(),
            "edited since\n"
        );
        // saved already: nothing more
        assert!(refresh_before(&policy, &env(&t, &store, &pkg)).is_empty());
        // gone: nothing to save
        fs::remove_file(&live).unwrap();
        assert!(refresh_before(&policy, &env(&t, &store, &pkg)).is_empty());
    }

    #[test]
    fn only_a_file_nobody_else_can_write_in_directories_nobody_else_can_write_is_trusted() {
        let t = Tmp::new("trust");
        let (uid, _) = me();
        let exe = t.write("bin/emrac", "", 0o755);
        fs::set_permissions(exe.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&t.0, fs::Permissions::from_mode(0o755)).unwrap();
        // a parent above the temp dir we do not control may be group writable, so test the pieces we do
        let ok = check_trust(&exe, uid, &[exe.as_path()]);
        if let Err(e) = &ok {
            assert!(!e.contains("emrac is not where"), "{e}"); // only an ancestor outside the test may object
        }
        // wrong place
        assert!(check_trust(&exe, uid, &[Path::new("/usr/bin/emrac")])
            .unwrap_err()
            .contains("not where root may run"));
        // someone else owns it
        assert!(check_trust(&exe, uid + 1, &[exe.as_path()])
            .unwrap_err()
            .contains("not owned by root"));
        // anyone can write it
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(check_trust(&exe, uid, &[exe.as_path()])
            .unwrap_err()
            .contains("writable"));
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(exe.parent().unwrap(), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(check_trust(&exe, uid, &[exe.as_path()])
            .unwrap_err()
            .contains("writable"));
    }

    #[test]
    fn loading_a_missing_file_is_an_empty_policy_and_a_broken_one_an_error() {
        let t = Tmp::new("load");
        assert!(load(&t.0.join("none.toml")).unwrap().is_empty());
        let bad = t.write(
            "bad.toml",
            "[[override]]\npath = \"x\"\nmode = \"1\"\n",
            0o644,
        );
        assert!(load(&bad).unwrap_err().contains("bad.toml"));
        let mut saved = Policy::default();
        saved.diverts.push(Divert::new("/usr/lib/y"));
        let f = t.write("good.toml", &saved.to_text(), 0o644);
        assert_eq!(load(&f).unwrap(), saved);
    }

    #[test]
    fn a_diversion_is_stale_only_when_the_package_changed_the_file_since() {
        assert_eq!(is_stale(Some("a"), Some("a")), Some(false));
        assert_eq!(is_stale(Some("a"), Some("b")), Some(true));
        assert_eq!(is_stale(None, Some("b")), None);
        assert_eq!(is_stale(Some("a"), None), None);
    }

    #[test]
    fn the_files_whose_fixes_matter_most_are_called_out() {
        assert!(divert_risks(0o644, b"plain text").is_empty());
        assert_eq!(divert_risks(0o644, b"\x7fELF\x02\x01").len(), 1);
        assert_eq!(divert_risks(0o4755, b"\x7fELF").len(), 2);
        assert_eq!(divert_risks(0o2755, b"#!/bin/sh").len(), 1);
    }

    #[test]
    fn a_package_change_set_aside_is_warned_about_every_time() {
        let t = Tmp::new("stale");
        let store = t.0.join("store");
        let live = t.write("usr/lib/y", "mine\n", 0o644);
        let sha = |s: &str| {
            Sha256::digest(s.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let dir = divert_dir(&store, "/usr/lib/y");
        fs::create_dir_all(&dir).unwrap();
        replace_file(&live, &dir.join("local"), &fs::metadata(&live).unwrap()).unwrap();
        let mut divert = Divert::new("/usr/lib/y");
        divert.base = Some(sha("package v1\n"));
        let policy = Policy {
            overrides: vec![],
            diverts: vec![divert.clone()],
        };
        // pacman writes v2, which differs from the version diverted against
        let v2 = sha("package v2\n");
        let pkg = |_: &str| Some(v2.clone());
        fs::write(&live, "package v2\n").unwrap();
        let ev = apply(&policy, &env(&t, &store, &pkg));
        assert_eq!(ev[0].kind, EventKind::Stale, "{ev:?}");
        assert!(ev[0].text.contains("security fixes") && ev[0].text.contains("divert diff"));
        // a reinstall of the very version it was diverted against is not a change
        let v1 = sha("package v1\n");
        let same = |_: &str| Some(v1.clone());
        fs::write(&live, "package v1\n").unwrap();
        let ev = apply(&policy, &env(&t, &store, &same));
        assert_eq!(ev[0].kind, EventKind::Applied, "{ev:?}");
    }

    #[test]
    fn root_does_not_act_on_files_in_directories_others_can_change() {
        let t = Tmp::new("location");
        let (uid, _) = me();
        let f = t.write("a/b/file", "x", 0o644);
        let (a, b) = (t.0.join("a"), t.0.join("a/b"));
        for d in [&a, &b] {
            fs::set_permissions(d, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(check_location(&t.0, &f, uid).is_ok());
        fs::set_permissions(&b, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(check_location(&t.0, &f, uid)
            .unwrap_err()
            .contains("writable by someone other than root"));
        fs::set_permissions(&b, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_location(&t.0, &f, uid + 1)
            .unwrap_err()
            .contains("not owned by root"));
        // a directory that does not exist yet is fine
        assert!(check_location(&t.0, &t.0.join("a/new/file"), uid).is_ok());

        // and applying refuses to touch such a file, for an override and for a diversion
        let store = t.0.join("store");
        let none = |_: &str| None;
        let policy = Policy {
            overrides: vec![Override {
                path: "/a/b/file".into(),
                mode: Some("600".into()),
                owner: None,
                group: None,
                note: None,
            }],
            diverts: vec![Divert::new("/a/b/file")],
        };
        fs::set_permissions(&b, fs::Permissions::from_mode(0o777)).unwrap();
        let ev = apply(&policy, &env(&t, &store, &none));
        assert!(
            ev.iter()
                .all(|e| e.kind == EventKind::Failed && e.text.contains("writable by someone")),
            "{ev:?}"
        );
        assert_eq!(
            fs::metadata(&f).unwrap().mode() & 0o777,
            0o644,
            "left alone"
        );
    }

    #[test]
    fn a_diversion_lives_under_an_id_of_its_path() {
        let a = divert_dir(Path::new("/s"), "/usr/lib/a");
        let b = divert_dir(Path::new("/s"), "/usr/lib/b");
        assert_ne!(a, b);
        assert_eq!(a, divert_dir(Path::new("/s"), "/usr/lib/a"));
        assert_eq!(a.parent(), Some(Path::new("/s")));
        assert_eq!(a.file_name().unwrap().len(), 16);
    }
}

//! Changing config files for `emrac pacnew`: validating the candidate, keeping
//! copies, replacing the file, recording it, and putting it back. See
//! `docs/plan/u30-pacnew-revert.md`.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::history::now;
use crate::pacnew::{self, Merge, Pending, Validation};
use crate::pacnew_log::{self as log, Record, Standing, State};
use crate::sources::Sources;

/// What to do with a pending file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacnewAction {
    /// Keep the live file; delete the `.pacnew` / `.pacsave`.
    Keep,
    /// Replace the live file with the `.pacnew`.
    Take,
    /// Replace the live file with the clean three-way merge.
    Merge,
    /// Replace the live file with text the user edited.
    Edit(String),
}

impl PacnewAction {
    fn name(&self) -> &'static str {
        match self {
            PacnewAction::Keep => "keep",
            PacnewAction::Take => "take",
            PacnewAction::Merge => "merge",
            PacnewAction::Edit(_) => "edit",
        }
    }
}

/// What a change did.
#[derive(Debug, Clone)]
pub struct Applied {
    pub id: String,
    /// The folder holding the copies of the live file and the `.pacnew`.
    pub backup_dir: Option<PathBuf>,
    /// What the candidate was checked with, if it could be.
    pub checked: Option<String>,
}

/// What a revert put back.
#[derive(Debug, Clone)]
pub struct Reverted {
    pub id: String,
    pub file: String,
    pub restored_live: bool,
    pub restored_pending: bool,
}

fn io_err(context: String) -> impl FnOnce(std::io::Error) -> Error {
    move |source| Error::Io { context, source }
}

impl Sources {
    fn sudo(&self, program: &str, args: &[&str]) -> Result<()> {
        self.pacman.run_sudo(
            program,
            &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        )
    }

    /// A file's bytes, asking `sudo` when the user can't read it.
    fn read_privileged(&self, path: &str) -> Result<Vec<u8>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(bytes),
            Err(_) => {
                let out = Command::new("sudo")
                    .args(["cat", "--", path])
                    .output()
                    .map_err(|e| Error::CommandSpawn("sudo".to_string(), e))?;
                if !out.status.success() {
                    return Err(Error::CommandFailed {
                        command: format!("sudo cat {path}"),
                        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
                    });
                }
                Ok(out.stdout)
            }
        }
    }

    /// The text that would become the live file under `action`, if any.
    pub fn pacnew_candidate(&self, file: &Pending, action: &PacnewAction) -> Option<String> {
        match action {
            PacnewAction::Keep => None,
            PacnewAction::Take => file.texts.theirs.clone(),
            PacnewAction::Merge => match &file.merged {
                Some(Merge::Clean(text)) => Some(text.clone()),
                _ => None,
            },
            PacnewAction::Edit(text) => Some(text.clone()),
        }
    }

    /// Checks the candidate for `action`; `Keep` has nothing to check.
    pub fn pacnew_validate(&self, file: &Pending, action: &PacnewAction) -> Result<Validation> {
        match self.pacnew_candidate(file, action) {
            Some(text) => pacnew::validate(&file.live, &text),
            None => Ok(Validation::NoValidator),
        }
    }

    /// Carries out the user's choice for one pending file: check the candidate,
    /// copy the live file and the `.pacnew` into `~/.local/share/emrac/`, replace
    /// the live file (written beside it and renamed over it) through `sudo`, and
    /// record all of it.
    pub fn pacnew_apply(&self, file: &Pending, action: PacnewAction) -> Result<Applied> {
        let candidate = self.pacnew_candidate(file, &action);
        if !matches!(action, PacnewAction::Keep) && candidate.is_none() {
            return Err(Error::Simulation(format!(
                "{} has nothing to {} (no clean merge or no new file)",
                file.live,
                action.name()
            )));
        }
        let mut checked = None;
        if let Some(text) = &candidate {
            if pacnew::has_conflict_markers(text) {
                return Err(Error::PacnewMarkers(file.live.clone()));
            }
            match pacnew::validate(&file.live, text)? {
                Validation::Invalid(message) => {
                    return Err(Error::PacnewInvalid {
                        file: file.live.clone(),
                        message,
                    })
                }
                Validation::Valid(what) => checked = Some(what),
                Validation::NoValidator => {}
            }
        }

        let home = std::env::var("HOME").map_err(|_| Error::NoHomeDir)?;
        let log_path = log::log_path()?;
        let id = log::next_id(&log::read(&log_path)?);
        let live_bytes = self.read_privileged(&file.live)?;
        let pending_bytes = self.read_privileged(&file.pending)?;
        let live_meta = std::fs::metadata(&file.live)
            .map_err(io_err(format!("couldn't read {}", file.live)))?;
        let pending_meta = std::fs::metadata(&file.pending)
            .map_err(io_err(format!("couldn't read {}", file.pending)))?;
        let before = log::sha256(&live_bytes)?;
        let after = match (&action, &candidate) {
            (PacnewAction::Keep, _) => before.clone(),
            (PacnewAction::Take, _) => log::sha256(&pending_bytes)?,
            (_, Some(text)) => log::sha256(text.as_bytes())?,
            (_, None) => before.clone(),
        };

        let stamp = format!("{}-{id}", now());
        let live_backup = pacnew::backup_path(Path::new(&home), &stamp, &file.live);
        let pending_backup = PathBuf::from(format!(
            "{}.pacnew",
            pacnew::backup_path(Path::new(&home), &stamp, &file.live).display()
        ));
        let mut record = Record {
            id: id.clone(),
            time: now(),
            file: file.live.clone(),
            package: file.package.clone(),
            action: action.name().to_string(),
            class: file.class.label().to_string(),
            before,
            after,
            mode: live_meta.mode() & 0o7777,
            uid: live_meta.uid(),
            gid: live_meta.gid(),
            live_backup: Some(live_backup.clone()),
            pending_backup: Some(pending_backup.clone()),
            pending: file.pending.clone(),
            pending_mode: pending_meta.mode() & 0o7777,
            pending_uid: pending_meta.uid(),
            pending_gid: pending_meta.gid(),
            state: State::Started,
            reverts: None,
            error: None,
        };
        log::append(&log_path, &record)?;

        let work = (|| -> Result<()> {
            write_private(&live_backup, &live_bytes)?;
            write_private(&pending_backup, &pending_bytes)?;
            match &action {
                PacnewAction::Keep => self.sudo("rm", &["-f", "--", &file.pending]),
                PacnewAction::Take => self.sudo("mv", &["-f", "--", &file.pending, &file.live]),
                PacnewAction::Merge | PacnewAction::Edit(_) => {
                    let text = candidate.as_deref().unwrap_or_default();
                    self.replace_file(
                        &file.live,
                        text.as_bytes(),
                        record.mode,
                        record.uid,
                        record.gid,
                    )?;
                    self.sudo("rm", &["-f", "--", &file.pending])
                }
            }
        })();
        match work {
            Ok(()) => {
                record.state = State::Done;
                log::append(&log_path, &record)?;
                Ok(Applied {
                    id,
                    backup_dir: live_backup.parent().map(Path::to_path_buf),
                    checked,
                })
            }
            Err(err) => {
                record.state = State::Failed;
                record.error = Some(err.to_string());
                let _ = log::append(&log_path, &record);
                Err(err)
            }
        }
    }

    /// Writes `bytes` beside `path` with the given mode and owner, then renames
    /// it over `path`, so the file is never half-written.
    fn replace_file(&self, path: &str, bytes: &[u8], mode: u32, uid: u32, gid: u32) -> Result<()> {
        let dir =
            std::env::temp_dir().join(format!("emrac-replace-{}-{}", std::process::id(), now()));
        write_private(&dir.join("new"), bytes)?;
        let tmp = dir.join("new").display().to_string();
        let beside = format!("{path}.emrac-new");
        let result = self
            .sudo(
                "install",
                &[
                    "-m",
                    &format!("{mode:o}"),
                    "-o",
                    &uid.to_string(),
                    "-g",
                    &gid.to_string(),
                    "--",
                    &tmp,
                    &beside,
                ],
            )
            .and_then(|()| self.sudo("mv", &["-f", "--", &beside, path]));
        let _ = std::fs::remove_dir_all(&dir);
        if result.is_err() {
            let _ = self.sudo("rm", &["-f", "--", &beside]);
        }
        result
    }

    /// Undoes the newest recorded change to `file` (any file when `None`):
    /// puts the saved live file back with its mode and owner, and the
    /// `.pacnew` if the change used it up. Refuses when the live file was
    /// edited since, unless `force`.
    pub fn pacnew_revert(&self, file: Option<&str>, force: bool) -> Result<Reverted> {
        let log_path = log::log_path()?;
        let all = log::read(&log_path)?;
        let target = all
            .iter()
            .rev()
            .filter(|r| r.state == State::Done && r.action != "revert")
            .filter(|r| file.is_none_or(|f| r.file == f || r.pending == f))
            .find(|r| log::standing(r, &all, Some("")) != Standing::Reverted)
            .cloned()
            .ok_or_else(|| {
                Error::PacnewNothingToRevert(file.map(|f| format!(" for {f}")).unwrap_or_default())
            })?;

        let now_bytes = self.read_privileged(&target.file)?;
        let now_hash = log::sha256(&now_bytes)?;
        if now_hash != target.after && !force {
            return Err(Error::PacnewDrift {
                file: target.file.clone(),
            });
        }

        let restore_live = target.action != "keep";
        let live_backup = target.live_backup.clone();
        let live_bytes = match (&live_backup, restore_live) {
            (Some(path), true) => Some(std::fs::read(path).map_err(io_err(format!(
                "the saved copy {} can't be read",
                path.display()
            )))?),
            _ => None,
        };
        let pending_bytes = match &target.pending_backup {
            Some(path) if !Path::new(&target.pending).exists() => {
                Some(std::fs::read(path).map_err(io_err(format!(
                    "the saved copy {} can't be read",
                    path.display()
                )))?)
            }
            _ => None,
        };

        let mut record = Record {
            id: log::next_id(&all),
            time: now(),
            action: "revert".to_string(),
            before: now_hash,
            after: target.before.clone(),
            state: State::Started,
            reverts: Some(target.id.clone()),
            error: None,
            ..target.clone()
        };
        log::append(&log_path, &record)?;

        let work = (|| -> Result<()> {
            if let Some(bytes) = &live_bytes {
                self.replace_file(&target.file, bytes, target.mode, target.uid, target.gid)?;
            }
            if let Some(bytes) = &pending_bytes {
                self.replace_file(
                    &target.pending,
                    bytes,
                    target.pending_mode,
                    target.pending_uid,
                    target.pending_gid,
                )?;
            }
            Ok(())
        })();
        match work {
            Ok(()) => {
                record.state = State::Done;
                log::append(&log_path, &record)?;
                Ok(Reverted {
                    id: target.id,
                    file: target.file,
                    restored_live: live_bytes.is_some(),
                    restored_pending: pending_bytes.is_some(),
                })
            }
            Err(err) => {
                record.state = State::Failed;
                record.error = Some(err.to_string());
                let _ = log::append(&log_path, &record);
                Err(err)
            }
        }
    }

    /// Every recorded change, oldest first, with how it stands now. Files the
    /// user can't read are not compared (no `sudo` is asked for here).
    pub fn pacnew_log(&self) -> Result<Vec<(Record, Standing)>> {
        let all = log::read(&log::log_path()?)?;
        let mut out = Vec::new();
        for record in &all {
            let now_hash = std::fs::read(&record.file)
                .ok()
                .and_then(|b| log::sha256(&b).ok());
            out.push((
                record.clone(),
                log::standing(record, &all, now_hash.as_deref()),
            ));
        }
        Ok(out)
    }
}

/// Writes `bytes` to `path` (its folders private, the file mode 0600).
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(parent) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(io_err(format!("couldn't create {}", parent.display())))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(io_err(format!("couldn't create {}", path.display())))?;
    f.write_all(bytes)
        .map_err(io_err(format!("couldn't write {}", path.display())))?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    Ok(())
}

//! A freshly synced, throwaway copy of pacman's databases, so an upgrade can be
//! *previewed* against the repositories as they are now — the way
//! pacman-contrib's `checkupdates` does it. The real database is never
//! touched and no root is needed: the new directory symlinks the real `local`
//! database and `fakeroot -- pacman -Sy --dbpath <dir>` fills its `sync/`.
//! See `docs/plan/u22-fresh-upgrade-view.md`.

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::backend::PacmanBackend;
use crate::error::{Error, Result};

pub struct FreshDb {
    path: PathBuf,
}

/// The real uid, from `/proc/self/status` (`Uid:\t1000\t…`).
fn current_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    parse_uid(&status)
}

fn parse_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Makes sure `dir` is a directory we own that nobody else can reach. It sits
/// in a world-writable place at a predictable name, so anything else there —
/// a symlink, someone else's directory — must be refused, not used.
pub(crate) fn ensure_private_dir(dir: &Path, uid: u32) -> std::result::Result<(), String> {
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(format!(
                    "{} exists and is not a plain directory",
                    dir.display()
                ));
            }
            if meta.uid() != uid {
                return Err(format!("{} belongs to another user", dir.display()));
            }
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(format!(
                    "{} is accessible to other users (mode {:o})",
                    dir.display(),
                    meta.permissions().mode() & 0o777
                ));
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("couldn't create {}: {e}", dir.display())),
        Err(err) => Err(format!("couldn't inspect {}: {err}", dir.display())),
    }
}

impl FreshDb {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Syncs a throwaway database and returns it. Needs the network and
    /// `fakeroot`; any failure is an `Err` the caller can fall back from.
    pub fn sync() -> Result<Self> {
        let fail = |why: String| Error::FreshSync(why);
        let uid =
            current_uid().ok_or_else(|| fail("couldn't determine the current user".into()))?;
        let real = PacmanBackend::new().db_path()?;
        let dir = std::env::temp_dir().join(format!("emrac-fresh-db-{uid}"));
        ensure_private_dir(&dir, uid).map_err(fail)?;

        // `local` must be the real installed-package database, whatever a
        // previous run left there.
        let link = dir.join("local");
        let target = real.join("local");
        if fs::read_link(&link).ok().as_deref() != Some(target.as_path()) {
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&target, &link)
                .map_err(|e| fail(format!("couldn't link the local database: {e}")))?;
        }
        let _ = fs::remove_file(dir.join("db.lck"));

        let run = |sandbox_flag: bool| {
            let mut cmd = Command::new("fakeroot");
            cmd.args(["--", "pacman", "-Sy"]);
            if sandbox_flag {
                cmd.arg("--disable-sandbox-filesystem");
            }
            cmd.arg("--dbpath")
                .arg(&dir)
                .args(["--logfile", "/dev/null"])
                .stdin(Stdio::null())
                .output()
        };
        let mut output = run(true).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => fail("`fakeroot` isn't installed".into()),
            _ => fail(format!("couldn't run fakeroot: {e}")),
        })?;
        // pacman older than 7.0 doesn't know the sandbox flag.
        if !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("unrecognized option")
        {
            output = run(false).map_err(|e| fail(format!("couldn't run fakeroot: {e}")))?;
        }
        let _ = fs::remove_file(dir.join("db.lck"));
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let last = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("it failed");
            return Err(fail(last.trim().to_string()));
        }
        Ok(Self { path: dir })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-freshdb-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_uid_is_read_from_proc_status_text() {
        assert_eq!(
            parse_uid("Name:\tx\nUid:\t1000\t1000\t1000\t1000\nGid:\t5\n"),
            Some(1000)
        );
        assert_eq!(parse_uid("Name:\tx\n"), None);
        assert!(current_uid().is_some());
    }

    #[test]
    fn a_missing_directory_is_created_private_and_accepted_again() {
        let uid = current_uid().unwrap();
        let dir = scratch("create");
        ensure_private_dir(&dir, uid).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        ensure_private_dir(&dir, uid).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_other_users_can_reach_is_refused() {
        let uid = current_uid().unwrap();
        let dir = scratch("open");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let err = ensure_private_dir(&dir, uid).unwrap_err();
        assert!(err.contains("accessible to other users"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlink_or_a_file_in_the_way_is_refused_not_followed() {
        let uid = current_uid().unwrap();
        let base = scratch("link");
        fs::create_dir(&base).unwrap();
        let target = base.join("elsewhere");
        fs::create_dir(&target).unwrap();
        let link = base.join("db");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = ensure_private_dir(&link, uid).unwrap_err();
        assert!(err.contains("not a plain directory"), "{err}");

        let file = base.join("file");
        fs::write(&file, "x").unwrap();
        assert!(ensure_private_dir(&file, uid)
            .unwrap_err()
            .contains("not a plain directory"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_directory_owned_by_someone_else_is_refused() {
        let uid = current_uid().unwrap();
        let dir = scratch("owner");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let err = ensure_private_dir(&dir, uid + 1).unwrap_err();
        assert!(err.contains("belongs to another user"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }
}

//! Protecting pacman's own database (u20, part 3; `additions.md` C6): a small
//! backup of `/var/lib/pacman/local` before each transaction, `pacman -Dk`
//! before and after, and a look at pacman's lock. See
//! `docs/plan/r2/u20-trust.md` and `docs/design/pacman-db.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// How many backups are kept.
pub const KEEP: usize = 3;
const DB_ROOT: &str = "/var/lib/pacman";
const LOCK: &str = "/var/lib/pacman/db.lck";

pub fn dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/pacman-db"))
}

fn err(msg: impl Into<String>) -> Error {
    Error::PacmanDb(msg.into())
}

/// `local-<unix>.tar.gz`
fn file_name(now: u64) -> String {
    format!("local-{now}.tar.gz")
}

/// The time in a backup's file name.
pub fn time_of(name: &str) -> Option<u64> {
    name.strip_prefix("local-")?
        .strip_suffix(".tar.gz")?
        .parse()
        .ok()
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub time: u64,
    pub bytes: u64,
}

/// The backups in `dir`, newest first.
pub fn backups_in(dir: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let time = time_of(&e.file_name().to_string_lossy())?;
                    Some(Backup {
                        bytes: e.metadata().ok()?.len(),
                        path: e.path(),
                        time,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|b| std::cmp::Reverse(b.time));
    out
}

pub fn backups() -> Vec<Backup> {
    dir().map(|d| backups_in(&d)).unwrap_or_default()
}

/// Makes a backup of `<root>/local` in `dest`, without the `mtree` files
/// (a fraction of the size, and only `pacman -Qkk` needs them), and removes
/// the oldest beyond `KEEP`. Returns the new backup.
pub fn backup_to(root: &Path, dest: &Path, now: u64) -> Result<Backup> {
    fs::create_dir_all(dest).map_err(|e| err(format!("{}: {e}", dest.display())))?;
    let path = dest.join(file_name(now));
    let part = path.with_extension("gz.part");
    let status = Command::new("tar")
        .arg("--exclude=mtree")
        .arg("-czf")
        .arg(&part)
        .arg("-C")
        .arg(root)
        .arg("local")
        .status()
        .map_err(|e| err(format!("couldn't run tar: {e}")))?;
    if !status.success() {
        let _ = fs::remove_file(&part);
        return Err(err(format!("tar failed ({status})")));
    }
    fs::rename(&part, &path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    for old in backups_in(dest).into_iter().skip(KEEP) {
        let _ = fs::remove_file(old.path);
    }
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(Backup {
        path,
        time: now,
        bytes,
    })
}

/// Backs up the real database.
pub fn backup(now: u64) -> Result<Backup> {
    backup_to(Path::new(DB_ROOT), &dir()?, now)
}

/// What `pacman -Dk` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consistency {
    pub ok: bool,
    /// The problems it reported, one per line.
    pub problems: Vec<String>,
}

/// Reads the output of `pacman -Dk`.
pub fn parse_dk(stdout: &str, stderr: &str, success: bool) -> Consistency {
    let problems: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("No database errors"))
        .map(str::to_string)
        .collect();
    Consistency {
        ok: success && problems.is_empty(),
        problems,
    }
}

/// Runs `pacman -Dk` (read-only, no root).
pub fn check() -> Consistency {
    match Command::new("pacman").arg("-Dk").output() {
        Ok(o) => parse_dk(
            &String::from_utf8_lossy(&o.stdout),
            &String::from_utf8_lossy(&o.stderr),
            o.status.success(),
        ),
        Err(e) => Consistency {
            ok: false,
            problems: vec![format!("couldn't run pacman -Dk: {e}")],
        },
    }
}

/// pacman's lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lock {
    #[default]
    Absent,
    /// The file is there and a pacman is running: wait for it.
    Held,
    /// The file is there and no pacman runs: left by a crash.
    Stale,
}

pub fn lock_from(file_exists: bool, pacman_running: bool) -> Lock {
    match (file_exists, pacman_running) {
        (false, _) => Lock::Absent,
        (true, true) => Lock::Held,
        (true, false) => Lock::Stale,
    }
}

fn pacman_running() -> bool {
    fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    && fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == "pacman")
            })
        })
        .unwrap_or(false)
}

pub fn lock() -> Lock {
    lock_from(Path::new(LOCK).exists(), pacman_running())
}

/// The commands that put a backup back (run as root; shown, never run).
pub fn restore_recipe(backup: &Path) -> String {
    format!(
        "sudo mv {root}/local {root}/local.broken && sudo tar -xzf {} -C {root} && pacman -Dk",
        backup.display(),
        root = DB_ROOT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-pacmandb-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_report_of_dk_is_read() {
        assert_eq!(
            parse_dk("No database errors have been found!\n", "", true),
            Consistency {
                ok: true,
                problems: vec![]
            }
        );
        let bad = parse_dk(
            "",
            "error: foo: missing file\nerror: bar: wrong version\n",
            false,
        );
        assert!(!bad.ok);
        assert_eq!(bad.problems.len(), 2);
        assert!(
            !parse_dk("", "", false).ok,
            "a failed run is not a clean database"
        );
    }

    #[test]
    fn the_lock_is_absent_held_or_stale() {
        assert_eq!(lock_from(false, true), Lock::Absent);
        assert_eq!(lock_from(true, true), Lock::Held);
        assert_eq!(lock_from(true, false), Lock::Stale);
    }

    #[test]
    fn a_backup_leaves_out_mtree_keeps_the_rest_and_only_the_newest_few_stay() {
        let d = scratch("backup");
        let root = d.join("pacman");
        let pkg = root.join("local/foo-1.0-1");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("desc"), "%NAME%\nfoo\n").unwrap();
        fs::write(pkg.join("files"), "%FILES%\nusr/bin/foo\n").unwrap();
        fs::write(pkg.join("mtree"), "big mtree").unwrap();
        fs::write(root.join("local/ALPM_DB_VERSION"), "9\n").unwrap();
        let dest = d.join("backups");
        for t in [100u64, 200, 300, 400, 500] {
            backup_to(&root, &dest, t).unwrap();
        }
        let kept = backups_in(&dest);
        assert_eq!(
            kept.iter().map(|b| b.time).collect::<Vec<_>>(),
            [500, 400, 300]
        );
        let list = Command::new("tar")
            .arg("-tzf")
            .arg(&kept[0].path)
            .output()
            .unwrap();
        let names = String::from_utf8_lossy(&list.stdout).into_owned();
        assert!(
            names.contains("local/foo-1.0-1/desc") && names.contains("local/foo-1.0-1/files"),
            "{names}"
        );
        assert!(names.contains("ALPM_DB_VERSION"));
        assert!(!names.contains("mtree"), "{names}");
        assert!(kept[0].bytes > 0);
        assert!(
            backup_to(&d.join("no-such-root"), &dest, 600).is_err(),
            "a failed tar leaves no file"
        );
        assert!(!dest.join("local-600.tar.gz").exists());
        assert!(
            !dest.join("local-600.tar.gz.part").exists()
                && !dest.join("local-600.gz.part").exists()
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn names_carry_their_time_and_the_recipe_names_the_file() {
        assert_eq!(time_of("local-1792022400.tar.gz"), Some(1_792_022_400));
        assert_eq!(time_of("local-x.tar.gz"), None);
        assert_eq!(time_of("other.tar.gz"), None);
        let r = restore_recipe(Path::new("/home/u/b/local-1.tar.gz"));
        assert!(
            r.contains("local.broken") && r.contains("local-1.tar.gz") && r.contains("pacman -Dk")
        );
    }
}

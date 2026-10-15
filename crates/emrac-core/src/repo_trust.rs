//! Repository trust, a slice of C5 (u20, part 4): a repository database that
//! is older than one already seen (a mirror serving old or rolled-back data),
//! and a plan that would install an older version than the installed one.
//! See `docs/plan/r2/u20-trust.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// A database this much older than the newest seen is called rolled back.
pub const TOLERANCE: u64 = 86_400;
const SYNC_DIR: &str = "/var/lib/pacman/sync";

pub type Marks = BTreeMap<String, u64>;

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/repo-marks.json"))
}

pub fn load() -> Marks {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(marks: &Marks) {
    let Ok(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.part");
    if std::fs::write(
        &tmp,
        serde_json::to_string_pretty(marks).expect("serializable"),
    )
    .is_ok()
    {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// Modification time of each `<repo>.db` in `dir` (the server's own time,
/// which pacman keeps on the file).
pub fn current_in(dir: &Path) -> Marks {
    let mut out = Marks::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(repo) = name.strip_suffix(".db") else {
                continue;
            };
            if let Some(t) = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            {
                out.insert(repo.to_string(), t.as_secs());
            }
        }
    }
    out
}

pub fn current() -> Marks {
    current_in(Path::new(SYNC_DIR))
}

/// A repository whose database is older than the newest one seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rollback {
    pub repo: String,
    pub seen: u64,
    pub now: u64,
}

pub fn rollbacks(marks: &Marks, current: &Marks) -> Vec<Rollback> {
    current
        .iter()
        .filter_map(|(repo, now)| {
            let seen = *marks.get(repo)?;
            (now + TOLERANCE < seen).then(|| Rollback {
                repo: repo.clone(),
                seen,
                now: *now,
            })
        })
        .collect()
}

/// The marks after looking at `current`: each rises to the newest seen. A
/// rolled-back repository keeps its old (higher) mark.
pub fn raised(marks: &Marks, current: &Marks) -> Marks {
    let mut out = marks.clone();
    for (repo, now) in current {
        let e = out.entry(repo.clone()).or_insert(*now);
        *e = (*e).max(*now);
    }
    out
}

/// Packages that would be installed in an older version than the installed
/// one: `(name, installed, planned)`. `older(a, b)` says `a` is older than `b`.
pub fn downgrades(
    planned: &[(String, String)],
    installed: &dyn Fn(&str) -> Option<String>,
    older: &dyn Fn(&str, &str) -> bool,
) -> Vec<(String, String, String)> {
    planned
        .iter()
        .filter_map(|(name, version)| {
            let have = installed(name)?;
            older(version, &have).then(|| (name.clone(), have, version.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(pairs: &[(&str, u64)]) -> Marks {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn a_database_much_older_than_one_seen_is_a_rollback_and_a_small_lag_is_not() {
        let seen = marks(&[("core", 1_000_000), ("extra", 1_000_000)]);
        let now = marks(&[
            ("core", 1_000_000 - 2 * TOLERANCE),
            ("extra", 1_000_000 - 3600),
            ("new", 5),
        ]);
        let r = rollbacks(&seen, &now);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].repo, "core");
        assert_eq!(
            (r[0].seen, r[0].now),
            (1_000_000, 1_000_000 - 2 * TOLERANCE)
        );
        assert!(
            rollbacks(&Marks::new(), &now).is_empty(),
            "nothing seen yet"
        );
    }

    #[test]
    fn marks_only_rise() {
        let seen = marks(&[("core", 100), ("extra", 500)]);
        let now = marks(&[("core", 200), ("extra", 100), ("multilib", 7)]);
        let up = raised(&seen, &now);
        assert_eq!(up, marks(&[("core", 200), ("extra", 500), ("multilib", 7)]));
    }

    #[test]
    fn database_times_are_read_from_the_files() {
        let d = std::env::temp_dir().join(format!("emrac-repotrust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("core.db"), b"x").unwrap();
        std::fs::write(d.join("core.db.sig"), b"x").unwrap();
        std::fs::write(d.join("README"), b"x").unwrap();
        let m = current_in(&d);
        assert_eq!(m.keys().collect::<Vec<_>>(), ["core"]);
        assert!(m["core"] > 1_700_000_000);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plan_that_installs_an_older_version_is_found() {
        let planned = vec![
            ("a".to_string(), "1.0-1".to_string()),
            ("b".to_string(), "3.0-1".to_string()),
            ("c".to_string(), "1.0-1".to_string()),
        ];
        let installed = |n: &str| match n {
            "a" => Some("2.0-1".to_string()),
            "b" => Some("2.0-1".to_string()),
            _ => None,
        };
        let older = |a: &str, b: &str| {
            let n = |s: &str| s.split('-').next().unwrap().parse::<f64>().unwrap();
            n(a) < n(b)
        };
        let d = downgrades(&planned, &installed, &older);
        assert_eq!(
            d,
            vec![("a".to_string(), "2.0-1".to_string(), "1.0-1".to_string())]
        );
    }
}

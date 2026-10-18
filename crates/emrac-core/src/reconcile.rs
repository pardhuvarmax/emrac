//! After a transaction that failed or was cut off: packages pacman still lists
//! whose files are not all there.
//!
//! pacman writes a package's database entry after (or, when removing, before)
//! its files. Run against a full disk, a failing extraction still left the
//! package recorded as installed with a file missing, and the run ended with
//! "no packages were upgraded" (`research/pacman-commit-order.md`). Nothing
//! tells you; the package just misbehaves later. This looks at the packages
//! the transaction named and says which are in that state and how to mend them.
//! See `docs/plan/r3/u6-reconcile.md`.

use std::path::Path;

use serde::Serialize;

use crate::backend::{AlpmBackend, PacmanBackend};
use crate::plan::{Plan, PlanAction};
use crate::verify::no_extract_excludes;

/// A package pacman lists although it is not whole.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Damaged {
    pub package: String,
    pub version: String,
    /// What the transaction was doing to it.
    pub removing: bool,
    /// Files pacman records for it that are not on disk (up to a handful shown).
    pub missing: Vec<String>,
    /// How many are missing in all.
    pub missing_count: usize,
    pub total_files: usize,
}

impl Damaged {
    /// One sentence for the user.
    pub fn headline(&self) -> String {
        let shown = self.missing.join(", ");
        let more = self.missing_count.saturating_sub(self.missing.len());
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        let (is, gone) = if self.missing_count == 1 {
            ("is", "is already gone")
        } else {
            ("are", "are already gone")
        };
        if self.removing {
            format!(
                "pacman still lists {} {} although {} of its {} files {gone} ({shown}{tail}): the removal did not finish",
                self.package, self.version, self.missing_count, self.total_files
            )
        } else {
            format!(
                "pacman lists {} {} as installed, but {} of its {} files {is} missing ({shown}{tail})",
                self.package, self.version, self.missing_count, self.total_files
            )
        }
    }

    /// The command that mends it.
    pub fn fix(&self) -> String {
        if self.removing {
            format!("emrac remove {}", self.package)
        } else {
            format!("emrac reinstall {}", self.package)
        }
    }
}

/// Which of `files` (as pacman records them, directories ending in `/`) are
/// not under `root`. Directories, `backup=()` config files (a missing one may
/// be deliberate) and anything `NoExtract` leaves out are not counted.
pub(crate) fn missing_files(
    root: &Path,
    files: &[String],
    backup: &[String],
    no_extract: &[String],
) -> Vec<String> {
    files
        .iter()
        .filter(|f| !f.ends_with('/'))
        .filter(|f| !backup.iter().any(|b| b == *f))
        .filter(|f| !no_extract_excludes(no_extract, f))
        .filter(|f| root.join(f).symlink_metadata().is_err())
        .cloned()
        .collect()
}

/// The packages `plan` named that are recorded but not whole. Read-only; if
/// pacman's configuration cannot be read the answer is "none found".
pub fn after_failure(plan: &Plan) -> Vec<Damaged> {
    let Ok(alpm) = AlpmBackend::init() else {
        return Vec::new();
    };
    let no_extract = PacmanBackend::new().no_extract().unwrap_or_default();
    let root = alpm.root();
    let removing = matches!(plan.action, PlanAction::Remove | PlanAction::Purge);
    let mut out = Vec::new();
    for planned in &plan.packages {
        let Some(recorded) = alpm.recorded_files(&planned.name) else {
            continue;
        };
        let missing = missing_files(
            Path::new(&root),
            &recorded.files,
            &recorded.backup,
            &no_extract,
        );
        if missing.is_empty() {
            continue;
        }
        out.push(Damaged {
            package: planned.name.clone(),
            version: recorded.version,
            removing,
            missing_count: missing.len(),
            total_files: recorded.files.iter().filter(|f| !f.ends_with('/')).count(),
            missing: missing.into_iter().take(3).collect(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("emrac-reconcile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_regular_files_that_should_be_there_count() {
        let root = temp("missing");
        std::fs::create_dir_all(root.join("usr/share/tp")).unwrap();
        std::fs::write(root.join("usr/share/tp/here"), "x").unwrap();
        std::os::unix::fs::symlink("nowhere", root.join("usr/share/tp/dangling")).unwrap();
        let files: Vec<String> = [
            "usr/share/tp/",
            "usr/share/tp/here",
            "usr/share/tp/dangling", // a symlink counts as present, even dangling
            "usr/share/tp/gone",
            "usr/share/doc/tp/readme", // NoExtract leaves it out on purpose
            "etc/tp.conf",             // a config file: may be deleted deliberately
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let got = missing_files(
            &root,
            &files,
            &["etc/tp.conf".to_string()],
            &["usr/share/doc/*".to_string()],
        );
        assert_eq!(got, ["usr/share/tp/gone"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_message_and_the_fix_depend_on_what_was_happening() {
        let mut d = Damaged {
            package: "tp".into(),
            version: "1-1".into(),
            removing: false,
            missing: vec!["usr/share/tp/a.txt".into()],
            missing_count: 4,
            total_files: 9,
        };
        let text = d.headline();
        assert!(
            text.contains("as installed, but 4 of its 9 files are missing"),
            "{text}"
        );
        assert!(text.contains("usr/share/tp/a.txt and 3 more"));
        assert_eq!(d.fix(), "emrac reinstall tp");
        d.removing = true;
        assert!(d.headline().contains("the removal did not finish"));
        assert_eq!(d.fix(), "emrac remove tp");
    }
}

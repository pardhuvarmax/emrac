//! Choosing where `emrac reinstall` gets each package from. Pure logic; the
//! lookups themselves live in `Sources`.

use std::path::PathBuf;

use serde::Serialize;

use crate::plan::Plan;
use crate::rollback::Download;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReinstallSource {
    /// A package file for exactly the installed version is already on disk.
    File,
    /// The repositories still carry the installed version.
    Repository,
    /// Downloaded from the Arch Linux Archive after confirmation.
    Archive,
    /// Rebuilt from emrac's existing (already reviewed) AUR checkout.
    Rebuild,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinstallItem {
    pub name: String,
    pub version: String,
    pub source: ReinstallSource,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinstallPlan {
    pub plan: Plan,
    pub items: Vec<ReinstallItem>,
    /// Package files for `pacman -U` (sources `File` and `Archive`).
    pub files: Vec<PathBuf>,
    /// Names for `pacman -S` (source `Repository`).
    pub repository: Vec<String>,
    /// AUR packages to rebuild (source `Rebuild`).
    pub rebuild: Vec<String>,
    /// The subset of `files` that still has to be downloaded first.
    pub downloads: Vec<Download>,
}

/// Where to get a package of the installed version from, or why nowhere.
///
/// Never the repositories' *current* version unless it is the installed one:
/// reinstalling must not turn into a partial upgrade.
pub(crate) fn choose_source(
    foreign: bool,
    installed: &str,
    sync_version: Option<&str>,
    has_local_file: bool,
    archive_allowed: bool,
    in_archive: impl FnOnce() -> bool,
    has_checkout: bool,
) -> Result<ReinstallSource, String> {
    if has_local_file {
        return Ok(ReinstallSource::File);
    }
    if foreign {
        return if has_checkout {
            Ok(ReinstallSource::Rebuild)
        } else {
            Err(
                "there's no cached build of the AUR package; run `emrac install` to build it"
                    .to_string(),
            )
        };
    }
    if sync_version == Some(installed) {
        return Ok(ReinstallSource::Repository);
    }
    if archive_allowed && in_archive() {
        return Ok(ReinstallSource::Archive);
    }
    let archive = if archive_allowed {
        "it isn't available from the Arch Linux Archive"
    } else {
        "the Arch Linux Archive wasn't checked (--offline)"
    };
    Err(match sync_version {
        Some(sync) => {
            format!("{installed} isn't cached, the repositories now have {sync}, and {archive}")
        }
        None => format!("{installed} isn't cached and {archive}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(
        foreign: bool,
        sync: Option<&str>,
        file: bool,
        archive: bool,
        checkout: bool,
    ) -> Result<ReinstallSource, String> {
        choose_source(foreign, "2-1", sync, file, true, || archive, checkout)
    }

    #[test]
    fn a_local_file_always_wins() {
        for foreign in [false, true] {
            assert_eq!(
                pick(foreign, Some("3-1"), true, true, true),
                Ok(ReinstallSource::File)
            );
        }
    }

    #[test]
    fn official_uses_the_repository_only_when_it_still_has_the_installed_version() {
        assert_eq!(
            pick(false, Some("2-1"), false, false, false),
            Ok(ReinstallSource::Repository)
        );
        // Repo moved on: -S would upgrade, so go to the Archive instead.
        assert_eq!(
            pick(false, Some("3-1"), false, true, false),
            Ok(ReinstallSource::Archive)
        );
    }

    #[test]
    fn official_with_nothing_available_explains_why() {
        let err = pick(false, Some("3-1"), false, false, false).unwrap_err();
        assert!(err.contains("2-1") && err.contains("3-1"), "{err}");
        let err = pick(false, None, false, false, false).unwrap_err();
        assert!(err.contains("isn't cached"), "{err}");
    }

    #[test]
    fn foreign_rebuilds_from_an_existing_checkout_or_fails() {
        assert_eq!(
            pick(true, None, false, true, true),
            Ok(ReinstallSource::Rebuild)
        );
        let err = pick(true, None, false, true, false).unwrap_err();
        assert!(err.contains("emrac install"), "{err}");
    }

    #[test]
    fn the_archive_is_not_consulted_when_not_needed() {
        let r = choose_source(
            false,
            "2-1",
            Some("2-1"),
            false,
            true,
            || panic!("consulted"),
            false,
        );
        assert_eq!(r, Ok(ReinstallSource::Repository));
        let r = choose_source(true, "2-1", None, false, true, || panic!("consulted"), true);
        assert_eq!(r, Ok(ReinstallSource::Rebuild));
        let r = choose_source(
            false,
            "2-1",
            Some("9-1"),
            true,
            true,
            || panic!("consulted"),
            false,
        );
        assert_eq!(r, Ok(ReinstallSource::File));
    }

    #[test]
    fn offline_never_consults_the_archive_and_says_so() {
        let r = choose_source(
            false,
            "2-1",
            Some("3-1"),
            false,
            false,
            || panic!("consulted"),
            false,
        );
        let err = r.unwrap_err();
        assert!(err.contains("--offline") && err.contains("3-1"), "{err}");
    }
}

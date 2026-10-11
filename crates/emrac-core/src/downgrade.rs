//! Choosing what `emrac downgrade` should go back to. Pure logic over
//! version strings; the file/Archive lookups live in `Sources`.

use std::cmp::Ordering;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::plan::Plan;
use crate::rollback::Download;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct DowngradePlan {
    pub plan: Plan,
    /// Package files for `pacman -U`.
    pub files: Vec<PathBuf>,
    /// The subset of `files` that still has to be downloaded first.
    pub downloads: Vec<Download>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListedVersion {
    pub version: String,
    /// Where a file for it can be had: `local`, `archive`, or `local+archive`.
    pub source: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionList {
    pub package: String,
    pub installed: Option<String>,
    /// Newest first.
    pub versions: Vec<ListedVersion>,
}

pub(crate) fn vercmp(a: &str, b: &str) -> Ordering {
    alpm::vercmp(a, b)
}

/// The version to go back to. With an explicit `requested` it only has to
/// be older than `installed`; without one, it is the newest of `available`
/// that is older. (File availability for an explicit version is checked
/// where the files are looked up.)
pub(crate) fn choose_target(
    pkg: &str,
    installed: &str,
    requested: Option<&str>,
    available: &[String],
) -> Result<String> {
    match requested {
        Some(requested) => {
            if vercmp(requested, installed) != Ordering::Less {
                return Err(Error::NotOlder {
                    pkg: pkg.to_string(),
                    installed: installed.to_string(),
                    requested: requested.to_string(),
                });
            }
            Ok(requested.to_string())
        }
        None => available
            .iter()
            .filter(|v| vercmp(v, installed) == Ordering::Less)
            .max_by(|a, b| vercmp(a, b))
            .cloned()
            .ok_or_else(|| Error::NoOlderVersion(pkg.to_string())),
    }
}

/// Merges where each version can be had from, newest first.
pub(crate) fn merge_versions(
    pkg: &str,
    installed: Option<&str>,
    local: &[String],
    archive: &[String],
) -> VersionList {
    let mut all: Vec<&String> = local.iter().chain(archive.iter()).collect();
    all.sort_by(|a, b| vercmp(b, a));
    all.dedup();

    let versions = all
        .into_iter()
        .map(|version| {
            let source = match (local.contains(version), archive.contains(version)) {
                (true, true) => "local+archive",
                (true, false) => "local",
                _ => "archive",
            };
            ListedVersion {
                version: version.clone(),
                source: source.to_string(),
                installed: installed == Some(version.as_str()),
            }
        })
        .collect();

    VersionList {
        package: pkg.to_string(),
        installed: installed.map(str::to_string),
        versions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn without_a_version_it_is_the_newest_older_one() {
        let avail = v(&["1.8.0-2", "1.9-1", "1.10-1", "2.3.2-1", "3.0-1"]);
        assert_eq!(
            choose_target("t", "2.3.2-1", None, &avail).unwrap(),
            "1.10-1"
        );
        // 1.10 > 1.9 numerically, not lexically.
        assert_eq!(choose_target("t", "1.10-1", None, &avail).unwrap(), "1.9-1");
    }

    #[test]
    fn epochs_and_pkgrel_order_correctly() {
        let avail = v(&["5.0-1", "1:1.0-1", "1:1.0-2"]);
        assert_eq!(
            choose_target("t", "1:1.0-2", None, &avail).unwrap(),
            "1:1.0-1"
        );
        assert_eq!(
            choose_target("t", "1:1.0-1", None, &avail).unwrap(),
            "5.0-1"
        );
    }

    #[test]
    fn nothing_older_is_an_error() {
        let avail = v(&["2.0-1", "3.0-1"]);
        assert!(matches!(
            choose_target("t", "2.0-1", None, &avail),
            Err(Error::NoOlderVersion(_))
        ));
        assert!(matches!(
            choose_target("t", "2.0-1", None, &[]),
            Err(Error::NoOlderVersion(_))
        ));
    }

    #[test]
    fn an_explicit_version_must_be_older() {
        assert_eq!(
            choose_target("t", "2.0-1", Some("1.0-1"), &[]).unwrap(),
            "1.0-1"
        );
        for requested in ["2.0-1", "2.0-2", "3.0-1", "1:0.1-1"] {
            assert!(
                matches!(
                    choose_target("t", "2.0-1", Some(requested), &[]),
                    Err(Error::NotOlder { .. })
                ),
                "{requested}"
            );
        }
    }

    #[test]
    fn merged_list_is_newest_first_with_sources_and_installed_marker() {
        let list = merge_versions(
            "t",
            Some("2.0-1"),
            &v(&["1.0-1", "2.0-1"]),
            &v(&["1.0-1", "1.5-1", "2.0-1", "3.0-1"]),
        );
        let got: Vec<_> = list
            .versions
            .iter()
            .map(|e| (e.version.as_str(), e.source.as_str(), e.installed))
            .collect();
        assert_eq!(
            got,
            [
                ("3.0-1", "archive", false),
                ("2.0-1", "local+archive", true),
                ("1.5-1", "archive", false),
                ("1.0-1", "local+archive", false),
            ]
        );
    }
}

//! Packages that were installed only to build something, and can go again.
//!
//! `makepkg -s` installs an AUR package's build dependencies and leaves them.
//! After a build, emrac knows what the build added; this module says which of
//! those nothing needs any more. See `docs/plan/r3/u7-build-tools.md`.

use std::collections::HashSet;

use serde::Serialize;

/// A package that appeared during a build, as a dependency.
#[derive(Debug, Clone)]
pub(crate) struct NewDependency {
    pub name: String,
    pub version: String,
    pub size: u64,
    /// Installed packages that require it or list it as an optional
    /// dependency.
    pub dependents: Vec<String>,
}

/// A build tool that can be removed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuildTool {
    pub name: String,
    pub version: String,
    pub size: u64,
}

/// Of `added`, the ones that can be removed together: nothing outside the
/// removed set depends on them. A package whose only dependents are other
/// candidates goes with them (`cmake`, then the library only `cmake` needed);
/// one the freshly built package needs stays, because that package is installed
/// by now and is one of its dependents.
pub(crate) fn removable(added: &[NewDependency]) -> Vec<BuildTool> {
    let mut set: HashSet<&str> = added.iter().map(|d| d.name.as_str()).collect();
    loop {
        let keep: Vec<&str> = added
            .iter()
            .filter(|d| set.contains(d.name.as_str()))
            .filter(|d| d.dependents.iter().any(|dep| !set.contains(dep.as_str())))
            .map(|d| d.name.as_str())
            .collect();
        if keep.is_empty() {
            break;
        }
        for name in keep {
            set.remove(name);
        }
    }
    let mut out: Vec<BuildTool> = added
        .iter()
        .filter(|d| set.contains(d.name.as_str()))
        .map(|d| BuildTool {
            name: d.name.clone(),
            version: d.version.clone(),
            size: d.size,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(name: &str, dependents: &[&str]) -> NewDependency {
        NewDependency {
            name: name.into(),
            version: "1-1".into(),
            size: 100,
            dependents: dependents.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(tools: &[BuildTool]) -> Vec<&str> {
        tools.iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn nothing_depending_on_it_means_it_can_go() {
        assert_eq!(names(&removable(&[dep("cmake", &[])])), ["cmake"]);
    }

    #[test]
    fn what_only_a_removed_tool_needed_goes_with_it() {
        let added = [dep("cmake", &[]), dep("libuv", &["cmake"])];
        assert_eq!(names(&removable(&added)), ["cmake", "libuv"]);
    }

    #[test]
    fn what_the_built_package_needs_stays_and_so_does_what_hangs_on_it() {
        // `libfoo` is a runtime dependency of the package just built; `libbar` only exists for libfoo.
        let added = [
            dep("cmake", &[]),
            dep("libfoo", &["built-pkg"]),
            dep("libbar", &["libfoo"]),
        ];
        assert_eq!(names(&removable(&added)), ["cmake"]);
    }

    #[test]
    fn something_shared_with_a_kept_tool_stays() {
        // python is needed by cmake (removable) and by the built package: it stays.
        let added = [dep("cmake", &[]), dep("python", &["cmake", "built-pkg"])];
        assert_eq!(names(&removable(&added)), ["cmake"]);
    }

    #[test]
    fn a_ring_of_tools_that_only_need_each_other_goes_together() {
        let added = [dep("a", &["b"]), dep("b", &["a"])];
        assert_eq!(names(&removable(&added)), ["a", "b"]);
    }

    #[test]
    fn nothing_added_nothing_proposed() {
        assert!(removable(&[]).is_empty());
    }
}

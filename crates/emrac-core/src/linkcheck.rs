//! Link-check for software built outside the package manager (u25,
//! `additions.md` B8): before an upgrade or a removal changes a shared library,
//! the programs nobody packaged (`/usr/local`, `~/.local`, unowned files under
//! `/opt`) are read for the libraries they need, and any that the change would take
//! away are named in the preview with the package change that causes it.
//!
//! Package-level soname checks cannot see these programs: no package depends on
//! them. Only ELF headers are read; nothing is run.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::stale_builds::{missing_libs, read_elf};

/// A program or library that no package owns, and what it needs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Binary {
    pub path: String,
    pub needed: Vec<String>,
    pub search: Vec<String>,
}

const MAX_FILES: usize = 8000;
const MAX_DEPTH: usize = 6;

/// ELF files under `roots`. `skip` says which paths a package owns (relative to
/// `base`): those are not candidates.
pub fn candidates(base: &Path, roots: &[PathBuf], skip: &dyn Fn(&str) -> bool) -> Vec<Binary> {
    let mut out = Vec::new();
    let mut seen = 0usize;
    for root in roots {
        walk(base, root, 0, skip, &mut out, &mut seen);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk(
    base: &Path,
    dir: &Path,
    depth: usize,
    skip: &dyn Fn(&str) -> bool,
    out: &mut Vec<Binary>,
    seen: &mut usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if *seen >= MAX_FILES {
            return;
        }
        let path = e.path();
        let Ok(m) = path.symlink_metadata() else {
            continue;
        };
        if m.is_dir() {
            walk(base, &path, depth + 1, skip, out, seen);
        } else if m.is_file() && m.len() > 64 {
            *seen += 1;
            let rel = path
                .strip_prefix(base)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            if skip(&rel) {
                continue;
            }
            if let Some(info) = read_elf(&path) {
                if !info.needed.is_empty() {
                    out.push(Binary {
                        path: format!("/{rel}"),
                        needed: info.needed,
                        search: info.search,
                    });
                }
            }
        }
    }
}

/// The file name a soname provide stands for: `libfoo.so=1-64` is `libfoo.so.1`.
pub fn soname_file(provide: &str) -> Option<String> {
    let (name, rest) = provide.split_once('=')?;
    if !name.contains(".so") {
        return None;
    }
    let version = rest.split('-').next()?;
    (!version.is_empty()).then(|| format!("{name}.{version}"))
}

/// A library a package change takes away.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Change {
    pub package: String,
    pub from: String,
    /// `None`: the package is removed.
    pub to: Option<String>,
    /// The soname file (`libfoo.so.1`).
    pub lib: String,
    /// The provide it came from (`libfoo.so=1-64`).
    pub provide: String,
}

/// The sonames `old` provides that `new` does not.
pub fn lost(
    package: &str,
    from: &str,
    to: Option<&str>,
    old: &[String],
    new: &[String],
) -> Vec<Change> {
    old.iter()
        .filter(|p| !new.contains(p))
        .filter_map(|p| {
            Some(Change {
                package: package.to_string(),
                from: from.to_string(),
                to: to.map(str::to_string),
                lib: soname_file(p)?,
                provide: p.clone(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Hit {
    pub binary: String,
    pub change: Change,
}

/// The programs that need a library a change takes away. `elsewhere(provide)`
/// says another installed package provides it, so it stays.
pub fn hits(binaries: &[Binary], changes: &[Change], elsewhere: &dyn Fn(&str) -> bool) -> Vec<Hit> {
    let mut out = Vec::new();
    for b in binaries {
        for c in changes {
            if b.needed.iter().any(|n| n == &c.lib) && !elsewhere(&c.provide) {
                out.push(Hit {
                    binary: b.path.clone(),
                    change: c.clone(),
                });
            }
        }
    }
    out
}

/// One line per hit for a plan's warnings (at most `limit`, then a count).
pub fn warning_lines(hits: &[Hit], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = hits
        .iter()
        .take(limit)
        .map(|h| {
            let what = match &h.change.to {
                Some(to) => format!(
                    "{} {} \u{2192} {} no longer provides {}",
                    h.change.package, h.change.from, to, h.change.lib
                ),
                None => format!(
                    "removing {} {} takes {} away",
                    h.change.package, h.change.from, h.change.lib
                ),
            };
            format!(
                "{} is not from a package and needs {}: {what}",
                h.binary, h.change.lib
            )
        })
        .collect();
    if hits.len() > limit {
        out.push(format!(
            "and {} more program{} outside the package manager would lose a library",
            hits.len() - limit,
            if hits.len() - limit == 1 { "" } else { "s" }
        ));
    }
    out
}

/// Programs that cannot find a library they need right now.
pub fn broken_now(
    binaries: &[Binary],
    system: &HashSet<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<(Binary, Vec<String>)> {
    let own: HashSet<String> = HashSet::new();
    binaries
        .iter()
        .filter_map(|b| {
            let info = crate::stale_builds::ElfInfo {
                needed: b.needed.clone(),
                search: b.search.clone(),
            };
            let origin = Path::new(&b.path).parent().unwrap_or(Path::new("/"));
            let missing = missing_libs(&info, origin, system, &own, exists);
            (!missing.is_empty()).then(|| (b.clone(), missing))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(path: &str, needed: &[&str]) -> Binary {
        Binary {
            path: path.into(),
            needed: needed.iter().map(|s| s.to_string()).collect(),
            search: vec![],
        }
    }

    #[test]
    fn a_soname_provide_is_the_file_name_of_the_library() {
        assert_eq!(
            soname_file("libfoo.so=1-64").as_deref(),
            Some("libfoo.so.1")
        );
        assert_eq!(
            soname_file("libz.so=1.3-64").as_deref(),
            Some("libz.so.1.3")
        );
        assert_eq!(soname_file("python=3.12"), None);
        assert_eq!(soname_file("libfoo.so"), None);
    }

    #[test]
    fn what_a_package_change_takes_away_is_what_the_old_provides_and_the_new_does_not() {
        let old = vec![
            "libfoo.so=1-64".to_string(),
            "libbar.so=2-64".to_string(),
            "foo=1.0".to_string(),
        ];
        let new = vec!["libfoo.so=2-64".to_string(), "libbar.so=2-64".to_string()];
        let l = lost("foo", "1.0-1", Some("2.0-1"), &old, &new);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].lib, "libfoo.so.1");
        assert_eq!(l[0].provide, "libfoo.so=1-64");
        // a removal loses everything it provided
        assert_eq!(lost("foo", "1.0-1", None, &old, &[]).len(), 2);
    }

    #[test]
    fn programs_outside_the_package_manager_that_need_a_lost_library_are_named() {
        let changes = lost(
            "foo",
            "1.0-1",
            Some("2.0-1"),
            &["libfoo.so=1-64".to_string()],
            &["libfoo.so=2-64".to_string()],
        );
        let bins = vec![
            b("/usr/local/bin/uses-foo", &["libc.so.6", "libfoo.so.1"]),
            b("/opt/app/bin/other", &["libc.so.6"]),
        ];
        let h = hits(&bins, &changes, &|_| false);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].binary, "/usr/local/bin/uses-foo");
        // another package keeps providing it: not a hit
        assert!(hits(&bins, &changes, &|p| p == "libfoo.so=1-64").is_empty());
        let lines = warning_lines(&h, 5);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/usr/local/bin/uses-foo is not from a package"));
        assert!(lines[0].contains("foo 1.0-1 \u{2192} 2.0-1 no longer provides libfoo.so.1"));
    }

    #[test]
    fn many_hits_are_cut_and_counted() {
        let c = lost("foo", "1-1", None, &["libfoo.so=1-64".to_string()], &[]);
        let bins: Vec<Binary> = (0..8)
            .map(|i| b(&format!("/opt/a{i}"), &["libfoo.so.1"]))
            .collect();
        let lines = warning_lines(&hits(&bins, &c, &|_| false), 5);
        assert_eq!(lines.len(), 6);
        assert!(lines[5].contains("and 3 more programs"));
        assert!(lines[0].contains("removing foo 1-1 takes libfoo.so.1 away"));
    }

    #[test]
    fn a_program_whose_library_is_missing_now_is_found() {
        let system: HashSet<String> = ["libc.so.6".to_string()].into();
        let bins = vec![
            b("/usr/local/bin/x", &["libc.so.6", "libgone.so.3"]),
            b("/usr/local/bin/y", &["libc.so.6"]),
        ];
        let r = broken_now(&bins, &system, &|_| false);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1, ["libgone.so.3"]);
    }
}

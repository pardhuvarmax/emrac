//! Stray files (u25, `additions.md` A9): files under `/usr` and `/opt` that no
//! package owns. Left behind by `make install`, `pip install` as root, a script
//! that copied something in, an uninstalled package whose files were changed.
//!
//! Ownership is pacman's own record (`/var/lib/pacman/local/*/files`). Nothing
//! is changed or removed; the report says what each stray looks like it came from.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// What pacman says it owns.
#[derive(Debug, Default)]
pub struct Owned {
    /// Every owned path, relative to the root, directories with a trailing `/`.
    pub paths: HashSet<String>,
    /// Owned directory (no trailing slash) → the package that owns it.
    pub dir_owner: HashMap<String, String>,
}

/// Reads the `files` lists under a pacman `local` database directory.
pub fn read_owned(local: &Path) -> Owned {
    read_owned_where(local, &|_| true)
}

/// As `read_owned`, keeping only the paths `keep` accepts (cheaper when only one
/// part of the tree matters, such as `opt/`).
pub fn read_owned_where(local: &Path, keep: &dyn Fn(&str) -> bool) -> Owned {
    let mut out = Owned::default();
    let Ok(rd) = std::fs::read_dir(local) else {
        return out;
    };
    // a directory several packages share (`usr/bin`) says nothing about who put a stray there
    let mut owners: HashMap<String, u32> = HashMap::new();
    for entry in rd.flatten() {
        let dir = entry.file_name().to_string_lossy().into_owned();
        // `name-version-release`: the name is everything before the last two dashes
        let name = {
            let mut parts = dir.rsplitn(3, '-');
            parts.next();
            parts.next();
            parts.next().unwrap_or(&dir).to_string()
        };
        let Ok(text) = std::fs::read_to_string(entry.path().join("files")) else {
            continue;
        };
        let mut in_files = false;
        for line in text.lines() {
            if line == "%FILES%" {
                in_files = true;
                continue;
            }
            if line.starts_with('%') {
                in_files = false;
                continue;
            }
            if !in_files || line.is_empty() || !keep(line) {
                continue;
            }
            if let Some(d) = line.strip_suffix('/') {
                *owners.entry(d.to_string()).or_insert(0) += 1;
                out.dir_owner
                    .entry(d.to_string())
                    .or_insert_with(|| name.clone());
            }
            out.paths.insert(line.to_string());
        }
    }
    out.dir_owner.retain(|d, _| owners.get(d) == Some(&1));
    out
}

/// What a stray looks like it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Clue {
    /// `/usr/local`: where manual installs belong.
    Local,
    /// A Python package installed outside pacman (its metadata says `pip`, or it
    /// sits in `site-packages` without an owner).
    Python(String),
    /// A global npm package.
    Npm,
    /// An installed Ruby gem.
    Gem,
    /// Put inside a directory that a package owns.
    InsideOf(String),
    /// Nothing says.
    Unknown,
}

impl Clue {
    pub fn heading(&self) -> String {
        match self {
            Clue::Local => "In /usr/local (where manual installs belong)".into(),
            Clue::Python(how) => format!("Python packages installed outside pacman ({how})"),
            Clue::Npm => "Global npm packages".into(),
            Clue::Gem => "Ruby gems installed outside pacman".into(),
            Clue::InsideOf(pkg) => format!("Put into a directory that {pkg} owns"),
            Clue::Unknown => "Not owned by any package, and nothing says where it came from".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stray {
    /// Absolute.
    pub path: String,
    pub is_dir: bool,
    /// Files in it (1 for a file).
    pub files: u64,
    pub bytes: u64,
    /// Newest modification inside it, Unix seconds.
    pub mtime: u64,
    pub clue: Clue,
}

/// Files that pacman hooks and tools create under a package's directory: owned
/// by no package and not strays.
pub fn is_generated(rel: &str) -> bool {
    const EXACT: [&str; 9] = [
        "usr/share/info/dir",
        "usr/lib/locale/locale-archive",
        "usr/share/applications/mimeinfo.cache",
        "usr/share/glib-2.0/schemas/gschemas.compiled",
        "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
        "usr/lib/gio/modules/giomodule.cache",
        "usr/lib/udev/hwdb.bin",
        "usr/share/mime/mime.cache",
        "usr/lib/ghc/package.conf.d/package.cache",
    ];
    if EXACT.contains(&rel) {
        return true;
    }
    const PREFIX: [&str; 5] = [
        "usr/share/mime/",
        "usr/lib/modules/",
        "usr/share/icons/",
        "usr/share/fonts/",
        "usr/lib/python3/dist-packages/__pycache__/",
    ];
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if rel.starts_with("usr/lib/modules/") {
        return true; // built by depmod and dkms
    }
    if (rel.starts_with("usr/share/icons/") && name.ends_with(".cache"))
        || (rel.starts_with("usr/share/fonts/")
            && (name.starts_with("fonts.") || name.ends_with(".cache")))
        || (rel.starts_with("usr/share/mime/") && !name.is_empty())
    {
        return true;
    }
    let _ = PREFIX;
    // byte-compiled Python and Perl
    rel.contains("/__pycache__/") || rel.ends_with(".pyc") || rel.ends_with("/__pycache__")
}

fn clue_of(rel: &str, owned: &Owned, base: &Path) -> Clue {
    if rel.starts_with("usr/local/") || rel == "usr/local" {
        return Clue::Local;
    }
    if let Some(i) = rel.find("site-packages/") {
        let after = &rel[i + "site-packages/".len()..];
        let top = after.split('/').next().unwrap_or("");
        let site = &rel[..i + "site-packages".len()];
        // `X-1.0.dist-info/INSTALLER` names the installer
        let mut how = "an installer other than pacman".to_string();
        if top.ends_with(".dist-info") {
            if let Ok(t) = std::fs::read_to_string(base.join(site).join(top).join("INSTALLER")) {
                how = t.trim().to_string();
            }
        } else if top.ends_with(".egg-info") {
            how = "setuptools".to_string();
        }
        if let Ok(rd) = std::fs::read_dir(base.join(site)) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".dist-info") {
                    let stem = n.trim_end_matches(".dist-info");
                    let pkg = top.trim_end_matches(".py").replace('_', "-").to_lowercase();
                    if stem.to_lowercase().replace('_', "-").starts_with(&pkg) {
                        if let Ok(t) = std::fs::read_to_string(e.path().join("INSTALLER")) {
                            how = t.trim().to_string();
                        }
                    }
                }
            }
        }
        return Clue::Python(how);
    }
    if rel.starts_with("usr/lib/node_modules/") {
        return Clue::Npm;
    }
    if rel.starts_with("usr/lib/ruby/gems/") || rel.starts_with("usr/share/gems/") {
        return Clue::Gem;
    }
    // the nearest owned ancestor directory
    let mut p = rel;
    while let Some(i) = p.rfind('/') {
        p = &p[..i];
        if let Some(pkg) = owned.dir_owner.get(p) {
            return Clue::InsideOf(pkg.clone());
        }
    }
    Clue::Unknown
}

fn mtime(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Totals of an unowned directory: files, bytes, newest mtime.
fn tally(dir: &Path, files: &mut u64, bytes: &mut u64, newest: &mut u64, budget: &mut u32) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(m) = e.path().symlink_metadata() else {
            continue;
        };
        if m.is_dir() {
            tally(&e.path(), files, bytes, newest, budget);
        } else {
            *files += 1;
            *bytes += m.len();
            *newest = (*newest).max(mtime(&m));
        }
    }
}

/// Options for one scan.
pub struct Scan<'a> {
    /// The root the paths are under (`/`, or a test directory).
    pub base: &'a Path,
    /// Directories to look in, relative to `base` (`usr`, `opt`).
    pub roots: &'a [&'a str],
    pub owned: &'a Owned,
    /// Look in `usr/local` too (its contents are expected, so it is off by default).
    pub include_local: bool,
    /// Show the files tools generate under package directories as well.
    pub include_generated: bool,
}

/// Walks the roots and returns what no package owns, directories collapsed.
pub fn scan(opts: &Scan) -> Vec<Stray> {
    let mut out = Vec::new();
    for root in opts.roots {
        walk(opts, root, &mut out);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk(opts: &Scan, rel: &str, out: &mut Vec<Stray>) {
    let Ok(rd) = std::fs::read_dir(opts.base.join(rel)) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        let Ok(m) = e.path().symlink_metadata() else {
            continue;
        };
        let is_dir = m.is_dir();
        // /usr/local is expected to hold manual installs, whoever owns the directory
        if child == "usr/local" && !opts.include_local {
            continue;
        }
        let owned = if is_dir {
            opts.owned.paths.contains(&format!("{child}/"))
        } else {
            opts.owned.paths.contains(&child)
        };
        if owned {
            if is_dir {
                walk(opts, &child, out);
            }
            continue;
        }
        if !opts.include_generated && is_generated(&child) {
            continue;
        }
        let (mut files, mut bytes, mut newest) = (u64::from(!is_dir), m.len(), mtime(&m));
        if is_dir {
            files = 0;
            bytes = 0;
            newest = mtime(&m);
            let mut budget = 200_000u32;
            tally(&e.path(), &mut files, &mut bytes, &mut newest, &mut budget);
            // a directory of nothing but generated files is not a stray
            if files == 0 && !opts.include_generated {
                continue;
            }
        }
        out.push(Stray {
            path: format!("/{child}"),
            is_dir,
            files,
            bytes,
            mtime: newest,
            clue: clue_of(&child, opts.owned, opts.base),
        });
    }
}

/// Groups strays by what they look like, in a fixed order.
pub fn group(strays: &[Stray]) -> Vec<(Clue, Vec<&Stray>)> {
    let mut order: Vec<Clue> = Vec::new();
    for s in strays {
        if !order.contains(&s.clue) {
            order.push(s.clue.clone());
        }
    }
    let rank = |c: &Clue| match c {
        Clue::Python(_) => 0,
        Clue::Npm => 1,
        Clue::Gem => 2,
        Clue::InsideOf(_) => 3,
        Clue::Local => 4,
        Clue::Unknown => 5,
    };
    order.sort_by_key(|c| (rank(c), c.heading()));
    order
        .into_iter()
        .map(|c| {
            let items = strays.iter().filter(|s| s.clue == c).collect();
            (c, items)
        })
        .collect()
}

/// The root a scan of the real system uses.
pub fn system_base() -> PathBuf {
    PathBuf::from("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-strays-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(base: &Path, rel: &str, text: &str) {
        let p = base.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn db(base: &Path, list: &[(&str, &[&str])]) -> PathBuf {
        let local = base.join("var/lib/pacman/local");
        for (pkg, files) in list {
            let d = local.join(format!("{pkg}-1.0-1"));
            fs::create_dir_all(&d).unwrap();
            let mut t = String::from("%FILES%\n");
            for f in *files {
                t.push_str(f);
                t.push('\n');
            }
            t.push_str("\n%BACKUP%\n");
            fs::write(d.join("files"), t).unwrap();
        }
        local
    }

    #[test]
    fn the_owned_set_and_the_directory_owners_are_read_from_pacmans_lists() {
        let base = tree("owned");
        let local = db(
            &base,
            &[
                (
                    "foo",
                    &[
                        "usr/",
                        "usr/bin/",
                        "usr/bin/foo",
                        "usr/lib/",
                        "usr/lib/foo/",
                        "usr/lib/foo/x.so",
                    ],
                ),
                ("bar", &["usr/", "usr/bin/", "usr/bin/bar", "usr/lib/"]),
            ],
        );
        let o = read_owned(&local);
        assert!(o.paths.contains("usr/bin/foo") && o.paths.contains("usr/lib/foo/"));
        assert_eq!(
            o.dir_owner.get("usr/lib/foo").map(String::as_str),
            Some("foo")
        );
        assert!(!o.paths.contains("%BACKUP%"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn what_no_package_owns_is_found_with_a_clue_and_what_is_owned_or_generated_is_not() {
        let base = tree("scan");
        let local = db(
            &base,
            &[
                (
                    "foo",
                    &[
                        "usr/",
                        "usr/bin/",
                        "usr/bin/foo",
                        "usr/lib/",
                        "usr/lib/foo/",
                        "usr/lib/foo/x.so",
                    ],
                ),
                (
                    "bar",
                    &[
                        "usr/",
                        "usr/bin/",
                        "usr/bin/bar",
                        "usr/lib/",
                        "usr/lib/python3.99/",
                        "usr/lib/python3.99/site-packages/",
                    ],
                ),
            ],
        );
        put(&base, "usr/bin/foo", "owned");
        put(&base, "usr/bin/handmade", "make install put this here");
        put(&base, "usr/lib/foo/plugin.so", "dropped in foo's directory");
        put(
            &base,
            "usr/lib/python3.99/site-packages/leftpad/__init__.py",
            "x",
        );
        put(
            &base,
            "usr/lib/python3.99/site-packages/leftpad-1.0.dist-info/INSTALLER",
            "pip\n",
        );
        put(
            &base,
            "usr/lib/python3.99/site-packages/leftpad/__pycache__/a.pyc",
            "x",
        );
        put(&base, "usr/share/info/dir", "generated by install-info");
        put(&base, "usr/share/mime/mime.cache", "generated");
        put(&base, "opt/strayapp/bin/run", "unpackaged app");
        put(&base, "usr/local/bin/mine", "local");
        let owned = read_owned(&local);
        let s = |local: bool| {
            scan(&Scan {
                base: &base,
                roots: &["usr", "opt"],
                owned: &owned,
                include_local: local,
                include_generated: false,
            })
        };
        let found = s(false);
        let paths: Vec<&str> = found.iter().map(|x| x.path.as_str()).collect();
        assert!(paths.contains(&"/usr/bin/handmade"), "{paths:?}");
        assert!(paths.contains(&"/usr/lib/foo/plugin.so"));
        assert!(
            paths.contains(&"/opt/strayapp"),
            "an unowned directory is one stray"
        );
        assert!(!paths.contains(&"/usr/bin/foo"), "owned");
        assert!(
            !paths
                .iter()
                .any(|p| p.contains("mime.cache") || p.ends_with("info/dir")),
            "generated"
        );
        assert!(
            !paths.iter().any(|p| p.contains("/usr/local")),
            "/usr/local is off by default"
        );
        let by = |p: &str| found.iter().find(|x| x.path == p).unwrap();
        assert_eq!(
            by("/usr/lib/foo/plugin.so").clue,
            Clue::InsideOf("foo".into())
        );
        assert_eq!(by("/usr/bin/handmade").clue, Clue::Unknown);
        let py = found
            .iter()
            .find(|x| x.path.ends_with("site-packages/leftpad"))
            .unwrap();
        assert_eq!(py.clue, Clue::Python("pip".into()));
        assert!(py.is_dir && py.files >= 1);
        // /usr/local on request
        assert!(s(true)
            .iter()
            .any(|x| x.path == "/usr/local" && x.clue == Clue::Local));
        // grouped in a fixed order
        let grouped = group(&found);
        assert!(matches!(grouped[0].0, Clue::Python(_)));
        assert_eq!(grouped.last().unwrap().0, Clue::Unknown);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn generated_files_are_recognised() {
        for g in [
            "usr/share/info/dir",
            "usr/lib/modules/6.1/modules.dep",
            "usr/lib/python3.12/site-packages/x/__pycache__/a.cpython-312.pyc",
            "usr/share/icons/hicolor/icon-theme.cache",
        ] {
            assert!(is_generated(g), "{g}");
        }
        for h in [
            "usr/bin/foo",
            "usr/lib/python3.12/site-packages/x/a.py",
            "opt/app/run",
        ] {
            assert!(!is_generated(h), "{h}");
        }
    }
}

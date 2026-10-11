//! `emrac verify`: do an installed package's files still match what pacman
//! recorded in its `mtree`? The mtree is parsed here (libalpm's iterator
//! doesn't expose the digests); everything but the final filesystem lookups
//! is pure.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MtreeEntry {
    /// Relative to the root, no leading `./`.
    pub path: String,
    pub kind: EntryKind,
    pub mode: Option<u32>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Missing,
    WrongType,
    SizeMismatch,
    ContentMismatch,
    ModeMismatch,
    LinkMismatch,
    /// Couldn't be looked at (usually root-only): not evidence of a problem.
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub path: String,
    pub kind: IssueKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Issue {
    /// Real damage, as opposed to "couldn't check".
    pub fn is_problem(&self) -> bool {
        self.kind != IssueKind::Unreadable
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageReport {
    pub package: String,
    pub version: String,
    pub files_checked: usize,
    pub issues: Vec<Issue>,
    /// `backup=()` files whose contents differ — expected, not a problem.
    pub modified_config: Vec<String>,
}

impl PackageReport {
    pub fn problems(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.is_problem())
    }

    pub fn problem_count(&self) -> usize {
        self.problems().count()
    }

    pub fn unreadable_count(&self) -> usize {
        self.issues.len() - self.problem_count()
    }

    pub fn is_ok(&self) -> bool {
        self.problem_count() == 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub packages: Vec<PackageReport>,
}

impl VerifyReport {
    pub fn broken(&self) -> Vec<&PackageReport> {
        self.packages.iter().filter(|p| !p.is_ok()).collect()
    }
}

/// Reads a package's mtree: gzip-compressed in the local db, but plain text
/// is accepted too.
pub(crate) fn read_mtree(path: &Path) -> Result<String> {
    let io = |source| Error::Io {
        context: format!("couldn't read {}", path.display()),
        source,
    };
    let bytes = fs::read(path).map_err(io)?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        GzDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .map_err(io)?;
        Ok(text)
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Parses mtree text into the entries pacman installed. `/set` and `/unset`
/// defaults are honored; the package's own metadata files (`./.PKGINFO`,
/// ...) are not installed files and are skipped.
pub(crate) fn parse_mtree(text: &str) -> Vec<MtreeEntry> {
    let mut defaults: HashMap<String, String> = HashMap::new();
    let mut entries = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(first) = words.next() else { continue };

        match first {
            "/set" => {
                for word in words {
                    if let Some((k, v)) = word.split_once('=') {
                        defaults.insert(k.to_string(), v.to_string());
                    }
                }
            }
            "/unset" => {
                for word in words {
                    if word == "all" {
                        defaults.clear();
                    } else {
                        defaults.remove(word);
                    }
                }
            }
            _ => {
                let name = unescape(first);
                let rel = name.strip_prefix("./").unwrap_or(&name);
                if rel.is_empty() || rel == "." || is_package_metadata(rel) {
                    continue;
                }
                let mut keys = defaults.clone();
                for word in words {
                    if let Some((k, v)) = word.split_once('=') {
                        keys.insert(k.to_string(), v.to_string());
                    }
                }
                let kind = match keys.get("type").map(String::as_str) {
                    Some("dir") => EntryKind::Dir,
                    Some("link") => EntryKind::Link,
                    _ => EntryKind::File,
                };
                entries.push(MtreeEntry {
                    path: rel.to_string(),
                    kind,
                    mode: keys
                        .get("mode")
                        .and_then(|m| u32::from_str_radix(m, 8).ok()),
                    size: keys.get("size").and_then(|s| s.parse().ok()),
                    sha256: keys.get("sha256digest").cloned(),
                    link: keys.get("link").map(|l| unescape(l)),
                });
            }
        }
    }
    entries
}

fn is_package_metadata(rel: &str) -> bool {
    !rel.contains('/')
        && matches!(
            rel,
            ".PKGINFO" | ".BUILDINFO" | ".INSTALL" | ".CHANGELOG" | ".MTREE"
        )
}

/// mtree escapes unusual bytes as `\NNN` (octal) and a backslash as `\\`.
fn unescape(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if i + 3 < bytes.len()
                && bytes[i + 1..i + 4]
                    .iter()
                    .all(|b| (b'0'..=b'7').contains(b))
            {
                let value = u32::from_str_radix(&input[i + 1..i + 4], 8).unwrap_or(0);
                out.push(value as u8);
                i += 4;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Compares one recorded entry with the filesystem under `root`.
/// `is_backup`: a `backup=()` file, whose contents are allowed to differ.
/// Returns `(issue, modified_config)`.
pub(crate) fn check_entry(
    root: &Path,
    entry: &MtreeEntry,
    is_backup: bool,
) -> (Option<Issue>, bool) {
    let path = root.join(&entry.path);
    let issue = |kind, detail: Option<String>| Issue {
        path: path.display().to_string(),
        kind,
        detail,
    };

    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (Some(issue(IssueKind::Missing, None)), false);
        }
        Err(err) => {
            return (
                Some(issue(IssueKind::Unreadable, Some(err.to_string()))),
                false,
            );
        }
    };

    let ft = meta.file_type();
    let actual_kind = if ft.is_symlink() {
        Some(EntryKind::Link)
    } else if ft.is_dir() {
        Some(EntryKind::Dir)
    } else if ft.is_file() {
        Some(EntryKind::File)
    } else {
        None
    };
    if actual_kind != Some(entry.kind) {
        let is = match actual_kind {
            Some(EntryKind::Link) => "a symlink",
            Some(EntryKind::Dir) => "a directory",
            Some(EntryKind::File) => "a regular file",
            None if ft.is_fifo() || ft.is_socket() => "a special file",
            None => "something else",
        };
        let should = match entry.kind {
            EntryKind::Link => "a symlink",
            EntryKind::Dir => "a directory",
            EntryKind::File => "a regular file",
        };
        return (
            Some(issue(
                IssueKind::WrongType,
                Some(format!("is {is}, should be {should}")),
            )),
            false,
        );
    }

    match entry.kind {
        EntryKind::Link => {
            if let (Some(want), Ok(have)) = (&entry.link, fs::read_link(&path)) {
                if have.to_string_lossy() != *want {
                    return (
                        Some(issue(
                            IssueKind::LinkMismatch,
                            Some(format!(
                                "points to {}, should point to {want}",
                                have.display()
                            )),
                        )),
                        false,
                    );
                }
            }
            (None, false)
        }
        EntryKind::Dir => (mode_issue(&meta, entry, &issue), false),
        EntryKind::File => {
            // Contents first: a config file may legitimately differ.
            let mut modified = false;
            let mut content_issue = None;
            if let Some(want) = entry.size {
                if meta.len() != want {
                    content_issue = Some(issue(
                        IssueKind::SizeMismatch,
                        Some(format!("is {} bytes, should be {want}", meta.len())),
                    ));
                }
            }
            if content_issue.is_none() {
                if let Some(want) = &entry.sha256 {
                    match alpm::compute_sha256sum(path.display().to_string()) {
                        Ok(have) if have == *want => {}
                        Ok(_) => {
                            content_issue = Some(issue(IssueKind::ContentMismatch, None));
                        }
                        Err(_) => {
                            return (
                                Some(issue(
                                    IssueKind::Unreadable,
                                    Some("couldn't read the file to check it".into()),
                                )),
                                false,
                            );
                        }
                    }
                }
            }
            if let Some(found) = content_issue {
                if is_backup {
                    modified = true;
                } else {
                    return (Some(found), false);
                }
            }
            (mode_issue(&meta, entry, &issue), modified)
        }
    }
}

fn mode_issue(
    meta: &fs::Metadata,
    entry: &MtreeEntry,
    issue: &dyn Fn(IssueKind, Option<String>) -> Issue,
) -> Option<Issue> {
    let want = entry.mode?;
    let have = meta.mode() & 0o7777;
    (have != want).then(|| {
        issue(
            IssueKind::ModeMismatch,
            Some(format!("is {have:o}, should be {want:o}")),
        )
    })
}

/// Verifies every entry of one package. `root` is pacman's `RootDir`;
/// `backup` are the package's config files, relative to it.
pub(crate) fn verify_entries(
    package: &str,
    version: &str,
    root: &Path,
    entries: &[MtreeEntry],
    backup: &[String],
) -> PackageReport {
    let mut issues = Vec::new();
    let mut modified_config = Vec::new();

    for entry in entries {
        let is_backup = backup.iter().any(|b| b == &entry.path);
        let (issue, modified) = check_entry(root, entry, is_backup);
        issues.extend(issue);
        if modified {
            modified_config.push(root.join(&entry.path).display().to_string());
        }
    }

    PackageReport {
        package: package.to_string(),
        version: version.to_string(),
        files_checked: entries.len(),
        issues,
        modified_config,
    }
}

/// Keeps only the mtree entries libalpm recorded as actually installed.
/// The mtree describes the whole package, but files excluded at install
/// time (`NoExtract` — the stock Arch container image drops docs and
/// locales, for instance) are not in the local file list and must not be
/// reported missing. `installed` holds the file list's names; directories
/// there carry a trailing `/`.
pub(crate) fn retain_installed(entries: &mut Vec<MtreeEntry>, installed: &[String]) {
    let wanted: std::collections::HashSet<&str> = installed
        .iter()
        .map(|name| name.trim_end_matches('/'))
        .collect();
    entries.retain(|e| wanted.contains(e.path.as_str()));
}

/// Does pacman's `NoExtract` list exclude `path` (relative to the root;
/// directories carry a trailing `/`)? Patterns are `fnmatch` globs where `*`
/// also crosses `/`; a `!` pattern re-includes what an earlier one excluded,
/// and the last matching pattern wins.
pub(crate) fn no_extract_excludes(patterns: &[String], path: &str) -> bool {
    let mut excluded = false;
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(negated) if glob_match(negated, path) => excluded = false,
            Some(_) => {}
            None if glob_match(pattern, path) => excluded = true,
            None => {}
        }
    }
    excluded
}

/// Minimal `fnmatch(pattern, text, 0)`: `*` (any run, `/` included), `?`
/// (any one character), everything else literal.
fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(star_at) = star {
            pi = star_at + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Drops entries `NoExtract` kept off this system.
pub(crate) fn drop_no_extract(entries: &mut Vec<MtreeEntry>, patterns: &[String]) {
    entries.retain(|e| {
        let path = if e.kind == EntryKind::Dir {
            format!("{}/", e.path)
        } else {
            e.path.clone()
        };
        !no_extract_excludes(patterns, &path)
    });
}

/// Where a package's mtree lives in the local db.
pub(crate) fn mtree_path(db_path: &str, name: &str, version: &str) -> PathBuf {
    Path::new(db_path)
        .join("local")
        .join(format!("{name}-{version}"))
        .join("mtree")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    const MTREE: &str = "#mtree
/set type=file uid=0 gid=0 mode=644
./.BUILDINFO time=1.0 size=10 sha256digest=aa
./.PKGINFO time=1.0 size=10 sha256digest=bb
/set mode=755
./usr time=1.0 type=dir
./usr/bin time=1.0 type=dir
./usr/bin/tool time=1.0 size=6 sha256digest=0
./usr/share/doc.txt time=1.0 mode=644 size=3 sha256digest=1
./usr/lib/libx.so time=1.0 type=link link=libx.so.1
./usr/share/with\\040space time=1.0 mode=644 size=1
";

    #[test]
    fn mtree_defaults_overrides_links_and_metadata() {
        let e = parse_mtree(MTREE);
        let paths: Vec<_> = e.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "usr",
                "usr/bin",
                "usr/bin/tool",
                "usr/share/doc.txt",
                "usr/lib/libx.so",
                "usr/share/with space"
            ]
        );
        assert_eq!(e[0].kind, EntryKind::Dir);
        assert_eq!(e[0].mode, Some(0o755));
        assert_eq!(e[2].mode, Some(0o755)); // inherited from /set
        assert_eq!(e[2].size, Some(6));
        assert_eq!(e[3].mode, Some(0o644)); // overridden
        assert_eq!(e[4].kind, EntryKind::Link);
        assert_eq!(e[4].link.as_deref(), Some("libx.so.1"));
    }

    #[test]
    fn unset_and_escapes() {
        let e = parse_mtree(
            "/set mode=700 type=file\n/unset mode\n./a size=1\n/unset all\n./b\n./we\\134ird\\n",
        );
        assert_eq!(e[0].mode, None);
        assert_eq!(e[1].kind, EntryKind::File);
        assert_eq!(unescape("a\\134b"), "a\\b");
        assert_eq!(unescape("a\\\\b"), "a\\b");
        assert_eq!(unescape("no\\escape"), "no\\escape");
        assert_eq!(unescape("tab\\011"), "tab\t");
    }

    struct Root(PathBuf);
    impl Root {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("emrac-verify-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Root(dir)
        }
        fn write(&self, rel: &str, content: &str, mode: u32) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, content).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // sha256("hello\n")
    const HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    fn file(path: &str, size: u64, sha: Option<&str>, mode: u32) -> MtreeEntry {
        MtreeEntry {
            path: path.into(),
            kind: EntryKind::File,
            mode: Some(mode),
            size: Some(size),
            sha256: sha.map(String::from),
            link: None,
        }
    }

    fn kinds(r: &PackageReport) -> Vec<IssueKind> {
        r.issues.iter().map(|i| i.kind).collect()
    }

    #[test]
    fn a_clean_package_has_no_issues() {
        let root = Root::new("clean");
        root.write("usr/bin/tool", "hello\n", 0o755);
        fs::create_dir_all(root.0.join("usr/dir")).unwrap();
        fs::set_permissions(root.0.join("usr/dir"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("tool", root.0.join("usr/bin/alias")).unwrap();
        let entries = vec![
            file("usr/bin/tool", 6, Some(HELLO), 0o755),
            MtreeEntry {
                path: "usr/dir".into(),
                kind: EntryKind::Dir,
                mode: Some(0o755),
                size: None,
                sha256: None,
                link: None,
            },
            MtreeEntry {
                path: "usr/bin/alias".into(),
                kind: EntryKind::Link,
                mode: None,
                size: None,
                sha256: None,
                link: Some("tool".into()),
            },
        ];
        let r = verify_entries("p", "1-1", &root.0, &entries, &[]);
        assert!(r.is_ok(), "{:?}", r.issues);
        assert_eq!(r.files_checked, 3);
        assert!(r.modified_config.is_empty());
    }

    #[test]
    fn each_kind_of_damage_is_reported() {
        let root = Root::new("damage");
        root.write("a/size", "hello\n!", 0o644);
        root.write("a/content", "HELLO\n", 0o644);
        root.write("a/mode", "hello\n", 0o600);
        root.write("a/notlink", "x", 0o644);
        symlink("elsewhere", root.0.join("a/link")).unwrap();
        let entries = vec![
            file("a/missing", 6, Some(HELLO), 0o644),
            file("a/size", 6, Some(HELLO), 0o644),
            file("a/content", 6, Some(HELLO), 0o644),
            file("a/mode", 6, Some(HELLO), 0o644),
            MtreeEntry {
                path: "a/notlink".into(),
                kind: EntryKind::Link,
                mode: None,
                size: None,
                sha256: None,
                link: Some("t".into()),
            },
            MtreeEntry {
                path: "a/link".into(),
                kind: EntryKind::Link,
                mode: None,
                size: None,
                sha256: None,
                link: Some("t".into()),
            },
        ];
        let r = verify_entries("p", "1-1", &root.0, &entries, &[]);
        assert_eq!(
            kinds(&r),
            [
                IssueKind::Missing,
                IssueKind::SizeMismatch,
                IssueKind::ContentMismatch,
                IssueKind::ModeMismatch,
                IssueKind::WrongType,
                IssueKind::LinkMismatch,
            ]
        );
        assert_eq!(r.problem_count(), 6);
        assert!(!r.is_ok());
        assert!(r.issues[3].detail.as_ref().unwrap().contains("600"));
    }

    #[test]
    fn edited_config_is_not_a_problem_but_missing_config_is() {
        let root = Root::new("config");
        root.write("etc/a.conf", "my edit\n", 0o644);
        let entries = vec![
            file("etc/a.conf", 6, Some(HELLO), 0o644),
            file("etc/gone.conf", 6, Some(HELLO), 0o644),
        ];
        let backup = vec!["etc/a.conf".to_string(), "etc/gone.conf".to_string()];
        let r = verify_entries("p", "1-1", &root.0, &entries, &backup);
        assert_eq!(kinds(&r), [IssueKind::Missing]);
        assert_eq!(r.modified_config.len(), 1);
        assert!(r.modified_config[0].ends_with("etc/a.conf"));
    }

    #[test]
    fn a_wrong_mode_on_an_edited_config_is_still_reported() {
        let root = Root::new("configmode");
        root.write("etc/a.conf", "my edit\n", 0o666);
        let entries = vec![file("etc/a.conf", 6, Some(HELLO), 0o644)];
        let r = verify_entries("p", "1-1", &root.0, &entries, &["etc/a.conf".into()]);
        assert_eq!(kinds(&r), [IssueKind::ModeMismatch]);
        assert_eq!(r.modified_config.len(), 1);
    }

    #[test]
    fn unreadable_is_counted_but_is_not_a_problem() {
        let issue = Issue {
            path: "/x".into(),
            kind: IssueKind::Unreadable,
            detail: None,
        };
        let report = PackageReport {
            package: "p".into(),
            version: "1".into(),
            files_checked: 1,
            issues: vec![issue],
            modified_config: vec![],
        };
        assert!(report.is_ok());
        assert_eq!(report.unreadable_count(), 1);
        assert_eq!(report.problem_count(), 0);
    }

    #[test]
    fn mtree_files_are_read_plain_or_gzipped() {
        use flate2::{write::GzEncoder, Compression};
        use std::io::Write;
        let root = Root::new("gz");
        let plain = root.0.join("plain");
        fs::write(&plain, "#mtree\n./a\n").unwrap();
        assert_eq!(read_mtree(&plain).unwrap(), "#mtree\n./a\n");

        let gz = root.0.join("gz");
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"#mtree\n./b\n").unwrap();
        fs::write(&gz, enc.finish().unwrap()).unwrap();
        assert_eq!(read_mtree(&gz).unwrap(), "#mtree\n./b\n");

        assert!(matches!(
            read_mtree(&root.0.join("nope")),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn mtree_path_layout() {
        assert_eq!(
            mtree_path("/var/lib/pacman", "tree", "2.3.2-1"),
            PathBuf::from("/var/lib/pacman/local/tree-2.3.2-1/mtree")
        );
    }

    #[test]
    fn entries_not_in_the_installed_file_list_are_dropped() {
        let mut entries = parse_mtree(
            "/set type=file mode=644\n./usr type=dir\n./usr/bin/tool size=1\n./usr/share/locale/de/tool.mo size=1\n",
        );
        retain_installed(
            &mut entries,
            &["usr/".to_string(), "usr/bin/tool".to_string()],
        );
        let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["usr", "usr/bin/tool"]);
    }

    fn pats(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn globs_follow_fnmatch_for_star_and_question_mark() {
        assert!(glob_match(
            "usr/share/locale/*",
            "usr/share/locale/de/LC_MESSAGES/x.mo"
        ));
        assert!(glob_match("usr/share/locale/*", "usr/share/locale/"));
        assert!(!glob_match("usr/share/locale/*", "usr/share/locale"));
        assert!(glob_match(
            "*locale*/en*/*",
            "usr/share/locale/en_GB/LC_MESSAGES/x.mo"
        ));
        assert!(glob_match(
            "usr/share/*locales/en_??",
            "usr/share/i18n/locales/en_US"
        ));
        assert!(!glob_match(
            "usr/share/*locales/en_??",
            "usr/share/i18n/locales/en_USA"
        ));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(!glob_match("abc", "abcd"));
    }

    #[test]
    fn no_extract_last_match_wins_and_bang_re_includes() {
        let p = pats(&["usr/share/locale/*", "!*locale*/en*/*", "usr/share/man/*"]);
        assert!(no_extract_excludes(
            &p,
            "usr/share/locale/de/LC_MESSAGES/a.mo"
        ));
        assert!(!no_extract_excludes(
            &p,
            "usr/share/locale/en_GB/LC_MESSAGES/a.mo"
        ));
        assert!(no_extract_excludes(&p, "usr/share/man/man1/a.1.gz"));
        assert!(!no_extract_excludes(&p, "usr/bin/a"));
        assert!(!no_extract_excludes(&[], "usr/bin/a"));
        // A later exclusion beats an earlier re-include.
        let p = pats(&["!usr/x/*", "usr/x/*"]);
        assert!(no_extract_excludes(&p, "usr/x/y"));
    }

    #[test]
    fn drop_no_extract_treats_directories_with_a_trailing_slash() {
        let mut entries = parse_mtree(
            "/set type=file mode=644\n./usr/share/man type=dir\n./usr/share/man/man1 type=dir\n./usr/share/man/man1/a.1.gz size=1\n./usr/bin/a size=1\n",
        );
        drop_no_extract(&mut entries, &pats(&["usr/share/man/*"]));
        let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        // `usr/share/man/` itself matches `usr/share/man/*` (`*` may be
        // empty), as do its children.
        assert_eq!(paths, ["usr/bin/a"]);
    }
}

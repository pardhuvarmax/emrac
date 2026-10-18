//! `emrac bundle` (u14): a folder of repository packages and their signatures,
//! carried to a machine without network. See `docs/plan/r3/u14-bundle.md`.
//!
//! The manifest's hashes catch a damaged copy; trust comes from pacman checking
//! each package's signature against the target's own keyring, which `apply`
//! insists on (`signed_config`).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::SyncPackage;
use crate::error::{Error, Result};
use crate::exchange::valid_package_name;

pub const FORMAT: u32 = 1;
pub const MAX_PACKAGES: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub format: u32,
    pub emrac: String,
    /// `YYYY-MM-DD`, UTC.
    pub created: String,
    /// `x86_64`, as the packages were built for.
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundlePackage {
    pub name: String,
    pub version: String,
    pub repo: String,
    /// File name inside `pkg/`.
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub sig_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub meta: Meta,
    /// What was asked for (empty: every upgrade the target waits for).
    #[serde(default)]
    pub requested: Vec<String>,
    /// The date of the target's exchange file, when `--for` was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_target: Option<String>,
    #[serde(default)]
    pub package: Vec<BundlePackage>,
    #[serde(default)]
    pub skipped: Vec<Skipped>,
}

fn bad(why: impl Into<String>) -> Error {
    Error::Bundle(why.into())
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// A package file name that stays inside `pkg/` and belongs to its package.
pub fn file_name_ok(name: &str, version: &str, file: &str) -> bool {
    !file.is_empty()
        && file.len() <= 255
        && !file.contains(['/', '\0', '\\'])
        && !file.starts_with('.')
        && !file.ends_with(".sig")
        && file.starts_with(&format!("{name}-{version}-"))
        && file.contains(".pkg.tar")
}

pub fn render(m: &Manifest) -> String {
    format!(
        "# An emrac bundle (`emrac bundle create`). `emrac bundle verify <dir>` checks it;\n\
         # `emrac bundle apply <dir>` installs it, and pacman checks every signature.\n\n{}",
        toml::to_string_pretty(m).unwrap_or_default()
    )
}

pub fn parse(text: &str) -> Result<Manifest> {
    let m: Manifest =
        toml::from_str(text).map_err(|e| bad(format!("not a bundle manifest: {e}")))?;
    if m.meta.format != FORMAT {
        return Err(bad(format!(
            "bundle format {} is not one this emrac reads (it reads format {FORMAT})",
            m.meta.format
        )));
    }
    if m.package.len() > MAX_PACKAGES {
        return Err(bad(format!("more than {MAX_PACKAGES} packages")));
    }
    if m.meta.arch.is_empty()
        || !m
            .meta
            .arch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(bad("the architecture is not valid"));
    }
    let mut files = HashSet::new();
    for p in &m.package {
        if !valid_package_name(&p.name) {
            return Err(bad(format!("'{}' is not a package name", p.name)));
        }
        if p.version.is_empty()
            || p.version.len() > 100
            || !p.version.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(bad(format!("'{}' has an invalid version", p.name)));
        }
        if !file_name_ok(&p.name, &p.version, &p.file) {
            return Err(bad(format!(
                "'{}' is not a valid file for {} {}",
                p.file, p.name, p.version
            )));
        }
        if !is_hex64(&p.sha256) || !is_hex64(&p.sig_sha256) {
            return Err(bad(format!("{}: a checksum is not a SHA-256", p.name)));
        }
        if !files.insert(p.file.as_str()) {
            return Err(bad(format!("{} is listed twice", p.file)));
        }
    }
    Ok(m)
}

/// Of the closure, what a target with `target` installed lacks or has older.
/// `newer(a, b)` is true when version `a` is newer than version `b`.
pub fn needed(
    closure: &[SyncPackage],
    target: &BTreeMap<String, String>,
    newer: &dyn Fn(&str, &str) -> bool,
) -> Vec<SyncPackage> {
    closure
        .iter()
        .filter(|p| match target.get(&p.name) {
            None => true,
            Some(have) => newer(&p.version, have),
        })
        .cloned()
        .collect()
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(|e| bad(format!("{}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| bad(format!("{}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Everything wrong with the files of the bundle in `dir`; empty is good.
pub fn verify_dir(dir: &Path, m: &Manifest) -> Vec<String> {
    let pkg_dir = dir.join("pkg");
    let mut problems = Vec::new();
    let mut listed: HashSet<String> = HashSet::new();
    for p in &m.package {
        listed.insert(p.file.clone());
        listed.insert(format!("{}.sig", p.file));
        let file = pkg_dir.join(&p.file);
        let sig = pkg_dir.join(format!("{}.sig", p.file));
        for (path, want_size, want_hash, what) in [
            (&file, Some(p.size), &p.sha256, "package"),
            (&sig, None, &p.sig_sha256, "signature"),
        ] {
            match fs::symlink_metadata(path) {
                Err(_) => {
                    problems.push(format!(
                        "{}: the {what} file is missing ({})",
                        p.name,
                        path.display()
                    ));
                    continue;
                }
                Ok(meta) if !meta.is_file() => {
                    problems.push(format!("{}: the {what} is not a plain file", p.name));
                    continue;
                }
                Ok(meta) => {
                    if want_size.is_some_and(|s| s != meta.len()) {
                        problems.push(format!(
                            "{}: the {what} has a different size than the manifest says",
                            p.name
                        ));
                        continue;
                    }
                }
            }
            match sha256_file(path) {
                Ok(h) if h == *want_hash => {}
                Ok(_) => problems.push(format!(
                    "{}: the {what} does not match its checksum",
                    p.name
                )),
                Err(e) => problems.push(e.to_string()),
            }
        }
    }
    match fs::read_dir(&pkg_dir) {
        Ok(entries) => {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !listed.contains(&name) {
                    problems.push(format!("{name}: in the bundle but not in the manifest"));
                }
            }
        }
        Err(e) => problems.push(format!("{}: {e}", pkg_dir.display())),
    }
    problems.sort();
    problems
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Install,
    Upgrade,
    /// Already installed at this version.
    Same,
    /// The installed version is newer: never downgraded.
    Older,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub from: Option<String>,
    pub to: String,
    pub kind: ChangeKind,
}

/// What applying the bundle would do on a machine with `installed`.
/// `newer(a, b)` is true when version `a` is newer than version `b`.
pub fn compare(
    m: &Manifest,
    installed: &[(String, String)],
    newer: &dyn Fn(&str, &str) -> bool,
) -> Vec<Change> {
    let have: BTreeMap<&str, &str> = installed
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    m.package
        .iter()
        .map(|p| {
            let from = have.get(p.name.as_str()).map(|v| (*v).to_string());
            let kind = match &from {
                None => ChangeKind::Install,
                Some(v) if *v == p.version => ChangeKind::Same,
                Some(v) if newer(&p.version, v) => ChangeKind::Upgrade,
                Some(_) => ChangeKind::Older,
            };
            Change {
                name: p.name.clone(),
                from,
                to: p.version.clone(),
                kind,
            }
        })
        .collect()
}

/// `pacman.conf` with `LocalFileSigLevel = Required`: pacman then refuses a
/// local package file that is unsigned or wrongly signed.
pub fn signed_config(conf: &str) -> Result<String> {
    let mut out = Vec::new();
    let mut in_options = false;
    let mut seen_options = false;
    let mut replaced = false;
    for line in conf.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_options = trimmed == "[options]";
            seen_options |= in_options;
            out.push(line.to_string());
            if in_options {
                out.push("LocalFileSigLevel = Required".to_string());
                replaced = true;
            }
            continue;
        }
        if in_options && trimmed.starts_with("LocalFileSigLevel") {
            continue; // dropped: ours comes first and pacman uses the last one it reads
        }
        out.push(line.to_string());
    }
    if !seen_options || !replaced {
        return Err(bad("pacman.conf has no [options] section"));
    }
    // pacman takes the last assignment; make ours the last one in [options].
    let text = out.join("\n");
    Ok(text + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn pkg(name: &str, version: &str) -> BundlePackage {
        BundlePackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            file: format!("{name}-{version}-x86_64.pkg.tar.zst"),
            size: 3,
            sha256: H.into(),
            sig_sha256: H.into(),
        }
    }

    fn manifest(packages: Vec<BundlePackage>) -> Manifest {
        Manifest {
            meta: Meta {
                format: FORMAT,
                emrac: "0.3.0".into(),
                created: "2026-10-18".into(),
                arch: "x86_64".into(),
            },
            requested: vec!["a".into()],
            for_target: None,
            package: packages,
            skipped: vec![Skipped {
                name: "aur-thing".into(),
                why: "from the AUR".into(),
            }],
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("emrac-bundle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("pkg")).unwrap();
        d
    }

    fn newer(a: &str, b: &str) -> bool {
        let n = |v: &str| {
            v.split('-')
                .next()
                .and_then(|x| x.parse::<u32>().ok())
                .unwrap_or(0)
        };
        n(a) > n(b)
    }

    #[test]
    fn the_manifest_round_trips() {
        let m = manifest(vec![pkg("a", "1-1"), pkg("b", "2-1")]);
        assert_eq!(parse(&render(&m)).unwrap(), m);
    }

    #[test]
    fn a_hostile_manifest_is_refused() {
        let ok = manifest(vec![pkg("a", "1-1")]);
        assert!(parse(&render(&ok)).is_ok());
        type Mutation = Box<dyn Fn(&mut Manifest)>;
        let cases: Vec<(&str, Mutation)> = vec![
            (
                "a path in the file name",
                Box::new(|m| m.package[0].file = "../../etc/passwd".into()),
            ),
            (
                "a file of another package",
                Box::new(|m| m.package[0].file = "evil-1-1-x86_64.pkg.tar.zst".into()),
            ),
            (
                "a signature as the package",
                Box::new(|m| m.package[0].file = "a-1-1-x86_64.pkg.tar.zst.sig".into()),
            ),
            (
                "a hidden file",
                Box::new(|m| m.package[0].file = ".a-1-1.pkg.tar.zst".into()),
            ),
            (
                "not a package archive",
                Box::new(|m| m.package[0].file = "a-1-1-x86_64.txt".into()),
            ),
            (
                "an option as the name",
                Box::new(|m| {
                    m.package[0].name = "--noconfirm".into();
                    m.package[0].file = "--noconfirm-1-1.pkg.tar.zst".into()
                }),
            ),
            (
                "a short checksum",
                Box::new(|m| m.package[0].sha256 = "abc".into()),
            ),
            (
                "the same file twice",
                Box::new(|m| {
                    let p = m.package[0].clone();
                    m.package.push(p)
                }),
            ),
            ("another format", Box::new(|m| m.meta.format = 7)),
            (
                "a strange architecture",
                Box::new(|m| m.meta.arch = "x86 64; rm".into()),
            ),
        ];
        for (why, change) in cases {
            let mut m = ok.clone();
            change(&mut m);
            assert!(parse(&render(&m)).is_err(), "{why}");
        }
        assert!(parse("not toml [").is_err());
    }

    #[test]
    fn only_what_the_target_lacks_or_has_older_is_needed() {
        let sp = |n: &str, v: &str| SyncPackage {
            name: n.into(),
            version: v.into(),
            repo: "extra".into(),
            filename: format!("{n}-{v}-x86_64.pkg.tar.zst"),
            download_size: 1,
        };
        let closure = [sp("a", "5"), sp("b", "3"), sp("c", "9")];
        let target = BTreeMap::from([
            ("a".to_string(), "4".to_string()),
            ("b".to_string(), "3".to_string()),
        ]);
        let names: Vec<_> = needed(&closure, &target, &newer)
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(
            names,
            ["a", "c"],
            "a is older there, b is current, c is missing"
        );
    }

    fn write_bundle(d: &Path, packages: &[(&str, &[u8])]) -> Manifest {
        let mut list = Vec::new();
        for (name, bytes) in packages {
            let mut p = pkg(name, "1-1");
            fs::write(d.join("pkg").join(&p.file), bytes).unwrap();
            fs::write(d.join("pkg").join(format!("{}.sig", p.file)), b"sig").unwrap();
            p.size = bytes.len() as u64;
            p.sha256 = sha256_file(&d.join("pkg").join(&p.file)).unwrap();
            p.sig_sha256 = sha256_file(&d.join("pkg").join(format!("{}.sig", p.file))).unwrap();
            list.push(p);
        }
        manifest(list)
    }

    #[test]
    fn a_good_bundle_verifies() {
        let d = scratch("good");
        let m = write_bundle(&d, &[("a", b"hello"), ("b", b"world!")]);
        assert_eq!(verify_dir(&d, &m), Vec::<String>::new());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn damage_is_found_one_way_or_another() {
        let d = scratch("bad");
        let m = write_bundle(&d, &[("a", b"hello"), ("b", b"world!")]);
        // a changed byte, same size
        fs::write(d.join("pkg").join(&m.package[0].file), b"hellO").unwrap();
        // a missing signature
        fs::remove_file(d.join("pkg").join(format!("{}.sig", m.package[1].file))).unwrap();
        // a file nobody listed, and a link
        fs::write(d.join("pkg/extra.pkg.tar.zst"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("pkg/link")).unwrap();
        let problems = verify_dir(&d, &m).join("\n");
        assert!(
            problems.contains("a: the package does not match its checksum"),
            "{problems}"
        );
        assert!(
            problems.contains("b: the signature file is missing"),
            "{problems}"
        );
        assert!(
            problems.contains("extra.pkg.tar.zst: in the bundle but not in the manifest"),
            "{problems}"
        );
        assert!(
            problems.contains("link: in the bundle but not in the manifest"),
            "{problems}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_size_that_differs_and_a_symlinked_package_are_found() {
        let d = scratch("size");
        let mut m = write_bundle(&d, &[("a", b"hello"), ("b", b"world!")]);
        m.package[0].size = 99;
        let real = d.join("pkg").join(&m.package[1].file);
        fs::rename(&real, d.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(d.join("elsewhere"), &real).unwrap();
        let problems = verify_dir(&d, &m).join("\n");
        assert!(
            problems.contains("a: the package has a different size"),
            "{problems}"
        );
        assert!(
            problems.contains("b: the package is not a plain file"),
            "{problems}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_comparison_never_downgrades() {
        let m = manifest(vec![
            pkg("a", "5-1"),
            pkg("b", "3-1"),
            pkg("c", "2-1"),
            pkg("d", "9-1"),
        ]);
        let installed = vec![
            ("a".to_string(), "4-1".to_string()),
            ("b".to_string(), "3-1".to_string()),
            ("c".to_string(), "7-1".to_string()),
        ];
        let kinds: Vec<_> = compare(&m, &installed, &newer)
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                ChangeKind::Upgrade,
                ChangeKind::Same,
                ChangeKind::Older,
                ChangeKind::Install
            ]
        );
    }

    #[test]
    fn the_signed_config_requires_signatures_and_changes_nothing_else() {
        let conf = "[options]\nHoldPkg = pacman\nLocalFileSigLevel = Optional\nSigLevel = Required\n\n[core]\nInclude = /etc/pacman.d/mirrorlist\n";
        let out = signed_config(conf).unwrap();
        assert!(out.contains("LocalFileSigLevel = Required"));
        assert!(!out.contains("Optional"));
        assert!(
            out.contains("HoldPkg = pacman")
                && out.contains("[core]\nInclude = /etc/pacman.d/mirrorlist")
        );
        assert_eq!(out.matches("LocalFileSigLevel").count(), 1);
        // none there at all
        let out = signed_config("[options]\nHoldPkg = pacman\n").unwrap();
        assert!(out.contains("LocalFileSigLevel = Required"));
        // not a pacman.conf
        assert!(signed_config("nothing here\n").is_err());
        // a comment mentioning it is left alone
        let out = signed_config("[options]\n#LocalFileSigLevel = Optional\n").unwrap();
        assert!(
            out.contains("#LocalFileSigLevel = Optional")
                && out.contains("LocalFileSigLevel = Required")
        );
    }
}

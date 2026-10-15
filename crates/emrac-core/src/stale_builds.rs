//! Which AUR (foreign) packages need building again (u19, from
//! `additions.md` A14): the ones a library or language update has broken, and
//! the ones that may break because something they were built against changed
//! after they were built. Every rule is a pure function over facts that the
//! caller reads; `scan` reads them. See `docs/plan/r2/u19-rebuild.md`.

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// What an ELF file says about the libraries it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElfInfo {
    pub needed: Vec<String>,
    /// `DT_RPATH` and `DT_RUNPATH` entries, `$ORIGIN` not yet replaced.
    pub search: Vec<String>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Reads `DT_NEEDED` and the search paths of a little-endian ELF file (32 or
/// 64 bit) without running anything. `None`: not such a file, or not readable
/// as one. Only the headers and the dynamic section are read.
pub fn read_elf(path: &Path) -> Option<ElfInfo> {
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; 64];
    f.read_exact(&mut head).ok()?;
    if &head[..4] != b"\x7fELF" || head[5] != 1 {
        return None;
    }
    let wide = match head[4] {
        2 => true,
        1 => false,
        _ => return None,
    };
    let (phoff, phentsize, phnum) = if wide {
        (
            u64_at(&head, 0x20)?,
            u16_at(&head, 0x36)? as u64,
            u16_at(&head, 0x38)? as u64,
        )
    } else {
        (
            u32_at(&head, 0x1c)? as u64,
            u16_at(&head, 0x2a)? as u64,
            u16_at(&head, 0x2c)? as u64,
        )
    };
    if phnum == 0 || phnum > 256 || phentsize < if wide { 56 } else { 32 } {
        return None;
    }
    let mut table = vec![0u8; (phentsize * phnum) as usize];
    f.seek(SeekFrom::Start(phoff)).ok()?;
    f.read_exact(&mut table).ok()?;

    // (offset, vaddr, filesz) of the PT_LOAD segments and the dynamic section.
    let mut loads = Vec::new();
    let mut dynamic = None;
    for i in 0..phnum as usize {
        let p = i * phentsize as usize;
        let kind = u32_at(&table, p)?;
        let (off, vaddr, filesz) = if wide {
            (
                u64_at(&table, p + 8)?,
                u64_at(&table, p + 16)?,
                u64_at(&table, p + 32)?,
            )
        } else {
            (
                u32_at(&table, p + 4)? as u64,
                u32_at(&table, p + 8)? as u64,
                u32_at(&table, p + 16)? as u64,
            )
        };
        match kind {
            1 => loads.push((off, vaddr, filesz)),
            2 => dynamic = Some((off, filesz)),
            _ => {}
        }
    }
    let (dyn_off, dyn_size) = dynamic?;
    if dyn_size == 0 || dyn_size > 1 << 20 {
        return None;
    }
    let mut dyn_bytes = vec![0u8; dyn_size as usize];
    f.seek(SeekFrom::Start(dyn_off)).ok()?;
    f.read_exact(&mut dyn_bytes).ok()?;

    let entry = if wide { 16 } else { 8 };
    let mut needed = Vec::new();
    let mut search = Vec::new();
    let mut strtab = None;
    for chunk in dyn_bytes.chunks_exact(entry) {
        let (tag, val) = if wide {
            (u64_at(chunk, 0)? as i64, u64_at(chunk, 8)?)
        } else {
            (u32_at(chunk, 0)? as i32 as i64, u32_at(chunk, 4)? as u64)
        };
        match tag {
            0 => break,
            1 => needed.push(val),
            5 => strtab = Some(val),
            15 | 29 => search.push(val),
            _ => {}
        }
    }
    let strtab_off = {
        let vaddr = strtab?;
        let (off, base, _) = loads
            .iter()
            .copied()
            .find(|(_, v, size)| vaddr >= *v && vaddr < v + size)?;
        off + (vaddr - base)
    };
    let string = |at: u64| -> Option<String> {
        let mut f = File::open(path).ok()?;
        f.seek(SeekFrom::Start(strtab_off + at)).ok()?;
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while out.len() < 512 {
            f.read_exact(&mut byte).ok()?;
            if byte[0] == 0 {
                return String::from_utf8(out).ok();
            }
            out.push(byte[0]);
        }
        None
    };
    Some(ElfInfo {
        needed: needed.into_iter().filter_map(string).collect(),
        search: search
            .into_iter()
            .filter_map(string)
            .flat_map(|s| s.split(':').map(str::to_string).collect::<Vec<_>>())
            .collect(),
    })
}

/// The needed libraries that nothing provides: not in `system` (file names in
/// the library directories), not shipped by the package itself, and not
/// in one of the file's own search paths (`$ORIGIN` is the file's folder).
pub fn missing_libs(
    info: &ElfInfo,
    origin: &Path,
    system: &HashSet<String>,
    own: &HashSet<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let dirs: Vec<PathBuf> = info
        .search
        .iter()
        .map(|d| {
            PathBuf::from(
                d.replace("$ORIGIN", &origin.display().to_string())
                    .replace("${ORIGIN}", &origin.display().to_string()),
            )
        })
        .collect();
    info.needed
        .iter()
        .filter(|lib| {
            // A needed name with a slash is a path (`$ORIGIN/../lib/libx.so`).
            if lib.contains('/') {
                let path = PathBuf::from(
                    lib.replace("$ORIGIN", &origin.display().to_string())
                        .replace("${ORIGIN}", &origin.display().to_string()),
                );
                return !exists(&path);
            }
            !system.contains(*lib)
                && !own.contains(*lib)
                && !dirs.iter().any(|d| exists(&d.join(lib)))
        })
        .cloned()
        .collect()
}

/// Interpreters whose version is part of a path their packages install into.
const INTERPRETERS: [(&str, &str); 2] = [("python", "usr/lib/python"), ("perl", "usr/lib/perl5/")];

/// `3.14` from `3.14.0-1`; `5.42` from `5.42.0-1`.
pub fn minor_of(version: &str) -> Option<String> {
    let v = version.split_once(':').map_or(version, |(_, rest)| rest);
    let mut parts = v.split(['.', '-']);
    let (a, b) = (parts.next()?, parts.next()?);
    (a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()))
        .then(|| format!("{a}.{b}"))
}

/// Files installed under another interpreter version than the one now on the
/// system: `usr/lib/python3.13/...` while Python is 3.14. One line per
/// interpreter, naming the old version.
pub fn interpreter_mismatches(files: &[String], versions: &[(&str, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, prefix) in INTERPRETERS {
        let Some((_, now)) = versions.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let mut old: Vec<String> = Vec::new();
        for f in files {
            let Some(rest) = f.strip_prefix(prefix) else {
                continue;
            };
            // python: "3.13/site-packages/..." (prefix "usr/lib/python"); perl: "5.40.0/..." or "5.40/..."
            let dir = rest.split('/').next().unwrap_or("");
            let Some(ver) = minor_of(&format!("{dir}-")) else {
                continue;
            };
            if ver != *now && !old.contains(&ver) {
                old.push(ver);
            }
        }
        for v in old {
            out.push(format!(
                "installs into {name} {v}'s folder, but {name} is now {now}"
            ));
        }
    }
    out
}

/// One `upgraded` line of pacman's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upgrade {
    pub time: u64,
    pub package: String,
    pub from: String,
    pub to: String,
}

/// `2026-10-15T21:40:08+0530` as unix seconds.
pub fn parse_log_time(s: &str) -> Option<u64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = rest.get(..8)?;
    let mut t = time.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    let zone = rest.get(8..)?;
    let offset = if zone.len() == 5 {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let hh: i64 = zone.get(1..3)?.parse().ok()?;
        let mm: i64 = zone.get(3..5)?.parse().ok()?;
        sign * (hh * 3600 + mm * 60)
    } else {
        0
    };
    // Days from civil (Howard Hinnant).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec - offset).ok()
}

/// The `upgraded name (old -> new)` lines of a pacman log.
pub fn parse_upgrades(log: &str) -> Vec<Upgrade> {
    log.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix('[')?;
            let (stamp, rest) = rest.split_once("] [ALPM] upgraded ")?;
            let (package, versions) = rest.split_once(" (")?;
            let (from, to) = versions.trim_end_matches(')').split_once(" -> ")?;
            Some(Upgrade {
                time: parse_log_time(stamp)?,
                package: package.to_string(),
                from: from.to_string(),
                to: to.to_string(),
            })
        })
        .collect()
}

/// Libraries where a new minor version can break what was built before it
/// without changing a soname.
const WATCHED: [(&str, &str); 2] = [("qt6-base", "qt6-"), ("qt5-base", "qt5-")];

/// Upgrades of a watched library, after `built`, that changed its minor
/// version, for a package that depends on that library family.
pub fn abi_bumps(upgrades: &[Upgrade], built: u64, depends: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (lib, family) in WATCHED {
        if !depends.iter().any(|d| d.starts_with(family)) {
            continue;
        }
        let mut changes: Vec<&Upgrade> = upgrades
            .iter()
            .filter(|u| u.package == lib && u.time > built)
            .filter(|u| minor_of(&u.from) != minor_of(&u.to))
            .collect();
        changes.sort_by_key(|u| u.time);
        if let (Some(first), Some(last)) = (changes.first(), changes.last()) {
            out.push(format!(
                "built before {lib} went {} → {} ({})",
                minor_of(&first.from).unwrap_or_else(|| first.from.clone()),
                minor_of(&last.to).unwrap_or_else(|| last.to.clone()),
                crate::history::format_utc_date(last.time)
            ));
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleBuild {
    pub name: String,
    pub version: String,
    /// Unix seconds of the build.
    pub built: u64,
    /// Broken now: a library is missing, or it sits in an old interpreter's folder.
    pub broken: Vec<String>,
    /// May break: something it was built against changed after.
    pub maybe: Vec<String>,
}

/// What `scan` needs to know about one foreign package.
#[derive(Debug, Clone)]
pub struct Foreign {
    pub name: String,
    pub version: String,
    pub built: u64,
    pub depends: Vec<String>,
    pub files: Vec<String>,
}

/// File names present in the system's library directories.
pub fn system_libs() -> HashSet<String> {
    let mut dirs: Vec<PathBuf> = ["/usr/lib", "/usr/lib32", "/usr/lib64", "/lib", "/lib64"]
        .iter()
        .map(PathBuf::from)
        .collect();
    // Directories named in /etc/ld.so.conf and its includes.
    let mut conf: Vec<PathBuf> = vec![PathBuf::from("/etc/ld.so.conf")];
    let mut seen = 0;
    while let Some(c) = conf.pop() {
        seen += 1;
        if seen > 64 {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&c) else {
            continue;
        };
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if let Some(pattern) = line.strip_prefix("include ") {
                if let Some(dir) = Path::new(pattern.trim()).parent() {
                    if let Ok(rd) = std::fs::read_dir(dir) {
                        conf.extend(
                            rd.flatten()
                                .map(|e| e.path())
                                .filter(|p| p.extension().is_some_and(|x| x == "conf")),
                        );
                    }
                }
            } else {
                dirs.push(PathBuf::from(line));
            }
        }
    }
    let mut out = HashSet::new();
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            out.extend(
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned()),
            );
        }
    }
    out
}

/// Programs and libraries in the system's own folders: a missing library
/// there is a real breakage. Deeper folders and `/opt` are usually bundled
/// applications, whose missing libraries are often optional plugins.
fn is_primary(rel: &str) -> bool {
    if rel.starts_with("usr/bin/") {
        return true;
    }
    ["usr/lib/", "usr/lib32/"]
        .iter()
        .any(|p| rel.strip_prefix(p).is_some_and(|rest| !rest.contains('/')))
}

const SCANNED: [&str; 5] = ["usr/bin/", "usr/lib/", "usr/lib32/", "usr/libexec/", "opt/"];
const MAX_FILES: usize = 6000;

/// Judges one package. `interpreters` are the installed versions of python
/// and perl as `(name, minor)`.
pub fn judge(
    pkg: &Foreign,
    system: &HashSet<String>,
    interpreters: &[(&str, String)],
    upgrades: &[Upgrade],
    root: &Path,
) -> StaleBuild {
    let own: HashSet<String> = pkg
        .files
        .iter()
        .filter_map(|f| f.rsplit('/').next().map(str::to_string))
        .collect();
    let mut broken: Vec<String> = Vec::new();
    let mut maybe: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut bundled: Vec<String> = Vec::new();
    for rel in pkg
        .files
        .iter()
        .filter(|f| !f.ends_with('/') && SCANNED.iter().any(|p| f.starts_with(p)))
        .take(MAX_FILES)
    {
        let path = root.join(rel);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(info) = read_elf(&path) else {
            continue;
        };
        let origin = path.parent().unwrap_or(Path::new("/"));
        let target = if is_primary(rel) {
            &mut missing
        } else {
            &mut bundled
        };
        for lib in missing_libs(&info, origin, system, &own, &|p| p.exists()) {
            if !target.contains(&lib) {
                target.push(lib);
            }
        }
    }
    missing.sort();
    bundled.sort();
    bundled.retain(|l| !missing.contains(l));
    if !missing.is_empty() {
        broken.push(format!(
            "needs {}, which nothing installed provides",
            missing
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !bundled.is_empty() {
        maybe.push(format!(
            "a bundled program or plugin needs {}{}, which nothing installed provides (often optional)",
            bundled.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
            if bundled.len() > 3 { format!(" and {} more", bundled.len() - 3) } else { String::new() }
        ));
    }
    broken.extend(interpreter_mismatches(&pkg.files, interpreters));
    StaleBuild {
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        built: pkg.built,
        broken,
        maybe: {
            maybe.extend(abi_bumps(upgrades, pkg.built, &pkg.depends));
            maybe
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_elf_reader_agrees_with_readelf_on_a_real_binary_and_a_library() {
        let Ok(out) = std::process::Command::new("readelf")
            .args(["-d", "/usr/bin/true"])
            .output()
        else {
            eprintln!("readelf is not installed: skipped");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let expected: Vec<String> = text
            .lines()
            .filter(|l| l.contains("(NEEDED)"))
            .filter_map(|l| l.split('[').nth(1)?.split(']').next().map(str::to_string))
            .collect();
        let info = read_elf(Path::new("/usr/bin/true")).expect("an ELF file");
        assert_eq!(info.needed, expected);
        assert!(!info.needed.is_empty());
        assert!(
            read_elf(Path::new("/etc/passwd")).is_none(),
            "not an ELF file"
        );
        assert!(read_elf(Path::new("/nonexistent")).is_none());
        let lib = read_elf(Path::new("/usr/lib/libz.so.1"));
        if let Some(lib) = lib {
            assert!(
                lib.needed.iter().any(|n| n.starts_with("libc.so")),
                "{lib:?}"
            );
        }
    }

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_library_is_missing_unless_the_system_the_package_or_a_search_path_has_it() {
        let info = ElfInfo {
            needed: vec![
                "libc.so.6".into(),
                "libgone.so.2".into(),
                "libmine.so.1".into(),
                "libplug.so.3".into(),
            ],
            search: vec!["$ORIGIN/../lib".into()],
        };
        let system = set(&["libc.so.6"]);
        let own = set(&["libmine.so.1"]);
        let exists = |p: &Path| p == Path::new("/opt/app/bin/../lib/libplug.so.3");
        let missing = missing_libs(&info, Path::new("/opt/app/bin"), &system, &own, &exists);
        assert_eq!(missing, ["libgone.so.2"]);
        let none = missing_libs(&ElfInfo::default(), Path::new("/"), &system, &own, &exists);
        assert!(none.is_empty());
    }

    #[test]
    fn a_needed_name_with_a_path_is_checked_as_a_path() {
        let info = ElfInfo {
            needed: vec![
                "$ORIGIN/../lib/libpy.so.1".into(),
                "$ORIGIN/../lib/libgone.so.1".into(),
            ],
            search: vec![],
        };
        let exists = |p: &Path| p == Path::new("/opt/app/bin/../lib/libpy.so.1");
        let m = missing_libs(
            &info,
            Path::new("/opt/app/bin"),
            &HashSet::new(),
            &HashSet::new(),
            &exists,
        );
        assert_eq!(m, ["$ORIGIN/../lib/libgone.so.1"]);
    }

    #[test]
    fn only_the_systems_own_folders_count_as_broken_and_bundled_apps_as_maybe() {
        assert!(is_primary("usr/bin/prog"));
        assert!(is_primary("usr/lib/libfoo.so.1"));
        assert!(is_primary("usr/lib32/libfoo.so.1"));
        assert!(!is_primary("usr/lib/foo/plugins/x.so"));
        assert!(!is_primary("opt/app/bin/app"));
    }

    #[test]
    fn interpreter_folders_of_an_old_version_are_found() {
        let files: Vec<String> = [
            "usr/lib/python3.13/site-packages/foo/__init__.py",
            "usr/lib/python3.13/site-packages/foo/x.py",
            "usr/lib/perl5/5.40/vendor_perl/Foo.pm",
            "usr/share/foo/readme",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let versions = [("python", "3.14".to_string()), ("perl", "5.42".to_string())];
        let m = interpreter_mismatches(&files, &versions);
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(
            m[0].contains("python 3.13") && m[0].contains("now 3.14"),
            "{m:?}"
        );
        assert!(m[1].contains("perl 5.40"), "{m:?}");
        assert!(interpreter_mismatches(&files, &[("python", "3.13".to_string())]).is_empty());
        assert!(
            interpreter_mismatches(&files, &[]).is_empty(),
            "no interpreter installed: nothing to say"
        );
        assert_eq!(minor_of("3.14.0-1").as_deref(), Some("3.14"));
        assert_eq!(minor_of("1:5.42.0-1").as_deref(), Some("5.42"));
        assert_eq!(minor_of("garbage"), None);
    }

    #[test]
    fn pacman_log_lines_give_upgrades_with_their_times() {
        let log = "\
[2026-10-14T20:25:34+0530] [ALPM] upgraded qt6-base (6.11.2-1 -> 6.12.0-3)
[2026-10-14T20:25:35+0530] [ALPM] installed foo (1.0-1)
[2026-10-15T09:00:00+0000] [ALPM] upgraded qt6-base (6.12.0-3 -> 6.12.0-4)
not a log line
";
        let u = parse_upgrades(log);
        assert_eq!(u.len(), 2);
        assert_eq!(
            (u[0].package.as_str(), u[0].from.as_str(), u[0].to.as_str()),
            ("qt6-base", "6.11.2-1", "6.12.0-3")
        );
        assert_eq!(parse_log_time("1970-01-01T00:00:00+0000"), Some(0));
        assert_eq!(parse_log_time("1970-01-01T05:30:00+0530"), Some(0));
        assert_eq!(
            parse_log_time("2026-10-15T00:00:00+0000"),
            Some(1_792_022_400)
        );
        assert!(parse_log_time("yesterday").is_none());
    }

    #[test]
    fn a_minor_bump_of_qt_after_the_build_may_break_what_depends_on_qt() {
        let upgrades = parse_upgrades(
            "[2026-10-14T20:25:34+0000] [ALPM] upgraded qt6-base (6.11.2-1 -> 6.12.0-3)\n[2026-10-15T09:00:00+0000] [ALPM] upgraded qt6-base (6.12.0-3 -> 6.12.0-4)\n",
        );
        let built_before = parse_log_time("2026-08-21T21:57:18+0000").unwrap();
        let built_after = parse_log_time("2026-10-14T21:00:00+0000").unwrap();
        let deps = vec!["qt6-declarative".to_string(), "qt6-base".to_string()];
        let m = abi_bumps(&upgrades, built_before, &deps);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(
            m[0].contains("qt6-base went 6.11 → 6.12") && m[0].contains("2026-10-14"),
            "{m:?}"
        );
        assert!(
            abi_bumps(&upgrades, built_after, &deps).is_empty(),
            "built after the bump: fine"
        );
        assert!(
            abi_bumps(&upgrades, built_before, &["gtk3".to_string()]).is_empty(),
            "does not use Qt"
        );
        // a patch-level bump alone is not a change of ABI
        let patch = parse_upgrades(
            "[2026-10-15T09:00:00+0000] [ALPM] upgraded qt6-base (6.12.0-3 -> 6.12.1-1)\n",
        );
        assert!(abi_bumps(&patch, built_before, &deps).is_empty());
    }

    #[test]
    fn a_package_is_judged_from_its_files_its_dependencies_and_the_log() {
        let dir = std::env::temp_dir().join(format!("emrac-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
        std::fs::copy("/usr/bin/true", dir.join("usr/bin/prog")).unwrap();
        let pkg = Foreign {
            name: "prog".into(),
            version: "1-1".into(),
            built: 0,
            depends: vec![],
            files: vec![
                "usr/bin/".into(),
                "usr/bin/prog".into(),
                "usr/lib/python3.13/site-packages/a.py".into(),
            ],
        };
        // The system provides nothing: libc is "missing" for the copy of /usr/bin/true.
        let stale = judge(
            &pkg,
            &HashSet::new(),
            &[("python", "3.14".to_string())],
            &[],
            &dir,
        );
        assert!(
            stale.broken.iter().any(|b| b.contains("needs libc.so.6")),
            "{:?}",
            stale.broken
        );
        assert!(
            stale.broken.iter().any(|b| b.contains("python 3.13")),
            "{:?}",
            stale.broken
        );
        // With the libraries present it is only the interpreter folder.
        let libs = system_libs();
        let ok = judge(&pkg, &libs, &[], &[], &dir);
        assert!(ok.broken.is_empty(), "{:?}", ok.broken);
        // The same file under /opt is a bundled program: only a maybe.
        std::fs::create_dir_all(dir.join("opt/app")).unwrap();
        std::fs::copy("/usr/bin/true", dir.join("opt/app/app")).unwrap();
        let bundled = Foreign {
            files: vec!["opt/app/app".into()],
            ..pkg.clone()
        };
        let b = judge(&bundled, &HashSet::new(), &[], &[], &dir);
        assert!(b.broken.is_empty(), "{:?}", b.broken);
        assert!(
            b.maybe
                .iter()
                .any(|m| m.contains("bundled") && m.contains("libc.so.6")),
            "{:?}",
            b.maybe
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

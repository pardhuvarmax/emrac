//! Update risk before an upgrade and the report after it (u20, part 5;
//! `additions.md` A5, A7, A19 and the post-commit health check). Rules over
//! facts; `Sources::upgrade_facts` reads the system. Without root only what a
//! normal user can see is looked at, and the report says so.
//! See `docs/plan/r2/u20-trust.md`.

use std::collections::BTreeSet;

use serde::Serialize;

/// One package an upgrade changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub name: String,
    pub from: Option<String>,
    pub to: String,
}

// -------------------------------------------------------------- bumps

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bump {
    /// Only the package release number (`-1` to `-2`), or no change.
    Release,
    Patch,
    Minor,
    Major,
}

fn numbers(version: &str) -> Vec<u64> {
    let v = version.split_once(':').map_or(version, |(_, r)| r);
    let v = v.split('-').next().unwrap_or(v);
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// How big a step `from` → `to` is, by the leading numbers of the version.
pub fn bump(from: &str, to: &str) -> Bump {
    let (a, b) = (numbers(from), numbers(to));
    let at = |v: &Vec<u64>, i: usize| v.get(i).copied().unwrap_or(0);
    if at(&a, 0) != at(&b, 0) {
        Bump::Major
    } else if at(&a, 1) != at(&b, 1) {
        Bump::Minor
    } else if a.get(2..) != b.get(2..) {
        Bump::Patch
    } else {
        Bump::Release
    }
}

// --------------------------------------------------------------- risk

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Low,
    Medium,
    High,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Low => "LOW",
            Level::Medium => "MEDIUM",
            Level::High => "HIGH",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Risk {
    pub level: Level,
    pub reasons: Vec<String>,
}

fn is_kernel(name: &str) -> bool {
    matches!(name, "linux" | "linux-lts" | "linux-zen" | "linux-hardened")
        || name.starts_with("linux-rt")
        || name.starts_with("linux-cachyos")
}

/// Parts of the system whose upgrade can leave it unbootable or broken.
const CRITICAL: [&str; 9] = [
    "glibc",
    "systemd",
    "pacman",
    "grub",
    "limine",
    "mkinitcpio",
    "filesystem",
    "openssl",
    "linux-api-headers",
];
/// Parts that break things often enough to warrant care.
const CAREFUL: [&str; 14] = [
    "nvidia",
    "nvidia-dkms",
    "nvidia-utils",
    "mesa",
    "xorg-server",
    "wayland",
    "linux-firmware",
    "gcc",
    "gcc-libs",
    "binutils",
    "python",
    "perl",
    "qt6-base",
    "qt5-base",
];

/// The risk of a set of changes. `aur_rebuilds` and `critical_news` come
/// from outside the plan.
pub fn risk(changes: &[Change], aur_rebuilds: usize, critical_news: usize) -> Risk {
    let mut high: Vec<String> = Vec::new();
    let mut medium: Vec<String> = Vec::new();
    let step = |c: &Change| {
        let kind = c.from.as_deref().map_or(Bump::Major, |f| bump(f, &c.to));
        let what = match &c.from {
            Some(f) => format!("{} {f} → {}", c.name, c.to),
            None => format!("{} {} (new)", c.name, c.to),
        };
        (kind, what)
    };
    for c in changes {
        let (kind, what) = step(c);
        if kind == Bump::Release && c.from.is_some() {
            continue;
        }
        let name = c.name.as_str();
        if is_kernel(name) {
            if kind >= Bump::Minor {
                high.push(format!("kernel: {what}"));
            } else {
                medium.push(format!("kernel: {what}"));
            }
        } else if CRITICAL.contains(&name) {
            // systemd numbers its stable point releases 259.1, 259.2: only a
            // new first number is a new systemd.
            let high_from = if name == "systemd" {
                Bump::Major
            } else {
                Bump::Minor
            };
            if kind >= high_from {
                high.push(what);
            } else {
                medium.push(what);
            }
        } else if (CAREFUL.contains(&name)
            || name.starts_with("nvidia")
            || name.starts_with("lib32-nvidia"))
            && (kind >= Bump::Minor || name.contains("nvidia"))
        {
            medium.push(what);
        }
    }
    if critical_news > 0 {
        high.push(format!(
            "{critical_news} unread Arch news item{} marked critical",
            if critical_news == 1 { " is" } else { "s are" }
        ));
    }
    if aur_rebuilds > 0 {
        medium.push(format!(
            "{aur_rebuilds} AUR package{} to build",
            if aur_rebuilds == 1 { "" } else { "s" }
        ));
    }
    if changes.len() > 150 {
        medium.push(format!("{} packages change", changes.len()));
    }
    let level = if !high.is_empty() {
        Level::High
    } else if !medium.is_empty() {
        Level::Medium
    } else {
        Level::Low
    };
    high.extend(medium);
    high.truncate(8);
    Risk {
        level,
        reasons: high,
    }
}

// ------------------------------------------------------------- report

/// What the report needs to know about the machine.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub changes: Vec<Change>,
    /// Unix seconds when the upgrade began.
    pub since: u64,
    /// `uname -r`.
    pub running_kernel: String,
    /// `(directory under /usr/lib/modules, its pkgbase)`.
    pub kernel_dirs: Vec<(String, String)>,
    /// Active services shipped by changed packages: `(unit, package, started)`.
    pub services: Vec<(String, String, u64)>,
    pub failed_now: Vec<String>,
    /// `None`: the failures before the upgrade are not known.
    pub failed_before: Option<Vec<String>>,
    /// AUR packages broken now (u19), binary ones left out.
    pub stale_broken: Vec<String>,
    /// `.pacnew`/`.pacsave` files new since the upgrade began.
    pub pacnew_new: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub notable: Vec<String>,
    /// Why a reboot is advised (empty: not needed).
    pub reboot: Vec<String>,
    /// `(unit, package)` to restart.
    pub restart: Vec<(String, String)>,
    pub failed: Vec<String>,
    /// The failures are those that appeared during the upgrade (the ones
    /// before were known); otherwise they are simply what fails now.
    pub failed_new: bool,
    pub stale: Vec<String>,
    pub pacnew_new: usize,
}

impl Report {
    /// Nothing needs doing.
    pub fn all_clear(&self) -> bool {
        self.reboot.is_empty()
            && self.restart.is_empty()
            && self.failed.is_empty()
            && self.stale.is_empty()
            && self.pacnew_new == 0
    }
}

/// The kernel package a release string belongs to, by its suffix.
fn kernel_family(release: &str) -> Option<&'static str> {
    if release.ends_with("-zen") {
        Some("linux-zen")
    } else if release.ends_with("-lts") {
        Some("linux-lts")
    } else if release.ends_with("-hardened") {
        Some("linux-hardened")
    } else if release.contains("-arch") {
        Some("linux")
    } else {
        None
    }
}

/// Packages whose upgrade leaves running programs on the old copy of a core
/// library, so that logging out or rebooting is the clean answer.
const REBOOT_PACKAGES: [&str; 3] = ["glibc", "systemd", "linux-firmware"];

pub fn assess(f: &Facts, newer: &dyn Fn(&str, &str) -> bool) -> Report {
    let mut r = Report::default();

    for c in &f.changes {
        let name = c.name.as_str();
        let big = is_kernel(name)
            || CRITICAL.contains(&name)
            || CAREFUL.contains(&name)
            || name.starts_with("nvidia");
        if big {
            let b = c
                .from
                .as_deref()
                .map_or(Bump::Major, |from| bump(from, &c.to));
            if b > Bump::Release || c.from.is_none() {
                r.notable.push(match &c.from {
                    Some(from) => format!("{name} {from} → {}", c.to),
                    None => format!("{name} {} (new)", c.to),
                });
            }
        }
    }

    // The kernel in use against the newest installed of the same kind. The
    // kind is read from the modules folder of the running kernel, or, when an
    // upgrade has removed that folder, from the release's suffix (`-zen`):
    // with no installed kernel of that kind nothing is said, which is also
    // what a container, running the host's kernel, needs.
    let exact = f
        .kernel_dirs
        .iter()
        .find(|(dir, _)| *dir == f.running_kernel)
        .map(|(_, base)| base.clone());
    let removed = exact.is_none();
    let base = exact.or_else(|| {
        let family = kernel_family(&f.running_kernel)?;
        f.kernel_dirs
            .iter()
            .any(|(_, b)| b == family)
            .then(|| family.to_string())
    });
    if let Some(base) = base {
        let newest = f
            .kernel_dirs
            .iter()
            .filter(|(_, b)| *b == base)
            .map(|(d, _)| d.as_str())
            .fold(None::<&str>, |best, d| match best {
                Some(b) if !newer(b, d) => Some(b),
                _ => Some(d),
            });
        if let Some(newest) = newest {
            if removed {
                r.reboot.push(format!(
                    "{base} was upgraded to {newest} and you are still running {}, whose modules are gone, so devices that load a module later will fail",
                    f.running_kernel
                ));
            } else if newest != f.running_kernel && newer(&f.running_kernel, newest) {
                r.reboot.push(format!(
                    "{base} {newest} is installed, you are running {}",
                    f.running_kernel
                ));
            }
        }
    }
    for c in &f.changes {
        if REBOOT_PACKAGES.contains(&c.name.as_str())
            && c.from
                .as_deref()
                .is_some_and(|from| bump(from, &c.to) > Bump::Release)
        {
            r.reboot.push(format!(
                "{} was upgraded: programs already running keep using the old copy until they restart",
                c.name
            ));
        }
    }

    // Services started before the upgrade, from a package it changed.
    let mut seen = BTreeSet::new();
    for (unit, package, started) in &f.services {
        if *started < f.since && seen.insert(unit.clone()) {
            r.restart.push((unit.clone(), package.clone()));
        }
    }
    r.restart.sort();

    r.failed_new = f.failed_before.is_some();
    r.failed = match &f.failed_before {
        Some(before) => f
            .failed_now
            .iter()
            .filter(|u| !before.contains(u))
            .cloned()
            .collect(),
        None => f.failed_now.clone(),
    };
    r.stale = f.stale_broken.clone();
    r.pacnew_new = f.pacnew_new;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(name: &str, from: Option<&str>, to: &str) -> Change {
        Change {
            name: name.into(),
            from: from.map(str::to_string),
            to: to.into(),
        }
    }

    fn newer(a: &str, b: &str) -> bool {
        // b newer than a, by comparing the numbers
        numbers(b) > numbers(a)
    }

    #[test]
    fn version_steps_are_classified() {
        assert_eq!(bump("2.43-1", "2.43-2"), Bump::Release);
        assert_eq!(bump("2.43.1-1", "2.43.2-1"), Bump::Patch);
        assert_eq!(bump("2.43-1", "2.44-1"), Bump::Minor);
        assert_eq!(bump("2.43-1", "3.0-1"), Bump::Major);
        assert_eq!(
            bump("1:6.11.2-1", "1:6.12.0-3"),
            Bump::Minor,
            "an epoch is ignored"
        );
        assert_eq!(bump("7.2.9.zen1-1", "7.2.10.zen1-1"), Bump::Patch);
    }

    #[test]
    fn the_risk_is_high_for_the_kernel_the_c_library_and_critical_news_and_low_for_ordinary_updates(
    ) {
        let ordinary = vec![
            ch("bash", Some("5.2-1"), "5.3-1"),
            ch("curl", Some("8.1-1"), "8.1-2"),
        ];
        let r = risk(&ordinary, 0, 0);
        assert_eq!(r.level, Level::Low);
        assert!(r.reasons.is_empty());

        let kernel = risk(
            &[ch("linux-zen", Some("7.2.9.zen1-1"), "7.3.0.zen1-1")],
            0,
            0,
        );
        assert_eq!(kernel.level, Level::High);
        assert!(
            kernel.reasons[0].starts_with("kernel: linux-zen"),
            "{:?}",
            kernel.reasons
        );
        let patch = risk(
            &[ch("linux", Some("7.2.9.arch1-1"), "7.2.10.arch1-1")],
            0,
            0,
        );
        assert_eq!(patch.level, Level::Medium, "a patch release of the kernel");

        let glibc = risk(&[ch("glibc", Some("2.43-1"), "2.44-1")], 0, 0);
        assert_eq!(glibc.level, Level::High);
        assert_eq!(
            risk(&[ch("glibc", Some("2.43-1"), "2.43-2")], 0, 0).level,
            Level::Low,
            "a rebuild only"
        );
        assert_eq!(
            risk(&[ch("systemd", Some("270.1-1"), "270.2-1")], 0, 0).level,
            Level::Medium
        );

        let news = risk(&ordinary, 0, 2);
        assert_eq!(news.level, Level::High);
        assert!(
            news.reasons[0].contains("2 unread Arch news items are marked critical"),
            "{:?}",
            news.reasons
        );
        assert_eq!(risk(&ordinary, 3, 0).level, Level::Medium);
        let gpu = risk(&[ch("nvidia-utils", Some("600.1-1"), "600.2-1")], 0, 0);
        assert_eq!(gpu.level, Level::Medium);
        let many: Vec<Change> = (0..151)
            .map(|i| ch(&format!("p{i}"), Some("1-1"), "2-1"))
            .collect();
        assert_eq!(risk(&many, 0, 0).level, Level::Medium);
        let mixed = risk(
            &[
                ch("glibc", Some("2.43-1"), "2.44-1"),
                ch("mesa", Some("26.0-1"), "26.1-1"),
            ],
            1,
            0,
        );
        assert_eq!(mixed.level, Level::High);
        assert!(
            mixed.reasons.iter().any(|r| r.contains("mesa"))
                && mixed.reasons.iter().any(|r| r.contains("AUR"))
        );
    }

    fn base_facts() -> Facts {
        Facts {
            changes: vec![
                ch("linux-zen", Some("7.2.8.zen1-1"), "7.2.9.zen1-1"),
                ch("openssh", Some("10.0-1"), "10.1-1"),
            ],
            since: 1_000,
            running_kernel: "7.2.8-zen1-1-zen".into(),
            kernel_dirs: vec![
                ("7.2.9-zen1-1-zen".into(), "linux-zen".into()),
                ("6.18.56-1-lts".into(), "linux-lts".into()),
            ],
            ..Facts::default()
        }
    }

    #[test]
    fn a_removed_or_older_running_kernel_calls_for_a_reboot() {
        let r = assess(&base_facts(), &newer);
        assert!(
            r.reboot
                .iter()
                .any(|x| x.contains("linux-zen was upgraded to 7.2.9-zen1-1-zen")
                    && x.contains("whose modules are gone")),
            "{:?}",
            r.reboot
        );
        let mut f = base_facts();
        f.kernel_dirs
            .push(("7.2.8-zen1-1-zen".into(), "linux-zen".into()));
        let r = assess(&f, &newer);
        assert_eq!(r.reboot.len(), 1, "{:?}", r.reboot);
        assert!(
            r.reboot[0].contains(
                "linux-zen 7.2.9-zen1-1-zen is installed, you are running 7.2.8-zen1-1-zen"
            ),
            "{:?}",
            r.reboot
        );
        let mut current = base_facts();
        current.running_kernel = "7.2.9-zen1-1-zen".into();
        assert!(assess(&current, &newer).reboot.is_empty());
        // another kernel being newer than the running one is not the running one's concern
        current
            .kernel_dirs
            .push(("7.0.1-1-lts".into(), "linux-lts".into()));
        assert!(assess(&current, &newer).reboot.is_empty());
        let none = Facts {
            running_kernel: String::new(),
            ..Facts::default()
        };
        assert!(
            assess(&none, &newer).reboot.is_empty(),
            "nothing known: nothing said"
        );
    }

    #[test]
    fn core_libraries_and_services_started_before_the_upgrade_are_listed() {
        let mut f = Facts {
            since: 5_000,
            running_kernel: "k".into(),
            ..Facts::default()
        };
        f.changes = vec![
            ch("glibc", Some("2.43-1"), "2.44-1"),
            ch("openssh", Some("10.0-1"), "10.1-1"),
            ch("linux-firmware", Some("1-1"), "1-2"),
        ];
        f.services = vec![
            ("sshd.service".into(), "openssh".into(), 100),
            ("cron.service".into(), "cronie".into(), 9_000),
            ("sshd.service".into(), "openssh".into(), 100),
        ];
        let r = assess(&f, &newer);
        assert_eq!(
            r.restart,
            vec![("sshd.service".to_string(), "openssh".to_string())]
        );
        assert_eq!(
            r.reboot.len(),
            1,
            "glibc yes, a firmware rebuild no: {:?}",
            r.reboot
        );
        assert!(r.reboot[0].starts_with("glibc"));
        assert!(!r.all_clear());
    }

    #[test]
    fn only_new_failures_count_when_the_old_ones_are_known() {
        let mut f = Facts {
            running_kernel: "k".into(),
            ..Facts::default()
        };
        f.failed_now = vec!["vbox.service".into(), "new.service".into()];
        f.failed_before = Some(vec!["vbox.service".into()]);
        let r = assess(&f, &newer);
        assert_eq!(r.failed, ["new.service"]);
        assert!(r.failed_new);
        f.failed_before = None;
        assert!(!assess(&f, &newer).failed_new);
        assert_eq!(assess(&f, &newer).failed.len(), 2);
        f.stale_broken = vec!["tool".into()];
        f.pacnew_new = 2;
        let r = assess(&f, &newer);
        assert_eq!((r.stale.len(), r.pacnew_new), (1, 2));
        assert!(
            Facts::default().changes.is_empty() && assess(&Facts::default(), &newer).all_clear()
        );
    }

    #[test]
    fn notable_changes_name_the_big_steps_only() {
        let f = Facts {
            changes: vec![
                ch("linux", Some("7.2.9-1"), "7.3.0-1"),
                ch("glibc", Some("2.43-1"), "2.43-2"),
                ch("bash", Some("5.2-1"), "5.3-1"),
                ch("mesa", None, "26.1-1"),
            ],
            running_kernel: String::new(),
            ..Facts::default()
        };
        let r = assess(&f, &newer);
        assert_eq!(r.notable, ["linux 7.2.9-1 → 7.3.0-1", "mesa 26.1-1 (new)"]);
    }
}

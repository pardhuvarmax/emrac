//! Pre-flight checks (u17): the things that break a transaction halfway or
//! leave a machine unbootable, looked at before anything is changed. Facts are
//! gathered from the system (`gather`) and judged by `evaluate`, a pure
//! function the tests drive. See `docs/plan/r2/u17-preflight.md`.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::doctor::{Check, Status};
use crate::plan::{Plan, PlanAction};

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fs {
    pub avail: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default)]
pub struct KernelFacts {
    /// The package, which is also the name used in `/boot` (`linux`, `linux-lts`).
    pub package: String,
    /// Size of `/boot/vmlinuz-<package>` and the initramfs images, when readable.
    pub files: Vec<(String, u64)>,
    /// `Some(true/false)`: whether `/boot/initramfs-<package>-fallback.img` exists;
    /// `None`: `/boot` can't be read.
    pub fallback_initramfs: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct NetFacts {
    pub tried: Vec<String>,
    pub answered: Vec<String>,
}

/// Upgrades waiting that this transaction would leave behind.
#[derive(Debug, Clone, Default)]
pub struct Partial {
    pub pending: Vec<String>,
    /// Packages you pinned on purpose; not counted.
    pub pinned: usize,
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub action: PlanAction,
    pub offline: bool,
    pub names: Vec<String>,
    /// Packages removed by conflict resolution, besides the named ones.
    pub removed_names: Vec<String>,
    pub download: u64,
    /// Installed-size growth (0 when the plan shrinks the system).
    pub growth: u64,
    pub aur_builds: bool,
    pub touches_bootloader: bool,
    pub root: Option<Fs>,
    pub cache: Option<Fs>,
    pub boot: Option<Fs>,
    /// The EFI system partition and where it is mounted.
    pub esp: Option<(String, Fs)>,
    pub battery: Option<u8>,
    pub on_ac: Option<bool>,
    pub net: Option<NetFacts>,
    pub kernels: Vec<KernelFacts>,
    pub boot_readable: bool,
    /// Bootloader configuration files that could be read: path and text.
    pub boot_configs: Vec<(String, String)>,
    pub partial: Option<Partial>,
}

impl Facts {
    /// A starting point for tests and for `gather`.
    pub fn for_plan(plan: &Plan, offline: bool, aur_builds: bool) -> Facts {
        Facts {
            action: plan.action,
            offline,
            names: plan.packages.iter().map(|p| p.name.clone()).collect(),
            removed_names: plan.removals.iter().map(|r| r.name.clone()).collect(),
            download: plan.total_download_size,
            growth: plan.total_installed_size_delta.max(0) as u64,
            aur_builds,
            touches_bootloader: false,
            root: None,
            cache: None,
            boot: None,
            esp: None,
            battery: None,
            on_ac: None,
            net: None,
            kernels: Vec::new(),
            boot_readable: false,
            boot_configs: Vec::new(),
            partial: None,
        }
    }

    fn touched_kernels(&self) -> Vec<&KernelFacts> {
        if !matches!(
            self.action,
            PlanAction::Upgrade | PlanAction::Reinstall | PlanAction::Downgrade
        ) {
            return Vec::new();
        }
        self.kernels
            .iter()
            .filter(|k| self.names.contains(&k.package))
            .collect()
    }
}

fn check(
    id: &'static str,
    title: &'static str,
    status: Status,
    summary: impl Into<String>,
) -> Check {
    Check {
        id,
        title,
        status,
        summary: summary.into(),
        detail: Vec::new(),
        fix: None,
    }
}

/// Whether `text` names the kernel image `image` as a whole word, so that
/// `vmlinuz-linux` is not found inside `vmlinuz-linux-zen`.
pub fn names_image(text: &str, image: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(image) {
        let end = from + at + image.len();
        let next = text[end..].chars().next();
        if !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return true;
        }
        from = end;
    }
    false
}

fn mib(bytes: u64) -> String {
    format!("{} MiB", bytes / MIB)
}

/// The judgement. Every applicable check is returned, passing ones included.
pub fn evaluate(f: &Facts) -> Vec<Check> {
    let mut out = Vec::new();
    let installing = matches!(
        f.action,
        PlanAction::Install | PlanAction::Upgrade | PlanAction::Downgrade | PlanAction::Reinstall
    );
    let kernels = f.touched_kernels();

    // Space on / and where the cache lives.
    if installing {
        let same = f.root.is_some() && f.root == f.cache;
        if let Some(root) = f.root {
            let need = f.growth + if same { f.download } else { 0 } + 512 * MIB;
            out.push(if root.avail < need {
                let mut c = check(
                    "space-root",
                    "Space on /",
                    Status::Problem,
                    format!("{} free, this needs about {}", mib(root.avail), mib(need)),
                );
                c.fix = Some("emrac cache clean   (then `emrac orphans`)".into());
                c
            } else {
                check(
                    "space-root",
                    "Space on /",
                    Status::Ok,
                    format!("{} free", mib(root.avail)),
                )
            });
        }
        if let (Some(cache), false) = (f.cache, same) {
            if f.download > 0 {
                let need = f.download + 256 * MIB;
                out.push(if cache.avail < need {
                    let mut c = check(
                        "space-cache",
                        "Space for downloads",
                        Status::Problem,
                        format!(
                            "{} free in the package cache, the download is {}",
                            mib(cache.avail),
                            mib(f.download)
                        ),
                    );
                    c.fix = Some("emrac cache clean".into());
                    c
                } else {
                    check(
                        "space-cache",
                        "Space for downloads",
                        Status::Ok,
                        format!("{} free", mib(cache.avail)),
                    )
                });
            }
        }
    }

    // Space where a kernel lands.
    if !kernels.is_empty() {
        let largest = kernels
            .iter()
            .flat_map(|k| k.files.iter().map(|(_, s)| *s))
            .max();
        if let Some(boot) = f.boot {
            let need = largest.map_or(128 * MIB, |m| m + m / 2 + 16 * MIB);
            out.push(if boot.avail < need {
                let mut c = check(
                    "space-boot",
                    "Space on /boot",
                    Status::Problem,
                    format!(
                        "{} free, a kernel upgrade needs about {}: a full /boot mid-upgrade leaves the machine unbootable",
                        mib(boot.avail),
                        mib(need)
                    ),
                );
                c.fix = Some("remove old kernels or initramfs images from /boot, or enlarge it".into());
                c
            } else {
                check("space-boot", "Space on /boot", Status::Ok, format!("{} free", mib(boot.avail)))
            });
        }
    }
    if !kernels.is_empty() || f.touches_bootloader {
        if let Some((mount, esp)) = &f.esp {
            let need = 64 * MIB;
            out.push(if esp.avail < need {
                check(
                    "space-esp",
                    "Space on the EFI partition",
                    Status::Problem,
                    format!(
                        "{} free on {mount}, at least {} is needed",
                        mib(esp.avail),
                        mib(need)
                    ),
                )
            } else {
                check(
                    "space-esp",
                    "Space on the EFI partition",
                    Status::Ok,
                    format!("{} free on {mount}", mib(esp.avail)),
                )
            });
        }
    }

    // Power.
    let risky = !kernels.is_empty() || f.aur_builds || f.download > 500 * MIB;
    if let (true, Some(pct), Some(false)) = (risky, f.battery, f.on_ac) {
        out.push(if pct < 20 {
            let mut c = check(
                "power",
                "Power",
                Status::Problem,
                format!("on battery at {pct} %: a long upgrade or a build could run it flat in the middle"),
            );
            c.fix = Some("plug in the charger".into());
            c
        } else if pct < 50 {
            check("power", "Power", Status::Warning, format!("on battery at {pct} %: plug in for a long upgrade or a build"))
        } else {
            check("power", "Power", Status::Note, format!("on battery at {pct} %"))
        });
    }

    // Network.
    if !f.offline && (f.download > 0 || f.aur_builds) {
        if let Some(net) = &f.net {
            if !net.tried.is_empty() {
                out.push(if net.answered.is_empty() {
                    let mut c = check(
                        "network",
                        "Network",
                        Status::Problem,
                        "none of the first mirrors answered: the download would fail",
                    );
                    c.detail = net
                        .tried
                        .iter()
                        .map(|m| format!("no answer from {m}"))
                        .collect();
                    c.fix = Some(
                        "check the connection, or run with --offline if everything is cached"
                            .into(),
                    );
                    c
                } else if net.answered.len() < net.tried.len() {
                    check(
                        "network",
                        "Network",
                        Status::Note,
                        format!(
                            "{} of {} mirrors answered; pacman tries the others",
                            net.answered.len(),
                            net.tried.len()
                        ),
                    )
                } else {
                    check("network", "Network", Status::Ok, "the mirrors answer")
                });
            }
        }
    }

    // The kernel safety net.
    if !kernels.is_empty() {
        let names: Vec<&str> = kernels.iter().map(|k| k.package.as_str()).collect();
        let other: Vec<&str> = f
            .kernels
            .iter()
            .map(|k| k.package.as_str())
            .filter(|p| !names.contains(p))
            .collect();
        out.push(if other.is_empty() {
            let mut c = check(
                "kernel-fallback",
                "Fallback kernel",
                Status::Warning,
                format!("{} is the only installed kernel: if the new one does not boot there is nothing to boot instead", names.join(", ")),
            );
            c.fix = Some(
                if names.contains(&"linux-lts") {
                    "sudo pacman -S linux linux-headers"
                } else {
                    "sudo pacman -S linux-lts linux-lts-headers"
                }
                .into(),
            );
            c
        } else {
            check("kernel-fallback", "Fallback kernel", Status::Ok, format!("{} also installed", other.join(", ")))
        });

        let missing: Vec<&str> = kernels
            .iter()
            .filter(|k| k.fallback_initramfs == Some(false))
            .map(|k| k.package.as_str())
            .collect();
        out.push(if !f.boot_readable {
            check(
                "kernel-initramfs",
                "Fallback initramfs",
                Status::Note,
                "/boot can't be read as a normal user, so this was not checked",
            )
        } else if missing.is_empty() {
            check(
                "kernel-initramfs",
                "Fallback initramfs",
                Status::Ok,
                "present",
            )
        } else {
            let mut c = check(
                "kernel-initramfs",
                "Fallback initramfs",
                Status::Note,
                format!("no initramfs-{}-fallback.img in /boot", missing.join(", ")),
            );
            c.detail.push(
                "the fallback preset in /etc/mkinitcpio.d/ was probably removed on purpose".into(),
            );
            c
        });

        for k in &kernels {
            let image = format!("vmlinuz-{}", k.package);
            out.push(if f.boot_configs.is_empty() {
                check(
                    "kernel-bootloader",
                    "Boot entry",
                    Status::Note,
                    if f.boot_readable {
                        "no GRUB, systemd-boot or Limine configuration was recognised, so the boot entry was not checked".to_string()
                    } else {
                        "the bootloader configuration can't be read as a normal user, so the boot entry was not checked".to_string()
                    },
                )
            } else if f.boot_configs.iter().any(|(_, text)| names_image(text, &image)) {
                check("kernel-bootloader", "Boot entry", Status::Ok, format!("the bootloader configuration names {image}"))
            } else {
                let mut c = check(
                    "kernel-bootloader",
                    "Boot entry",
                    Status::Warning,
                    format!("the bootloader configuration does not name {image}"),
                );
                c.detail = f.boot_configs.iter().map(|(p, _)| format!("read {p}")).collect();
                c.fix = Some("regenerate the bootloader entries (for GRUB: sudo grub-mkconfig -o /boot/grub/grub.cfg)".into());
                c
            });
        }
    }

    // Taking away the last kernel.
    if matches!(f.action, PlanAction::Remove | PlanAction::Purge) && !f.kernels.is_empty() {
        let removed = |p: &str| f.names.iter().chain(&f.removed_names).any(|n| n == p);
        if f.kernels.iter().all(|k| removed(&k.package)) {
            let mut c = check(
                "kernel-removal",
                "Last kernel",
                Status::Problem,
                "this removes every installed kernel: the machine would not boot",
            );
            c.detail = f
                .kernels
                .iter()
                .map(|k| format!("removes {}", k.package))
                .collect();
            c.fix = Some("install another kernel first (sudo pacman -S linux-lts)".into());
            out.push(c);
        }
    }

    // Partial upgrade.
    if let Some(p) = &f.partial {
        out.push(if p.pending.is_empty() {
            check("partial-upgrade", "Partial upgrade", Status::Ok, "no upgrades are waiting that this would leave behind")
        } else {
            let mut c = check(
                "partial-upgrade",
                "Partial upgrade",
                Status::Problem,
                format!(
                    "{} installed package{} {} an upgrade waiting; adding or upgrading only some packages is a partial upgrade, which Arch does not support",
                    p.pending.len(),
                    if p.pending.len() == 1 { "" } else { "s" },
                    if p.pending.len() == 1 { "has" } else { "have" }
                ),
            );
            c.detail = p.pending.iter().take(12).cloned().collect();
            if p.pending.len() > 12 {
                c.detail.push(format!("and {} more", p.pending.len() - 12));
            }
            if p.pinned > 0 {
                c.detail.push(format!("({} pinned package{} not counted)", p.pinned, if p.pinned == 1 { "" } else { "s" }));
            }
            c.fix = Some("emrac upgrade   (or --allow-partial for this one run)".into());
            c
        });
    }

    out.sort_by_key(|c| std::cmp::Reverse(c.status));
    out
}

// ------------------------------------------------------------ gathering

fn df(path: &str) -> Option<Fs> {
    let out = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    let (avail, total) = crate::doctor::parse_df(&String::from_utf8_lossy(&out.stdout))?;
    Some(Fs { avail, total })
}

/// `(mount point, file system type)` pairs of `/proc/mounts`.
pub fn parse_mounts(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (_, mount, fstype) = (f.next()?, f.next()?, f.next()?);
            Some((mount.replace("\\040", " "), fstype.to_string()))
        })
        .collect()
}

/// `host:port` of an `https://` or `http://` mirror address.
pub fn host_port(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    if host.is_empty() {
        return None;
    }
    let default = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    Some(if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{default}")
    })
}

fn reachable(host_port: &str) -> bool {
    host_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(|_| true))
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(4)).is_ok())
}

fn mirror_facts() -> Option<NetFacts> {
    let repo = Command::new("pacman-conf")
        .arg("--repo-list")
        .output()
        .ok()?;
    let first = String::from_utf8_lossy(&repo.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    let servers = Command::new("pacman-conf")
        .arg(format!("--repo={first}"))
        .arg("Server")
        .output()
        .ok()?;
    let mut net = NetFacts::default();
    for url in String::from_utf8_lossy(&servers.stdout).lines() {
        if net.tried.len() == 3 {
            break;
        }
        let Some(hp) = host_port(url.trim()) else {
            continue;
        };
        if net.tried.contains(&hp) {
            continue;
        }
        net.tried.push(hp.clone());
        if reachable(&hp) {
            net.answered.push(hp);
        }
    }
    Some(net)
}

/// Battery percentage and whether mains power is connected.
fn power_facts() -> (Option<u8>, Option<bool>) {
    let (mut battery, mut ac) = (None, None);
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let read = |f: &str| {
                std::fs::read_to_string(e.path().join(f))
                    .ok()
                    .map(|s| s.trim().to_string())
            };
            match read("type").as_deref() {
                Some("Battery") => {
                    if battery.is_none() {
                        battery = read("capacity").and_then(|c| c.parse().ok());
                    }
                }
                Some("Mains") => {
                    let online = read("online").is_some_and(|o| o == "1");
                    ac = Some(ac.unwrap_or(false) || online);
                }
                _ => {}
            }
        }
    }
    (battery, if battery.is_some() { ac } else { None })
}

/// Kernel packages named by the `pkgbase` files under `/usr/lib/modules`.
pub fn kernel_bases(pkgbase_files: &[String]) -> Vec<String> {
    let mut v: Vec<String> = pkgbase_files
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

const BOOT_CONFIGS: [&str; 8] = [
    "/boot/grub/grub.cfg",
    "/boot/limine.conf",
    "/boot/limine.cfg",
    "/efi/limine.conf",
    "/boot/EFI/limine/limine.conf",
    "/boot/loader/loader.conf",
    "/efi/loader/loader.conf",
    "/boot/efi/loader/loader.conf",
];

fn read_capped(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut s = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(2 * 1024 * 1024)
        .read_to_string(&mut s)
        .ok()?;
    Some(s)
}

/// Collects the facts about this machine for `plan`. `is_installed` says
/// whether a package is installed; `partial` is filled by the caller.
pub fn gather(
    plan: &Plan,
    offline: bool,
    aur_builds: bool,
    is_installed: &dyn Fn(&str) -> bool,
) -> Facts {
    let mut f = Facts::for_plan(plan, offline, aur_builds);
    f.root = df("/");
    f.cache = df("/var/cache/pacman/pkg");
    f.boot = df("/boot");
    let mounts = std::fs::read_to_string("/proc/mounts")
        .map(|t| parse_mounts(&t))
        .unwrap_or_default();
    f.esp = ["/efi", "/boot/efi", "/boot"].iter().find_map(|m| {
        mounts
            .iter()
            .any(|(mp, ty)| mp == m && matches!(ty.as_str(), "vfat" | "fat" | "msdos"))
            .then(|| df(m).map(|fs| (m.to_string(), fs)))
            .flatten()
    });
    (f.battery, f.on_ac) = power_facts();
    if !offline && (plan.total_download_size > 0 || aur_builds) {
        f.net = mirror_facts();
    }

    let bases: Vec<String> = std::fs::read_dir("/usr/lib/modules")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path().join("pkgbase")).ok())
                .collect()
        })
        .unwrap_or_default();
    f.boot_readable = std::fs::read_dir("/boot").is_ok();
    f.kernels = kernel_bases(&bases)
        .into_iter()
        .filter(|b| is_installed(b))
        .map(|package| {
            let mut files = Vec::new();
            let mut fallback = None;
            if f.boot_readable {
                for name in [
                    format!("vmlinuz-{package}"),
                    format!("initramfs-{package}.img"),
                    format!("initramfs-{package}-fallback.img"),
                ] {
                    if let Ok(m) = std::fs::metadata(Path::new("/boot").join(&name)) {
                        files.push((name, m.len()));
                    }
                }
                fallback = Some(files.iter().any(|(n, _)| n.ends_with("-fallback.img")));
            }
            KernelFacts {
                package,
                files,
                fallback_initramfs: fallback,
            }
        })
        .collect();

    for path in BOOT_CONFIGS {
        if let Some(text) = read_capped(Path::new(path)) {
            f.boot_configs.push((path.to_string(), text));
        }
    }
    for dir in [
        "/boot/loader/entries",
        "/efi/loader/entries",
        "/boot/efi/loader/entries",
    ] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten().take(64) {
                if let Some(text) = read_capped(&e.path()) {
                    f.boot_configs.push((e.path().display().to_string(), text));
                }
            }
        }
    }
    f.touches_bootloader = plan.packages.iter().any(|p| {
        matches!(
            p.name.as_str(),
            "grub" | "systemd" | "limine" | "refind" | "efibootmgr"
        )
    });
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlannedPackage;

    fn plan(action: PlanAction, names: &[&str]) -> Plan {
        Plan {
            action,
            packages: names
                .iter()
                .map(|n| PlannedPackage {
                    name: n.to_string(),
                    version: "2-1".into(),
                    repo: "core".into(),
                    explicit: true,
                    previous_version: Some("1-1".into()),
                })
                .collect(),
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: vec![],
            provider_choices: vec![],
            warnings: vec![],
        }
    }

    fn facts(action: PlanAction, names: &[&str]) -> Facts {
        let mut f = Facts::for_plan(&plan(action, names), false, false);
        f.root = Some(Fs {
            avail: 50 * 1024 * MIB,
            total: 100 * 1024 * MIB,
        });
        f.cache = f.root;
        f.boot_readable = true;
        f
    }

    fn kernel(name: &str) -> KernelFacts {
        KernelFacts {
            package: name.into(),
            files: vec![
                (format!("vmlinuz-{name}"), 15 * MIB),
                (format!("initramfs-{name}-fallback.img"), 90 * MIB),
            ],
            fallback_initramfs: Some(true),
        }
    }

    fn by<'a>(cs: &'a [Check], id: &str) -> &'a Check {
        cs.iter().find(|c| c.id == id).unwrap_or_else(|| {
            panic!(
                "no {id} in {:?}",
                cs.iter().map(|c| c.id).collect::<Vec<_>>()
            )
        })
    }

    #[test]
    fn space_on_root_and_for_downloads_is_judged_and_added_when_they_share_a_disk() {
        let mut f = facts(PlanAction::Install, &["a"]);
        f.growth = 1024 * MIB;
        f.download = 100 * MIB;
        f.root = Some(Fs {
            avail: 4096 * MIB,
            total: 9000 * MIB,
        });
        f.cache = f.root;
        assert_eq!(by(&evaluate(&f), "space-root").status, Status::Ok);
        f.root = Some(Fs {
            avail: 1500 * MIB,
            total: 9000 * MIB,
        });
        f.cache = f.root;
        let c = evaluate(&f);
        assert_eq!(
            by(&c, "space-root").status,
            Status::Problem,
            "1024 + 100 + 512 > 1500"
        );
        assert!(
            c.iter().all(|x| x.id != "space-cache"),
            "one disk: one check"
        );
        f.cache = Some(Fs {
            avail: 200 * MIB,
            total: 900 * MIB,
        });
        f.root = Some(Fs {
            avail: 9000 * MIB,
            total: 20_000 * MIB,
        });
        let c = evaluate(&f);
        assert_eq!(
            by(&c, "space-cache").status,
            Status::Problem,
            "100 + 256 > 200"
        );
        assert_eq!(by(&c, "space-root").status, Status::Ok);
        let remove = facts(PlanAction::Remove, &["a"]);
        assert!(
            evaluate(&remove).iter().all(|c| !c.id.starts_with("space")),
            "removing needs no space"
        );
    }

    #[test]
    fn a_kernel_upgrade_checks_boot_space_fallbacks_and_the_boot_entry() {
        let mut f = facts(PlanAction::Upgrade, &["linux"]);
        f.kernels = vec![kernel("linux"), kernel("linux-lts")];
        f.boot = Some(Fs {
            avail: 400 * MIB,
            total: 1024 * MIB,
        });
        f.esp = Some((
            "/boot".into(),
            Fs {
                avail: 400 * MIB,
                total: 1024 * MIB,
            },
        ));
        f.boot_configs = vec![(
            "/boot/grub/grub.cfg".into(),
            "linux /vmlinuz-linux root=/dev/x".into(),
        )];
        let c = evaluate(&f);
        for id in [
            "space-boot",
            "space-esp",
            "kernel-fallback",
            "kernel-initramfs",
            "kernel-bootloader",
        ] {
            assert_eq!(by(&c, id).status, Status::Ok, "{id}");
        }
        // Nearly full, a single kernel, no entry for it.
        f.boot = Some(Fs {
            avail: 100 * MIB,
            total: 1024 * MIB,
        });
        f.kernels = vec![kernel("linux")];
        f.boot_configs = vec![(
            "/boot/grub/grub.cfg".into(),
            "linux /vmlinuz-linux-zen".into(),
        )];
        let c = evaluate(&f);
        assert_eq!(
            by(&c, "space-boot").status,
            Status::Problem,
            "1.5 x 90 + 16 = 151 MiB needed"
        );
        assert_eq!(by(&c, "kernel-fallback").status, Status::Warning);
        assert!(by(&c, "kernel-fallback")
            .fix
            .as_deref()
            .unwrap()
            .contains("linux-lts"));
        assert_eq!(by(&c, "kernel-bootloader").status, Status::Warning);
        // Only an LTS kernel: the suggestion is the other way round.
        f.kernels = vec![kernel("linux-lts")];
        f.names = vec!["linux-lts".into()];
        assert!(by(&evaluate(&f), "kernel-fallback")
            .fix
            .as_deref()
            .unwrap()
            .contains("pacman -S linux linux-headers"));
        // A package that is not a kernel touches none of this.
        let other = facts(PlanAction::Upgrade, &["bash"]);
        assert!(evaluate(&other)
            .iter()
            .all(|c| !c.id.starts_with("kernel") && c.id != "space-boot"));
    }

    #[test]
    fn an_unreadable_boot_degrades_to_notes_and_a_fixed_space_floor() {
        let mut f = facts(PlanAction::Upgrade, &["linux"]);
        f.kernels = vec![
            KernelFacts {
                package: "linux".into(),
                files: vec![],
                fallback_initramfs: None,
            },
            kernel("linux-zen"),
        ];
        f.boot_readable = false;
        f.boot = Some(Fs {
            avail: 100 * MIB,
            total: 1024 * MIB,
        });
        let c = evaluate(&f);
        assert_eq!(
            by(&c, "space-boot").status,
            Status::Problem,
            "128 MiB floor when sizes are unknown"
        );
        assert_eq!(by(&c, "kernel-initramfs").status, Status::Note);
        assert_eq!(by(&c, "kernel-bootloader").status, Status::Note);
        assert!(by(&c, "kernel-bootloader")
            .summary
            .contains("can't be read"));
    }

    #[test]
    fn removing_the_last_kernel_is_a_problem_and_removing_one_of_two_is_not() {
        let mut f = facts(PlanAction::Remove, &["linux"]);
        f.kernels = vec![kernel("linux")];
        assert_eq!(by(&evaluate(&f), "kernel-removal").status, Status::Problem);
        f.kernels = vec![kernel("linux"), kernel("linux-lts")];
        assert!(evaluate(&f).iter().all(|c| c.id != "kernel-removal"));
        f.names = vec!["linux".into()];
        f.removed_names = vec!["linux-lts".into()];
        assert_eq!(
            by(&evaluate(&f), "kernel-removal").status,
            Status::Problem,
            "conflict removals count"
        );
    }

    #[test]
    fn power_matters_only_on_battery_and_only_for_risky_work() {
        let mut f = facts(PlanAction::Install, &["a"]);
        f.battery = Some(10);
        f.on_ac = Some(false);
        assert!(
            evaluate(&f).iter().all(|c| c.id != "power"),
            "a small install is not risky"
        );
        f.aur_builds = true;
        assert_eq!(by(&evaluate(&f), "power").status, Status::Problem);
        f.battery = Some(35);
        assert_eq!(by(&evaluate(&f), "power").status, Status::Warning);
        f.battery = Some(80);
        assert_eq!(by(&evaluate(&f), "power").status, Status::Note);
        f.on_ac = Some(true);
        assert!(evaluate(&f).iter().all(|c| c.id != "power"));
        f.on_ac = None;
        f.battery = None;
        assert!(
            evaluate(&f).iter().all(|c| c.id != "power"),
            "no battery: nothing to say"
        );
    }

    #[test]
    fn the_network_check_needs_a_download_and_distinguishes_some_from_none() {
        let mut f = facts(PlanAction::Install, &["a"]);
        f.net = Some(NetFacts {
            tried: vec!["a:443".into(), "b:443".into()],
            answered: vec![],
        });
        assert!(
            evaluate(&f).iter().all(|c| c.id != "network"),
            "nothing to download"
        );
        f.download = 10 * MIB;
        assert_eq!(by(&evaluate(&f), "network").status, Status::Problem);
        f.net = Some(NetFacts {
            tried: vec!["a:443".into(), "b:443".into()],
            answered: vec!["b:443".into()],
        });
        assert_eq!(by(&evaluate(&f), "network").status, Status::Note);
        f.net = Some(NetFacts {
            tried: vec!["a:443".into()],
            answered: vec!["a:443".into()],
        });
        assert_eq!(by(&evaluate(&f), "network").status, Status::Ok);
        f.offline = true;
        assert!(evaluate(&f).iter().all(|c| c.id != "network"));
    }

    #[test]
    fn a_waiting_upgrade_makes_a_partial_upgrade_a_problem_that_lists_the_packages() {
        let mut f = facts(PlanAction::Install, &["newpkg"]);
        f.partial = Some(Partial {
            pending: vec![],
            pinned: 0,
        });
        assert_eq!(by(&evaluate(&f), "partial-upgrade").status, Status::Ok);
        f.partial = Some(Partial {
            pending: (0..15).map(|i| format!("pkg{i}")).collect(),
            pinned: 2,
        });
        let c = evaluate(&f);
        let p = by(&c, "partial-upgrade");
        assert_eq!(p.status, Status::Problem);
        assert!(
            p.summary.starts_with("15 installed packages have"),
            "{}",
            p.summary
        );
        assert_eq!(
            p.detail.len(),
            14,
            "12 shown, 'and 3 more', and the pinned note: {:?}",
            p.detail
        );
        assert!(p.detail.iter().any(|d| d.contains("and 3 more")));
        assert!(p.detail.iter().any(|d| d.contains("2 pinned packages")));
        assert!(p.fix.as_deref().unwrap().contains("emrac upgrade"));
        assert_eq!(c[0].id, "partial-upgrade", "problems come first");
    }

    #[test]
    fn a_kernel_image_is_found_as_a_whole_word_only() {
        assert!(names_image(
            "linux /vmlinuz-linux root=/dev/x",
            "vmlinuz-linux"
        ));
        assert!(names_image(
            "linux /vmlinuz-linux\ninitrd /initramfs-linux.img",
            "vmlinuz-linux"
        ));
        assert!(names_image(
            "kernel_path: boot():/vmlinuz-linux",
            "vmlinuz-linux"
        ));
        assert!(!names_image(
            "linux /vmlinuz-linux-zen root=/dev/x",
            "vmlinuz-linux"
        ));
        assert!(!names_image("linux /vmlinuz-linux_2", "vmlinuz-linux"));
        assert!(names_image(
            "a /vmlinuz-linux-zen\nb /vmlinuz-linux",
            "vmlinuz-linux"
        ));
        assert!(!names_image("", "vmlinuz-linux"));
    }

    #[test]
    fn mount_tables_and_mirror_addresses_are_parsed() {
        let mounts = parse_mounts("/dev/nvme0n1p1 /boot vfat rw 0 0\n/dev/sda2 / ext4 rw 0 0\nsys /sys\\040x sysfs rw 0 0\n");
        assert_eq!(mounts[0], ("/boot".into(), "vfat".into()));
        assert_eq!(mounts[2].0, "/sys x");
        assert_eq!(
            host_port("https://mirror.example.org/archlinux/core/os/x86_64").unwrap(),
            "mirror.example.org:443"
        );
        assert_eq!(
            host_port("http://m.example:8080/x").unwrap(),
            "m.example:8080"
        );
        assert_eq!(host_port("http://m.example/x").unwrap(), "m.example:80");
        assert!(host_port("ftp://x/y").is_none() && host_port("not a url").is_none());
        assert_eq!(
            kernel_bases(&[
                "linux\n".into(),
                "linux-lts\n".into(),
                "linux\n".into(),
                "\n".into()
            ]),
            ["linux", "linux-lts"]
        );
    }
}

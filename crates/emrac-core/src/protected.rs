//! Packages `emrac bisect` never changes on the live host (u24, the hard boundary).
//!
//! Putting an older kernel package under a running kernel removes the modules
//! the running one needs; replacing glibc or systemd leaves running programs
//! with code the files no longer match. So bisect may downgrade ordinary
//! packages, but not these: a state that needs one changed cannot be visited on
//! this machine. There is no flag that lifts it; the way past it is a disposable
//! environment, which is the `isolated` mode that is planned, not built.
//!
//! The list is a versioned policy, not a guess: `POLICY_VERSION` changes with it,
//! and `bisect.protected_extra` in the config only ever adds to it.

/// Changes whenever the groups below change.
pub const POLICY_VERSION: u32 = 1;

pub struct Group {
    pub name: &'static str,
    pub why: &'static str,
    pub packages: &'static [&'static str],
}

pub const GROUPS: &[Group] = &[
    Group {
        name: "kernel",
        why: "the running kernel keeps running, but its modules directory would change under it",
        packages: &[
            "linux",
            "linux-lts",
            "linux-zen",
            "linux-hardened",
            "linux-rt",
            "linux-rt-lts",
            "linux-firmware",
            "linux-firmware-whence",
        ],
    },
    Group {
        name: "core-runtime",
        why:
            "running programs have the old library loaded; the files on disk would no longer match",
        packages: &[
            "glibc",
            "lib32-glibc",
            "gcc-libs",
            "lib32-gcc-libs",
            "libgcc",
            "libstdc++",
        ],
    },
    Group {
        name: "init-system",
        why: "PID 1 and the service manager are running from the files being replaced",
        packages: &["systemd", "systemd-libs", "systemd-sysvcompat"],
    },
    Group {
        name: "package-manager",
        why: "the tools doing the work, and the records of what is installed",
        packages: &[
            "pacman",
            "pacman-mirrorlist",
            "archlinux-keyring",
            "filesystem",
            "emrac",
        ],
    },
    Group {
        name: "boot-storage",
        why: "a wrong version here can leave the machine unbootable or unable to mount its disks",
        packages: &[
            "mkinitcpio",
            "mkinitcpio-busybox",
            "dracut",
            "grub",
            "limine",
            "refind",
            "efibootmgr",
            "syslinux",
            "lvm2",
            "device-mapper",
            "cryptsetup",
            "mdadm",
            "btrfs-progs",
            "e2fsprogs",
            "xfsprogs",
            "dosfstools",
            "f2fs-tools",
        ],
    },
    Group {
        name: "essential-utilities",
        why: "the shell and base tools every other command, script and hook runs through",
        packages: &[
            "bash",
            "coreutils",
            "util-linux",
            "util-linux-libs",
            "shadow",
            "pam",
            "sudo",
            "readline",
            "ncurses",
        ],
    },
];

/// Why a package is protected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    pub package: String,
    pub group: String,
    pub why: String,
}

/// The protection `name` is under, if any. `extra` is the user's additions.
pub fn protection(name: &str, extra: &[String]) -> Option<Protection> {
    for g in GROUPS {
        if g.packages.contains(&name) {
            return Some(Protection {
                package: name.to_string(),
                group: g.name.to_string(),
                why: g.why.to_string(),
            });
        }
    }
    // a kernel package that is not in the list: `linux-<flavour>`, `linux-<flavour>-nvidia`
    // modules are built against one kernel, so they follow it
    if let Some(rest) = name.strip_prefix("linux-") {
        let flavour = rest.split('-').next().unwrap_or(rest);
        if matches!(
            flavour,
            "lts" | "zen" | "hardened" | "rt" | "cachyos" | "xanmod" | "mainline"
        ) && !rest.ends_with("-headers")
            && !rest.contains("docs")
        {
            return Some(Protection {
                package: name.to_string(),
                group: "kernel".into(),
                why: GROUPS[0].why.into(),
            });
        }
    }
    extra.iter().any(|e| e == name).then(|| Protection {
        package: name.to_string(),
        group: "yours".into(),
        why: "you listed it in bisect.protected_extra".into(),
    })
}

/// The protected ones among `names`.
pub fn blocked(names: &[String], extra: &[String]) -> Vec<Protection> {
    let mut out: Vec<Protection> = names.iter().filter_map(|n| protection(n, extra)).collect();
    out.sort_by(|a, b| a.package.cmp(&b.package));
    out.dedup_by(|a, b| a.package == b.package);
    out
}

/// `a, b` or `a, b and 3 more`, with each one's group.
pub fn describe(blocked: &[Protection]) -> String {
    let shown: Vec<String> = blocked
        .iter()
        .take(5)
        .map(|p| format!("{} ({})", p.package, p.group))
        .collect();
    let more = blocked.len().saturating_sub(5);
    if more > 0 {
        format!("{} and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_minimum_set_is_protected_and_ordinary_packages_are_not() {
        for p in [
            "linux",
            "linux-lts",
            "glibc",
            "systemd",
            "pacman",
            "bash",
            "coreutils",
            "mkinitcpio",
            "grub",
        ] {
            assert!(protection(p, &[]).is_some(), "{p}");
        }
        for p in [
            "firefox",
            "mpv",
            "git",
            "vim",
            "ripgrep",
            "linux-headers",
            "linux-docs",
        ] {
            assert!(protection(p, &[]).is_none(), "{p}");
        }
    }

    #[test]
    fn a_kernel_flavour_and_its_neighbours_follow_the_kernel() {
        assert_eq!(protection("linux-cachyos", &[]).unwrap().group, "kernel");
        assert_eq!(protection("linux-xanmod", &[]).unwrap().group, "kernel");
        assert!(protection("linux-lts-headers", &[]).is_none());
    }

    #[test]
    fn the_users_additions_add_and_never_remove() {
        let extra = names(&["fixc"]);
        assert_eq!(protection("fixc", &extra).unwrap().group, "yours");
        assert!(protection("fixc", &[]).is_none());
        assert!(protection("glibc", &extra).is_some());
    }

    #[test]
    fn the_blocked_list_is_sorted_unique_and_described() {
        let b = blocked(&names(&["glibc", "firefox", "linux", "glibc"]), &[]);
        assert_eq!(
            b.iter().map(|p| p.package.as_str()).collect::<Vec<_>>(),
            ["glibc", "linux"]
        );
        assert_eq!(describe(&b), "glibc (core-runtime), linux (kernel)");
    }
}

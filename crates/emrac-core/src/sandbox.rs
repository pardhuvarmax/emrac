//! Running the build steps of `makepkg` inside bubblewrap. The argument list
//! is built by a pure function so it can be tested without running anything;
//! the probe actually runs `bwrap`, so a disabled user-namespace setup is
//! caught too. See `docs/plan/u19-sandbox.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// What a sandboxed run may see and touch.
#[derive(Debug, Clone)]
pub struct SandboxSpec {
    /// The build directory: the only place (besides `extra_rw`) that is
    /// writable and visible from the host.
    pub dir: PathBuf,
    /// Further writable directories (`SRCDEST`, `PKGDEST`, … from makepkg.conf).
    pub extra_rw: Vec<PathBuf>,
    /// Replaced by an empty tmpfs, so the build sees an empty home.
    pub home: PathBuf,
    /// `false` (the default) gives the build no network at all.
    pub network: bool,
    /// Hide `/run/user` (agent sockets, dbus). Only if it exists.
    pub hide_run_user: bool,
    /// The environment the build gets; nothing else is passed.
    pub env: Vec<(String, String)>,
}

/// Where tools are looked up inside the sandbox.
const SANDBOX_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/bin";

/// Variables passed through from the caller's environment. Everything else —
/// tokens, `SSH_AUTH_SOCK`, `AWS_*` — is dropped.
const ENV_WHITELIST: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LANGUAGE",
    "TERM",
    "TZ",
    "MAKEFLAGS",
    "SOURCE_DATE_EPOCH",
    "PKGDEST",
    "SRCDEST",
    "SRCPKGDEST",
    "BUILDDIR",
    "LOGDEST",
    "PACKAGER",
];

/// The environment for a sandboxed build, from the caller's.
pub fn clean_env(host: &[(String, String)]) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), SANDBOX_PATH.into()),
        ("SHELL".into(), "/bin/bash".into()),
    ];
    env.extend(
        host.iter()
            .filter(|(k, _)| ENV_WHITELIST.contains(&k.as_str()) || k.starts_with("LC_"))
            .cloned(),
    );
    env
}

/// The `bwrap` arguments (everything before the command to run).
pub fn bwrap_args(spec: &SandboxSpec) -> Vec<String> {
    let s = |p: &Path| p.display().to_string();
    let mut a: Vec<String> = [
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/home",
    ]
    .map(String::from)
    .to_vec();
    if spec.network {
        a.push("--share-net".into());
    }
    if !spec.home.as_os_str().is_empty() && spec.home != Path::new("/") {
        a.extend(["--tmpfs".into(), s(&spec.home)]);
    }
    if spec.hide_run_user {
        a.extend(["--tmpfs".into(), "/run/user".into()]);
    }
    // Writable binds come after the tmpfs mounts so they show through them.
    for dir in std::iter::once(&spec.dir).chain(&spec.extra_rw) {
        a.extend(["--bind".into(), s(dir), s(dir)]);
    }
    a.extend(["--chdir".into(), s(&spec.dir), "--clearenv".into()]);
    for (k, v) in &spec.env {
        a.extend(["--setenv".into(), k.clone(), v.clone()]);
    }
    a
}

/// Is a sandbox usable here? `Err` carries a message that says what to do.
///
/// The probe uses the same mounts as a real run (notably a fresh `/proc`):
/// rootless containers often allow namespaces but forbid mounting `/proc`,
/// and a probe that skipped it would report "usable" and then fail mid-build.
/// A host `/proc` is deliberately *not* used as a fallback — with a shared
/// `/proc` the build could read emrac's own environment.
pub fn probe() -> Result<(), String> {
    let output = Command::new("bwrap")
        .args([
            "--unshare-all",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
            "true",
        ])
        .stdin(Stdio::null())
        .output();
    match output {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err("bubblewrap isn't installed — `sudo pacman -S bubblewrap`".to_string())
        }
        Err(err) => Err(format!("couldn't run bwrap: {err}")),
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.lines().next().unwrap_or("it failed").trim();
            Err(format!(
                "bwrap can't create a sandbox here ({why}) — unprivileged user namespaces may be disabled, or this environment (a container?) forbids mounting /proc"
            ))
        }
    }
}

/// Runs `command` in the sandbox with the terminal attached.
pub fn run(spec: &SandboxSpec, command: &[String]) -> std::io::Result<ExitStatus> {
    Command::new("bwrap")
        .args(bwrap_args(spec))
        .args(command)
        .status()
}

/// Directories the user's makepkg config points outside the build directory
/// (`SRCDEST`, `PKGDEST`, `BUILDDIR`, `LOGDEST`), created if missing so they
/// can be bound. Reads `/etc/makepkg.conf`, the XDG config and `~/.makepkg.conf`
/// the way makepkg does.
pub fn makepkg_dirs() -> Vec<PathBuf> {
    const SCRIPT: &str = r#"
        [ -f /etc/makepkg.conf ] && . /etc/makepkg.conf
        [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf" ] && . "${XDG_CONFIG_HOME:-$HOME/.config}/pacman/makepkg.conf"
        [ -f "$HOME/.makepkg.conf" ] && . "$HOME/.makepkg.conf"
        printf '%s\n' "${SRCDEST:-}" "${PKGDEST:-}" "${BUILDDIR:-}" "${LOGDEST:-}" "${SRCPKGDEST:-}"
    "#;
    let Ok(out) = Command::new("bash")
        .arg("-c")
        .arg(SCRIPT)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs.retain(|d| std::fs::create_dir_all(d).is_ok());
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SandboxSpec {
        SandboxSpec {
            dir: "/home/u/.cache/emrac/build/foo".into(),
            extra_rw: vec!["/data/src".into()],
            home: "/home/u".into(),
            network: false,
            hide_run_user: true,
            env: vec![
                ("PATH".into(), SANDBOX_PATH.into()),
                ("HOME".into(), "/home/u".into()),
            ],
        }
    }

    fn pos(args: &[String], needle: &[&str]) -> Option<usize> {
        args.windows(needle.len())
            .position(|w| w.iter().zip(needle).all(|(a, b)| a == b))
    }

    #[test]
    fn everything_is_unshared_and_the_network_is_off_by_default() {
        let a = bwrap_args(&spec());
        assert_eq!(a[0], "--unshare-all");
        assert!(a.contains(&"--die-with-parent".to_string()));
        assert!(a.contains(&"--new-session".to_string()));
        assert!(!a.contains(&"--share-net".to_string()));
        assert!(
            pos(&a, &["--ro-bind", "/", "/"]).is_some(),
            "the root is read-only"
        );
    }

    #[test]
    fn the_network_is_only_shared_when_asked() {
        let mut s = spec();
        s.network = true;
        let a = bwrap_args(&s);
        let unshare = pos(&a, &["--unshare-all"]).unwrap();
        let share = pos(&a, &["--share-net"]).unwrap();
        assert!(share > unshare, "--share-net must come after --unshare-all");
    }

    #[test]
    fn home_and_user_runtime_dirs_are_hidden() {
        let a = bwrap_args(&spec());
        assert!(pos(&a, &["--tmpfs", "/home"]).is_some());
        assert!(
            pos(&a, &["--tmpfs", "/home/u"]).is_some(),
            "$HOME is an empty tmpfs"
        );
        assert!(pos(&a, &["--tmpfs", "/run/user"]).is_some());
        assert!(pos(&a, &["--tmpfs", "/tmp"]).is_some());

        let mut s = spec();
        s.hide_run_user = false;
        assert!(pos(&bwrap_args(&s), &["--tmpfs", "/run/user"]).is_none());
        s.home = "/".into();
        assert!(
            pos(&bwrap_args(&s), &["--tmpfs", "/"]).is_none(),
            "never tmpfs the root"
        );
    }

    #[test]
    fn only_the_build_dir_and_configured_dirs_are_writable_and_after_the_tmpfs() {
        let a = bwrap_args(&spec());
        let binds: Vec<&str> = a
            .windows(3)
            .filter(|w| w[0] == "--bind")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(binds, ["/home/u/.cache/emrac/build/foo", "/data/src"]);
        let home_tmpfs = pos(&a, &["--tmpfs", "/home/u"]).unwrap();
        let build_bind = pos(&a, &["--bind", "/home/u/.cache/emrac/build/foo"]).unwrap();
        assert!(
            build_bind > home_tmpfs,
            "the bind must show through the tmpfs"
        );
        assert!(pos(&a, &["--chdir", "/home/u/.cache/emrac/build/foo"]).is_some());
    }

    #[test]
    fn the_environment_is_cleared_then_rebuilt() {
        let a = bwrap_args(&spec());
        let clear = pos(&a, &["--clearenv"]).unwrap();
        let first_set = pos(&a, &["--setenv"]).unwrap();
        assert!(first_set > clear);
        assert!(pos(&a, &["--setenv", "HOME", "/home/u"]).is_some());
    }

    #[test]
    fn secrets_do_not_make_it_into_the_build_environment() {
        let host: Vec<(String, String)> = [
            ("HOME", "/home/u"),
            ("USER", "u"),
            ("PATH", "/home/u/.cargo/bin:/usr/bin"),
            ("LC_ALL", "C"),
            ("LANG", "en_US.UTF-8"),
            ("MAKEFLAGS", "-j8"),
            ("GITHUB_TOKEN", "ghp_secret"),
            ("SSH_AUTH_SOCK", "/run/user/1000/ssh"),
            ("AWS_SECRET_ACCESS_KEY", "x"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let env = clean_env(&host);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for kept in [
            "HOME",
            "USER",
            "LC_ALL",
            "LANG",
            "MAKEFLAGS",
            "PATH",
            "SHELL",
        ] {
            assert!(keys.contains(&kept), "{kept}");
        }
        for dropped in [
            "GITHUB_TOKEN",
            "SSH_AUTH_SOCK",
            "AWS_SECRET_ACCESS_KEY",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            assert!(!keys.contains(&dropped), "{dropped}");
        }
        let path = env.iter().find(|(k, _)| k == "PATH").unwrap();
        assert_eq!(
            path.1, SANDBOX_PATH,
            "PATH is the system one, not the caller's"
        );
        assert_eq!(keys.iter().filter(|k| **k == "PATH").count(), 1);
    }
}

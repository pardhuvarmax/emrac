//! Running the build steps of `makepkg` inside bubblewrap. The argument list
//! is built by a pure function so it can be tested without running anything;
//! the probe actually runs `bwrap`, so a disabled user-namespace setup is
//! caught too. See `docs/plan/u19-sandbox.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::config::BuildConfig;

/// Resource limits for a sandboxed build; `0` turns a limit off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub timeout_minutes: u32,
    pub memory_percent: u32,
    pub max_tasks: u32,
    pub tmpfs_gib: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self::from_config(&BuildConfig::default())
    }
}

impl Limits {
    pub fn from_config(c: &BuildConfig) -> Self {
        Self {
            timeout_minutes: c.timeout_minutes,
            memory_percent: c.memory_percent,
            max_tasks: c.max_tasks,
            tmpfs_gib: c.tmpfs_gib,
        }
    }

    pub fn timeout(&self) -> Option<Duration> {
        (self.timeout_minutes > 0)
            .then(|| Duration::from_secs(u64::from(self.timeout_minutes) * 60))
    }

    pub fn tmpfs_bytes(&self) -> Option<u64> {
        (self.tmpfs_gib > 0).then(|| u64::from(self.tmpfs_gib) << 30)
    }
}

/// `systemd-run` arguments that put the command in a named cgroup scope with
/// the memory and process limits. Empty when neither limit is set.
pub fn scope_args(limits: &Limits, mem_total: u64, unit: &str) -> Vec<String> {
    let mut props: Vec<String> = Vec::new();
    if limits.memory_percent > 0 {
        let max = mem_total / 100 * u64::from(limits.memory_percent);
        props.push(format!("MemoryMax={max}"));
        props.push("MemorySwapMax=0".into());
        // By default systemd answers the first OOM kill by terminating the
        // whole scope, leaving nothing to report. Only the offender should
        // die; the build then fails normally and the counters say why.
        props.push("OOMPolicy=continue".into());
    }
    if limits.max_tasks > 0 {
        props.push(format!("TasksMax={}", limits.max_tasks));
    }
    if props.is_empty() {
        return Vec::new();
    }
    let mut a: Vec<String> = ["--user", "--scope", "--quiet"].map(String::from).to_vec();
    a.push(format!("--unit={unit}"));
    for p in props {
        a.extend(["-p".into(), p]);
    }
    a.push("--".into());
    a
}

/// Total RAM in bytes, from `/proc/meminfo`.
pub fn mem_total_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Can the user's systemd manager give us a limited scope? (No user session,
/// cgroup v1, or a container say no.)
pub fn scope_available(limits: &Limits) -> bool {
    let Some(total) = mem_total_bytes() else {
        return false;
    };
    let mut args = scope_args(
        limits,
        total,
        &format!("emrac-probe-{}", std::process::id()),
    );
    if args.is_empty() {
        return false;
    }
    args.push("true".into());
    Command::new("systemd-run")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// What the kernel counted in the run's cgroup scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CgroupEvents {
    /// Processes the OOM killer shot because the memory limit was hit.
    pub oom_kills: u64,
    /// Times a fork/clone was refused because the task limit was hit.
    pub fork_denials: u64,
}

/// How a sandboxed run ended.
#[derive(Debug)]
pub enum Outcome {
    /// Finished (or was killed from inside); the events say if a limit did it.
    Exited(ExitStatus, CgroupEvents),
    TimedOut,
}

/// `oom_kill` from a cgroup's `memory.events`.
pub fn parse_oom_kills(memory_events: &str) -> u64 {
    counter(memory_events, "oom_kill")
}

/// `max` from a cgroup's `pids.events`.
pub fn parse_fork_denials(pids_events: &str) -> u64 {
    counter(pids_events, "max")
}

fn counter(text: &str, key: &str) -> u64 {
    text.lines()
        .filter_map(|l| l.split_once(' '))
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
}

/// The counters the wrapper below writes: `memory.events`, a `---` line, then
/// `pids.events`.
pub fn parse_wrapper_events(text: &str) -> CgroupEvents {
    let (mem, pids) = text.split_once("---").unwrap_or((text, ""));
    CgroupEvents {
        oom_kills: parse_oom_kills(mem),
        fork_denials: parse_fork_denials(pids),
    }
}

/// The script run inside the scope. The scope's cgroup vanishes the moment its
/// last process exits — milliseconds after an OOM kill — so polling it from
/// outside is a race that loses (measured: it caught the kill in 4 of 12 runs).
/// This shell stays in the scope until it has read the counters itself, and
/// hands them over in a file.
const SCOPE_WRAPPER: &str = r#"events="$1"; shift
"$@"; status=$?
cg=/sys/fs/cgroup$(sed -n 's/^0:://p' /proc/self/cgroup)
{ cat "$cg/memory.events"; echo ---; cat "$cg/pids.events"; } > "$events" 2>/dev/null
exit $status"#;

/// How much of the end of a run's output is kept.
const TAIL_BYTES: usize = 32 * 1024;

/// Runs `cmd`, killing it if it outlives `timeout`, and returns the end of its
/// output (which is still shown as it arrives). Output goes through pipes, so the program sees no terminal:
/// no colours or progress bars. Its stdin stays attached.
pub fn run_command_tail(
    mut cmd: Command,
    timeout: Option<Duration>,
) -> std::io::Result<(Outcome, String)> {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let mut readers = Vec::new();
    let sources: [(Option<Box<dyn Read + Send>>, bool); 2] = [
        (
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            false,
        ),
        (
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            true,
        ),
    ];
    for (source, to_stderr) in sources {
        let Some(mut source) = source else {
            continue;
        };
        let tail = Arc::clone(&tail);
        readers.push(std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = source.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let chunk = &buf[..n];
                let _ = if to_stderr {
                    let mut e = std::io::stderr().lock();
                    e.write_all(chunk).and_then(|_| e.flush())
                } else {
                    let mut o = std::io::stdout().lock();
                    o.write_all(chunk).and_then(|_| o.flush())
                };
                let mut tail = tail.lock().unwrap();
                tail.extend_from_slice(chunk);
                if tail.len() > TAIL_BYTES {
                    let excess = tail.len() - TAIL_BYTES;
                    tail.drain(..excess);
                }
            }
        }));
    }
    let outcome = wait_for(child, timeout)?;
    // After a timeout a straggler could still hold a pipe open; don't wait for it.
    if matches!(outcome, Outcome::Exited(..)) {
        for reader in readers {
            let _ = reader.join();
        }
    }
    let text = String::from_utf8_lossy(&tail.lock().unwrap()).into_owned();
    Ok((outcome, text))
}

fn wait_for(mut child: std::process::Child, timeout: Option<Duration>) -> std::io::Result<Outcome> {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Outcome::Exited(status, CgroupEvents::default()));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            // bwrap dies with its parent, and with the namespace's init
            // everything inside it.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Outcome::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

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
    /// Size cap for the `/tmp` and `$HOME` tmpfs mounts.
    pub tmpfs_bytes: Option<u64>,
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
        // Inside the sandbox's user namespace only the caller's own id is
        // mapped, so fakeroot's real chown to root fails with EINVAL (and
        // `cp -a` or `chown` in a package() aborts). The ownership fakeroot
        // records for the package is all that matters.
        ("FAKEROOTDONTTRYCHOWN".into(), "1".into()),
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
    ]
    .map(String::from)
    .to_vec();
    if let Some(bytes) = spec.tmpfs_bytes {
        a.extend(["--size".into(), bytes.to_string()]);
    }
    a.extend([
        "--tmpfs".into(),
        "/tmp".into(),
        "--tmpfs".into(),
        "/home".into(),
    ]);
    if spec.network {
        a.push("--share-net".into());
    }
    if !spec.home.as_os_str().is_empty() && spec.home != Path::new("/") {
        if let Some(bytes) = spec.tmpfs_bytes {
            a.extend(["--size".into(), bytes.to_string()]);
        }
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

/// Runs `command` in the sandbox with the terminal attached. With
/// `use_scope` the memory and process limits are enforced by a cgroup scope
/// (see `scope_available`) and the kernel's own counters say afterwards whether
/// a limit was what stopped the build; the timeout needs nothing.
pub fn run(
    spec: &SandboxSpec,
    command: &[String],
    limits: &Limits,
    use_scope: bool,
) -> std::io::Result<(Outcome, String)> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = format!("{}-{nanos}", std::process::id());
    let scope = if use_scope {
        scope_args(
            limits,
            mem_total_bytes().unwrap_or(0),
            &format!("emrac-build-{unique}"),
        )
    } else {
        Vec::new()
    };
    let events_file = std::env::temp_dir().join(format!("emrac-events-{unique}"));
    let cmd = if scope.is_empty() {
        let mut c = Command::new("bwrap");
        c.args(bwrap_args(spec)).args(command);
        c
    } else {
        let mut c = Command::new("systemd-run");
        c.args(&scope)
            .args(["sh", "-c", SCOPE_WRAPPER, "emrac-scope"])
            .arg(&events_file)
            .arg("bwrap")
            .args(bwrap_args(spec))
            .args(command);
        c
    };
    let outcome = run_command_tail(cmd, limits.timeout());
    let events = std::fs::read_to_string(&events_file)
        .map(|t| parse_wrapper_events(&t))
        .unwrap_or_default();
    let _ = std::fs::remove_file(&events_file);
    match outcome? {
        (Outcome::Exited(status, _), tail) => Ok((Outcome::Exited(status, events), tail)),
        (Outcome::TimedOut, tail) => Ok((Outcome::TimedOut, tail)),
    }
}

/// Does the end of a build's output say it tried to reach the network and
/// could not (a name that would not resolve, a refused connection, ...)?
pub fn looks_like_network_failure(output: &str) -> bool {
    const SIGNS: &[&str] = &[
        "could not resolve host",
        "couldn't resolve host",
        "temporary failure in name resolution",
        "temporary failure resolving",
        "name or service not known",
        "failed to lookup address",
        "network is unreachable",
        "no route to host",
        "connection refused",
        "connection timed out",
        "failed to connect to",
        "unable to access 'http",
        "spurious network error",
        "dial tcp",
        "curl: (6)",
        "curl: (7)",
        "curl: (28)",
        "enotfound",
        "eai_again",
    ];
    let lower = output.to_lowercase();
    SIGNS.iter().any(|s| lower.contains(s))
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
            tmpfs_bytes: None,
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
    fn tmpfs_mounts_get_a_size_cap_when_one_is_set() {
        let mut s = spec();
        s.tmpfs_bytes = Some(8 << 30);
        let a = bwrap_args(&s);
        let size = (8u64 << 30).to_string();
        assert!(
            pos(&a, &["--size", &size, "--tmpfs", "/tmp"]).is_some(),
            "{a:?}"
        );
        assert!(
            pos(&a, &["--size", &size, "--tmpfs", "/home/u"]).is_some(),
            "{a:?}"
        );
        // /home itself only holds the path down to the build dir.
        assert!(pos(&a, &["--size", &size, "--tmpfs", "/home"]).is_none());
        assert!(!bwrap_args(&spec()).contains(&"--size".to_string()));
    }

    #[test]
    fn limits_become_systemd_scope_properties() {
        let l = Limits {
            timeout_minutes: 60,
            memory_percent: 50,
            max_tasks: 1000,
            tmpfs_gib: 4,
        };
        let a = scope_args(&l, 16_000_000_000, "emrac-build-1");
        assert_eq!(&a[..3], ["--user", "--scope", "--quiet"]);
        assert!(a.contains(&"--unit=emrac-build-1".to_string()), "{a:?}");
        assert!(a.contains(&"MemoryMax=8000000000".to_string()), "{a:?}");
        assert!(a.contains(&"MemorySwapMax=0".to_string()));
        assert!(a.contains(&"OOMPolicy=continue".to_string()), "{a:?}");
        assert!(a.contains(&"TasksMax=1000".to_string()));
        assert_eq!(a.last().unwrap(), "--", "the command follows");

        // A zero turns that limit off; with neither, there is no wrapper.
        let a = scope_args(
            &Limits {
                memory_percent: 0,
                ..l
            },
            16_000_000_000,
            "u",
        );
        assert!(!a.iter().any(|p| p.starts_with("Memory")), "{a:?}");
        assert!(a.contains(&"TasksMax=1000".to_string()));
        let none = Limits {
            memory_percent: 0,
            max_tasks: 0,
            ..l
        };
        assert!(scope_args(&none, 16_000_000_000, "u").is_empty());
    }

    #[test]
    fn limits_convert_from_the_config_and_zero_means_off() {
        let d = Limits::default();
        assert_eq!(d.timeout(), Some(Duration::from_secs(120 * 60)));
        assert_eq!(d.tmpfs_bytes(), Some(8 << 30));
        let off = Limits {
            timeout_minutes: 0,
            tmpfs_gib: 0,
            ..d
        };
        assert_eq!(off.timeout(), None);
        assert_eq!(off.tmpfs_bytes(), None);
    }

    #[test]
    fn the_end_of_the_output_is_returned_from_both_streams_and_capped() {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "echo out-line; echo err-line >&2; i=0; while [ $i -lt 5000 ]; do echo padding-padding-padding-padding; i=$((i+1)); done; echo the-end",
        ]);
        let (out, tail) = run_command_tail(cmd, None).unwrap();
        assert!(matches!(out, Outcome::Exited(ref s, _) if s.success()));
        assert!(tail.ends_with("the-end\n"), "{tail:?}");
        assert!(tail.len() <= TAIL_BYTES, "{}", tail.len());
        assert!(!tail.contains("out-line"), "the start was cut off");
    }

    #[test]
    fn network_failures_are_recognised_and_other_failures_are_not() {
        for yes in [
            "warning: spurious network error (2 tries remaining): [6] Could not resolve hostname (Could not resolve host: index.crates.io)",
            "fatal: unable to access 'https://x/': Could not resolve host: x",
            "curl: (6) Could not resolve host: example.org",
            "Temporary failure in name resolution",
            "dial tcp: lookup proxy.golang.org: no such host\nGet https://x: dial tcp 1.2.3.4:443: connect: Network is unreachable",
        ] {
            assert!(looks_like_network_failure(yes), "{yes}");
        }
        for no in [
            "==> ERROR: One or more PGP signatures could not be verified!",
            "error: use of undeclared identifier 'foo'",
            "make: *** [Makefile:14: build] Error 2",
            "",
        ] {
            assert!(!looks_like_network_failure(no), "{no}");
        }
    }

    #[test]
    fn the_timeout_kills_a_run_on_time_and_a_normal_exit_is_reported() {
        let mut slow = Command::new("sleep");
        slow.arg("30");
        let start = Instant::now();
        let (out, _) = run_command_tail(slow, Some(Duration::from_millis(300))).unwrap();
        assert!(matches!(out, Outcome::TimedOut), "{out:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );

        let (ok, _) = run_command_tail(Command::new("true"), Some(Duration::from_secs(5))).unwrap();
        assert!(
            matches!(&ok, Outcome::Exited(s, _) if s.success()),
            "{ok:?}"
        );
        let (fail, _) = run_command_tail(Command::new("false"), None).unwrap();
        assert!(
            matches!(&fail, Outcome::Exited(s, _) if s.code() == Some(1)),
            "{fail:?}"
        );
    }

    #[test]
    fn cgroup_counters_are_parsed_and_the_two_max_lines_are_not_confused() {
        let text = "low 0\nhigh 0\nmax 44\noom 1\noom_kill 1\n---\nmax 5\n";
        assert_eq!(
            parse_wrapper_events(text),
            CgroupEvents {
                oom_kills: 1,
                fork_denials: 5
            }
        );
        assert_eq!(
            parse_wrapper_events("max 44\noom_kill 0\n---\n"),
            CgroupEvents::default(),
            "memory.events' `max` is not a fork denial"
        );
        assert_eq!(parse_wrapper_events(""), CgroupEvents::default());
        assert_eq!(
            parse_oom_kills("oom 3\noom_kill 2\n"),
            2,
            "oom_kill, not oom"
        );
    }

    #[test]
    fn the_scope_wrapper_hands_back_the_commands_exit_status() {
        let dir = std::env::temp_dir().join(format!("emrac-wrap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let events = dir.join("events");
        for (cmd, code) in [("true", 0), ("false", 1)] {
            let status = Command::new("sh")
                .args(["-c", SCOPE_WRAPPER, "emrac-scope"])
                .arg(&events)
                .arg(cmd)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(code), "{cmd}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(
            env.iter()
                .any(|(k, v)| k == "FAKEROOTDONTTRYCHOWN" && v == "1"),
            "fakeroot must not really chown inside the user namespace"
        );
        let path = env.iter().find(|(k, _)| k == "PATH").unwrap();
        assert_eq!(
            path.1, SANDBOX_PATH,
            "PATH is the system one, not the caller's"
        );
        assert_eq!(keys.iter().filter(|k| **k == "PATH").count(), 1);
    }
}

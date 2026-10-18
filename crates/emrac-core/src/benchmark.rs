//! `emrac benchmark` (u9): what an installed package costs to start.
//!
//! One executable at a time is run as `<exe> --version` with stdin closed,
//! output discarded and an empty directory as its working directory: the first
//! run, then the median of the further runs, and the largest resident set seen.
//! This is the cost of starting the program, not of a workload, and the report
//! says so. See `docs/plan/r3/u9-stats-benchmark.md`.

use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Newest records kept in the benchmarks file.
pub const KEEP_RECORDS: usize = 200;
/// A run longer than this is killed and reported as not finished.
pub const RUN_LIMIT: Duration = Duration::from_secs(10);
pub const DEFAULT_RUNS: u32 = 5;
pub const MAX_RUNS: u32 = 50;

/// One run of a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub micros: u64,
    /// Largest resident set, in KiB (`ru_maxrss`).
    pub max_rss_kib: u64,
    pub exit_ok: bool,
}

/// What `measure` found for one executable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Measurement {
    pub exe: String,
    pub size_bytes: u64,
    /// The first run, in microseconds; `None` when it did not finish.
    pub first_micros: Option<u64>,
    /// Median of the later runs; `None` when none finished.
    pub warm_micros: Option<u64>,
    pub runs: u32,
    pub max_rss_kib: u64,
    /// What a program that does nothing reports when started the same way. On
    /// Linux a child's peak starts at the launcher's own size, so a figure
    /// that is not above this says nothing about the program.
    #[serde(default)]
    pub floor_rss_kib: u64,
    /// At least one run exited non-zero (many tools reject `--version`).
    pub nonzero_exit: bool,
    /// How many runs did not finish within the limit.
    pub timeouts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchRecord {
    /// Unix seconds.
    pub time: u64,
    pub package: String,
    pub version: String,
    pub installed_size: u64,
    pub cores: u32,
    pub measurement: Measurement,
}

pub fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2
    })
}

/// Runs `exe --version` once in `cwd`; `None` when it did not finish in `limit`.
pub fn run_once(exe: &Path, cwd: &Path, limit: Duration) -> Result<Option<Run>> {
    let mut command = Command::new(exe);
    command
        .arg("--version")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own process group, so a timeout kills what it started too.
    command.process_group(0);
    let start = Instant::now();
    let mut attempts = 0;
    let child = loop {
        match command.spawn() {
            Ok(child) => break child,
            // Another thread forked while this file was still open for writing.
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(Error::Benchmark(format!("{}: {e}", exe.display()))),
        }
    };
    let pid = child.id() as libc::pid_t;
    // The child is reaped by wait4 below, not by `Child`.
    std::mem::forget(child);
    loop {
        let mut status: libc::c_int = 0;
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, &mut usage) };
        if r == pid {
            return Ok(Some(Run {
                micros: start.elapsed().as_micros() as u64,
                max_rss_kib: usage.ru_maxrss.max(0) as u64,
                exit_ok: libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
            }));
        }
        if r < 0 {
            return Err(Error::Benchmark(format!(
                "{}: lost the process",
                exe.display()
            )));
        }
        if start.elapsed() >= limit {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                let mut status: libc::c_int = 0;
                libc::waitpid(pid, &mut status, 0);
            }
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Peak size reported for a program that does nothing, started as the others are.
fn launcher_floor(cwd: &Path, limit: Duration) -> u64 {
    ["/usr/bin/true", "/bin/true"]
        .iter()
        .find_map(|p| run_once(Path::new(p), cwd, limit).ok().flatten())
        .map_or(0, |r| r.max_rss_kib)
}

impl Measurement {
    /// The memory figure that means something: `Some` only when the peak is
    /// clearly above the launcher's floor.
    pub fn real_rss_kib(&self) -> Option<u64> {
        (self.max_rss_kib > self.floor_rss_kib + 512).then_some(self.max_rss_kib)
    }
}

/// `runs` further runs after the first.
pub fn measure(exe: &Path, runs: u32, limit: Duration) -> Result<Measurement> {
    let runs = runs.clamp(1, MAX_RUNS);
    let size_bytes = fs::metadata(exe).map(|m| m.len()).unwrap_or(0);
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let cwd = std::env::temp_dir().join(format!(
        "emrac-bench-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&cwd).map_err(|e| Error::Benchmark(format!("{}: {e}", cwd.display())))?;
    let mut m = Measurement {
        exe: exe.display().to_string(),
        size_bytes,
        first_micros: None,
        warm_micros: None,
        runs,
        max_rss_kib: 0,
        floor_rss_kib: launcher_floor(&cwd, limit),
        nonzero_exit: false,
        timeouts: 0,
    };
    let mut warm: Vec<u64> = Vec::new();
    let mut outcome = Ok(());
    for i in 0..=runs {
        match run_once(exe, &cwd, limit) {
            Ok(Some(run)) => {
                m.max_rss_kib = m.max_rss_kib.max(run.max_rss_kib);
                m.nonzero_exit |= !run.exit_ok;
                if i == 0 {
                    m.first_micros = Some(run.micros);
                } else {
                    warm.push(run.micros);
                }
            }
            Ok(None) => {
                m.timeouts += 1;
                // A program that waits for input will do so every time.
                if i == 0 {
                    break;
                }
            }
            Err(e) => {
                outcome = Err(e);
                break;
            }
        }
    }
    let _ = fs::remove_dir_all(&cwd);
    outcome?;
    m.warm_micros = median(&mut warm);
    Ok(m)
}

fn store_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/benchmarks.jsonl"))
}

fn append_to(path: &Path, record: &BenchRecord) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut lines: Vec<String> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    lines.push(serde_json::to_string(record).unwrap_or_default());
    if lines.len() > KEEP_RECORDS {
        lines.drain(..lines.len() - KEEP_RECORDS);
    }
    let tmp = path.with_extension("jsonl.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all((lines.join("\n") + "\n").as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

fn read_from(path: &Path) -> Vec<BenchRecord> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn record(record: &BenchRecord) -> Result<()> {
    let path = store_path()?;
    append_to(&path, record).map_err(|e| Error::Benchmark(format!("{}: {e}", path.display())))
}

/// Earlier results for `package`, oldest first.
pub fn history(package: &str) -> Vec<BenchRecord> {
    store_path()
        .map(|p| read_from(&p))
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.package == package)
        .collect()
}

/// The files of a package that `benchmark` may run: regular executables
/// directly in `/usr/bin` or `/usr/sbin`, from the list pacman recorded.
pub fn runnable(files: &[String], root: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    files
        .iter()
        .filter(|f| {
            ["usr/bin/", "usr/sbin/"].iter().any(|dir| {
                f.strip_prefix(dir)
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
        })
        .map(|f| root.join(f))
        .filter(|p| {
            fs::symlink_metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("emrac-bench-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn the_median_of_odd_even_and_no_values() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5]), Some(5));
        assert_eq!(median(&mut [9, 1, 5]), Some(5));
        assert_eq!(median(&mut [4, 1, 3, 2]), Some(2));
    }

    #[test]
    fn a_quick_program_gets_a_first_and_a_warm_time() {
        let d = scratch("quick");
        let exe = script(&d, "quick", "exit 0");
        let m = measure(&exe, 3, Duration::from_secs(5)).unwrap();
        assert!(m.first_micros.is_some() && m.warm_micros.is_some());
        assert_eq!((m.runs, m.timeouts), (3, 0));
        assert!(!m.nonzero_exit);
        assert!(m.max_rss_kib > 0);
        assert!(m.size_bytes > 0);
        // a script that does nothing is not above the floor of one that does nothing
        assert_eq!(
            m.real_rss_kib().is_some(),
            m.max_rss_kib > m.floor_rss_kib + 512
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_program_that_rejects_the_flag_is_flagged_but_still_timed() {
        let d = scratch("reject");
        let exe = script(&d, "reject", "exit 3");
        let m = measure(&exe, 2, Duration::from_secs(5)).unwrap();
        assert!(m.nonzero_exit && m.first_micros.is_some());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_program_that_never_finishes_is_killed_and_no_time_is_invented() {
        let d = scratch("hang");
        let exe = script(&d, "hang", "sleep 30");
        let start = Instant::now();
        let m = measure(&exe, 3, Duration::from_millis(200)).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5), "it was killed");
        assert_eq!((m.first_micros, m.warm_micros), (None, None));
        assert_eq!(
            m.timeouts, 1,
            "it does not try again after the first run hangs"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        assert!(measure(Path::new("/no/such/exe"), 1, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn it_runs_in_an_empty_directory_with_closed_input() {
        let d = scratch("env");
        // Fails if there is anything in the directory or if stdin has data.
        let exe = script(
            &d,
            "env",
            "[ -z \"$(ls -A)\" ] && ! read -r x && exit 0; exit 9",
        );
        let m = measure(&exe, 1, Duration::from_secs(5)).unwrap();
        assert!(!m.nonzero_exit);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn only_executables_directly_in_the_bin_directories_are_runnable() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("runnable");
        fs::create_dir_all(d.join("usr/bin/sub")).unwrap();
        fs::create_dir_all(d.join("usr/share")).unwrap();
        let exe = script(&d.join("usr/bin"), "tool", "exit 0");
        let plain = d.join("usr/bin/data");
        fs::write(&plain, "x").unwrap();
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink("tool", d.join("usr/bin/link")).unwrap();
        script(&d.join("usr/share"), "elsewhere", "exit 0");
        let files: Vec<String> = [
            "usr/",
            "usr/bin/",
            "usr/bin/tool",
            "usr/bin/data",
            "usr/bin/link",
            "usr/bin/sub/",
            "usr/bin/sub/deep",
            "usr/share/elsewhere",
            "usr/bin/missing",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(runnable(&files, &d), vec![exe]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_store_appends_caps_and_reads_back_by_package() {
        let d = scratch("store");
        let path = d.join("benchmarks.jsonl");
        let rec = |pkg: &str, time| BenchRecord {
            time,
            package: pkg.into(),
            version: "1".into(),
            installed_size: 10,
            cores: 4,
            measurement: Measurement {
                exe: "/usr/bin/x".into(),
                size_bytes: 1,
                first_micros: Some(5),
                warm_micros: Some(3),
                runs: 5,
                max_rss_kib: 100,
                floor_rss_kib: 0,
                nonzero_exit: false,
                timeouts: 0,
            },
        };
        for t in 0..(KEEP_RECORDS as u64 + 5) {
            append_to(&path, &rec(if t % 2 == 0 { "a" } else { "b" }, t)).unwrap();
        }
        let all = read_from(&path);
        assert_eq!(all.len(), KEEP_RECORDS);
        assert_eq!(all[0].time, 5);
        assert!(all
            .iter()
            .filter(|r| r.package == "a")
            .all(|r| r.time % 2 == 0));
        let _ = fs::remove_dir_all(&d);
    }
}

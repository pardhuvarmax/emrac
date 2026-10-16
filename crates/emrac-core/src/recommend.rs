//! `emrac recommend` (u2): is it worth building this package from source on
//! this machine, or should the binary be installed?
//!
//! The engine is pure: `evaluate(&Facts)` turns stated inputs into a verdict,
//! a score and the reasons, by the rules in `docs/design/recommend.md`. No
//! weights are hidden and nothing is learned: every point comes from a rule
//! and every number from a named input. `gather_machine` and the parsers
//! read the inputs; the build-times file records how long builds took here.
//! See `docs/plan/r3/u2-recommend.md`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ---------------------------------------------------------------- the rules

/// The score a source build starts from.
pub const START: i32 = 50;
/// 65 and over: source. 35 and under: binary.
pub const SOURCE_AT: i32 = 65;
pub const BINARY_AT: i32 = 35;
/// With the repository's own flags a local build gives the same code.
pub const SAME_FLAGS_CAP: i32 = 30;
pub const TUNED_GAIN: i32 = 25;
pub const OPT_GAIN: i32 = 5;
pub const TRUST_PENALTY: i32 = 5;
pub const FIT_PENALTY: i32 = 20;
/// Newest lines kept in the build-times file.
pub const KEEP_BUILDS: usize = 500;
/// Space a build wants: sources, the build tree and the package.
pub const DISK_NEEDED_MIB: u64 = 2048;

/// What the package is made of, as far as it can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// Architecture-independent: nothing is compiled.
    NoCompile,
    C,
    Cpp,
    Rust,
    Go,
    Java,
    Unknown,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::NoCompile => "nothing to compile",
            Class::C => "C",
            Class::Cpp => "C++",
            Class::Rust => "Rust",
            Class::Go => "Go",
            Class::Java => "Java",
            Class::Unknown => "unknown toolchain",
        }
    }

    /// Seconds of one core per MiB of installed size. Rough, and shown as
    /// such: `benchmark` and the build-times file replace it with data.
    fn secs_per_mib(self) -> f64 {
        match self {
            Class::NoCompile => 0.05,
            Class::C => 5.0,
            Class::Cpp => 60.0,
            Class::Rust => 25.0,
            Class::Go => 6.0,
            Class::Java => 8.0,
            Class::Unknown => 12.0,
        }
    }

    /// The least a build of the class takes, whatever the size: configure,
    /// dependency crates, linking. A small binary is not a quick build.
    fn floor_secs(self) -> f64 {
        match self {
            Class::NoCompile => 1.0,
            Class::C => 20.0,
            Class::Cpp => 60.0,
            Class::Rust => 90.0,
            Class::Go => 40.0,
            Class::Java => 60.0,
            Class::Unknown => 45.0,
        }
    }

    /// Seconds a typical package of the class takes when its size is not
    /// known (the AUR carries no sizes).
    fn typical_secs(self) -> f64 {
        match self {
            Class::NoCompile => 5.0,
            Class::C => 90.0,
            Class::Cpp => 600.0,
            Class::Rust => 300.0,
            Class::Go => 120.0,
            Class::Java => 150.0,
            Class::Unknown => 180.0,
        }
    }

    /// Memory a build of the class wants at its peak, in MiB.
    fn peak_mib(self) -> u64 {
        match self {
            Class::NoCompile | Class::C | Class::Go => 1024,
            Class::Java | Class::Unknown => 2048,
            Class::Cpp | Class::Rust => 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn word(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Binary,
    Source,
    /// Close: the binary is the default.
    Either,
    /// No binary exists.
    SourceOnly,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Binary => "binary",
            Verdict::Source => "source",
            Verdict::Either => "either",
            Verdict::SourceOnly => "source (the only option)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Source,
    Binary,
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Origin {
    /// `core`, `extra`, … or `aur`.
    pub repo: String,
}

impl Origin {
    pub fn is_aur(&self) -> bool {
        self.repo == "aur"
    }
}

/// How the makepkg configuration differs from what the repositories use.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Flags {
    pub cflags: String,
    /// The `-march=` value, if any.
    pub march: Option<String>,
    /// The `-O` level, if any.
    pub opt: Option<String>,
    /// A repository built for x86-64-v3 or v4 is enabled: its binaries are
    /// already tuned.
    pub tuned_repo: bool,
    /// `-j` from MAKEFLAGS.
    pub jobs: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Machine {
    pub cores: u32,
    pub mem_mib: u64,
    pub free_disk_mib: Option<u64>,
    /// 1 for x86-64, 2, 3 or 4 for the v2 to v4 levels the CPU supports; 0 when unknown.
    pub isa_level: u8,
}

/// Everything the rules look at.
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub name: String,
    pub origin: Origin,
    /// An official binary, or an AUR `-bin` sibling.
    pub binary: Option<String>,
    pub arch_any: bool,
    pub installed_mib: Option<f64>,
    pub download_mib: Option<f64>,
    pub deps_direct: usize,
    pub deps_total: Option<usize>,
    pub class: Class,
    /// Where the class came from, for the reasons.
    pub class_from: String,
    /// Seconds of earlier builds of this package on this machine.
    pub measured: Vec<u64>,
    /// Installs and upgrades recorded in the last 90 days.
    pub updates_90d: u32,
    pub flags: Flags,
    pub machine: Machine,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reason {
    pub rule: &'static str,
    pub side: Side,
    pub text: String,
    /// Points added to or taken from the score.
    pub points: i32,
    /// Where the input came from.
    pub input: String,
    /// The score is capped at this because of the reason.
    pub cap: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub name: String,
    pub verdict: Verdict,
    /// `None` when there is no choice to score (no binary, or nothing to compile).
    pub score: Option<i32>,
    pub confidence: Confidence,
    pub confidence_why: String,
    /// Build time on this machine, seconds: `(low, likely, high)`.
    pub build_secs: Option<(u64, u64, u64)>,
    /// How the build time was reached: `measured` or `estimated`.
    pub build_basis: &'static str,
    pub reasons: Vec<Reason>,
    pub notes: Vec<String>,
}

// ------------------------------------------------------------------ engine

fn median(v: &[u64]) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    Some(s[s.len() / 2])
}

/// Usable cores: parallel builds do not scale linearly.
fn effective_cores(f: &Facts) -> f64 {
    let cores = f
        .flags
        .jobs
        .map_or(f.machine.cores, |j| j.min(f.machine.cores))
        .max(1);
    1.0 + (f64::from(cores) - 1.0) * 0.5
}

/// `(low, likely, high)` seconds and how it was reached.
pub fn build_time(f: &Facts) -> (u64, u64, u64, &'static str) {
    if let Some(m) = median(&f.measured) {
        return (m, m, m, "measured");
    }
    let mib = f.installed_mib.or(f.download_mib.map(|d| d * 3.0));
    let likely = match mib {
        Some(m) => m * f.class.secs_per_mib() / effective_cores(f),
        None => f.class.typical_secs() / effective_cores(f).sqrt(),
    }
    .max(f.class.floor_secs());
    (
        (likely * 0.5) as u64,
        likely as u64,
        (likely * 2.0) as u64,
        "estimated",
    )
}

fn cost_points(secs: u64) -> i32 {
    match secs {
        0..=119 => 0,
        120..=599 => -10,
        600..=1799 => -20,
        1800..=3599 => -30,
        _ => -40,
    }
}

fn churn_points(updates: u32) -> i32 {
    match updates {
        0..=2 => 0,
        3..=5 => -8,
        _ => -15,
    }
}

/// The x86-64 level a `-march=` value asks for, `None` for a value that is
/// not a level (`native`, a CPU name).
fn march_level(march: &str) -> Option<u8> {
    match march {
        "x86-64" | "x86-64-v1" => Some(1),
        "x86-64-v2" => Some(2),
        "x86-64-v3" => Some(3),
        "x86-64-v4" => Some(4),
        _ => None,
    }
}

/// Does the configuration build something the repository's binary is not?
/// `Some(supported)`: it is tuned, and the CPU does (or does not) run it.
fn tuned(f: &Facts) -> Option<bool> {
    let march = f.flags.march.as_deref()?;
    match march_level(march) {
        Some(l) if l <= 1 => None,
        Some(l) => {
            // A tuned repository already ships this level.
            if f.flags.tuned_repo && l <= 3 {
                None
            } else {
                Some(f.machine.isa_level == 0 || f.machine.isa_level >= l)
            }
        }
        None => Some(true), // native, znver3, …
    }
}

fn mins(secs: u64) -> String {
    if secs < 90 {
        format!("{secs} s")
    } else if secs < 5400 {
        format!("{} min", (secs + 30) / 60)
    } else {
        format!("{:.1} h", secs as f64 / 3600.0)
    }
}

pub fn evaluate(f: &Facts) -> Report {
    let mut reasons: Vec<Reason> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let (lo, likely, hi, basis) = build_time(f);
    let time_text = if basis == "measured" {
        format!(
            "{} (measured on this machine{})",
            mins(likely),
            if f.measured.len() > 1 {
                format!(", {} builds", f.measured.len())
            } else {
                String::new()
            }
        )
    } else {
        format!(
            "about {}, between {} and {} (estimated: {})",
            mins(likely),
            mins(lo),
            mins(hi),
            f.class_from
        )
    };

    // Nothing to compile: the binary is the package.
    if f.arch_any || f.class == Class::NoCompile {
        reasons.push(Reason {
            rule: "nothing-to-compile",
            side: Side::Both,
            text: format!(
                "nothing is compiled ({}): a source build adds no speed, only the build's risk",
                f.class_from
            ),
            points: -START,
            input: "the package's metadata".into(),
            cap: None,
        });
        return Report {
            name: f.name.clone(),
            verdict: if f.binary.is_some() {
                Verdict::Binary
            } else {
                Verdict::SourceOnly
            },
            score: f.binary.is_some().then_some(0),
            confidence: Confidence::High,
            confidence_why: "the verdict does not depend on any estimate".into(),
            build_secs: None,
            build_basis: basis,
            reasons,
            notes,
        };
    }

    let mut score = START;
    let mut cap: Option<i32> = None;

    // Gain: does the local build differ from the repository's?
    match tuned(f) {
        Some(true) => {
            score += TUNED_GAIN;
            reasons.push(Reason {
                rule: "gain-tuned",
                side: Side::Source,
                text: format!(
                    "your makepkg flags target {}: a local build can use instructions the repository's generic build cannot (a possible gain, not a measured one; `emrac benchmark` will measure it)",
                    f.flags.march.as_deref().unwrap_or("this CPU")
                ),
                points: TUNED_GAIN,
                input: format!("CFLAGS `{}` (makepkg.conf)", f.flags.cflags),
                cap: None,
            });
        }
        Some(false) => {
            cap = Some(SAME_FLAGS_CAP);
            reasons.push(Reason {
                rule: "gain-unsupported",
                side: Side::Source,
                text: format!(
                    "your makepkg flags ask for {}, which this CPU (level {}) does not run: the build would not work, so no gain counts",
                    f.flags.march.as_deref().unwrap_or("a newer level"),
                    f.machine.isa_level
                ),
                points: 0,
                input: format!("CFLAGS `{}`, CPU flags (/proc/cpuinfo)", f.flags.cflags),
                cap,
            });
        }
        None => {
            cap = Some(SAME_FLAGS_CAP);
            reasons.push(Reason {
                rule: "gain-none",
                side: Side::Source,
                text: if f.flags.tuned_repo {
                    "an x86-64-v3 or v4 repository is enabled and your flags match it: a local build gives the same code as the binary".into()
                } else {
                    "your makepkg flags are the repository's defaults: a local build gives the same code as the binary".into()
                },
                points: 0,
                input: format!(
                    "CFLAGS `{}` (makepkg.conf)",
                    if f.flags.cflags.is_empty() {
                        "not set"
                    } else {
                        &f.flags.cflags
                    }
                ),
                cap,
            });
        }
    }
    if f.flags.opt.as_deref() == Some("3") {
        score += OPT_GAIN;
        reasons.push(Reason {
            rule: "gain-opt",
            side: Side::Source,
            text: "-O3 is set; the repository builds with -O2".into(),
            points: OPT_GAIN,
            input: format!("CFLAGS `{}`", f.flags.cflags),
            cap: None,
        });
    }

    // Cost.
    let cost = cost_points(likely);
    score += cost;
    reasons.push(Reason {
        rule: "cost",
        side: Side::Source,
        text: format!("a build here takes {time_text}"),
        points: cost,
        input: if basis == "measured" {
            "build-times.jsonl (emrac's own builds)".into()
        } else {
            format!("{}, {} cores", f.class_from, f.machine.cores)
        },
        cap: None,
    });

    // Churn.
    let churn = churn_points(f.updates_90d);
    if churn != 0 || f.updates_90d > 0 {
        score += churn;
        reasons.push(Reason {
            rule: "churn",
            side: Side::Source,
            text: if churn == 0 {
                format!(
                    "{} update(s) in 90 days: a rebuild each time is cheap enough",
                    f.updates_90d
                )
            } else {
                format!(
                    "{} updates in 90 days: each one would be another build",
                    f.updates_90d
                )
            },
            points: churn,
            input: "emrac history".into(),
            cap: None,
        });
    }

    // Trust.
    if f.binary.is_some() && !f.origin.is_aur() {
        score -= TRUST_PENALTY;
        reasons.push(Reason {
            rule: "trust",
            side: Side::Binary,
            text: "the official binary is signed by Arch's keys; a source build is as trusted as its audit".into(),
            points: -TRUST_PENALTY,
            input: "the package comes from an official repository".into(),
            cap: None,
        });
    }

    // Fit.
    let peak = f.class.peak_mib();
    let mut short = Vec::new();
    if f.machine.mem_mib > 0 && f.machine.mem_mib < peak {
        short.push(format!(
            "memory ({} MiB, a {} build wants about {peak} MiB)",
            f.machine.mem_mib,
            f.class.label()
        ));
    }
    if let Some(free) = f.machine.free_disk_mib {
        if free < DISK_NEEDED_MIB {
            short.push(format!(
                "disk ({free} MiB free, about {DISK_NEEDED_MIB} MiB wanted)"
            ));
        }
    }
    if !short.is_empty() {
        score -= FIT_PENALTY;
        reasons.push(Reason {
            rule: "fit",
            side: Side::Source,
            text: format!("this machine is short of {}", short.join(" and ")),
            points: -FIT_PENALTY,
            input: "/proc/meminfo, df".into(),
            cap: None,
        });
    }

    // Weight of the dependency tree: shown, not scored.
    if let Some(total) = f.deps_total {
        notes.push(format!(
            "{} direct and {total} transitive dependencies",
            f.deps_direct
        ));
    }
    if f.binary.is_none() {
        // Nothing to choose between.
        reasons.retain(|r| r.side != Side::Binary);
        return Report {
            name: f.name.clone(),
            verdict: Verdict::SourceOnly,
            score: None,
            confidence: if basis == "measured" {
                Confidence::High
            } else {
                Confidence::Medium
            },
            confidence_why: "there is no binary to compare with".into(),
            build_secs: Some((lo, likely, hi)),
            build_basis: basis,
            reasons,
            notes,
        };
    }

    if let Some(c) = cap {
        score = score.min(c);
    }
    let score = score.clamp(0, 100);
    let verdict = verdict_of(score);

    // Confidence: how much the verdict depends on an uncertain input.
    let decided_by_flags = cap.is_some() && verdict == Verdict::Binary;
    let stable = |secs: u64| {
        let mut g = f.clone();
        g.measured = vec![secs];
        evaluate(&g).score.map(verdict_of) == Some(verdict)
    };
    let (confidence, why) = if decided_by_flags {
        (
            Confidence::High,
            "the verdict follows from the flags, not from an estimate".to_string(),
        )
    } else if f.measured.len() >= 3 {
        (
            Confidence::High,
            format!(
                "{} builds of this package were measured here",
                f.measured.len()
            ),
        )
    } else if !f.measured.is_empty() {
        (
            Confidence::Medium,
            "one build of this package was measured here; three make it high".to_string(),
        )
    } else if stable(lo) && stable(hi) {
        (
            Confidence::Medium,
            "the build time is only estimated, but the verdict is the same at half and at double the estimate".to_string(),
        )
    } else {
        (
            Confidence::Low,
            "the build time is only estimated and the verdict changes within the estimate's range; build it once and emrac will have a measured time".to_string(),
        )
    };

    Report {
        name: f.name.clone(),
        verdict,
        score: Some(score),
        confidence,
        confidence_why: why,
        build_secs: Some((lo, likely, hi)),
        build_basis: basis,
        reasons,
        notes,
    }
}

fn verdict_of(score: i32) -> Verdict {
    if score >= SOURCE_AT {
        Verdict::Source
    } else if score <= BINARY_AT {
        Verdict::Binary
    } else {
        Verdict::Either
    }
}

// ------------------------------------------------------------------ inputs

/// Reads `CFLAGS` and `MAKEFLAGS` from makepkg.conf text; the last
/// assignment wins, as in the shell.
pub fn parse_makepkg(text: &str, into: &mut Flags) {
    // a value may continue on the next line after a trailing backslash
    let text = text.replace("\\\n", " ");
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("CFLAGS=") {
            into.cflags = v.trim_matches(['"', '\'']).to_string();
        } else if let Some(v) = line.strip_prefix("MAKEFLAGS=") {
            let v = v.trim_matches(['"', '\'']);
            into.jobs = v
                .split_whitespace()
                .find_map(|w| w.strip_prefix("-j"))
                .and_then(|n| n.parse().ok())
                .or(into.jobs);
        }
    }
    into.march = into
        .cflags
        .split_whitespace()
        .rev()
        .find_map(|w| w.strip_prefix("-march="))
        .map(str::to_string);
    into.opt = into
        .cflags
        .split_whitespace()
        .rev()
        .find_map(|w| w.strip_prefix("-O"))
        .map(str::to_string);
}

/// Is a repository for a newer x86-64 level enabled in pacman.conf text?
pub fn tuned_repo(pacman_conf: &str) -> bool {
    pacman_conf.lines().any(|l| {
        let l = l.trim();
        l.starts_with('[')
            && l.ends_with(']')
            && (l.contains("-v3") || l.contains("-v4") || l.contains("x86-64-v"))
    })
}

/// The x86-64 level (1 to 4) the CPU flags of `/proc/cpuinfo` allow; 0 when
/// no flags line is given.
pub fn isa_level(cpuinfo: &str) -> u8 {
    let Some(line) = cpuinfo.lines().find(|l| l.starts_with("flags")) else {
        return 0;
    };
    let flags: std::collections::HashSet<&str> = line
        .split(':')
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .collect();
    let has = |names: &[&str]| names.iter().all(|n| flags.contains(n));
    let v2 = has(&["cx16", "popcnt", "ssse3", "sse4_1", "sse4_2"]);
    let v3 = v2 && has(&["avx", "avx2", "bmi1", "bmi2", "fma", "movbe", "f16c", "abm"]);
    let v4 = v3 && has(&["avx512f", "avx512bw", "avx512cd", "avx512dq", "avx512vl"]);
    match (v2, v3, v4) {
        (_, _, true) => 4,
        (_, true, _) => 3,
        (true, _, _) => 2,
        _ => 1,
    }
}

/// The toolchain class from an AUR package's `makedepends`.
pub fn class_from_makedepends(makedepends: &[String]) -> (Class, String) {
    let has = |names: &[&str]| {
        makedepends.iter().any(|d| {
            let n = d.split(['<', '>', '=']).next().unwrap_or(d);
            names.contains(&n)
        })
    };
    let starts = |prefixes: &[&str]| {
        makedepends
            .iter()
            .any(|d| prefixes.iter().any(|p| d.starts_with(p)))
    };
    if has(&["rust", "cargo", "rustup"]) {
        (
            Class::Rust,
            "its makedepends name Rust (rust or cargo)".into(),
        )
    } else if has(&["go"]) {
        (Class::Go, "its makedepends name Go".into())
    } else if starts(&["jdk", "maven", "gradle"]) {
        (Class::Java, "its makedepends name Java".into())
    } else if has(&["cmake", "meson", "ninja", "clang", "llvm", "boost"]) || starts(&["qt"]) {
        (
            Class::Cpp,
            "its makedepends name a C++ build system or library".into(),
        )
    } else if has(&[
        "gcc",
        "make",
        "autoconf",
        "automake",
        "pkgconf",
        "pkg-config",
    ]) {
        (Class::C, "its makedepends name a C toolchain".into())
    } else if has(&[
        "python-setuptools",
        "python-build",
        "python-installer",
        "npm",
        "nodejs",
    ]) {
        (
            Class::NoCompile,
            "its makedepends name only an interpreter's packaging tools".into(),
        )
    } else {
        (
            Class::Unknown,
            "its makedepends say nothing about the toolchain".into(),
        )
    }
}

/// The class of an official package, from its architecture and
/// dependencies (the sync database has no makedepends).
pub fn class_from_depends(arch_any: bool, depends: &[String]) -> (Class, String) {
    if arch_any {
        return (Class::NoCompile, "its architecture is `any`".into());
    }
    let names: Vec<&str> = depends
        .iter()
        .map(|d| d.split(['<', '>', '=', ':']).next().unwrap_or(d))
        .collect();
    let any_prefix = |p: &[&str]| names.iter().any(|n| p.iter().any(|x| n.starts_with(x)));
    if any_prefix(&["qt", "boost", "llvm", "gtkmm", "libstdc++"]) || names.contains(&"gcc-libs") {
        (
            Class::Cpp,
            "its dependencies (gcc-libs, Qt, Boost…) point to C++ or Rust".into(),
        )
    } else {
        (
            Class::Unknown,
            "its dependencies say only that it is a native program (C, Rust or Go)".into(),
        )
    }
}

/// Memory, cores and the CPU level from the proc files given as text.
pub fn machine_from(
    meminfo: &str,
    cpuinfo: &str,
    cores: u32,
    free_disk_mib: Option<u64>,
) -> Machine {
    let mem_kib: u64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Machine {
        cores: cores.max(1),
        mem_mib: mem_kib / 1024,
        free_disk_mib,
        isa_level: isa_level(cpuinfo),
    }
}

/// Reads the machine and the makepkg flags from this system.
pub fn gather_machine() -> (Machine, Flags) {
    let read = |p: &str| fs::read_to_string(p).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let mut flags = Flags::default();
    for path in [
        "/etc/makepkg.conf".to_string(),
        format!("{home}/.makepkg.conf"),
        format!("{home}/.config/pacman/makepkg.conf"),
    ] {
        let text = read(&path);
        if !text.is_empty() {
            parse_makepkg(&text, &mut flags);
        }
    }
    flags.tuned_repo = tuned_repo(&read("/etc/pacman.conf"));
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let probe = std::path::Path::new(&home).join(".cache");
    let probe = if probe.exists() {
        probe
    } else {
        PathBuf::from(&home)
    };
    let free = std::process::Command::new("df")
        .arg("-Pk")
        .arg(&probe)
        .output()
        .ok()
        .and_then(|o| crate::doctor::parse_df(&String::from_utf8_lossy(&o.stdout)))
        .map(|(avail, _)| avail / 1_048_576);
    (
        machine_from(&read("/proc/meminfo"), &read("/proc/cpuinfo"), cores, free),
        flags,
    )
}

// ------------------------------------------------------------- build times

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildTime {
    /// Unix seconds when the build finished.
    pub time: u64,
    pub name: String,
    pub version: String,
    pub secs: u64,
    pub cores: u32,
    pub ok: bool,
}

fn build_times_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/build-times.jsonl"))
}

fn append_to(path: &std::path::Path, entry: &BuildTime) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut lines: Vec<String> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    lines.push(serde_json::to_string(entry).unwrap_or_default());
    if lines.len() > KEEP_BUILDS {
        lines.drain(..lines.len() - KEEP_BUILDS);
    }
    let tmp = path.with_extension("jsonl.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all((lines.join("\n") + "\n").as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

/// Notes how long a build took. Never fails the build: a write error is lost.
pub fn record_build(name: &str, version: &str, secs: u64, ok: bool) {
    let Ok(path) = build_times_path() else { return };
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let _ = append_to(
        &path,
        &BuildTime {
            time: crate::journal::unix_now(),
            name: name.to_string(),
            version: version.to_string(),
            secs,
            cores,
            ok,
        },
    );
}

fn secs_for(path: &std::path::Path, name: &str, cores: u32) -> Vec<u64> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<BuildTime>(l).ok())
        .filter(|b| b.ok && b.name == name && b.cores == cores)
        .map(|b| b.secs)
        .collect()
}

/// Earlier successful builds of `name` on a machine with this many cores.
pub fn measured_builds(name: &str, cores: u32) -> Vec<u64> {
    build_times_path()
        .map(|p| secs_for(&p, name, cores))
        .unwrap_or_default()
}

/// Installs and upgrades of `name` that committed in the 90 days before `now`.
pub fn updates_in_window(
    history: &[crate::history::TransactionRecord],
    name: &str,
    now: u64,
) -> u32 {
    use crate::history::TxStatus;
    use crate::plan::PlanAction;
    let since = now.saturating_sub(90 * 86_400);
    history
        .iter()
        .filter(|r| {
            r.status == TxStatus::Committed
                && r.timestamp >= since
                && matches!(r.action, PlanAction::Install | PlanAction::Upgrade)
                && r.packages.iter().any(|p| p.name == name)
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCH_DEFAULT: &str = "-march=x86-64 -mtune=generic -O2 -pipe -fno-plt";

    fn facts() -> Facts {
        let mut flags = Flags::default();
        parse_makepkg(&format!("CFLAGS=\"{ARCH_DEFAULT}\"\n"), &mut flags);
        Facts {
            name: "ripgrep".into(),
            origin: Origin {
                repo: "extra".into(),
            },
            binary: Some("ripgrep".into()),
            arch_any: false,
            installed_mib: Some(12.0),
            download_mib: Some(3.0),
            deps_direct: 2,
            deps_total: Some(6),
            class: Class::Rust,
            class_from: "its dependencies".into(),
            measured: vec![],
            updates_90d: 0,
            flags,
            machine: Machine {
                cores: 8,
                mem_mib: 16384,
                free_disk_mib: Some(100_000),
                isa_level: 3,
            },
        }
    }

    fn native(f: &mut Facts) {
        let mut flags = Flags::default();
        parse_makepkg("CFLAGS=\"-march=native -O2 -pipe\"\n", &mut flags);
        f.flags = flags;
    }

    #[test]
    fn the_repositorys_own_flags_cap_the_score_and_say_why() {
        let r = evaluate(&facts());
        assert_eq!(r.verdict, Verdict::Binary);
        assert!(r.score.unwrap() <= SAME_FLAGS_CAP);
        let first = &r.reasons[0];
        assert_eq!(first.rule, "gain-none");
        assert_eq!(first.cap, Some(SAME_FLAGS_CAP));
        assert!(first.text.contains("defaults"), "{}", first.text);
        assert_eq!(r.confidence, Confidence::High, "{}", r.confidence_why);
    }

    #[test]
    fn a_tuned_build_of_something_quick_is_worth_it() {
        let mut f = facts();
        native(&mut f);
        f.class = Class::C;
        f.installed_mib = Some(1.0);
        let r = evaluate(&f);
        assert!(r
            .reasons
            .iter()
            .any(|x| x.rule == "gain-tuned" && x.points == TUNED_GAIN));
        // 50 + 25 + 0 (quick) - 5 (trust) = 70
        assert_eq!(r.score, Some(70));
        assert_eq!(r.verdict, Verdict::Source);
    }

    #[test]
    fn a_long_build_outweighs_the_gain() {
        let mut f = facts();
        native(&mut f);
        f.class = Class::Cpp;
        f.installed_mib = Some(3000.0);
        f.updates_90d = 6;
        let r = evaluate(&f);
        assert_eq!(r.verdict, Verdict::Binary, "{r:?}");
        assert!(r
            .reasons
            .iter()
            .any(|x| x.rule == "cost" && x.points <= -30));
        assert!(r
            .reasons
            .iter()
            .any(|x| x.rule == "churn" && x.points == -15));
    }

    #[test]
    fn cost_bands_and_churn_bands_follow_the_plan() {
        for (secs, pts) in [
            (0, 0),
            (119, 0),
            (120, -10),
            (599, -10),
            (600, -20),
            (1799, -20),
            (1800, -30),
            (3599, -30),
            (3600, -40),
        ] {
            assert_eq!(cost_points(secs), pts, "{secs}");
        }
        for (n, pts) in [(0, 0), (2, 0), (3, -8), (5, -8), (6, -15), (30, -15)] {
            assert_eq!(churn_points(n), pts, "{n}");
        }
    }

    #[test]
    fn verdict_boundaries() {
        assert_eq!(verdict_of(65), Verdict::Source);
        assert_eq!(verdict_of(64), Verdict::Either);
        assert_eq!(verdict_of(36), Verdict::Either);
        assert_eq!(verdict_of(35), Verdict::Binary);
    }

    #[test]
    fn an_architecture_independent_package_has_nothing_to_compile() {
        let mut f = facts();
        f.arch_any = true;
        native(&mut f);
        let r = evaluate(&f);
        assert_eq!(r.verdict, Verdict::Binary);
        assert_eq!(r.score, Some(0));
        assert_eq!(r.reasons[0].rule, "nothing-to-compile");
        assert_eq!(r.confidence, Confidence::High);
        assert!(r.build_secs.is_none());
    }

    #[test]
    fn no_binary_means_source_is_the_only_option_and_nothing_is_scored() {
        let mut f = facts();
        f.origin = Origin { repo: "aur".into() };
        f.binary = None;
        f.installed_mib = None;
        let r = evaluate(&f);
        assert_eq!(r.verdict, Verdict::SourceOnly);
        assert_eq!(r.score, None);
        assert!(r.build_secs.is_some());
        assert!(r.reasons.iter().all(|x| x.side != Side::Binary));
    }

    #[test]
    fn a_flag_the_cpu_cannot_run_does_not_count() {
        let mut f = facts();
        let mut flags = Flags::default();
        parse_makepkg("CFLAGS=\"-march=x86-64-v4 -O2\"\n", &mut flags);
        f.flags = flags;
        f.machine.isa_level = 2;
        let r = evaluate(&f);
        assert!(r.reasons.iter().any(|x| x.rule == "gain-unsupported"));
        assert!(r.score.unwrap() <= SAME_FLAGS_CAP);
    }

    #[test]
    fn a_tuned_repository_makes_v3_flags_no_gain_but_native_still_is_one() {
        let mut f = facts();
        let mut flags = Flags {
            tuned_repo: true,
            ..Flags::default()
        };
        parse_makepkg("CFLAGS=\"-march=x86-64-v3 -O2\"\n", &mut flags);
        f.flags = flags.clone();
        assert!(evaluate(&f).reasons.iter().any(|x| x.rule == "gain-none"));
        parse_makepkg("CFLAGS=\"-march=native -O2\"\n", &mut flags);
        f.flags = flags;
        assert!(evaluate(&f).reasons.iter().any(|x| x.rule == "gain-tuned"));
    }

    #[test]
    fn measured_builds_replace_the_estimate_and_raise_the_confidence() {
        let mut f = facts();
        native(&mut f);
        f.class = Class::Cpp;
        f.installed_mib = Some(60.0);
        let est = evaluate(&f);
        assert_eq!(est.build_basis, "estimated");
        f.measured = vec![200];
        let one = evaluate(&f);
        assert_eq!(one.build_basis, "measured");
        assert_eq!(one.build_secs, Some((200, 200, 200)));
        assert_eq!(one.confidence, Confidence::Medium);
        f.measured = vec![200, 210, 190];
        let three = evaluate(&f);
        assert_eq!(three.confidence, Confidence::High);
        assert_eq!(three.build_secs.unwrap().1, 200, "the median");
    }

    #[test]
    fn a_verdict_that_flips_inside_the_estimates_range_has_low_confidence() {
        let mut f = facts();
        native(&mut f);
        f.class = Class::Cpp;
        // about 150 s: half of it is quick (source), double is not (either)
        f.installed_mib = Some(177.0);
        let r = evaluate(&f);
        assert_eq!(r.verdict, Verdict::Either, "{r:?}");
        assert_eq!(r.confidence, Confidence::Low, "{}", r.confidence_why);
        assert!(r.confidence_why.contains("changes"));
        // a measured build removes the doubt
        f.measured = vec![150, 150, 150];
        assert_eq!(evaluate(&f).confidence, Confidence::High);
    }

    #[test]
    fn short_memory_or_disk_costs_points_and_names_what_is_short() {
        let mut f = facts();
        native(&mut f);
        f.class = Class::Rust;
        f.machine.mem_mib = 2048;
        f.machine.free_disk_mib = Some(500);
        let r = evaluate(&f);
        let fit = r.reasons.iter().find(|x| x.rule == "fit").unwrap();
        assert_eq!(fit.points, -FIT_PENALTY);
        assert!(fit.text.contains("memory") && fit.text.contains("disk"));
    }

    #[test]
    fn makepkg_conf_text_is_read_last_assignment_wins() {
        let mut f = Flags::default();
        parse_makepkg("CFLAGS=\"-march=x86-64 -O2\"\nMAKEFLAGS=\"-j4\"\n", &mut f);
        parse_makepkg("# mine\nCFLAGS='-march=native -O3 -pipe'\n", &mut f);
        let mut g = Flags::default();
        parse_makepkg("CFLAGS=\"-pipe \\\n  -march=x86-64-v3 -O2\"\n", &mut g);
        assert_eq!(g.march.as_deref(), Some("x86-64-v3"), "{g:?}");
        assert_eq!(f.march.as_deref(), Some("native"));
        assert_eq!(f.opt.as_deref(), Some("3"));
        assert_eq!(f.jobs, Some(4));
        assert!(!tuned_repo("[core]\n[extra]\n"));
        assert!(tuned_repo("[core]\n[cachyos-v3]\n"));
    }

    #[test]
    fn the_cpu_level_follows_the_flags() {
        let base = "flags : fpu sse sse2";
        let v2 = "flags : cx16 popcnt ssse3 sse4_1 sse4_2";
        let v3 = "flags : cx16 popcnt ssse3 sse4_1 sse4_2 avx avx2 bmi1 bmi2 fma movbe f16c abm";
        let v4 = format!("{v3} avx512f avx512bw avx512cd avx512dq avx512vl");
        assert_eq!(isa_level(base), 1);
        assert_eq!(isa_level(v2), 2);
        assert_eq!(isa_level(v3), 3);
        assert_eq!(isa_level(&v4), 4);
        assert_eq!(isa_level("model name : x"), 0);
    }

    #[test]
    fn the_toolchain_is_guessed_from_makedepends_and_from_dependencies() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(class_from_makedepends(&v(&["cargo"])).0, Class::Rust);
        assert_eq!(
            class_from_makedepends(&v(&["cmake", "qt6-base"])).0,
            Class::Cpp
        );
        assert_eq!(class_from_makedepends(&v(&["go"])).0, Class::Go);
        assert_eq!(
            class_from_makedepends(&v(&["jdk17-openjdk"])).0,
            Class::Java
        );
        assert_eq!(class_from_makedepends(&v(&["make"])).0, Class::C);
        assert_eq!(
            class_from_makedepends(&v(&["python-build"])).0,
            Class::NoCompile
        );
        assert_eq!(class_from_makedepends(&v(&[])).0, Class::Unknown);
        assert_eq!(class_from_depends(true, &v(&["glibc"])).0, Class::NoCompile);
        assert_eq!(
            class_from_depends(false, &v(&["glibc", "gcc-libs"])).0,
            Class::Cpp
        );
        assert_eq!(class_from_depends(false, &v(&["glibc"])).0, Class::Unknown);
    }

    #[test]
    fn machine_facts_come_from_the_proc_text() {
        let m = machine_from(
            "MemTotal:       16384000 kB\n",
            "flags : cx16\n",
            0,
            Some(5),
        );
        assert_eq!((m.cores, m.mem_mib, m.free_disk_mib), (1, 16000, Some(5)));
    }

    #[test]
    fn the_build_times_file_is_written_capped_and_read_back_by_package_and_cores() {
        let d = std::env::temp_dir().join(format!("emrac-bt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let path = d.join("build-times.jsonl");
        let entry = |name: &str, secs, cores, ok| BuildTime {
            time: 1,
            name: name.into(),
            version: "1".into(),
            secs,
            cores,
            ok,
        };
        append_to(&path, &entry("a", 100, 8, true)).unwrap();
        append_to(&path, &entry("a", 120, 8, true)).unwrap();
        append_to(&path, &entry("a", 999, 8, false)).unwrap();
        append_to(&path, &entry("a", 50, 4, true)).unwrap();
        append_to(&path, &entry("b", 7, 8, true)).unwrap();
        assert_eq!(
            secs_for(&path, "a", 8),
            vec![100, 120],
            "failures and other core counts are not used"
        );
        for i in 0..KEEP_BUILDS + 20 {
            append_to(&path, &entry("c", i as u64, 8, true)).unwrap();
        }
        let n = fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(n, KEEP_BUILDS);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn updates_count_only_committed_installs_and_upgrades_in_90_days() {
        use crate::history::{TransactionRecord, TxStatus};
        use crate::plan::{PlanAction, PlannedPackage};
        let rec = |ts, action, status| TransactionRecord {
            id: "x".into(),
            timestamp: ts,
            action,
            packages: vec![PlannedPackage {
                name: "p".into(),
                version: "1".into(),
                repo: "extra".into(),
                explicit: true,
                previous_version: None,
            }],
            removed: vec![],
            status,
            error: None,
            rollback_of: None,
            plan_digest: None,
        };
        let now = 100 * 86_400;
        let h = vec![
            rec(now - 10 * 86_400, PlanAction::Upgrade, TxStatus::Committed),
            rec(now - 20 * 86_400, PlanAction::Install, TxStatus::Committed),
            rec(now - 30 * 86_400, PlanAction::Upgrade, TxStatus::Failed),
            rec(now - 40 * 86_400, PlanAction::Remove, TxStatus::Committed),
            rec(now - 91 * 86_400, PlanAction::Upgrade, TxStatus::Committed),
        ];
        assert_eq!(updates_in_window(&h, "p", now), 2);
        assert_eq!(updates_in_window(&h, "other", now), 0);
    }
}

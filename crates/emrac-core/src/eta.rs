//! How long an operation will take (u26, SPEC III.4): builds from what the same
//! package took here, scaled for the cores there are now; downloads and installs
//! from what earlier transactions achieved on this machine; the total with a range
//! and a confidence; the live estimate refined while a build runs; and a record of
//! how far the estimates were off, which is shown with them.
//!
//! An estimate that cannot be made says so. It never guesses a number for a first
//! build of something large.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::recommend::BuildTime;
use crate::stale_builds::is_binary_package;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
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

/// One piece of the estimate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Part {
    /// `download and install`, `build brave-bin`.
    pub label: String,
    pub secs: u64,
    pub low: u64,
    pub high: u64,
    pub confidence: Confidence,
    /// What it is based on.
    pub basis: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Estimate {
    pub parts: Vec<Part>,
    /// Things that need time and could not be estimated, with why.
    pub unknown: Vec<String>,
}

impl Estimate {
    /// `(seconds, low, high, confidence)` of the parts that could be estimated.
    pub fn total(&self) -> Option<(u64, u64, u64, Confidence)> {
        if self.parts.is_empty() {
            return None;
        }
        let secs = self.parts.iter().map(|p| p.secs).sum();
        let low = self.parts.iter().map(|p| p.low).sum();
        let high = self.parts.iter().map(|p| p.high).sum();
        let mut conf = self
            .parts
            .iter()
            .map(|p| p.confidence)
            .min()
            .unwrap_or(Confidence::Low);
        // something that could not be estimated at all caps what is said
        if !self.unknown.is_empty() {
            conf = conf.min(Confidence::Low);
        }
        Some((secs, low, high, conf))
    }
}

/// `2 min 10 s`, `45 s`, `1 h 04 min`.
pub fn words(secs: u64) -> String {
    crate::build_ui::elapsed_words(secs * 1000)
}

/// The build time scaled from `then` cores to `now` cores, assuming 85 % of the work
/// runs in parallel (Amdahl). Hardware changes are bounded: a guess that is more
/// than four times off either way is a bad guess.
pub fn scale_cores(secs: u64, then: u32, now: u32) -> u64 {
    if then == 0 || now == 0 || then == now {
        return secs;
    }
    let p = 0.85;
    let t = |c: u32| (1.0 - p) + p / f64::from(c);
    let factor = (t(now) / t(then)).clamp(0.25, 4.0);
    (secs as f64 * factor).round() as u64
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2
    }
}

/// How long building `name` will take, from earlier builds here. `None`: nothing
/// to base an honest number on.
pub fn build_part(name: &str, builds: &[BuildTime], cores_now: u32) -> Option<Part> {
    let mut mine: Vec<u64> = builds
        .iter()
        .rev()
        .filter(|b| b.ok && b.name == name)
        .take(5)
        .map(|b| scale_cores(b.secs, b.cores, cores_now))
        .collect();
    if !mine.is_empty() {
        let n = mine.len();
        let lo = *mine.iter().min()?;
        let hi = *mine.iter().max()?;
        let med = median(&mut mine);
        let spread = if med == 0 {
            0.0
        } else {
            (hi - lo) as f64 / med as f64
        };
        let (low, high) = if n == 1 {
            (med * 7 / 10, med * 3 / 2)
        } else {
            (lo, hi)
        };
        let confidence = if n >= 3 && spread <= 0.35 {
            Confidence::High
        } else {
            Confidence::Medium
        };
        return Some(Part {
            label: format!("build {name}"),
            secs: med,
            low,
            high,
            confidence,
            basis: format!(
                "{n} earlier build{} of it here{}",
                if n == 1 { "" } else { "s" },
                if cores_now != builds.iter().rev().find(|b| b.ok && b.name == name)?.cores {
                    ", scaled for the cores there are now"
                } else {
                    ""
                }
            ),
        });
    }
    // a prebuilt (-bin) package is mostly a download and a repackaging
    if is_binary_package(name) {
        let mut others: Vec<u64> = builds
            .iter()
            .rev()
            .filter(|b| b.ok && is_binary_package(&b.name))
            .take(10)
            .map(|b| b.secs)
            .collect();
        if others.len() >= 3 {
            let med = median(&mut others);
            return Some(Part {
                label: format!("build {name}"),
                secs: med,
                low: med / 2,
                high: med * 3,
                confidence: Confidence::Low,
                basis: format!("your other {} -bin packages, not this one", others.len()),
            });
        }
    }
    None
}

/// One finished official transaction: what it moved and how long it took.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Op {
    pub time: u64,
    pub bytes: u64,
    pub packages: u32,
    pub secs: u64,
}

const BIG: u64 = 30 << 20;
const SMALL: u64 = 5 << 20;

/// The download and install of `bytes` in `packages` packages, from earlier
/// transactions; `mirror_kib` is a last resort (what `emrac mirrors` measured).
pub fn transfer_part(
    bytes: u64,
    packages: usize,
    ops: &[Op],
    mirror_kib: Option<u64>,
) -> Option<Part> {
    if packages == 0 {
        return None;
    }
    // seconds per package when almost nothing was downloaded
    let mut fixed: Vec<u64> = ops
        .iter()
        .filter(|o| o.bytes < SMALL && o.packages > 0)
        .map(|o| (o.secs * 1000) / u64::from(o.packages))
        .collect();
    let per_pkg_ms = if fixed.is_empty() {
        1500
    } else {
        median(&mut fixed).max(300)
    };
    let mut rates: Vec<u64> = ops
        .iter()
        .filter(|o| o.bytes >= BIG && o.secs > 0)
        .map(|o| o.bytes / o.secs)
        .collect();
    let (rate, basis, confidence) = if rates.len() >= 2 {
        (
            median(&mut rates),
            format!("{} earlier transactions here", rates.len()),
            Confidence::Medium,
        )
    } else if rates.len() == 1 {
        (
            rates[0],
            "one earlier transaction here".to_string(),
            Confidence::Low,
        )
    } else if let Some(k) = mirror_kib.filter(|k| *k > 0) {
        (
            k * 1024 * 6 / 10,
            "the mirror speed `emrac mirrors` measured".to_string(),
            Confidence::Low,
        )
    } else if bytes < SMALL {
        (1, "typical overhead".to_string(), Confidence::Low)
    } else {
        return None;
    };
    let download = if bytes < SMALL {
        0
    } else {
        bytes / rate.max(1)
    };
    let fixed_secs = (per_pkg_ms * packages as u64).div_ceil(1000);
    let secs = download + fixed_secs;
    Some(Part {
        label: "download and install".to_string(),
        secs,
        low: secs * 6 / 10,
        high: secs * 2 + 2,
        confidence,
        basis,
    })
}

/// How far an estimate was from what happened.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Accuracy {
    pub time: u64,
    pub estimated: u64,
    pub actual: u64,
}

/// "your estimates have been within a quarter in 7 of the last 9": `None` with
/// fewer than three to go on.
pub fn accuracy_summary(records: &[Accuracy]) -> Option<String> {
    let last: Vec<&Accuracy> = records.iter().rev().take(20).collect();
    if last.len() < 3 {
        return None;
    }
    let near = last
        .iter()
        .filter(|a| {
            let (e, t) = (a.estimated as f64, a.actual as f64);
            (e - t).abs() <= 0.25 * t.max(5.0)
        })
        .count();
    Some(format!(
        "estimates so far: within a quarter of the real time in {near} of the last {}",
        last.len()
    ))
}

/// The bar a build screen shows next to its clock: how long is left, or that it is
/// taking longer than usual.
pub fn live_note(expected_ms: u64, elapsed_ms: u64) -> String {
    if elapsed_ms < expected_ms {
        format!(
            "~{} left",
            crate::build_ui::elapsed_words(expected_ms - elapsed_ms)
        )
    } else {
        format!(
            "longer than the usual {}",
            crate::build_ui::elapsed_words(expected_ms)
        )
    }
}

// --- the files

fn data_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac"))
}

fn append_capped<T: Serialize>(path: &Path, entry: &T, keep: usize) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut lines: Vec<String> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    lines.push(serde_json::to_string(entry).unwrap_or_default());
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }
    let tmp = path.with_extension("jsonl.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all((lines.join("\n") + "\n").as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

fn read_all<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn ops_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("ops.jsonl"))
}

pub fn accuracy_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("eta-accuracy.jsonl"))
}

pub fn read_ops() -> Vec<Op> {
    ops_path().map(|p| read_all(&p)).unwrap_or_default()
}

pub fn read_accuracy() -> Vec<Accuracy> {
    accuracy_path().map(|p| read_all(&p)).unwrap_or_default()
}

/// Notes an official transaction that worked. Never fails it.
pub fn record_op(bytes: u64, packages: u32, secs: u64) {
    if let Ok(p) = ops_path() {
        let _ = append_capped(
            &p,
            &Op {
                time: crate::journal::unix_now(),
                bytes,
                packages,
                secs,
            },
            200,
        );
    }
}

/// Notes how an estimate compared with what happened.
pub fn record_accuracy(estimated: u64, actual: u64) {
    if let Ok(p) = accuracy_path() {
        let _ = append_capped(
            &p,
            &Accuracy {
                time: crate::journal::unix_now(),
                estimated,
                actual,
            },
            200,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, secs: u64, cores: u32, ok: bool) -> BuildTime {
        BuildTime {
            time: 1,
            name: name.into(),
            version: String::new(),
            secs,
            cores,
            ok,
        }
    }

    #[test]
    fn more_cores_shorten_a_build_less_than_proportionally() {
        assert_eq!(scale_cores(600, 4, 4), 600);
        let faster = scale_cores(600, 4, 16);
        assert!(faster < 600 && faster > 600 / 4, "{faster}");
        let slower = scale_cores(600, 16, 4);
        assert!(slower > 600 && slower < 600 * 4, "{slower}");
        // never more than four times either way
        assert_eq!(scale_cores(100, 1, 100_000), 25);
        assert_eq!(scale_cores(100, 100_000, 1), 400);
    }

    #[test]
    fn a_package_built_before_is_estimated_from_those_builds_with_a_range() {
        let history = vec![
            b("ripgrep", 40, 8, true),
            b("ripgrep", 44, 8, true),
            b("ripgrep", 42, 8, true),
            b("ripgrep", 900, 8, false),
        ];
        let p = build_part("ripgrep", &history, 8).unwrap();
        assert_eq!(p.secs, 42);
        assert_eq!((p.low, p.high), (40, 44));
        assert_eq!(
            p.confidence,
            Confidence::High,
            "three close builds; the failed one is left out"
        );
        // one build: a wide range, medium
        let one = build_part("fd", &[b("fd", 100, 8, true)], 8).unwrap();
        assert_eq!((one.secs, one.low, one.high), (100, 70, 150));
        assert_eq!(one.confidence, Confidence::Medium);
        // scattered builds are not high
        let wild = build_part(
            "x",
            &[
                b("x", 10, 8, true),
                b("x", 100, 8, true),
                b("x", 300, 8, true),
            ],
            8,
        )
        .unwrap();
        assert_eq!(wild.confidence, Confidence::Medium);
    }

    #[test]
    fn a_first_build_of_something_large_gets_no_number_but_a_bin_package_gets_a_low_one() {
        let history = vec![
            b("a-bin", 20, 8, true),
            b("b-bin", 30, 8, true),
            b("c-bin", 25, 8, true),
            b("big", 3600, 8, true),
        ];
        assert!(build_part("firefox", &history, 8).is_none());
        let p = build_part("new-bin", &history, 8).unwrap();
        assert_eq!(p.secs, 25);
        assert_eq!(p.confidence, Confidence::Low);
        assert!(p.basis.contains("-bin"));
        assert!(
            build_part("new-bin", &history[..2], 8).is_none(),
            "two is too few"
        );
    }

    #[test]
    fn a_transfer_is_estimated_from_earlier_transactions_then_from_the_mirror_then_not_at_all() {
        let mib = 1 << 20;
        let ops = vec![
            Op {
                time: 1,
                bytes: 100 * mib,
                packages: 10,
                secs: 30,
            },
            Op {
                time: 2,
                bytes: 60 * mib,
                packages: 5,
                secs: 20,
            },
            Op {
                time: 3,
                bytes: mib,
                packages: 2,
                secs: 4,
            },
        ];
        let p = transfer_part(90 * mib, 6, &ops, None).unwrap();
        assert_eq!(p.confidence, Confidence::Medium);
        // median rate of 100/30 and 60/20 = about 3.2 MiB/s: ~28 s of download plus 2 s per package
        assert!((30..=50).contains(&p.secs), "{}", p.secs);
        // one transaction to go on
        let lone = transfer_part(50 * mib, 3, &ops[..1], None).unwrap();
        assert_eq!(lone.confidence, Confidence::Low);
        // the mirror, when nothing else
        let m = transfer_part(60 * mib, 3, &[], Some(10_000)).unwrap();
        assert_eq!(m.confidence, Confidence::Low);
        assert!(m.basis.contains("mirror"));
        // a big download with nothing to go on is not guessed
        assert!(transfer_part(500 * mib, 3, &[], None).is_none());
        // a tiny one is only overhead
        assert!(transfer_part(1000, 2, &[], None).is_some());
        assert!(transfer_part(10, 0, &ops, None).is_none());
    }

    #[test]
    fn the_total_adds_the_parts_and_takes_the_weakest_confidence() {
        let part = |secs, low, high, c| Part {
            label: "x".into(),
            secs,
            low,
            high,
            confidence: c,
            basis: String::new(),
        };
        let e = Estimate {
            parts: vec![
                part(100, 80, 150, Confidence::High),
                part(40, 20, 90, Confidence::Medium),
            ],
            unknown: vec![],
        };
        assert_eq!(e.total(), Some((140, 100, 240, Confidence::Medium)));
        let with_unknown = Estimate {
            unknown: vec!["firefox (a first build here)".into()],
            ..e.clone()
        };
        assert_eq!(with_unknown.total().unwrap().3, Confidence::Low);
        assert_eq!(Estimate::default().total(), None);
    }

    #[test]
    fn the_accuracy_is_told_only_when_there_is_enough_to_say() {
        let a = |e, t| Accuracy {
            time: 1,
            estimated: e,
            actual: t,
        };
        assert!(accuracy_summary(&[a(100, 100), a(50, 50)]).is_none());
        let s = accuracy_summary(&[a(100, 110), a(100, 200), a(60, 60), a(300, 310)]).unwrap();
        assert!(s.contains("3 of the last 4"), "{s}");
    }

    #[test]
    fn the_live_note_counts_down_and_then_says_it_is_slow() {
        assert_eq!(live_note(240_000, 100_000), "~2 min 20 s left");
        assert_eq!(
            live_note(240_000, 250_000),
            "longer than the usual 4 min 00 s"
        );
    }
}

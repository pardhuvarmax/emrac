//! Mirror Intelligence (u16): measure the mirrors in `mirrorlist`, remember how
//! they did, and put them in an order the history supports.
//! See `docs/plan/r3/u16-mirror-intelligence.md`.
//!
//! The order is a stated rule, not a weighted score: good (fast first), stale,
//! unreliable, untested. It only ever reorders the mirrors that were already
//! active in the file.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Newest samples kept in the history file.
pub const KEEP_SAMPLES: usize = 3000;
/// A mirror is judged by its last this-many attempts.
pub const RECENT: usize = 10;
/// Share of the recent attempts that must have worked.
pub const RELIABLE_PERCENT: usize = 80;
/// A mirror this far behind Arch is stale (seconds).
pub const STALE_AFTER: u64 = 86_400;
const MAX_DB: u64 = 8 << 20;

fn bad(why: impl Into<String>) -> Error {
    Error::Mirrors(why.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The `Server` address as written (with `$repo` and `$arch`).
    pub url: String,
    pub active: bool,
}

/// Every `Server = ...` line, active or commented out, in file order.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let t = line.trim();
            let (active, rest) = match t.strip_prefix('#') {
                Some(r) => (false, r.trim_start()),
                None => (true, t),
            };
            let rest = rest
                .strip_prefix("Server")?
                .trim_start()
                .strip_prefix('=')?;
            let url = rest.trim();
            (url.starts_with("http://") || url.starts_with("https://")).then(|| Entry {
                url: url.to_string(),
                active,
            })
        })
        .collect()
}

/// The address of `file` under `repo` for this architecture.
pub fn probe_url(server: &str, repo: &str, arch: &str, file: &str) -> String {
    let base = server.replace("$repo", repo).replace("$arch", arch);
    format!("{}/{file}", base.trim_end_matches('/'))
}

/// The top of the mirror (before `$repo`), where Arch mirrors keep `lastsync`.
pub fn root_of(server: &str) -> String {
    let cut = server.find("$repo").unwrap_or(server.len());
    server[..cut].trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sample {
    /// Unix seconds when it was measured.
    pub time: u64,
    pub url: String,
    pub ok: bool,
    /// Until the headers arrived.
    pub ttfb_ms: u64,
    pub total_ms: u64,
    pub bytes: u64,
    /// How far behind Arch the mirror said it was (its `lastsync`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_age_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn agent_get(
    url: &str,
    timeout: Duration,
) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    ureq::get(url)
        .header("User-Agent", concat!("emrac/", env!("CARGO_PKG_VERSION")))
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
}

/// Downloads `core.db` from `server` and records how it went. Never fails:
/// a mirror that does not answer is a sample with `ok: false`.
pub fn measure(server: &str, repo: &str, arch: &str, now: u64, timeout: Duration) -> Sample {
    let url = probe_url(server, repo, arch, &format!("{repo}.db"));
    let started = Instant::now();
    let mut sample = Sample {
        time: now,
        url: server.to_string(),
        ok: false,
        ttfb_ms: 0,
        total_ms: 0,
        bytes: 0,
        sync_age_s: None,
        error: None,
    };
    match agent_get(&url, timeout) {
        Ok(mut response) => {
            sample.ttfb_ms = started.elapsed().as_millis() as u64;
            match response
                .body_mut()
                .with_config()
                .limit(MAX_DB)
                .read_to_vec()
            {
                Ok(bytes) if !bytes.is_empty() => {
                    sample.total_ms = started.elapsed().as_millis() as u64;
                    sample.bytes = bytes.len() as u64;
                    sample.ok = true;
                }
                Ok(_) => sample.error = Some("an empty answer".into()),
                Err(e) => sample.error = Some(format!("the download failed: {e}")),
            }
        }
        Err(e) => sample.error = Some(e.to_string()),
    }
    if sample.ok {
        let last = format!("{}/lastsync", root_of(server));
        if let Ok(mut r) = agent_get(&last, Duration::from_secs(5)) {
            if let Ok(text) = r.body_mut().with_config().limit(64).read_to_string() {
                if let Ok(t) = text.trim().parse::<u64>() {
                    sample.sync_age_s = Some(now.saturating_sub(t));
                }
            }
        }
    }
    sample
}

// ------------------------------------------------------------------ history

pub fn history_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/mirrors.jsonl"))
}

fn append_to(path: &Path, samples: &[Sample]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut lines: Vec<String> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    lines.extend(
        samples
            .iter()
            .map(|s| serde_json::to_string(s).unwrap_or_default()),
    );
    if lines.len() > KEEP_SAMPLES {
        lines.drain(..lines.len() - KEEP_SAMPLES);
    }
    let tmp = path.with_extension("jsonl.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all((lines.join("\n") + "\n").as_bytes())?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

pub fn record(samples: &[Sample]) -> Result<()> {
    let path = history_path()?;
    append_to(&path, samples).map_err(|e| bad(format!("{}: {e}", path.display())))
}

fn read_from(path: &Path) -> Vec<Sample> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn read_history() -> Vec<Sample> {
    history_path().map(|p| read_from(&p)).unwrap_or_default()
}

// ------------------------------------------------------------------ ranking

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Stats {
    pub attempts: usize,
    pub successes: usize,
    /// Of the last `RECENT` attempts.
    pub recent_attempts: usize,
    pub recent_successes: usize,
    pub last_ok: bool,
    pub median_total_ms: Option<u64>,
    pub median_ttfb_ms: Option<u64>,
    /// Of the successful recent ones.
    pub kib_per_s: Option<u64>,
    /// From the newest successful sample that knew it.
    pub sync_age_s: Option<u64>,
    pub last_error: Option<String>,
}

fn median(v: &mut [u64]) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2
    })
}

/// What the history says about each mirror (samples are oldest first).
pub fn summarize(samples: &[Sample]) -> BTreeMap<String, Stats> {
    let mut by: BTreeMap<&str, Vec<&Sample>> = BTreeMap::new();
    for s in samples {
        by.entry(&s.url).or_default().push(s);
    }
    by.into_iter()
        .map(|(url, list)| {
            let recent: Vec<&Sample> = list.iter().rev().take(RECENT).copied().collect();
            let ok_recent: Vec<&&Sample> = recent.iter().filter(|s| s.ok).collect();
            let mut totals: Vec<u64> = ok_recent.iter().map(|s| s.total_ms).collect();
            let mut ttfbs: Vec<u64> = ok_recent.iter().map(|s| s.ttfb_ms).collect();
            let median_total = median(&mut totals);
            let bytes: u64 = ok_recent.iter().map(|s| s.bytes).sum();
            let ms: u64 = ok_recent.iter().map(|s| s.total_ms.max(1)).sum();
            let stats = Stats {
                attempts: list.len(),
                successes: list.iter().filter(|s| s.ok).count(),
                recent_attempts: recent.len(),
                recent_successes: ok_recent.len(),
                last_ok: list.last().is_some_and(|s| s.ok),
                median_total_ms: median_total,
                median_ttfb_ms: median(&mut ttfbs),
                kib_per_s: (!ok_recent.is_empty() && bytes > 0).then(|| bytes * 1000 / ms / 1024),
                sync_age_s: list
                    .iter()
                    .rev()
                    .find(|s| s.ok && s.sync_age_s.is_some())
                    .and_then(|s| s.sync_age_s),
                last_error: list
                    .iter()
                    .rev()
                    .find(|s| !s.ok)
                    .and_then(|s| s.error.clone()),
            };
            (url.to_string(), stats)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Good,
    Stale,
    Unreliable,
    Untested,
}

impl Group {
    pub fn word(self) -> &'static str {
        match self {
            Group::Good => "good",
            Group::Stale => "stale",
            Group::Unreliable => "unreliable",
            Group::Untested => "untested",
        }
    }
}

pub fn group_of(stats: Option<&Stats>) -> Group {
    let Some(s) = stats else {
        return Group::Untested;
    };
    if s.attempts == 0 {
        return Group::Untested;
    }
    let reliable = s.last_ok && s.recent_successes * 100 >= s.recent_attempts * RELIABLE_PERCENT;
    if !reliable {
        Group::Unreliable
    } else if s.sync_age_s.is_some_and(|a| a > STALE_AFTER) {
        Group::Stale
    } else {
        Group::Good
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Ranked {
    pub url: String,
    pub group: Group,
    pub stats: Option<Stats>,
}

/// `active` in the order the file has them, put in the order the history supports.
pub fn rank(active: &[String], stats: &BTreeMap<String, Stats>) -> Vec<Ranked> {
    let mut v: Vec<(usize, Ranked)> = active
        .iter()
        .enumerate()
        .map(|(i, url)| {
            let s = stats.get(url);
            (
                i,
                Ranked {
                    url: url.clone(),
                    group: group_of(s),
                    stats: s.cloned(),
                },
            )
        })
        .collect();
    v.sort_by(|(ia, a), (ib, b)| {
        a.group.cmp(&b.group).then_with(|| match a.group {
            Group::Untested => ia.cmp(ib),
            Group::Unreliable => {
                let rate = |r: &Ranked| {
                    r.stats
                        .as_ref()
                        .map_or(0, |s| s.recent_successes * 1000 / s.recent_attempts.max(1))
                };
                rate(b)
                    .cmp(&rate(a))
                    .then_with(|| total(a).cmp(&total(b)))
                    .then_with(|| a.url.cmp(&b.url))
            }
            _ => total(a).cmp(&total(b)).then_with(|| a.url.cmp(&b.url)),
        })
    });
    v.into_iter().map(|(_, r)| r).collect()
}

fn total(r: &Ranked) -> u64 {
    r.stats
        .as_ref()
        .and_then(|s| s.median_total_ms)
        .unwrap_or(u64::MAX)
}

// ----------------------------------------------------------------- rewriting

/// The mirrorlist with the active mirrors in `order` (the best `keep` active,
/// the rest commented out). The text before the first `Server` line is kept,
/// and the mirrors that were commented out stay as they were. Only mirrors that
/// were active can be active afterwards.
pub fn render_mirrorlist(original: &str, order: &[String], keep: Option<usize>) -> Result<String> {
    let entries = parse(original);
    let active: Vec<&str> = entries
        .iter()
        .filter(|e| e.active)
        .map(|e| e.url.as_str())
        .collect();
    if active.is_empty() {
        return Err(bad("the file has no active Server line"));
    }
    if let Some(stranger) = order.iter().find(|u| !active.contains(&u.as_str())) {
        return Err(bad(format!(
            "{stranger} was not an active mirror in the file; a ranking only reorders"
        )));
    }
    if order.len() != active.len() || active.iter().any(|a| !order.iter().any(|o| o == a)) {
        return Err(bad("the order must name every active mirror exactly once"));
    }
    let keep = keep.unwrap_or(order.len()).clamp(1, order.len());
    let mut out = String::new();
    let mut header_done = false;
    for line in original.lines() {
        let t = line.trim();
        let is_server = !parse(line).is_empty();
        if !header_done && is_server {
            header_done = true;
            for url in &order[..keep] {
                out.push_str(&format!("Server = {url}\n"));
            }
            for url in &order[keep..] {
                out.push_str(&format!("#Server = {url}\n"));
            }
        }
        if is_server {
            // active ones are written above; commented ones stay where they are
            if t.starts_with('#') {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    const LIST: &str = "##\n## Arch Linux repository mirrorlist\n##\n\n## Worldwide\n#Server = https://dead.example/$repo/os/$arch\nServer = https://b.example/archlinux/$repo/os/$arch\nServer = https://a.example/$repo/os/$arch\n\n## Elsewhere\nServer = http://c.example/arch/$repo/os/$arch\n#Server = ftp://ignored.example/$repo\n";

    fn s(url: &str, ok: bool, total: u64, age: Option<u64>) -> Sample {
        Sample {
            time: 1,
            url: url.into(),
            ok,
            ttfb_ms: total / 4,
            total_ms: if ok { total } else { 0 },
            bytes: if ok { 100_000 } else { 0 },
            sync_age_s: age,
            error: (!ok).then(|| "timed out".into()),
        }
    }

    #[test]
    fn the_mirrorlist_is_parsed_active_and_commented() {
        let e = parse(LIST);
        assert_eq!(e.len(), 4, "the ftp line is not a mirror emrac can measure");
        assert_eq!(
            e[0],
            Entry {
                url: "https://dead.example/$repo/os/$arch".into(),
                active: false
            }
        );
        assert!(e[1].active && e[2].active && e[3].active);
        assert_eq!(e[3].url, "http://c.example/arch/$repo/os/$arch");
    }

    #[test]
    fn urls_are_built_the_way_pacman_builds_them() {
        assert_eq!(
            probe_url(
                "https://m.example/archlinux/$repo/os/$arch",
                "core",
                "x86_64",
                "core.db"
            ),
            "https://m.example/archlinux/core/os/x86_64/core.db"
        );
        assert_eq!(
            root_of("https://m.example/archlinux/$repo/os/$arch"),
            "https://m.example/archlinux"
        );
        assert_eq!(
            root_of("https://m.example/$repo/os/$arch"),
            "https://m.example"
        );
    }

    #[test]
    fn the_summary_takes_medians_over_successes_and_remembers_the_last_error() {
        let a = "https://a/$repo";
        let samples = vec![
            s(a, true, 100, Some(50)),
            s(a, false, 0, None),
            s(a, true, 300, Some(60)),
            s(a, true, 200, Some(70)),
        ];
        let st = summarize(&samples).remove(a).unwrap();
        assert_eq!((st.attempts, st.successes), (4, 3));
        assert_eq!(st.median_total_ms, Some(200));
        assert_eq!(st.sync_age_s, Some(70), "the newest successful one");
        assert_eq!(st.last_error.as_deref(), Some("timed out"));
        assert!(st.last_ok);
        assert!(st.kib_per_s.unwrap() > 0);
    }

    #[test]
    fn only_the_last_ten_attempts_count_for_reliability() {
        let a = "https://a/$repo";
        let mut samples: Vec<Sample> = (0..20).map(|_| s(a, false, 0, None)).collect();
        samples.extend((0..10).map(|_| s(a, true, 100, Some(10))));
        let st = summarize(&samples).remove(a).unwrap();
        assert_eq!((st.recent_attempts, st.recent_successes), (10, 10));
        assert_eq!(group_of(Some(&st)), Group::Good, "it recovered");
    }

    #[test]
    fn each_group_is_decided_by_its_stated_rule() {
        let g = |samples: Vec<Sample>| group_of(summarize(&samples).values().next());
        let a = "https://a/$repo";
        assert_eq!(group_of(None), Group::Untested);
        assert_eq!(g(vec![s(a, true, 100, Some(10))]), Group::Good);
        assert_eq!(
            g(vec![s(a, true, 100, None)]),
            Group::Good,
            "an unknown age is not stale"
        );
        assert_eq!(
            g(vec![s(a, true, 100, Some(STALE_AFTER + 1))]),
            Group::Stale
        );
        assert_eq!(
            g(vec![s(a, true, 100, Some(STALE_AFTER))]),
            Group::Good,
            "a day exactly is not yet stale"
        );
        assert_eq!(
            g(vec![s(a, true, 100, Some(1)), s(a, false, 0, None)]),
            Group::Unreliable,
            "its last attempt failed"
        );
        let mut seven_of_ten: Vec<Sample> = (0..7).map(|_| s(a, true, 100, Some(1))).collect();
        seven_of_ten.extend((0..3).map(|_| s(a, false, 0, None)));
        seven_of_ten.rotate_left(3); // ends with successes
        assert_eq!(g(seven_of_ten), Group::Unreliable, "70% is below 80%");
        let mut eight_of_ten: Vec<Sample> = (0..8).map(|_| s(a, true, 100, Some(1))).collect();
        eight_of_ten.extend((0..2).map(|_| s(a, false, 0, None)));
        eight_of_ten.rotate_left(2);
        assert_eq!(g(eight_of_ten), Group::Good, "80% is enough");
    }

    #[test]
    fn the_order_is_good_by_speed_then_stale_then_unreliable_then_untested_in_file_order() {
        let urls: Vec<String> = ["slow", "fast", "stale", "flaky", "new1", "new2", "mid"]
            .iter()
            .map(|n| format!("https://{n}/$repo"))
            .collect();
        let u = |n: &str| format!("https://{n}/$repo");
        let samples = vec![
            s(&u("slow"), true, 900, Some(10)),
            s(&u("fast"), true, 100, Some(10)),
            s(&u("mid"), true, 300, None),
            s(&u("stale"), true, 50, Some(STALE_AFTER * 3)),
            s(&u("flaky"), true, 10, Some(1)),
            s(&u("flaky"), false, 0, None),
        ];
        let ranked = rank(&urls, &summarize(&samples));
        let names: Vec<&str> = ranked
            .iter()
            .map(|r| {
                r.url
                    .trim_start_matches("https://")
                    .trim_end_matches("/$repo")
            })
            .collect();
        assert_eq!(
            names,
            ["fast", "mid", "slow", "stale", "flaky", "new1", "new2"]
        );
        assert_eq!(ranked[0].group, Group::Good);
        assert_eq!(ranked[3].group, Group::Stale);
        assert_eq!(ranked[4].group, Group::Unreliable);
        assert_eq!(ranked[5].group, Group::Untested);
    }

    #[test]
    fn a_tie_goes_to_the_name_so_the_result_is_stable() {
        let urls = vec!["https://b/$repo".to_string(), "https://a/$repo".to_string()];
        let samples = vec![s(&urls[0], true, 100, None), s(&urls[1], true, 100, None)];
        let r = rank(&urls, &summarize(&samples));
        assert_eq!(r[0].url, "https://a/$repo");
    }

    #[test]
    fn a_rewritten_list_keeps_the_header_reorders_and_never_adds() {
        let order = vec![
            "http://c.example/arch/$repo/os/$arch".to_string(),
            "https://a.example/$repo/os/$arch".to_string(),
            "https://b.example/archlinux/$repo/os/$arch".to_string(),
        ];
        let out = render_mirrorlist(LIST, &order, None).unwrap();
        assert!(out.starts_with("##\n## Arch Linux repository mirrorlist\n##\n\n## Worldwide\n"));
        let active: Vec<_> = parse(&out)
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.url)
            .collect();
        assert_eq!(active, order);
        assert!(
            out.contains("#Server = https://dead.example/$repo/os/$arch"),
            "a commented mirror stays"
        );
        assert!(
            out.contains("#Server = ftp://ignored.example/$repo"),
            "and so does any other line"
        );
        assert!(out.contains("## Elsewhere"));
        // the same list ranked in its own order is the same list
        let same: Vec<String> = parse(LIST)
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.url)
            .collect();
        let again = render_mirrorlist(LIST, &same, None).unwrap();
        assert_eq!(parse(&again).iter().filter(|e| e.active).count(), 3);
    }

    #[test]
    fn keep_comments_out_the_rest() {
        let order: Vec<String> = parse(LIST)
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.url)
            .collect();
        let out = render_mirrorlist(LIST, &order, Some(1)).unwrap();
        assert_eq!(parse(&out).iter().filter(|e| e.active).count(), 1);
        assert!(out.contains(&format!("#Server = {}", order[2])));
        // keep 0 still keeps one: a list needs a mirror
        let out = render_mirrorlist(LIST, &order, Some(0)).unwrap();
        assert_eq!(parse(&out).iter().filter(|e| e.active).count(), 1);
    }

    #[test]
    fn a_ranking_that_adds_drops_or_repeats_a_mirror_is_refused() {
        let order: Vec<String> = parse(LIST)
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.url)
            .collect();
        let mut stranger = order.clone();
        stranger[0] = "https://evil.example/$repo/os/$arch".into();
        assert!(render_mirrorlist(LIST, &stranger, None)
            .unwrap_err()
            .to_string()
            .contains("only reorders"));
        assert!(
            render_mirrorlist(LIST, &order[..2], None).is_err(),
            "one dropped"
        );
        let mut dup = order.clone();
        dup[1] = dup[0].clone();
        assert!(render_mirrorlist(LIST, &dup, None).is_err(), "one repeated");
        assert!(
            render_mirrorlist("# nothing active\n#Server = https://x/$repo\n", &[], None).is_err()
        );
    }

    #[test]
    fn the_history_is_appended_and_capped() {
        let d = std::env::temp_dir().join(format!("emrac-mirrors-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let path = d.join("mirrors.jsonl");
        for i in 0..(KEEP_SAMPLES as u64 + 5) {
            let mut x = s("https://a/$repo", true, 100, None);
            x.time = i;
            append_to(&path, &[x]).unwrap();
        }
        let all = read_from(&path);
        assert_eq!(all.len(), KEEP_SAMPLES);
        assert_eq!(all[0].time, 5);
        let _ = fs::remove_dir_all(&d);
    }

    /// A small HTTP server: `routes` maps a path to (status, body).
    fn serve(routes: Vec<(&'static str, u16, String)>, connections: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map_or((404, String::new()), |(_, st, b)| (*st, b.clone()));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_working_mirror_is_timed_and_its_freshness_read() {
        let now = 1_000_000;
        let base = serve(
            vec![
                ("/core/os/x86_64/core.db", 200, "x".repeat(5000)),
                ("/lastsync", 200, format!("{}\n", now - 7200)),
            ],
            2,
        );
        let server = format!("{base}/$repo/os/$arch");
        let m = measure(&server, "core", "x86_64", now, Duration::from_secs(5));
        assert!(m.ok, "{m:?}");
        assert_eq!(m.bytes, 5000);
        assert_eq!(m.sync_age_s, Some(7200));
        assert!(m.total_ms >= m.ttfb_ms);
        assert_eq!(m.url, server);
    }

    #[test]
    fn a_mirror_without_lastsync_is_ok_with_an_unknown_age() {
        let base = serve(vec![("/core/os/x86_64/core.db", 200, "db".into())], 2);
        let m = measure(
            &format!("{base}/$repo/os/$arch"),
            "core",
            "x86_64",
            10,
            Duration::from_secs(5),
        );
        assert!(m.ok && m.sync_age_s.is_none());
    }

    #[test]
    fn a_failing_or_closed_mirror_is_a_sample_not_an_error() {
        let base = serve(vec![("/core/os/x86_64/core.db", 500, "no".into())], 1);
        let m = measure(
            &format!("{base}/$repo/os/$arch"),
            "core",
            "x86_64",
            10,
            Duration::from_secs(5),
        );
        assert!(!m.ok && m.error.is_some() && m.bytes == 0);
        // nobody listening
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let m = measure(
            &format!("http://127.0.0.1:{port}/$repo/os/$arch"),
            "core",
            "x86_64",
            10,
            Duration::from_secs(2),
        );
        assert!(!m.ok && m.error.is_some());
        // an empty answer is not a working mirror
        let base = serve(vec![("/core/os/x86_64/core.db", 200, String::new())], 1);
        let m = measure(
            &format!("{base}/$repo/os/$arch"),
            "core",
            "x86_64",
            10,
            Duration::from_secs(5),
        );
        assert!(!m.ok && m.error.as_deref() == Some("an empty answer"));
    }
}

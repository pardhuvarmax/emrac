//! The package report card (u18): six dimensions, each a number from a stated
//! rule on a stated input, or unknown. No overall score on purpose.
//! See `docs/plan/r3/u18-package-score.md`.

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bench {
    /// Median over the package's programs, microseconds.
    pub warm_micros: u64,
    /// Largest peak memory that is above the launcher floor, KiB.
    pub peak_rss_kib: Option<u64>,
    pub programs: usize,
    /// Unix seconds of the newest record.
    pub recorded: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildFact {
    pub secs: u64,
    pub measured: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditCounts {
    pub critical: usize,
    pub high: usize,
    pub warning: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFact {
    pub score: i32,
    pub verdict: String,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub official: bool,
    pub bench: Option<Bench>,
    pub build: Option<BuildFact>,
    /// Days since the repository build (official) or the AUR page's last change.
    pub age_days: Option<u64>,
    pub out_of_date_days: Option<u64>,
    pub orphan: bool,
    pub votes: Option<u32>,
    pub popularity: Option<f64>,
    /// Repository packages that depend on it (official only).
    pub dependents: Option<usize>,
    /// Whether the repository database carries its signature (official only).
    pub signed: Option<bool>,
    pub audit: Option<AuditCounts>,
    /// Why there is no audit (offline, could not be read).
    pub audit_note: Option<String>,
    pub trust_high: usize,
    pub trust_warning: usize,
    /// Arch news items of the last 90 days that name the package.
    pub news_mentions: usize,
    pub source: Option<SourceFact>,
    /// What the Arch Security Tracker says about the version (official only).
    pub tracker: Option<TrackerFact>,
    /// Why the tracker was not checked (offline with nothing saved, no network).
    pub tracker_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackerFact {
    /// The version that was checked: the installed one, else the repository's.
    pub version: String,
    pub entries: Vec<crate::security_tracker::Entry>,
    /// Hours since the list was fetched.
    pub list_age_hours: u64,
    /// Said when the list is older than a refresh would give.
    pub stale_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rule {
    pub text: String,
    /// The points it moved the score by, when it was an adjustment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i32>,
}

/// How firm the evidence behind a dimension is: not interchangeable, and the
/// overall number says how much of it each kind contributed.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Official metadata, the tracker, a signature in the database.
    Authoritative,
    /// Measured on this machine, or emrac's own analysis of what it fetched.
    Corroborating,
    /// A proxy: age, votes, an estimate, advice from rules.
    Heuristic,
}

impl Evidence {
    pub fn word(self) -> &'static str {
        match self {
            Evidence::Authoritative => "authoritative",
            Evidence::Corroborating => "corroborating",
            Evidence::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Dimension {
    pub key: &'static str,
    pub title: &'static str,
    /// 0 to 100, or unknown.
    pub score: Option<u8>,
    pub summary: String,
    pub rules: Vec<Rule>,
    pub evidence: Evidence,
    /// What to run to make an unknown dimension known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// How much each dimension counts in the overall number: 0 leaves it out, 1 is
/// the default for every one, 10 is the most.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Weights {
    pub performance: u8,
    pub build_time: u8,
    pub maintenance: u8,
    pub security: u8,
    pub popularity: u8,
    pub source: u8,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            performance: 1,
            build_time: 1,
            maintenance: 1,
            security: 1,
            popularity: 1,
            source: 1,
        }
    }
}

pub const MAX_WEIGHT: u8 = 10;
pub const KEYS: [&str; 6] = [
    "performance",
    "build_time",
    "maintenance",
    "security",
    "popularity",
    "source",
];

/// A dimension's key from what a person might type.
pub fn normalize_key(word: &str) -> Option<&'static str> {
    match word.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "performance" | "perf" => Some("performance"),
        "build_time" | "build" | "buildtime" => Some("build_time"),
        "maintenance" | "maintained" => Some("maintenance"),
        "security" | "secure" => Some("security"),
        "popularity" | "popular" => Some("popularity"),
        "source" | "source_worthiness" => Some("source"),
        _ => None,
    }
}

impl Weights {
    pub fn get(&self, key: &str) -> u8 {
        match key {
            "performance" => self.performance,
            "build_time" => self.build_time,
            "maintenance" => self.maintenance,
            "security" => self.security,
            "popularity" => self.popularity,
            _ => self.source,
        }
    }

    pub fn set(&mut self, key: &str, value: u8) {
        match key {
            "performance" => self.performance = value,
            "build_time" => self.build_time = value,
            "maintenance" => self.maintenance = value,
            "security" => self.security = value,
            "popularity" => self.popularity = value,
            _ => self.source = value,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Weights::default()
    }

    /// `security=5,popularity=0` on top of `self`.
    pub fn with_list(mut self, text: &str) -> std::result::Result<Weights, String> {
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| format!("'{part}' is not name=number, like security=5"))?;
            let key = normalize_key(k).ok_or_else(|| {
                format!(
                    "'{}' is not a dimension; they are {}",
                    k.trim(),
                    KEYS.join(", ")
                )
            })?;
            let n: u8 = v
                .trim()
                .parse()
                .ok()
                .filter(|n| *n <= MAX_WEIGHT)
                .ok_or_else(|| format!("'{}' is not a weight from 0 to {MAX_WEIGHT}", v.trim()))?;
            self.set(key, n);
        }
        Ok(self)
    }

    /// The named dimensions count three times as much as the rest.
    pub fn caring_about(mut self, text: &str) -> std::result::Result<Weights, String> {
        for word in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let key = normalize_key(word).ok_or_else(|| {
                format!("'{word}' is not a dimension; they are {}", KEYS.join(", "))
            })?;
            self.set(key, 3);
        }
        Ok(self)
    }

    /// `security 5, popularity 0` for what differs from the default.
    pub fn describe_changes(&self) -> String {
        KEYS.iter()
            .filter(|k| self.get(k) != 1)
            .map(|k| format!("{} {}", k.replace('_', " "), self.get(k)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A hard problem that is shown whatever the numbers average to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Gate {
    pub name: &'static str,
    pub text: String,
}

/// How many of the dimensions in the overall number rest on each kind of evidence.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct EvidenceMix {
    pub authoritative: usize,
    pub corroborating: usize,
    pub heuristic: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LeftOut {
    pub key: &'static str,
    pub title: &'static str,
    pub why: &'static str,
}

/// The weighted mean of the dimensions that are known and have a weight.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Overall {
    pub score: Option<u8>,
    /// Dimensions that went into it.
    pub used: usize,
    /// Dimensions that have a weight (0 leaves one out).
    pub of: usize,
    /// The share of the weight that is known, in percent.
    pub coverage_percent: u8,
    pub weights: Weights,
    pub left_out: Vec<LeftOut>,
    pub evidence: EvidenceMix,
}

pub fn overall(dimensions: &[Dimension], w: &Weights) -> Overall {
    let (mut num, mut den, mut known_w, mut all_w) = (0u32, 0u32, 0u32, 0u32);
    let (mut used, mut of) = (0, 0);
    let mut left_out = Vec::new();
    let mut mix = EvidenceMix::default();
    for d in dimensions {
        let weight = u32::from(w.get(d.key));
        if weight == 0 {
            left_out.push(LeftOut {
                key: d.key,
                title: d.title,
                why: "its weight is 0",
            });
            continue;
        }
        of += 1;
        all_w += weight;
        match d.score {
            Some(s) => {
                match d.evidence {
                    Evidence::Authoritative => mix.authoritative += 1,
                    Evidence::Corroborating => mix.corroborating += 1,
                    Evidence::Heuristic => mix.heuristic += 1,
                }
                used += 1;
                known_w += weight;
                num += weight * u32::from(s);
                den += weight;
            }
            None => left_out.push(LeftOut {
                key: d.key,
                title: d.title,
                why: "unknown",
            }),
        }
    }
    Overall {
        score: (den > 0).then(|| ((2 * num + den) / (2 * den)) as u8),
        used,
        of,
        coverage_percent: (100 * known_w + all_w / 2).checked_div(all_w).unwrap_or(0) as u8,
        weights: *w,
        left_out,
        evidence: mix,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Card {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub dimensions: Vec<Dimension>,
    pub overall: Overall,
    /// Hard problems, shown whatever the numbers average to.
    pub gates: Vec<Gate>,
}

impl Card {
    pub fn known(&self) -> usize {
        self.dimensions.iter().filter(|d| d.score.is_some()).count()
    }
}

fn rule(text: impl Into<String>, points: Option<i32>) -> Rule {
    Rule {
        text: text.into(),
        points,
    }
}

/// The score of the first limit that `value` does not exceed, else `last`.
fn band(value: f64, limits: &[(f64, u8)], last: u8) -> u8 {
    limits
        .iter()
        .find(|(limit, _)| value <= *limit)
        .map_or(last, |(_, score)| *score)
}

fn clamp(points: i32) -> u8 {
    points.clamp(0, 100) as u8
}

fn duration(secs: u64) -> String {
    if secs < 90 {
        format!("{secs} s")
    } else if secs < 5400 {
        format!("{} min", (secs + 30) / 60)
    } else {
        format!("{:.1} h", secs as f64 / 3600.0)
    }
}

fn millis(micros: u64) -> String {
    if micros < 1_000 {
        format!("{micros} \u{b5}s")
    } else if micros < 1_000_000 {
        format!("{:.1} ms", micros as f64 / 1_000.0)
    } else {
        format!("{:.2} s", micros as f64 / 1_000_000.0)
    }
}

fn unknown(
    key: &'static str,
    title: &'static str,
    summary: impl Into<String>,
    hint: Option<String>,
) -> Dimension {
    Dimension {
        key,
        title,
        score: None,
        summary: summary.into(),
        rules: Vec::new(),
        evidence: Evidence::Heuristic,
        hint,
    }
}

// ---------------------------------------------------------------- dimensions

pub const PERFORMANCE_LIMITS: &[(f64, u8)] = &[
    (10_000.0, 100),
    (50_000.0, 85),
    (200_000.0, 65),
    (1_000_000.0, 40),
];
const MIB256_KIB: u64 = 256 * 1024;
const GIB_KIB: u64 = 1024 * 1024;

fn performance(f: &Facts) -> Dimension {
    let Some(b) = &f.bench else {
        return unknown(
            "performance",
            "Performance",
            "not measured: only an installed package that was benchmarked has one",
            Some(format!("emrac benchmark {}", f.name)),
        );
    };
    let base = band(b.warm_micros as f64, PERFORMANCE_LIMITS, 15);
    let mut rules = vec![rule(
        format!(
            "median warm start {}: up to 10 ms scores 100, 50 ms 85, 200 ms 65, 1 s 40, slower 15 (this: {base})",
            millis(b.warm_micros)
        ),
        None,
    )];
    let mut points = i32::from(base);
    match b.peak_rss_kib {
        Some(k) if k > GIB_KIB => {
            points -= 25;
            rules.push(rule(
                format!(
                    "peak memory {:.1} GiB is over 1 GiB",
                    k as f64 / GIB_KIB as f64
                ),
                Some(-25),
            ));
        }
        Some(k) if k > MIB256_KIB => {
            points -= 10;
            rules.push(rule(
                format!("peak memory {:.0} MiB is over 256 MiB", k as f64 / 1024.0),
                Some(-10),
            ));
        }
        _ => {}
    }
    Dimension {
        key: "performance",
        title: "Performance",
        score: Some(clamp(points)),
        summary: format!(
            "starts in {} (warm), {} program{}, benchmarked {}",
            millis(b.warm_micros),
            b.programs,
            if b.programs == 1 { "" } else { "s" },
            crate::history::format_utc_date(b.recorded)
        ),
        rules,
        hint: None,
        evidence: Evidence::Heuristic,
    }
}

pub const BUILD_LIMITS: &[(f64, u8)] = &[
    (60.0, 100),
    (300.0, 85),
    (900.0, 65),
    (3600.0, 40),
    (10800.0, 20),
];

fn build_time(f: &Facts) -> Dimension {
    let Some(b) = &f.build else {
        return unknown(
            "build_time",
            "Build time",
            "no build time is known and none can be estimated",
            None,
        );
    };
    let score = band(b.secs as f64, BUILD_LIMITS, 5);
    Dimension {
        key: "build_time",
        title: "Build time",
        score: Some(score),
        summary: format!(
            "about {} on this machine ({})",
            duration(b.secs),
            if b.measured { "measured: your own builds" } else { "estimated" }
        ),
        rules: vec![rule(
            format!(
                "{}: up to 1 min scores 100, 5 min 85, 15 min 65, 1 h 40, 3 h 20, longer 5 (this: {score})",
                duration(b.secs)
            ),
            None,
        )],
        hint: None,
        evidence: Evidence::Heuristic,
    }
}

pub const AGE_LIMITS: &[(f64, u8)] = &[
    (30.0, 100),
    (90.0, 90),
    (180.0, 75),
    (365.0, 55),
    (730.0, 30),
];

fn maintenance(f: &Facts) -> Dimension {
    let Some(days) = f.age_days else {
        return unknown("maintenance", "Maintenance", "its age is not known", None);
    };
    let base = band(days as f64, AGE_LIMITS, 15);
    let what = if f.official {
        "rebuilt in the repository"
    } else {
        "last changed on the AUR"
    };
    let mut points = i32::from(base);
    let mut rules = vec![rule(
        format!("{what} {days} days ago: up to 30 days scores 100, 90 days 90, 180 days 75, a year 55, two years 30, older 15 (this: {base})"),
        None,
    )];
    let mut extra = String::new();
    if let Some(d) = f.out_of_date_days {
        points -= 30;
        rules.push(rule(format!("flagged out of date {d} days ago"), Some(-30)));
        extra.push_str(&format!(", flagged out of date {d} days ago"));
    }
    if !f.official && f.orphan {
        points = points.min(40);
        rules.push(rule("it has no maintainer (orphaned): at most 40", None));
        extra.push_str(", no maintainer");
    }
    Dimension {
        key: "maintenance",
        title: "Maintenance",
        score: Some(clamp(points)),
        summary: format!("{what} {days} days ago{extra}"),
        rules,
        hint: None,
        evidence: Evidence::Heuristic,
    }
}

fn security(f: &Facts) -> Dimension {
    if f.official {
        let Some(signed) = f.signed else {
            return unknown(
                "security",
                "Security",
                "the repository database could not be read",
                None,
            );
        };
        let mut points = if signed { 95 } else { 50 };
        let mut rules = vec![rule(
            if signed {
                "an official repository package whose signature the database carries: 95"
            } else {
                "an official repository package whose database has no signature for it: 50"
            },
            None,
        )];
        let mut summary = if signed {
            "official package, signed by an Arch packager".to_string()
        } else {
            "official package with no signature in the database".to_string()
        };
        if f.news_mentions > 0 {
            let cut = (15 * f.news_mentions as i32).min(30);
            points -= cut;
            rules.push(rule(
                format!(
                    "{} Arch news item(s) of the last 90 days name it",
                    f.news_mentions
                ),
                Some(-cut),
            ));
            summary.push_str(&format!(
                "; named in {} recent Arch news item(s)",
                f.news_mentions
            ));
        }
        tracker_rules(f, &mut points, &mut rules, &mut summary);
        return Dimension {
            key: "security",
            title: "Security",
            score: Some(clamp(points)),
            summary,
            rules,
            hint: None,
            evidence: Evidence::Heuristic,
        };
    }
    let mut points: i32 = 80;
    let mut rules = vec![rule(
        "an AUR package: community-uploaded and not reviewed by Arch: starts at 80",
        None,
    )];
    let mut parts: Vec<String> = Vec::new();
    match &f.audit {
        Some(a) => {
            if a.critical > 0 {
                points = 0;
                rules.push(rule(
                    format!("{} critical audit finding(s): 0", a.critical),
                    None,
                ));
                parts.push(format!("{} critical audit finding(s)", a.critical));
            } else {
                let high = 30 * a.high as i32;
                let warn = (10 * a.warning as i32).min(30);
                if a.high > 0 {
                    points -= high;
                    rules.push(rule(
                        format!("{} high audit finding(s), 30 each", a.high),
                        Some(-high),
                    ));
                    parts.push(format!("{} high audit finding(s)", a.high));
                }
                if a.warning > 0 {
                    points -= warn;
                    rules.push(rule(
                        format!("{} audit warning(s), 10 each, at most 30", a.warning),
                        Some(-warn),
                    ));
                    parts.push(format!("{} audit warning(s)", a.warning));
                }
                if a.high == 0 && a.warning == 0 {
                    parts.push("the PKGBUILD audit found nothing".to_string());
                }
            }
        }
        None => {
            let why = f
                .audit_note
                .clone()
                .unwrap_or_else(|| "not audited".to_string());
            rules.push(rule(format!("the PKGBUILD was not audited ({why})"), None));
            parts.push(format!("not audited ({why})"));
        }
    }
    if f.trust_high > 0 {
        let cut = 25 * f.trust_high as i32;
        points -= cut;
        rules.push(rule(
            format!("{} high trust signal(s), 25 each", f.trust_high),
            Some(-cut),
        ));
        parts.push(format!("{} high trust signal(s)", f.trust_high));
    }
    if f.trust_warning > 0 {
        let cut = 10 * f.trust_warning as i32;
        points -= cut;
        rules.push(rule(
            format!("{} trust warning(s), 10 each", f.trust_warning),
            Some(-cut),
        ));
        parts.push(format!("{} trust warning(s)", f.trust_warning));
    }
    rules.push(rule(
        "the Arch Security Tracker covers official packages only, so no CVE data was looked at; that is not a clean bill",
        None,
    ));
    Dimension {
        key: "security",
        title: "Security",
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        hint: None,
        evidence: Evidence::Heuristic,
    }
}

/// The most the tracker alone can take off a security score.
pub const TRACKER_CAP: i32 = 60;

/// What the Arch Security Tracker adds to an official package's security.
fn tracker_rules(f: &Facts, points: &mut i32, rules: &mut Vec<Rule>, summary: &mut String) {
    use crate::security_tracker::Standing;
    let Some(t) = &f.tracker else {
        let why = f
            .tracker_note
            .clone()
            .unwrap_or_else(|| "no reason given".to_string());
        rules.push(rule(
            format!("the Arch Security Tracker was not checked ({why}); that is not a clean bill"),
            None,
        ));
        summary.push_str("; the Arch Security Tracker was not checked");
        return;
    };
    if let Some(note) = &t.stale_note {
        rules.push(rule(
            format!("the tracker list is {} h old: {note}", t.list_age_hours),
            None,
        ));
    }
    if t.entries.is_empty() {
        rules.push(rule(
            format!(
                "the Arch Security Tracker (list {} h old) has no open entry for {}; it lists only what its maintainers recorded, so that is not a clean bill",
                t.list_age_hours, t.version
            ),
            None,
        ));
        summary.push_str(&format!(
            "; no open Arch Security Tracker entry for {}",
            t.version
        ));
        return;
    }
    let mut total = 0;
    for e in &t.entries {
        let pts = e.points();
        total += pts;
        let cves = match e.found.cves.len() {
            0 => "no CVE listed".to_string(),
            1..=3 => e.found.cves.join(", "),
            n => format!("{}, and {} more", e.found.cves[..3].join(", "), n - 3),
        };
        let what = match e.standing {
            Standing::PossiblyStale => format!("{}, counts half", e.standing.word()),
            Standing::Ignored => format!("{}, counts nothing", e.standing.word()),
            other => other.word().to_string(),
        };
        rules.push(rule(
            format!(
                "{} ({}, affected from {}; {what}): {cves}; {}",
                e.found.id,
                e.found.severity.word(),
                e.found.affected,
                e.why
            ),
            (pts != 0).then_some(-pts),
        ));
    }
    let cut = total.min(TRACKER_CAP);
    if total > TRACKER_CAP {
        rules.push(rule(
            format!("the tracker takes off at most {TRACKER_CAP} in all ({total} listed)"),
            Some(total - cut),
        ));
    }
    *points -= cut;
    let confirmed = t.entries.iter().filter(|e| e.standing.confirmed()).count();
    let stale = t
        .entries
        .iter()
        .filter(|e| e.standing == Standing::PossiblyStale)
        .count();
    let ignored = t
        .entries
        .iter()
        .filter(|e| e.standing == Standing::Ignored)
        .count();
    let top = t
        .entries
        .iter()
        .take(3)
        .map(|e| format!("{} {}", e.found.id, e.found.severity.word()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut counts = format!("{confirmed} open");
    if stale > 0 {
        counts.push_str(&format!(", {stale} possibly stale"));
    }
    if ignored > 0 {
        counts.push_str(&format!(", {ignored} ignored by you"));
    }
    summary.push_str(&format!(
        "; Arch Security Tracker entries for {} ({counts}): {top}{}",
        t.version,
        if t.entries.len() > 3 { ", ..." } else { "" }
    ));
}

pub const VOTE_LIMITS: &[(f64, u8)] = &[(0.0, 10), (9.0, 30), (49.0, 50), (199.0, 70), (999.0, 85)];
pub const POPULARITY_LIMITS: &[(f64, u8)] =
    &[(0.0099, 20), (0.0999, 40), (0.9999, 60), (4.9999, 80)];

fn popularity(f: &Facts) -> Dimension {
    if f.official {
        let n = f
            .dependents
            .map_or(String::new(), |d| format!(" ({d} packages depend on it)"));
        return unknown(
            "popularity",
            "Popularity",
            format!("the official repositories keep no popularity data{n}"),
            None,
        );
    }
    let (Some(votes), Some(pop)) = (f.votes, f.popularity) else {
        return unknown(
            "popularity",
            "Popularity",
            "the AUR gave no votes or popularity",
            None,
        );
    };
    let by_votes = band(f64::from(votes), VOTE_LIMITS, 100);
    let by_pop = band(pop, POPULARITY_LIMITS, 100);
    let score = (u16::from(by_votes) + u16::from(by_pop)).div_ceil(2) as u8;
    Dimension {
        key: "popularity",
        title: "Popularity",
        score: Some(score),
        summary: format!("{votes} votes, popularity {pop:.2}"),
        rules: vec![
            rule(format!("{votes} votes: none scores 10, under 10 30, 50 50, 200 70, 1000 85, more 100 (this: {by_votes})"), None),
            rule(format!("popularity {pop:.2}: under 0.01 scores 20, 0.1 40, 1 60, 5 80, more 100 (this: {by_pop})"), None),
            rule(format!("the average of the two: {score}"), None),
        ],
        hint: None,
        evidence: Evidence::Heuristic,
    }
}

fn source_worthiness(f: &Facts) -> Dimension {
    match &f.source {
        Some(s) => Dimension {
            key: "source",
            title: "Source",
            score: Some(clamp(s.score)),
            summary: format!(
                "the source or binary advice is \"{}\" (`emrac explain {}` shows why)",
                s.verdict, f.name
            ),
            rules: vec![rule(format!("the Source Suitability Score of `emrac recommend`, as it is: {}", s.score), None)],
            hint: None,
            evidence: Evidence::Heuristic,
        },
        None => unknown(
            "source",
            "Source",
            "there is no choice to score: no binary to compare a source build with, or nothing to compile",
            Some(format!("emrac recommend {}", f.name)),
        ),
    }
}

/// The report card of a package with every dimension counting the same.
pub fn card(f: &Facts) -> Card {
    card_with(f, &Weights::default())
}

/// The report card, its overall number weighted as asked.
pub fn card_with(f: &Facts, w: &Weights) -> Card {
    let mut dimensions = vec![
        performance(f),
        build_time(f),
        maintenance(f),
        security(f),
        popularity(f),
        source_worthiness(f),
    ];
    for d in &mut dimensions {
        d.evidence = match d.key {
            "performance" => Evidence::Corroborating,
            "build_time" if f.build.as_ref().is_some_and(|b| b.measured) => Evidence::Corroborating,
            "security" if f.official => Evidence::Authoritative,
            "security" => Evidence::Corroborating,
            _ => Evidence::Heuristic,
        };
    }
    Card {
        name: f.name.clone(),
        version: f.version.clone(),
        repo: f.repo.clone(),
        overall: overall(&dimensions, w),
        gates: gates(f),
        dimensions,
    }
}

/// The problems that are shown whatever the average is: a confirmed High or
/// Critical Arch Security Tracker entry that applies to the version, or a
/// critical finding in an AUR package's PKGBUILD. Entries that are possibly
/// stale or that you ignored never open a gate.
pub fn gates(f: &Facts) -> Vec<Gate> {
    use crate::security_tracker::Severity;
    let mut out = Vec::new();
    if let Some(t) = &f.tracker {
        let hard: Vec<&crate::security_tracker::Entry> = t
            .entries
            .iter()
            .filter(|e| e.standing.confirmed() && e.found.severity >= Severity::High)
            .collect();
        if !hard.is_empty() {
            let list = hard
                .iter()
                .map(|e| format!("{} {}", e.found.id, e.found.severity.word()))
                .collect::<Vec<_>>()
                .join(", ");
            let fixes = hard.iter().filter(|e| e.found.fixed.is_some()).count();
            out.push(Gate {
                name: "security",
                text: format!(
                    "{} confirmed High or Critical Arch Security Tracker {} to {}: {list}{}",
                    hard.len(),
                    if hard.len() == 1 {
                        "entry applies"
                    } else {
                        "entries apply"
                    },
                    t.version,
                    if fixes > 0 {
                        format!("; {fixes} with a fix available")
                    } else {
                        String::new()
                    }
                ),
            });
        }
    }
    if f.audit.as_ref().is_some_and(|a| a.critical > 0) {
        out.push(Gate {
            name: "pkgbuild",
            text: "the PKGBUILD audit has a critical finding".to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn official() -> Facts {
        Facts {
            name: "neovim".into(),
            version: "0.10.2-1".into(),
            repo: "extra".into(),
            official: true,
            signed: Some(true),
            age_days: Some(6),
            dependents: Some(212),
            ..Facts::default()
        }
    }

    fn aur() -> Facts {
        Facts {
            name: "thing".into(),
            version: "1-1".into(),
            repo: "aur".into(),
            official: false,
            age_days: Some(10),
            votes: Some(120),
            popularity: Some(0.5),
            audit: Some(AuditCounts::default()),
            ..Facts::default()
        }
    }

    fn dim<'a>(c: &'a Card, key: &str) -> &'a Dimension {
        c.dimensions.iter().find(|d| d.key == key).unwrap()
    }

    fn score_of(f: &Facts, key: &str) -> Option<u8> {
        dim(&card(f), key).score
    }

    #[test]
    fn there_are_six_dimensions_in_the_specs_order() {
        let c = card(&official());
        let keys: Vec<&str> = c.dimensions.iter().map(|d| d.key).collect();
        assert_eq!(
            keys,
            [
                "performance",
                "build_time",
                "maintenance",
                "security",
                "popularity",
                "source"
            ]
        );
        assert_eq!(keys, KEYS);
    }

    #[test]
    fn what_has_no_data_is_unknown_not_a_number() {
        let c = card(&official());
        assert_eq!(dim(&c, "performance").score, None);
        assert_eq!(
            dim(&c, "performance").hint.as_deref(),
            Some("emrac benchmark neovim")
        );
        assert_eq!(dim(&c, "build_time").score, None);
        assert_eq!(dim(&c, "popularity").score, None);
        assert!(dim(&c, "popularity")
            .summary
            .contains("212 packages depend on it"));
        assert_eq!(dim(&c, "source").score, None);
        assert_eq!(c.known(), 2, "maintenance and security");
    }

    #[test]
    fn performance_bands_have_inclusive_edges_and_memory_takes_points_off() {
        let mut f = official();
        let at = |micros: u64, rss: Option<u64>| Facts {
            bench: Some(Bench {
                warm_micros: micros,
                peak_rss_kib: rss,
                programs: 1,
                recorded: 0,
            }),
            ..f.clone()
        };
        for (micros, want) in [
            (1, 100),
            (10_000, 100),
            (10_001, 85),
            (50_000, 85),
            (50_001, 65),
            (200_000, 65),
            (200_001, 40),
            (1_000_000, 40),
            (1_000_001, 15),
        ] {
            assert_eq!(
                score_of(&at(micros, None), "performance"),
                Some(want),
                "{micros} us"
            );
        }
        assert_eq!(
            score_of(&at(5_000, Some(300 * 1024)), "performance"),
            Some(90)
        );
        assert_eq!(
            score_of(&at(5_000, Some(2 * 1024 * 1024)), "performance"),
            Some(75)
        );
        assert_eq!(
            score_of(&at(5_000, Some(256 * 1024)), "performance"),
            Some(100),
            "256 MiB exactly is not over"
        );
        assert_eq!(
            score_of(&at(2_000_000, Some(4 * 1024 * 1024)), "performance"),
            Some(0),
            "15 - 25 stays at 0"
        );
        f.bench = None;
        assert_eq!(score_of(&f, "performance"), None);
    }

    #[test]
    fn build_time_bands() {
        let at = |secs: u64| Facts {
            build: Some(BuildFact {
                secs,
                measured: true,
            }),
            ..official()
        };
        for (secs, want) in [
            (10, 100),
            (60, 100),
            (61, 85),
            (300, 85),
            (301, 65),
            (900, 65),
            (901, 40),
            (3600, 40),
            (3601, 20),
            (10800, 20),
            (10801, 5),
        ] {
            assert_eq!(score_of(&at(secs), "build_time"), Some(want), "{secs} s");
        }
        assert!(dim(&card(&at(120)), "build_time")
            .summary
            .contains("measured: your own builds"));
        let est = Facts {
            build: Some(BuildFact {
                secs: 1700,
                measured: false,
            }),
            ..official()
        };
        assert!(
            dim(&card(&est), "build_time").summary.contains("28 min")
                && dim(&card(&est), "build_time").summary.contains("estimated")
        );
    }

    #[test]
    fn maintenance_bands_and_the_aur_penalties() {
        let at = |days: u64| Facts {
            age_days: Some(days),
            ..official()
        };
        for (days, want) in [
            (0, 100),
            (30, 100),
            (31, 90),
            (90, 90),
            (91, 75),
            (180, 75),
            (181, 55),
            (365, 55),
            (366, 30),
            (730, 30),
            (731, 15),
        ] {
            assert_eq!(
                score_of(&at(days), "maintenance"),
                Some(want),
                "{days} days"
            );
        }
        let flagged = Facts {
            out_of_date_days: Some(20),
            ..aur()
        };
        assert_eq!(score_of(&flagged, "maintenance"), Some(70), "100 - 30");
        let orphan = Facts {
            orphan: true,
            ..aur()
        };
        assert_eq!(score_of(&orphan, "maintenance"), Some(40), "capped at 40");
        let both = Facts {
            orphan: true,
            out_of_date_days: Some(5),
            age_days: Some(500),
            ..aur()
        };
        assert_eq!(score_of(&both, "maintenance"), Some(0), "30 - 30");
        // an orphan flag on an official package means nothing
        assert_eq!(
            score_of(
                &Facts {
                    orphan: true,
                    ..official()
                },
                "maintenance"
            ),
            Some(100)
        );
        assert_eq!(
            score_of(
                &Facts {
                    age_days: None,
                    ..official()
                },
                "maintenance"
            ),
            None
        );
        assert!(dim(&card(&official()), "maintenance")
            .summary
            .starts_with("rebuilt in the repository 6 days ago"));
        assert!(dim(&card(&aur()), "maintenance")
            .summary
            .starts_with("last changed on the AUR 10 days ago"));
    }

    #[test]
    fn official_security_is_signature_and_news() {
        assert_eq!(score_of(&official(), "security"), Some(95));
        assert_eq!(
            score_of(
                &Facts {
                    signed: Some(false),
                    ..official()
                },
                "security"
            ),
            Some(50)
        );
        assert_eq!(
            score_of(
                &Facts {
                    news_mentions: 1,
                    ..official()
                },
                "security"
            ),
            Some(80)
        );
        assert_eq!(
            score_of(
                &Facts {
                    news_mentions: 2,
                    ..official()
                },
                "security"
            ),
            Some(65)
        );
        assert_eq!(
            score_of(
                &Facts {
                    news_mentions: 9,
                    ..official()
                },
                "security"
            ),
            Some(65),
            "at most 30 off"
        );
        assert_eq!(
            score_of(
                &Facts {
                    signed: None,
                    ..official()
                },
                "security"
            ),
            None
        );
        let c = card(&official());
        let r = &dim(&c, "security").rules;
        assert!(
            r.iter().any(|x| x.text.contains("not a clean bill")),
            "it never claims a clean bill"
        );
    }

    #[test]
    fn aur_security_starts_at_80_and_the_audit_and_trust_signals_take_off() {
        assert_eq!(score_of(&aur(), "security"), Some(80));
        let high = Facts {
            audit: Some(AuditCounts {
                high: 1,
                ..AuditCounts::default()
            }),
            ..aur()
        };
        assert_eq!(score_of(&high, "security"), Some(50));
        let two_high = Facts {
            audit: Some(AuditCounts {
                high: 2,
                ..AuditCounts::default()
            }),
            ..aur()
        };
        assert_eq!(score_of(&two_high, "security"), Some(20));
        let warns = Facts {
            audit: Some(AuditCounts {
                warning: 5,
                ..AuditCounts::default()
            }),
            ..aur()
        };
        assert_eq!(
            score_of(&warns, "security"),
            Some(50),
            "warnings take at most 30"
        );
        let critical = Facts {
            audit: Some(AuditCounts {
                critical: 1,
                high: 3,
                warning: 3,
            }),
            ..aur()
        };
        assert_eq!(score_of(&critical, "security"), Some(0));
        let trust = Facts {
            trust_high: 1,
            trust_warning: 2,
            ..aur()
        };
        assert_eq!(score_of(&trust, "security"), Some(35), "80 - 25 - 20");
        let low = Facts {
            trust_high: 5,
            ..aur()
        };
        assert_eq!(score_of(&low, "security"), Some(0), "never below 0");
    }

    #[test]
    fn an_unaudited_aur_package_says_so_instead_of_pretending() {
        let f = Facts {
            audit: None,
            audit_note: Some("--offline is set".into()),
            ..aur()
        };
        let d = dim(&card(&f), "security").clone();
        assert_eq!(d.score, Some(80));
        assert!(
            d.summary.contains("not audited (--offline is set)"),
            "{}",
            d.summary
        );
    }

    #[test]
    fn popularity_is_the_average_of_votes_and_popularity_for_the_aur_only() {
        let at = |votes: u32, pop: f64| Facts {
            votes: Some(votes),
            popularity: Some(pop),
            ..aur()
        };
        assert_eq!(
            score_of(&at(0, 0.0), "popularity"),
            Some(15),
            "(10 + 20) / 2"
        );
        assert_eq!(
            score_of(&at(120, 0.5), "popularity"),
            Some(65),
            "(70 + 60) / 2"
        );
        assert_eq!(score_of(&at(5000, 20.0), "popularity"), Some(100));
        assert_eq!(
            score_of(&at(9, 0.0099), "popularity"),
            Some(25),
            "(30 + 20) / 2"
        );
        assert_eq!(
            score_of(&at(10, 0.1), "popularity"),
            Some(55),
            "(50 + 60) / 2"
        );
        assert_eq!(
            score_of(
                &Facts {
                    votes: None,
                    ..aur()
                },
                "popularity"
            ),
            None
        );
        assert_eq!(
            score_of(&official(), "popularity"),
            None,
            "the repositories have no vote data"
        );
    }

    #[test]
    fn source_worthiness_is_the_recommend_score_as_it_is() {
        let f = Facts {
            source: Some(SourceFact {
                score: 22,
                verdict: "binary".into(),
            }),
            ..official()
        };
        let d = dim(&card(&f), "source").clone();
        assert_eq!(d.score, Some(22));
        assert!(d.summary.contains("\"binary\"") && d.summary.contains("emrac explain neovim"));
        let off = Facts {
            source: Some(SourceFact {
                score: 140,
                verdict: "source".into(),
            }),
            ..official()
        };
        assert_eq!(score_of(&off, "source"), Some(100), "kept within 0 to 100");
        assert_eq!(
            dim(&card(&official()), "source").hint.as_deref(),
            Some("emrac recommend neovim")
        );
    }

    #[test]
    fn a_full_card_and_its_json() {
        let f = Facts {
            bench: Some(Bench {
                warm_micros: 4300,
                peak_rss_kib: None,
                programs: 2,
                recorded: 1_790_000_000,
            }),
            build: Some(BuildFact {
                secs: 1700,
                measured: false,
            }),
            source: Some(SourceFact {
                score: 22,
                verdict: "binary".into(),
            }),
            ..official()
        };
        let c = card(&f);
        assert_eq!(c.known(), 5);
        assert!(dim(&c, "performance")
            .summary
            .starts_with("starts in 4.3 ms (warm), 2 programs, benchmarked 2026-"));
        let v: serde_json::Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["dimensions"].as_array().unwrap().len(), 6);
        assert_eq!(v["dimensions"][0]["score"], 100);
        assert!(v["dimensions"][4]["score"].is_null());
        for d in &c.dimensions {
            if let Some(s) = d.score {
                assert!(s <= 100);
            }
        }
    }

    fn full() -> Facts {
        Facts {
            bench: Some(Bench {
                warm_micros: 4300,
                peak_rss_kib: None,
                programs: 1,
                recorded: 1,
            }),
            build: Some(BuildFact {
                secs: 240,
                measured: true,
            }),
            source: Some(SourceFact {
                score: 20,
                verdict: "binary".into(),
            }),
            ..official()
        }
    }

    #[test]
    fn the_overall_is_the_mean_of_the_known_dimensions_rounded_and_says_what_it_covers() {
        // performance 100, build 85, maintenance 100, security 95, popularity unknown, source 20
        let c = card(&full());
        let o = &c.overall;
        assert_eq!(o.score, Some(80), "(100 + 85 + 100 + 95 + 20) / 5 = 80");
        assert_eq!((o.used, o.of), (5, 6));
        assert_eq!(o.coverage_percent, 83, "5 of 6 equal weights");
        assert_eq!(o.left_out.len(), 1);
        assert_eq!(
            (o.left_out[0].key, o.left_out[0].why),
            ("popularity", "unknown")
        );
        assert!(o.weights.is_default());
    }

    #[test]
    fn the_overall_rounds_half_up_and_never_leaves_0_to_100() {
        let mk = |a: u8, b: u8| {
            let dims = vec![
                Dimension {
                    key: "security",
                    title: "Security",
                    score: Some(a),
                    summary: String::new(),
                    rules: vec![],
                    hint: None,
                    evidence: Evidence::Heuristic,
                },
                Dimension {
                    key: "maintenance",
                    title: "Maintenance",
                    score: Some(b),
                    summary: String::new(),
                    rules: vec![],
                    hint: None,
                    evidence: Evidence::Heuristic,
                },
            ];
            overall(&dims, &Weights::default()).score
        };
        assert_eq!(mk(50, 51), Some(51), "50.5 rounds up");
        assert_eq!(mk(50, 50), Some(50));
        assert_eq!(mk(0, 0), Some(0));
        assert_eq!(mk(100, 100), Some(100));
    }

    #[test]
    fn nothing_known_or_everything_weighted_zero_is_no_overall_number() {
        assert_eq!(
            card(&Facts {
                name: "x".into(),
                official: true,
                ..Facts::default()
            })
            .overall
            .score,
            None
        );
        let zero = Weights {
            performance: 0,
            build_time: 0,
            maintenance: 0,
            security: 0,
            popularity: 0,
            source: 0,
        };
        let c = card_with(&full(), &zero);
        assert_eq!(c.overall.score, None);
        assert_eq!(
            (c.overall.used, c.overall.of, c.overall.coverage_percent),
            (0, 0, 0)
        );
        assert_eq!(c.overall.left_out.len(), 6);
    }

    #[test]
    fn a_weight_changes_the_overall_in_the_stated_way() {
        let f = full();
        // security counts 5 times: (100 + 85 + 100 + 95*5 + 20) / 9
        let w = Weights::default().with_list("security=5").unwrap();
        let c = card_with(&f, &w);
        assert_eq!(c.overall.score, Some(87), "(100+85+100+475+20)/9 = 86.7");
        assert_eq!(
            c.overall.coverage_percent, 90,
            "9 of 10 of the weight is known (popularity, weight 1, is not)"
        );
        // a weight of 0 leaves a dimension out, known or not
        let no_source = card_with(&f, &Weights::default().with_list("source=0").unwrap());
        assert_eq!(
            no_source.overall.score,
            Some(95),
            "(100 + 85 + 100 + 95) / 4 = 95"
        );
        assert!(no_source
            .overall
            .left_out
            .iter()
            .any(|l| l.key == "source" && l.why == "its weight is 0"));
        // popularity is unknown, so its weight cannot move the number
        let pop = card_with(&f, &Weights::default().with_list("popularity=10").unwrap());
        assert_eq!(pop.overall.score, Some(80));
        assert!(
            pop.overall.coverage_percent < 50,
            "but the coverage says how little is known"
        );
    }

    #[test]
    fn weights_are_parsed_strictly_with_friendly_names() {
        let w = Weights::default()
            .with_list("security=5, popularity=0 ,build-time=2,perf=3")
            .unwrap();
        assert_eq!(
            (w.security, w.popularity, w.build_time, w.performance),
            (5, 0, 2, 3)
        );
        assert_eq!(w.maintenance, 1);
        for bad in [
            "security",
            "security=",
            "security=11",
            "security=-1",
            "security=x",
            "speed=3",
            "=3",
        ] {
            assert!(Weights::default().with_list(bad).is_err(), "{bad}");
        }
        assert!(Weights::default().with_list("").unwrap().is_default());
        let e = Weights::default().with_list("speed=3").unwrap_err();
        assert!(
            e.contains("performance, build_time, maintenance, security, popularity, source"),
            "{e}"
        );
        let c = Weights::default()
            .caring_about("security, maintenance")
            .unwrap();
        assert_eq!((c.security, c.maintenance, c.source), (3, 3, 1));
        assert!(Weights::default().caring_about("looks").is_err());
        assert_eq!(
            w.describe_changes(),
            "performance 3, build time 2, security 5, popularity 0"
        );
        assert_eq!(Weights::default().describe_changes(), "");
        for k in KEYS {
            assert_eq!(normalize_key(k), Some(k));
        }
    }

    fn open(
        id: &str,
        sev: crate::security_tracker::Severity,
        fixed: Option<&str>,
    ) -> crate::security_tracker::Match {
        crate::security_tracker::Match {
            id: id.into(),
            severity: sev,
            cves: vec![
                "CVE-2026-1".into(),
                "CVE-2026-2".into(),
                "CVE-2026-3".into(),
                "CVE-2026-4".into(),
            ],
            affected: "1.0-1".into(),
            fixed: fixed.map(str::to_string),
        }
    }

    fn entry(m: crate::security_tracker::Match) -> crate::security_tracker::Entry {
        use crate::security_tracker::{Entry, Standing};
        let (standing, why) = match &m.fixed {
            Some(f) => (
                Standing::FixKnown,
                format!("fixed in {f}, this version is older"),
            ),
            None => (
                Standing::OpenRecent,
                "no fix is recorded; its newest CVE is from 2026".to_string(),
            ),
        };
        Entry {
            found: m,
            standing,
            why,
        }
    }

    fn tracked(matches: Vec<crate::security_tracker::Match>) -> Facts {
        Facts {
            tracker: Some(TrackerFact {
                version: "2.0-1".into(),
                entries: matches.into_iter().map(entry).collect(),
                list_age_hours: 3,
                stale_note: None,
            }),
            ..official()
        }
    }

    #[test]
    fn the_tracker_takes_off_by_severity_and_says_which_entries() {
        use crate::security_tracker::Severity::*;
        assert_eq!(score_of(&tracked(vec![]), "security"), Some(95));
        assert_eq!(
            score_of(&tracked(vec![open("AVG-1", Low, None)]), "security"),
            Some(90)
        );
        assert_eq!(
            score_of(&tracked(vec![open("AVG-1", Medium, None)]), "security"),
            Some(83)
        );
        assert_eq!(
            score_of(
                &tracked(vec![open("AVG-1", High, Some("3.0-1"))]),
                "security"
            ),
            Some(70)
        );
        assert_eq!(
            score_of(&tracked(vec![open("AVG-1", Critical, None)]), "security"),
            Some(55)
        );
        let two = tracked(vec![open("AVG-1", High, None), open("AVG-2", Medium, None)]);
        assert_eq!(score_of(&two, "security"), Some(58), "95 - 25 - 12");
        let d = dim(&card(&two), "security").clone();
        assert!(
            d.summary.contains(
                "Arch Security Tracker entries for 2.0-1 (2 open): AVG-1 high, AVG-2 medium"
            ),
            "{}",
            d.summary
        );
        assert!(d.rules.iter().any(|r| r.text.contains("AVG-1 (high, affected from 1.0-1; open): CVE-2026-1, CVE-2026-2, CVE-2026-3, and 1 more; no fix is recorded") && r.points == Some(-25)), "{:?}", d.rules);
    }

    #[test]
    fn the_tracker_cannot_take_off_more_than_60_and_says_so() {
        use crate::security_tracker::Severity::*;
        let many = tracked(vec![
            open("AVG-1", Critical, None),
            open("AVG-2", Critical, None),
            open("AVG-3", High, None),
        ]);
        assert_eq!(score_of(&many, "security"), Some(35), "95 - 60");
        let d = dim(&card(&many), "security").clone();
        assert!(
            d.rules
                .iter()
                .any(|r| r.text.contains("at most 60 in all (105 listed)") && r.points == Some(45)),
            "{:?}",
            d.rules
        );
        // news and the tracker together can go lower, each with its own cap
        let both = Facts {
            news_mentions: 2,
            ..many
        };
        assert_eq!(score_of(&both, "security"), Some(5), "95 - 30 - 60");
    }

    #[test]
    fn a_fixed_entry_says_this_version_is_older_than_the_fix() {
        use crate::security_tracker::Severity::*;
        let f = tracked(vec![open("AVG-1", High, Some("3.0-1"))]);
        let d = dim(&card(&f), "security").clone();
        assert!(
            d.rules
                .iter()
                .any(|r| r.text.contains("fixed in 3.0-1, this version is older")),
            "{:?}",
            d.rules
        );
    }

    #[test]
    fn a_tracker_that_was_not_checked_is_said_not_assumed_clean() {
        let f = Facts {
            tracker: None,
            tracker_note: Some("--offline is set and no list has been saved yet".into()),
            ..official()
        };
        let d = dim(&card(&f), "security").clone();
        assert_eq!(
            d.score,
            Some(95),
            "nothing is taken off for what was not looked at"
        );
        assert!(
            d.summary
                .contains("the Arch Security Tracker was not checked"),
            "{}",
            d.summary
        );
        assert!(d.rules.iter().any(|r| r
            .text
            .contains("was not checked (--offline is set and no list has been saved yet)")));
        // a clean answer is not called a clean bill
        let clean = dim(&card(&tracked(vec![])), "security").clone();
        assert!(clean
            .rules
            .iter()
            .any(|r| r.text.contains("not a clean bill")));
        assert!(clean
            .summary
            .contains("no open Arch Security Tracker entry for 2.0-1"));
        // a stale list is said to be old
        let stale = Facts {
            tracker: Some(TrackerFact {
                version: "2-1".into(),
                entries: vec![],
                list_age_hours: 90,
                stale_note: Some("could not refresh (offline); using the saved list".into()),
            }),
            ..official()
        };
        assert!(dim(&card(&stale), "security")
            .rules
            .iter()
            .any(|r| r.text.contains("90 h old")));
    }

    #[test]
    fn the_tracker_does_not_apply_to_the_aur() {
        let d = dim(&card(&aur()), "security").clone();
        assert!(d
            .rules
            .iter()
            .any(|r| r.text.contains("covers official packages only")));
        assert!(!d.rules.iter().any(|r| r.text.contains("was not checked")));
    }

    #[test]
    fn the_overall_is_in_the_json() {
        let v = serde_json::to_value(card(&full())).unwrap();
        assert_eq!(v["overall"]["score"], 80);
        assert_eq!(v["overall"]["coverage_percent"], 83);
        assert_eq!(v["overall"]["weights"]["security"], 1);
        assert_eq!(v["overall"]["left_out"][0]["key"], "popularity");
    }

    fn stale_entry(
        id: &str,
        sev: crate::security_tracker::Severity,
    ) -> crate::security_tracker::Entry {
        use crate::security_tracker::{Entry, Standing};
        Entry {
            found: open(id, sev, None),
            standing: Standing::PossiblyStale,
            why: "no fix is recorded and its newest CVE is 4 years old".into(),
        }
    }

    fn with_entries(entries: Vec<crate::security_tracker::Entry>) -> Facts {
        Facts {
            tracker: Some(TrackerFact {
                version: "2.0-1".into(),
                entries,
                list_age_hours: 1,
                stale_note: None,
            }),
            ..official()
        }
    }

    #[test]
    fn a_possibly_stale_entry_counts_half_and_says_why() {
        use crate::security_tracker::Severity::*;
        // a High is 25; stale it is 12
        let f = with_entries(vec![stale_entry("AVG-1", High)]);
        assert_eq!(score_of(&f, "security"), Some(83), "95 - 12");
        let d = dim(&card(&f), "security").clone();
        assert!(
            d.summary.contains("(0 open, 1 possibly stale)"),
            "{}",
            d.summary
        );
        assert!(
            d.rules
                .iter()
                .any(|r| r.text.contains("possibly stale, counts half")
                    && r.text.contains("4 years old")
                    && r.points == Some(-12)),
            "{:?}",
            d.rules
        );
    }

    #[test]
    fn an_ignored_entry_counts_nothing_and_is_still_listed() {
        use crate::security_tracker::{Entry, Severity::*, Standing};
        let ignored = Entry {
            found: open("AVG-1", Critical, None),
            standing: Standing::Ignored,
            why: "you ignored it: disputed".into(),
        };
        let f = with_entries(vec![ignored]);
        assert_eq!(score_of(&f, "security"), Some(95));
        let d = dim(&card(&f), "security").clone();
        assert!(d.summary.contains("1 ignored by you"), "{}", d.summary);
        assert!(
            d.rules.iter().any(|r| r.text.contains("counts nothing")
                && r.text.contains("you ignored it: disputed")
                && r.points.is_none()),
            "{:?}",
            d.rules
        );
    }

    #[test]
    fn a_gate_opens_only_for_a_confirmed_high_or_critical_entry() {
        use crate::security_tracker::{Entry, Severity::*, Standing};
        assert!(card(&official()).gates.is_empty());
        assert!(
            card(&tracked(vec![open("AVG-1", Medium, None)]))
                .gates
                .is_empty(),
            "medium is advice, not a gate"
        );
        let g = card(&tracked(vec![open("AVG-1", High, None)])).gates;
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "security");
        assert!(g[0].text.contains("1 confirmed High or Critical Arch Security Tracker entry applies to 2.0-1: AVG-1 high"), "{}", g[0].text);
        let fixable = card(&tracked(vec![
            open("AVG-1", Critical, Some("3.0-1")),
            open("AVG-2", High, None),
        ]))
        .gates;
        assert!(
            fixable[0].text.contains("2 confirmed")
                && fixable[0].text.contains("1 with a fix available"),
            "{}",
            fixable[0].text
        );
        // a possibly stale or ignored one never opens it, however severe
        let stale = card(&with_entries(vec![stale_entry("AVG-3", Critical)])).gates;
        assert!(stale.is_empty());
        let ig = Entry {
            found: open("AVG-4", Critical, None),
            standing: Standing::Ignored,
            why: String::new(),
        };
        assert!(card(&with_entries(vec![ig])).gates.is_empty());
        // and the gate stays whatever the numbers average to
        let c = card(&Facts {
            tracker: tracked(vec![open("AVG-1", Critical, None)]).tracker,
            ..full()
        });
        assert!(c.overall.score.unwrap() >= 70, "the average is still high");
        assert_eq!(c.gates.len(), 1);
        // a critical PKGBUILD audit finding is a gate too
        let aur_bad = Facts {
            audit: Some(AuditCounts {
                critical: 1,
                high: 0,
                warning: 0,
            }),
            ..aur()
        };
        assert_eq!(card(&aur_bad).gates[0].name, "pkgbuild");
    }

    #[test]
    fn each_dimension_says_how_firm_its_evidence_is() {
        let c = card(&full());
        let e = |k: &str| dim(&c, k).evidence;
        assert_eq!(
            e("security"),
            Evidence::Authoritative,
            "official: the tracker and the signature"
        );
        assert_eq!(e("performance"), Evidence::Corroborating, "measured here");
        assert_eq!(e("maintenance"), Evidence::Heuristic);
        assert_eq!(e("popularity"), Evidence::Heuristic);
        assert_eq!(e("source"), Evidence::Heuristic);
        assert_eq!(e("build_time"), Evidence::Corroborating, "measured builds");
        let est = Facts {
            build: Some(BuildFact {
                secs: 100,
                measured: false,
            }),
            ..full()
        };
        assert_eq!(
            dim(&card(&est), "build_time").evidence,
            Evidence::Heuristic,
            "an estimate is a proxy"
        );
        assert_eq!(
            dim(&card(&aur()), "security").evidence,
            Evidence::Corroborating,
            "emrac's own analysis of a PKGBUILD"
        );
        assert_eq!(Evidence::Authoritative.word(), "authoritative");
    }

    #[test]
    fn the_overall_counts_the_evidence_it_rests_on() {
        let c = card(&full());
        // performance (corroborating), build (corroborating), maintenance (heuristic),
        // security (authoritative), source (heuristic); popularity is unknown
        let m = c.overall.evidence;
        assert_eq!((m.authoritative, m.corroborating, m.heuristic), (1, 2, 2));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["overall"]["evidence"]["heuristic"], 2);
        assert_eq!(v["dimensions"][3]["evidence"], "authoritative");
        assert!(v["gates"].as_array().unwrap().is_empty());
    }
}

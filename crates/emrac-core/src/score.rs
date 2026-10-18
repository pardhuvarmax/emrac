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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rule {
    pub text: String,
    /// The points it moved the score by, when it was an adjustment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Dimension {
    pub key: &'static str,
    pub title: &'static str,
    /// 0 to 100, or unknown.
    pub score: Option<u8>,
    pub summary: String,
    pub rules: Vec<Rule>,
    /// What to run to make an unknown dimension known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Card {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub dimensions: Vec<Dimension>,
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
        rules.push(rule("no vulnerability database is consulted (emrac keeps none), so this is not a clean bill", None));
        return Dimension {
            key: "security",
            title: "Security",
            score: Some(clamp(points)),
            summary,
            rules,
            hint: None,
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
        "no vulnerability database is consulted (emrac keeps none), so this is not a clean bill",
        None,
    ));
    Dimension {
        key: "security",
        title: "Security",
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        hint: None,
    }
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
        },
        None => unknown(
            "source",
            "Source",
            "there is no choice to score: no binary to compare a source build with, or nothing to compile",
            Some(format!("emrac recommend {}", f.name)),
        ),
    }
}

/// The report card of a package.
pub fn card(f: &Facts) -> Card {
    Card {
        name: f.name.clone(),
        version: f.version.clone(),
        repo: f.repo.clone(),
        dimensions: vec![
            performance(f),
            build_time(f),
            maintenance(f),
            security(f),
            popularity(f),
            source_worthiness(f),
        ],
    }
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
    fn there_are_six_dimensions_in_the_specs_order_and_no_overall() {
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
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("overall") && !json.contains("total"));
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
}

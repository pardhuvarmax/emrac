//! More evidence for the report card (u20): reproducibility, provenance,
//! compatibility, packaging quality and upstream health. Pure: the facts come
//! from `score_sources` (fetching and parsing) and the repositories; here they
//! become dimensions by stated rules. See `docs/plan/r3/u20-score-sources.md`.

use crate::score::{band, clamp, rule, unknown, Dimension, Evidence, Facts, Gate};

// ------------------------------------------------------------------- facts

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproStatus {
    Good,
    Bad,
    /// Still queued, or the rebuilder could not decide.
    Unknown,
}

/// What the Arch rebuilders (reproducible.archlinux.org) say about a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproFact {
    pub status: ReproStatus,
    /// The version the rebuilders checked.
    pub version: String,
    pub suite: String,
    pub built_at: String,
    pub attested: bool,
    /// Hours since the answer was fetched.
    pub age_hours: u64,
    /// Said when the answer is older than a refresh would give.
    pub stale_note: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RepoKind {
    /// core, extra, multilib and their testing and staging repositories.
    Arch,
    #[default]
    Aur,
    /// Any other repository in pacman.conf.
    ThirdParty,
}

/// The repositories Arch itself runs.
pub fn is_arch_repo(name: &str) -> bool {
    matches!(
        name,
        "core"
            | "extra"
            | "multilib"
            | "core-testing"
            | "extra-testing"
            | "multilib-testing"
            | "testing"
            | "staging"
            | "gnome-unstable"
            | "kde-unstable"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceFact {
    pub kind: RepoKind,
    pub repo: String,
    pub packager: Option<String>,
    /// pacman.conf requires a valid package signature from this repository.
    pub sig_required: bool,
    /// pacman.conf asks for no package signature from it.
    pub sig_never: bool,
    pub repo_packages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatFact {
    pub arch: String,
    pub system_arch: Vec<String>,
    pub missing_deps: Vec<String>,
    pub conflicts: Vec<String>,
    /// Dependencies that are not installed yet.
    pub new_deps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagingFact {
    /// The package base the history is of.
    pub base: String,
    pub days_since_change: u64,
    /// Commits to the PKGBUILD in the last 365 days (of the newest 100).
    pub recent_commits: usize,
    /// Different authors among the newest commits.
    pub authors: usize,
    pub commits_seen: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScorecardFact {
    /// 0 to 10.
    pub score: f64,
    pub date: String,
    /// Checks that scored below 5, worst first (name, score).
    pub weak: Vec<(String, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsvFact {
    /// The upstream version that was asked about.
    pub version: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpstreamFact {
    /// `owner/repo` on GitHub, when the package's URL names one.
    pub repo: Option<String>,
    pub scorecard: Option<ScorecardFact>,
    pub osv: Option<OsvFact>,
}

// -------------------------------------------------------------- dimensions

pub const REPRO_BAD: u8 = 20;

/// Whether the rebuilders' answer is for another version than the one asked about.
fn newer_note(f: &Facts, r: &ReproFact) -> bool {
    let want = if f.repro_for.is_empty() {
        &f.version
    } else {
        &f.repro_for
    };
    !want.is_empty() && &r.version != want
}

pub fn reproducibility(f: &Facts) -> Dimension {
    let key = "reproducibility";
    let title = "Reproducibility";
    if f.repo_kind != RepoKind::Arch {
        return unknown(
            key,
            title,
            "the Arch rebuilders check packages from the Arch repositories only",
            None,
        );
    }
    let Some(r) = &f.repro else {
        let why = f
            .repro_note
            .clone()
            .unwrap_or_else(|| "the rebuilders have no record of this package".to_string());
        return unknown(
            key,
            title,
            why,
            Some(format!(
                "https://reproducible.archlinux.org/ lists {}",
                f.name
            )),
        );
    };
    if newer_note(f, r) {
        return unknown(
            key,
            title,
            format!(
                "the rebuilders checked {}, not {}: a result for another version says nothing about this one",
                r.version,
                if f.repro_for.is_empty() { &f.version } else { &f.repro_for }
            ),
            None,
        );
    }
    let (score, text) = match r.status {
        ReproStatus::Good => (
            100,
            "the Arch rebuilders rebuilt this version and got the same files",
        ),
        ReproStatus::Bad => (
            REPRO_BAD,
            "the Arch rebuilders rebuilt this version and the files differ",
        ),
        ReproStatus::Unknown => {
            return unknown(
                key,
                title,
                "the rebuilders have not decided yet (queued or not rebuildable)",
                None,
            )
        }
    };
    let mut summary = format!("{text} ({} in {})", r.version, r.suite);
    if r.attested {
        summary.push_str("; an attestation is published");
    }
    if let Some(n) = &r.stale_note {
        summary.push_str(&format!("; {n}"));
    }
    Dimension {
        key,
        title,
        score: Some(score),
        summary,
        rules: vec![
            rule(
                "reproduced scores 100, differs scores 20, undecided is unknown; a result for another version is unknown",
                None,
            ),
            rule(
                "a failure to reproduce is not proof of tampering (build environments differ) and a success is not proof of safety: it shows the published files match the source rebuilt independently",
                None,
            ),
        ],
        evidence: Evidence::Authoritative,
        hint: None,
    }
}

pub fn provenance(f: &Facts) -> Dimension {
    let key = "provenance";
    let title = "Provenance";
    let Some(p) = &f.provenance else {
        return unknown(key, title, "where it comes from could not be read", None);
    };
    let mut rules = Vec::new();
    let mut points: i32;
    let mut parts: Vec<String> = Vec::new();
    match p.kind {
        RepoKind::Arch => {
            points = 70;
            rules.push(rule("from an Arch repository: 70", None));
            parts.push(format!("from the {} repository", p.repo));
            match f.signed {
                Some(true) => {
                    points += 15;
                    rules.push(rule("the database carries its signature", Some(15)));
                    parts.push("signature in the database".to_string());
                }
                Some(false) => {
                    points -= 20;
                    rules.push(rule("the database has no signature for it", Some(-20)));
                    parts.push("no signature in the database".to_string());
                }
                None => {}
            }
            if let Some(r) = &f.repro {
                if r.attested && !newer_note(f, r) {
                    points += 15;
                    rules.push(rule(
                        "an attestation is published for this version",
                        Some(15),
                    ));
                    parts.push("attested".to_string());
                }
            }
            if let Some(who) = &p.packager {
                // the name, not the address
                let who = who.split('<').next().unwrap_or(who).trim();
                if !who.is_empty() {
                    parts.push(format!("packaged by {who}"));
                }
            }
        }
        RepoKind::ThirdParty => {
            points = 30;
            rules.push(rule(
                format!("from {}, a repository Arch does not run: 30", p.repo),
                None,
            ));
            parts.push(format!("from {}, not an Arch repository", p.repo));
            if p.sig_never {
                points -= 25;
                rules.push(rule(
                    "pacman.conf asks for no package signature from it",
                    Some(-25),
                ));
                parts.push("no signature required".to_string());
            } else if p.sig_required {
                points += 20;
                rules.push(rule(
                    "pacman.conf requires a valid package signature from it",
                    Some(20),
                ));
                parts.push("signature required".to_string());
            } else {
                parts.push("signature optional".to_string());
            }
            parts.push(format!("{} packages in the repository", p.repo_packages));
        }
        RepoKind::Aur => {
            points = 30;
            rules.push(rule(
                "from the AUR, built here from a PKGBUILD anyone could have uploaded: 30",
                None,
            ));
            parts.push("from the AUR".to_string());
            if f.orphan {
                points -= 15;
                rules.push(rule("it has no maintainer", Some(-15)));
                parts.push("no maintainer".to_string());
            }
            if f.trust_high > 0 {
                let p = (f.trust_high as i32 * 20).min(40);
                points -= p;
                rules.push(rule(
                    format!(
                        "{} high trust signal(s): -20 each, at most -40",
                        f.trust_high
                    ),
                    Some(-p),
                ));
                parts.push(format!("{} high trust signal(s)", f.trust_high));
            }
            if f.trust_warning > 0 {
                let p = (f.trust_warning as i32 * 5).min(15);
                points -= p;
                rules.push(rule(
                    format!("{} trust warning(s): -5 each, at most -15", f.trust_warning),
                    Some(-p),
                ));
            }
        }
    }
    Dimension {
        key,
        title,
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        evidence: if p.kind == RepoKind::Arch {
            Evidence::Authoritative
        } else {
            Evidence::Corroborating
        },
        hint: None,
    }
}

pub fn compatibility(f: &Facts) -> Dimension {
    let key = "compatibility";
    let title = "Compatibility";
    let Some(c) = &f.compat else {
        return unknown(
            key,
            title,
            "only a package in a configured repository is checked here; the AUR is resolved when it is built",
            None,
        );
    };
    let mut rules = Vec::new();
    let mut parts = Vec::new();
    let arch_ok = c.arch == "any"
        || c.arch.is_empty()
        || c.system_arch.is_empty()
        || c.system_arch.contains(&c.arch);
    let mut points = 100;
    if !arch_ok {
        points = 0;
        rules.push(rule(
            format!(
                "built for {}, this machine is {}: 0",
                c.arch,
                c.system_arch.join(", ")
            ),
            None,
        ));
        parts.push(format!("built for {}", c.arch));
    } else {
        parts.push(format!(
            "architecture {}",
            if c.arch.is_empty() { "any" } else { &c.arch }
        ));
    }
    if !c.missing_deps.is_empty() {
        let p = (c.missing_deps.len() as i32 * 40).min(80);
        points -= p;
        rules.push(rule(
            format!(
                "nothing installed or in the databases satisfies: {} (-40 each, at most -80)",
                c.missing_deps.join(", ")
            ),
            Some(-p),
        ));
        parts.push(format!(
            "{} dependency(ies) cannot be met",
            c.missing_deps.len()
        ));
    }
    if !c.conflicts.is_empty() {
        let p = (c.conflicts.len() as i32 * 30).min(60);
        points -= p;
        rules.push(rule(
            format!(
                "conflicts with installed {} and does not replace it (-30 each, at most -60)",
                c.conflicts.join(", ")
            ),
            Some(-p),
        ));
        parts.push(format!("conflicts with {}", c.conflicts.join(", ")));
    }
    if c.missing_deps.is_empty() && c.conflicts.is_empty() && arch_ok {
        parts.push("dependencies can be met, no conflicts".to_string());
    }
    if c.new_deps > 0 {
        rules.push(rule(
            format!(
                "{} dependency(ies) are not installed yet (a fact, no points)",
                c.new_deps
            ),
            None,
        ));
    }
    Dimension {
        key,
        title,
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        evidence: Evidence::Authoritative,
        hint: None,
    }
}

pub const PACKAGING_AGE_LIMITS: &[(f64, u8)] = &[(90.0, 100), (365.0, 85), (730.0, 65)];

pub fn packaging(f: &Facts) -> Dimension {
    let key = "packaging";
    let title = "Packaging quality";
    let Some(p) = &f.packaging else {
        return unknown(
            key,
            title,
            f.packaging_note
                .clone()
                .unwrap_or_else(|| "the packaging history is looked up for Arch repository packages (not with --offline)".to_string()),
            None,
        );
    };
    let base = band(p.days_since_change as f64, PACKAGING_AGE_LIMITS, 45);
    let mut points = i32::from(base);
    let mut rules = vec![rule(
        format!(
            "the PKGBUILD last changed {} days ago: up to 90 days scores 100, a year 85, two years 65, older 45 (this: {base}); a stable package can be old",
            p.days_since_change
        ),
        None,
    )];
    let mut parts = vec![format!("PKGBUILD changed {} days ago", p.days_since_change)];
    if p.recent_commits == 0 {
        points -= 10;
        rules.push(rule(
            "no change to the PKGBUILD in the last year",
            Some(-10),
        ));
    } else {
        parts.push(format!("{} change(s) in the last year", p.recent_commits));
    }
    if p.authors <= 1 && p.commits_seen > 1 {
        points -= 10;
        rules.push(rule("every recent change is by one person", Some(-10)));
        parts.push("one packager".to_string());
    } else if p.authors > 1 {
        parts.push(format!("{} packagers", p.authors));
    }
    Dimension {
        key,
        title,
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        evidence: Evidence::Heuristic,
        hint: None,
    }
}

pub fn upstream(f: &Facts) -> Dimension {
    let key = "upstream";
    let title = "Upstream health";
    let Some(u) = &f.upstream else {
        return unknown(
            key,
            title,
            f.upstream_note
                .clone()
                .unwrap_or_else(|| "upstream is looked up unless --offline is set".to_string()),
            None,
        );
    };
    if u.scorecard.is_none() && u.osv.is_none() {
        return unknown(
            key,
            title,
            match &u.repo {
                None => "upstream not identified: the package's URL is not a GitHub repository"
                    .to_string(),
                Some(r) => format!("neither the OpenSSF Scorecard nor OSV know github.com/{r}"),
            },
            None,
        );
    }
    let mut rules = Vec::new();
    let mut parts = Vec::new();
    let mut points: i32 = if let Some(s) = &u.scorecard {
        let p = (s.score * 10.0).round() as i32;
        rules.push(rule(
            format!(
                "OpenSSF Scorecard {:.1} of 10 ({}): ten times the score",
                s.score, s.date
            ),
            None,
        ));
        parts.push(format!("Scorecard {:.1} of 10", s.score));
        if !s.weak.is_empty() {
            let weak: Vec<String> = s
                .weak
                .iter()
                .take(4)
                .map(|(n, v)| format!("{n} {v}"))
                .collect();
            rules.push(rule(format!("weak checks: {}", weak.join(", ")), None));
        }
        p
    } else {
        rules.push(rule(
            "no Scorecard for this repository: starts at 100",
            None,
        ));
        100
    };
    if let Some(o) = &u.osv {
        if o.ids.is_empty() {
            parts.push(format!("no OSV entry for {}", o.version));
            rules.push(rule(
                format!("OSV lists nothing for upstream {}", o.version),
                None,
            ));
        } else {
            let p = (o.ids.len() as i32 * 5).min(30);
            points -= p;
            parts.push(format!(
                "{} OSV entr{} for {}",
                o.ids.len(),
                if o.ids.len() == 1 { "y" } else { "ies" },
                o.version
            ));
            rules.push(rule(
                format!(
                    "OSV lists {} for upstream {} (-5 each, at most -30): {}; Arch may have patched them",
                    o.ids.len(),
                    o.version,
                    o.ids.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ),
                Some(-p),
            ));
        }
    }
    Dimension {
        key,
        title,
        score: Some(clamp(points)),
        summary: parts.join("; "),
        rules,
        evidence: Evidence::Heuristic,
        hint: None,
    }
}

/// Gates this module adds: a repository that asks for no package signature.
pub fn gates(f: &Facts) -> Vec<Gate> {
    let mut out = Vec::new();
    if let Some(p) = &f.provenance {
        if p.kind == RepoKind::ThirdParty && p.sig_never {
            out.push(Gate {
                name: "provenance",
                text: format!(
                    "the repository {} asks for no package signature (SigLevel Never), so nothing proves who built {}",
                    p.repo, f.name
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{card, card_with, Weights};

    fn arch() -> Facts {
        Facts {
            name: "pacman".into(),
            version: "7.1-3".into(),
            repo: "core".into(),
            official: true,
            repo_kind: RepoKind::Arch,
            repro_for: "7.1-3".into(),
            signed: Some(true),
            ..Facts::default()
        }
    }

    fn repro(status: ReproStatus, version: &str, attested: bool) -> ReproFact {
        ReproFact {
            status,
            version: version.into(),
            suite: "core".into(),
            built_at: "2026-10-16T14:08:58".into(),
            attested,
            age_hours: 1,
            stale_note: None,
        }
    }

    #[test]
    fn reproducibility_is_for_the_exact_version_and_unknown_otherwise() {
        let mut f = arch();
        f.repro = Some(repro(ReproStatus::Good, "7.1-3", true));
        let d = reproducibility(&f);
        assert_eq!((d.score, d.evidence), (Some(100), Evidence::Authoritative));
        assert!(d.summary.contains("same files") && d.summary.contains("attestation"));
        f.repro = Some(repro(ReproStatus::Bad, "7.1-3", false));
        assert_eq!(reproducibility(&f).score, Some(20));
        // another version says nothing about this one
        f.repro = Some(repro(ReproStatus::Good, "7.0-1", false));
        let d = reproducibility(&f);
        assert_eq!(d.score, None);
        assert!(
            d.summary.contains("checked 7.0-1, not 7.1-3"),
            "{}",
            d.summary
        );
        // undecided
        f.repro = Some(repro(ReproStatus::Unknown, "7.1-3", false));
        assert_eq!(reproducibility(&f).score, None);
        // no record, and why
        f.repro = None;
        assert!(reproducibility(&f).summary.contains("no record"));
        f.repro_note = Some("not checked: --offline is set and nothing was saved".into());
        assert!(reproducibility(&f).summary.contains("not checked"));
    }

    #[test]
    fn reproducibility_does_not_cover_the_aur_or_other_repositories() {
        for kind in [RepoKind::Aur, RepoKind::ThirdParty] {
            let mut f = arch();
            f.repo_kind = kind;
            f.repro = Some(repro(ReproStatus::Good, "7.1-3", true));
            let d = reproducibility(&f);
            assert_eq!(d.score, None, "{kind:?}");
            assert!(d.summary.contains("Arch repositories only"));
        }
    }

    #[test]
    fn a_failed_reproduction_lowers_the_dimension_and_opens_no_gate() {
        let mut f = arch();
        f.repro = Some(repro(ReproStatus::Bad, "7.1-3", false));
        assert!(gates(&f).is_empty());
        assert!(card(&f).gates.is_empty());
    }

    fn prov(kind: RepoKind) -> ProvenanceFact {
        ProvenanceFact {
            kind,
            repo: "core".into(),
            packager: Some("A Packager <a@example.org>".into()),
            sig_required: true,
            sig_never: false,
            repo_packages: 300,
        }
    }

    #[test]
    fn provenance_of_an_arch_package_counts_its_signature_and_attestation() {
        let mut f = arch();
        f.provenance = Some(prov(RepoKind::Arch));
        assert_eq!(provenance(&f).score, Some(85), "70 + 15 for the signature");
        f.repro = Some(repro(ReproStatus::Good, "7.1-3", true));
        let d = provenance(&f);
        assert_eq!(d.score, Some(100), "+15 for the attestation, capped at 100");
        assert!(d.summary.contains("packaged by A Packager") && d.summary.contains("attested"));
        // an attestation for another version does not count
        f.repro = Some(repro(ReproStatus::Good, "7.0-1", true));
        assert_eq!(provenance(&f).score, Some(85));
        f.repro = None;
        f.signed = Some(false);
        assert_eq!(provenance(&f).score, Some(50));
        assert_eq!(provenance(&f).evidence, Evidence::Authoritative);
    }

    #[test]
    fn provenance_of_a_third_party_repository_depends_on_its_signature_level() {
        let mut f = arch();
        f.repo_kind = RepoKind::ThirdParty;
        let mut p = prov(RepoKind::ThirdParty);
        p.repo = "chaotic-aur".into();
        f.provenance = Some(p.clone());
        let d = provenance(&f);
        assert_eq!((d.score, d.evidence), (Some(50), Evidence::Corroborating));
        assert!(
            d.summary.contains("not an Arch repository")
                && d.summary.contains("signature required")
        );
        p.sig_required = false;
        p.sig_never = true;
        f.provenance = Some(p.clone());
        assert_eq!(provenance(&f).score, Some(5));
        let g = gates(&f);
        assert_eq!(g.len(), 1);
        assert!(g[0].text.contains("chaotic-aur") && g[0].text.contains("no package signature"));
        p.sig_never = false;
        f.provenance = Some(p);
        assert!(gates(&f).is_empty());
        assert_eq!(provenance(&f).score, Some(30));
    }

    #[test]
    fn provenance_of_an_aur_package_loses_points_for_orphans_and_trust_signals() {
        let mut f = Facts {
            name: "x".into(),
            ..Facts::default()
        };
        f.provenance = Some(prov(RepoKind::Aur));
        assert_eq!(provenance(&f).score, Some(30));
        f.orphan = true;
        f.trust_high = 1;
        f.trust_warning = 2;
        assert_eq!(
            provenance(&f).score,
            Some(0),
            "30 - 15 - 20 - 10, never below 0"
        );
        f.trust_high = 5;
        let d = provenance(&f);
        assert!(d.rules.iter().any(|r| r.points == Some(-40)), "at most -40");
    }

    #[test]
    fn provenance_is_unknown_when_it_could_not_be_read() {
        assert_eq!(provenance(&arch()).score, None);
    }

    fn compat() -> CompatFact {
        CompatFact {
            arch: "x86_64".into(),
            system_arch: vec!["x86_64".into()],
            missing_deps: vec![],
            conflicts: vec![],
            new_deps: 0,
        }
    }

    #[test]
    fn compatibility_starts_at_100_and_loses_points_for_each_problem() {
        let mut f = arch();
        assert_eq!(compatibility(&f).score, None, "no facts: unknown");
        let with = |f: &mut Facts, new_deps: usize, missing: &[&str], conflicts: &[&str]| {
            let mut c = compat();
            c.new_deps = new_deps;
            c.missing_deps = missing.iter().map(|s| s.to_string()).collect();
            c.conflicts = conflicts.iter().map(|s| s.to_string()).collect();
            f.compat = Some(c);
            compatibility(f).score
        };
        assert_eq!(with(&mut f, 0, &[], &[]), Some(100));
        assert_eq!(
            with(&mut f, 3, &[], &[]),
            Some(100),
            "new dependencies are a fact, not points"
        );
        assert_eq!(with(&mut f, 0, &["libx"], &[]), Some(60));
        assert_eq!(
            with(&mut f, 0, &["a", "b", "c"], &[]),
            Some(20),
            "at most -80"
        );
        assert_eq!(with(&mut f, 0, &[], &["old"]), Some(70));
        assert_eq!(
            with(&mut f, 0, &[], &["a", "b", "c"]),
            Some(40),
            "at most -60"
        );
        assert_eq!(with(&mut f, 0, &["a"], &["b"]), Some(30));
    }

    #[test]
    fn a_package_for_another_architecture_scores_zero() {
        let mut f = arch();
        let mut c = compat();
        c.arch = "aarch64".into();
        f.compat = Some(c.clone());
        let d = compatibility(&f);
        assert_eq!(d.score, Some(0));
        assert!(d.summary.contains("built for aarch64"));
        c.arch = "any".into();
        f.compat = Some(c);
        assert_eq!(compatibility(&f).score, Some(100));
    }

    fn pk(days: u64, recent: usize, authors: usize) -> PackagingFact {
        PackagingFact {
            base: "pacman".into(),
            days_since_change: days,
            recent_commits: recent,
            authors,
            commits_seen: 10,
        }
    }

    #[test]
    fn packaging_bands_have_inclusive_edges_and_two_adjustments() {
        let mut f = arch();
        for (days, score) in [
            (0, 100),
            (90, 100),
            (91, 85),
            (365, 85),
            (366, 65),
            (730, 65),
            (731, 45),
        ] {
            f.packaging = Some(pk(days, 3, 2));
            assert_eq!(packaging(&f).score, Some(score), "{days} days");
        }
        f.packaging = Some(pk(30, 0, 2));
        assert_eq!(packaging(&f).score, Some(90), "no change in a year: -10");
        f.packaging = Some(pk(30, 3, 1));
        assert_eq!(packaging(&f).score, Some(90), "one packager: -10");
        f.packaging = Some(pk(900, 0, 1));
        assert_eq!(packaging(&f).score, Some(25));
        assert_eq!(packaging(&f).evidence, Evidence::Heuristic);
        f.packaging = None;
        f.packaging_note = Some("no packaging repository named x".into());
        assert!(packaging(&f).summary.contains("no packaging repository"));
    }

    #[test]
    fn upstream_uses_the_scorecard_and_takes_off_for_osv_entries() {
        let mut f = arch();
        assert_eq!(upstream(&f).score, None);
        let sc = ScorecardFact {
            score: 7.4,
            date: "2026-10-12".into(),
            weak: vec![("Code-Review".into(), 0)],
        };
        let mut u = UpstreamFact {
            repo: Some("a/b".into()),
            scorecard: Some(sc),
            osv: None,
        };
        f.upstream = Some(u.clone());
        let d = upstream(&f);
        assert_eq!((d.score, d.evidence), (Some(74), Evidence::Heuristic));
        assert!(d.rules.iter().any(|r| r.text.contains("Code-Review 0")));
        u.osv = Some(OsvFact {
            version: "1.0".into(),
            ids: vec!["A".into(), "B".into()],
        });
        f.upstream = Some(u.clone());
        assert_eq!(upstream(&f).score, Some(64), "two OSV entries: -10");
        u.osv = Some(OsvFact {
            version: "1.0".into(),
            ids: (0..20).map(|i| format!("V{i}")).collect(),
        });
        f.upstream = Some(u.clone());
        assert_eq!(upstream(&f).score, Some(44), "at most -30");
        // OSV alone starts at 100
        u.scorecard = None;
        u.osv = Some(OsvFact {
            version: "1.0".into(),
            ids: vec![],
        });
        f.upstream = Some(u.clone());
        assert_eq!(upstream(&f).score, Some(100));
        // neither knows the repository
        u.osv = None;
        f.upstream = Some(u);
        let d = upstream(&f);
        assert_eq!(d.score, None);
        assert!(d.summary.contains("neither the OpenSSF Scorecard nor OSV"));
    }

    #[test]
    fn an_upstream_that_is_not_on_github_says_so() {
        let mut f = arch();
        f.upstream = Some(UpstreamFact {
            repo: None,
            scorecard: None,
            osv: None,
        });
        assert!(upstream(&f).summary.contains("not a GitHub repository"));
    }

    #[test]
    fn the_proxy_dimensions_and_the_unreadable_ones_are_left_out_by_default() {
        let mut f = arch();
        f.age_days = Some(5);
        f.packaging = Some(pk(10, 3, 2));
        f.upstream = Some(UpstreamFact {
            repo: Some("a/b".into()),
            scorecard: Some(ScorecardFact {
                score: 1.0,
                date: "d".into(),
                weak: vec![],
            }),
            osv: None,
        });
        f.provenance = Some(prov(RepoKind::Arch));
        f.repro = Some(repro(ReproStatus::Good, "7.1-3", false));
        f.compat = Some(compat());
        let c = card(&f);
        let left: Vec<&str> = c.overall.left_out.iter().map(|l| l.key).collect();
        for k in ["maintenance", "packaging", "upstream", "popularity"] {
            assert!(left.contains(&k), "{k} should be left out: {left:?}");
        }
        // reproducibility, provenance, compatibility and security count
        for k in ["reproducibility", "provenance", "compatibility", "security"] {
            assert!(!left.contains(&k), "{k}");
        }
        // but they still show in the card with their numbers
        assert!(c
            .dimensions
            .iter()
            .any(|d| d.key == "upstream" && d.score == Some(10)));
        // opting in counts them
        let on = Weights::default()
            .with_list("upstream=5,packaging=2")
            .unwrap();
        let c2 = card_with(&f, &on);
        assert_ne!(c.overall.score, c2.overall.score);
        assert!(!c2.overall.left_out.iter().any(|l| l.key == "upstream"));
    }

    #[test]
    fn the_evidence_mix_counts_the_new_authoritative_dimensions() {
        let mut f = arch();
        f.provenance = Some(prov(RepoKind::Arch));
        f.repro = Some(repro(ReproStatus::Good, "7.1-3", false));
        f.compat = Some(compat());
        let c = card(&f);
        assert!(
            c.overall.evidence.authoritative >= 4,
            "{:?}",
            c.overall.evidence
        );
    }

    #[test]
    fn the_arch_repositories_are_named_exactly() {
        for r in ["core", "extra", "multilib", "extra-testing", "kde-unstable"] {
            assert!(is_arch_repo(r), "{r}");
        }
        for r in ["chaotic-aur", "archlinuxcn", "core2", "aur", ""] {
            assert!(!is_arch_repo(r), "{r}");
        }
    }
}

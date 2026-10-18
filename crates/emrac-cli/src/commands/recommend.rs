//! `emrac recommend` (u2): source or binary, from stated rules.

use emrac_core::recommend::{self, Facts, Report, Side, Verdict};
use emrac_core::{Error, Result, Sources};

/// What the user asked to see.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Both,
    Source,
    Binary,
}

/// `[source|binary] PKG` from the command line.
pub fn parse_args(args: &[String]) -> Result<(View, &str)> {
    match args {
        [pkg] => Ok((View::Both, pkg)),
        [side, pkg] if side == "source" => Ok((View::Source, pkg)),
        [side, pkg] if side == "binary" => Ok((View::Binary, pkg)),
        [other, _] => Err(Error::Recommend(format!(
            "expected `source` or `binary` before the package, not '{other}'"
        ))),
        _ => Err(Error::Recommend("give a package name".into())),
    }
}

pub fn run(sources: &Sources, args: &[String], offline: bool, json: bool) -> Result<()> {
    let (view, pkg) = parse_args(args)?;
    let facts = sources.recommend_facts(pkg, offline)?;
    let report = recommend::evaluate(&facts);
    if json {
        let value = serde_json::json!({ "facts": facts, "report": report });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return Ok(());
    }
    print_report(&facts, &report, view);
    Ok(())
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

fn wanted(side: Side, view: View) -> bool {
    matches!(
        (side, view),
        (Side::Both, _)
            | (_, View::Both)
            | (Side::Source, View::Source)
            | (Side::Binary, View::Binary)
    )
}

/// The reasons that weigh most first: a cap, then the biggest change in points.
pub fn by_weight(report: &Report, view: View) -> Vec<&recommend::Reason> {
    let mut v: Vec<&recommend::Reason> = report
        .reasons
        .iter()
        .filter(|r| wanted(r.side, view))
        .collect();
    v.sort_by_key(|r| (r.cap.is_none(), std::cmp::Reverse(r.points.abs())));
    v
}

pub fn print_report(facts: &Facts, report: &Report, view: View) {
    match view {
        View::Both => println!(
            "Recommendation for {}: {}",
            report.name,
            report.verdict.word()
        ),
        View::Source => println!("The source side of {}", report.name),
        View::Binary => println!("The binary side of {}", report.name),
    }
    if view == View::Both {
        if let Some(score) = report.score {
            println!(
                "  Score       {score} of 100 source suitability (65 and over: source; 35 and under: binary)"
            );
        }
        println!(
            "  Confidence  {} \u{2014} {}",
            report.confidence.word(),
            report.confidence_why
        );
    }
    if view != View::Binary {
        match report.build_secs {
            Some((lo, likely, hi)) if report.build_basis == "measured" => {
                println!(
                    "  Build here  {} (measured; {lo}\u{2013}{hi} s)",
                    duration(likely)
                );
            }
            Some((lo, likely, hi)) => println!(
                "  Build here  about {}, between {} and {} (estimated)",
                duration(likely),
                duration(lo),
                duration(hi)
            ),
            None => println!("  Build here  nothing to compile"),
        }
    }
    if view != View::Source {
        match (&facts.binary, facts.download_mib, facts.installed_mib) {
            (Some(b), Some(d), Some(i)) => {
                println!("  Binary      {b}: {d:.1} MiB to download, {i:.1} MiB installed")
            }
            (Some(b), _, _) => {
                println!("  Binary      {b} (AUR; its size is not known before it is built)")
            }
            (None, _, _) => {
                println!("  Binary      none: there is no official binary and no -bin package")
            }
        }
    }
    for note in &report.notes {
        println!("  Note        {note}");
    }
    let shown = by_weight(report, view);
    if !shown.is_empty() {
        println!("  Why");
        for r in shown.iter().take(3) {
            let pts = match (r.cap, r.points) {
                (Some(c), _) => format!("score capped at {c}"),
                (None, 0) => "no points".to_string(),
                (None, p) => format!("{p:+} points"),
            };
            println!("    - {} ({pts})", r.text);
        }
        if shown.len() > 3 {
            println!(
                "    ... {} more (`emrac explain {}` shows every rule and what would change the answer)",
                shown.len() - 3,
                report.name
            );
        }
        if view != View::Both {
            println!(
                "  Overall: {}; `emrac recommend {}` weighs both sides.",
                report.verdict.word(),
                report.name
            );
        }
    }
    if report.verdict == Verdict::SourceOnly {
        println!(
            "  Review and audit still apply: an AUR build is read and audited before it runs."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::recommend::{Confidence, Reason};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_side_comes_before_the_package_and_only_two_words_are_sides() {
        assert!(matches!(parse_args(&args(&["rg"])), Ok((View::Both, "rg"))));
        assert!(matches!(
            parse_args(&args(&["source", "rg"])),
            Ok((View::Source, "rg"))
        ));
        assert!(matches!(
            parse_args(&args(&["binary", "rg"])),
            Ok((View::Binary, "rg"))
        ));
        let e = parse_args(&args(&["fast", "rg"]))
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("'fast'") && e.contains("source"), "{e}");
        assert!(parse_args(&args(&[])).is_err());
    }

    #[test]
    fn the_reasons_that_weigh_most_come_first_and_a_side_shows_only_its_own() {
        let reason = |rule, side, points, cap| Reason {
            rule,
            side,
            text: rule.to_string(),
            points,
            input: String::new(),
            cap,
        };
        let report = Report {
            name: "x".into(),
            verdict: Verdict::Binary,
            score: Some(20),
            confidence: Confidence::High,
            confidence_why: String::new(),
            build_secs: None,
            build_basis: "estimated",
            reasons: vec![
                reason("small", Side::Source, -5, None),
                reason("cost", Side::Source, -30, None),
                reason("trust", Side::Binary, -5, None),
                reason("flags", Side::Source, 0, Some(30)),
            ],
            notes: vec![],
        };
        let order: Vec<_> = by_weight(&report, View::Both)
            .iter()
            .map(|r| r.rule)
            .collect();
        assert_eq!(order, ["flags", "cost", "small", "trust"]);
        let source: Vec<_> = by_weight(&report, View::Source)
            .iter()
            .map(|r| r.rule)
            .collect();
        assert_eq!(source, ["flags", "cost", "small"]);
        let binary: Vec<_> = by_weight(&report, View::Binary)
            .iter()
            .map(|r| r.rule)
            .collect();
        assert_eq!(binary, ["trust"]);
    }
}

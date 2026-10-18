//! `emrac explain` (u3): how the recommendation of `recommend` was reached,
//! rule by rule, and what would change it.

use emrac_core::recommend::{self, Alternative, ExplainedRule, Explanation, Verdict};
use emrac_core::{Result, Sources};

pub fn run(sources: &Sources, pkg: &str, offline: bool, json: bool) -> Result<()> {
    let facts = sources.recommend_facts(pkg, offline)?;
    let explanation = recommend::explain(&facts);
    if json {
        let value = serde_json::json!({ "facts": facts, "explanation": explanation });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return Ok(());
    }
    print_explanation(&explanation);
    Ok(())
}

/// A source as one tidy line: runs of spaces collapsed, long values cut.
fn tidy(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 100 {
        let cut: String = one.chars().take(99).collect();
        format!("{cut}\u{2026}")
    } else {
        one
    }
}

/// Points as they are shown: `+25`, `-20`, or the cap.
fn points(rule: &ExplainedRule) -> String {
    match (rule.cap, rule.points) {
        (Some(cap), _) => format!("capped at {cap}"),
        (None, 0) => "0".to_string(),
        (None, p) => format!("{p:+}"),
    }
}

/// Which side of the thresholds a score falls on, in words.
fn threshold(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Source => "65 or more",
        Verdict::Binary => "35 or less",
        Verdict::Either => "between 36 and 64",
        Verdict::SourceOnly => "there is no binary",
    }
}

fn alternative_line(a: &Alternative, now: &Explanation) -> String {
    let mut effects: Vec<String> = Vec::new();
    if a.score != now.report.score || a.verdict != now.report.verdict {
        match a.score {
            Some(s) => effects.push(format!("the score would be {s} ({})", a.verdict.word())),
            None => effects.push(format!("the verdict would be {}", a.verdict.word())),
        }
    }
    if a.confidence != now.report.confidence {
        effects.push(format!("the confidence would be {}", a.confidence.word()));
    }
    format!("{}: {}", a.change, effects.join(", "))
}

pub fn print_explanation(e: &Explanation) {
    let report = &e.report;
    println!(
        "Recommendation for {}: {}    confidence: {} \u{2014} {}",
        report.name,
        report.verdict.word(),
        report.confidence.word(),
        report.confidence_why
    );
    println!();

    let width = e
        .rules
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0)
        .max(5);
    if report.score.is_some() {
        println!("  \u{2713} {:<width$}  {:>14}", "start", recommend::START);
    }
    for rule in &e.rules {
        let mark = if rule.helps { "\u{2713}" } else { "\u{2717}" };
        println!("  {mark} {:<width$}  {:>14}", rule.label, points(rule));
        println!("      {}", rule.text);
        println!("      from: {}", tidy(&rule.input));
    }
    println!();
    match (report.score, e.before_cap) {
        (Some(score), Some(before)) if before != score => println!(
            "  Total {before}, limited to {score} of 100 \u{2192} {} ({})",
            report.verdict.word(),
            threshold(report.verdict)
        ),
        (Some(score), _) => println!(
            "  Score {score} of 100 \u{2192} {} ({})",
            report.verdict.word(),
            threshold(report.verdict)
        ),
        (None, _) => println!(
            "  Nothing is scored: {}",
            if report.verdict == Verdict::SourceOnly {
                "there is no binary to choose instead"
            } else {
                "there is nothing to choose between"
            }
        ),
    }
    for note in &report.notes {
        println!("  Note: {note}");
    }
    if !e.alternatives.is_empty() {
        println!();
        println!("What would change this (the engine run again with only that changed):");
        for a in &e.alternatives {
            println!("  - {}", alternative_line(a, e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::recommend::{Confidence, Report};

    fn rule(
        rule: &'static str,
        label: &'static str,
        points: i32,
        cap: Option<i32>,
    ) -> ExplainedRule {
        ExplainedRule {
            rule,
            label,
            points,
            cap,
            helps: cap.is_none() && points >= 0,
            text: format!("{rule} text"),
            input: format!("{rule} input"),
        }
    }

    #[test]
    fn points_say_the_cap_instead_of_zero_and_signs_otherwise() {
        assert_eq!(points(&rule("a", "a", 0, Some(30))), "capped at 30");
        assert_eq!(points(&rule("a", "a", 25, None)), "+25");
        assert_eq!(points(&rule("a", "a", -20, None)), "-20");
        assert_eq!(points(&rule("a", "a", 0, None)), "0");
    }

    #[test]
    fn an_alternative_names_only_what_changes() {
        let report = Report {
            name: "x".into(),
            verdict: Verdict::Binary,
            score: Some(22),
            confidence: Confidence::Medium,
            confidence_why: String::new(),
            build_secs: None,
            build_basis: "estimated",
            reasons: vec![],
            notes: vec![],
        };
        let e = Explanation {
            report,
            rules: vec![],
            before_cap: Some(22),
            alternatives: vec![],
        };
        let score_only = Alternative {
            rule: "gain-tuned",
            change: "`-march=native`".into(),
            score: Some(47),
            verdict: Verdict::Either,
            confidence: Confidence::Medium,
        };
        assert_eq!(
            alternative_line(&score_only, &e),
            "`-march=native`: the score would be 47 (either)"
        );
        let confidence_only = Alternative {
            rule: "confidence",
            change: "three builds".into(),
            score: Some(22),
            verdict: Verdict::Binary,
            confidence: Confidence::High,
        };
        assert_eq!(
            alternative_line(&confidence_only, &e),
            "three builds: the confidence would be high"
        );
    }

    #[test]
    fn a_source_is_one_line_and_cut_when_long() {
        assert_eq!(tidy("CFLAGS  `-O2    -pipe`"), "CFLAGS `-O2 -pipe`");
        let long = "x".repeat(300);
        let t = tidy(&long);
        assert_eq!(t.chars().count(), 100);
        assert!(t.ends_with('\u{2026}'));
    }

    #[test]
    fn the_thresholds_are_the_engines() {
        assert_eq!(threshold(Verdict::Source), "65 or more");
        assert_eq!(threshold(Verdict::Binary), "35 or less");
        assert_eq!(recommend::SOURCE_AT, 65);
        assert_eq!(recommend::BINARY_AT, 35);
    }
}

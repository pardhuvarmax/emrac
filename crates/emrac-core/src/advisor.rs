//! The Package Advisor (u15): advice under a plan that is unusually big.
//! See `docs/plan/r3/u15-package-advisor.md`. Advice only: it blocks nothing and
//! is no part of the plan or its id.

use serde::Serialize;

use crate::doctor::human;
use crate::plan::PlanAction;

const MIB: u64 = 1 << 20;
/// A plan must be at least this big before "one package is most of it" is said.
const DOMINANT_MIN: u64 = 300 * MIB;
/// And that package must be at least this share of the installed size.
const DOMINANT_PERCENT: u64 = 60;
/// Growth that is a quarter of the free space is worth saying, if it is at least this.
const SHARE_MIN: u64 = 256 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub enabled: bool,
    pub large_download: u64,
    pub large_growth: u64,
    pub many_dependencies: u32,
}

impl Thresholds {
    pub fn from_config(c: &crate::config::AdvisorConfig) -> Self {
        Self {
            enabled: c.enabled,
            large_download: u64::from(c.large_download_mib) * MIB,
            large_growth: u64::from(c.large_growth_mib) * MIB,
            many_dependencies: c.many_dependencies,
        }
    }
}

/// One package of the plan, with the sizes the repository gives for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sized {
    pub name: String,
    /// Asked for, not pulled in.
    pub explicit: bool,
    pub download: u64,
    pub installed: u64,
    /// Not in a sync database (an AUR package): its sizes are not known.
    pub unknown: bool,
}

#[derive(Debug, Clone)]
pub struct Input {
    pub action: PlanAction,
    pub packages: Vec<Sized>,
    pub download_total: u64,
    /// Installed-size growth of the whole plan (0 when it shrinks).
    pub growth: u64,
    /// Free bytes on `/`, when known.
    pub free: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    LargeDownload,
    LargeGrowth,
    FreeSpaceShare,
    ManyDependencies,
    OnePackageIsMostOfIt,
    UnknownSizes,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Advice {
    pub kind: Kind,
    pub text: String,
    /// A command that shows more, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

fn verb(action: PlanAction) -> &'static str {
    match action {
        PlanAction::Install => "install",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Reinstall => "reinstall",
        PlanAction::Downgrade => "downgrade",
        PlanAction::Remove => "removal",
        PlanAction::Purge => "purge",
    }
}

/// The biggest `n` of `items` by `size`, as `name 1.2 GiB, name 301 MiB`.
fn biggest(items: &[(&str, u64)], n: usize) -> String {
    let mut v: Vec<&(&str, u64)> = items.iter().filter(|(_, s)| *s > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    v.iter()
        .take(n)
        .map(|(name, size)| format!("{name} {}", human(*size)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn advise(input: &Input, t: &Thresholds) -> Vec<Advice> {
    if !t.enabled
        || !matches!(
            input.action,
            PlanAction::Install | PlanAction::Upgrade | PlanAction::Reinstall
        )
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    let what = verb(input.action);
    let first_asked = input
        .packages
        .iter()
        .find(|p| p.explicit)
        .map(|p| p.name.clone());

    if t.large_download > 0 && input.download_total >= t.large_download {
        let items: Vec<(&str, u64)> = input
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.download))
            .collect();
        out.push(Advice {
            kind: Kind::LargeDownload,
            text: format!(
                "Large download: this {what} downloads {} (advice starts at {}).{}",
                human(input.download_total),
                human(t.large_download),
                match biggest(&items, 3) {
                    b if b.is_empty() => String::new(),
                    b => format!(" Biggest: {b}."),
                }
            ),
            hint: None,
        });
    }

    let growth_big = t.large_growth > 0 && input.growth >= t.large_growth;
    if growth_big {
        let items: Vec<(&str, u64)> = input
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.installed))
            .collect();
        out.push(Advice {
            kind: Kind::LargeGrowth,
            text: format!(
                "Large disk growth: this {what} needs {} more disk (advice starts at {}).{}",
                human(input.growth),
                human(t.large_growth),
                match biggest(&items, 3) {
                    b if b.is_empty() => String::new(),
                    b => format!(" Biggest: {b}."),
                }
            ),
            hint: None,
        });
    }
    if let Some(free) = input.free {
        if free > 0 && input.growth >= SHARE_MIN && input.growth.saturating_mul(4) >= free {
            out.push(Advice {
                kind: Kind::FreeSpaceShare,
                text: format!(
                    "A lot of your free space: it needs {} more, {}% of the {} free on /.",
                    human(input.growth),
                    input.growth.saturating_mul(100) / free,
                    human(free)
                ),
                hint: None,
            });
        }
    }

    let deps = input.packages.iter().filter(|p| !p.explicit).count() as u32;
    if t.many_dependencies > 0 && deps >= t.many_dependencies {
        let asked = input.packages.iter().filter(|p| p.explicit).count();
        out.push(Advice {
            kind: Kind::ManyDependencies,
            text: format!(
                "Many new packages: it pulls in {deps} packages you did not ask for (for {asked} you did)."
            ),
            hint: first_asked
                .as_ref()
                .map(|p| format!("to see what it pulls in: emrac tree {p} --depth 2")),
        });
    }

    let total_installed: u64 = input.packages.iter().map(|p| p.installed).sum();
    // For an upgrade the sizes are those of the new versions, which says little.
    if input.action == PlanAction::Install
        && input.packages.len() >= 2
        && total_installed >= DOMINANT_MIN
    {
        if let Some(top) = input.packages.iter().max_by_key(|p| p.installed) {
            let pct = top.installed * 100 / total_installed;
            if pct >= DOMINANT_PERCENT {
                out.push(Advice {
                    kind: Kind::OnePackageIsMostOfIt,
                    text: format!(
                        "One package is most of it: {} is {} of the {} this {what} installs ({pct}%).",
                        top.name,
                        human(top.installed),
                        human(total_installed)
                    ),
                    hint: None,
                });
            }
        }
    }

    let unknown = input.packages.iter().filter(|p| p.unknown).count();
    if unknown > 0 && !out.is_empty() {
        out.push(Advice {
            kind: Kind::UnknownSizes,
            text: format!(
                "{unknown} AUR package{} not counted above: the size of a package from the AUR is not known until it is built.",
                if unknown == 1 { " is" } else { "s are" }
            ),
            hint: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, explicit: bool, download_mib: u64, installed_mib: u64) -> Sized {
        Sized {
            name: name.into(),
            explicit,
            download: download_mib * MIB,
            installed: installed_mib * MIB,
            unknown: false,
        }
    }

    fn input(packages: Vec<Sized>) -> Input {
        let download_total = packages.iter().map(|p| p.download).sum();
        let growth = packages.iter().map(|p| p.installed).sum();
        Input {
            action: PlanAction::Install,
            packages,
            download_total,
            growth,
            free: None,
        }
    }

    fn defaults() -> Thresholds {
        Thresholds {
            enabled: true,
            large_download: 1024 * MIB,
            large_growth: 2048 * MIB,
            many_dependencies: 40,
        }
    }

    fn kinds(i: &Input, t: &Thresholds) -> Vec<Kind> {
        advise(i, t).into_iter().map(|a| a.kind).collect()
    }

    #[test]
    fn a_small_plan_gets_no_advice() {
        let i = input(vec![p("vim", true, 2, 40), p("gpm", false, 1, 1)]);
        assert!(advise(&i, &defaults()).is_empty());
    }

    #[test]
    fn the_download_threshold_is_inclusive_and_names_the_biggest() {
        let at = input(vec![
            p("a", true, 700, 10),
            p("b", false, 324, 10),
            p("c", false, 1, 1),
        ]);
        let out = advise(&at, &defaults());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::LargeDownload);
        assert!(
            out[0].text.contains("1.0 GiB") && out[0].text.contains("Biggest: a 700"),
            "{}",
            out[0].text
        );
        let below = input(vec![p("a", true, 700, 10), p("b", false, 323, 10)]);
        assert!(advise(&below, &defaults()).is_empty());
    }

    #[test]
    fn growth_in_gib_is_large_growth() {
        let i = input(vec![p("fonts", true, 100, 2100)]);
        assert_eq!(kinds(&i, &defaults()), [Kind::LargeGrowth]);
        let a = &advise(&i, &defaults())[0];
        assert!(
            a.text.contains("needs 2.1 GiB more") || a.text.contains("2.0 GiB"),
            "{}",
            a.text
        );
    }

    #[test]
    fn a_quarter_of_the_free_space_is_said_even_below_the_growth_limit() {
        let mut i = input(vec![p("a", true, 50, 800)]);
        i.free = Some(3000 * MIB);
        let out = advise(&i, &defaults());
        assert_eq!(
            out.iter().map(|a| a.kind).collect::<Vec<_>>(),
            [Kind::FreeSpaceShare]
        );
        assert!(out[0].text.contains("26%"), "{}", out[0].text);
        // plenty of room: nothing
        i.free = Some(100_000 * MIB);
        assert!(advise(&i, &defaults()).is_empty());
        // a tiny plan on a tiny disk: below the 256 MiB floor, nothing
        let mut small = input(vec![p("a", true, 5, 100)]);
        small.free = Some(200 * MIB);
        assert!(advise(&small, &defaults()).is_empty());
    }

    #[test]
    fn many_dependencies_counts_only_those_not_asked_for_and_gives_a_command() {
        let mut pk = vec![p("big", true, 5, 50)];
        pk.extend((0..40).map(|i| p(&format!("d{i}"), false, 1, 1)));
        let out = advise(&input(pk), &defaults());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::ManyDependencies);
        assert!(out[0]
            .text
            .contains("40 packages you did not ask for (for 1 you did)"));
        assert_eq!(
            out[0].hint.as_deref(),
            Some("to see what it pulls in: emrac tree big --depth 2")
        );
        let mut few = vec![p("big", true, 5, 50)];
        few.extend((0..39).map(|i| p(&format!("d{i}"), false, 1, 1)));
        assert!(advise(&input(few), &defaults()).is_empty());
    }

    #[test]
    fn one_package_that_is_most_of_a_plan_is_named() {
        let i = input(vec![
            p("llvm-libs", true, 40, 900),
            p("zlib", false, 1, 100),
        ]);
        let out = advise(&i, &defaults());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::OnePackageIsMostOfIt);
        assert!(
            out[0]
                .text
                .contains("llvm-libs is 900.0 MiB of the 1000.0 MiB")
                && out[0].text.contains("90%"),
            "{}",
            out[0].text
        );
        // a plan of one package has no "most of it"
        assert!(advise(&input(vec![p("llvm-libs", true, 40, 900)]), &defaults()).is_empty());
        // a share below 60%, or a plan under 300 MiB, says nothing
        assert!(advise(
            &input(vec![p("a", true, 1, 500), p("b", false, 1, 400)]),
            &defaults()
        )
        .is_empty());
        assert!(advise(
            &input(vec![p("a", true, 1, 200), p("b", false, 1, 20)]),
            &defaults()
        )
        .is_empty());
    }

    #[test]
    fn a_zero_threshold_turns_that_rule_off_and_enabled_false_turns_all_off() {
        let i = input(vec![p("a", true, 5000, 9000)]);
        assert!(kinds(&i, &defaults()).contains(&Kind::LargeDownload));
        let t = Thresholds {
            large_download: 0,
            large_growth: 0,
            ..defaults()
        };
        assert!(advise(&i, &t).is_empty());
        let off = Thresholds {
            enabled: false,
            ..defaults()
        };
        assert!(advise(&i, &off).is_empty());
    }

    #[test]
    fn removals_and_downgrades_get_no_advice() {
        for action in [PlanAction::Remove, PlanAction::Purge, PlanAction::Downgrade] {
            let mut i = input(vec![p("a", true, 5000, 9000)]);
            i.action = action;
            assert!(advise(&i, &defaults()).is_empty(), "{action:?}");
        }
        let mut up = input(vec![p("a", true, 5000, 9000), p("b", true, 1, 10)]);
        up.action = PlanAction::Upgrade;
        assert!(
            !kinds(&up, &defaults()).contains(&Kind::OnePackageIsMostOfIt),
            "an upgrade's sizes are the new versions' sizes"
        );
        assert!(advise(&up, &defaults())[0]
            .text
            .contains("this upgrade downloads"));
    }

    #[test]
    fn aur_packages_are_said_to_be_uncounted_only_when_there_is_other_advice() {
        let mut aur = p("some-aur", true, 0, 0);
        aur.unknown = true;
        let quiet = input(vec![aur.clone(), p("small", false, 1, 1)]);
        assert!(advise(&quiet, &defaults()).is_empty());
        let mut loud = input(vec![aur, p("huge", false, 3000, 3000)]);
        loud.growth = 3000 * MIB;
        let out = advise(&loud, &defaults());
        assert_eq!(out.last().unwrap().kind, Kind::UnknownSizes);
        assert!(out
            .last()
            .unwrap()
            .text
            .starts_with("1 AUR package is not counted"));
    }
}

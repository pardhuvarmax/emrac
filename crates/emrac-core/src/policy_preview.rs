//! What a plan does to files under your local file policy (`override`,
//! `divert`), told before it runs. See `docs/plan/r3/u8-override-divert.md`.

use serde::Serialize;

use crate::hooks::Operation;
use crate::policy::Policy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// pacman resets the mode and owner; emrac sets yours back afterwards.
    OverrideReapplied,
    /// pacman replaces the file; emrac puts yours back. The package's version
    /// of it is the one you diverted from, so nothing is missed.
    DivertKept,
    /// As above, but the package ships a different file than the one you
    /// diverted from: its changes, security fixes included, do NOT take effect.
    DivertMissesChanges,
    /// As above; whether the package changes the file is not known until it is
    /// downloaded.
    DivertMaybeMissesChanges,
    /// The package is removed; your copy waits in the store and comes back
    /// when the package is installed again.
    DivertWaitsInStore,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PolicyEffect {
    pub kind: &'static str,
    pub path: String,
    pub package: String,
    pub effect: Effect,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PolicyPreview {
    pub effects: Vec<PolicyEffect>,
    /// The hook that re-applies the policy is not in place (or may not run):
    /// the policy would NOT be re-applied after this transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_problem: Option<String>,
}

impl PolicyPreview {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.hook_problem.is_none()
    }

    /// Diversions whose package changes the file: the ones that need a decision.
    pub fn missed_changes(&self) -> impl Iterator<Item = &PolicyEffect> {
        self.effects.iter().filter(|e| {
            matches!(
                e.effect,
                Effect::DivertMissesChanges | Effect::DivertMaybeMissesChanges
            )
        })
    }
}

/// A package the plan touches and the files of it that matter (no leading `/`).
pub struct Touch {
    pub package: String,
    pub operation: Operation,
    pub files: Vec<String>,
}

fn names(files: &[String], path: &str) -> bool {
    let rel = path.trim_start_matches('/');
    files
        .iter()
        .any(|f| f == rel || f.strip_suffix('/') == Some(rel))
}

/// The effects of `touches` on `policy`. `package_sha(package, rel)` is the
/// sha256 of the file in the package that is about to be installed, when that
/// package is at hand.
pub fn effects(
    policy: &Policy,
    touches: &[Touch],
    package_sha: &dyn Fn(&str, &str) -> Option<String>,
) -> Vec<PolicyEffect> {
    let mut out = Vec::new();
    for touch in touches {
        for o in &policy.overrides {
            if names(&touch.files, &o.path) && touch.operation != Operation::Remove {
                out.push(PolicyEffect {
                    kind: "override",
                    path: o.path.clone(),
                    package: touch.package.clone(),
                    effect: Effect::OverrideReapplied,
                    text: format!(
                        "pacman resets its mode and owner; emrac sets {} back afterwards",
                        [
                            o.mode.as_ref().map(|m| format!("mode {m}")),
                            o.owner.as_ref().map(|u| format!("owner {u}")),
                            o.group.as_ref().map(|g| format!("group {g}")),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", ")
                    ),
                });
            }
        }
        for d in &policy.diverts {
            if !names(&touch.files, &d.path) {
                continue;
            }
            let (effect, text) = if touch.operation == Operation::Remove {
                (
                    Effect::DivertWaitsInStore,
                    "removed with the package; emrac puts your copy back as a file no package owns"
                        .to_string(),
                )
            } else {
                let rel = d.path.trim_start_matches('/');
                match (d.base.as_deref(), package_sha(&touch.package, rel)) {
                    (Some(base), Some(now)) if base == now => (
                        Effect::DivertKept,
                        "pacman replaces it with the version you diverted from; emrac puts yours back, nothing is missed".to_string(),
                    ),
                    (Some(_), Some(_)) => (
                        Effect::DivertMissesChanges,
                        "the new package ships a DIFFERENT version of this file than the one you diverted from: emrac keeps yours, so the package's changes, security fixes included, will NOT take effect".to_string(),
                    ),
                    _ => (
                        Effect::DivertMaybeMissesChanges,
                        "pacman replaces it and emrac puts yours back; if the new package changes the file, those changes, security fixes included, will NOT take effect (known once it is downloaded)".to_string(),
                    ),
                }
            };
            out.push(PolicyEffect {
                kind: "divert",
                path: d.path.clone(),
                package: touch.package.clone(),
                effect,
                text,
            });
        }
    }
    out.sort_by(|a, b| (&a.path, a.kind).cmp(&(&b.path, b.kind)));
    out.dedup_by(|a, b| a.path == b.path && a.kind == b.kind);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Divert, Override};

    fn policy() -> Policy {
        let mut d = Divert::new("/usr/lib/libfoo.so.1");
        d.base = Some("base".into());
        Policy {
            overrides: vec![Override {
                path: "/usr/bin/tool".into(),
                mode: Some("4750".into()),
                owner: Some("root".into()),
                group: None,
                note: None,
            }],
            diverts: vec![d],
        }
    }

    fn touch(op: Operation, files: &[&str]) -> Touch {
        Touch {
            package: "foo".into(),
            operation: op,
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn an_override_is_reapplied_on_install_and_upgrade_only() {
        let none = |_: &str, _: &str| None;
        let up = effects(
            &policy(),
            &[touch(Operation::Upgrade, &["usr/bin/tool"])],
            &none,
        );
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].effect, Effect::OverrideReapplied);
        assert!(up[0].text.contains("mode 4750") && up[0].text.contains("owner root"));
        assert!(effects(
            &policy(),
            &[touch(Operation::Remove, &["usr/bin/tool"])],
            &none
        )
        .is_empty());
        assert!(effects(
            &policy(),
            &[touch(Operation::Upgrade, &["usr/bin/other"])],
            &none
        )
        .is_empty());
    }

    #[test]
    fn a_diversion_says_whether_the_package_changes_the_file() {
        let files = &["usr/lib/libfoo.so.1"];
        let same = |_: &str, _: &str| Some("base".to_string());
        let other = |_: &str, _: &str| Some("new".to_string());
        let unknown = |_: &str, _: &str| None;
        let kept = effects(&policy(), &[touch(Operation::Upgrade, files)], &same);
        assert_eq!(kept[0].effect, Effect::DivertKept);
        let missed = effects(&policy(), &[touch(Operation::Upgrade, files)], &other);
        assert_eq!(missed[0].effect, Effect::DivertMissesChanges);
        assert!(
            missed[0].text.contains("security fixes") && missed[0].text.contains("NOT take effect")
        );
        let maybe = effects(&policy(), &[touch(Operation::Upgrade, files)], &unknown);
        assert_eq!(maybe[0].effect, Effect::DivertMaybeMissesChanges);
        let removed = effects(&policy(), &[touch(Operation::Remove, files)], &unknown);
        assert_eq!(removed[0].effect, Effect::DivertWaitsInStore);
    }

    #[test]
    fn a_diversion_without_a_base_is_never_called_harmless() {
        let mut p = policy();
        p.diverts[0].base = None;
        let same = |_: &str, _: &str| Some("base".to_string());
        let e = effects(
            &p,
            &[touch(Operation::Upgrade, &["usr/lib/libfoo.so.1"])],
            &same,
        );
        assert_eq!(e[0].effect, Effect::DivertMaybeMissesChanges);
    }

    #[test]
    fn only_changes_that_are_missed_need_a_decision() {
        let other = |_: &str, _: &str| Some("new".to_string());
        let preview = PolicyPreview {
            effects: effects(
                &policy(),
                &[touch(
                    Operation::Upgrade,
                    &["usr/lib/libfoo.so.1", "usr/bin/tool"],
                )],
                &other,
            ),
            hook_problem: None,
        };
        assert_eq!(preview.effects.len(), 2);
        assert_eq!(preview.missed_changes().count(), 1);
        assert!(PolicyPreview::default().is_empty());
        assert!(!PolicyPreview {
            effects: vec![],
            hook_problem: Some("x".into())
        }
        .is_empty());
    }
}

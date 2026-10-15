use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanAction {
    Install,
    Remove,
    Upgrade,
    /// Only produced by a rollback of an upgrade.
    Downgrade,
    /// The same version put back: repair, re-run install scripts.
    Reinstall,
    /// A removal that also discards modified configuration files.
    Purge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedPackage {
    pub name: String,
    pub version: String,
    pub repo: String,
    /// `true` if the user explicitly asked for this package; `false` if
    /// pacman is pulling it in as a dependency (install) or it's coming
    /// along via `--cascade`/`--recursive` (remove).
    pub explicit: bool,
    /// For upgrades and downgrades, the version installed before this
    /// transaction — what a rollback needs to go back to. `None` for
    /// installs/removes, and for records written before rollback support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
}

/// A package the transaction removes although nobody asked for it to be:
/// pacman removes it to resolve a conflict or because another package
/// replaces it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedRemoval {
    pub name: String,
    pub version: String,
    /// `conflicts with <pkg>`, `replaced by <pkg>`, …
    pub reason: String,
}

/// A dependency several packages can satisfy: pacman asks which one
/// ("Enter a number (default=1)"), and takes the first when run with `--yes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderChoice {
    /// What is being provided, as written in the dependency (`jack`).
    pub dependency: String,
    /// The candidates in pacman's order; the first is the default.
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub action: PlanAction,
    pub packages: Vec<PlannedPackage>,
    pub total_download_size: u64,
    /// Positive for install, negative for remove. Accounts for `removals`.
    pub total_installed_size_delta: i64,
    /// What conflict and replacement resolution will additionally remove,
    /// according to libalpm itself.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removals: Vec<PlannedRemoval>,
    /// Dependencies with more than one possible provider.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provider_choices: Vec<ProviderChoice>,
    /// Things the user should know about how far this plan can be trusted
    /// (e.g. the removals could not be worked out).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Digit grouping for plan ids: how many hex digits are shown.
pub const SHORT_DIGEST: usize = 12;

impl Plan {
    /// A fingerprint of what this plan does: the action, each package with
    /// its version, repository and previous version, the packages removed
    /// on the side, and the provider choices. Not the sizes (derived) and not
    /// the order (a plan is a set). Two plans with the same digest do the
    /// same thing; `emrac ... --expect-plan <digest>` runs only that one.
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut lines: Vec<String> = Vec::new();
        for p in &self.packages {
            lines.push(format!(
                "P\t{}\t{}\t{}\t{}",
                p.name,
                p.version,
                p.repo,
                p.previous_version.as_deref().unwrap_or("")
            ));
        }
        for r in &self.removals {
            lines.push(format!("R\t{}\t{}", r.name, r.version));
        }
        for c in &self.provider_choices {
            lines.push(format!("C\t{}\t{}", c.dependency, c.providers.join(",")));
        }
        lines.sort();
        let mut text = format!("emrac-plan-v1\t{:?}\n", self.action);
        for l in lines {
            text.push_str(&l);
            text.push('\n');
        }
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// The first digits of the digest, as shown.
    pub fn short_digest(&self) -> String {
        self.digest()[..SHORT_DIGEST].to_string()
    }

    /// What differs between this plan and `after`, one line each.
    pub fn changes_to(&self, after: &Plan) -> Vec<String> {
        let mut out = Vec::new();
        if self.action != after.action {
            out.push(format!("the action is now {:?}", after.action).to_lowercase());
        }
        let key = |p: &PlannedPackage| p.name.clone();
        for p in &after.packages {
            match self.packages.iter().find(|q| key(q) == key(p)) {
                None => out.push(format!("added: {}/{} {}", p.repo, p.name, p.version)),
                Some(q) if q.version != p.version || q.repo != p.repo => {
                    out.push(format!("changed: {} {} → {}", p.name, q.version, p.version))
                }
                Some(_) => {}
            }
        }
        for q in &self.packages {
            if !after.packages.iter().any(|p| key(p) == key(q)) {
                out.push(format!("dropped: {}/{} {}", q.repo, q.name, q.version));
            }
        }
        for r in &after.removals {
            if !self.removals.iter().any(|x| x.name == r.name) {
                out.push(format!("will now also remove: {} {}", r.name, r.version));
            }
        }
        for r in &self.removals {
            if !after.removals.iter().any(|x| x.name == r.name) {
                out.push(format!("no longer removes: {} {}", r.name, r.version));
            }
        }
        if out.is_empty() && self.digest() != after.digest() {
            out.push("a provider choice differs".to_string());
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

#[cfg(test)]
mod digest_tests {
    use super::*;

    fn pkg(name: &str, version: &str) -> PlannedPackage {
        PlannedPackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            explicit: true,
            previous_version: Some("0-1".into()),
        }
    }

    fn plan(action: PlanAction, packages: Vec<PlannedPackage>) -> Plan {
        Plan {
            action,
            packages,
            total_download_size: 1,
            total_installed_size_delta: 2,
            removals: vec![],
            provider_choices: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn the_digest_ignores_order_sizes_and_warnings_and_sees_everything_else() {
        let a = plan(PlanAction::Upgrade, vec![pkg("a", "2-1"), pkg("b", "3-1")]);
        let mut b = plan(PlanAction::Upgrade, vec![pkg("b", "3-1"), pkg("a", "2-1")]);
        b.total_download_size = 999;
        b.warnings.push("x".into());
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.digest().len(), 64);
        assert_eq!(a.short_digest(), a.digest()[..12]);
        let version = plan(PlanAction::Upgrade, vec![pkg("a", "2-2"), pkg("b", "3-1")]);
        assert_ne!(a.digest(), version.digest());
        let action = plan(PlanAction::Install, vec![pkg("a", "2-1"), pkg("b", "3-1")]);
        assert_ne!(a.digest(), action.digest());
        let mut removal = a.clone();
        removal.removals.push(PlannedRemoval {
            name: "old".into(),
            version: "1".into(),
            reason: "conflicts".into(),
        });
        assert_ne!(a.digest(), removal.digest());
        let mut choice = a.clone();
        choice.provider_choices.push(ProviderChoice {
            dependency: "jack".into(),
            providers: vec!["x".into(), "y".into()],
        });
        assert_ne!(a.digest(), choice.digest());
        let mut repo = a.clone();
        repo.packages[0].repo = "core".into();
        assert_ne!(a.digest(), repo.digest());
    }

    #[test]
    fn the_changes_between_two_plans_are_listed() {
        let before = plan(PlanAction::Upgrade, vec![pkg("a", "2-1"), pkg("b", "3-1")]);
        let mut after = plan(PlanAction::Upgrade, vec![pkg("a", "2-2"), pkg("c", "1-1")]);
        after.removals.push(PlannedRemoval {
            name: "old".into(),
            version: "1".into(),
            reason: "x".into(),
        });
        let c = before.changes_to(&after);
        assert!(c.contains(&"changed: a 2-1 → 2-2".to_string()), "{c:?}");
        assert!(c.contains(&"added: extra/c 1-1".to_string()));
        assert!(c.contains(&"dropped: extra/b 3-1".to_string()));
        assert!(c.contains(&"will now also remove: old 1".to_string()));
        assert!(before.changes_to(&before).is_empty());
    }
}

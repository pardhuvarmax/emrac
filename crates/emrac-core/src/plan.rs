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

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

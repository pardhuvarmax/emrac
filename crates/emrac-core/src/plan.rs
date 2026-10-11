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

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub action: PlanAction,
    pub packages: Vec<PlannedPackage>,
    pub total_download_size: u64,
    /// Positive for install, negative for remove.
    pub total_installed_size_delta: i64,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

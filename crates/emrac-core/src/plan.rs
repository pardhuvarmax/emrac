use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanAction {
    Install,
    Remove,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedPackage {
    pub name: String,
    pub version: String,
    pub repo: String,
    /// `true` if the user explicitly asked for this package; `false` if
    /// pacman is pulling it in as a dependency (install) or it's coming
    /// along via `--cascade`/`--recursive` (remove).
    pub explicit: bool,
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

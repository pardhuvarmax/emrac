//! What `emrac purge` will destroy that `remove` would have kept: pacman
//! saves a removed package's *modified* `backup=()` files as `.pacsave`;
//! purge (`pacman -Rn`) deletes them. This module classifies those files.

use serde::Serialize;

use crate::plan::Plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigState {
    /// Same as the package shipped it; removed with the package either way.
    Unmodified,
    /// Changed locally: `remove` keeps it as `.pacsave`, `purge` deletes it.
    Modified,
    /// Already gone.
    Missing,
    /// Couldn't be read (often root-only), so it may be modified.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFile {
    pub package: String,
    pub path: String,
    pub state: ConfigState,
}

impl ConfigFile {
    /// Would purging lose data that `remove` would have kept?
    pub fn at_risk(&self) -> bool {
        matches!(self.state, ConfigState::Modified | ConfigState::Unknown)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgePlan {
    pub plan: Plan,
    pub config_files: Vec<ConfigFile>,
}

impl PurgePlan {
    pub fn at_risk(&self) -> Vec<&ConfigFile> {
        self.config_files.iter().filter(|f| f.at_risk()).collect()
    }
}

/// What looking at a config file on disk turned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observed {
    Missing,
    Hash(String),
    Unreadable,
}

pub(crate) fn classify(expected_hash: &str, observed: &Observed) -> ConfigState {
    match observed {
        Observed::Missing => ConfigState::Missing,
        Observed::Unreadable => ConfigState::Unknown,
        Observed::Hash(actual) if actual == expected_hash => ConfigState::Unmodified,
        Observed::Hash(_) => ConfigState::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        let h = |s: &str| Observed::Hash(s.to_string());
        assert_eq!(classify("abc", &h("abc")), ConfigState::Unmodified);
        assert_eq!(classify("abc", &h("def")), ConfigState::Modified);
        assert_eq!(classify("abc", &Observed::Missing), ConfigState::Missing);
        assert_eq!(classify("abc", &Observed::Unreadable), ConfigState::Unknown);
    }

    #[test]
    fn only_modified_and_unknown_are_at_risk() {
        let file = |state| ConfigFile {
            package: "p".into(),
            path: "/etc/p.conf".into(),
            state,
        };
        assert!(!file(ConfigState::Unmodified).at_risk());
        assert!(!file(ConfigState::Missing).at_risk());
        assert!(file(ConfigState::Modified).at_risk());
        assert!(file(ConfigState::Unknown).at_risk());
    }
}

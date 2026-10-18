//! What a plan will do beyond its package list, worked out before it runs and
//! shown under it: the pacman hooks it triggers and the config files it will
//! leave beside your edits. None of it is part of the plan or its id, because
//! it depends on the system, not on what the plan does.

use serde::Serialize;

use crate::config_preview::ConfigPreview;
use crate::hooks::HookPreview;
use crate::policy_preview::PolicyPreview;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PlanPreview {
    #[serde(skip_serializing_if = "HookPreview::is_empty")]
    pub hooks: HookPreview,
    #[serde(skip_serializing_if = "ConfigPreview::is_empty")]
    pub config: ConfigPreview,
    #[serde(skip_serializing_if = "PolicyPreview::is_empty")]
    pub policy: PolicyPreview,
}

impl PlanPreview {
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty() && self.config.is_empty() && self.policy.is_empty()
    }
}

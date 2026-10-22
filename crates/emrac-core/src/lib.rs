pub mod advisor;
pub mod arch_news;
mod audit;
pub mod aur_trust;
mod backend;
pub mod benchmark;
pub mod bisect;
mod bug_report;
pub mod build_stats;
pub mod build_tools;
pub mod build_ui;
pub mod bundle;
mod cache;
mod config;
pub mod config_preview;
mod config_style;
mod deps;
mod doctor;
mod downgrade;
mod error;
pub mod eta;
pub mod exchange;
pub mod fault;
pub mod graph_render;
mod history;
mod history_log;
pub mod hook_watch;
pub mod hooks;
pub mod install_mode;
pub mod journal;
pub mod ledger;
pub mod ledger_watch;
pub mod linkcheck;
mod manifest;
pub mod mirrors;
mod package;
pub mod pacman_db;
pub mod pacman_run;
pub mod pacman_ui;
mod pacnew;
mod pacnew_log;
mod pacnew_ops;
mod pins;
mod plan;
pub mod plan_preview;
pub mod policy;
pub mod policy_ops;
pub mod policy_preview;
pub mod preflight;
pub mod protected;
mod purge;
pub mod rebuild_ack;
pub mod recipe_digest;
pub mod recommend;
pub mod reconcile;
mod reinstall;
pub mod repo_trust;
mod rollback;
mod sandbox;
pub mod score;
pub mod score_more;
pub mod score_sources;
pub mod security_tracker;
pub mod self_check;
pub mod self_meta;
pub mod self_ops;
pub mod self_upgrade;
mod snapshot;
pub mod source_built;
mod sources;
pub mod stale_builds;
pub mod strays;
pub mod style;
pub mod timeline;
pub mod upgrade_report;
mod verify;
pub mod voice;

pub use audit::{
    audit, scriptlet_name, AuditReport, ExtraFile, Finding, Severity, Verdict, MAX_EXTRA_FILES,
};
pub use backend::version_is_newer;

/// The version of libalpm emrac is linked with.
pub fn libalpm_version() -> &'static str {
    alpm::version()
}
pub use backend::AurSyncOutcome;
pub use backend::BuildOptions;
pub use cache::{
    parse_duration, parse_size, Decision as CacheDecision, Op as CacheOp, Outcome as CacheOutcome,
    Scope as CacheScope,
};
pub use config::{BuildConfig, Config, SafetyConfig, SelfConfig};
pub use deps::{
    DepEntry, DepsReport, Edge, GraphReport, ImpactItem, Orphan, RdepsReport, RemoveImpactReport,
    TreeLine, TreeReport, UnitFact, WhyReport,
};
pub use doctor::{Check as DoctorCheck, DoctorReport, Status as DoctorStatus};
pub use downgrade::{DowngradePlan, ListedVersion, VersionList};
pub use error::{Error, Result};
pub use history::{
    format_utc, format_utc_date, parse_date, ArchiveInfo, Break as HistoryBreak, History,
    TransactionRecord, TxStatus, VerifyReport as HistoryVerify, RECENT_KEPT,
};
pub use history_log::{AnchorCheck, ArchivePlan};
pub use install_mode::SourceMode;
pub use manifest::Manifest;
pub use package::{PackageDetails, PackageSummary};
pub use pacnew::unified_diff;
pub use pacnew::{has_conflict_markers, merge3_markers, Validation as PacnewValidation};
pub use pacnew::{Class as PacnewClass, Merge as PacnewMerge, Pending as PacnewFile};
pub use pacnew_log::{Record as PacnewRecord, Standing as PacnewStanding, State as PacnewState};
pub use pacnew_ops::{Applied as PacnewApplied, PacnewAction, Reverted as PacnewReverted};
pub use pins::{Pin, PinStore};
pub use plan::{Plan, PlanAction, PlannedPackage, PlannedRemoval, ProviderChoice, SHORT_DIGEST};
pub use purge::{ConfigFile, ConfigState, PurgePlan};
pub use reinstall::{ReinstallItem, ReinstallPlan, ReinstallSource};
pub use rollback::{parse_tx_id, rolled_back_by, Download, RollbackPlan, SkippedPackage};
pub use sandbox::Limits;
pub use snapshot::{
    RestoreDiff, RestoreEntry, RestoreItem, RestorePlan, RestoreSource, Snapshot, SnapshotPackage,
    SnapshotStore,
};
pub use sources::{
    audit_path, inspect_package, is_package_file_name, package_list_state, ArchSource,
    BenchmarkTargets, LocalInstall, OfficialInspection, PackageList, SearchResults, SearchScope,
    Sources, UpgradePlan,
};
pub use verify::{EntryKind, Issue, IssueKind, PackageReport, VerifyReport};
pub use voice::Voice;

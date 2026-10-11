mod backend;
mod config;
mod downgrade;
mod error;
mod history;
mod package;
mod plan;
mod purge;
mod reinstall;
mod rollback;
mod snapshot;
mod sources;
mod verify;

pub use backend::AurSyncOutcome;
pub use config::Config;
pub use downgrade::{DowngradePlan, ListedVersion, VersionList};
pub use error::{Error, Result};
pub use history::{parse_date, History, TransactionRecord, TxStatus};
pub use package::{PackageDetails, PackageSummary};
pub use plan::{Plan, PlanAction, PlannedPackage};
pub use purge::{ConfigFile, ConfigState, PurgePlan};
pub use reinstall::{ReinstallItem, ReinstallPlan, ReinstallSource};
pub use rollback::{rolled_back_by, Download, RollbackPlan, SkippedPackage};
pub use snapshot::{
    RestoreDiff, RestoreEntry, RestoreItem, RestorePlan, RestoreSource, Snapshot, SnapshotPackage,
    SnapshotStore,
};
pub use sources::{SearchResults, SearchScope, Sources, UpgradePlan};
pub use verify::{EntryKind, Issue, IssueKind, PackageReport, VerifyReport};

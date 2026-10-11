mod backend;
mod config;
mod error;
mod history;
mod package;
mod plan;
mod rollback;
mod sources;

pub use backend::AurSyncOutcome;
pub use config::Config;
pub use error::{Error, Result};
pub use history::{parse_date, History, TransactionRecord, TxStatus};
pub use package::{PackageDetails, PackageSummary};
pub use plan::{Plan, PlanAction, PlannedPackage};
pub use rollback::{rolled_back_by, RollbackPlan, SkippedPackage};
pub use sources::{SearchResults, SearchScope, Sources, UpgradePlan};

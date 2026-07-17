mod backend;
mod error;
mod package;
mod plan;
mod sources;

pub use backend::AurSyncOutcome;
pub use error::{Error, Result};
pub use package::{PackageDetails, PackageSummary};
pub use plan::{Plan, PlanAction, PlannedPackage};
pub use sources::{SearchResults, SearchScope, Sources};

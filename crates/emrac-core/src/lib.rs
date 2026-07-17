mod backend;
mod error;
mod package;
mod sources;

pub use error::{Error, Result};
pub use package::{PackageDetails, PackageSummary};
pub use sources::{SearchResults, SearchScope, Sources};

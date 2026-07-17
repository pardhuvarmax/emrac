mod backend;
mod error;
mod package;

pub use backend::AlpmBackend;
pub use error::{Error, Result};
pub use package::{PackageDetails, PackageSummary};

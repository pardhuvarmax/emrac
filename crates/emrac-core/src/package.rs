use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PackageSummary {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageDetails {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub description: Option<String>,
    pub license: Vec<String>,
    pub url: Option<String>,
    pub depends: Vec<String>,
    pub provides: Vec<String>,
    /// Declared installed size in bytes. `None` for AUR packages, which
    /// carry no such metadata until actually built.
    pub installed_size: Option<u64>,
    /// AUR-only metadata. `None` for official-repo packages, whose sync db
    /// carries none of this.
    pub maintainer: Option<String>,
    pub votes: Option<u32>,
    pub popularity: Option<f64>,
    /// Unix timestamp the package was flagged out-of-date, if it is.
    pub out_of_date: Option<i64>,
}

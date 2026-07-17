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
    pub installed_size: u64,
}

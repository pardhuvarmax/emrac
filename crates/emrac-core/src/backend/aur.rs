use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::package::{PackageDetails, PackageSummary};

const BASE_URL: &str = "https://aur.archlinux.org/rpc/";
const USER_AGENT: &str = concat!(
    "emrac/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pardhuvarmax/emrac)"
);
const TIMEOUT: Duration = Duration::from_secs(5);

/// AUR access via the aurweb RPC v5 interface. Read-only: this only ever
/// issues GET requests, never mutates anything.
pub struct AurBackend;

impl AurBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn search(&self, query: &str) -> Result<Vec<PackageSummary>> {
        let response: RpcResponse<AurSearchResult> =
            self.request(&[("v", "5"), ("type", "search"), ("arg", query)])?;

        Ok(response
            .results
            .into_iter()
            .map(|r| PackageSummary {
                name: r.name,
                version: r.version,
                repo: "aur".to_string(),
                description: r.description,
            })
            .collect())
    }

    /// `Ok(None)` means "not in the AUR", distinct from `Err` which means
    /// the request itself failed (network, timeout, malformed response).
    pub fn info(&self, name: &str) -> Result<Option<PackageDetails>> {
        Ok(self.info_multi(&[name.to_string()])?.into_iter().next())
    }

    /// Batched `info` lookup — one request, `results` in whatever order the
    /// AUR returns them (not necessarily matching `names`' order, and
    /// silently omitting anything not found). Used for checking many
    /// installed AUR packages' latest versions at once during `upgrade`.
    pub fn info_multi(&self, names: &[String]) -> Result<Vec<PackageDetails>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }

        let mut params: Vec<(&str, &str)> = vec![("v", "5"), ("type", "info")];
        params.extend(names.iter().map(|name| ("arg[]", name.as_str())));

        let response: RpcResponse<AurInfoResult> = self.request(&params)?;

        Ok(response
            .results
            .into_iter()
            .map(|r| PackageDetails {
                name: r.name,
                version: r.version,
                repo: "aur".to_string(),
                description: r.description,
                license: r.license,
                url: r.url,
                depends: r.depends,
                // The v5 RPC schema doesn't expose Provides/Conflicts/Replaces.
                provides: Vec::new(),
                installed_size: None,
                maintainer: r.maintainer,
                votes: Some(r.num_votes),
                popularity: Some(r.popularity),
                out_of_date: r.out_of_date,
            })
            .collect())
    }

    fn request<T>(&self, params: &[(&str, &str)]) -> Result<RpcResponse<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut req = ureq::get(BASE_URL)
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_global(Some(TIMEOUT))
            .build();

        for (key, value) in params {
            req = req.query(*key, *value);
        }

        let mut response = req
            .call()
            .map_err(|err| Error::Aur(format!("request to the AUR failed: {err}")))?;

        let body: RpcResponse<T> = response
            .body_mut()
            .read_json()
            .map_err(|err| Error::Aur(format!("couldn't parse the AUR's response: {err}")))?;

        if body.kind == "error" {
            let message = body.error.unwrap_or_else(|| "unknown error".to_string());
            return Err(Error::Aur(message));
        }

        Ok(body)
    }
}

impl Default for AurBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    #[serde(rename = "type")]
    kind: String,
    error: Option<String>,
    results: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct AurSearchResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AurInfoResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "License", default)]
    license: Vec<String>,
    #[serde(rename = "Depends", default)]
    depends: Vec<String>,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "NumVotes")]
    num_votes: u32,
    #[serde(rename = "Popularity")]
    popularity: f64,
    #[serde(rename = "OutOfDate")]
    out_of_date: Option<i64>,
}

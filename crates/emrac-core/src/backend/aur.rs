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
const TIMEOUT: Duration = Duration::from_secs(10);
/// Tries after the first for a transient failure, with a growing pause.
const RETRIES: u32 = 2;

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

    /// The AUR git repo a package is built from. A split package such as
    /// `stm32cubemx-jre` lives in its base's repo (`stm32cubemx`); cloning
    /// it under its own name gives an empty repository.
    pub fn package_base(&self, name: &str) -> Result<Option<String>> {
        let params = [("v", "5"), ("type", "info"), ("arg[]", name)];
        let response: RpcResponse<AurBaseResult> = self.request(&params)?;
        Ok(response
            .results
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| r.package_base))
    }

    fn request<T>(&self, params: &[(&str, &str)]) -> Result<RpcResponse<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut attempt = 0;
        let mut response = loop {
            let mut req = ureq::get(BASE_URL)
                .header("User-Agent", USER_AGENT)
                .config()
                .timeout_global(Some(TIMEOUT))
                .build();
            for (key, value) in params {
                req = req.query(*key, *value);
            }
            match req.call() {
                Ok(response) => break response,
                Err(err) if attempt < RETRIES && is_transient(&err) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(500 * u64::from(attempt)));
                }
                Err(err) => {
                    return Err(Error::Aur(format!("request to the AUR failed: {err}")));
                }
            }
        };

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

/// Worth another try: the network hiccuped or the server had a bad moment.
/// A 4xx (we asked wrongly) or a malformed answer is not.
fn is_transient(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::StatusCode(code) => *code >= 500 || *code == 429,
        ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::ConnectionFailed
        | ureq::Error::HostNotFound => true,
        _ => false,
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
struct AurBaseResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "PackageBase")]
    package_base: String,
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

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn package_base_is_read_from_the_rpc_answer() {
        let body = r#"{"type":"multiinfo","results":[{"Name":"x-jre","PackageBase":"x"}]}"#;
        let parsed: RpcResponse<AurBaseResult> = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.results[0].name, "x-jre");
        assert_eq!(parsed.results[0].package_base, "x");
    }

    #[test]
    fn only_transient_failures_are_retried() {
        assert!(is_transient(&ureq::Error::Timeout(ureq::Timeout::Global)));
        assert!(is_transient(&ureq::Error::StatusCode(503)));
        assert!(is_transient(&ureq::Error::StatusCode(429)));
        assert!(is_transient(&ureq::Error::ConnectionFailed));
        assert!(is_transient(&ureq::Error::HostNotFound));
        assert!(!is_transient(&ureq::Error::StatusCode(404)));
        assert!(!is_transient(&ureq::Error::StatusCode(400)));
        assert!(!is_transient(&ureq::Error::BadUri("x".into())));
    }
}

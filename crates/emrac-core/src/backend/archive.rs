use std::env;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::rollback::{is_package_file, package_file_version};

const BASE_URL: &str = "https://archive.archlinux.org/packages";
const USER_AGENT: &str = concat!(
    "emrac/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pardhuvarmax/emrac)"
);
const LISTING_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_LISTING_BYTES: u64 = 64 * 1024 * 1024;

/// A package file located in the Archive, not yet downloaded.
pub struct ArchiveFile {
    pub url: String,
    pub filename: String,
}

/// Arch Linux Archive access (`archive.archlinux.org`), which keeps every
/// official package version ever shipped. The only module that talks to it.
/// Planning only ever does read-only listing GETs; `download` is the one
/// call that writes, and the caller invokes it after the user confirmed.
pub struct ArchiveBackend;

impl ArchiveBackend {
    pub fn new() -> Self {
        Self
    }

    /// Where downloads for `pkg` are kept: `~/.cache/emrac/archive/<pkg>`.
    pub fn cache_dir(&self, pkg: &str) -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoHomeDir)?;
        Ok(PathBuf::from(home).join(".cache/emrac/archive").join(pkg))
    }

    /// Finds the exact `name`/`version` file in the Archive. `Ok(None)`
    /// means the Archive doesn't have it (including a package it has never
    /// heard of); `Err` means it couldn't be asked.
    pub fn find(&self, name: &str, version: &str) -> Result<Option<ArchiveFile>> {
        let Some((dir_url, hrefs)) = self.listing(name)? else {
            return Ok(None);
        };
        Ok(
            pick_file(&hrefs, name, version).map(|filename| ArchiveFile {
                url: format!("{dir_url}{}", percent_encode(&filename)),
                filename,
            }),
        )
    }

    /// Every version of `name` the Archive has (unordered, deduplicated).
    /// `Ok(None)` if the Archive has never heard of the package.
    pub fn versions(&self, name: &str) -> Result<Option<Vec<String>>> {
        let Some((_, hrefs)) = self.listing(name)? else {
            return Ok(None);
        };
        let mut versions: Vec<String> = Vec::new();
        for href in &hrefs {
            if let Some(version) = package_file_version(href, name) {
                if !versions.contains(&version) {
                    versions.push(version);
                }
            }
        }
        Ok(Some(versions))
    }

    /// The directory URL and the (decoded) file names in it, or `None` on a
    /// 404.
    fn listing(&self, name: &str) -> Result<Option<(String, Vec<String>)>> {
        let Some(dir_url) = package_dir_url(name) else {
            return Ok(None);
        };

        let mut response = match ureq::get(&dir_url)
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_global(Some(LISTING_TIMEOUT))
            .build()
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(err) => {
                return Err(Error::Archive(format!(
                    "couldn't list {name} in the Arch Linux Archive: {err}"
                )));
            }
        };

        let html = response
            .body_mut()
            .with_config()
            .limit(MAX_LISTING_BYTES)
            .read_to_string()
            .map_err(|err| {
                Error::Archive(format!(
                    "couldn't read the Archive's listing for {name}: {err}"
                ))
            })?;

        Ok(Some((dir_url, listing_hrefs(&html))))
    }

    /// Downloads `url` to `dest` via `<dest>.part`, renamed on success so an
    /// interrupted download is never mistaken for a package.
    pub fn download(&self, url: &str, dest: &Path) -> Result<()> {
        let io_err = |what: &str, err: io::Error| Error::Io {
            context: format!("{what} {}", dest.display()),
            source: err,
        };

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err("couldn't create the folder for", e))?;
        }

        let mut response = ureq::get(url)
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .build()
            .call()
            .map_err(|err| Error::Archive(format!("couldn't download {url}: {err}")))?;

        let part = PathBuf::from(format!("{}.part", dest.display()));
        let result = (|| {
            let mut file = File::create(&part).map_err(|e| io_err("couldn't write", e))?;
            io::copy(&mut response.body_mut().as_reader(), &mut file)
                .map_err(|e| Error::Archive(format!("download of {url} was interrupted: {e}")))?;
            fs::rename(&part, dest).map_err(|e| io_err("couldn't finish writing", e))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&part);
        }
        result
    }
}

impl Default for ArchiveBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// `https://archive.archlinux.org/packages/<first char>/<name>/`
fn package_dir_url(name: &str) -> Option<String> {
    let first = name.chars().next()?;
    // Package names are `[a-z0-9@._+-]`; refuse anything that could alter
    // the URL path.
    if name.contains(['/', '?', '#', '%', ' ']) || name.starts_with('.') {
        return None;
    }
    Some(format!("{BASE_URL}/{first}/{}/", percent_encode(name)))
}

/// Every `href="..."` value in an HTML listing, percent-decoded.
pub(crate) fn listing_hrefs(html: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("href=\"") {
        rest = &rest[start + 6..];
        let Some(end) = rest.find('"') else { break };
        hrefs.push(percent_decode(&rest[..end]));
        rest = &rest[end..];
    }
    hrefs
}

/// The exact package file for `name`/`version` among `files`, preferring
/// `.zst`. Same shape matcher as the local cache scan.
pub(crate) fn pick_file(files: &[String], name: &str, version: &str) -> Option<String> {
    let prefix = format!("{name}-{version}-");
    files
        .iter()
        .filter(|f| is_package_file(f, &prefix))
        .min_by_key(|f| (!f.ends_with(".zst"), f.as_str()))
        .cloned()
}

pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes everything but unreserved characters, so `:` and `+` in names and
/// versions survive in a URL path.
pub(crate) fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs_are_extracted_and_decoded() {
        let html = r#"<a href="../">../</a>
<a href="alsa-card-profiles-1%3A0.3.20-1-x86_64.pkg.tar.zst">x</a>
<a href="gtk%2B-1-1-x86_64.pkg.tar.zst.sig">y</a> <a href="broken"#;
        assert_eq!(
            listing_hrefs(html),
            [
                "../",
                "alsa-card-profiles-1:0.3.20-1-x86_64.pkg.tar.zst",
                "gtk+-1-1-x86_64.pkg.tar.zst.sig"
            ]
        );
    }

    #[test]
    fn percent_roundtrip_and_malformed_escapes() {
        assert_eq!(percent_decode("a%3Ab%2Bc"), "a:b+c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_encode("1:2.0-1+x"), "1%3A2.0-1%2Bx");
        for s in ["plain-1.0-1", "1:2.0-1", "gtk+", "a b/c"] {
            assert_eq!(percent_decode(&percent_encode(s)), s);
        }
    }

    #[test]
    fn pick_file_is_exact_and_prefers_zst() {
        let files: Vec<String> = [
            "foo-1.0-1-x86_64.pkg.tar.xz",
            "foo-1.0-1-x86_64.pkg.tar.zst",
            "foo-1.0-1-x86_64.pkg.tar.zst.sig",
            "foo-bar-1.0-1-x86_64.pkg.tar.zst",
            "foo-1.0-10-x86_64.pkg.tar.zst",
            "foo-1:2.0-1-any.pkg.tar.zst",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            pick_file(&files, "foo", "1.0-1").as_deref(),
            Some("foo-1.0-1-x86_64.pkg.tar.zst")
        );
        assert_eq!(
            pick_file(&files, "foo", "1:2.0-1").as_deref(),
            Some("foo-1:2.0-1-any.pkg.tar.zst")
        );
        assert!(pick_file(&files, "foo", "9.9-9").is_none());
        assert!(pick_file(&files, "fo", "1.0-1").is_none());
    }

    #[test]
    fn xz_is_used_when_there_is_no_zst() {
        let files = vec!["foo-1.0-1-x86_64.pkg.tar.xz".to_string()];
        assert_eq!(
            pick_file(&files, "foo", "1.0-1").as_deref(),
            Some("foo-1.0-1-x86_64.pkg.tar.xz")
        );
    }

    #[test]
    fn directory_urls() {
        assert_eq!(
            package_dir_url("tree").as_deref(),
            Some("https://archive.archlinux.org/packages/t/tree/")
        );
        assert_eq!(
            package_dir_url("gtk+").as_deref(),
            Some("https://archive.archlinux.org/packages/g/gtk%2B/")
        );
        assert!(package_dir_url("").is_none());
        assert!(package_dir_url("../etc").is_none());
        assert!(package_dir_url("a/b").is_none());
    }
}

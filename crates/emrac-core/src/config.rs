use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Persisted defaults for flags that already exist on the CLI today. A
/// config value only ever raises the effective default — see
/// `emrac-cli`'s `main.rs`, which OR-merges each of these against the
/// matching CLI flag before dispatch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub flags: Flags,
    #[serde(default)]
    pub review: Review,
    #[serde(default)]
    pub search: Search,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flags {
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub yes: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub quiet: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Review {
    #[serde(default)]
    pub skip_pkgbuild: bool,
    #[serde(default)]
    pub skip_diff: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Search {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub aur: bool,
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoConfigDir)?;
        Ok(PathBuf::from(home).join(".config/emrac/config.toml"))
    }

    /// A missing file is just an all-defaults config, not an error — every
    /// command already works with zero config today, and that has to stay
    /// true. A file that exists but doesn't parse is an error.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(Error::Io {
                    context: format!("couldn't read your config at {}", path.display()),
                    source: err,
                });
            }
        };
        toml::from_str(&raw).map_err(|err| Error::ConfigParse {
            path: path.display().to_string(),
            source: Box::new(err),
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| Error::ConfigWrite {
                path: path.display().to_string(),
                source: err,
            })?;
        }
        fs::write(&path, self.to_pretty_string()).map_err(|err| Error::ConfigWrite {
            path: path.display().to_string(),
            source: err,
        })
    }

    pub fn to_pretty_string(&self) -> String {
        toml::to_string_pretty(self).expect("Config is always serializable")
    }

    pub fn get(&self, key: &str) -> Result<String> {
        Ok(match key {
            "flags.offline" => self.flags.offline.to_string(),
            "flags.yes" => self.flags.yes.to_string(),
            "flags.dry_run" => self.flags.dry_run.to_string(),
            "flags.json" => self.flags.json.to_string(),
            "flags.quiet" => self.flags.quiet.to_string(),
            "review.skip_pkgbuild" => self.review.skip_pkgbuild.to_string(),
            "review.skip_diff" => self.review.skip_diff.to_string(),
            "search.limit" => match self.search.limit {
                Some(n) => n.to_string(),
                None => "unset".to_string(),
            },
            "search.official" => self.search.official.to_string(),
            "search.aur" => self.search.aur.to_string(),
            _ => return Err(Error::UnknownConfigKey(key.to_string())),
        })
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let invalid = || Error::InvalidConfigValue {
            key: key.to_string(),
            value: value.to_string(),
        };
        match key {
            "flags.offline" => self.flags.offline = parse_bool(value).ok_or_else(invalid)?,
            "flags.yes" => self.flags.yes = parse_bool(value).ok_or_else(invalid)?,
            "flags.dry_run" => self.flags.dry_run = parse_bool(value).ok_or_else(invalid)?,
            "flags.json" => self.flags.json = parse_bool(value).ok_or_else(invalid)?,
            "flags.quiet" => self.flags.quiet = parse_bool(value).ok_or_else(invalid)?,
            "review.skip_pkgbuild" => {
                self.review.skip_pkgbuild = parse_bool(value).ok_or_else(invalid)?;
            }
            "review.skip_diff" => {
                self.review.skip_diff = parse_bool(value).ok_or_else(invalid)?;
            }
            "search.limit" => {
                self.search.limit = match value {
                    "unset" | "none" => None,
                    n => Some(n.parse::<usize>().map_err(|_| invalid())?),
                };
            }
            "search.official" => self.search.official = parse_bool(value).ok_or_else(invalid)?,
            "search.aur" => self.search.aur = parse_bool(value).ok_or_else(invalid)?,
            _ => return Err(Error::UnknownConfigKey(key.to_string())),
        }
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<()> {
        match key {
            "flags.offline" => self.flags.offline = false,
            "flags.yes" => self.flags.yes = false,
            "flags.dry_run" => self.flags.dry_run = false,
            "flags.json" => self.flags.json = false,
            "flags.quiet" => self.flags.quiet = false,
            "review.skip_pkgbuild" => self.review.skip_pkgbuild = false,
            "review.skip_diff" => self.review.skip_diff = false,
            "search.limit" => self.search.limit = None,
            "search.official" => self.search.official = false,
            "search.aur" => self.search.aur = false,
            _ => return Err(Error::UnknownConfigKey(key.to_string())),
        }
        Ok(())
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

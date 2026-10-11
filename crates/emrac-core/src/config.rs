use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
        Self::load_from(&Self::path()?)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
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

    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| Error::ConfigWrite {
                path: path.display().to_string(),
                source: err,
            })?;
        }
        fs::write(path, self.to_pretty_string()).map_err(|err| Error::ConfigWrite {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test scratch dir under the system temp dir, removed on drop.
    /// Tests go through `load_from`/`save_to` with explicit paths — never
    /// `$HOME` — so they can run in parallel without racing on process env.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                env::temp_dir().join(format!("emrac-config-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join(".config/emrac/config.toml")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const KEYS: [&str; 10] = [
        "flags.offline",
        "flags.yes",
        "flags.dry_run",
        "flags.json",
        "flags.quiet",
        "review.skip_pkgbuild",
        "review.skip_diff",
        "search.limit",
        "search.official",
        "search.aur",
    ];

    #[test]
    fn defaults_are_all_off() {
        let config = Config::default();
        for key in KEYS {
            let expected = if key == "search.limit" {
                "unset"
            } else {
                "false"
            };
            assert_eq!(config.get(key).unwrap(), expected, "{key}");
        }
    }

    #[test]
    fn set_get_unset_roundtrip_for_every_bool_key() {
        for key in KEYS.iter().filter(|k| **k != "search.limit") {
            let mut config = Config::default();
            config.set(key, "true").unwrap();
            assert_eq!(config.get(key).unwrap(), "true", "{key}");
            // Setting one key must not flip any other.
            for other in KEYS.iter().filter(|k| *k != key && **k != "search.limit") {
                assert_eq!(config.get(other).unwrap(), "false", "{other} after {key}");
            }
            config.unset(key).unwrap();
            assert_eq!(config.get(key).unwrap(), "false", "{key}");
        }
    }

    #[test]
    fn search_limit_accepts_numbers_and_unset() {
        let mut config = Config::default();
        config.set("search.limit", "5").unwrap();
        assert_eq!(config.search.limit, Some(5));
        config.set("search.limit", "unset").unwrap();
        assert_eq!(config.search.limit, None);
        config.set("search.limit", "7").unwrap();
        config.set("search.limit", "none").unwrap();
        assert_eq!(config.search.limit, None);
        config.set("search.limit", "3").unwrap();
        config.unset("search.limit").unwrap();
        assert_eq!(config.get("search.limit").unwrap(), "unset");
    }

    #[test]
    fn invalid_values_are_rejected_and_leave_config_unchanged() {
        let mut config = Config::default();
        for (key, value) in [
            ("flags.offline", "nonsense"),
            ("flags.offline", "True"),
            ("flags.offline", "1"),
            ("search.limit", "-1"),
            ("search.limit", "abc"),
            ("search.limit", "1.5"),
        ] {
            let err = config.set(key, value).unwrap_err();
            assert!(
                matches!(err, Error::InvalidConfigValue { .. }),
                "{key}={value}"
            );
        }
        assert_eq!(
            config.to_pretty_string(),
            Config::default().to_pretty_string()
        );
    }

    #[test]
    fn unknown_keys_error_on_get_set_and_unset() {
        let mut config = Config::default();
        assert!(matches!(
            config.get("nope"),
            Err(Error::UnknownConfigKey(_))
        ));
        assert!(matches!(
            config.set("nope", "true"),
            Err(Error::UnknownConfigKey(_))
        ));
        assert!(matches!(
            config.unset("nope"),
            Err(Error::UnknownConfigKey(_))
        ));
        // Table name alone isn't a key.
        assert!(matches!(
            config.get("flags"),
            Err(Error::UnknownConfigKey(_))
        ));
    }

    #[test]
    fn toml_roundtrip_preserves_values() {
        let mut config = Config::default();
        config.set("flags.offline", "true").unwrap();
        config.set("review.skip_diff", "true").unwrap();
        config.set("search.limit", "12").unwrap();
        config.set("search.aur", "true").unwrap();

        let parsed: Config = toml::from_str(&config.to_pretty_string()).unwrap();
        for key in KEYS {
            assert_eq!(parsed.get(key).unwrap(), config.get(key).unwrap(), "{key}");
        }
    }

    #[test]
    fn partial_and_empty_files_fill_in_defaults() {
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(
            empty.to_pretty_string(),
            Config::default().to_pretty_string()
        );

        let partial: Config = toml::from_str("[search]\nlimit = 4\n").unwrap();
        assert_eq!(partial.search.limit, Some(4));
        assert!(!partial.flags.offline);
        assert!(!partial.search.aur);
    }

    #[test]
    fn malformed_toml_and_wrong_types_fail_to_parse() {
        assert!(toml::from_str::<Config>("[flags\noffline =").is_err());
        assert!(toml::from_str::<Config>("[flags]\noffline = \"yes\"\n").is_err());
        assert!(toml::from_str::<Config>("[search]\nlimit = -1\n").is_err());
    }

    #[test]
    fn load_from_missing_file_is_defaults_and_does_not_create_it() {
        let scratch = Scratch::new("missing");
        let config = Config::load_from(&scratch.file()).unwrap();
        assert_eq!(
            config.to_pretty_string(),
            Config::default().to_pretty_string()
        );
        assert!(!scratch.file().exists());
    }

    #[test]
    fn save_to_creates_parent_dirs_and_load_from_reads_it_back() {
        let scratch = Scratch::new("roundtrip");
        let mut config = Config::default();
        config.set("flags.offline", "true").unwrap();
        config.set("search.limit", "9").unwrap();
        config.save_to(&scratch.file()).unwrap();

        let loaded = Config::load_from(&scratch.file()).unwrap();
        for key in KEYS {
            assert_eq!(loaded.get(key).unwrap(), config.get(key).unwrap(), "{key}");
        }
    }

    #[test]
    fn unset_then_save_resets_only_that_key_on_disk() {
        let scratch = Scratch::new("unset");
        let mut config = Config::default();
        config.set("flags.yes", "true").unwrap();
        config.set("search.limit", "4").unwrap();
        config.save_to(&scratch.file()).unwrap();

        let mut loaded = Config::load_from(&scratch.file()).unwrap();
        loaded.unset("search.limit").unwrap();
        loaded.save_to(&scratch.file()).unwrap();

        let reloaded = Config::load_from(&scratch.file()).unwrap();
        assert_eq!(reloaded.get("search.limit").unwrap(), "unset");
        assert_eq!(reloaded.get("flags.yes").unwrap(), "true");
    }

    #[test]
    fn load_from_broken_file_reports_parse_error_with_path() {
        let scratch = Scratch::new("broken");
        fs::create_dir_all(scratch.file().parent().unwrap()).unwrap();
        fs::write(scratch.file(), "[flags\noffline =").unwrap();

        let err = Config::load_from(&scratch.file()).unwrap_err();
        assert!(matches!(err, Error::ConfigParse { .. }));
        assert!(err
            .to_string()
            .contains(&scratch.file().display().to_string()));
    }

    #[test]
    fn load_from_unreadable_path_is_an_io_error_not_defaults() {
        let scratch = Scratch::new("isdir");
        // Reading a directory as a file fails with something other than NotFound.
        let err = Config::load_from(&scratch.0).unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
    }

    #[test]
    fn save_to_fails_cleanly_when_parent_is_a_file() {
        let scratch = Scratch::new("blocked");
        let blocker = scratch.0.join("blocker");
        fs::write(&blocker, "x").unwrap();
        let err = Config::default()
            .save_to(&blocker.join("config.toml"))
            .unwrap_err();
        assert!(matches!(err, Error::ConfigWrite { .. }));
    }
}

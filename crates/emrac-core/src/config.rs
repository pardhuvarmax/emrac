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
    #[serde(default)]
    pub snapshot: SnapshotConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default, rename = "self")]
    pub self_service: SelfConfig,
}

/// Where emrac looks for news of its own releases (`emrac self`). See
/// `docs/plan/r2/u12-self-service.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfConfig {
    /// `https://` address or `file://` path of `releases.json`.
    #[serde(default = "default_self_source")]
    pub source: String,
    /// `"stable"` or `"prerelease"` (which also sees stable ones).
    #[serde(default = "default_self_channel")]
    pub channel: String,
    /// Refuse an index without a valid signature by `signing_key`.
    #[serde(default)]
    pub require_signature: bool,
    /// Fingerprint of the key that signs `releases.json.asc`.
    #[serde(default)]
    pub signing_key: String,
}

pub const DEFAULT_SELF_SOURCE: &str = "https://pardhuvarmax.github.io/emrac/releases.json";

fn default_self_source() -> String {
    DEFAULT_SELF_SOURCE.to_string()
}
fn default_self_channel() -> String {
    "stable".to_string()
}

impl Default for SelfConfig {
    fn default() -> Self {
        Self {
            source: default_self_source(),
            channel: default_self_channel(),
            require_signature: false,
            signing_key: String::new(),
        }
    }
}

/// What `emrac cache clean` keeps. See `docs/plan/r1/u29-lean-cache.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// `"standard"` keeps the installed and the previous version of every
    /// package; `"lean"` keeps only what undoing the last transactions needs
    /// and what cannot be downloaded again.
    #[serde(default = "default_cache_policy")]
    pub policy: String,
    /// With `lean`: everything done in the last this-many days must stay
    /// undoable (the newest change is kept however old it is).
    #[serde(default = "default_keep_days")]
    pub keep_days: u32,
}

fn default_cache_policy() -> String {
    "standard".to_string()
}
fn default_keep_days() -> u32 {
    14
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            policy: default_cache_policy(),
            keep_days: default_keep_days(),
        }
    }
}

impl CacheConfig {
    pub fn is_lean(&self) -> bool {
        self.policy == "lean"
    }
}

/// Limits on sandboxed AUR builds. `0` turns a limit off. Unlike
/// `--no-sandbox` these may live in the config: they only restrict a build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Kill the build after this many minutes.
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u32,
    /// Memory ceiling as a percentage of total RAM (swap is disallowed).
    #[serde(default = "default_memory_percent")]
    pub memory_percent: u32,
    /// Most processes/threads the build may have.
    #[serde(default = "default_max_tasks")]
    pub max_tasks: u32,
    /// Size cap for the build's `/tmp` and `$HOME`, in GiB.
    #[serde(default = "default_tmpfs_gib")]
    pub tmpfs_gib: u32,
}

fn default_timeout_minutes() -> u32 {
    120
}
fn default_memory_percent() -> u32 {
    75
}
fn default_max_tasks() -> u32 {
    4096
}
fn default_tmpfs_gib() -> u32 {
    8
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            timeout_minutes: default_timeout_minutes(),
            memory_percent: default_memory_percent(),
            max_tasks: default_max_tasks(),
            tmpfs_gib: default_tmpfs_gib(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Save a restore point right before `emrac upgrade` changes anything.
    #[serde(default)]
    pub before_upgrade: bool,
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
    /// Download official packages first and show their manifests before
    /// installing (`--inspect`).
    #[serde(default)]
    pub inspect_official: bool,
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

    /// The file as it is written: framed and documented (see `config_style`).
    pub fn to_pretty_string(&self) -> String {
        crate::config_style::render(self)
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
            "review.inspect_official" => self.review.inspect_official.to_string(),
            "search.limit" => match self.search.limit {
                Some(n) => n.to_string(),
                None => "unset".to_string(),
            },
            "search.official" => self.search.official.to_string(),
            "search.aur" => self.search.aur.to_string(),
            "snapshot.before_upgrade" => self.snapshot.before_upgrade.to_string(),
            "build.timeout_minutes" => self.build.timeout_minutes.to_string(),
            "build.memory_percent" => self.build.memory_percent.to_string(),
            "build.max_tasks" => self.build.max_tasks.to_string(),
            "build.tmpfs_gib" => self.build.tmpfs_gib.to_string(),
            "cache.policy" => self.cache.policy.clone(),
            "cache.keep_days" => self.cache.keep_days.to_string(),
            "self.source" => self.self_service.source.clone(),
            "self.channel" => self.self_service.channel.clone(),
            "self.require_signature" => self.self_service.require_signature.to_string(),
            "self.signing_key" => self.self_service.signing_key.clone(),
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
            "review.inspect_official" => {
                self.review.inspect_official = parse_bool(value).ok_or_else(invalid)?;
            }
            "search.limit" => {
                self.search.limit = match value {
                    "unset" | "none" => None,
                    n => Some(n.parse::<usize>().map_err(|_| invalid())?),
                };
            }
            "search.official" => self.search.official = parse_bool(value).ok_or_else(invalid)?,
            "search.aur" => self.search.aur = parse_bool(value).ok_or_else(invalid)?,
            "snapshot.before_upgrade" => {
                self.snapshot.before_upgrade = parse_bool(value).ok_or_else(invalid)?;
            }
            "build.timeout_minutes" => {
                self.build.timeout_minutes = value.parse().map_err(|_| invalid())?;
            }
            "build.memory_percent" => {
                let pct: u32 = value.parse().map_err(|_| invalid())?;
                if pct > 100 {
                    return Err(invalid());
                }
                self.build.memory_percent = pct;
            }
            "build.max_tasks" => {
                self.build.max_tasks = value.parse().map_err(|_| invalid())?;
            }
            "build.tmpfs_gib" => {
                self.build.tmpfs_gib = value.parse().map_err(|_| invalid())?;
            }
            "cache.policy" => {
                if !matches!(value, "standard" | "lean") {
                    return Err(invalid());
                }
                self.cache.policy = value.to_string();
            }
            "cache.keep_days" => {
                self.cache.keep_days = value.parse().map_err(|_| invalid())?;
            }
            "self.source" => {
                if !(value.starts_with("https://") || value.starts_with("file://")) {
                    return Err(invalid());
                }
                self.self_service.source = value.to_string();
            }
            "self.channel" => {
                if !matches!(value, "stable" | "prerelease") {
                    return Err(invalid());
                }
                self.self_service.channel = value.to_string();
            }
            "self.require_signature" => {
                self.self_service.require_signature = parse_bool(value).ok_or_else(invalid)?;
            }
            "self.signing_key" => {
                let fpr: String = value.split_whitespace().collect();
                if !fpr.is_empty()
                    && !(fpr.len() >= 16
                        && fpr.len() <= 64
                        && fpr.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err(invalid());
                }
                self.self_service.signing_key = fpr.to_ascii_uppercase();
            }
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
            "review.inspect_official" => self.review.inspect_official = false,
            "search.limit" => self.search.limit = None,
            "search.official" => self.search.official = false,
            "search.aur" => self.search.aur = false,
            "snapshot.before_upgrade" => self.snapshot.before_upgrade = false,
            "build.timeout_minutes" => self.build.timeout_minutes = default_timeout_minutes(),
            "build.memory_percent" => self.build.memory_percent = default_memory_percent(),
            "build.max_tasks" => self.build.max_tasks = default_max_tasks(),
            "build.tmpfs_gib" => self.build.tmpfs_gib = default_tmpfs_gib(),
            "cache.policy" => self.cache.policy = default_cache_policy(),
            "cache.keep_days" => self.cache.keep_days = default_keep_days(),
            "self.source" => self.self_service.source = default_self_source(),
            "self.channel" => self.self_service.channel = default_self_channel(),
            "self.require_signature" => self.self_service.require_signature = false,
            "self.signing_key" => self.self_service.signing_key.clear(),
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

    const KEYS: [&str; 12] = [
        "flags.offline",
        "flags.yes",
        "flags.dry_run",
        "flags.json",
        "flags.quiet",
        "review.skip_pkgbuild",
        "review.skip_diff",
        "review.inspect_official",
        "search.limit",
        "search.official",
        "search.aur",
        "snapshot.before_upgrade",
    ];

    #[test]
    fn build_limits_have_safe_defaults_and_validate() {
        let mut config = Config::default();
        for (key, default) in [
            ("build.timeout_minutes", "120"),
            ("build.memory_percent", "75"),
            ("build.max_tasks", "4096"),
            ("build.tmpfs_gib", "8"),
        ] {
            assert_eq!(config.get(key).unwrap(), default, "{key}");
            config.set(key, "7").unwrap();
            assert_eq!(config.get(key).unwrap(), "7", "{key}");
            config.set(key, "0").unwrap();
            assert_eq!(config.get(key).unwrap(), "0", "0 turns a limit off: {key}");
            config.unset(key).unwrap();
            assert_eq!(config.get(key).unwrap(), default, "unset restores {key}");
            for bad in ["-1", "abc", "1.5", ""] {
                assert!(config.set(key, bad).is_err(), "{key}={bad:?}");
            }
        }
        assert!(
            config.set("build.memory_percent", "101").is_err(),
            "a percentage"
        );
        config.set("build.memory_percent", "100").unwrap();
    }

    #[test]
    fn the_self_service_keys_validate_and_unset_restores_the_defaults() {
        let mut c = Config::default();
        assert_eq!(c.get("self.source").unwrap(), DEFAULT_SELF_SOURCE);
        assert_eq!(c.get("self.channel").unwrap(), "stable");
        assert_eq!(c.get("self.require_signature").unwrap(), "false");
        c.set("self.source", "file:///tmp/releases.json").unwrap();
        c.set("self.channel", "prerelease").unwrap();
        c.set("self.require_signature", "true").unwrap();
        c.set("self.signing_key", "ab12 cd34 ef56 7890 ab12")
            .unwrap();
        assert_eq!(c.get("self.signing_key").unwrap(), "AB12CD34EF567890AB12");
        for (key, bad) in [
            ("self.source", "http://example.com/releases.json"),
            ("self.source", "ftp://x"),
            ("self.channel", "nightly"),
            ("self.require_signature", "yes"),
            ("self.signing_key", "not hex"),
            ("self.signing_key", "ABCD"),
        ] {
            assert!(c.set(key, bad).is_err(), "{key}={bad}");
        }
        for key in [
            "self.source",
            "self.channel",
            "self.require_signature",
            "self.signing_key",
        ] {
            c.unset(key).unwrap();
        }
        assert_eq!(c.get("self.source").unwrap(), DEFAULT_SELF_SOURCE);
        assert_eq!(c.get("self.signing_key").unwrap(), "");
    }

    #[test]
    fn the_cache_policy_is_standard_or_lean_and_keeps_at_least_nothing_invalid() {
        let mut config = Config::default();
        assert_eq!(config.get("cache.policy").unwrap(), "standard");
        assert!(!config.cache.is_lean());
        assert_eq!(config.get("cache.keep_days").unwrap(), "14");
        config.set("cache.policy", "lean").unwrap();
        assert!(config.cache.is_lean());
        for bad in ["Lean", "tight", ""] {
            assert!(config.set("cache.policy", bad).is_err(), "{bad:?}");
        }
        config.set("cache.keep_days", "3").unwrap();
        assert_eq!(config.get("cache.keep_days").unwrap(), "3");
        for bad in ["-1", "two", "1.5"] {
            assert!(config.set("cache.keep_days", bad).is_err(), "{bad:?}");
        }
        config.unset("cache.policy").unwrap();
        config.unset("cache.keep_days").unwrap();
        assert_eq!(config.get("cache.policy").unwrap(), "standard");
        assert_eq!(config.get("cache.keep_days").unwrap(), "14");
        let partial: Config = toml::from_str("[cache]\npolicy = \"lean\"\n").unwrap();
        assert!(partial.cache.is_lean());
        assert_eq!(partial.cache.keep_days, 14);
    }

    #[test]
    fn a_partial_build_section_keeps_the_other_defaults() {
        let config: Config = toml::from_str("[build]\nmemory_percent = 50\n").unwrap();
        assert_eq!(config.build.memory_percent, 50);
        assert_eq!(config.build.timeout_minutes, 120);
        assert_eq!(config.build.max_tasks, 4096);
        assert_eq!(config.build.tmpfs_gib, 8);
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(
            empty.build.timeout_minutes, 120,
            "no [build] section at all"
        );
    }

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

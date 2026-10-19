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
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub advisor: AdvisorConfig,
    #[serde(default)]
    pub clui: ClUiConfig,
    #[serde(default)]
    pub score: ScoreConfig,
    #[serde(default)]
    pub install: InstallConfig,
}

/// What `install` does with the packages it is asked for (u23).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallConfig {
    /// `repository` (the default): an official package is the repository's
    /// binary. `recommend`: it is built from source where `emrac recommend`
    /// says source with some confidence. `--source` and `--binary` win.
    #[serde(default = "default_prefer")]
    pub prefer: String,
}

fn default_prefer() -> String {
    "repository".to_string()
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            prefer: default_prefer(),
        }
    }
}

/// How much each dimension of `emrac score` counts in the overall number
/// (0 leaves it out, 10 is the most, 1 is the default for all).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreConfig {
    #[serde(default = "default_weight")]
    pub performance: u8,
    #[serde(default = "default_weight")]
    pub build_time: u8,
    #[serde(default = "weight_0")]
    pub maintenance: u8,
    #[serde(default = "default_weight")]
    pub security: u8,
    #[serde(default = "default_weight")]
    pub reproducibility: u8,
    #[serde(default = "default_weight")]
    pub provenance: u8,
    #[serde(default = "default_weight")]
    pub compatibility: u8,
    #[serde(default = "weight_0")]
    pub packaging: u8,
    #[serde(default = "weight_0")]
    pub upstream: u8,
    #[serde(default = "weight_0")]
    pub popularity: u8,
    #[serde(default = "default_weight")]
    pub source: u8,
}

fn default_weight() -> u8 {
    1
}

fn weight_0() -> u8 {
    0
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            performance: 1,
            build_time: 1,
            maintenance: 0,
            security: 1,
            reproducibility: 1,
            provenance: 1,
            compatibility: 1,
            packaging: 0,
            upstream: 0,
            popularity: 0,
            source: 1,
        }
    }
}

impl ScoreConfig {
    pub fn weights(&self) -> crate::score::Weights {
        crate::score::Weights {
            performance: self.performance.min(crate::score::MAX_WEIGHT),
            build_time: self.build_time.min(crate::score::MAX_WEIGHT),
            maintenance: self.maintenance.min(crate::score::MAX_WEIGHT),
            security: self.security.min(crate::score::MAX_WEIGHT),
            reproducibility: self.reproducibility.min(crate::score::MAX_WEIGHT),
            provenance: self.provenance.min(crate::score::MAX_WEIGHT),
            compatibility: self.compatibility.min(crate::score::MAX_WEIGHT),
            packaging: self.packaging.min(crate::score::MAX_WEIGHT),
            upstream: self.upstream.min(crate::score::MAX_WEIGHT),
            popularity: self.popularity.min(crate::score::MAX_WEIGHT),
            source: self.source.min(crate::score::MAX_WEIGHT),
        }
    }

    fn slot(&mut self, key: &str) -> Option<&mut u8> {
        Some(match key {
            "score.performance" => &mut self.performance,
            "score.build_time" => &mut self.build_time,
            "score.maintenance" => &mut self.maintenance,
            "score.security" => &mut self.security,
            "score.popularity" => &mut self.popularity,
            "score.reproducibility" => &mut self.reproducibility,
            "score.provenance" => &mut self.provenance,
            "score.compatibility" => &mut self.compatibility,
            "score.packaging" => &mut self.packaging,
            "score.upstream" => &mut self.upstream,
            "score.source" => &mut self.source,
            _ => return None,
        })
    }
}

/// How emrac looks and moves (u17). The words are kept as text in the file and
/// checked when set; `settings` turns them into the type `style` uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClUiConfig {
    /// `regular`: nothing moves, as before. `active`: spinners and progress bars.
    #[serde(default = "default_animations")]
    pub animations: String,
    /// `auto` (a terminal, not `NO_COLOR`), `always` or `never`.
    #[serde(default = "default_auto")]
    pub color: String,
    /// `auto` (when the locale is UTF-8), `always` or `never` (ASCII marks and lines).
    #[serde(default = "default_auto")]
    pub unicode: String,
    /// `default`, `light`, `mono` or `high-contrast`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// `emrac`: pacman's transaction is shown in emrac's style on a terminal
    /// (its questions and errors stay as they are). `native`: pacman's own output.
    #[serde(default = "default_pacman")]
    pub pacman: String,
    /// `emrac`: an AUR build is shown in emrac's style on a terminal (phases,
    /// a live line for the compiler, errors at once, the output kept in a log).
    /// `native`: makepkg's own output.
    #[serde(default = "default_pacman")]
    pub builds: String,
}

fn default_animations() -> String {
    "regular".to_string()
}
fn default_auto() -> String {
    "auto".to_string()
}
fn default_pacman() -> String {
    "emrac".to_string()
}
fn default_theme() -> String {
    "default".to_string()
}

impl Default for ClUiConfig {
    fn default() -> Self {
        Self {
            animations: default_animations(),
            color: default_auto(),
            unicode: default_auto(),
            theme: default_theme(),
            pacman: default_pacman(),
            builds: default_pacman(),
        }
    }
}

impl ClUiConfig {
    /// The settings `style` works from; a value that is not one of the words
    /// (a hand-edited file) falls back to the default.
    pub fn settings(&self) -> crate::style::Settings {
        use crate::style::{Animations, Mode, Settings, Theme};
        Settings {
            color: Mode::parse(&self.color).unwrap_or_default(),
            unicode: Mode::parse(&self.unicode).unwrap_or_default(),
            animations: Animations::parse(&self.animations).unwrap_or_default(),
            theme: Theme::parse(&self.theme).unwrap_or_default(),
            plain_output: false,
        }
    }
}

/// The Package Advisor (u15): when a plan is big enough to be worth a word.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisorConfig {
    /// `false` turns the advice off.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Advise when a plan downloads at least this much (MiB); 0 turns this off.
    #[serde(default = "default_large_download_mib")]
    pub large_download_mib: u32,
    /// Advise when a plan grows the installed size by at least this much (MiB); 0 turns this off.
    #[serde(default = "default_large_growth_mib")]
    pub large_growth_mib: u32,
    /// Advise when this many dependencies come along; 0 turns this off.
    #[serde(default = "default_many_dependencies")]
    pub many_dependencies: u32,
}

fn default_true() -> bool {
    true
}
fn default_large_download_mib() -> u32 {
    1024
}
fn default_large_growth_mib() -> u32 {
    2048
}
fn default_many_dependencies() -> u32 {
    40
}

impl Default for AdvisorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            large_download_mib: default_large_download_mib(),
            large_growth_mib: default_large_growth_mib(),
            many_dependencies: default_many_dependencies(),
        }
    }
}

/// How strict the checks before a transaction are (u17).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// `"block"` stops on a problem, `"warn"` shows it and goes on, `"off"` skips the checks.
    #[serde(default = "default_preflight")]
    pub preflight: String,
    /// `"refuse"`, `"warn"` or `"off"`: what to do when installing or upgrading only some
    /// packages would leave upgrades waiting (a partial upgrade).
    #[serde(default = "default_partial")]
    pub partial_upgrade: String,
    /// The Arch news before an upgrade: `"gate"` warns about unread news and
    /// stops for a critical item, `"warn"` never stops, `"off"` does not look.
    #[serde(default = "default_news")]
    pub arch_news: String,
    /// `"on"` backs up pacman's local database before each transaction and
    /// checks it before and after (`pacman -Dk`); `"off"` does neither.
    #[serde(default = "default_db_backup")]
    pub db_backup: String,
    /// What to do when a plan would install a version with a confirmed open High or
    /// Critical Arch Security Tracker entry: `"warn"` shows it, `"block"` stops, `"off"` does not look.
    #[serde(default = "default_vulnerabilities")]
    pub vulnerabilities: String,
}

fn default_vulnerabilities() -> String {
    "warn".to_string()
}

fn default_db_backup() -> String {
    "on".to_string()
}

fn default_news() -> String {
    "gate".to_string()
}

fn default_preflight() -> String {
    "block".to_string()
}
fn default_partial() -> String {
    "refuse".to_string()
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            preflight: default_preflight(),
            partial_upgrade: default_partial(),
            arch_news: default_news(),
            db_backup: default_db_backup(),
            vulnerabilities: default_vulnerabilities(),
        }
    }
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
    /// What to do with the packages a build installed only to build:
    /// `ask`, `always` or `never` remove them.
    #[serde(default = "default_remove_make_deps")]
    pub remove_make_deps: String,
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
fn default_remove_make_deps() -> String {
    "ask".to_string()
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            timeout_minutes: default_timeout_minutes(),
            memory_percent: default_memory_percent(),
            max_tasks: default_max_tasks(),
            tmpfs_gib: default_tmpfs_gib(),
            remove_make_deps: default_remove_make_deps(),
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
            "build.remove_make_deps" => self.build.remove_make_deps.clone(),
            "cache.policy" => self.cache.policy.clone(),
            "cache.keep_days" => self.cache.keep_days.to_string(),
            "safety.preflight" => self.safety.preflight.clone(),
            "safety.partial_upgrade" => self.safety.partial_upgrade.clone(),
            "safety.arch_news" => self.safety.arch_news.clone(),
            "safety.db_backup" => self.safety.db_backup.clone(),
            "safety.vulnerabilities" => self.safety.vulnerabilities.clone(),
            "install.prefer" => self.install.prefer.clone(),
            "score.performance" => self.score.performance.to_string(),
            "score.build_time" => self.score.build_time.to_string(),
            "score.maintenance" => self.score.maintenance.to_string(),
            "score.security" => self.score.security.to_string(),
            "score.popularity" => self.score.popularity.to_string(),
            "score.reproducibility" => self.score.reproducibility.to_string(),
            "score.provenance" => self.score.provenance.to_string(),
            "score.compatibility" => self.score.compatibility.to_string(),
            "score.packaging" => self.score.packaging.to_string(),
            "score.upstream" => self.score.upstream.to_string(),
            "score.source" => self.score.source.to_string(),
            "clui.animations" => self.clui.animations.clone(),
            "clui.color" => self.clui.color.clone(),
            "clui.unicode" => self.clui.unicode.clone(),
            "clui.theme" => self.clui.theme.clone(),
            "clui.pacman" => self.clui.pacman.clone(),
            "clui.builds" => self.clui.builds.clone(),
            "advisor.enabled" => self.advisor.enabled.to_string(),
            "advisor.large_download_mib" => self.advisor.large_download_mib.to_string(),
            "advisor.large_growth_mib" => self.advisor.large_growth_mib.to_string(),
            "advisor.many_dependencies" => self.advisor.many_dependencies.to_string(),
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
            "build.remove_make_deps" => {
                if !matches!(value, "ask" | "always" | "never") {
                    return Err(invalid());
                }
                self.build.remove_make_deps = value.to_string();
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
            k if k.starts_with("score.") && self.score.slot(k).is_some() => {
                let n: u8 = value
                    .parse()
                    .ok()
                    .filter(|n| *n <= crate::score::MAX_WEIGHT)
                    .ok_or_else(invalid)?;
                *self.score.slot(k).expect("checked above") = n;
            }
            "clui.animations" => {
                crate::style::Animations::parse(value).ok_or_else(invalid)?;
                self.clui.animations = value.to_string();
            }
            "clui.color" => {
                crate::style::Mode::parse(value).ok_or_else(invalid)?;
                self.clui.color = value.to_string();
            }
            "clui.unicode" => {
                crate::style::Mode::parse(value).ok_or_else(invalid)?;
                self.clui.unicode = value.to_string();
            }
            "clui.theme" => {
                crate::style::Theme::parse(value).ok_or_else(invalid)?;
                self.clui.theme = value.to_string();
            }
            "clui.pacman" => {
                if !matches!(value, "emrac" | "native") {
                    return Err(invalid());
                }
                self.clui.pacman = value.to_string();
            }
            "clui.builds" => {
                if !matches!(value, "emrac" | "native") {
                    return Err(invalid());
                }
                self.clui.builds = value.to_string();
            }
            "advisor.enabled" => {
                self.advisor.enabled = parse_bool(value).ok_or_else(invalid)?;
            }
            "advisor.large_download_mib" => {
                self.advisor.large_download_mib = value.parse().map_err(|_| invalid())?;
            }
            "advisor.large_growth_mib" => {
                self.advisor.large_growth_mib = value.parse().map_err(|_| invalid())?;
            }
            "advisor.many_dependencies" => {
                self.advisor.many_dependencies = value.parse().map_err(|_| invalid())?;
            }
            "safety.preflight" => {
                if !matches!(value, "block" | "warn" | "off") {
                    return Err(invalid());
                }
                self.safety.preflight = value.to_string();
            }
            "safety.partial_upgrade" => {
                if !matches!(value, "refuse" | "warn" | "off") {
                    return Err(invalid());
                }
                self.safety.partial_upgrade = value.to_string();
            }
            "safety.db_backup" => {
                if !matches!(value, "on" | "off") {
                    return Err(invalid());
                }
                self.safety.db_backup = value.to_string();
            }
            "install.prefer" => {
                if crate::install_mode::Prefer::parse(value).is_none() {
                    return Err(invalid());
                }
                self.install.prefer = value.to_string();
            }
            "safety.vulnerabilities" => {
                if !matches!(value, "warn" | "block" | "off") {
                    return Err(invalid());
                }
                self.safety.vulnerabilities = value.to_string();
            }
            "safety.arch_news" => {
                if !matches!(value, "gate" | "warn" | "off") {
                    return Err(invalid());
                }
                self.safety.arch_news = value.to_string();
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
            "build.remove_make_deps" => self.build.remove_make_deps = default_remove_make_deps(),
            "cache.policy" => self.cache.policy = default_cache_policy(),
            "cache.keep_days" => self.cache.keep_days = default_keep_days(),
            k if k.starts_with("score.") && self.score.slot(k).is_some() => {
                *self.score.slot(k).expect("checked above") =
                    crate::score::Weights::default_for(k.trim_start_matches("score."));
            }
            "clui.animations" => self.clui.animations = default_animations(),
            "clui.color" => self.clui.color = default_auto(),
            "clui.unicode" => self.clui.unicode = default_auto(),
            "clui.theme" => self.clui.theme = default_theme(),
            "clui.pacman" => self.clui.pacman = default_pacman(),
            "clui.builds" => self.clui.builds = default_pacman(),
            "advisor.enabled" => self.advisor.enabled = true,
            "advisor.large_download_mib" => {
                self.advisor.large_download_mib = default_large_download_mib()
            }
            "advisor.large_growth_mib" => {
                self.advisor.large_growth_mib = default_large_growth_mib()
            }
            "advisor.many_dependencies" => {
                self.advisor.many_dependencies = default_many_dependencies()
            }
            "safety.preflight" => self.safety.preflight = default_preflight(),
            "safety.partial_upgrade" => self.safety.partial_upgrade = default_partial(),
            "safety.arch_news" => self.safety.arch_news = default_news(),
            "safety.db_backup" => self.safety.db_backup = default_db_backup(),
            "safety.vulnerabilities" => self.safety.vulnerabilities = default_vulnerabilities(),
            "install.prefer" => self.install.prefer = default_prefer(),
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
    fn the_safety_keys_default_to_the_strict_settings_and_validate() {
        let mut c = Config::default();
        assert_eq!(c.get("safety.preflight").unwrap(), "block");
        assert_eq!(c.get("safety.partial_upgrade").unwrap(), "refuse");
        assert_eq!(c.get("safety.arch_news").unwrap(), "gate");
        assert_eq!(c.get("safety.db_backup").unwrap(), "on");
        c.set("safety.db_backup", "off").unwrap();
        assert!(c.set("safety.db_backup", "maybe").is_err());
        c.unset("safety.db_backup").unwrap();
        c.set("safety.arch_news", "warn").unwrap();
        assert!(c.set("safety.arch_news", "block").is_err());
        c.unset("safety.arch_news").unwrap();
        c.set("safety.preflight", "warn").unwrap();
        c.set("safety.partial_upgrade", "off").unwrap();
        assert_eq!(c.get("safety.preflight").unwrap(), "warn");
        for (k, bad) in [
            ("safety.preflight", "never"),
            ("safety.partial_upgrade", "block"),
            ("safety.preflight", ""),
        ] {
            assert!(c.set(k, bad).is_err(), "{k}={bad}");
        }
        c.unset("safety.preflight").unwrap();
        c.unset("safety.partial_upgrade").unwrap();
        assert_eq!(c.get("safety.preflight").unwrap(), "block");
        assert_eq!(c.get("safety.partial_upgrade").unwrap(), "refuse");
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
    fn removing_build_tools_is_ask_always_or_never() {
        let mut config = Config::default();
        assert_eq!(config.get("build.remove_make_deps").unwrap(), "ask");
        for value in ["always", "never", "ask"] {
            config.set("build.remove_make_deps", value).unwrap();
            assert_eq!(config.get("build.remove_make_deps").unwrap(), value);
        }
        assert!(config.set("build.remove_make_deps", "yes").is_err());
        assert!(config.set("build.remove_make_deps", "").is_err());
        config.set("build.remove_make_deps", "never").unwrap();
        config.unset("build.remove_make_deps").unwrap();
        assert_eq!(config.get("build.remove_make_deps").unwrap(), "ask");
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.build.remove_make_deps, "ask");
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

    #[test]
    fn score_weights_are_checked_and_reset() {
        let mut c = Config::default();
        assert_eq!(c.get("score.security").unwrap(), "1");
        assert!(c.score.weights().is_default());
        c.set("score.security", "5").unwrap();
        c.set("score.popularity", "0").unwrap();
        assert_eq!(c.get("score.security").unwrap(), "5");
        let w = c.score.weights();
        assert_eq!((w.security, w.popularity, w.source), (5, 0, 1));
        for bad in ["11", "-1", "many", "1.5", ""] {
            assert!(c.set("score.security", bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            c.get("score.security").unwrap(),
            "5",
            "a refused value changes nothing"
        );
        assert!(c.set("score.nonsense", "1").is_err());
        c.unset("score.security").unwrap();
        assert_eq!(c.get("score.security").unwrap(), "1");
        let again: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(again.get("score.popularity").unwrap(), "0");
        // a hand-edited number over the limit is held at the limit
        let hand: Config = toml::from_str("[score]\nsecurity = 99\n").unwrap();
        assert_eq!(hand.score.weights().security, crate::score::MAX_WEIGHT);
    }
}

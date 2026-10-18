//! `emrac export` and `emrac import` (u12): a small file that describes this
//! machine, and a report of how another one differs from it.
//!
//! A file may come from someone else, so what it can change is narrow: a
//! safe subset of the settings, pins (asked about, and `--yes` does not answer),
//! and the explicit packages to install through the normal install path.
//! Repositories and the keys that switch protections off, or point emrac's own
//! updates somewhere, are never applied. See `docs/plan/r3/u12-export-import.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::pins::Pin;
use crate::snapshot::SnapshotPackage;

pub const FORMAT: u32 = 1;
pub const MAX_BYTES: usize = 5 << 20;
pub const MAX_PACKAGES: usize = 20_000;

/// The settings that are exported and may be applied. Not in the list, on purpose:
/// `self.*` (where emrac updates itself from), `safety.*`, `review.skip_*`, `flags.*`.
pub const SAFE_KEYS: &[&str] = &[
    "search.limit",
    "search.official",
    "search.aur",
    "snapshot.before_upgrade",
    "build.timeout_minutes",
    "build.memory_percent",
    "build.max_tasks",
    "build.tmpfs_gib",
    "build.remove_make_deps",
    "cache.policy",
    "cache.keep_days",
    "review.inspect_official",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub packages: bool,
    pub config: bool,
    pub repos: bool,
    pub pins: bool,
}

impl Scope {
    pub const ALL: Scope = Scope {
        packages: true,
        config: true,
        repos: true,
        pins: true,
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub format: u32,
    pub emrac: String,
    /// `YYYY-MM-DD`, UTC.
    pub exported: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExPackage {
    pub name: String,
    pub version: String,
    /// Installed on purpose, not as a dependency.
    pub explicit: bool,
    /// Not in a sync database (almost always from the AUR).
    pub foreign: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExPin {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Exchange {
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package: Vec<ExPackage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pin: Vec<ExPin>,
}

fn bad(why: impl Into<String>) -> Error {
    Error::Exchange(why.into())
}

/// What pacman accepts in a package name, and never a leading `-` or `.`.
pub fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 100 && v.chars().all(|c| c.is_ascii_graphic())
}

fn valid_repo(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The file for this machine's state.
pub fn build(
    scope: Scope,
    installed: &[SnapshotPackage],
    repos: &[String],
    config: &Config,
    pins: &[Pin],
    exported: &str,
) -> Exchange {
    let mut package: Vec<ExPackage> = if scope.packages {
        installed
            .iter()
            .map(|p| ExPackage {
                name: p.name.clone(),
                version: p.version.clone(),
                explicit: p.explicit,
                foreign: p.foreign,
            })
            .collect()
    } else {
        Vec::new()
    };
    package.sort_by(|a, b| a.name.cmp(&b.name));
    let mut config_map = BTreeMap::new();
    if scope.config {
        for key in SAFE_KEYS {
            if let Ok(value) = config.get(key) {
                if value != "unset" {
                    config_map.insert((*key).to_string(), value);
                }
            }
        }
    }
    Exchange {
        meta: Meta {
            format: FORMAT,
            emrac: env!("CARGO_PKG_VERSION").to_string(),
            exported: exported.to_string(),
        },
        package,
        repo: if scope.repos {
            repos.to_vec()
        } else {
            Vec::new()
        },
        config: config_map,
        pin: if scope.pins {
            pins.iter()
                .map(|p| ExPin {
                    name: p.name.clone(),
                    reason: p.reason.clone(),
                    until: p.until.map(|_| p.until_date().unwrap_or_default()),
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

pub fn render(ex: &Exchange) -> String {
    format!(
        "# An emrac exchange file (`emrac export`). Read it before you import it:\n\
         # `emrac import <file>` shows what would change and asks about each part.\n\
         # It never applies repositories, `self.*`, `safety.*`, `flags.*` or\n\
         # `review.skip_*`, whatever the file says.\n\n{}",
        toml::to_string_pretty(ex).unwrap_or_default()
    )
}

/// Reads and validates a file's text.
pub fn parse(text: &str) -> Result<Exchange> {
    if text.len() > MAX_BYTES {
        return Err(bad(format!(
            "the file is larger than {} MiB",
            MAX_BYTES >> 20
        )));
    }
    let ex: Exchange =
        toml::from_str(text).map_err(|e| bad(format!("not an exchange file: {e}")))?;
    if ex.meta.format != FORMAT {
        return Err(bad(format!(
            "format {} is not one this emrac reads (it reads format {FORMAT})",
            ex.meta.format
        )));
    }
    if ex.package.len() > MAX_PACKAGES {
        return Err(bad(format!("more than {MAX_PACKAGES} packages")));
    }
    for p in &ex.package {
        if !valid_package_name(&p.name) {
            return Err(bad(format!("'{}' is not a package name", p.name)));
        }
        if !valid_version(&p.version) {
            return Err(bad(format!("'{}' has an invalid version", p.name)));
        }
    }
    for r in &ex.repo {
        if !valid_repo(r) {
            return Err(bad(format!("'{r}' is not a repository name")));
        }
    }
    for p in &ex.pin {
        if !valid_package_name(&p.name) {
            return Err(bad(format!("'{}' is not a package name (pin)", p.name)));
        }
        if let Some(u) = &p.until {
            crate::history::parse_date(u)
                .map_err(|_| bad(format!("pin '{}': '{u}' is not a date", p.name)))?;
        }
    }
    Ok(ex)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConfigChange {
    pub key: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ImportReport {
    /// Explicit packages that are not installed here.
    pub to_install: Vec<ExPackage>,
    /// Installed here at another version: (name, theirs, ours). Never changed.
    pub version_differs: Vec<(String, String, String)>,
    pub already_installed: usize,
    /// Not installed, not explicit: they come along as dependencies.
    pub dependencies_left: usize,
    pub config_changes: Vec<ConfigChange>,
    /// Config keys in the file that are not applied, with why.
    pub config_refused: Vec<(String, &'static str)>,
    /// Repositories the file has that are not enabled here. Never applied.
    pub repos_missing: Vec<String>,
    /// Pins the file has that are not in force here.
    pub new_pins: Vec<ExPin>,
}

fn why_refused(key: &str) -> &'static str {
    if key.starts_with("self.") {
        "it decides where emrac updates itself from"
    } else if key.starts_with("safety.")
        || key.starts_with("review.skip_")
        || key.starts_with("flags.")
    {
        "it switches a protection off or answers a question for you"
    } else {
        "not a setting that can be exchanged"
    }
}

/// How `ex` differs from this machine.
pub fn compare(
    ex: &Exchange,
    scope: Scope,
    installed: &[(String, String)],
    config: &Config,
    repos: &[String],
    pins: &[Pin],
) -> ImportReport {
    let mut report = ImportReport::default();
    if scope.packages {
        let have: BTreeMap<&str, &str> = installed
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        for p in &ex.package {
            match have.get(p.name.as_str()) {
                Some(ours) => {
                    report.already_installed += 1;
                    if *ours != p.version {
                        report.version_differs.push((
                            p.name.clone(),
                            p.version.clone(),
                            (*ours).to_string(),
                        ));
                    }
                }
                None if p.explicit => report.to_install.push(p.clone()),
                None => report.dependencies_left += 1,
            }
        }
    }
    if scope.config {
        for (key, value) in &ex.config {
            if !SAFE_KEYS.contains(&key.as_str()) {
                report.config_refused.push((key.clone(), why_refused(key)));
                continue;
            }
            match config.get(key) {
                Ok(now) if now == *value => {}
                Ok(now) => report.config_changes.push(ConfigChange {
                    key: key.clone(),
                    from: now,
                    to: value.clone(),
                }),
                Err(_) => report
                    .config_refused
                    .push((key.clone(), "this emrac has no such setting")),
            }
        }
    }
    if scope.repos {
        report.repos_missing = ex
            .repo
            .iter()
            .filter(|r| !repos.contains(r))
            .cloned()
            .collect();
    }
    if scope.pins {
        report.new_pins = ex
            .pin
            .iter()
            .filter(|p| !pins.iter().any(|q| q.name == p.name))
            .cloned()
            .collect();
    }
    report
}

/// Sets each change in `config` (every value is checked by `Config::set`).
pub fn apply_config(config: &mut Config, changes: &[ConfigChange]) -> Result<()> {
    for c in changes {
        if !SAFE_KEYS.contains(&c.key.as_str()) {
            return Err(bad(format!("'{}' cannot be applied from a file", c.key)));
        }
        config.set(&c.key, &c.to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(name: &str, version: &str, explicit: bool, foreign: bool) -> SnapshotPackage {
        SnapshotPackage {
            name: name.into(),
            version: version.into(),
            explicit,
            foreign,
        }
    }

    fn sample() -> Exchange {
        let mut config = Config::default();
        config.set("build.timeout_minutes", "90").unwrap();
        config.set("flags.yes", "true").unwrap();
        config.self_service.source = "https://example.invalid/releases.json".into();
        build(
            Scope::ALL,
            &[
                sp("zsh", "5.9-1", true, false),
                sp("glibc", "2.44-1", false, false),
                sp("yay", "13-1", true, true),
            ],
            &["core".into(), "extra".into()],
            &config,
            &[],
            "2026-10-18",
        )
    }

    fn installed(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_file_round_trips() {
        let ex = sample();
        let text = render(&ex);
        assert!(text.starts_with("# An emrac exchange file"));
        assert_eq!(parse(&text).unwrap(), ex);
        assert_eq!(ex.package[0].name, "glibc", "sorted by name");
    }

    #[test]
    fn only_the_safe_settings_are_exported() {
        let ex = sample();
        assert_eq!(
            ex.config.get("build.timeout_minutes").map(String::as_str),
            Some("90")
        );
        for k in ex.config.keys() {
            assert!(SAFE_KEYS.contains(&k.as_str()), "{k}");
        }
        let text = toml::to_string_pretty(&ex).unwrap();
        assert!(
            !text.contains("flags")
                && !text.contains("example.invalid")
                && !text.contains("signing_key")
        );
    }

    #[test]
    fn the_scope_leaves_parts_out() {
        let ex = build(
            Scope {
                packages: true,
                config: false,
                repos: false,
                pins: false,
            },
            &[sp("a", "1", true, false)],
            &["core".into()],
            &Config::default(),
            &[],
            "2026-10-18",
        );
        assert!(ex.repo.is_empty() && ex.config.is_empty() && ex.pin.is_empty());
        assert_eq!(ex.package.len(), 1);
    }

    #[test]
    fn a_file_is_refused_when_it_is_wrong() {
        let head = "[meta]\nformat = 1\nemrac = \"x\"\nexported = \"2026-10-18\"\n";
        assert!(parse(&format!(
            "{head}[[package]]\nname = \"a\"\nversion = \"1\"\nexplicit = true\nforeign = false\n"
        ))
        .is_ok());
        for (why, text) in [
            ("a name that is an option", format!("{head}[[package]]\nname = \"--noconfirm\"\nversion = \"1\"\nexplicit = true\nforeign = false\n")),
            ("a name with a slash", format!("{head}[[package]]\nname = \"a/b\"\nversion = \"1\"\nexplicit = true\nforeign = false\n")),
            ("a version with a space", format!("{head}[[package]]\nname = \"a\"\nversion = \"1 2\"\nexplicit = true\nforeign = false\n")),
            ("a repository with a slash", format!("repo = [\"a/b\"]\n{head}")),
            ("a pin that is not a date", format!("{head}[[pin]]\nname = \"a\"\nuntil = \"soon\"\n")),
            ("another format", head.replace("format = 1", "format = 2")),
            ("not toml", "this is not toml [".to_string()),
            ("no meta", "repo = []\n".to_string()),
        ] {
            assert!(parse(&text).is_err(), "{why}");
        }
        assert!(parse(&"#".repeat(MAX_BYTES + 1)).is_err(), "too large");
    }

    #[test]
    fn explicit_packages_to_install_and_dependencies_left_to_come_along() {
        let ex = sample();
        let r = compare(
            &ex,
            Scope::ALL,
            &installed(&[("glibc", "2.44-1"), ("zsh", "5.8-1")]),
            &Config::default(),
            &[],
            &[],
        );
        assert_eq!(
            r.to_install
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["yay"]
        );
        assert_eq!(r.already_installed, 2);
        assert_eq!(
            r.version_differs,
            [("zsh".to_string(), "5.9-1".to_string(), "5.8-1".to_string())]
        );
        let r = compare(
            &ex,
            Scope::ALL,
            &installed(&[("zsh", "5.9-1")]),
            &Config::default(),
            &[],
            &[],
        );
        assert_eq!(
            r.dependencies_left, 1,
            "glibc is not explicit: it comes with what needs it"
        );
    }

    #[test]
    fn only_differing_safe_settings_are_changes() {
        let ex = sample();
        let mut ours = Config::default();
        let r = compare(&ex, Scope::ALL, &[], &ours, &[], &[]);
        assert_eq!(
            r.config_changes,
            [ConfigChange {
                key: "build.timeout_minutes".into(),
                from: "120".into(),
                to: "90".into()
            }]
        );
        apply_config(&mut ours, &r.config_changes).unwrap();
        assert_eq!(ours.get("build.timeout_minutes").unwrap(), "90");
        assert!(compare(&ex, Scope::ALL, &[], &ours, &[], &[])
            .config_changes
            .is_empty());
    }

    #[test]
    fn a_hostile_file_cannot_change_what_protects_you() {
        let mut ex = sample();
        for (k, v) in [
            ("self.source", "https://evil.invalid/releases.json"),
            ("self.signing_key", "AAAA"),
            ("safety.preflight", "off"),
            ("review.skip_pkgbuild", "true"),
            ("flags.yes", "true"),
            ("no.such.key", "1"),
        ] {
            ex.config.insert(k.into(), v.into());
        }
        let ours = Config::default();
        let r = compare(&ex, Scope::ALL, &[], &ours, &[], &[]);
        assert_eq!(r.config_changes.len(), 1, "only build.timeout_minutes");
        assert_eq!(r.config_refused.len(), 6, "{:?}", r.config_refused);
        assert!(r
            .config_refused
            .iter()
            .any(|(k, why)| k == "self.source" && why.contains("updates itself")));
        assert!(r
            .config_refused
            .iter()
            .any(|(k, why)| k == "safety.preflight" && why.contains("protection")));
        // and applying one directly is refused as well
        let mut c = Config::default();
        let evil = [ConfigChange {
            key: "self.source".into(),
            from: String::new(),
            to: "x".into(),
        }];
        assert!(apply_config(&mut c, &evil).is_err());
        assert_eq!(
            c.get("self.source").unwrap(),
            Config::default().get("self.source").unwrap()
        );
    }

    #[test]
    fn a_bad_value_for_a_safe_setting_is_refused_by_the_setting_itself() {
        let mut c = Config::default();
        let bad_value = [ConfigChange {
            key: "build.timeout_minutes".into(),
            from: "120".into(),
            to: "forever".into(),
        }];
        assert!(apply_config(&mut c, &bad_value).is_err());
    }

    #[test]
    fn repositories_are_only_reported_and_pins_already_in_force_are_not_new() {
        let mut ex = sample();
        ex.pin = vec![
            ExPin {
                name: "linux".into(),
                reason: Some("dkms".into()),
                until: None,
            },
            ExPin {
                name: "mesa".into(),
                reason: None,
                until: None,
            },
        ];
        let held = [Pin {
            name: "mesa".into(),
            reason: None,
            until: None,
            created: 1,
        }];
        let r = compare(
            &ex,
            Scope::ALL,
            &[],
            &Config::default(),
            &["core".into()],
            &held,
        );
        assert_eq!(r.repos_missing, ["extra"]);
        assert_eq!(
            r.new_pins
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["linux"]
        );
    }

    #[test]
    fn a_part_outside_the_scope_is_not_compared() {
        let ex = sample();
        let scope = Scope {
            packages: false,
            config: false,
            repos: false,
            pins: false,
        };
        assert_eq!(
            compare(&ex, scope, &[], &Config::default(), &[], &[]),
            ImportReport::default()
        );
    }
}

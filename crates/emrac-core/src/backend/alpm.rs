use std::process::Command;

use alpm::{Alpm, SigLevel};

use crate::error::{Error, Result};
use crate::package::{PackageDetails, PackageSummary};

/// Read-only access to the official repositories via libalpm, scoped to the
/// sync databases pacman has already downloaded locally (`/var/lib/pacman/sync`).
/// No network access, no root required.
pub struct AlpmBackend {
    handle: Alpm,
}

impl AlpmBackend {
    pub fn init() -> Result<Self> {
        let root = pacman_conf("RootDir")?;
        let db_path = pacman_conf("DBPath")?;
        let repos = pacman_conf_list("--repo-list")?;

        let handle = Alpm::new(root, db_path).map_err(Error::AlpmInit)?;

        for repo in repos {
            handle
                .register_syncdb(repo.clone(), SigLevel::USE_DEFAULT)
                .map_err(|source| Error::RegisterSyncDb { repo, source })?;
        }

        Ok(Self { handle })
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<PackageSummary> {
        let needle = query.to_lowercase();
        let mut results: Vec<PackageSummary> = Vec::new();

        for db in self.handle.syncdbs() {
            for pkg in db.pkgs() {
                let name_match = pkg.name().to_lowercase().contains(&needle);
                let desc_match = pkg
                    .desc()
                    .map(|d| d.to_lowercase().contains(&needle))
                    .unwrap_or(false);

                if name_match || desc_match {
                    results.push(PackageSummary {
                        name: pkg.name().to_string(),
                        version: pkg.version().to_string(),
                        repo: db.name().to_string(),
                        description: pkg.desc().map(str::to_string),
                    });
                }
            }
        }

        results.sort_by(|a, b| a.name.cmp(&b.name).then(a.repo.cmp(&b.repo)));
        results.dedup_by(|a, b| a.name == b.name);

        if let Some(limit) = limit {
            results.truncate(limit);
        }

        results
    }

    pub fn info(&self, name: &str) -> Result<PackageDetails> {
        for db in self.handle.syncdbs() {
            if let Ok(pkg) = db.pkg(name) {
                return Ok(PackageDetails {
                    name: pkg.name().to_string(),
                    version: pkg.version().to_string(),
                    repo: db.name().to_string(),
                    description: pkg.desc().map(str::to_string),
                    license: pkg.licenses().into_iter().map(str::to_string).collect(),
                    url: pkg.url().map(str::to_string),
                    depends: pkg.depends().into_iter().map(|d| d.to_string()).collect(),
                    provides: pkg.provides().into_iter().map(|d| d.to_string()).collect(),
                    installed_size: Some(pkg.isize().max(0) as u64),
                    maintainer: None,
                    votes: None,
                    popularity: None,
                    out_of_date: None,
                });
            }
        }

        Err(Error::PackageNotFound(name.to_string()))
    }

    /// Looks up `names` in the sync databases (official repos) — used to
    /// enrich an install plan with real repo/version/size data instead of
    /// parsing it out of pacman's text output.
    pub fn sync_resolved(&self, names: &[String]) -> Vec<ResolvedPackage> {
        names
            .iter()
            .filter_map(|name| {
                self.handle.syncdbs().into_iter().find_map(|db| {
                    db.pkg(name.as_str()).ok().map(|pkg| ResolvedPackage {
                        name: pkg.name().to_string(),
                        version: pkg.version().to_string(),
                        repo: db.name().to_string(),
                        download_size: pkg.size().max(0) as u64,
                        installed_size: pkg.isize().max(0) as u64,
                    })
                })
            })
            .collect()
    }

    /// Looks up `names` in the local database (currently installed
    /// packages) — used to enrich a removal plan with the size that will
    /// actually be freed.
    pub fn local_resolved(&self, names: &[String]) -> Vec<ResolvedPackage> {
        let local = self.handle.localdb();
        names
            .iter()
            .filter_map(|name| {
                local.pkg(name.as_str()).ok().map(|pkg| ResolvedPackage {
                    name: pkg.name().to_string(),
                    version: pkg.version().to_string(),
                    repo: "local".to_string(),
                    download_size: 0,
                    installed_size: pkg.isize().max(0) as u64,
                })
            })
            .collect()
    }

    /// Names of installed packages that aren't in any sync database — the
    /// standard definition of a "foreign" package (matches `pacman -Qm`),
    /// almost always AUR-built. Used to find AUR packages that might need
    /// upgrading without shelling out.
    pub fn foreign_package_names(&self) -> Vec<String> {
        self.handle
            .localdb()
            .pkgs()
            .into_iter()
            .filter(|pkg| {
                !self
                    .handle
                    .syncdbs()
                    .into_iter()
                    .any(|db| db.pkg(pkg.name()).is_ok())
            })
            .map(|pkg| pkg.name().to_string())
            .collect()
    }

    /// Where an installed package came from, or `None` if it isn't
    /// installed at all. Used to route a named `upgrade` target to the
    /// right backend (official vs. AUR).
    pub fn installed_source(&self, name: &str) -> Option<InstalledSource> {
        self.handle.localdb().pkg(name).ok()?;

        let is_official = self
            .handle
            .syncdbs()
            .into_iter()
            .any(|db| db.pkg(name).is_ok());

        Some(if is_official {
            InstalledSource::Official
        } else {
            InstalledSource::Foreign
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledSource {
    Official,
    Foreign,
}

/// `true` if `candidate` is a newer version than `current`, using libalpm's
/// own version-comparison rules (handles epoch/pkgrel correctly, not just
/// naive string comparison).
pub fn version_is_newer(current: &str, candidate: &str) -> bool {
    alpm::vercmp(candidate, current) == std::cmp::Ordering::Greater
}

pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub download_size: u64,
    pub installed_size: u64,
}

fn pacman_conf(key: &str) -> Result<String> {
    let output = Command::new("pacman-conf")
        .arg(key)
        .output()
        .map_err(Error::PacmanConfSpawn)?;

    if !output.status.success() {
        return Err(Error::PacmanConfStatus(format!(
            "pacman-conf {key} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn pacman_conf_list(arg: &str) -> Result<Vec<String>> {
    let output = Command::new("pacman-conf")
        .arg(arg)
        .output()
        .map_err(Error::PacmanConfSpawn)?;

    if !output.status.success() {
        return Err(Error::PacmanConfStatus(format!(
            "pacman-conf {arg} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

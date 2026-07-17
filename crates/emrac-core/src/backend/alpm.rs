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
                    installed_size: pkg.isize().max(0) as u64,
                });
            }
        }

        Err(Error::PackageNotFound(name.to_string()))
    }
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

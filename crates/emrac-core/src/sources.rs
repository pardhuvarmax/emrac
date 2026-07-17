use std::collections::HashSet;

use crate::backend::{
    AlpmBackend, AurBackend, AurBuildBackend, AurSyncOutcome, PacmanBackend, ResolvedPackage,
};
use crate::error::{Error, Result};
use crate::package::{PackageDetails, PackageSummary};
use crate::plan::{Plan, PlanAction, PlannedPackage};

/// Which package sources a search should cover. Defaults to both.
#[derive(Debug, Clone, Copy)]
pub struct SearchScope {
    pub official: bool,
    pub aur: bool,
}

impl Default for SearchScope {
    fn default() -> Self {
        Self {
            official: true,
            aur: true,
        }
    }
}

pub struct SearchResults {
    pub packages: Vec<PackageSummary>,
    /// Set when the AUR was in scope but couldn't be reached. Official
    /// results are still returned in that case rather than failing outright.
    pub aur_warning: Option<String>,
}

/// The single entry point the CLI (and, later, the TUI) should use: merges
/// official repos (via libalpm) and the AUR (via its RPC API) behind one
/// `search`/`info` surface.
pub struct Sources {
    alpm: AlpmBackend,
    aur: AurBackend,
    aur_build: AurBuildBackend,
    pacman: PacmanBackend,
}

impl Sources {
    pub fn init() -> Result<Self> {
        Ok(Self {
            alpm: AlpmBackend::init()?,
            aur: AurBackend::new(),
            aur_build: AurBuildBackend::new(),
            pacman: PacmanBackend::new(),
        })
    }

    pub fn search(
        &self,
        query: &str,
        scope: SearchScope,
        limit: Option<usize>,
    ) -> Result<SearchResults> {
        let mut packages = Vec::new();
        let mut aur_warning = None;

        if scope.official {
            packages.extend(self.alpm.search(query, None));
        }

        if scope.aur {
            match self.aur.search(query) {
                Ok(aur_packages) => packages.extend(aur_packages),
                Err(err) => aur_warning = Some(err.to_string()),
            }
        }

        if let Some(limit) = limit {
            packages.truncate(limit);
        }

        Ok(SearchResults {
            packages,
            aur_warning,
        })
    }

    /// Tries official repos first, then the AUR unless `offline` is set.
    pub fn info(&self, name: &str, offline: bool) -> Result<PackageDetails> {
        match self.alpm.info(name) {
            Ok(details) => return Ok(details),
            Err(Error::PackageNotFound(_)) => {}
            Err(other) => return Err(other),
        }

        if offline {
            return Err(Error::PackageNotFoundOffline(name.to_string()));
        }

        match self.aur.info(name)? {
            Some(details) => Ok(details),
            None => Err(Error::PackageNotFoundAnywhere(name.to_string())),
        }
    }

    /// Splits `pkgs` into what's available in the official repos vs. the
    /// AUR, so install can take a different path for each. Not found
    /// anywhere (respecting `offline`) is an error, same as `info`.
    pub fn classify_install(
        &self,
        pkgs: &[String],
        offline: bool,
    ) -> Result<(Vec<String>, Vec<PackageDetails>)> {
        let mut official = Vec::new();
        let mut aur = Vec::new();

        for pkg in pkgs {
            match self.alpm.info(pkg) {
                Ok(_) => official.push(pkg.clone()),
                Err(Error::PackageNotFound(_)) => {
                    if offline {
                        return Err(Error::PackageNotFoundOffline(pkg.clone()));
                    }
                    match self.aur.info(pkg)? {
                        Some(details) => aur.push(details),
                        None => return Err(Error::PackageNotFoundAnywhere(pkg.clone())),
                    }
                }
                Err(other) => return Err(other),
            }
        }

        Ok((official, aur))
    }

    /// Resolves what `install` would do, official repos and the AUR alike.
    /// No root, and no filesystem/build side effects — AUR entries are
    /// previewed from AUR metadata alone, not by cloning yet.
    pub fn plan_install(&self, pkgs: &[String], offline: bool) -> Result<Plan> {
        let (official, aur) = self.classify_install(pkgs, offline)?;

        let mut packages = Vec::new();
        let mut total_download_size = 0u64;
        let mut total_installed_size_delta = 0i64;

        if !official.is_empty() {
            let names = self.pacman.resolve_install(&official)?;
            let resolved = self.alpm.sync_resolved(&names);
            let mut official_plan = build_plan(PlanAction::Install, &official, resolved, 1);
            total_download_size += official_plan.total_download_size;
            total_installed_size_delta += official_plan.total_installed_size_delta;
            packages.append(&mut official_plan.packages);
        }

        for details in aur {
            packages.push(PlannedPackage {
                name: details.name,
                version: details.version,
                repo: "aur".to_string(),
                explicit: true,
            });
        }

        Ok(Plan {
            action: PlanAction::Install,
            packages,
            total_download_size,
            total_installed_size_delta,
        })
    }

    /// Actually installs `names` (official repos only) via `sudo pacman
    /// -S`. Prompts for a password interactively if needed.
    pub fn install_official(&self, names: &[String]) -> Result<()> {
        self.pacman.execute_install(names)
    }

    /// Clones (first time) or fetches (repeat) the AUR package's git
    /// checkout, reporting whether there's anything new to review. Doesn't
    /// advance an existing checkout — call `aur_advance` after the caller
    /// confirms a `Changed` diff.
    pub fn aur_sync(&self, name: &str) -> Result<AurSyncOutcome> {
        self.aur_build.sync(name)
    }

    /// Fast-forwards the cached checkout to what `aur_sync` last fetched.
    pub fn aur_advance(&self, name: &str) -> Result<()> {
        self.aur_build.advance(name)
    }

    /// Reads the PKGBUILD off the current checkout (the first-build case,
    /// where there's no previous version to diff against).
    pub fn read_pkgbuild(&self, name: &str) -> Result<String> {
        self.aur_build.read_pkgbuild(name)
    }

    /// Builds and installs an AUR package via `makepkg -si`. Prompts
    /// interactively (build output, dependency sync, sudo password) same
    /// as a real terminal session with `makepkg` would.
    pub fn build_aur(&self, name: &str) -> Result<()> {
        self.aur_build.build_and_install(name)
    }

    /// Resolves what `remove` would do. No root, nothing is mutated.
    pub fn plan_remove(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<Plan> {
        let names = self.pacman.resolve_remove(pkgs, cascade, recursive)?;
        let resolved = self.alpm.local_resolved(&names);
        Ok(build_plan(PlanAction::Remove, pkgs, resolved, -1))
    }

    /// Actually removes `pkgs` via `sudo pacman -R`. Prompts for a
    /// password interactively if needed.
    pub fn remove(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<()> {
        self.pacman.execute_remove(pkgs, cascade, recursive)
    }
}

/// Shared by `plan_install`/`plan_remove`: turns resolved packages into a
/// `Plan`, marking which ones the user explicitly asked for vs. which came
/// along as dependencies/dependents. `size_sign` is `1` for install
/// (size added) or `-1` for remove (size freed).
fn build_plan(
    action: PlanAction,
    requested: &[String],
    resolved: Vec<ResolvedPackage>,
    size_sign: i64,
) -> Plan {
    let explicit: HashSet<&str> = requested.iter().map(String::as_str).collect();

    let mut total_download_size = 0u64;
    let mut total_installed_size_delta = 0i64;

    let packages = resolved
        .into_iter()
        .map(|r| {
            total_download_size += r.download_size;
            total_installed_size_delta += size_sign * r.installed_size as i64;
            PlannedPackage {
                explicit: explicit.contains(r.name.as_str()),
                name: r.name,
                version: r.version,
                repo: r.repo,
            }
        })
        .collect();

    Plan {
        action,
        packages,
        total_download_size,
        total_installed_size_delta,
    }
}

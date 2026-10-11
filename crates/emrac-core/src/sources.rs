use std::collections::HashSet;
use std::path::PathBuf;

use crate::backend::{
    AlpmBackend, ArchiveBackend, AurBackend, AurBuildBackend, AurSyncOutcome, InstalledSource,
    PacmanBackend, ResolvedPackage, version_is_newer,
};
use crate::downgrade::{self, DowngradePlan, VersionList};
use crate::error::{Error, Result};
use crate::history::{History, TxStatus};
use crate::package::{PackageDetails, PackageSummary};
use crate::plan::{Plan, PlanAction, PlannedPackage};
use crate::rollback::{self, Download, RemoteFile, RollbackPlan};

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

pub struct UpgradePlan {
    pub plan: Plan,
    /// Set on a full upgrade (no explicit targets) when the AUR couldn't be
    /// reached while checking for upgradable foreign packages — official
    /// upgrades are still returned in that case. A targeted AUR upgrade
    /// treats the same failure as a hard error instead, same distinction
    /// `search` vs. `info` already draw.
    pub aur_warning: Option<String>,
}

/// The single entry point the CLI (and, later, the TUI) should use: merges
/// official repos (via libalpm) and the AUR (via its RPC API) behind one
/// `search`/`info` surface.
pub struct Sources {
    alpm: AlpmBackend,
    aur: AurBackend,
    aur_build: AurBuildBackend,
    archive: ArchiveBackend,
    pacman: PacmanBackend,
}

impl Sources {
    pub fn init() -> Result<Self> {
        Ok(Self {
            alpm: AlpmBackend::init()?,
            aur: AurBackend::new(),
            aur_build: AurBuildBackend::new(),
            archive: ArchiveBackend::new(),
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
                previous_version: None,
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

    /// Splits explicitly-named upgrade targets into official vs. AUR.
    /// Unlike `classify_install`, every name must already be installed —
    /// `upgrade` isn't `install`.
    pub fn classify_upgrade(&self, pkgs: &[String]) -> Result<(Vec<String>, Vec<String>)> {
        let mut official = Vec::new();
        let mut aur = Vec::new();

        for pkg in pkgs {
            match self.alpm.installed_source(pkg) {
                Some(InstalledSource::Official) => official.push(pkg.clone()),
                Some(InstalledSource::Foreign) => aur.push(pkg.clone()),
                None => return Err(Error::PackageNotInstalledForUpgrade(pkg.clone())),
            }
        }

        Ok((official, aur))
    }

    /// Resolves what `upgrade` would do — everything (`pkgs` empty) or just
    /// the named targets. No root, and (same as `plan_install`) no
    /// filesystem/build side effects for AUR entries.
    pub fn plan_upgrade(&self, pkgs: &[String], offline: bool) -> Result<UpgradePlan> {
        if pkgs.is_empty() {
            self.plan_full_upgrade(offline)
        } else {
            self.plan_targeted_upgrade(pkgs)
        }
    }

    fn plan_full_upgrade(&self, offline: bool) -> Result<UpgradePlan> {
        let names = self.pacman.resolve_full_upgrade()?;
        let (mut packages, total_download_size, total_installed_size_delta) =
            self.official_upgrade_packages(&names);

        let mut aur_warning = None;

        if !offline {
            let foreign_names = self.alpm.foreign_package_names();
            if !foreign_names.is_empty() {
                match self.aur.info_multi(&foreign_names) {
                    Ok(aur_details) => {
                        let installed = self.alpm.local_resolved(&foreign_names);
                        for details in aur_details {
                            let Some(current) = installed.iter().find(|r| r.name == details.name)
                            else {
                                continue;
                            };
                            if version_is_newer(&current.version, &details.version) {
                                packages.push(PlannedPackage {
                                    name: details.name,
                                    version: details.version,
                                    repo: "aur".to_string(),
                                    explicit: true,
                                    previous_version: Some(current.version.clone()),
                                });
                            }
                        }
                    }
                    Err(err) => aur_warning = Some(err.to_string()),
                }
            }
        }

        Ok(UpgradePlan {
            plan: Plan {
                action: PlanAction::Upgrade,
                packages,
                total_download_size,
                total_installed_size_delta,
            },
            aur_warning,
        })
    }

    fn plan_targeted_upgrade(&self, pkgs: &[String]) -> Result<UpgradePlan> {
        let (official, aur) = self.classify_upgrade(pkgs)?;

        let names = if official.is_empty() {
            Vec::new()
        } else {
            self.pacman.resolve_install(&official)?
        };
        let (mut packages, total_download_size, total_installed_size_delta) =
            self.official_upgrade_packages(&names);

        for name in &aur {
            let details = self
                .aur
                .info(name)?
                .ok_or_else(|| Error::PackageNotFoundAnywhere(name.clone()))?;
            packages.push(PlannedPackage {
                previous_version: self.installed_version(&details.name),
                name: details.name,
                version: details.version,
                repo: "aur".to_string(),
                explicit: true,
            });
        }

        Ok(UpgradePlan {
            plan: Plan {
                action: PlanAction::Upgrade,
                packages,
                total_download_size,
                total_installed_size_delta,
            },
            aur_warning: None,
        })
    }

    /// Actually upgrades official packages: everything (`pkgs` empty) via a
    /// real combined `sudo pacman -Syu`, or just the named targets via the
    /// same path `install_official` already uses (`pacman -S` on an
    /// installed package upgrades it).
    pub fn upgrade_official(&self, pkgs: &[String]) -> Result<()> {
        if pkgs.is_empty() {
            self.pacman.execute_full_upgrade()
        } else {
            self.pacman.execute_install(pkgs)
        }
    }

    /// Shared by `plan_full_upgrade`/`plan_targeted_upgrade`: turns
    /// resolved official target names into `PlannedPackage`s with the *net*
    /// size delta (new installed size − old), unlike `build_plan`'s flat
    /// `size_sign` (install/remove never have both an old and new size).
    fn official_upgrade_packages(&self, names: &[String]) -> (Vec<PlannedPackage>, u64, i64) {
        let new = self.alpm.sync_resolved(names);
        let old = self.alpm.local_resolved(names);

        let mut packages = Vec::new();
        let mut total_download_size = 0u64;
        let mut total_installed_size_delta = 0i64;

        for pkg in new {
            let old_pkg = old.iter().find(|r| r.name == pkg.name);
            let old_size = old_pkg.map(|r| r.installed_size).unwrap_or(0);
            let previous_version = old_pkg.map(|r| r.version.clone());

            total_download_size += pkg.download_size;
            total_installed_size_delta += pkg.installed_size as i64 - old_size as i64;

            packages.push(PlannedPackage {
                explicit: true,
                name: pkg.name,
                version: pkg.version,
                repo: pkg.repo,
                previous_version,
            });
        }

        (packages, total_download_size, total_installed_size_delta)
    }

    /// Resolves what `remove` would do. No root, nothing is mutated.
    pub fn plan_remove(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<Plan> {
        let names = self.pacman.resolve_remove(pkgs, cascade, recursive)?;
        let resolved = self.alpm.local_resolved(&names);
        Ok(build_plan(PlanAction::Remove, pkgs, resolved, -1))
    }

    /// The installed version of `name`, if it is installed.
    pub fn installed_version(&self, name: &str) -> Option<String> {
        self.alpm.installed_version(name)
    }

    /// Plans reversing a recorded transaction: `tx` names one, `None` means
    /// the most recent one that can still be rolled back. No root, nothing
    /// is mutated. Package files missing from disk are looked up in the Arch
    /// Linux Archive (read-only) unless `offline`.
    pub fn plan_rollback(&self, tx: Option<&str>, offline: bool) -> Result<RollbackPlan> {
        self.plan_reversal(tx, false, offline)
    }

    /// Plans re-applying the most recent rollback — i.e. reversing it.
    pub fn plan_redo(&self, offline: bool) -> Result<RollbackPlan> {
        self.plan_reversal(None, true, offline)
    }

    fn plan_reversal(&self, tx: Option<&str>, redo: bool, offline: bool) -> Result<RollbackPlan> {
        let history = History::all()?;
        let cache_dirs = self.pacman.cache_dirs()?;
        let installed = |name: &str| self.alpm.installed_version(name);
        let find_file = |name: &str, version: &str| self.locate_file(&cache_dirs, name, version);
        let find_remote = |name: &str, version: &str| self.locate_remote(offline, name, version);

        if let Some(tx) = tx {
            let id = rollback::parse_tx_id(tx)?;
            let record = rollback::find_record(&history, &id)?;
            return rollback::plan_rollback(record, &history, &installed, &find_file, &find_remote);
        }

        // A failed transaction that never got as far as changing anything
        // (e.g. a locked database) leaves nothing to reverse; stepping past
        // it is what "undo the last thing" means. A committed one with
        // nothing left to reverse stops here and is reported as such.
        let candidates = if redo {
            rollback::redo_candidates(&history)
        } else {
            rollback::candidates(&history)
        };
        for record in candidates {
            let plan =
                rollback::plan_rollback(record, &history, &installed, &find_file, &find_remote)?;
            if plan.plan.is_empty() && record.status == TxStatus::Failed {
                continue;
            }
            return Ok(plan);
        }
        Err(if redo {
            Error::NoTransactionToRedo
        } else {
            Error::NoTransactionToRollBack
        })
    }

    /// A package file for exactly `name`/`version` already on this machine:
    /// pacman's cache, the AUR build dir, or emrac's archive cache.
    fn locate_file(&self, cache_dirs: &[PathBuf], name: &str, version: &str) -> Option<PathBuf> {
        let mut dirs = cache_dirs.to_vec();
        if let Ok(build_dir) = self.aur_build.build_dir(name) {
            dirs.push(build_dir);
        }
        if let Ok(archive_dir) = self.archive.cache_dir(name) {
            dirs.push(archive_dir);
        }
        rollback::find_package_file(&dirs, name, version)
    }

    /// The same file in the Arch Linux Archive (read-only lookup), unless
    /// `offline`.
    fn locate_remote(
        &self,
        offline: bool,
        name: &str,
        version: &str,
    ) -> Result<Option<RemoteFile>> {
        if offline {
            return Ok(None);
        }
        let Some(file) = self.archive.find(name, version)? else {
            return Ok(None);
        };
        Ok(Some(RemoteFile {
            dest: self.archive.cache_dir(name)?.join(&file.filename),
            url: file.url,
        }))
    }

    /// Downloads whatever hasn't been fetched yet, plus each detached
    /// signature (best effort — pacman verifies one when it sits beside the
    /// package).
    fn fetch_downloads(&self, downloads: &[Download]) -> Result<()> {
        for download in downloads {
            if !download.dest.exists() {
                self.archive.download(&download.url, &download.dest)?;
            }
            let sig_dest = PathBuf::from(format!("{}.sig", download.dest.display()));
            if !sig_dest.exists() {
                let _ = self
                    .archive
                    .download(&format!("{}.sig", download.url), &sig_dest);
            }
        }
        Ok(())
    }

    /// Plans `emrac downgrade`: `version` is the target, or `None` for the
    /// newest version older than the installed one. No root, nothing is
    /// written; the Archive is only listed (and not at all if `offline` or
    /// the package is foreign/AUR).
    pub fn plan_downgrade(
        &self,
        pkg: &str,
        version: Option<&str>,
        offline: bool,
    ) -> Result<DowngradePlan> {
        let installed = self
            .alpm
            .installed_version(pkg)
            .ok_or_else(|| Error::PackageNotInstalledForDowngrade(pkg.to_string()))?;
        let official = matches!(
            self.alpm.installed_source(pkg),
            Some(InstalledSource::Official)
        );
        let repo = self
            .alpm
            .sync_resolved(&[pkg.to_string()])
            .into_iter()
            .next()
            .map(|r| r.repo)
            .unwrap_or_else(|| "aur".to_string());
        let use_archive = official && !offline;

        let cache_dirs = self.pacman.cache_dirs()?;
        let target = if version.is_some() {
            downgrade::choose_target(pkg, &installed, version, &[])?
        } else {
            let mut available = self.local_versions(&cache_dirs, pkg);
            if use_archive {
                available.extend(self.archive.versions(pkg)?.unwrap_or_default());
            }
            downgrade::choose_target(pkg, &installed, None, &available)?
        };

        let mut files = Vec::new();
        let mut downloads = Vec::new();
        if let Some(path) = self.locate_file(&cache_dirs, pkg, &target) {
            files.push(path);
        } else if let Some(remote) = self.locate_remote(!use_archive, pkg, &target)? {
            files.push(remote.dest.clone());
            downloads.push(Download {
                name: pkg.to_string(),
                version: target.clone(),
                url: remote.url,
                dest: remote.dest,
            });
        } else {
            return Err(Error::VersionUnavailable {
                pkg: pkg.to_string(),
                version: target,
            });
        }

        Ok(DowngradePlan {
            plan: Plan {
                action: PlanAction::Downgrade,
                packages: vec![PlannedPackage {
                    name: pkg.to_string(),
                    version: target,
                    repo,
                    explicit: true,
                    previous_version: Some(installed),
                }],
                total_download_size: 0,
                total_installed_size_delta: 0,
            },
            files,
            downloads,
        })
    }

    /// Versions of `pkg` that `emrac downgrade` could go to (and the one
    /// installed), newest first.
    pub fn list_versions(&self, pkg: &str, offline: bool) -> Result<VersionList> {
        let installed = self.alpm.installed_version(pkg);
        let official = !matches!(
            self.alpm.installed_source(pkg),
            Some(InstalledSource::Foreign)
        );
        let local = self.local_versions(&self.pacman.cache_dirs()?, pkg);
        let archive = if official && !offline {
            self.archive.versions(pkg)?.unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(downgrade::merge_versions(
            pkg,
            installed.as_deref(),
            &local,
            &archive,
        ))
    }

    fn local_versions(&self, cache_dirs: &[PathBuf], pkg: &str) -> Vec<String> {
        let mut dirs = cache_dirs.to_vec();
        if let Ok(dir) = self.aur_build.build_dir(pkg) {
            dirs.push(dir);
        }
        if let Ok(dir) = self.archive.cache_dir(pkg) {
            dirs.push(dir);
        }
        rollback::package_versions_in_dirs(&dirs, pkg)
    }

    /// Fetches what's missing, then installs the chosen package file with
    /// `sudo pacman -U`.
    pub fn execute_downgrade(&self, downgrade: &DowngradePlan) -> Result<()> {
        self.fetch_downloads(&downgrade.downloads)?;
        self.pacman.execute_install_files(&downgrade.files)
    }

    /// Runs a rollback plan: a removal via `sudo pacman -R`, anything else
    /// by installing the located package files with `sudo pacman -U` —
    /// after fetching whichever of them still have to be downloaded. The
    /// detached signature is fetched next to each package, best effort;
    /// pacman verifies it when present.
    pub fn execute_rollback(&self, rollback: &RollbackPlan) -> Result<()> {
        self.fetch_downloads(&rollback.downloads)?;

        match rollback.plan.action {
            PlanAction::Remove => {
                let names: Vec<String> = rollback
                    .plan
                    .packages
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                self.pacman.execute_remove(&names, false, false)
            }
            _ => {
                let files: Vec<PathBuf> = rollback.files.clone();
                self.pacman.execute_install_files(&files)
            }
        }
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
                previous_version: None,
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

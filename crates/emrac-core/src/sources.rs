use std::collections::HashSet;
use std::path::PathBuf;

use crate::audit::{self, AuditReport};
use crate::backend::{
    version_is_newer, AlpmBackend, ArchiveBackend, AurBackend, AurBuildBackend, AurSyncOutcome,
    InstalledSource, PacmanBackend, ResolvedPackage,
};
use crate::deps::{self, DepsReport, Orphan, RdepsReport, WhyReport};
use crate::downgrade::{self, DowngradePlan, VersionList};
use crate::error::{Error, Result};
use crate::history::{now, History, TxStatus};
use crate::manifest::{self, Manifest};
use crate::package::{PackageDetails, PackageSummary};
use crate::pins::{self, Pin, PinStore};
use crate::plan::{Plan, PlanAction, PlannedPackage};
use crate::purge::PurgePlan;
use crate::reinstall::{self, ReinstallItem, ReinstallPlan, ReinstallSource};
use crate::rollback::{self, Download, RemoteFile, RollbackPlan};
use crate::snapshot::{
    self, RestoreEntry, RestorePlan, RestoreSource, Snapshot, SnapshotPackage, SnapshotStore,
};
use crate::verify::VerifyReport;

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

/// What `Sources::inspect_official` found.
pub struct OfficialInspection {
    /// Each inspected package file with its manifest.
    pub found: Vec<(PathBuf, Manifest)>,
    /// Packages whose file wasn't in the cache.
    pub missing: Vec<String>,
}

pub struct UpgradePlan {
    pub plan: Plan,
    /// Set on a full upgrade (no explicit targets) when the AUR couldn't be
    /// reached while checking for upgradable foreign packages — official
    /// upgrades are still returned in that case. A targeted AUR upgrade
    /// treats the same failure as a hard error instead, same distinction
    /// `search` vs. `info` already draw.
    pub aur_warning: Option<String>,
    /// Pinned packages that have an update waiting and are being held back
    /// (full upgrade only).
    pub held: Vec<Pin>,
    /// Pins whose expiry has passed; they are not enforced.
    pub expired_pins: Vec<Pin>,
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

    /// Downloads official packages (and their dependencies) into pacman's
    /// cache without installing them, so they can be inspected first.
    pub fn download_official(&self, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        self.pacman.execute_download(names)
    }

    /// The same for a full upgrade (honouring pins).
    pub fn download_official_upgrades(&self) -> Result<()> {
        let (active, _) = self.pins()?;
        let ignore: Vec<String> = active.into_iter().map(|p| p.name).collect();
        self.pacman.execute_download_upgrade(&ignore)
    }

    /// Inspects the cached package file of every official package in `plan`
    /// (AUR entries are inspected after their own build). Returns the
    /// manifests with their file paths, and the names whose file wasn't found.
    pub fn inspect_official(&self, plan: &Plan) -> Result<OfficialInspection> {
        let cache_dirs = self.pacman.cache_dirs()?;
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for pkg in plan.packages.iter().filter(|p| p.repo != "aur") {
            match rollback::find_package_file(&cache_dirs, &pkg.name, &pkg.version) {
                Some(path) => {
                    let manifest = inspect_package(&path)?;
                    found.push((path, manifest));
                }
                None => missing.push(pkg.name.clone()),
            }
        }
        Ok(OfficialInspection { found, missing })
    }

    /// Clones (first time) or fetches (repeat) the AUR package's git
    /// checkout, reporting whether there's anything new to review. Doesn't
    /// advance an existing checkout — call `aur_advance` after the caller
    /// confirms a `Changed` diff.
    pub fn aur_sync(&self, name: &str) -> Result<AurSyncOutcome> {
        self.aur_build.sync(name)
    }

    /// Audits a PKGBUILD that came from `name`'s checkout: `pkgbuild` is its
    /// text, and the `install=` scriptlet (if any) is read from the working
    /// tree or, with `rev`, from that git revision.
    pub fn audit_aur_text(&self, name: &str, pkgbuild: &str, rev: Option<&str>) -> AuditReport {
        let scriptlet = audit::scriptlet_name(pkgbuild).and_then(|file| {
            self.aur_build
                .read_file(name, rev, &file)
                .map(|text| (file, text))
        });
        // The rest of the repo: helper scripts, patches, committed binaries.
        let extras: Vec<audit::ExtraFile> = self
            .aur_build
            .list_files(name, rev)
            .into_iter()
            .filter(|f| f != "PKGBUILD")
            .take(audit::MAX_EXTRA_FILES)
            .filter_map(|path| {
                let bytes = self.aur_build.read_bytes(name, rev, &path)?;
                Some(audit::ExtraFile { path, bytes })
            })
            .collect();
        audit::audit(
            name,
            pkgbuild,
            scriptlet.as_ref().map(|(f, t)| (f.as_str(), t.as_str())),
            &extras,
        )
    }

    /// `emrac audit <aur package>`: audits what upstream has now (fetching,
    /// but never advancing, the checkout).
    pub fn audit_aur(&self, name: &str) -> Result<AuditReport> {
        Ok(match self.aur_build.sync(name)? {
            AurSyncOutcome::Changed { new_pkgbuild, .. } => {
                self.audit_aur_text(name, &new_pkgbuild, Some("FETCH_HEAD"))
            }
            AurSyncOutcome::FirstClone | AurSyncOutcome::UpToDate => {
                let pkgbuild = self.aur_build.read_pkgbuild(name)?;
                self.audit_aur_text(name, &pkgbuild, None)
            }
        })
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

    /// Builds an AUR package (`makepkg -s`) **without installing it** and
    /// returns the package files. Prompts interactively (build output,
    /// dependency sync, sudo password) like a real `makepkg` session.
    pub fn build_aur_package(&self, name: &str) -> Result<Vec<PathBuf>> {
        self.aur_build.build(name)?;
        self.aur_build.built_files(name)
    }

    /// Installs built package files with `sudo pacman -U`.
    pub fn install_built(&self, files: &[PathBuf]) -> Result<()> {
        self.pacman.execute_install_files(files)
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

    /// `(active, expired)` pins.
    fn pins(&self) -> Result<(Vec<Pin>, Vec<Pin>)> {
        Ok(pins::partition(PinStore::list()?, now()))
    }

    fn plan_full_upgrade(&self, offline: bool) -> Result<UpgradePlan> {
        let (active, expired_pins) = self.pins()?;
        let ignore: Vec<String> = active.iter().map(|p| p.name.clone()).collect();
        let names = self.pacman.resolve_full_upgrade(&ignore)?;
        let mut held_names: Vec<String> = if ignore.is_empty() {
            Vec::new()
        } else {
            // What would be upgraded without the pins: the difference is
            // what they hold back.
            let all = self.pacman.resolve_full_upgrade(&[])?;
            all.into_iter().filter(|n| ignore.contains(n)).collect()
        };
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
                                if ignore.contains(&details.name) {
                                    held_names.push(details.name);
                                    continue;
                                }
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
            held: active
                .into_iter()
                .filter(|p| held_names.contains(&p.name))
                .collect(),
            expired_pins,
        })
    }

    fn plan_targeted_upgrade(&self, pkgs: &[String]) -> Result<UpgradePlan> {
        let (active, _) = self.pins()?;
        if let Some(pin) = active.iter().find(|p| pkgs.contains(&p.name)) {
            let detail = match pin.describe() {
                d if d.is_empty() => String::new(),
                d => format!(" ({d})"),
            };
            return Err(Error::PackagePinned {
                name: pin.name.clone(),
                detail,
            });
        }
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
            held: Vec::new(),
            expired_pins: Vec::new(),
        })
    }

    /// Actually upgrades official packages: everything (`pkgs` empty) via a
    /// real combined `sudo pacman -Syu`, or just the named targets via the
    /// same path `install_official` already uses (`pacman -S` on an
    /// installed package upgrades it).
    pub fn upgrade_official(&self, pkgs: &[String]) -> Result<()> {
        if pkgs.is_empty() {
            let (active, _) = self.pins()?;
            let ignore: Vec<String> = active.into_iter().map(|p| p.name).collect();
            self.pacman.execute_full_upgrade(&ignore)
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

    /// Verifies the installed files of each named package against pacman's
    /// record of them. Read-only; no root. Names that aren't installed are
    /// an error, reported together.
    pub fn verify(&self, pkgs: &[String]) -> Result<VerifyReport> {
        let mut names: Vec<&String> = Vec::new();
        for pkg in pkgs {
            if !names.contains(&pkg) {
                names.push(pkg);
            }
        }
        let missing: Vec<&str> = names
            .iter()
            .filter(|p| self.alpm.installed_version(p).is_none())
            .map(|p| p.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(Error::NotInstalledForVerify(missing.join(", ")));
        }

        let no_extract = self.pacman.no_extract()?;
        let packages = names
            .into_iter()
            .map(|name| self.alpm.verify_installed(name, &no_extract))
            .collect::<Result<Vec<_>>>()?;
        Ok(VerifyReport { packages })
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

    /// Plans `emrac reinstall`: the installed version of each named package
    /// again, never the repositories' newer one (see `reinstall.rs`). No
    /// root and nothing written; the Archive is only listed, and only for an
    /// official package whose installed version is neither on disk nor still
    /// in the repositories (and not at all if `offline`).
    pub fn plan_reinstall(&self, pkgs: &[String], offline: bool) -> Result<ReinstallPlan> {
        let mut names: Vec<&String> = Vec::new();
        for pkg in pkgs {
            if !names.contains(&pkg) {
                names.push(pkg);
            }
        }

        let missing: Vec<&str> = names
            .iter()
            .filter(|p| self.alpm.installed_version(p).is_none())
            .map(|p| p.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(Error::NotInstalledForReinstall(missing.join(", ")));
        }

        let cache_dirs = self.pacman.cache_dirs()?;
        let mut packages = Vec::new();
        let mut items = Vec::new();
        let mut files = Vec::new();
        let mut repository = Vec::new();
        let mut rebuild = Vec::new();
        let mut downloads = Vec::new();
        let mut problems = Vec::new();

        for pkg in names {
            let installed = self
                .alpm
                .installed_version(pkg)
                .expect("checked installed above");
            let foreign = matches!(
                self.alpm.installed_source(pkg),
                Some(InstalledSource::Foreign)
            );
            let sync = self
                .alpm
                .sync_resolved(std::slice::from_ref(pkg))
                .into_iter()
                .next();
            let repo = sync
                .as_ref()
                .map(|r| r.repo.clone())
                .unwrap_or_else(|| "aur".to_string());
            let local_file = self.locate_file(&cache_dirs, pkg, &installed);

            let mut remote: Option<RemoteFile> = None;
            let mut remote_error: Option<Error> = None;
            let source = reinstall::choose_source(
                foreign,
                &installed,
                sync.as_ref().map(|r| r.version.as_str()),
                local_file.is_some(),
                !offline,
                || {
                    match self.locate_remote(false, pkg, &installed) {
                        Ok(found) => remote = found,
                        Err(err) => remote_error = Some(err),
                    }
                    remote.is_some()
                },
                self.aur_build.has_checkout(pkg),
            );
            // An Archive we couldn't reach is not "the Archive doesn't have
            // it" — surface that instead of a misleading reason.
            if let Some(err) = remote_error {
                return Err(err);
            }
            let source = match source {
                Ok(source) => source,
                Err(why) => {
                    problems.push(format!("{pkg} ({why})"));
                    continue;
                }
            };

            match source {
                ReinstallSource::File => files.extend(local_file),
                ReinstallSource::Repository => repository.push(pkg.clone()),
                ReinstallSource::Rebuild => rebuild.push(pkg.clone()),
                ReinstallSource::Archive => {
                    let remote = remote.expect("Archive source implies a remote file");
                    files.push(remote.dest.clone());
                    downloads.push(Download {
                        name: pkg.clone(),
                        version: installed.clone(),
                        url: remote.url,
                        dest: remote.dest,
                    });
                }
            }
            items.push(ReinstallItem {
                name: pkg.clone(),
                version: installed.clone(),
                source,
            });
            packages.push(PlannedPackage {
                name: pkg.clone(),
                version: installed,
                repo,
                explicit: true,
                previous_version: None,
            });
        }

        if !problems.is_empty() {
            return Err(Error::CannotReinstall(problems.join("; ")));
        }

        Ok(ReinstallPlan {
            plan: Plan {
                action: PlanAction::Reinstall,
                packages,
                total_download_size: 0,
                total_installed_size_delta: 0,
            },
            items,
            files,
            repository,
            rebuild,
            downloads,
        })
    }

    /// Runs a reinstall plan: fetches what's missing, then package files
    /// (`pacman -U`), then repository packages (`pacman -S`), then AUR
    /// rebuilds (`makepkg -si` on the existing checkout). Stops at the first
    /// failure.
    pub fn execute_reinstall(&self, reinstall: &ReinstallPlan) -> Result<()> {
        self.fetch_downloads(&reinstall.downloads)?;
        if !reinstall.files.is_empty() {
            self.pacman.execute_install_files(&reinstall.files)?;
        }
        if !reinstall.repository.is_empty() {
            self.pacman.execute_install(&reinstall.repository)?;
        }
        // AUR rebuilds are driven by the caller (build, manifest, install).
        Ok(())
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
                self.pacman.execute_remove(&names, false, false, false)
            }
            _ => {
                let files: Vec<PathBuf> = rollback.files.clone();
                self.pacman.execute_install_files(&files)
            }
        }
    }

    /// `emrac pin`: holds installed packages back from `upgrade`.
    pub fn pin(
        &self,
        names: &[String],
        reason: Option<&str>,
        until: Option<u64>,
    ) -> Result<Vec<Pin>> {
        let missing: Vec<&str> = names
            .iter()
            .filter(|n| self.alpm.installed_version(n).is_none())
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(Error::NotInstalled(missing.join(", ")));
        }
        PinStore::pin(names, reason, until)
    }

    /// `emrac deps`: direct dependencies, optionally the whole closure.
    pub fn deps(&self, pkg: &str, recursive: bool) -> Result<DepsReport> {
        let (version, installed, depends, optional) = self
            .alpm
            .dependencies(pkg)
            .ok_or_else(|| Error::NoSuchPackage(pkg.to_string()))?;
        let recursive = (recursive && installed)
            .then(|| deps::closure(pkg, &|n| self.alpm.installed_dependency_names(n)));
        Ok(DepsReport {
            name: pkg.to_string(),
            version,
            installed,
            depends,
            optional,
            recursive,
        })
    }

    /// `emrac rdeps`: installed packages that need `pkg`.
    pub fn rdeps(&self, pkg: &str) -> Result<RdepsReport> {
        let (required_by, optional_for) = self
            .alpm
            .dependents(pkg)
            .ok_or_else(|| Error::NotInstalled(pkg.to_string()))?;
        Ok(RdepsReport {
            name: pkg.to_string(),
            required_by,
            optional_for,
        })
    }

    /// `emrac why`: why `pkg` is installed.
    pub fn why(&self, pkg: &str) -> Result<WhyReport> {
        let (required_by, _) = self
            .alpm
            .dependents(pkg)
            .ok_or_else(|| Error::NotInstalled(pkg.to_string()))?;
        let name = pkg.to_string();
        if self.alpm.is_explicit(pkg) {
            return Ok(WhyReport::Explicit { name, required_by });
        }
        let chain = deps::why_chain(
            pkg,
            &|n| self.alpm.dependents(n).map(|d| d.0).unwrap_or_default(),
            &|n| self.alpm.is_explicit(n),
        );
        Ok(match chain {
            Some(chain) => WhyReport::Dependency { name, chain },
            None => WhyReport::Orphan { name },
        })
    }

    /// `emrac orphans`.
    pub fn orphans(&self) -> Vec<Orphan> {
        self.alpm.orphans()
    }

    /// Saves the installed package set as a named restore point.
    pub fn create_snapshot(&self, name: Option<&str>, note: Option<&str>) -> Result<Snapshot> {
        SnapshotStore::create(name, note, self.alpm.installed_snapshot())
    }

    /// The restore point taken before an upgrade when the user opted in:
    /// `pre-upgrade-<unix time>`, keeping only the newest few.
    pub fn auto_snapshot(&self) -> Result<Snapshot> {
        const PREFIX: &str = "pre-upgrade-";
        const KEEP: usize = 5;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = format!("{PREFIX}{stamp}");
        let snapshot = self.create_snapshot(Some(&name), Some("before emrac upgrade"))?;
        SnapshotStore::prune_prefixed(PREFIX, KEEP)?;
        Ok(snapshot)
    }

    /// Plans `emrac restore`: what it takes to make the installed set match
    /// `snapshot`. No root, nothing is changed or downloaded; package files
    /// are found the way a rollback finds them (and the Archive only listed,
    /// unless `offline`). Every file that can't be found is reported at once.
    pub fn plan_restore(&self, name: &str, prune: bool, offline: bool) -> Result<RestorePlan> {
        let snapshot = SnapshotStore::load(name)?;
        let installed: Vec<(String, String)> = self
            .alpm
            .installed_snapshot()
            .into_iter()
            .map(|p: SnapshotPackage| (p.name, p.version))
            .collect();
        let diff = snapshot::diff(&snapshot, &installed, &version_is_newer);
        let cache_dirs = self.pacman.cache_dirs()?;

        let mut entries = Vec::new();
        let mut files = Vec::new();
        let mut downloads = Vec::new();
        let mut repository = Vec::new();
        let mut missing = Vec::new();

        for item in diff.items {
            if let Some(path) = self.locate_file(&cache_dirs, &item.name, &item.to) {
                files.push(path);
                entries.push(RestoreEntry {
                    item,
                    source: RestoreSource::File,
                });
                continue;
            }
            if item.foreign {
                missing.push(format!("{} {}", item.name, item.to));
                continue;
            }
            // A missing package that the repositories still carry at the
            // snapshot's version needs no file.
            let sync = self
                .alpm
                .sync_resolved(std::slice::from_ref(&item.name))
                .into_iter()
                .next();
            if item.action == PlanAction::Install
                && sync.as_ref().is_some_and(|s| s.version == item.to)
            {
                repository.push(item.name.clone());
                entries.push(RestoreEntry {
                    item,
                    source: RestoreSource::Repository,
                });
                continue;
            }
            match self.locate_remote(offline, &item.name, &item.to)? {
                Some(remote) => {
                    downloads.push(Download {
                        name: item.name.clone(),
                        version: item.to.clone(),
                        url: remote.url,
                        dest: remote.dest.clone(),
                    });
                    files.push(remote.dest);
                    entries.push(RestoreEntry {
                        item,
                        source: RestoreSource::Archive,
                    });
                }
                None => missing.push(format!("{} {}", item.name, item.to)),
            }
        }

        if !missing.is_empty() {
            return Err(Error::RestoreFilesMissing(missing.join(", ")));
        }

        Ok(RestorePlan {
            snapshot: snapshot.name,
            entries,
            extras: diff.extras,
            prune,
            files,
            downloads,
            repository,
        })
    }

    /// Carries out a restore, one pacman call at a time (`-U` for files,
    /// `-S` for repository installs, then `-R` for `--prune`), stopping at
    /// the first failure. `on_step` is told about every call that ran, as a
    /// plan per action, so each can be recorded in the history.
    pub fn execute_restore(
        &self,
        restore: &RestorePlan,
        on_step: &mut dyn FnMut(&Plan, &Result<()>),
    ) -> Result<()> {
        self.fetch_downloads(&restore.downloads)?;

        let plans = |source_is_repo: bool| -> Vec<Plan> {
            [
                PlanAction::Install,
                PlanAction::Upgrade,
                PlanAction::Downgrade,
            ]
            .into_iter()
            .filter_map(|action| {
                let packages: Vec<PlannedPackage> = restore
                    .entries
                    .iter()
                    .filter(|e| {
                        e.item.action == action
                            && (e.source == RestoreSource::Repository) == source_is_repo
                    })
                    .map(|e| PlannedPackage {
                        name: e.item.name.clone(),
                        version: e.item.to.clone(),
                        repo: if e.item.foreign { "aur" } else { "restore" }.to_string(),
                        explicit: true,
                        previous_version: e.item.from.clone(),
                    })
                    .collect();
                (!packages.is_empty()).then_some(Plan {
                    action,
                    packages,
                    total_download_size: 0,
                    total_installed_size_delta: 0,
                })
            })
            .collect()
        };

        if !restore.files.is_empty() {
            let result = self.pacman.execute_install_files(&restore.files);
            for plan in plans(false) {
                on_step(&plan, &result);
            }
            result?;
        }
        if !restore.repository.is_empty() {
            let result = self.pacman.execute_install(&restore.repository);
            for plan in plans(true) {
                on_step(&plan, &result);
            }
            result?;
        }
        if restore.prune && !restore.extras.is_empty() {
            let names: Vec<String> = restore.extras.iter().map(|(n, _)| n.clone()).collect();
            let result = self.pacman.execute_remove(&names, false, false, false);
            let plan = Plan {
                action: PlanAction::Remove,
                packages: restore
                    .extras
                    .iter()
                    .map(|(name, version)| PlannedPackage {
                        name: name.clone(),
                        version: version.clone(),
                        repo: "local".to_string(),
                        explicit: true,
                        previous_version: None,
                    })
                    .collect(),
                total_download_size: 0,
                total_installed_size_delta: 0,
            };
            on_step(&plan, &result);
            result?;
        }
        Ok(())
    }

    /// Actually removes `pkgs` via `sudo pacman -R`. Prompts for a
    /// password interactively if needed.
    pub fn remove(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<()> {
        self.pacman.execute_remove(pkgs, cascade, recursive, false)
    }

    /// Plans `emrac purge`: what `remove` would do, plus which of the
    /// packages' config files carry local changes that would be deleted
    /// rather than saved as `.pacsave`. No root, nothing is mutated.
    pub fn plan_purge(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<PurgePlan> {
        let mut plan = self.plan_remove(pkgs, cascade, recursive)?;
        plan.action = PlanAction::Purge;
        let names: Vec<String> = plan.packages.iter().map(|p| p.name.clone()).collect();
        let config_files = self.alpm.config_files(&names);
        Ok(PurgePlan { plan, config_files })
    }

    /// Removes `pkgs` via `sudo pacman -Rn`: modified config files are
    /// deleted instead of being saved as `.pacsave`.
    pub fn purge(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<()> {
        self.pacman.execute_remove(pkgs, cascade, recursive, true)
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

/// `emrac audit <path>`: a PKGBUILD file, or a directory holding one (its
/// `install=` scriptlet is read from beside it). Needs no `Sources`.
pub fn audit_path(path: &std::path::Path) -> Result<AuditReport> {
    let (dir, file) = if path.is_dir() {
        (path.to_path_buf(), path.join("PKGBUILD"))
    } else {
        (
            path.parent().map(PathBuf::from).unwrap_or_default(),
            path.to_path_buf(),
        )
    };
    let pkgbuild = std::fs::read_to_string(&file)
        .map_err(|_| Error::NoPkgbuild(file.display().to_string()))?;
    let scriptlet = audit::scriptlet_name(&pkgbuild).and_then(|name| {
        std::fs::read_to_string(dir.join(&name))
            .ok()
            .map(|t| (name, t))
    });
    // Auditing one file looks at the files beside it; a directory is walked.
    let extras = collect_extras(&dir, if path.is_dir() { 3 } else { 0 });
    Ok(audit::audit(
        &file.display().to_string(),
        &pkgbuild,
        scriptlet.as_ref().map(|(f, t)| (f.as_str(), t.as_str())),
        &extras,
    ))
}

/// The other files of a package directory, as `audit` wants them: at most
/// `MAX_EXTRA_FILES`, `max_depth` levels deep, skipping VCS data and makepkg's own
/// work and output (`src`, `pkg`, built packages).
fn collect_extras(dir: &std::path::Path, max_depth: usize) -> Vec<audit::ExtraFile> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: usize,
        max_depth: usize,
        out: &mut Vec<audit::ExtraFile>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if out.len() >= audit::MAX_EXTRA_FILES {
                return;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < max_depth && !matches!(name.as_str(), ".git" | "src" | "pkg") {
                    walk(root, &path, depth + 1, max_depth, out);
                }
            } else if kind.is_file() && !name.contains(".pkg.tar") {
                if let (Ok(bytes), Ok(rel)) = (std::fs::read(&path), path.strip_prefix(root)) {
                    out.push(audit::ExtraFile {
                        path: rel.display().to_string(),
                        bytes,
                    });
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, max_depth, &mut out);
    out
}

/// Reads a built package file (`*.pkg.tar.*`) and reports what it would
/// change on a system. Nothing is extracted or run. Needs no `Sources`.
pub fn inspect_package(path: &std::path::Path) -> Result<Manifest> {
    use crate::backend::{Extracted, PackageFile};
    let listing = PackageFile::list(path)?;
    let pkginfo = PackageFile::read(path, ".PKGINFO").unwrap_or_default();
    let scriptlet = PackageFile::read(path, ".INSTALL");
    // One extraction of just the small files worth reading, into a private
    // temporary directory that is removed afterwards.
    let extracted = Extracted::new(path, &manifest::wanted_members(&listing))?;
    Ok(manifest::inspect(
        &pkginfo,
        &listing,
        scriptlet.as_deref(),
        &|member| extracted.read(member),
    ))
}

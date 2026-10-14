use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::audit::{self, AuditReport};
use crate::backend::{
    version_is_newer, AlpmBackend, ArchiveBackend, AurBackend, AurBuildBackend, AurSyncOutcome,
    BuildOptions, FreshDb, InstalledSource, PacmanBackend, ResolvedPackage, SimOp, Simulation,
};
use crate::config::Config;
use crate::deps::{self, DepsReport, Orphan, RdepsReport, WhyReport};
use crate::doctor::{self, DiskFact, DoctorInputs, DoctorReport, ReportEnv, ToolFact};
use crate::downgrade::{self, DowngradePlan, VersionList};
use crate::error::{Error, Result};
use crate::history::{now, History, TxStatus};
use crate::manifest::{self, Manifest};
use crate::package::{PackageDetails, PackageSummary};
use crate::pins::{self, Pin, PinStore};
use crate::plan::{Plan, PlanAction, PlannedPackage, PlannedRemoval, ProviderChoice};
use crate::purge::PurgePlan;
use crate::reinstall::{self, ReinstallItem, ReinstallPlan, ReinstallSource};
use crate::rollback::{self, Download, RemoteFile, RollbackPlan};
use crate::sandbox;
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

/// How current the repository package list behind a plan is. An upgrade plan
/// is only meaningful against the repositories as they are *now*; against an
/// old list it silently leaves out everything published since.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum PackageList {
    /// Freshly synced, or refreshed within the last day.
    Fresh,
    /// Older than a day. `days` is its age (`None` if unknown); `why` says why
    /// it could not be refreshed for the preview.
    Stale {
        days: Option<u64>,
        why: Option<String>,
    },
}

/// Fresh if the list was refreshed within a day, otherwise stale.
pub fn package_list_state(age_days: Option<u64>, why: Option<String>) -> PackageList {
    if age_days.is_some_and(|d| d <= 1) {
        PackageList::Fresh
    } else {
        PackageList::Stale {
            days: age_days,
            why,
        }
    }
}

/// The repositories a full-upgrade plan was computed against.
struct UpgradeView {
    /// A handle on the fresh database, if there is one.
    alpm: Option<AlpmBackend>,
    dbpath: Option<PathBuf>,
    list: PackageList,
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
    /// How current the repository list this plan was computed from is.
    pub package_list: PackageList,
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

    /// Let pacman ask its own questions (package conflicts, provider choices,
    /// replacements, key imports) instead of taking `--noconfirm` defaults.
    /// The CLI turns this on unless `--yes` was given.
    pub fn set_interactive(&mut self, interactive: bool) {
        self.pacman.set_interactive(interactive);
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

        let mut plan = Plan {
            action: PlanAction::Install,
            packages,
            total_download_size,
            total_installed_size_delta,
            removals: Vec::new(),
            warnings: Vec::new(),
            provider_choices: Vec::new(),
        };
        if !official.is_empty() {
            attach_removals(
                &mut plan,
                AlpmBackend::simulate(None, SimOp::Install(&official)),
            );
        }
        Ok(plan)
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
    pub fn build_aur_package(&self, name: &str, opts: BuildOptions) -> Result<Vec<PathBuf>> {
        self.aur_build.build(name, opts)?;
        self.aur_build.built_files(name)
    }

    /// Builds the PKGBUILD in a local directory the same way (no install).
    pub fn build_dir_package(
        &self,
        dir: &std::path::Path,
        opts: BuildOptions,
    ) -> Result<Vec<PathBuf>> {
        self.aur_build.build_in(dir, opts)?;
        self.aur_build.built_files_in(dir)
    }

    /// Is a build sandbox usable here? The error says what to do.
    pub fn sandbox_check(&self) -> Result<()> {
        sandbox::probe().map_err(Error::SandboxUnavailable)
    }

    /// Whether the audit expects this checkout's build to use the network
    /// (`network-in-*` findings), so the user can be asked before it runs.
    pub fn predicts_network(&self, name: &str) -> bool {
        let Ok(pkgbuild) = self.aur_build.read_pkgbuild(name) else {
            return false;
        };
        self.audit_aur_text(name, &pkgbuild, None)
            .findings
            .iter()
            .any(|f| f.file == "PKGBUILD" && f.rule.starts_with("network-in-"))
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
        // The real upgrade syncs first, so the preview must too: against a
        // stale list it would leave out everything published since.
        let view = self.upgrade_view(offline);
        let dbpath = view.dbpath.as_deref();
        let names = self.pacman.resolve_full_upgrade(&ignore, dbpath)?;
        let mut held_names: Vec<String> = if ignore.is_empty() {
            Vec::new()
        } else {
            // What would be upgraded without the pins: the difference is
            // what they hold back.
            let all = self.pacman.resolve_full_upgrade(&[], dbpath)?;
            all.into_iter().filter(|n| ignore.contains(n)).collect()
        };
        let (mut packages, total_download_size, total_installed_size_delta) =
            self.official_upgrade_packages(&names, view.alpm.as_ref().unwrap_or(&self.alpm));

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

        let mut plan = Plan {
            action: PlanAction::Upgrade,
            packages,
            total_download_size,
            total_installed_size_delta,
            removals: Vec::new(),
            warnings: Vec::new(),
            provider_choices: Vec::new(),
        };
        // What `-Syu` would remove to resolve conflicts and replacements,
        // worked out against the same (fresh) databases as the plan itself.
        attach_removals(
            &mut plan,
            AlpmBackend::simulate(dbpath, SimOp::Upgrade { ignore: &ignore }),
        );
        Ok(UpgradePlan {
            plan,
            aur_warning,
            package_list: view.list,
            held: active
                .into_iter()
                .filter(|p| held_names.contains(&p.name))
                .collect(),
            expired_pins,
        })
    }

    /// The repositories to plan a full upgrade against: a freshly synced
    /// throwaway database when the network is reachable, otherwise pacman's
    /// own (possibly stale) one — and then the plan says so.
    fn upgrade_view(&self, offline: bool) -> UpgradeView {
        let age = self.sync_db_age_days();
        if offline {
            return UpgradeView {
                alpm: None,
                dbpath: None,
                list: package_list_state(age, Some("--offline".into())),
            };
        }
        let fresh = FreshDb::sync().and_then(|db| {
            let alpm = AlpmBackend::init_with_dbpath(db.path())?;
            Ok((alpm, db.path().to_path_buf()))
        });
        match fresh {
            Ok((alpm, path)) => UpgradeView {
                alpm: Some(alpm),
                dbpath: Some(path),
                list: PackageList::Fresh,
            },
            Err(err) => UpgradeView {
                alpm: None,
                dbpath: None,
                list: package_list_state(age, Some(err.to_string())),
            },
        }
    }

    /// Age in days of the newest sync database, if there is one.
    pub fn sync_db_age_days(&self) -> Option<u64> {
        let db_path = self.pacman.db_path().ok()?;
        std::fs::read_dir(db_path.join("sync"))
            .ok()?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "db"))
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .max()
            .map(|m| {
                std::time::SystemTime::now()
                    .duration_since(m)
                    .map(|d| d.as_secs() / 86_400)
                    .unwrap_or(0)
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
            self.official_upgrade_packages(&names, &self.alpm);

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

        let mut plan = Plan {
            action: PlanAction::Upgrade,
            packages,
            total_download_size,
            total_installed_size_delta,
            removals: Vec::new(),
            warnings: Vec::new(),
            provider_choices: Vec::new(),
        };
        if !official.is_empty() {
            attach_removals(
                &mut plan,
                AlpmBackend::simulate(None, SimOp::Install(&official)),
            );
        }
        Ok(UpgradePlan {
            plan,
            aur_warning: None,
            // Named packages are resolved and installed against the current
            // local databases both times, so preview and execution agree.
            package_list: PackageList::Fresh,
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
    fn official_upgrade_packages(
        &self,
        names: &[String],
        sync_view: &AlpmBackend,
    ) -> (Vec<PlannedPackage>, u64, i64) {
        let new = sync_view.sync_resolved(names);
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
                removals: Vec::new(),
                warnings: Vec::new(),
                provider_choices: Vec::new(),
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
                removals: Vec::new(),
                warnings: Vec::new(),
                provider_choices: Vec::new(),
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

    /// `emrac doctor`: gathers the facts and evaluates them. Read-only; nothing
    /// here needs root or the network.
    ///
    /// With `online`, the pending-upgrade count comes from a freshly synced
    /// throwaway database (needs the network); without it the check reports
    /// honestly how old the package list is instead.
    pub fn doctor(&self, online: bool) -> Result<DoctorReport> {
        Ok(doctor::evaluate(&self.doctor_inputs(online)))
    }

    /// The facts for the bug-report header.
    pub fn doctor_env(&self) -> ReportEnv {
        let (installed, foreign) = self.alpm.package_counts();
        let pacman_version = Command::new("pacman")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .find_map(|l| l.find("Pacman v").map(|at| l[at..].trim().to_string()))
            })
            .unwrap_or_else(|| "pacman (version unknown)".into());
        ReportEnv {
            emrac_version: env!("CARGO_PKG_VERSION").to_string(),
            pacman_version,
            kernel: read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_default(),
            installed,
            foreign,
        }
    }

    /// The report as a markdown block for a bug report, with the home
    /// directory, user name and host name scrubbed out.
    pub fn doctor_markdown(&self, report: &DoctorReport) -> String {
        let md = doctor::report_markdown(report, &self.doctor_env());
        let home = std::env::var("HOME").unwrap_or_default();
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_default();
        let host = read_trimmed("/proc/sys/kernel/hostname").unwrap_or_default();
        doctor::scrub(
            &md,
            &doctor::Scrub {
                home: &home,
                user: &user,
                host: &host,
            },
        )
    }

    fn doctor_inputs(&self, online: bool) -> DoctorInputs {
        let db_path = self
            .pacman
            .db_path()
            .unwrap_or_else(|_| PathBuf::from("/var/lib/pacman"));
        let cache_dirs = self.pacman.cache_dirs().unwrap_or_default();
        let now_secs = now();

        let sync_db_age_days = std::fs::read_dir(db_path.join("sync"))
            .ok()
            .and_then(|d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "db"))
                    .filter_map(|e| e.metadata().ok()?.modified().ok())
                    .max()
            })
            .map(|m| {
                std::time::SystemTime::now()
                    .duration_since(m)
                    .map(|d| d.as_secs() / 86_400)
                    .unwrap_or(0)
            });

        let mut disks = Vec::new();
        let mut df_targets: Vec<(String, PathBuf)> = vec![("/".into(), PathBuf::from("/"))];
        if let Some(c) = cache_dirs.first() {
            df_targets.push((c.display().to_string(), c.clone()));
        }
        let mut seen: Vec<(u64, u64)> = Vec::new();
        for (label, path) in df_targets {
            let out = Command::new("df").arg("-Pk").arg(&path).output();
            if let Some((avail, total)) = out
                .ok()
                .and_then(|o| doctor::parse_df(&String::from_utf8_lossy(&o.stdout)))
            {
                // The cache usually sits on `/`: report each filesystem once.
                if !seen.contains(&(avail, total)) {
                    seen.push((avail, total));
                    disks.push(DiskFact {
                        label,
                        available: avail,
                        total,
                    });
                }
            }
        }

        // Only meaningful where a kernel's modules are installed at all: a
        // container shares the host's kernel and has none of its own.
        let has_kernel_modules = std::fs::read_dir("/usr/lib/modules")
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
        let (pending_upgrades, pending_fresh, pending_error) = if online {
            match FreshDb::sync() {
                Ok(db) => (
                    self.pacman
                        .resolve_full_upgrade(&[], Some(db.path()))
                        .ok()
                        .map(|v| v.len()),
                    true,
                    None,
                ),
                Err(err) => (None, false, Some(err.to_string())),
            }
        } else {
            (
                self.pacman
                    .resolve_full_upgrade(&[], None)
                    .ok()
                    .map(|v| v.len()),
                false,
                None,
            )
        };
        let kernel_release =
            read_trimmed("/proc/sys/kernel/osrelease").filter(|_| has_kernel_modules);
        let kernel_modules_present = kernel_release
            .as_deref()
            .is_some_and(|r| Path::new("/usr/lib/modules").join(r).is_dir());

        let orphans = self.alpm.orphans();
        let (config_error, config_world_writable) = match Config::path() {
            Ok(path) => (
                Config::load().err().map(|e| e.to_string()),
                std::fs::metadata(&path)
                    .map(|m| {
                        use std::os::unix::fs::PermissionsExt;
                        m.permissions().mode() & 0o002 != 0
                    })
                    .unwrap_or(false),
            ),
            Err(_) => (None, false),
        };

        let expired_pins = self
            .pins()
            .map(|(_, expired)| expired.into_iter().map(|p| p.name).collect())
            .unwrap_or_default();
        let recent_failed_transactions = History::all()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.status == TxStatus::Failed && r.timestamp + 7 * 86_400 >= now_secs)
            .map(|r| format!("{} ({} {})", r.id, r.timestamp_utc(), action_word(r.action)))
            .collect();

        let tool = |name: &'static str,
                    package: &'static str,
                    needed_for: &'static str,
                    required: bool| ToolFact {
            name,
            package,
            present: on_path(name),
            needed_for,
            required,
        };

        DoctorInputs {
            unsatisfied: self.alpm.unsatisfied_dependencies(),
            lock_present: db_path.join("db.lck").exists(),
            pacman_running: Command::new("pgrep")
                .args(["-x", "pacman"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false),
            sync_db_age_days,
            pending_upgrades,
            pending_fresh,
            pending_error,
            pacnew_files: find_pacnew(Path::new("/etc")),
            failed_system_units: failed_units(&[]),
            failed_user_units: failed_units(&["--user"]),
            kernel_release,
            kernel_modules_present,
            disks,
            pacman_cache_bytes: (!cache_dirs.is_empty())
                .then(|| cache_dirs.iter().map(|d| dir_size(d, 2)).sum()),
            emrac_cache_bytes: std::env::var("HOME")
                .ok()
                .map(|h| dir_size(&PathBuf::from(h).join(".cache/emrac"), 6))
                .filter(|b| *b > 0),
            orphan_count: orphans.len(),
            orphan_bytes: orphans.iter().map(|o| o.installed_size).sum(),
            expired_pins,
            recent_failed_transactions,
            config: doctor::ConfigFacts {
                parse_error: config_error,
                world_writable: config_world_writable,
            },
            tools: vec![
                tool("makepkg", "base-devel", "building AUR packages", true),
                tool("git", "git", "fetching AUR packages", true),
                tool("bsdtar", "libarchive", "inspecting built packages", true),
                tool("fakeroot", "fakeroot", "building packages", true),
                tool("sudo", "sudo", "installing and removing packages", true),
                tool(
                    "pacman-conf",
                    "pacman",
                    "reading pacman's configuration",
                    true,
                ),
                tool("pacdiff", "pacman-contrib", "merging .pacnew files", false),
            ],
            sandbox: Some(sandbox::probe()),
            scope_available: Some(sandbox::scope_available(&sandbox::Limits::default())),
        }
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
                    removals: Vec::new(),
                    warnings: Vec::new(),
                    provider_choices: Vec::new(),
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
                removals: Vec::new(),
                warnings: Vec::new(),
                provider_choices: Vec::new(),
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
        removals: Vec::new(),
        warnings: Vec::new(),
        provider_choices: Vec::new(),
    }
}

/// Adds the removals libalpm says a transaction would cause to `plan`, and
/// counts the space they free. If the simulation could not run, says so in the
/// plan instead of silently leaving the section out.
fn attach_removals(plan: &mut Plan, simulated: Result<Simulation>) {
    match simulated {
        Ok(sim) => {
            for p in sim.providers {
                plan.provider_choices.push(ProviderChoice {
                    dependency: p.dependency,
                    providers: p.providers,
                });
            }
            for r in sim.removals {
                plan.total_installed_size_delta -= r.installed_size as i64;
                plan.removals.push(PlannedRemoval {
                    name: r.name,
                    version: r.version,
                    reason: r.reason,
                });
            }
        }
        Err(err) => plan.warnings.push(format!(
            "couldn't work out whether pacman will also remove packages to resolve conflicts ({err}); it will ask when it runs"
        )),
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
    let mut manifest = manifest::inspect(&pkginfo, &listing, scriptlet.as_deref(), &|member| {
        extracted.read(member)
    });
    // What installing it would remove: libalpm's own conflict and replacement
    // logic, run on the built file as `pacman -U` would. An AUR package's
    // conflicts are only known once it is built, so this is where they show.
    match AlpmBackend::simulate(None, SimOp::Files(&[path.to_path_buf()])) {
        Ok(sim) => {
            let removals: Vec<(String, String, String)> = sim
                .removals
                .into_iter()
                .map(|r| (r.name, r.version, r.reason))
                .collect();
            manifest::note_removals(&mut manifest, &removals);
        }
        Err(err) => manifest.summary.push(format!(
            "couldn't check for conflicts with installed packages: {err}"
        )),
    }
    Ok(manifest)
}

fn action_word(action: PlanAction) -> &'static str {
    match action {
        PlanAction::Install => "install",
        PlanAction::Remove => "remove",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Downgrade => "downgrade",
        PlanAction::Reinstall => "reinstall",
        PlanAction::Purge => "purge",
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Is `name` an executable somewhere on `$PATH`?
fn on_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            std::fs::metadata(dir.join(name))
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
    })
}

/// Names of failed systemd units (`extra` is `--user` for the user manager).
fn failed_units(extra: &[&str]) -> Vec<String> {
    Command::new("systemctl")
        .args(extra)
        .args(["--failed", "--no-legend", "--plain", "--no-pager"])
        .output()
        .map(|o| doctor::parse_failed_units(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// `*.pacnew` / `*.pacsave` files under `root`, a few levels deep.
fn find_pacnew(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if out.len() >= 200 {
                return;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < 5 {
                    walk(&path, depth + 1, out);
                }
            } else if kind.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".pacnew") || name.ends_with(".pacsave") {
                    out.push(path.display().to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out.sort();
    out
}

/// Total size of the files under `dir`, `depth` levels down.
fn dir_size(dir: &Path, depth: usize) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() && depth > 0 => dir_size(&e.path(), depth - 1),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod freshness_tests {
    use super::*;

    #[test]
    fn a_list_refreshed_today_or_yesterday_is_fresh() {
        for days in [0, 1] {
            assert_eq!(package_list_state(Some(days), None), PackageList::Fresh);
            // Even if a refresh was attempted and failed: the list is recent.
            assert_eq!(
                package_list_state(Some(days), Some("no network".into())),
                PackageList::Fresh
            );
        }
    }

    #[test]
    fn anything_older_or_of_unknown_age_is_stale_and_keeps_the_reason() {
        for days in [2, 10, 400] {
            assert_eq!(
                package_list_state(Some(days), Some("--offline".into())),
                PackageList::Stale {
                    days: Some(days),
                    why: Some("--offline".into())
                }
            );
        }
        assert_eq!(
            package_list_state(None, None),
            PackageList::Stale {
                days: None,
                why: None
            },
            "no date at all is not 'fresh'"
        );
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;
    use crate::backend::alpm::{Providers, Removal};

    fn empty_plan(delta: i64) -> Plan {
        Plan {
            action: PlanAction::Install,
            packages: Vec::new(),
            total_download_size: 0,
            total_installed_size_delta: delta,
            removals: Vec::new(),
            warnings: Vec::new(),
            provider_choices: Vec::new(),
        }
    }

    fn removal(name: &str, size: u64, reason: &str) -> Removal {
        Removal {
            name: name.into(),
            version: "1-1".into(),
            installed_size: size,
            reason: reason.into(),
        }
    }

    #[test]
    fn removals_are_listed_with_their_reason_and_their_space_is_subtracted() {
        let mut plan = empty_plan(1000);
        attach_removals(
            &mut plan,
            Ok(Simulation {
                removals: vec![
                    removal("jack2", 300, "conflicts with pipewire-jack"),
                    removal("old", 50, "replaced by new"),
                ],
                providers: Vec::new(),
            }),
        );
        assert_eq!(plan.total_installed_size_delta, 650);
        assert_eq!(plan.removals.len(), 2);
        assert_eq!(plan.removals[0].name, "jack2");
        assert_eq!(plan.removals[0].reason, "conflicts with pipewire-jack");
        assert_eq!(plan.removals[1].reason, "replaced by new");
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn provider_choices_are_carried_into_the_plan_in_pacmans_order() {
        let mut plan = empty_plan(0);
        attach_removals(
            &mut plan,
            Ok(Simulation {
                removals: Vec::new(),
                providers: vec![Providers {
                    dependency: "jack".into(),
                    providers: vec!["jack2".into(), "pipewire-jack".into()],
                }],
            }),
        );
        assert_eq!(plan.provider_choices.len(), 1);
        assert_eq!(plan.provider_choices[0].dependency, "jack");
        assert_eq!(
            plan.provider_choices[0].providers,
            ["jack2", "pipewire-jack"]
        );
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["provider_choices"][0]["providers"][1], "pipewire-jack");
    }

    #[test]
    fn nothing_found_leaves_the_plan_untouched() {
        let mut plan = empty_plan(42);
        attach_removals(&mut plan, Ok(Simulation::default()));
        assert_eq!(plan.total_installed_size_delta, 42);
        assert!(plan.removals.is_empty() && plan.warnings.is_empty());
        assert!(plan.provider_choices.is_empty());
    }

    #[test]
    fn a_failed_simulation_is_said_not_silently_dropped() {
        let mut plan = empty_plan(7);
        attach_removals(&mut plan, Err(Error::Simulation("boom".into())));
        assert_eq!(plan.total_installed_size_delta, 7, "nothing was subtracted");
        assert!(plan.removals.is_empty());
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains("boom") && plan.warnings[0].contains("it will ask"));
    }

    #[test]
    fn plans_without_extras_serialize_without_the_empty_fields() {
        let json = serde_json::to_value(empty_plan(0)).unwrap();
        for field in ["removals", "warnings", "provider_choices"] {
            assert!(json.get(field).is_none(), "{field}: {json}");
        }
    }
}

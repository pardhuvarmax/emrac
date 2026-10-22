use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::audit::{self, AuditReport};
use crate::aur_trust;
use crate::backend::{
    version_is_newer, AlpmBackend, ArchiveBackend, AurBackend, AurBuildBackend, AurSyncOutcome,
    BuildOptions, FreshDb, InstalledSource, PacmanBackend, ResolvedPackage, SimOp, Simulation,
};
use crate::build_tools::{self, BuildTool};
use crate::cache;
use crate::config::Config;
use crate::config_preview::{self, ConfigEffect, ConfigPreview};
use crate::deps::{
    self, DepsReport, GraphReport, ImpactItem, Orphan, RdepsReport, RemoveImpactReport, TreeReport,
    UnitFact, WhyReport,
};
use crate::doctor::{self, DiskFact, DoctorInputs, DoctorReport, ReportEnv, ToolFact};
use crate::downgrade::{self, DowngradePlan, VersionList};
use crate::error::{Error, Result};
use crate::history::{now, History, TxStatus};
use crate::hooks::{self, HookPreview};
use crate::install_mode::SourceMode;
use crate::manifest::{self, Manifest};
use crate::package::{PackageDetails, PackageSummary};
use crate::pacnew::{self, Class, Merge, Pending, Texts};
use crate::pins::{self, Pin, PinStore};
use crate::plan::{Plan, PlanAction, PlannedPackage, PlannedRemoval, ProviderChoice};
use crate::plan_preview::PlanPreview;
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
    pub(crate) pacman: PacmanBackend,
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
        self.plan_install_with(pkgs, offline, SourceMode::Auto)
            .map(|(plan, _)| plan)
    }

    /// Which of the official packages named are built from source: by the
    /// choice made (`--source`, `--binary`), else by `install.prefer`.
    fn official_by_source(
        &self,
        official: &[String],
        mode: SourceMode,
        offline: bool,
    ) -> Vec<String> {
        use crate::install_mode::{build_from_source, Prefer};
        let prefer = Config::load()
            .ok()
            .and_then(|c| Prefer::parse(&c.install.prefer))
            .unwrap_or_default();
        official
            .iter()
            .filter(|name| {
                let verdict = (mode == SourceMode::Auto && prefer == Prefer::Recommend)
                    .then(|| {
                        let facts = self.recommend_facts(name, offline).ok()?;
                        let report = crate::recommend::evaluate(&facts);
                        Some((report.verdict, report.confidence))
                    })
                    .flatten();
                build_from_source(mode, prefer, verdict)
            })
            .cloned()
            .collect()
    }

    /// `plan_install` with the choice of source or binary. Returns the plan and
    /// the names of the official packages that are built from source (their
    /// entries have `+source` after the repository). With `SourceMode::Binary` a
    /// package that exists only as source (the AUR's) is refused, and the
    /// message names its `-bin` counterpart when the AUR has one.
    pub fn plan_install_with(
        &self,
        pkgs: &[String],
        offline: bool,
        mode: SourceMode,
    ) -> Result<(Plan, Vec<String>)> {
        let (official, aur) = self.classify_install(pkgs, offline)?;
        if mode == SourceMode::Binary && !aur.is_empty() {
            let mut why = Vec::new();
            for d in &aur {
                let bin = format!("{}-bin", d.name);
                let has_bin = !offline
                    && !d.name.ends_with("-bin")
                    && self.aur.info(&bin).ok().flatten().is_some();
                why.push(if has_bin {
                    format!(
                        "{0} exists on the AUR only as source; its prebuilt counterpart is {bin}",
                        d.name
                    )
                } else {
                    format!("{0} exists on the AUR only as source", d.name)
                });
            }
            return Err(Error::Help(format!(
                "--binary means nothing is compiled, but {}. Name the prebuilt package, or drop --binary to build it",
                why.join("; ")
            )));
        }
        let by_source = self.official_by_source(&official, mode, offline);
        let binary: Vec<String> = official
            .iter()
            .filter(|n| !by_source.contains(n))
            .cloned()
            .collect();

        let mut packages = Vec::new();
        let mut total_download_size = 0u64;
        let mut total_installed_size_delta = 0i64;
        let mut warnings = Vec::new();

        if !binary.is_empty() {
            let names = self.pacman.resolve_install(&binary)?;
            let resolved = self.alpm.sync_resolved(&names);
            let mut official_plan = build_plan(PlanAction::Install, &binary, resolved, 1);
            // What is installed at the moment of the plan is part of the plan: a package
            // pacman puts in between (so that the install becomes a reinstall) changes it.
            for p in &mut official_plan.packages {
                p.previous_version = self.alpm.installed_version(&p.name);
            }
            total_download_size += official_plan.total_download_size;
            total_installed_size_delta += official_plan.total_installed_size_delta;
            packages.append(&mut official_plan.packages);
        }

        for name in &by_source {
            let src = self.arch_source(name)?;
            let installed = self.alpm.installed_version(name);
            total_installed_size_delta += self
                .alpm
                .sync_resolved(std::slice::from_ref(name))
                .first()
                .map_or(0, |r| r.installed_size as i64)
                - installed
                    .as_ref()
                    .and_then(|_| self.alpm.installed_size(name))
                    .unwrap_or(0) as i64;
            packages.push(PlannedPackage {
                name: name.clone(),
                version: src.version.clone(),
                repo: format!("{}{}", src.repo, crate::install_mode::SOURCE_SUFFIX),
                explicit: true,
                previous_version: installed,
            });
        }
        if !by_source.is_empty() {
            warnings.push(format!(
                "built from source here, from Arch's packaging source at the tag of the repository's version: {}. The build runs in the sandbox and `makepkg` installs what it needs to build. It has the repository's version, so it stays until the repository gets a newer one; the upgrade that would replace it with the binary says so first (`emrac pin` keeps it, but holds the package back from every upgrade, which is a partial upgrade for what depends on it)",
                by_source.join(", ")
            ));
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
            warnings,
            provider_choices: Vec::new(),
        };
        if !binary.is_empty() {
            attach_removals(
                &mut plan,
                AlpmBackend::simulate(None, SimOp::Install(&binary)),
            );
        }
        Ok((plan, by_source))
    }

    /// Notes that `name` was built here at `version` (`install --source`): its
    /// build policy is source from now on, until you end it.
    pub fn record_source_built(&self, name: &str, version: &str, key: &str) -> Result<()> {
        let recipe = self.aur_build.head(key);
        crate::source_built::record_to(
            &crate::source_built::path()?,
            name,
            version,
            recipe.as_deref(),
            crate::source_built::installed(name)
                .filter(|i| i.version == version)
                .as_ref(),
            now(),
        )
    }

    /// The packages with a source-build policy.
    pub fn source_policy(&self) -> Result<Vec<crate::source_built::Entry>> {
        crate::source_built::read()
    }

    /// Ends the source-build policy of these packages; which were ended.
    pub fn end_source_policy(&self, names: &[String]) -> Result<bool> {
        crate::source_built::forget_from(&crate::source_built::path()?, names)
    }

    /// Declares the policy for installed official packages that were not built
    /// here: the build waits (pending) for a rebuild or the next upgrade.
    pub fn declare_source_policy(&self, names: &[String]) -> Result<()> {
        let path = crate::source_built::path()?;
        for name in names {
            if crate::source_built::read_from(&path)?
                .iter()
                .any(|e| &e.name == name)
            {
                continue;
            }
            crate::source_built::record_to(&path, name, "", None, None, now())?;
        }
        crate::source_built::mark_pending(&path, names)
    }

    /// A rebuild is waiting for these packages.
    pub fn mark_source_pending(&self, names: &[String]) {
        if let Ok(path) = crate::source_built::path() {
            let _ = crate::source_built::mark_pending(&path, names);
        }
    }

    /// Marks, in the plan of an upgrade, the packages whose policy is to build
    /// from source: their entries get `+source` and the plan says what will happen.
    /// The upgrade itself installs the repository's binaries first, so that the
    /// system is never partly upgraded; the rebuild follows it.
    fn apply_source_policy(&self, plan: &mut Plan) -> Result<()> {
        let policy = crate::source_built::read()?;
        if policy.is_empty() {
            return Ok(());
        }
        let mut followed = Vec::new();
        for p in plan
            .packages
            .iter_mut()
            .filter(|p| p.repo != "aur" && !crate::install_mode::is_source_repo(&p.repo))
        {
            if policy.iter().any(|e| e.name == p.name) {
                p.repo = format!("{}{}", p.repo, crate::install_mode::SOURCE_SUFFIX);
                followed.push(p.name.clone());
            }
        }
        if !followed.is_empty() {
            plan.warnings.push(format!(
                "source-build policy: {} will be rebuilt from Arch's packaging source after the upgrade. The upgrade installs the repository's binaries first, so the system is never partly upgraded; if a rebuild fails, the binary stays installed, the policy is kept and emrac says so (`emrac from-source drop <package>` ends it)",
                followed.join(", ")
            ));
        }
        // A rebuild still waiting from an earlier upgrade is said in every plan
        // until it is done or dropped.
        let waiting: Vec<&str> = policy
            .iter()
            .filter(|e| {
                !followed.contains(&e.name)
                    && crate::source_built::state_of(
                        e,
                        crate::source_built::installed(&e.name).as_ref(),
                    )
                    .needs_attention()
            })
            .map(|e| e.name.as_str())
            .collect();
        if !waiting.is_empty() {
            plan.warnings.push(format!(
                "the source-build policy is not being honoured for {} (a rebuild is pending, or the repository's file is installed). Build it with `emrac from-source rebuild`, or end the policy with `emrac from-source drop`",
                waiting.join(", ")
            ));
        }
        Ok(())
    }

    /// How likely each installed policy package is to no longer work with the
    /// system around it (a library it needs gone, something it was built against
    /// changed since). Read from the files and pacman's log; an estimate.
    pub fn source_policy_risks(&self) -> Vec<(String, crate::source_built::Risk)> {
        let Ok(policy) = crate::source_built::read() else {
            return Vec::new();
        };
        let system = crate::stale_builds::system_libs();
        let interpreters: Vec<(&str, String)> = ["python", "perl"]
            .into_iter()
            .filter_map(|n| {
                let (version, _, _) = self.alpm.package_info(n)?;
                Some((n, crate::stale_builds::minor_of(&version)?))
            })
            .collect();
        let log = std::fs::read_to_string("/var/log/pacman.log");
        let upgrades = log
            .as_ref()
            .map(|t| crate::stale_builds::parse_upgrades(t))
            .unwrap_or_default();
        policy
            .into_iter()
            .filter_map(|e| {
                let detail = self.alpm.installed_detail(&e.name)?;
                let evidence = log.is_ok() && !detail.files.is_empty();
                let stale = crate::stale_builds::judge(
                    &detail,
                    &system,
                    &interpreters,
                    &upgrades,
                    Path::new("/"),
                );
                Some((e.name, crate::source_built::classify(&stale, evidence)))
            })
            .collect()
    }

    /// Source-build policies whose rebuild is pending (for `doctor`), or why the
    /// record cannot be read.
    pub fn source_policy_waiting(&self) -> std::result::Result<Vec<String>, String> {
        crate::source_built::read()
            .map(|l| {
                l.into_iter()
                    .filter(|e| {
                        crate::source_built::state_of(
                            e,
                            crate::source_built::installed(&e.name).as_ref(),
                        )
                        .needs_attention()
                    })
                    .map(|e| e.name)
                    .collect()
            })
            .map_err(|e| e.to_string())
    }

    /// The plan recorded for rebuilding a package under the policy: `repo` is
    /// the plan entry's, already marked `+source`.
    pub fn source_rebuild_plan(&self, name: &str, version: &str, repo: &str) -> Plan {
        Plan {
            action: PlanAction::Install,
            packages: vec![PlannedPackage {
                name: name.to_string(),
                version: version.to_string(),
                repo: repo.to_string(),
                explicit: true,
                previous_version: Some(version.to_string()),
            }],
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: Vec::new(),
            provider_choices: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Where an official package's source is at a version (the repository's, or
    /// one that an upgrade has just installed).
    pub fn arch_source_at(&self, name: &str, version: &str) -> Result<ArchSource> {
        let mut src = self.arch_source(name)?;
        src.version = version.to_string();
        Ok(src)
    }

    /// Where an official package's source is: its package base and the
    /// repository's version.
    pub fn arch_source(&self, name: &str) -> Result<ArchSource> {
        let meta = self
            .alpm
            .repo_meta(name)
            .ok_or_else(|| Error::PackageNotFound(name.to_string()))?;
        let version = self
            .alpm
            .sync_version(name)
            .ok_or_else(|| Error::PackageNotFound(name.to_string()))?;
        let repo = self
            .alpm
            .sync_resolved(std::slice::from_ref(&name.to_string()))
            .first()
            .map(|r| r.repo.clone())
            .unwrap_or_default();
        Ok(ArchSource {
            name: name.to_string(),
            base: meta.base,
            repo,
            version,
        })
    }

    /// Fetches the packaging source of `src` at its tag; the key of the checkout.
    pub fn arch_source_sync(&self, src: &ArchSource) -> Result<String> {
        self.aur_build.sync_arch(&src.base, &src.version)
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
        // (an official package built here has no cached file to read; its build is inspected)
        for pkg in plan
            .packages
            .iter()
            .filter(|p| p.repo != "aur" && !crate::install_mode::is_source_repo(&p.repo))
        {
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

    /// The trust signals for an AUR package about to be built (u20): what the
    /// AUR says, how the name compares with real packages, and what changed
    /// since you last built it. The record to keep once it is built is held
    /// until `aur_trust_commit`. A failed AUR lookup gives the change signals
    /// only.
    pub fn aur_trust(&self, name: &str, outcome: &AurSyncOutcome) -> Vec<aur_trust::Signal> {
        let (old, new) = match outcome {
            AurSyncOutcome::Changed { .. } => (
                self.aur_build.read_file(name, None, ".SRCINFO"),
                self.aur_build
                    .read_file(name, Some("FETCH_HEAD"), ".SRCINFO"),
            ),
            _ => (None, self.aur_build.read_file(name, None, ".SRCINFO")),
        };
        let old = old.map(|t| aur_trust::parse_srcinfo(&t));
        let new = new.map(|t| aur_trust::parse_srcinfo(&t));
        let store = aur_trust::load();
        let recorded = store.get(name);
        let mut signals = Vec::new();
        let mut facts = aur_trust::AurFacts {
            name: name.to_string(),
            ..aur_trust::AurFacts::default()
        };
        if let Ok(Some(d)) = self.aur.info(name) {
            facts = aur_trust::AurFacts {
                name: name.to_string(),
                maintainer: d.maintainer,
                first_submitted: d.first_submitted,
                votes: d.votes.unwrap_or(0),
                out_of_date: d.out_of_date.is_some(),
                provides: d.provides,
                conflicts: d.conflicts,
                replaces: d.replaces,
            };
            let official_names = self.alpm.sync_names();
            let official: std::collections::BTreeSet<&str> =
                official_names.iter().map(String::as_str).collect();
            let installed = self.alpm.installed_names();
            signals.extend(aur_trust::signals_from_aur(
                &facts,
                &official,
                &installed,
                recorded,
                now() as i64,
            ));
        }
        if let Some(new) = &new {
            signals.extend(aur_trust::signals_from_change(old.as_ref(), new, recorded));
            aur_trust::hold(name, aur_trust::record_of(&facts, new));
        }
        signals.sort_by_key(|s| std::cmp::Reverse(s.level));
        signals
    }

    /// The same signals without a checkout, for `emrac info` on an AUR package.
    pub fn aur_trust_for_info(&self, details: &PackageDetails) -> Vec<aur_trust::Signal> {
        let facts = aur_trust::AurFacts {
            name: details.name.clone(),
            maintainer: details.maintainer.clone(),
            first_submitted: details.first_submitted,
            votes: details.votes.unwrap_or(0),
            out_of_date: details.out_of_date.is_some(),
            provides: details.provides.clone(),
            conflicts: details.conflicts.clone(),
            replaces: details.replaces.clone(),
        };
        let official_names = self.alpm.sync_names();
        let official: std::collections::BTreeSet<&str> =
            official_names.iter().map(String::as_str).collect();
        let installed = self.alpm.installed_names();
        let store = aur_trust::load();
        let mut signals = aur_trust::signals_from_aur(
            &facts,
            &official,
            &installed,
            store.get(&details.name),
            now() as i64,
        );
        signals.sort_by_key(|s| std::cmp::Reverse(s.level));
        signals
    }

    /// The package was built and installed: keep what was learned about it.
    pub fn aur_trust_commit(&self, name: &str) {
        let _ = aur_trust::commit(name);
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
        let started = std::time::Instant::now();
        let built = self.aur_build.build(name, opts);
        self.note_build(name, started.elapsed().as_secs(), built.is_ok());
        match built {
            Ok(()) => {}
            // The dependency step is pacman's, which says only that it
            // failed; if a conflict is why, say what it is.
            Err(err @ Error::BuildFailed { .. }) if err.stopped_on_dependencies() => {
                let deps = self
                    .aur_build
                    .read_file(name, None, ".SRCINFO")
                    .map(|text| srcinfo_dependencies(&text))
                    .unwrap_or_default();
                return Err(dependency_conflict_for(&self.alpm, None, &deps)
                    .map(|c| Error::DependencyConflict(Box::new(c)))
                    .unwrap_or(err));
            }
            Err(err) => return Err(err),
        }
        self.aur_build.built_files(name)
    }

    /// Notes what the packaging files of `key` are now, as what was reviewed: the
    /// build checks that nothing changed after this.
    pub fn note_reviewed(&self, key: &str) -> Result<()> {
        let dir = self.aur_build.build_dir(key)?;
        crate::recipe_digest::note(key, crate::recipe_digest::read(&dir)?);
        Ok(())
    }

    /// Before a build: the packaging files are what was reviewed. With nothing
    /// noted yet (a build that was not reviewed first) the files as they are
    /// become the baseline, which the host step is then held to.
    pub fn check_reviewed(&self, key: &str) -> Result<()> {
        let dir = self.aur_build.build_dir(key)?;
        let now = crate::recipe_digest::read(&dir)?;
        match crate::recipe_digest::noted(key) {
            None => {
                crate::recipe_digest::note(key, now);
                Ok(())
            }
            Some(reviewed) => {
                let changes = crate::recipe_digest::changes(&reviewed, &now, true);
                if changes.is_empty() {
                    Ok(())
                } else {
                    Err(Error::RecipeChanged {
                        package: key.to_string(),
                        files: changes.join(", "),
                    })
                }
            }
        }
    }

    /// How long building `name` usually takes here, if that is known.
    pub fn build_expected_secs(&self, name: &str) -> Option<u64> {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
        crate::eta::build_part(name, &crate::recommend::read_builds(), cores).map(|p| p.secs)
    }

    /// How long this plan will take: the builds from what they took before, the
    /// download and install from what earlier transactions achieved.
    pub fn estimate(&self, plan: &Plan) -> crate::eta::Estimate {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
        let builds = crate::recommend::read_builds();
        let mut est = crate::eta::Estimate::default();
        let mut official = 0usize;
        for p in &plan.packages {
            if p.repo == "aur" || crate::install_mode::is_source_repo(&p.repo) {
                match crate::eta::build_part(&p.name, &builds, cores) {
                    Some(part) => est.parts.push(part),
                    None => est
                        .unknown
                        .push(format!("build {} (a first build here)", p.name)),
                }
            } else {
                official += 1;
            }
        }
        if official > 0 {
            let mirror = {
                let stats = crate::mirrors::summarize(&crate::mirrors::read_history());
                let mut kib: Vec<u64> = stats.values().filter_map(|s| s.kib_per_s).collect();
                kib.sort_unstable();
                kib.get(kib.len() / 2).copied()
            };
            match crate::eta::transfer_part(
                plan.total_download_size,
                official,
                &crate::eta::read_ops(),
                mirror,
            ) {
                Some(part) => est.parts.push(part),
                None => est.unknown.push(
                    "the download (no earlier transaction or mirror measurement to go on)".into(),
                ),
            }
        }
        est
    }

    /// The programs and libraries nobody packaged: `/usr/local`, `~/.local`, and
    /// what no package owns under `/opt`.
    pub fn unpackaged_binaries(&self) -> Vec<crate::linkcheck::Binary> {
        let base = Path::new("/");
        let mut roots: Vec<PathBuf> = ["bin", "sbin", "lib", "lib64", "libexec"]
            .iter()
            .map(|d| PathBuf::from("/usr/local").join(d))
            .collect();
        if let Ok(home) = std::env::var("HOME") {
            for d in ["bin", "lib", "lib64"] {
                roots.push(PathBuf::from(&home).join(".local").join(d));
            }
        }
        roots.push(PathBuf::from("/opt"));
        let owned = crate::strays::read_owned_where(Path::new("/var/lib/pacman/local"), &|p| {
            p.starts_with("opt/")
        });
        crate::linkcheck::candidates(base, &roots, &|rel| owned.paths.contains(rel))
    }

    /// Programs outside the package manager that this plan would leave without a
    /// library: one line each, for the plan's warnings. `sync` is the view of the
    /// repositories the plan was made from.
    pub(crate) fn linkcheck_warnings(&self, plan: &Plan, sync: &AlpmBackend) -> Vec<String> {
        use std::collections::HashSet;
        if !matches!(
            plan.action,
            PlanAction::Upgrade | PlanAction::Downgrade | PlanAction::Remove | PlanAction::Purge
        ) {
            return Vec::new();
        }
        let removing = matches!(plan.action, PlanAction::Remove | PlanAction::Purge);
        let mut changes = Vec::new();
        for p in plan.packages.iter().filter(|p| p.repo != "aur") {
            let old = self.alpm.installed_provides(&p.name);
            let from = self.alpm.installed_version(&p.name).unwrap_or_default();
            let (new, to) = if removing {
                (Vec::new(), None)
            } else {
                match sync.sync_provides(&p.name) {
                    Some(n) => (n, Some(p.version.as_str())),
                    None => continue,
                }
            };
            changes.extend(crate::linkcheck::lost(&p.name, &from, to, &old, &new));
        }
        for r in &plan.removals {
            let old = self.alpm.installed_provides(&r.name);
            changes.extend(crate::linkcheck::lost(&r.name, &r.version, None, &old, &[]));
        }
        if changes.is_empty() {
            return Vec::new();
        }
        let binaries = self.unpackaged_binaries();
        let leaving: HashSet<String> = plan
            .packages
            .iter()
            .map(|p| p.name.clone())
            .chain(plan.removals.iter().map(|r| r.name.clone()))
            .collect();
        let hits = crate::linkcheck::hits(&binaries, &changes, &|provide| {
            self.alpm.provided_by_other(provide, &leaving)
        });
        crate::linkcheck::warning_lines(&hits, 5)
    }

    /// What building `name` will make `makepkg -s` install on the host first, from
    /// its `.SRCINFO` (read, never run): new packages, and installed packages that
    /// must be upgraded to take them (a partial upgrade). `None` when there is no
    /// `.SRCINFO` to read.
    pub fn build_dependencies(&self, name: &str) -> Result<Option<BuildDeps>> {
        let Some(text) = self.aur_build.read_file(name, None, ".SRCINFO") else {
            return Ok(None);
        };
        let missing = self
            .alpm
            .uninstalled_repo_dependencies(&srcinfo_dependencies(&text));
        if missing.is_empty() {
            return Ok(Some(BuildDeps::default()));
        }
        let mut deps = BuildDeps::default();
        // pacman takes names, not `name>=1.0`; it picks the version that satisfies
        let names: Vec<String> = missing
            .iter()
            .map(|d| {
                d.split(['<', '>', '='])
                    .next()
                    .unwrap_or(d)
                    .trim()
                    .to_string()
            })
            .collect();
        for n in self.pacman.resolve_install(&names)? {
            if self.alpm.installed_version(&n).is_some() {
                deps.upgrade.push(n);
            } else {
                deps.install.push(n);
            }
        }
        Ok(Some(deps))
    }

    /// Swaps the conflicting packages named by `conflict` (see
    /// `DependencyConflict::command`). The caller has asked the user first.
    pub fn apply_dependency_swap(
        &self,
        conflict: &crate::backend::DependencyConflict,
    ) -> Result<()> {
        self.pacman.execute_swap(&conflict.removed, &conflict.by)
    }

    /// Builds the PKGBUILD in a local directory the same way (no install).
    pub fn build_dir_package(
        &self,
        dir: &std::path::Path,
        opts: BuildOptions,
    ) -> Result<Vec<PathBuf>> {
        let started = std::time::Instant::now();
        let built = self.aur_build.build_in(dir, opts);
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.note_build(&name, started.elapsed().as_secs(), built.is_ok());
        built?;
        self.aur_build.built_files_in(dir)
    }

    /// Notes how long a build took, for `recommend` (never fails the build).
    fn note_build(&self, name: &str, secs: u64, ok: bool) {
        crate::recommend::record_build(name, "", secs, ok);
    }

    /// What `recommend` weighs for `name`: the package's shape, the
    /// toolchain it needs, what emrac measured and recorded here, and this
    /// machine. Reads only; the AUR is asked unless `offline`.
    pub fn recommend_facts(&self, name: &str, offline: bool) -> Result<crate::recommend::Facts> {
        use crate::recommend::{self as rec, Class, Facts, Origin};
        const MIB: f64 = 1_048_576.0;
        let details = self.info(name, offline)?;
        let (machine, flags) = rec::gather_machine();
        let history = History::all().unwrap_or_default();
        let aur = details.repo == "aur";
        let mut f = Facts {
            name: details.name.clone(),
            origin: Origin {
                repo: details.repo.clone(),
            },
            binary: None,
            arch_any: false,
            installed_mib: None,
            download_mib: None,
            deps_direct: details.depends.len(),
            deps_total: None,
            class: Class::Unknown,
            class_from: String::new(),
            measured: rec::measured_builds(&details.name, machine.cores),
            updates_90d: rec::updates_in_window(&history, &details.name, now()),
            flags,
            machine,
        };
        if aur {
            let (class, why) = if details.name.ends_with("-bin") {
                (
                    Class::NoCompile,
                    "its name ends in -bin: it repackages upstream's binary".to_string(),
                )
            } else {
                rec::class_from_makedepends(&details.make_depends)
            };
            f.class = class;
            f.class_from = why;
            if !details.name.ends_with("-bin") && !offline {
                let sibling = format!("{}-bin", details.name);
                if let Ok(Some(_)) = self.aur.info(&sibling) {
                    f.binary = Some(sibling);
                }
            }
        } else if let Some(shape) = self.alpm.repo_shape(&details.name) {
            f.binary = Some(details.name.clone());
            f.arch_any = shape.arch_any;
            f.installed_mib = Some(shape.installed as f64 / MIB);
            f.download_mib = Some(shape.download as f64 / MIB);
            f.deps_direct = shape.depends.len();
            let (class, why) = rec::class_from_depends(shape.arch_any, &shape.depends);
            f.class = class;
            f.class_from = why;
            f.deps_total =
                Some(deps::closure(&details.name, &|n| self.alpm.sync_dependency_names(n)).len());
        }
        Ok(f)
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

    /// The plan to install local package files (`*.pkg.tar.*`), what `pacman -U`
    /// would do: an install, an upgrade, a reinstall or a downgrade by the version
    /// that is installed now, the packages libalpm would remove on the side, and
    /// the size change. Nothing is extracted or run. `args` are paths.
    pub fn plan_install_files(&self, args: &[String]) -> Result<LocalInstall> {
        if args.is_empty() {
            return Err(Error::Help("name a package file".into()));
        }
        let mut files: Vec<PathBuf> = Vec::new();
        for a in args {
            let path = std::fs::canonicalize(a).map_err(|e| Error::Help(format!("{a}: {e}")))?;
            if !path.is_file() || !is_package_file_name(&path.to_string_lossy()) {
                return Err(Error::Help(format!(
                    "{a} is not a package file (*.pkg.tar.zst, *.pkg.tar.xz, ...)"
                )));
            }
            if !files.contains(&path) {
                files.push(path);
            }
        }
        let mut packages: Vec<PlannedPackage> = Vec::new();
        let mut delta: i64 = 0;
        let mut warnings: Vec<String> = Vec::new();
        let mut actions: Vec<PlanAction> = Vec::new();
        for path in &files {
            let m = inspect_package(path)?;
            let installed = self.alpm.installed_version(&m.package);
            let action = match &installed {
                None => PlanAction::Install,
                Some(old) if version_is_newer(old, &m.version) => PlanAction::Upgrade,
                Some(old) if old == &m.version => PlanAction::Reinstall,
                Some(_) => PlanAction::Downgrade,
            };
            actions.push(action);
            delta += m.installed_size as i64
                - installed
                    .as_ref()
                    .and_then(|_| self.alpm.installed_size(&m.package))
                    .unwrap_or(0) as i64;
            let mut sig = path.clone().into_os_string();
            sig.push(".sig");
            if !std::path::Path::new(&sig).exists() {
                warnings.push(format!(
                    "{} has no signature next to it (a .sig file): pacman accepts it only because LocalFileSigLevel allows an unsigned local file; it is as trustworthy as where you got it",
                    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                ));
            }
            packages.push(PlannedPackage {
                name: m.package.clone(),
                version: m.version.clone(),
                repo: "local file".to_string(),
                explicit: true,
                previous_version: installed,
            });
        }
        let action = if actions.iter().all(|a| *a == actions[0]) {
            actions[0]
        } else {
            PlanAction::Install
        };
        let mut removals: Vec<PlannedRemoval> = Vec::new();
        match AlpmBackend::simulate(None, SimOp::Files(&files)) {
            Ok(sim) => {
                for r in sim.removals {
                    delta -= r.installed_size as i64;
                    removals.push(PlannedRemoval {
                        name: r.name,
                        version: r.version,
                        reason: r.reason,
                    });
                }
            }
            Err(err) => warnings.push(format!(
                "couldn't work out what installing this would remove or pull in: {err}"
            )),
        }
        Ok(LocalInstall {
            plan: Plan {
                action,
                packages,
                total_download_size: 0,
                total_installed_size_delta: delta,
                removals,
                provider_choices: Vec::new(),
                warnings,
            },
            files,
        })
    }

    /// Installs built package files with `sudo pacman -U`.
    pub fn install_built(&self, files: &[PathBuf]) -> Result<()> {
        self.pacman.execute_install_files(files)
    }

    /// What each cache entry would suffer under `op` in `scopes`: nothing is
    /// removed here. `older_than` is in seconds.
    pub fn cache_plan(
        &self,
        scopes: &[cache::Scope],
        op: cache::Op,
        older_than: Option<u64>,
        lean: Option<u32>,
    ) -> Result<Vec<cache::Decision>> {
        let home = std::env::var("HOME").map_err(|_| Error::NoHomeDir)?;
        let dirs = self.pacman.cache_dirs().unwrap_or_default();
        let entries = cache::scan(Path::new(&home), &dirs);
        let mut facts = cache::Facts {
            installed: self
                .alpm
                .installed_snapshot()
                .into_iter()
                .map(|p| (p.name, p.version))
                .collect(),
            older_than,
            now: now(),
            source_age: cache::DEFAULT_SOURCE_AGE,
            ..cache::Facts::default()
        };
        if let Some(days) = lean {
            facts.lean = true;
            let snapshot = self.alpm.installed_snapshot();
            facts.foreign = snapshot
                .iter()
                .filter(|p| p.foreign)
                .map(|p| p.name.clone())
                .collect();
            let official: Vec<String> = snapshot
                .iter()
                .filter(|p| !p.foreign)
                .map(|p| p.name.clone())
                .collect();
            facts.in_repo = self
                .alpm
                .sync_resolved(&official)
                .into_iter()
                .map(|r| (r.name, r.version))
                .collect();
            // What undoing anything from the last `days` days needs, whatever
            // each record's status says: a failed upgrade can have changed
            // packages before an AUR build failed. The newest change counts
            // however old it is.
            let cutoff = now().saturating_sub(u64::from(days) * 86_400);
            let mut newest_done = false;
            for record in History::list(None, None).unwrap_or_default() {
                let needs = undo_needs(&record, &facts.installed);
                if needs.is_empty() {
                    continue;
                }
                if record.timestamp >= cutoff || !newest_done {
                    facts.undo_needs.extend(needs);
                }
                newest_done = true;
            }
        }
        // Versions the last transactions name are what `undo` and `redo` need.
        for record in History::list(Some(crate::history::RECENT_KEPT), None).unwrap_or_default() {
            for p in &record.packages {
                facts.recorded.insert((p.name.clone(), p.version.clone()));
                if let Some(v) = &p.previous_version {
                    facts.recorded.insert((p.name.clone(), v.clone()));
                }
            }
            for r in &record.removed {
                facts.recorded.insert((r.name.clone(), r.version.clone()));
            }
        }
        Ok(cache::plan(&entries, scopes, op, &facts, &|a, b| {
            version_is_newer(b, a)
        }))
    }

    /// Removes the entries `decisions` mark. Everything must lie under the
    /// emrac cache or pacman's; files pacman's cache holds are root-owned and
    /// go through `sudo`. Returns what was freed and what could not be removed.
    pub fn cache_apply(&self, decisions: &[cache::Decision]) -> Result<cache::Outcome> {
        let home = PathBuf::from(std::env::var("HOME").map_err(|_| Error::NoHomeDir)?);
        let mut roots = vec![home.join(".cache/emrac")];
        roots.extend(self.pacman.cache_dirs().unwrap_or_default());
        let doomed: Vec<&cache::Decision> = decisions.iter().filter(|d| d.remove).collect();
        if doomed
            .iter()
            .any(|d| d.entry.scope == cache::Scope::Package)
            && Path::new("/var/lib/pacman/db.lck").exists()
        {
            return Err(Error::Simulation(
                "pacman is running (its database lock exists); not touching its package cache"
                    .to_string(),
            ));
        }
        let mut outcome = cache::Outcome::default();
        let mut privileged: Vec<String> = Vec::new();
        let mut privileged_bytes = 0;
        for d in doomed {
            let path = &d.entry.path;
            if !cache::is_under(path, &roots) {
                outcome
                    .failed
                    .push((path.clone(), "outside the cache folders; left alone".into()));
                continue;
            }
            // A package file's detached signature goes with it.
            let sig = PathBuf::from(format!("{}.sig", path.display()));
            let mut paths = vec![path.clone()];
            if d.entry.version.is_some() && sig.exists() {
                paths.push(sig);
            }
            if cache::owned_like(path, &home) {
                match cache::remove_tree(path) {
                    Ok(()) => {
                        outcome.removed += 1;
                        outcome.freed += d.entry.bytes;
                        for extra in &paths[1..] {
                            let _ = std::fs::remove_file(extra);
                        }
                    }
                    Err(err) => outcome.failed.push((path.clone(), err.to_string())),
                }
            } else {
                privileged.extend(paths.iter().map(|p| p.display().to_string()));
                privileged_bytes += d.entry.bytes;
                outcome.removed += 1;
            }
        }
        // Root-owned files, a batch per `sudo` call.
        for batch in privileged.chunks(200) {
            let mut args = vec!["-f".to_string(), "--".to_string()];
            args.extend(batch.iter().cloned());
            self.pacman.run_sudo("rm", &args)?;
        }
        outcome.freed += privileged_bytes;
        Ok(outcome)
    }

    /// Every pending `.pacnew` / `.pacsave`, judged by comparing the live
    /// file, the new one and the version the old package shipped. With
    /// `fetch`, an old package missing from the local caches may be downloaded
    /// from the Archive to get it. Nothing is changed.
    pub fn pacnew_scan(&self, fetch: bool) -> Result<Vec<Pending>> {
        let log = std::fs::read_to_string("/var/log/pacman.log").unwrap_or_default();
        let events = pacnew::parse_log(&log);
        let cache_dirs = self.pacman.cache_dirs().unwrap_or_default();
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        for (package, live, suffix, md5) in self.alpm.pending_config() {
            let pending = format!("{live}.{suffix}");
            seen.insert(pending.clone());
            out.push(self.judge_pending(
                Some(package),
                &live,
                &pending,
                suffix == "pacsave",
                &md5,
                &events,
                &cache_dirs,
                fetch,
            ));
        }
        for pending in find_pacnew(Path::new("/etc")) {
            if seen.contains(&pending) {
                continue;
            }
            let Some(live) = pacnew::live_path(&pending) else {
                continue;
            };
            out.push(self.judge_pending(
                None,
                live,
                &pending,
                pending.ends_with(".pacsave"),
                "",
                &events,
                &cache_dirs,
                fetch,
            ));
        }
        out.sort_by(|a, b| a.live.cmp(&b.live));
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn judge_pending(
        &self,
        package: Option<String>,
        live: &str,
        pending: &str,
        is_pacsave: bool,
        recorded_md5: &str,
        events: &[pacnew::LogEvent],
        cache_dirs: &[PathBuf],
        fetch: bool,
    ) -> Pending {
        let age_days = std::fs::metadata(pending)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
            .map(|d| d.as_secs() / 86_400);
        let mut file = Pending {
            live: live.to_string(),
            pending: pending.to_string(),
            package: package.clone(),
            class: Class::Unknown,
            reason: String::new(),
            base_version: None,
            age_days,
            critical: pacnew::is_critical(live),
            merge: None,
            texts: Texts::default(),
            merged: None,
        };
        if is_pacsave {
            file.class = Class::Pacsave;
            file.reason =
                "left when the package was removed; there is no newer version to compare".into();
            return file;
        }
        let read = |path: &str| {
            std::fs::read(path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
        };
        let (Some(theirs), Some(ours)) = (read(pending), read(live)) else {
            file.reason =
                "can't be read as text (no permission, binary, or the live file is missing)".into();
            return file;
        };
        file.texts.ours = Some(ours.clone());
        file.texts.theirs = Some(theirs.clone());
        if ours == theirs {
            (file.class, file.reason) = (
                Class::Identical,
                "the live file and the new one are the same".into(),
            );
            return file;
        }
        // A .pacnew made by an older upgrade than the installed package is
        // not what the installed package ships.
        if !recorded_md5.is_empty() && md5_of(pending).is_some_and(|m| m != recorded_md5) {
            file.reason =
                "this .pacnew is older than the installed package's file, so it says nothing about what changed"
                    .into();
            return file;
        }
        let member = live.trim_start_matches('/');
        // Walk the earlier versions, newest first. If the live file is exactly
        // what one of them shipped, it was never edited (just older than the
        // `.pacnew`); otherwise the first one that differs from the new file
        // is the base the two sides are compared against.
        let mut base: Option<(String, String)> = None;
        let mut unedited: Option<(String, String)> = None;
        if let Some(pkg) = package.as_deref() {
            for version in pacnew::earlier_versions(events, pkg, 6) {
                let Some(text) = self.old_file_text(pkg, &version, member, cache_dirs, fetch)
                else {
                    continue;
                };
                if text == ours {
                    unedited = Some((version, text));
                    break;
                }
                if base.is_none() && text != theirs {
                    base = Some((version, text));
                }
            }
        }
        let base = unedited.clone().or(base);
        let (class, reason) =
            pacnew::classify(base.as_ref().map(|(_, t)| t.as_str()), &ours, &theirs);
        let reason = match &unedited {
            Some((version, _)) => format!(
                "the live file is exactly what {version} shipped, so you never edited it; it is just older than the new one"
            ),
            None => reason.to_string(),
        };
        file.class = class;
        file.reason = reason;
        if let Some((version, text)) = base {
            file.base_version = Some(version);
            file.texts.base = Some(text);
        } else if package.is_some() {
            file.reason.push_str(if fetch {
                " (the earlier package versions weren't in the caches or the Archive)"
            } else {
                " (the earlier package versions aren't in the local caches; --fetch also looks in the Archive)"
            });
        }
        if class == Class::BothChanged {
            if let Some(base) = &file.texts.base {
                match pacnew::merge3(base, &ours, &theirs) {
                    Ok(merge) => {
                        file.merge = Some(
                            match &merge {
                                Merge::Clean(_) => "clean",
                                Merge::Conflicts(_) => "conflicts",
                            }
                            .to_string(),
                        );
                        file.merged = Some(merge);
                    }
                    Err(err) => file.reason.push_str(&format!(" (couldn't merge: {err})")),
                }
            }
        }
        file
    }

    /// One file as an older version of `pkg` shipped it: from a package file
    /// on this machine, else (with `fetch`) from the Archive.
    fn old_file_text(
        &self,
        pkg: &str,
        version: &str,
        member: &str,
        cache_dirs: &[PathBuf],
        fetch: bool,
    ) -> Option<String> {
        let path = match self.locate_file(cache_dirs, pkg, version) {
            Some(path) => path,
            None if fetch => {
                let file = self.archive.find(pkg, version).ok()??;
                let dest = self.archive.cache_dir(pkg).ok()?.join(&file.filename);
                if !dest.exists() {
                    self.archive.download(&file.url, &dest).ok()?;
                }
                dest
            }
            None => return None,
        };
        crate::backend::PackageFile::read(&path, member)
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
        let mut up = if pkgs.is_empty() {
            self.plan_full_upgrade(offline)?
        } else {
            self.plan_targeted_upgrade(pkgs)?
        };
        self.apply_source_policy(&mut up.plan)?;
        Ok(up)
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
        // AUR packages to rebuild, with their dependencies as the AUR lists them.
        let mut aur_builds: Vec<(String, Vec<String>)> = Vec::new();

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
                                aur_builds.push((details.name.clone(), details.depends.clone()));
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
        // An AUR rebuild whose dependencies would remove something still
        // required stops mid-build; say so now.
        let sync_view = view.alpm.as_ref().unwrap_or(&self.alpm);
        for (name, deps) in &aur_builds {
            if let Some(conflict) = dependency_conflict_for(sync_view, dbpath, deps) {
                plan.warnings.push(conflict.plan_warning(name));
            }
        }
        // programs nobody packaged that this upgrade would leave without a library
        let linked = self.linkcheck_warnings(&plan, sync_view);
        plan.warnings.extend(linked);
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
        let linked = self.linkcheck_warnings(&plan, &self.alpm);
        plan.warnings.extend(linked);
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
        let mut plan = build_plan(PlanAction::Remove, pkgs, resolved, -1);
        let linked = self.linkcheck_warnings(&plan, &self.alpm);
        plan.warnings.extend(linked);
        Ok(plan)
    }

    /// The names of everything installed right now, read fresh (a handle opened
    /// earlier does not see what pacman changed since). Empty if pacman's
    /// configuration cannot be read; callers must treat that as "unknown".
    pub fn installed_names_set(&self) -> HashSet<String> {
        AlpmBackend::init()
            .map(|alpm| alpm.installed_names().into_iter().collect())
            .unwrap_or_default()
    }

    /// The events of `emrac timeline`, newest first, from the records emrac keeps.
    /// A record that cannot be read is skipped.
    pub fn timeline(&self, filter: &crate::timeline::Filter) -> Vec<crate::timeline::Event> {
        let history = History::all_with_archive().unwrap_or_default();
        let snapshots = crate::snapshot::SnapshotStore::list().unwrap_or_default();
        let pacnew = crate::pacnew_log::log_path()
            .and_then(|p| crate::pacnew_log::read(&p))
            .unwrap_or_default();
        crate::timeline::collect(
            &history,
            &crate::recommend::read_builds(),
            &snapshots,
            &pacnew,
            filter,
        )
    }

    /// The inputs of `emrac score`: what the repositories or the AUR say, what
    /// emrac has measured here (benchmarks, builds), the audit of an AUR
    /// package's PKGBUILD (online only) and the cached Arch news.
    pub fn score_facts(&self, name: &str, offline: bool) -> Result<crate::score::Facts> {
        use crate::score::{AuditCounts, Bench, BuildFact, Facts, RepoKind, SourceFact};
        let details = self.info(name, offline)?;
        let official = details.repo != "aur";
        let t = now();
        let days_ago = |ts: i64| (t as i64 - ts).max(0) as u64 / 86_400;
        let mut f = Facts {
            name: details.name.clone(),
            version: details.version.clone(),
            repo: details.repo.clone(),
            official,
            ..Facts::default()
        };

        // what emrac measured here
        let records = crate::benchmark::history(&details.name);
        if !records.is_empty() {
            let newest = records.iter().map(|r| (r.measurement.exe.clone(), r)).fold(
                std::collections::BTreeMap::<String, &crate::benchmark::BenchRecord>::new(),
                |mut m, (exe, r)| {
                    m.insert(exe, r); // later records replace earlier ones
                    m
                },
            );
            let mut warms: Vec<u64> = newest
                .values()
                .filter_map(|r| r.measurement.warm_micros.or(r.measurement.first_micros))
                .collect();
            if !warms.is_empty() {
                warms.sort_unstable();
                f.bench = Some(Bench {
                    warm_micros: warms[warms.len() / 2],
                    peak_rss_kib: newest
                        .values()
                        .filter_map(|r| r.measurement.real_rss_kib())
                        .max(),
                    programs: newest.len(),
                    recorded: newest.values().map(|r| r.time).max().unwrap_or(0),
                });
            }
        }

        // build time and the source/binary advice
        if let Ok(facts) = self.recommend_facts(name, offline) {
            let report = crate::recommend::evaluate(&facts);
            f.build = report.build_secs.map(|(_, likely, _)| BuildFact {
                secs: likely,
                measured: report.build_basis == "measured",
            });
            f.source = report.score.map(|score| SourceFact {
                score,
                verdict: report.verdict.word().to_string(),
            });
        }

        f.repo_kind = if !official {
            RepoKind::Aur
        } else if crate::score_more::is_arch_repo(&details.repo) {
            RepoKind::Arch
        } else {
            RepoKind::ThirdParty
        };
        f.repro_for = self
            .alpm
            .installed_version(&details.name)
            .unwrap_or_else(|| details.version.clone());
        let mut meta_for_outside = None;
        if official {
            if let Some(meta) = self.alpm.repo_meta(&details.name) {
                f.age_days = Some(days_ago(meta.build_date));
                f.signed = Some(meta.signed);
                f.dependents = Some(meta.dependents);
                meta_for_outside = Some(meta);
            }
            // the Arch Security Tracker, for the installed version if there is one
            let checked = self
                .alpm
                .installed_version(&details.name)
                .unwrap_or_else(|| details.version.clone());
            match crate::security_tracker::load(offline, t) {
                Ok(list) => {
                    f.tracker = Some(crate::score::TrackerFact {
                        entries: crate::security_tracker::assess_now(
                            &list.groups,
                            &details.name,
                            &checked,
                            t,
                        ),
                        version: checked,
                        list_age_hours: t.saturating_sub(list.fetched_at) / 3600,
                        stale_note: list.note,
                    });
                }
                Err(e) => f.tracker_note = Some(e.to_string()),
            }
            // Arch news of the last 90 days that name it (the cached feed only)
            let news = crate::arch_news::load_fresh(true, t);
            let word = details.name.to_lowercase();
            f.news_mentions = news
                .items
                .iter()
                .filter(|i| t.saturating_sub(i.published) <= 90 * 86_400)
                .filter(|i| {
                    i.title
                        .to_lowercase()
                        .split(|c: char| {
                            !(c.is_alphanumeric() || c == '-' || c == '_' || c == '+' || c == '.')
                        })
                        .any(|w| w == word)
                })
                .count();
        } else {
            f.age_days = details.last_modified.map(days_ago);
            f.out_of_date_days = details.out_of_date.map(days_ago);
            f.orphan = details.maintainer.is_none();
            f.votes = details.votes;
            f.popularity = details.popularity;
            for signal in self.aur_trust_for_info(&details) {
                match signal.level {
                    crate::aur_trust::Level::High => f.trust_high += 1,
                    crate::aur_trust::Level::Warning => f.trust_warning += 1,
                    crate::aur_trust::Level::Note => {}
                }
            }
            if offline {
                f.audit_note = Some("--offline is set".to_string());
            } else {
                match self.audit_aur(name) {
                    Ok(report) => {
                        let count = |s: crate::audit::Severity| {
                            report.findings.iter().filter(|x| x.severity == s).count()
                        };
                        f.audit = Some(AuditCounts {
                            critical: count(crate::audit::Severity::Critical),
                            high: count(crate::audit::Severity::High),
                            warning: count(crate::audit::Severity::Warning),
                        });
                    }
                    Err(e) => f.audit_note = Some(format!("the PKGBUILD could not be read: {e}")),
                }
            }
        }
        self.score_outside(&mut f, &details, meta_for_outside.as_ref(), offline, t);
        Ok(f)
    }

    /// The facts of `emrac score` that come from outside the package databases:
    /// provenance and compatibility (the databases and pacman.conf), the Arch
    /// rebuilders, the packaging history, the OpenSSF Scorecard and OSV. A source
    /// that cannot be reached leaves its dimension unknown, with the reason.
    fn score_outside(
        &self,
        f: &mut crate::score::Facts,
        details: &PackageDetails,
        meta: Option<&crate::backend::alpm::RepoMeta>,
        offline: bool,
        t: u64,
    ) {
        use crate::score::{CompatFact, ProvenanceFact, UpstreamFact};
        use crate::score_sources as ss;
        let name = details.name.as_str();
        f.provenance = Some(ProvenanceFact {
            kind: f.repo_kind,
            repo: details.repo.clone(),
            packager: meta.and_then(|m| m.packager.clone()),
            sig_required: meta.is_some_and(|m| m.sig_required),
            sig_never: meta.is_some_and(|m| m.sig_never),
            repo_packages: meta.map_or(0, |m| m.repo_packages),
        });
        if let Some(m) = meta {
            f.compat = Some(CompatFact {
                arch: m.arch.clone(),
                system_arch: m.system_arch.clone(),
                missing_deps: m.missing_deps.clone(),
                conflicts: m.conflicts_installed.clone(),
                new_deps: m.new_deps,
                installed: false,
            });
        } else if f.repo_kind == crate::score::RepoKind::Aur {
            // an installed AUR build is checked from the local database
            f.compat = self.alpm.installed_compat(name);
        }
        if f.repo_kind == crate::score::RepoKind::Arch && ss::valid_name(name) {
            let req = ss::Request {
                kind: "repro",
                key: name,
                url: ss::address("EMRAC_REPRO_URL", ss::REPRO_URL, name),
                post: None,
                ttl: ss::REPRO_TTL,
            };
            match ss::get(&req, offline, t) {
                Ok(got) => {
                    if let Some(body) = got.body {
                        match ss::parse_repro(
                            &body,
                            name,
                            &f.repro_for,
                            got.fetched_at,
                            t,
                            got.note,
                        ) {
                            Ok(Some(r)) => f.repro = Some(r),
                            Ok(None) => {}
                            Err(e) => f.repro_note = Some(format!("not checked: {e}")),
                        }
                    }
                }
                Err(e) => f.repro_note = Some(format!("not checked: {e}")),
            }
        }
        if f.repo_kind == crate::score::RepoKind::Arch {
            let base = meta.map_or(name, |m| m.base.as_str());
            if !ss::valid_name(base) {
                f.packaging_note = Some("the package base has an unexpected name".to_string());
            } else {
                let req = ss::Request {
                    kind: "commits",
                    key: base,
                    url: ss::address("EMRAC_PACKAGING_URL", ss::COMMITS_URL, base),
                    post: None,
                    ttl: ss::SLOW_TTL,
                };
                match ss::get(&req, offline, t) {
                    Ok(got) => match got.body {
                        Some(body) => match ss::parse_commits(&body, base, t) {
                            Ok(Some(p)) => f.packaging = Some(p),
                            Ok(None) => {
                                f.packaging_note = Some(
                                    "the packaging repository has no PKGBUILD history".to_string(),
                                )
                            }
                            Err(e) => f.packaging_note = Some(format!("not checked: {e}")),
                        },
                        None => {
                            f.packaging_note = Some(format!(
                                "no packaging repository named {base} on gitlab.archlinux.org"
                            ))
                        }
                    },
                    Err(e) => f.packaging_note = Some(format!("not checked: {e}")),
                }
            }
        } else {
            f.packaging_note = Some(
                "the packaging history is Arch's; this package is not from an Arch repository"
                    .to_string(),
            );
        }
        // upstream: only a GitHub address can be named
        let repo = details.url.as_deref().and_then(ss::github_repo);
        match repo {
            None => {
                f.upstream_note = Some(
                    "upstream not identified: the package's URL is not a GitHub repository"
                        .to_string(),
                );
            }
            Some(repo) => {
                let mut up = UpstreamFact {
                    repo: Some(repo.clone()),
                    scorecard: None,
                    osv: None,
                };
                let mut problems = Vec::new();
                let req = ss::Request {
                    kind: "scorecard",
                    key: &repo.replace('/', "_"),
                    url: ss::address("EMRAC_SCORECARD_URL", ss::SCORECARD_URL, &repo)
                        .replace("%2F", "/"),
                    post: None,
                    ttl: ss::SLOW_TTL,
                };
                match ss::get(&req, offline, t) {
                    Ok(got) => {
                        if let Some(body) = got.body {
                            match ss::parse_scorecard(&body) {
                                Ok(s) => up.scorecard = s,
                                Err(e) => problems.push(format!("Scorecard: {e}")),
                            }
                        }
                    }
                    Err(e) => problems.push(format!("Scorecard: {e}")),
                }
                let ver = ss::upstream_version(&details.version);
                let post = serde_json::json!({
                    "package": {"name": format!("https://github.com/{repo}"), "ecosystem": "GIT"},
                    "version": ver,
                })
                .to_string();
                let req = ss::Request {
                    kind: "osv",
                    key: &format!("{}_{ver}", repo.replace('/', "_")),
                    url: std::env::var("EMRAC_OSV_URL").unwrap_or_else(|_| ss::OSV_URL.to_string()),
                    post: Some(post),
                    ttl: ss::SLOW_TTL,
                };
                match ss::get(&req, offline, t) {
                    Ok(got) => {
                        if let Some(body) = got.body {
                            match ss::parse_osv(&body, &ver) {
                                Ok(o) => up.osv = Some(o),
                                Err(e) => problems.push(format!("OSV: {e}")),
                            }
                        }
                    }
                    Err(e) => problems.push(format!("OSV: {e}")),
                }
                if up.scorecard.is_some() || up.osv.is_some() {
                    f.upstream = Some(up);
                } else if !problems.is_empty() {
                    f.upstream_note = Some(format!("not checked: {}", problems.join("; ")));
                } else {
                    f.upstream = Some(up);
                }
            }
        }
    }

    /// Makes a bundle in `dir` (which must not exist): the repository packages
    /// that `roots` need, or, with no roots, every upgrade `target` waits for.
    /// With `target`, only what that machine lacks or has older.
    pub fn bundle_create(
        &self,
        dir: &std::path::Path,
        roots: &[String],
        target: Option<&crate::exchange::Exchange>,
    ) -> Result<crate::bundle::Manifest> {
        use crate::bundle::{self, BundlePackage, Manifest, Meta, Skipped};
        let fail = |why: String| Error::Bundle(why);
        let target_versions: Option<std::collections::BTreeMap<String, String>> = target.map(|t| {
            t.package
                .iter()
                .map(|p| (p.name.clone(), p.version.clone()))
                .collect()
        });
        let mut skipped: Vec<Skipped> = Vec::new();
        let wanted: Vec<String> = if roots.is_empty() {
            let (Some(have), Some(t)) = (&target_versions, target) else {
                return Err(fail(
                    "name the packages, or give --for <the target's export file> to bundle its upgrades".into(),
                ));
            };
            let mut names = Vec::new();
            for p in &t.package {
                if p.foreign {
                    continue;
                }
                if let Some(newer) = self.alpm.sync_version(&p.name) {
                    if version_is_newer(have.get(&p.name).map_or("", String::as_str), &newer) {
                        names.push(p.name.clone());
                    }
                }
            }
            if names.is_empty() {
                return Err(fail(
                    "nothing to bundle: the target is up to date with these repositories".into(),
                ));
            }
            names
        } else {
            roots.to_vec()
        };
        for r in &wanted {
            if !crate::exchange::valid_package_name(r) {
                return Err(fail(format!("'{r}' is not a package name")));
            }
        }
        let (closure, unresolved) = self.alpm.sync_closure(&wanted);
        if !unresolved.is_empty() {
            return Err(fail(format!(
                "no repository provides: {} (AUR packages are not bundled: build them on a machine that has the network and install them normally)",
                unresolved.join(", ")
            )));
        }
        let chosen = match &target_versions {
            Some(have) => bundle::needed(&closure, have, &|a, b| version_is_newer(b, a)),
            None => closure,
        };
        if chosen.is_empty() {
            return Err(fail(
                "nothing to bundle: the target already has all of it".into(),
            ));
        }
        if dir.exists() {
            return Err(fail(format!(
                "{} already exists; a bundle goes in a new folder",
                dir.display()
            )));
        }
        let pkg_dir = dir.join("pkg");
        std::fs::create_dir_all(&pkg_dir)
            .map_err(|e| fail(format!("{}: {e}", pkg_dir.display())))?;
        let built = (|| -> Result<Manifest> {
            let specs: Vec<String> = chosen
                .iter()
                .map(|p| format!("{}/{}", p.repo, p.name))
                .collect();
            self.pacman.download_into(&pkg_dir, &specs)?;
            let mut packages = Vec::new();
            for p in &chosen {
                let file = pkg_dir.join(&p.filename);
                let sig = pkg_dir.join(format!("{}.sig", p.filename));
                if !file.is_file() {
                    return Err(fail(format!(
                        "pacman did not leave {} in the bundle",
                        p.filename
                    )));
                }
                if !sig.is_file() {
                    return Err(fail(format!(
                        "{} has no signature file; a bundle only carries signed packages",
                        p.filename
                    )));
                }
                packages.push(BundlePackage {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    repo: p.repo.clone(),
                    file: p.filename.clone(),
                    size: std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
                    sha256: bundle::sha256_file(&file)?,
                    sig_sha256: bundle::sha256_file(&sig)?,
                });
            }
            let manifest = Manifest {
                meta: Meta {
                    format: bundle::FORMAT,
                    emrac: env!("CARGO_PKG_VERSION").to_string(),
                    created: crate::history::format_utc_date(now()),
                    arch: std::env::consts::ARCH.to_string(),
                },
                requested: roots.to_vec(),
                for_target: target.map(|t| t.meta.exported.clone()),
                package: packages,
                skipped: std::mem::take(&mut skipped),
            };
            let path = dir.join("manifest.toml");
            std::fs::write(&path, bundle::render(&manifest))
                .map_err(|e| fail(format!("{}: {e}", path.display())))?;
            Ok(manifest)
        })();
        if built.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        built
    }

    /// The installed packages as (name, version), for comparing with a bundle.
    pub fn installed_pairs(&self) -> Vec<(String, String)> {
        self.alpm
            .installed_snapshot()
            .into_iter()
            .map(|p| (p.name, p.version))
            .collect()
    }

    /// Installs the bundle's package files; pacman must find every signature valid.
    pub fn execute_bundle(&self, files: &[std::path::PathBuf]) -> Result<()> {
        self.pacman.execute_install_files_signed(files)
    }

    /// The exchange file for this machine (`emrac export`).
    pub fn export(
        &self,
        scope: crate::exchange::Scope,
        config: &crate::config::Config,
    ) -> Result<crate::exchange::Exchange> {
        let pins = crate::pins::PinStore::list()?;
        Ok(crate::exchange::build(
            scope,
            &self.alpm.installed_snapshot(),
            &self.alpm.sync_repo_names(),
            config,
            &pins,
            &crate::history::format_utc_date(now()),
        ))
    }

    /// How an exchange file differs from this machine (`emrac import`).
    pub fn import_report(
        &self,
        ex: &crate::exchange::Exchange,
        scope: crate::exchange::Scope,
        config: &crate::config::Config,
    ) -> Result<crate::exchange::ImportReport> {
        let installed: Vec<(String, String)> = self
            .alpm
            .installed_snapshot()
            .into_iter()
            .map(|p| (p.name, p.version))
            .collect();
        let pins = crate::pins::PinStore::list()?;
        Ok(crate::exchange::compare(
            ex,
            scope,
            &installed,
            config,
            &self.alpm.sync_repo_names(),
            &pins,
        ))
    }

    /// Build and transaction statistics for `emrac stats`.
    pub fn stats(&self, since_secs: Option<u64>) -> crate::build_stats::Stats {
        let history = History::all().unwrap_or_default();
        crate::build_stats::compute(
            &crate::recommend::read_builds(),
            &history,
            since_secs,
            now(),
        )
    }

    /// What `emrac benchmark` may run for an installed package: its version,
    /// installed size and the executables it owns in `/usr/bin` and `/usr/sbin`.
    pub fn benchmark_targets(&self, name: &str) -> Result<BenchmarkTargets> {
        let alpm = AlpmBackend::init()?;
        let recorded = alpm.recorded_files(name).ok_or_else(|| {
            Error::Benchmark(format!(
                "'{name}' isn't installed, so there is nothing to measure"
            ))
        })?;
        let root = alpm.root();
        Ok(BenchmarkTargets {
            version: recorded.version.clone(),
            installed_size: alpm.installed_size(name).unwrap_or(0),
            executables: crate::benchmark::runnable(&recorded.files, std::path::Path::new(&root)),
        })
    }

    /// The tools a build left behind: packages installed since `before` as
    /// dependencies that nothing else needs (`docs/plan/r3/u7-build-tools.md`).
    /// Nothing is proposed when `before` is empty, because then nothing is
    /// known about what was there.
    pub fn build_tools_since(&self, before: &HashSet<String>) -> Vec<BuildTool> {
        if before.is_empty() {
            return Vec::new();
        }
        match AlpmBackend::init() {
            Ok(alpm) => build_tools::removable(&alpm.new_dependencies_since(before)),
            Err(_) => Vec::new(),
        }
    }

    /// Everything worth knowing about a plan beyond its packages: the hooks it
    /// runs and the config files it leaves as `.pacnew` or `.pacsave`.
    pub fn preview(&self, plan: &Plan) -> PlanPreview {
        PlanPreview {
            hooks: self.hook_preview(plan),
            config: self.config_preview(plan),
            policy: self.policy_preview(plan),
            advice: self.advice(plan),
        }
    }

    /// Advice for a plan that is unusually big: the Package Advisor
    /// (`docs/plan/r3/u15-package-advisor.md`). Sizes come from the sync
    /// databases; an AUR package has none until it is built.
    pub fn advice(&self, plan: &Plan) -> Vec<crate::advisor::Advice> {
        use crate::advisor::{advise, Input, Sized, Thresholds};
        let thresholds =
            Thresholds::from_config(&crate::config::Config::load().unwrap_or_default().advisor);
        if !thresholds.enabled || plan.is_empty() {
            return Vec::new();
        }
        let names: Vec<String> = plan.packages.iter().map(|p| p.name.clone()).collect();
        let resolved = self.alpm.sync_resolved(&names);
        let packages: Vec<Sized> = plan
            .packages
            .iter()
            .map(|p| match resolved.iter().find(|r| r.name == p.name) {
                Some(r) => Sized {
                    name: p.name.clone(),
                    explicit: p.explicit,
                    download: r.download_size,
                    installed: r.installed_size,
                    unknown: false,
                },
                None => Sized {
                    name: p.name.clone(),
                    explicit: p.explicit,
                    download: 0,
                    installed: 0,
                    unknown: true,
                },
            })
            .collect();
        advise(
            &Input {
                action: plan.action,
                packages,
                download_total: plan.total_download_size,
                growth: plan.total_installed_size_delta.max(0) as u64,
                free: crate::preflight::free_on_root(),
            },
            &thresholds,
        )
    }

    /// The files under your local file policy (`override`, `divert`) that the
    /// plan's packages touch, and what will happen to them. For a diversion,
    /// whether the new package changes the file is exact when that package is
    /// already in the cache, otherwise a maybe. Read-only.
    pub fn policy_preview(&self, plan: &Plan) -> crate::policy_preview::PolicyPreview {
        use crate::hooks::Operation;
        use crate::policy_preview::{effects, PolicyPreview, Touch};
        if plan.is_empty() {
            return PolicyPreview::default();
        }
        let policy = crate::policy::load(Path::new(crate::policy::POLICY_FILE)).unwrap_or_default();
        if policy.is_empty() {
            return PolicyPreview::default();
        }
        let touches: Vec<Touch> = self
            .alpm
            .hook_facts(plan)
            .into_iter()
            .map(|fact| {
                let mut files = fact.files.clone().unwrap_or_default();
                if fact.operation != Operation::Remove {
                    files.extend(fact.old_files.iter().cloned());
                }
                Touch {
                    package: fact.name,
                    operation: fact.operation,
                    files,
                }
            })
            .collect();
        let cache_dirs = self.pacman.cache_dirs().unwrap_or_default();
        let versions: std::collections::HashMap<&str, &str> = plan
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();
        // The sha256 of a file inside the package that is about to be installed.
        let package_sha = |package: &str, rel: &str| -> Option<String> {
            let version = versions.get(package)?;
            let file = rollback::find_package_file(&cache_dirs, package, version)?;
            let extracted = crate::backend::Extracted::new(&file, &[rel.to_string()]).ok()?;
            crate::policy::sha256_file(&extracted.path(rel)?).ok()
        };
        let found = effects(&policy, &touches, &package_sha);
        let hook_problem = if found.is_empty() {
            None
        } else {
            crate::policy_ops::Paths::system()
                .and_then(|paths| crate::policy_ops::status(&paths))
                .ok()
                .and_then(|status| status.hook_note)
        };
        PolicyPreview {
            effects: found,
            hook_problem,
        }
    }

    /// The config files of the plan's packages that you changed and what
    /// pacman will do with them (`.pacnew` beside yours, `.pacsave` on
    /// removal). When the new package is already in the cache its files are
    /// compared exactly; otherwise an edited file is reported as a maybe.
    /// Read-only; needs no root, except that a file only root can read is a maybe.
    pub fn config_preview(&self, plan: &Plan) -> ConfigPreview {
        if plan.is_empty() {
            return ConfigPreview::default();
        }
        let inputs = self.alpm.config_inputs(plan);
        if inputs.is_empty() {
            return ConfigPreview::default();
        }
        let no_upgrade = AlpmBackend::no_upgrade_patterns();
        let cache_dirs = self.pacman.cache_dirs().unwrap_or_default();

        // The md5 of each edited file in the new package, when that package is
        // in the cache: one extraction per package.
        let mut new_md5: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        let mut packages: Vec<(&str, &str)> = inputs
            .iter()
            .filter(|i| i.new_version.is_some())
            .map(|i| (i.package.as_str(), i.new_version.as_deref().unwrap_or("")))
            .collect();
        packages.sort();
        packages.dedup();
        for (name, version) in packages {
            let edited: Vec<String> = inputs
                .iter()
                .filter(|i| i.package == name)
                .filter(
                    |i| matches!(&i.observed, crate::purge::Observed::Hash(h) if *h != i.original),
                )
                .map(|i| i.rel.clone())
                .collect();
            if edited.is_empty() {
                continue;
            }
            let Some(file) = rollback::find_package_file(&cache_dirs, name, version) else {
                continue;
            };
            let Ok(extracted) = crate::backend::Extracted::new(&file, &edited) else {
                continue;
            };
            for rel in edited {
                if let Some(path) = extracted.path(&rel) {
                    if let Ok(md5) = alpm::compute_md5sum(path.display().to_string()) {
                        new_md5.insert((name.to_string(), rel), md5);
                    }
                }
            }
        }

        let mut effects = Vec::new();
        for input in &inputs {
            let verdict = if input.new_version.is_none() {
                config_preview::on_removal(&input.original, &input.observed)
            } else {
                let listed = no_upgrade
                    .iter()
                    .any(|pattern| hooks::glob_match(pattern, &input.rel));
                let shipped = input
                    .new_files
                    .as_ref()
                    .map(|files| files.contains(&input.rel));
                config_preview::on_upgrade(
                    &input.original,
                    &input.observed,
                    new_md5
                        .get(&(input.package.clone(), input.rel.clone()))
                        .map(String::as_str),
                    shipped,
                    listed,
                )
            };
            if let Some((effect, reason)) = verdict {
                effects.push(ConfigEffect {
                    package: input.package.clone(),
                    path: input.path.clone(),
                    effect,
                    reason: reason.to_string(),
                });
            }
        }
        effects.sort_by(|a, b| (&a.package, &a.path).cmp(&(&b.package, &b.path)));
        ConfigPreview { effects }
    }

    /// The pacman hooks `plan` will run, in the order pacman runs them, and
    /// which of them would stop the transaction. Read-only; needs no root. A
    /// hook directory that cannot be read is skipped, so an unusual setup gives
    /// a shorter list, never an error.
    pub fn hook_preview(&self, plan: &Plan) -> HookPreview {
        if plan.is_empty() {
            return HookPreview::default();
        }
        let dirs = AlpmBackend::hook_dirs().unwrap_or_default();
        let loaded = hooks::load_hooks(&dirs);
        let facts = self.alpm.hook_facts(plan);
        let coming: Vec<String> = if matches!(plan.action, PlanAction::Remove | PlanAction::Purge) {
            Vec::new()
        } else {
            plan.packages.iter().map(|p| p.name.clone()).collect()
        };
        let mut preview = hooks::plan_hooks(
            &loaded.hooks,
            &facts,
            &|dep| self.alpm.hook_dependency_met(dep, &coming),
            &|path| self.alpm.root_has(path),
        );
        preview.unusable = loaded.unusable;
        preview
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
        let mut packages = names
            .into_iter()
            .map(|name| self.alpm.verify_installed(name, &no_extract))
            .collect::<Result<Vec<_>>>()?;
        // What your own file policy explains (an overridden mode, a diverted
        // file's content) is not damage.
        let policy = crate::policy::load(Path::new(crate::policy::POLICY_FILE)).unwrap_or_default();
        for report in &mut packages {
            crate::verify::explain_by_policy(report, &policy);
        }
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

    /// The same file from the repositories (when they still offer exactly that
    /// version) or the Arch Linux Archive (read-only lookups), unless
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
        // The current version is on the mirrors; the Archive may not have it
        // yet. This is what lets a lean cache drop it (`docs/plan/r1/u29`).
        if let Some(url) = self.alpm.sync_download_url(name, version) {
            if let Some(filename) = url.rsplit('/').next() {
                return Ok(Some(RemoteFile {
                    dest: self.archive.cache_dir(name)?.join(filename),
                    url,
                }));
            }
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
        let mut report = doctor::evaluate(&self.doctor_inputs(online));
        report.checks.push(self.vuln_doctor_check(online));
        // Problems first, then warnings, notes and ok; the order within a group is kept.
        report.checks.sort_by_key(|c| std::cmp::Reverse(c.status));
        Ok(report)
    }

    /// Installed packages against the Arch Security Tracker. `offline` uses the
    /// saved list only.
    pub fn vuln_report(&self, offline: bool) -> Result<crate::security_tracker::VulnReport> {
        let t = now();
        let list = crate::security_tracker::load(offline, t)?;
        Ok(crate::security_tracker::report(
            &list,
            &self.installed_pairs(),
            &|n| self.alpm.sync_version(n),
            &|a, b| alpm::vercmp(a, b),
            &crate::security_tracker::read_ignores(),
            t,
        ))
    }

    /// The doctor's row about known vulnerabilities. It uses the saved list and
    /// asks the network only for `--online`.
    fn vuln_doctor_check(&self, online: bool) -> doctor::Check {
        use crate::security_tracker::Severity;
        let mk =
            |status, summary: String, detail: Vec<String>, fix: Option<String>| doctor::Check {
                id: "vulnerabilities",
                title: "Known vulnerabilities",
                status,
                summary,
                detail,
                fix,
            };
        let report = match self.vuln_report(!online) {
            Ok(r) => r,
            Err(e) => {
                return mk(
                    doctor::Status::Note,
                    format!("not checked: {e}"),
                    Vec::new(),
                    Some("emrac vulns".to_string()),
                )
            }
        };
        let confirmed: Vec<&crate::security_tracker::VulnPackage> = report
            .packages
            .iter()
            .filter(|p| p.worst_confirmed().is_some())
            .collect();
        let others = report.packages.len() - confirmed.len();
        let age = format!("(list {} h old)", report.list_age_hours);
        if confirmed.is_empty() {
            let extra = if others > 0 {
                format!("; {others} package(s) have only possibly stale or ignored entries (`emrac vulns --all`)")
            } else {
                String::new()
            };
            return mk(
                doctor::Status::Ok,
                format!("no confirmed open Arch Security Tracker entry for the {} installed packages {age}{extra}", report.checked),
                Vec::new(),
                None,
            );
        }
        let hard = report.hard();
        let fixable = confirmed
            .iter()
            .flat_map(|p| p.entries.iter().map(move |e| (p, e)))
            .filter(|(p, e)| {
                e.standing.confirmed() && p.fix_in_repos(e, &|a, b| alpm::vercmp(a, b))
            })
            .count();
        let detail: Vec<String> = confirmed
            .iter()
            .take(5)
            .map(|p| {
                let worst = p.worst_confirmed().unwrap_or(Severity::Unknown);
                format!(
                    "{} {}: {} confirmed, worst {}",
                    p.name,
                    p.version,
                    p.entries.iter().filter(|e| e.standing.confirmed()).count(),
                    worst.word()
                )
            })
            .collect();
        let status = if hard > 0 && fixable > 0 {
            doctor::Status::Warning
        } else {
            doctor::Status::Note
        };
        mk(
            status,
            format!(
                "{} installed package(s) have a confirmed open Arch Security Tracker entry ({hard} High or Critical, {fixable} with a fix already in the repositories) {age}",
                confirmed.len()
            ),
            detail,
            Some(if fixable > 0 { "emrac vulns --fixable, then emrac upgrade".to_string() } else { "emrac vulns".to_string() }),
        )
    }

    /// The pre-flight check about vulnerabilities for a plan: the versions the
    /// plan would install against the tracker, and what the plan fixes. `None`
    /// when there is nothing to say (a removal, only AUR packages, or offline
    /// with no saved list).
    pub fn vuln_check_plan(&self, plan: &Plan, offline: bool) -> Option<doctor::Check> {
        if !matches!(
            plan.action,
            PlanAction::Install
                | PlanAction::Upgrade
                | PlanAction::Reinstall
                | PlanAction::Downgrade
        ) {
            return None;
        }
        let official: Vec<&PlannedPackage> =
            plan.packages.iter().filter(|p| p.repo != "aur").collect();
        if official.is_empty() {
            return None;
        }
        let t = now();
        let mk =
            |status, summary: String, detail: Vec<String>, fix: Option<String>| doctor::Check {
                id: "vulnerabilities",
                title: "Known vulnerabilities",
                status,
                summary,
                detail,
                fix,
            };
        let activity = crate::style::Activity::spinner("checking the Arch Security Tracker");
        let list = match crate::security_tracker::load(offline, t) {
            Ok(l) => {
                activity.finish("checked the Arch Security Tracker");
                l
            }
            Err(e) => {
                activity.fail("could not check the Arch Security Tracker");
                return (!offline).then(|| {
                    mk(
                        doctor::Status::Note,
                        format!("not checked: {e}"),
                        Vec::new(),
                        None,
                    )
                });
            }
        };
        let ignored = crate::security_tracker::read_ignores();
        let year = crate::security_tracker::year_of(t);
        let cmp = |a: &str, b: &str| alpm::vercmp(a, b);
        let mut hard: Vec<String> = Vec::new();
        let mut fixed_by_plan: Vec<String> = Vec::new();
        for p in &official {
            let groups: Vec<crate::security_tracker::Group> = list
                .groups
                .iter()
                .filter(|g| g.status == "Vulnerable" && g.packages.contains(&p.name))
                .cloned()
                .collect();
            if groups.is_empty() {
                continue;
            }
            let after =
                crate::security_tracker::assess(&groups, &p.name, &p.version, &cmp, &ignored, year);
            for e in after.iter().filter(|e| {
                e.standing.confirmed()
                    && e.found.severity >= crate::security_tracker::Severity::High
            }) {
                hard.push(format!(
                    "{} {}: {} {}",
                    p.name,
                    p.version,
                    e.found.id,
                    e.found.severity.word()
                ));
            }
            if let Some(have) = self.alpm.installed_version(&p.name) {
                let before =
                    crate::security_tracker::assess(&groups, &p.name, &have, &cmp, &ignored, year);
                for e in before.iter().filter(|e| e.standing.confirmed()) {
                    if !after.iter().any(|a| a.found.id == e.found.id) {
                        fixed_by_plan.push(format!("{} {}", e.found.id, p.name));
                    }
                }
            }
        }
        let age = format!("(list {} h old)", t.saturating_sub(list.fetched_at) / 3600);
        if !hard.is_empty() {
            return Some(mk(
                doctor::Status::Problem,
                format!("{} confirmed open High or Critical Arch Security Tracker {} for what this would install {age}", hard.len(), if hard.len() == 1 { "entry" } else { "entries" }),
                hard.into_iter().take(6).collect(),
                Some("emrac vulns --all shows them; `emrac vulns ignore AVG-... --reason ...` sets one aside; safety.vulnerabilities sets how strict this is".to_string()),
            ));
        }
        if !fixed_by_plan.is_empty() {
            return Some(mk(
                doctor::Status::Note,
                format!(
                    "this fixes {} Arch Security Tracker {} {age}",
                    fixed_by_plan.len(),
                    if fixed_by_plan.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    }
                ),
                fixed_by_plan.into_iter().take(6).collect(),
                None,
            ));
        }
        Some(mk(
            doctor::Status::Ok,
            format!("no confirmed open High or Critical Arch Security Tracker entry for the versions to install {age}"),
            Vec::new(),
            None,
        ))
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

    /// The text of `emrac bug-report`: the doctor report, the emrac version and
    /// the last three failed transactions, scrubbed as one piece. Printed by the
    /// caller; never sent anywhere.
    pub fn bug_report(&self) -> Result<String> {
        let report = self.doctor(false)?;
        let mut text = format!(
            "# emrac bug report\n\nemrac {}\n\n",
            env!("CARGO_PKG_VERSION")
        );
        text.push_str(&doctor::report_markdown(&report, &self.doctor_env()));
        text.push_str(&crate::bug_report::failed_section(
            &History::all_with_archive().unwrap_or_default(),
        ));
        let home = std::env::var("HOME").unwrap_or_default();
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_default();
        let host = read_trimmed("/proc/sys/kernel/hostname").unwrap_or_default();
        Ok(doctor::scrub(
            &text,
            &doctor::Scrub {
                home: &home,
                user: &user,
                host: &host,
            },
        ))
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
        let acknowledged = History::acknowledged(false).unwrap_or_default();
        let history = History::verify()
            .ok()
            .filter(|v| v.chained + v.legacy > 0)
            .map(|v| crate::doctor::HistoryFacts {
                problem: v.problem.as_ref().map(|b| b.describe()),
                predate: v.legacy,
                chained: v.chained,
                archives: v.archives,
            });
        let history_has_records = History::all().is_ok_and(|r| !r.is_empty());
        let interrupted = match crate::journal::state() {
            Ok(crate::journal::State::Interrupted(found)) => Some(match &found.entry {
                Some(e) => format!(
                    "{} of {} packages, started {} UTC",
                    action_word(e.action),
                    e.packages.len(),
                    crate::history::format_utc(e.started)
                ),
                None => "its journal is incomplete".to_string(),
            }),
            _ => None,
        };
        let ledger_path = crate::ledger::path(crate::ledger::running_as_root());
        let ledger = match crate::ledger::read(&ledger_path) {
            Ok(Some(l)) => {
                let protection = crate::ledger::protection(&ledger_path);
                let comparison = if l.problem.is_none() {
                    History::path()
                        .ok()
                        .and_then(|p| crate::ledger::compare(&l, &p).ok())
                } else {
                    None
                };
                let differences = comparison
                    .as_ref()
                    .map(|c| {
                        let mut d = Vec::new();
                        if !c.missing.is_empty() {
                            d.push(format!(
                                "missing from your history: {}",
                                c.missing.join(", ")
                            ));
                        }
                        if !c.changed.is_empty() {
                            d.push(format!("changed since recorded: {}", c.changed.join(", ")));
                        }
                        if !c.archives.is_empty() {
                            d.push(format!(
                                "archive files that differ: {}",
                                c.archives.join(", ")
                            ));
                        }
                        d
                    })
                    .unwrap_or_default();
                Some(crate::doctor::LedgerFacts {
                    entries: l.count,
                    chain_problem: l.problem.as_ref().map(|b| b.describe()),
                    differences,
                    pending: comparison.as_ref().map_or(0, |c| c.pending),
                    root_owned: protection.as_ref().is_some_and(|p| p.root_owned),
                    world_writable: protection.as_ref().is_some_and(|p| p.world_writable),
                    append_only: protection.and_then(|p| p.append_only),
                })
            }
            _ => None,
        };
        let ledger_watch = ledger.as_ref().map(|_| {
            let mut trusted = vec!["/usr/bin/emrac".to_string()];
            if let Ok(p) = std::env::current_exe() {
                trusted.push(p.display().to_string());
            }
            crate::ledger_watch::facts(&trusted)
        });
        let db_backups = Config::load()
            .is_ok_and(|c| c.safety.db_backup == "on")
            .then(|| {
                let all = crate::pacman_db::backups();
                (all.len(), all.first().map(|b| (b.time, b.bytes)))
            });
        let repo_rollbacks =
            crate::repo_trust::rollbacks(&crate::repo_trust::load(), &crate::repo_trust::current())
                .into_iter()
                .map(|r| {
                    format!(
                        "{}: now from {}, seen from {}",
                        r.repo,
                        crate::history::format_utc(r.now),
                        crate::history::format_utc(r.seen)
                    )
                })
                .collect();
        let stale = self.stale_builds();
        let stale_broken = stale
            .iter()
            .filter(|s| !s.broken.is_empty() && !s.binary)
            .map(|s| s.name.clone())
            .collect();
        let stale_binary = stale
            .iter()
            .filter(|s| !s.broken.is_empty() && s.binary)
            .map(|s| s.name.clone())
            .collect();
        let stale_maybe = stale
            .iter()
            .filter(|s| s.broken.is_empty() && !s.binary)
            .map(|s| s.name.clone())
            .collect();
        let recent_failed_transactions = History::all()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                r.status == TxStatus::Failed
                    && r.timestamp + 7 * 86_400 >= now_secs
                    && !acknowledged.contains(&r.id)
            })
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

        // The local file policy (u8): diversions the package has moved on from,
        // and anything not in effect.
        let (stale_diversions, policy_drift) = match crate::policy_ops::Paths::system()
            .and_then(|paths| crate::policy_ops::status(&paths))
        {
            Ok(status) => {
                use crate::policy_ops::LineState;
                let stale = crate::policy_ops::stale_diversions(&status)
                    .into_iter()
                    .map(|l| l.path.clone())
                    .collect();
                let mut drift: Vec<String> = status
                    .lines
                    .iter()
                    .filter(|l| l.state == LineState::Drift)
                    .map(|l| format!("{} {}: {}", l.kind, l.path, l.text))
                    .collect();
                drift.extend(status.hook_note.clone());
                (stale, drift)
            }
            Err(_) => (Vec::new(), Vec::new()),
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
            stale_diversions,
            policy_drift,
            recent_failed_transactions,
            stale_broken,
            stale_binary,
            stale_maybe,
            source_policy: Some(self.source_policy_waiting()),
            source_risks: {
                let mut r: Vec<(String, &'static str, String, u8)> = self
                    .source_policy_risks()
                    .into_iter()
                    .filter_map(|(n, risk)| {
                        let level = match risk {
                            crate::source_built::Risk::Low => return None,
                            crate::source_built::Risk::Unknown(_) => 0,
                            crate::source_built::Risk::CompatibilityRisk(_) => 1,
                            crate::source_built::Risk::Broken(_) => 2,
                        };
                        Some((n, risk.word(), risk.why().to_string(), level))
                    })
                    .collect();
                r.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.cmp(&b.0)));
                r
            },
            db_backups,
            repo_rollbacks,
            history,
            ledger,
            history_has_records,
            interrupted,
            ledger_watch,
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

    /// The children of a package for `tree` and `graph`: installed packages
    /// satisfying its required dependencies and, if asked, its optional ones.
    fn dependency_children(&self, name: &str, optional: bool) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self
            .alpm
            .installed_dependency_names(name)
            .into_iter()
            .map(|n| (n, false))
            .collect();
        if optional {
            for n in self.alpm.installed_optional_names(name) {
                if !out.iter().any(|(m, _)| *m == n) {
                    out.push((n, true));
                }
            }
        }
        out
    }

    /// Foreign packages that need building again, or may (see `stale_builds`).
    /// Packages with nothing to report are left out, and so are the ones that
    /// may break only and that you acknowledged (`emrac rebuild --ack`).
    /// Read-only, no network.
    pub fn stale_builds(&self) -> Vec<crate::stale_builds::StaleBuild> {
        self.stale_builds_split().0
    }

    /// What `stale_builds` shows, and what it left out because you acknowledged
    /// it (those would be listed otherwise).
    pub fn stale_builds_split(
        &self,
    ) -> (
        Vec<crate::stale_builds::StaleBuild>,
        Vec<crate::stale_builds::StaleBuild>,
    ) {
        let system = crate::stale_builds::system_libs();
        let interpreters: Vec<(&str, String)> = ["python", "perl"]
            .into_iter()
            .filter_map(|n| {
                let (version, _, _) = self.alpm.package_info(n)?;
                Some((n, crate::stale_builds::minor_of(&version)?))
            })
            .collect();
        let upgrades = std::fs::read_to_string("/var/log/pacman.log")
            .map(|t| crate::stale_builds::parse_upgrades(&t))
            .unwrap_or_default();
        let acks = crate::rebuild_ack::read();
        let (mut shown, mut hidden) = (Vec::new(), Vec::new());
        for pkg in self.alpm.foreign_details() {
            let all =
                crate::stale_builds::judge(&pkg, &system, &interpreters, &upgrades, Path::new("/"));
            if all.broken.is_empty() && all.maybe.is_empty() {
                continue;
            }
            let says: Vec<String> = all.broken.iter().chain(&all.maybe).cloned().collect();
            if (all.broken.is_empty() || all.binary)
                && crate::rebuild_ack::covers(&acks, &pkg, &says)
            {
                hidden.push(all);
            } else {
                shown.push(all);
            }
        }
        (shown, hidden)
    }

    /// Acknowledges (`ack`) or takes back (`!ack`) the note that these installed
    /// AUR packages may need building again. A package that is broken now cannot
    /// be acknowledged: that does not mend it.
    pub fn rebuild_ack(
        &self,
        names: &[String],
        ack: bool,
        reason: Option<&str>,
    ) -> Result<Vec<String>> {
        let file = crate::rebuild_ack::path()?;
        let foreign = self.alpm.foreign_details();
        let (shown, _) = self.stale_builds_split();
        let mut done = Vec::new();
        for name in names {
            if !ack {
                if crate::rebuild_ack::remove_from(&file, name)? {
                    done.push(name.clone());
                    continue;
                }
                return Err(Error::Rebuild(format!("{name} is not acknowledged")));
            }
            let pkg = foreign
                .iter()
                .find(|f| &f.name == name)
                .ok_or_else(|| Error::Rebuild(format!("{name} is not an installed AUR package")))?;
            let note = shown.iter().find(|s| &s.name == name);
            if note.is_some_and(|s| !s.broken.is_empty() && !s.binary) {
                return Err(Error::Rebuild(format!(
                    "{name} is broken now (`emrac rebuild --check` says how); acknowledging would not mend it: `emrac rebuild {name}` does"
                )));
            }
            // a binary package that is broken can be acknowledged: a rebuild would
            // not mend it, only the vendor or a package that provides the library can
            let says: Vec<String> = note
                .map(|s| s.broken.iter().chain(&s.maybe).cloned().collect())
                .unwrap_or_default();
            if says.is_empty() {
                return Err(Error::Rebuild(format!(
                    "nothing is said about {name} that could be acknowledged (`emrac rebuild --check` lists what is)"
                )));
            }
            crate::rebuild_ack::add_to(&file, pkg, &says, reason, now())?;
            done.push(name.clone());
        }
        Ok(done)
    }

    /// The facts the after-upgrade report needs (u20): the running kernel,
    /// the services the changed packages own and when they started, failed
    /// units, broken AUR builds and new `.pacnew` files. `failed_before` is
    /// what was failing before the upgrade, when known. Read-only; as a
    /// normal user only what systemd shows to a user.
    pub fn upgrade_facts(
        &self,
        changes: Vec<crate::upgrade_report::Change>,
        since: u64,
        failed_before: Option<Vec<String>>,
    ) -> crate::upgrade_report::Facts {
        let running_kernel = read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_default();
        let kernel_dirs: Vec<(String, String)> = std::fs::read_dir("/usr/lib/modules")
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let base = std::fs::read_to_string(e.path().join("pkgbase")).ok()?;
                        Some((
                            e.file_name().to_string_lossy().into_owned(),
                            base.trim().to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut services = Vec::new();
        for c in &changes {
            let files = self.alpm.package_files(&c.name);
            for unit in deps::service_units(&files)
                .into_iter()
                .filter(|u| u.ends_with(".service"))
            {
                if let Some(started) = service_start(&unit) {
                    services.push((unit, c.name.clone(), started));
                }
            }
        }
        let stale_broken = self
            .stale_builds()
            .into_iter()
            .filter(|s| !s.broken.is_empty() && !s.binary)
            .map(|s| s.name)
            .collect();
        let pacnew_new = find_pacnew(Path::new("/etc"))
            .iter()
            .filter(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .is_some_and(|d| d.as_secs() >= since)
            })
            .count();
        crate::upgrade_report::Facts {
            changes,
            since,
            running_kernel,
            kernel_dirs,
            services,
            failed_now: failed_units(&[]),
            failed_before,
            stale_broken,
            pacnew_new,
        }
    }

    /// The units failing now (system manager), for comparing before and after.
    pub fn failed_units_now(&self) -> Vec<String> {
        failed_units(&[])
    }

    /// `emrac rebuild`: the plan for building installed AUR (foreign) packages
    /// again from source. Everything named must be installed and foreign.
    pub fn plan_rebuild(&self, pkgs: &[String]) -> Result<Plan> {
        let mut problems = Vec::new();
        let mut packages = Vec::new();
        for name in pkgs {
            match self.alpm.installed_source(name) {
                None => problems.push(format!(
                    "'{name}' isn't installed (`emrac install {name}` installs it)"
                )),
                Some(crate::backend::InstalledSource::Official) => problems.push(format!(
                    "'{name}' is an official package: it is built for you already (`emrac reinstall {name}` puts it back)"
                )),
                Some(crate::backend::InstalledSource::Foreign) => {
                    let (version, _, explicit) = self
                        .alpm
                        .package_info(name)
                        .ok_or_else(|| Error::NotInstalled(name.clone()))?;
                    if packages.iter().any(|p: &PlannedPackage| p.name == *name) {
                        continue;
                    }
                    packages.push(PlannedPackage {
                        name: name.clone(),
                        version: version.clone(),
                        repo: "aur".to_string(),
                        explicit,
                        previous_version: Some(version),
                    });
                }
            }
        }
        if !problems.is_empty() {
            return Err(Error::Rebuild(problems.join("\n")));
        }
        Ok(Plan {
            action: PlanAction::Reinstall,
            packages,
            total_download_size: 0,
            total_installed_size_delta: 0,
            removals: Vec::new(),
            provider_choices: Vec::new(),
            warnings: Vec::new(),
        })
    }

    /// The pre-flight checks for a plan (u17). `check_partial` adds the
    /// partial-upgrade gate, which only `install` and `upgrade` use.
    pub fn preflight(
        &self,
        plan: &Plan,
        aur_builds: bool,
        offline: bool,
        check_partial: bool,
    ) -> Result<Vec<doctor::Check>> {
        let mut facts = crate::preflight::gather(plan, offline, aur_builds, &|n| {
            self.alpm.package_info(n).is_some()
        });
        facts.unattended = !self.pacman.is_interactive();
        let marks = crate::repo_trust::load();
        let current = crate::repo_trust::current();
        facts.rollbacks = crate::repo_trust::rollbacks(&marks, &current);
        crate::repo_trust::save(&crate::repo_trust::raised(&marks, &current));
        if check_partial && matches!(plan.action, PlanAction::Install | PlanAction::Upgrade) {
            let planned: Vec<(String, String)> = plan
                .packages
                .iter()
                .map(|p| (p.name.clone(), p.version.clone()))
                .collect();
            facts.downgrades = crate::repo_trust::downgrades(
                &planned,
                &|n| self.alpm.installed_version(n),
                &|planned, have| crate::backend::version_is_newer(planned, have),
            );
        }
        let from_repos = plan.packages.iter().any(|p| p.repo != "aur");
        if check_partial
            && from_repos
            && matches!(plan.action, PlanAction::Install | PlanAction::Upgrade)
        {
            facts.partial = Some(self.partial_upgrade_facts(plan)?);
        }
        let mut checks = crate::preflight::evaluate(&facts);
        if Config::load().map_or(true, |c| c.safety.vulnerabilities != "off") {
            if let Some(c) = self.vuln_check_plan(plan, offline) {
                checks.push(c);
                checks.sort_by_key(|c| std::cmp::Reverse(c.status));
            }
        }
        Ok(checks)
    }

    /// Upgrades the repositories have for installed packages that this plan
    /// would leave waiting. Pins are the user's own choice and not counted.
    fn partial_upgrade_facts(&self, plan: &Plan) -> Result<crate::preflight::Partial> {
        let (active, _) = self.pins()?;
        let ignore: Vec<String> = active.iter().map(|p| p.name.clone()).collect();
        let all = self.pacman.resolve_full_upgrade(&[], None)?;
        let kept = self.pacman.resolve_full_upgrade(&ignore, None)?;
        let planned: std::collections::HashSet<&str> =
            plan.packages.iter().map(|p| p.name.as_str()).collect();
        Ok(crate::preflight::Partial {
            pinned: all.len().saturating_sub(kept.len()),
            pending: kept
                .into_iter()
                .filter(|n| !planned.contains(n.as_str()))
                .collect(),
        })
    }

    /// `emrac tree`: the dependencies of an installed package, as a tree.
    pub fn tree(&self, pkg: &str, depth: Option<usize>, optional: bool) -> Result<TreeReport> {
        let (version, _, _) = self
            .alpm
            .package_info(pkg)
            .ok_or_else(|| Error::NotInstalled(pkg.to_string()))?;
        Ok(TreeReport {
            name: pkg.to_string(),
            version,
            lines: deps::tree_lines(pkg, depth, &|n| self.dependency_children(n, optional)),
        })
    }

    /// `emrac graph`: the same graph as `tree`, as nodes and edges.
    pub fn graph(&self, pkg: &str, depth: Option<usize>, optional: bool) -> Result<GraphReport> {
        self.alpm
            .package_info(pkg)
            .ok_or_else(|| Error::NotInstalled(pkg.to_string()))?;
        Ok(deps::graph_of(pkg, depth, &|n| {
            self.dependency_children(n, optional)
        }))
    }

    /// `emrac cycles`: packages that require each other in a ring.
    pub fn cycles(&self) -> Vec<Vec<String>> {
        let names = self.alpm.installed_names();
        deps::cycles(&names, &|n| self.alpm.installed_dependency_names(n))
    }

    /// `emrac impact <pkg> --remove`.
    pub fn impact_remove(&self, pkg: &str) -> Result<RemoveImpactReport> {
        let (version, size, explicit) = self
            .alpm
            .package_info(pkg)
            .ok_or_else(|| Error::NotInstalled(pkg.to_string()))?;
        let impact = deps::remove_impact(
            pkg,
            &|n| self.alpm.dependents(n).map(|d| d.0).unwrap_or_default(),
            &|n| self.alpm.dependents(n).map(|d| d.1).unwrap_or_default(),
            &|n| self.alpm.installed_dependency_names(n),
            &|n| self.alpm.is_explicit(n),
        );
        let item = |n: &String| {
            let (version, installed_size, _) = self.alpm.package_info(n).unwrap_or_default();
            ImpactItem {
                name: n.clone(),
                version,
                installed_size,
            }
        };
        let breaks: Vec<ImpactItem> = impact.breaks.iter().map(item).collect();
        let unneeded: Vec<ImpactItem> = impact.unneeded.iter().map(item).collect();
        let reclaimed_bytes = size
            + breaks.iter().map(|i| i.installed_size).sum::<u64>()
            + unneeded.iter().map(|i| i.installed_size).sum::<u64>();

        let enabled = enabled_units();
        let mut units = Vec::new();
        let mut desktop = Vec::new();
        let gone = std::iter::once(pkg.to_string())
            .chain(impact.breaks.iter().cloned())
            .chain(impact.unneeded.iter().cloned());
        for name in gone {
            let files = self.alpm.package_files(&name);
            for unit in deps::service_units(&files) {
                units.push(UnitFact {
                    enabled: enabled.contains(&unit),
                    unit,
                    package: name.clone(),
                });
            }
            desktop.extend(deps::desktop_entries(&files));
        }
        desktop.sort();
        desktop.dedup();
        Ok(RemoveImpactReport {
            name: pkg.to_string(),
            version,
            explicit,
            installed_size: size,
            breaks,
            optional_for: impact.optional_for,
            unneeded,
            reclaimed_bytes,
            units,
            desktop_entries: desktop,
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
        self.plan_restore_snapshot(&snapshot, prune, offline)
    }

    /// The states the system was in, rebuilt from the history (`bisect`).
    pub fn bisect_timeline(&self) -> Result<crate::bisect::Timeline> {
        let records = History::all_with_archive()?;
        Ok(crate::bisect::reconstruct(
            &self.alpm.installed_snapshot(),
            &records,
        ))
    }

    /// Every package version any state in `timeline` needs that is not installed
    /// now and that cannot be found (the cache, the repositories, the Arch
    /// Linux Archive), as `name version`. Nothing is changed or downloaded.
    pub fn bisect_missing(
        &self,
        timeline: &crate::bisect::Timeline,
        from: usize,
        offline: bool,
    ) -> Result<Vec<String>> {
        let installed: Vec<(String, String)> = self
            .alpm
            .installed_snapshot()
            .into_iter()
            .map(|p: SnapshotPackage| (p.name, p.version))
            .collect();
        let cache_dirs = self.pacman.cache_dirs()?;
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut missing = Vec::new();
        for point in &timeline.points[from..] {
            for p in &point.packages {
                let key = (p.name.clone(), p.version.clone());
                if installed.contains(&key) || seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                if self.locate_file(&cache_dirs, &p.name, &p.version).is_some() {
                    continue;
                }
                if p.foreign {
                    missing.push(format!("{} {}", p.name, p.version));
                    continue;
                }
                let in_repo = self
                    .alpm
                    .sync_resolved(std::slice::from_ref(&p.name))
                    .into_iter()
                    .next()
                    .is_some_and(|s| s.version == p.version);
                if !in_repo && self.locate_remote(offline, &p.name, &p.version)?.is_none() {
                    missing.push(format!("{} {}", p.name, p.version));
                }
            }
        }
        Ok(missing)
    }

    /// What installing the package files of a restore would also remove, as libalpm
    /// works it out (conflicts, replacements): the part of the effect a list of
    /// versions does not show. `Err` when it cannot be worked out.
    pub fn restore_side_removals(&self, plan: &RestorePlan) -> Result<Vec<String>> {
        if plan.files.is_empty() || !plan.downloads.is_empty() {
            return Ok(Vec::new());
        }
        let sim = AlpmBackend::simulate(None, SimOp::Files(&plan.files))?;
        Ok(sim.removals.into_iter().map(|r| r.name).collect())
    }

    /// As `plan_restore`, for a snapshot that is not in the store.
    pub fn plan_restore_snapshot(
        &self,
        snapshot: &Snapshot,
        prune: bool,
        offline: bool,
    ) -> Result<RestorePlan> {
        let installed: Vec<(String, String)> = self
            .alpm
            .installed_snapshot()
            .into_iter()
            .map(|p: SnapshotPackage| (p.name, p.version))
            .collect();
        let diff = snapshot::diff(snapshot, &installed, &version_is_newer);
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
            snapshot: snapshot.name.clone(),
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

/// The package versions it takes to put things back as they were before
/// `record`: what an upgrade replaced, and what a removal took away. Only for
/// packages the record really changed (the installed version is the one it
/// installed, or the package is gone), so a failed record counts for exactly
/// what it did. An install needs nothing (undoing it removes the package).
fn undo_needs(
    record: &crate::history::TransactionRecord,
    installed: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in &record.packages {
        match record.action {
            PlanAction::Remove | PlanAction::Purge => {
                if !installed.contains_key(&p.name) {
                    out.push((p.name.clone(), p.version.clone()));
                }
            }
            PlanAction::Upgrade | PlanAction::Downgrade => {
                if let (Some(v), true) = (
                    &p.previous_version,
                    installed.get(&p.name) == Some(&p.version),
                ) {
                    out.push((p.name.clone(), v.clone()));
                }
            }
            PlanAction::Install | PlanAction::Reinstall => {}
        }
    }
    for r in &record.removed {
        if !installed.contains_key(&r.name) {
            out.push((r.name.clone(), r.version.clone()));
        }
    }
    out
}

/// The packages `makepkg -s` installs before a build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildDeps {
    /// Not installed yet.
    pub install: Vec<String>,
    /// Installed, and upgraded to satisfy the build dependencies: part of an
    /// upgrade that nothing else asked for.
    pub upgrade: Vec<String>,
}

/// The dependency lines of a `.SRCINFO` (`depends`, `makedepends`,
/// `checkdepends`, with or without an `_arch` suffix), deduplicated. Only
/// used to explain a failure or warn in a plan, never to decide what to build.
fn srcinfo_dependencies(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(" = ") else {
            continue;
        };
        let key = key.split('_').next().unwrap_or(key);
        if matches!(key, "depends" | "makedepends" | "checkdepends")
            && !value.trim().is_empty()
            && !out.iter().any(|v| v == value.trim())
        {
            out.push(value.trim().to_string());
        }
    }
    out
}

/// Would installing the repository packages among `deps` that are missing
/// remove an installed package another one still requires? `None` also when
/// nothing is missing or the simulation cannot run.
fn dependency_conflict_for(
    alpm: &AlpmBackend,
    dbpath: Option<&std::path::Path>,
    deps: &[String],
) -> Option<crate::backend::DependencyConflict> {
    let missing = alpm.uninstalled_repo_dependencies(deps);
    if missing.is_empty() {
        return None;
    }
    match AlpmBackend::simulate(dbpath, SimOp::Install(&missing)) {
        Err(Error::DependencyConflict(conflict)) => Some(*conflict),
        _ => None,
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

/// Where the source of an official package is.
#[derive(Debug, Clone)]
pub struct ArchSource {
    pub name: String,
    /// The package base: Arch's packaging repository is named after it.
    pub base: String,
    pub repo: String,
    /// The repository's version, which is the tag.
    pub version: String,
}

/// A plan to install local package files, and the files.
#[derive(Debug, Clone)]
pub struct LocalInstall {
    pub plan: Plan,
    pub files: Vec<PathBuf>,
}

/// Whether a path names a package file: `*.pkg.tar` with a compression suffix.
pub fn is_package_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".pkg.tar",
        ".pkg.tar.zst",
        ".pkg.tar.xz",
        ".pkg.tar.gz",
        ".pkg.tar.bz2",
        ".pkg.tar.lz4",
        ".pkg.tar.lrz",
        ".pkg.tar.lzo",
        ".pkg.tar.z",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
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
/// Names of systemd units linked from a `*.wants` / `*.requires` directory
/// under `/etc/systemd/system`, that is, enabled on this machine.
fn enabled_units() -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(rd) = std::fs::read_dir("/etc/systemd/system") else {
        return out;
    };
    for dir in rd.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if !(name.ends_with(".wants") || name.ends_with(".requires")) {
            continue;
        }
        if let Ok(units) = std::fs::read_dir(dir.path()) {
            out.extend(
                units
                    .flatten()
                    .map(|u| u.file_name().to_string_lossy().into_owned()),
            );
        }
    }
    out
}

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
/// When a running service was started (unix seconds); `None` if it isn't
/// running or systemd can't say.
fn service_start(unit: &str) -> Option<u64> {
    let out = Command::new("systemctl")
        .args([
            "show",
            unit,
            "-p",
            "ActiveState",
            "-p",
            "ExecMainStartTimestamp",
            "--timestamp=unix",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.lines().any(|l| l == "ActiveState=active") {
        return None;
    }
    text.lines()
        .find_map(|l| l.strip_prefix("ExecMainStartTimestamp=@"))
        .and_then(|t| t.trim().parse().ok())
}

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

/// md5 of a file as `md5sum` prints it (what pacman records for config files).
fn md5_of(path: &str) -> Option<String> {
    let out = Command::new("md5sum").arg(path).output().ok()?;
    out.status
        .success()
        .then(|| {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()
                .map(str::to_string)
        })
        .flatten()
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
mod srcinfo_tests {
    use super::*;
    use crate::history::{TransactionRecord, TxStatus};
    use crate::plan::{PlannedPackage, PlannedRemoval};

    fn record(
        action: PlanAction,
        status: TxStatus,
        packages: &[(&str, &str, Option<&str>)],
    ) -> TransactionRecord {
        TransactionRecord {
            id: "tx-1".into(),
            timestamp: 0,
            action,
            packages: packages
                .iter()
                .map(|(n, v, p)| PlannedPackage {
                    name: n.to_string(),
                    version: v.to_string(),
                    repo: "extra".into(),
                    explicit: true,
                    previous_version: p.map(str::to_string),
                })
                .collect(),
            removed: vec![PlannedRemoval {
                name: "dropped".into(),
                version: "1-1".into(),
                reason: "conflicts with new".into(),
            }],
            status,
            error: None,
            rollback_of: None,
            plan_digest: None,
        }
    }

    #[test]
    fn undoing_needs_the_old_versions_of_only_what_the_record_really_changed() {
        let installed: std::collections::HashMap<String, String> = [
            ("applied".to_string(), "2-1".to_string()),
            ("never".to_string(), "1-1".to_string()), // the build failed: still old
        ]
        .into();
        // A failed upgrade that changed one package and not the other.
        let failed = record(
            PlanAction::Upgrade,
            TxStatus::Failed,
            &[
                ("applied", "2-1", Some("1-1")),
                ("never", "2-1", Some("1-1")),
            ],
        );
        let mut got = undo_needs(&failed, &installed);
        got.sort();
        assert_eq!(
            got,
            [
                ("applied".into(), "1-1".into()),
                ("dropped".into(), "1-1".into())
            ]
        );
        // A removal needs the removed version; an install needs nothing.
        let removal = record(
            PlanAction::Remove,
            TxStatus::Committed,
            &[("gone", "3-1", None)],
        );
        assert!(undo_needs(&removal, &installed).contains(&("gone".into(), "3-1".into())));
        let install = record(
            PlanAction::Install,
            TxStatus::Committed,
            &[("new", "1-1", None)],
        );
        assert_eq!(
            undo_needs(&install, &installed),
            [("dropped".to_string(), "1-1".to_string())]
        );
    }

    #[test]
    fn dependencies_come_from_every_dependency_key_once() {
        let text = "pkgbase = x\n\tpkgver = 1\n\tdepends = scenefx\n\tdepends = libdrm\n\tmakedepends = meson\n\tcheckdepends = bats\n\tdepends_x86_64 = lib32-x\n\toptdepends = vim: editing\n\tdepends = scenefx\n\nrelease = 1\n";
        assert_eq!(
            srcinfo_dependencies(text),
            ["scenefx", "libdrm", "meson", "bats", "lib32-x"]
        );
        assert!(srcinfo_dependencies("").is_empty());
    }

    #[test]
    fn only_a_failed_dependency_step_is_explained() {
        let failed = |phase: &str| Error::BuildFailed {
            phase: phase.into(),
            status: "exit status: 8".into(),
            hint: String::new(),
        };
        assert!(failed("dependencies and sources").stopped_on_dependencies());
        assert!(!failed("build and package").stopped_on_dependencies());
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

/// The part of an installed package that `emrac benchmark` can run.
#[derive(Debug, Clone)]
pub struct BenchmarkTargets {
    pub version: String,
    pub installed_size: u64,
    pub executables: Vec<std::path::PathBuf>,
}

#[cfg(test)]
mod local_file_tests {
    use super::*;

    #[test]
    fn package_files_are_told_from_package_names() {
        for f in [
            "a-1-1-x86_64.pkg.tar.zst",
            "/x/y/b-2-1-any.pkg.tar.xz",
            "c.pkg.tar",
            "D-1.PKG.TAR.GZ",
            "./e.pkg.tar.lz4",
        ] {
            assert!(is_package_file_name(f), "{f}");
        }
        for n in [
            "vim",
            "pkg.tar",
            "a.pkg.tar.zst.sig",
            "foo.tar.gz",
            "lib32-pkg",
            "pkg.tar.zst.txt",
        ] {
            assert!(!is_package_file_name(n), "{n}");
        }
    }
}

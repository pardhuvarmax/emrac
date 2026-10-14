use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use std::cell::RefCell;
use std::rc::Rc;

use alpm::{Alpm, Question, SigLevel, TransFlag};

use crate::deps::{DepEntry, Orphan};
use crate::error::{Error, Result};
use crate::package::{PackageDetails, PackageSummary};
use crate::purge::{classify, ConfigFile, Observed};
use crate::snapshot::SnapshotPackage;
use crate::verify::{self, PackageReport};

/// Read-only access to the official repositories via libalpm, scoped to the
/// sync databases pacman has already downloaded locally (`/var/lib/pacman/sync`).
/// No network access, no root required.
pub struct AlpmBackend {
    handle: Alpm,
}

/// What `AlpmBackend::simulate` should prepare.
pub enum SimOp<'a> {
    /// `pacman -S <names>`: the named packages and their dependencies.
    Install(&'a [String]),
    /// `pacman -Su`, with these packages held back.
    Upgrade { ignore: &'a [String] },
}

/// A package a simulated transaction removes, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub name: String,
    pub version: String,
    pub installed_size: u64,
    pub reason: String,
}

impl AlpmBackend {
    pub fn init() -> Result<Self> {
        Self::init_with(None)
    }

    /// A handle on a different database directory — the freshly synced
    /// throwaway one (`FreshDb`) — instead of pacman's own.
    pub fn init_with_dbpath(db_path: &Path) -> Result<Self> {
        Self::init_with(Some(db_path.display().to_string()))
    }

    fn init_with(db_path_override: Option<String>) -> Result<Self> {
        Ok(Self {
            handle: Self::open_handle(db_path_override)?,
        })
    }

    /// A libalpm handle on pacman's configured root and sync repositories.
    fn open_handle(db_path_override: Option<String>) -> Result<Alpm> {
        let root = pacman_conf("RootDir")?;
        let db_path = match db_path_override {
            Some(path) => path,
            None => pacman_conf("DBPath")?,
        };
        let repos = pacman_conf_list("--repo-list")?;

        let handle = Alpm::new(root, db_path).map_err(Error::AlpmInit)?;

        for repo in repos {
            handle
                .register_syncdb(repo.clone(), SigLevel::USE_DEFAULT)
                .map_err(|source| Error::RegisterSyncDb { repo, source })?;
        }

        Ok(handle)
    }

    /// Prepares — never commits — the transaction pacman would run, and says
    /// what libalpm would remove besides what was asked for. This is pacman's
    /// own conflict and replacement logic, answering its two questions "yes"
    /// (what a user who accepts the transaction answers), so the plan can show
    /// the removals instead of leaving them to a prompt at execution time.
    ///
    /// `db_path` selects a different database directory (the freshly synced
    /// throwaway one); `None` is pacman's own. Needs no root: the transaction
    /// is initialised without taking the database lock and is released
    /// unchanged.
    pub fn simulate(db_path: Option<&Path>, op: SimOp) -> Result<Vec<Removal>> {
        let fail = |what: String| Error::Simulation(what);
        let mut handle = Self::open_handle(db_path.map(|p| p.display().to_string()))?;

        let decided: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
        handle.set_question_cb(Rc::clone(&decided), |question, decided| {
            match question.question() {
                Question::Conflict(mut q) => {
                    // "package1 and package2 are in conflict. Remove package2?"
                    let removed = q.conflict().package2().name().to_string();
                    let by = q.conflict().package1().name().to_string();
                    decided
                        .borrow_mut()
                        .push((removed, format!("conflicts with {by}")));
                    q.set_remove(true);
                }
                Question::Replace(q) => {
                    let old = q.oldpkg().name().to_string();
                    let new = q.newpkg().name().to_string();
                    decided
                        .borrow_mut()
                        .push((old, format!("replaced by {new}")));
                    q.set_replace(true);
                }
                _ => {}
            }
        });

        handle
            .trans_init(TransFlag::NO_LOCK)
            .map_err(|e| fail(e.to_string()))?;
        match op {
            SimOp::Install(names) => {
                for name in names {
                    let pkg = handle
                        .syncdbs()
                        .find_satisfier(name.as_str())
                        .ok_or_else(|| fail(format!("'{name}' isn't in the repositories")))?;
                    handle
                        .trans_add_pkg(pkg)
                        .map_err(|e| fail(e.error.to_string()))?;
                }
            }
            SimOp::Upgrade { ignore } => {
                for name in ignore {
                    handle
                        .add_ignorepkg(name.as_str())
                        .map_err(|e| fail(e.to_string()))?;
                }
                handle
                    .sync_sysupgrade(false)
                    .map_err(|e| fail(e.to_string()))?;
            }
        }
        handle.trans_prepare().map_err(|e| fail(e.to_string()))?;

        let reasons = decided.borrow();
        let removals: Vec<Removal> = handle
            .trans_remove()
            .iter()
            .map(|pkg| Removal {
                name: pkg.name().to_string(),
                version: pkg.version().to_string(),
                installed_size: pkg.isize().max(0) as u64,
                reason: reasons
                    .iter()
                    .find(|(n, _)| n == pkg.name())
                    .map(|(_, why)| why.clone())
                    .unwrap_or_else(|| "removed by the transaction".to_string()),
            })
            .collect();
        let _ = handle.trans_release();
        Ok(removals)
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

    /// The `backup=()` config files of the installed `names`, each checked
    /// against the hash recorded when it was installed.
    pub fn config_files(&self, names: &[String]) -> Vec<ConfigFile> {
        let root = self.handle.root().to_string();
        let local = self.handle.localdb();
        let mut files = Vec::new();
        for name in names {
            let Ok(pkg) = local.pkg(name.as_str()) else {
                continue;
            };
            for backup in pkg.backup() {
                let observed = observe(&root, backup.name());
                files.push(ConfigFile {
                    package: name.clone(),
                    path: Path::new(&root).join(backup.name()).display().to_string(),
                    state: classify(backup.hash(), &observed),
                });
            }
        }
        files
    }

    /// Checks an installed package's files against its mtree record.
    pub fn verify_installed(&self, name: &str, no_extract: &[String]) -> Result<PackageReport> {
        let pkg = self
            .handle
            .localdb()
            .pkg(name)
            .map_err(|_| Error::NotInstalledForVerify(name.to_string()))?;
        let version = pkg.version().to_string();
        let mtree = verify::read_mtree(&verify::mtree_path(self.handle.dbpath(), name, &version))?;
        let backup: Vec<String> = pkg.backup().iter().map(|b| b.name().to_string()).collect();
        let mut entries = verify::parse_mtree(&mtree);
        let installed: Vec<String> = pkg
            .files()
            .files()
            .iter()
            .map(|f| String::from_utf8_lossy(f.name()).into_owned())
            .collect();
        verify::retain_installed(&mut entries, &installed);
        verify::drop_no_extract(&mut entries, no_extract);
        Ok(verify::verify_entries(
            name,
            &version,
            Path::new(self.handle.root()),
            &entries,
            &backup,
        ))
    }

    /// The currently installed version of `name`, if it is installed.
    pub fn installed_version(&self, name: &str) -> Option<String> {
        self.handle
            .localdb()
            .pkg(name)
            .ok()
            .map(|pkg| pkg.version().to_string())
    }

    /// Dependencies of installed packages that nothing installed satisfies:
    /// `(package, dependency)`. What `pacman -Dk` reports.
    pub fn unsatisfied_dependencies(&self) -> Vec<(String, String)> {
        let local = self.handle.localdb();
        let mut out = Vec::new();
        for pkg in local.pkgs() {
            for dep in pkg.depends() {
                let spec = dep.to_string();
                if local.pkgs().find_satisfier(spec.as_str()).is_none() {
                    out.push((pkg.name().to_string(), spec));
                }
            }
        }
        out.sort();
        out
    }

    /// How many packages are installed, and how many of those are foreign.
    pub fn package_counts(&self) -> (usize, usize) {
        (
            self.handle.localdb().pkgs().len(),
            self.foreign_package_names().len(),
        )
    }

    /// Every installed package, as a snapshot records it.
    pub fn installed_snapshot(&self) -> Vec<SnapshotPackage> {
        let foreign: HashSet<String> = self.foreign_package_names().into_iter().collect();
        self.handle
            .localdb()
            .pkgs()
            .into_iter()
            .map(|pkg| SnapshotPackage {
                name: pkg.name().to_string(),
                version: pkg.version().to_string(),
                explicit: pkg.reason() == alpm::PackageReason::Explicit,
                foreign: foreign.contains(pkg.name()),
            })
            .collect()
    }

    /// Dependencies of `name`: the installed package if there is one, else
    /// the sync-db one. `None` if neither exists.
    pub fn dependencies(&self, name: &str) -> Option<(String, bool, Vec<DepEntry>, Vec<DepEntry>)> {
        let local = self.handle.localdb();
        let (pkg, installed) = match local.pkg(name) {
            Ok(pkg) => (pkg, true),
            Err(_) => (
                self.handle
                    .syncdbs()
                    .into_iter()
                    .find_map(|db| db.pkg(name).ok())?,
                false,
            ),
        };
        let entry = |dep: &alpm::Dep, with_desc: bool| DepEntry {
            spec: dep.to_string(),
            satisfied_by: local
                .pkgs()
                .find_satisfier(dep.to_string())
                .map(|p| (p.name().to_string(), p.version().to_string())),
            description: with_desc.then(|| dep.desc().unwrap_or_default().to_string()),
        };
        let depends = pkg.depends().iter().map(|d| entry(d, false)).collect();
        let optional = pkg.optdepends().iter().map(|d| entry(d, true)).collect();
        Some((pkg.version().to_string(), installed, depends, optional))
    }

    /// Names of the installed packages satisfying `name`'s required
    /// dependencies (duplicates removed, unsatisfied ones skipped).
    pub fn installed_dependency_names(&self, name: &str) -> Vec<String> {
        let local = self.handle.localdb();
        let Ok(pkg) = local.pkg(name) else {
            return Vec::new();
        };
        let mut names: Vec<String> = pkg
            .depends()
            .iter()
            .filter_map(|d| local.pkgs().find_satisfier(d.to_string()))
            .map(|p| p.name().to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn is_explicit(&self, name: &str) -> bool {
        self.handle
            .localdb()
            .pkg(name)
            .is_ok_and(|p| p.reason() == alpm::PackageReason::Explicit)
    }

    /// `(required_by, optional_for)` of an installed package.
    pub fn dependents(&self, name: &str) -> Option<(Vec<String>, Vec<String>)> {
        let pkg = self.handle.localdb().pkg(name).ok()?;
        let mut required: Vec<String> = pkg.required_by().iter().map(str::to_string).collect();
        let mut optional: Vec<String> = pkg.optional_for().iter().map(str::to_string).collect();
        required.sort();
        optional.sort();
        Some((required, optional))
    }

    /// Dependencies that nothing requires or optionally depends on.
    pub fn orphans(&self) -> Vec<Orphan> {
        let mut found: Vec<Orphan> = self
            .handle
            .localdb()
            .pkgs()
            .into_iter()
            .filter(|p| {
                p.reason() == alpm::PackageReason::Depend
                    && p.required_by().is_empty()
                    && p.optional_for().is_empty()
            })
            .map(|p| Orphan {
                name: p.name().to_string(),
                version: p.version().to_string(),
                installed_size: p.isize().max(0) as u64,
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
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

/// Looks at `rel` (a path relative to the root, as pacman records it).
pub(crate) fn observe(root: &str, rel: &str) -> Observed {
    let path = Path::new(root).join(rel);
    match path.try_exists() {
        Ok(false) => return Observed::Missing,
        Err(_) => return Observed::Unreadable,
        Ok(true) => {}
    }
    match alpm::compute_md5sum(path.display().to_string()) {
        Ok(hash) => Observed::Hash(hash),
        Err(_) => Observed::Unreadable,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::purge::ConfigState;
    use std::{env, fs};

    #[test]
    fn observe_and_classify_against_a_temp_root() {
        let root = env::temp_dir().join(format!("emrac-purge-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(root.join("etc/a.conf"), "hello\n").unwrap();
        let root_str = root.display().to_string();

        // md5("hello\n")
        let shipped = "b1946ac92492d2347c6235b4d2611184";
        let state = |rel: &str, hash: &str| classify(hash, &observe(&root_str, rel));
        assert_eq!(state("etc/a.conf", shipped), ConfigState::Unmodified);
        assert_eq!(
            state("etc/a.conf", "00000000000000000000000000000000"),
            ConfigState::Modified
        );
        assert_eq!(state("etc/gone.conf", shipped), ConfigState::Missing);

        fs::write(root.join("etc/a.conf"), "edited\n").unwrap();
        assert_eq!(state("etc/a.conf", shipped), ConfigState::Modified);
        let _ = fs::remove_dir_all(&root);
    }
}

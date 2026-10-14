use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use alpm::{Alpm, SigLevel};

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

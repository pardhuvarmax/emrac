use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::sandbox::{self, SandboxSpec};

/// How an AUR build is run.
#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    /// Run the build steps in bubblewrap (the default).
    pub sandbox: bool,
    /// Give the sandboxed build network access.
    pub allow_network: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            sandbox: true,
            allow_network: false,
        }
    }
}

fn failed(phase: &str, status: &std::process::ExitStatus, hint: &str) -> Error {
    Error::BuildFailed {
        phase: phase.to_string(),
        status: status.to_string(),
        hint: hint.to_string(),
    }
}

/// makepkg's exit status for "this package was already built".
const MAKEPKG_ALREADY_BUILT: i32 = 13;

fn l_is_empty(p: &Path) -> bool {
    p.as_os_str().is_empty()
}

/// What `AurBuildBackend::sync` found when checking a package's cached AUR
/// git checkout against upstream.
pub enum AurSyncOutcome {
    /// No local checkout existed yet; one was just cloned fresh.
    FirstClone,
    /// A checkout already existed and upstream has no new commits.
    UpToDate,
    /// A checkout already existed and upstream has moved on. The local
    /// checkout has *not* been advanced yet — call `advance` once the
    /// caller decides to proceed.
    Changed { diff: String, new_pkgbuild: String },
}

/// Builds AUR packages via `git` + `makepkg` — the only module that shells
/// out to either. Each package gets its own persistent checkout under
/// `~/.cache/emrac/build/<pkg>`, reused across runs so repeat builds only
/// need to review what changed rather than the whole PKGBUILD again.
pub struct AurBuildBackend;

impl AurBuildBackend {
    pub fn new() -> Self {
        Self
    }

    /// Clones the package fresh, or fetches and reports whether upstream has
    /// moved since the last sync. Never mutates an existing checkout.
    pub fn sync(&self, pkg: &str) -> Result<AurSyncOutcome> {
        let dir = self.build_dir(pkg)?;

        if !dir.exists() {
            if let Some(parent) = dir.parent() {
                fs::create_dir_all(parent).map_err(|source| Error::Io {
                    context: format!(
                        "couldn't create the build cache directory '{}'",
                        parent.display()
                    ),
                    source,
                })?;
            }

            let url = format!("https://aur.archlinux.org/{pkg}.git");
            self.git(None, &["clone", &url, &dir.to_string_lossy()])?;
            return Ok(AurSyncOutcome::FirstClone);
        }

        self.git(Some(&dir), &["fetch", "origin"])?;

        let head = self.git_output(Some(&dir), &["rev-parse", "HEAD"])?;
        let fetch_head = self.git_output(Some(&dir), &["rev-parse", "FETCH_HEAD"])?;

        if head == fetch_head {
            return Ok(AurSyncOutcome::UpToDate);
        }

        let diff = self.git_output(Some(&dir), &["diff", "HEAD", "FETCH_HEAD"])?;
        let new_pkgbuild = self.git_output(Some(&dir), &["show", "FETCH_HEAD:PKGBUILD"])?;
        Ok(AurSyncOutcome::Changed { diff, new_pkgbuild })
    }

    /// Fast-forwards the local checkout to what `sync` last fetched. Only
    /// call this after the caller has confirmed the `Changed` diff.
    pub fn advance(&self, pkg: &str) -> Result<()> {
        let dir = self.build_dir(pkg)?;
        self.git(Some(&dir), &["reset", "--hard", "FETCH_HEAD"])
    }

    /// Reads the PKGBUILD off the current checkout (used for the
    /// first-clone case, where it's already the version to review).
    pub fn read_pkgbuild(&self, pkg: &str) -> Result<String> {
        let path = self.build_dir(pkg)?.join("PKGBUILD");
        fs::read_to_string(&path).map_err(|source| Error::Io {
            context: format!("couldn't read the PKGBUILD at '{}'", path.display()),
            source,
        })
    }

    /// A file of the package's repo, from the working tree or (`rev`) from
    /// a git revision such as `FETCH_HEAD`. `None` if it isn't there.
    pub fn read_file(&self, pkg: &str, rev: Option<&str>, name: &str) -> Option<String> {
        let dir = self.build_dir(pkg).ok()?;
        match rev {
            None => fs::read_to_string(dir.join(name)).ok(),
            Some(rev) => self
                .git_output(Some(&dir), &["show", &format!("{rev}:{name}")])
                .ok(),
        }
    }

    /// Every file of the package's repo at `rev` (`HEAD` when `None`).
    pub fn list_files(&self, pkg: &str, rev: Option<&str>) -> Vec<String> {
        let Ok(dir) = self.build_dir(pkg) else {
            return Vec::new();
        };
        self.git_output(
            Some(&dir),
            &["ls-tree", "-r", "--name-only", rev.unwrap_or("HEAD")],
        )
        .map(|out| out.lines().map(str::to_string).collect())
        .unwrap_or_default()
    }

    /// A file's raw bytes at `rev` (`HEAD` when `None`).
    pub fn read_bytes(&self, pkg: &str, rev: Option<&str>, name: &str) -> Option<Vec<u8>> {
        let dir = self.build_dir(pkg).ok()?;
        let output = Command::new("git")
            .args(["show", &format!("{}:{name}", rev.unwrap_or("HEAD"))])
            .current_dir(&dir)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    }

    /// Builds `pkg` from its cached checkout. See `build_in`.
    pub fn build(&self, pkg: &str, opts: BuildOptions) -> Result<()> {
        self.build_in(&self.build_dir(pkg)?, opts)
    }

    /// Builds the PKGBUILD in `dir` **without installing the package** (emrac
    /// inspects it first). Stdio is inherited so build output and the sudo
    /// prompt reach the terminal. Never runs as root (makepkg refuses anyway).
    ///
    /// Unsandboxed this is `makepkg -s`. Sandboxed it is three steps:
    /// dependencies and verified sources on the host (the only part that needs
    /// `sudo` and the network), then extraction/`prepare()` and
    /// `build()`/`package()` inside bubblewrap with the network off unless
    /// `opts.allow_network`. makepkg's "already built" exit (13) is fine: the
    /// existing package file is used.
    pub fn build_in(&self, dir: &Path, opts: BuildOptions) -> Result<()> {
        if !opts.sandbox {
            let status = Command::new("makepkg")
                .args(["-s", "--noconfirm", "--needed"])
                .current_dir(dir)
                .status()
                .map_err(|err| Error::CommandSpawn("makepkg".to_string(), err))?;
            return match status.code() {
                Some(0) | Some(MAKEPKG_ALREADY_BUILT) => Ok(()),
                _ => Err(failed("build", &status, "")),
            };
        }

        sandbox::probe().map_err(Error::SandboxUnavailable)?;
        if self.built_files_in(dir).is_ok() {
            return Ok(());
        }

        // 1. Dependencies and verified sources, on the host.
        let status = Command::new("makepkg")
            .args(["-s", "--verifysource", "--noconfirm", "--needed"])
            .current_dir(dir)
            .status()
            .map_err(|err| Error::CommandSpawn("makepkg".to_string(), err))?;
        if !status.success() {
            return Err(failed("dependencies and sources", &status, ""));
        }

        // 2 and 3, sandboxed.
        let hint = if opts.allow_network {
            String::new()
        } else {
            ". The sandbox gives the build no network; if this package downloads dependencies while building, run again with `--allow-network`".to_string()
        };
        let spec = self.sandbox_spec(dir, opts.allow_network);
        let steps: [(&str, &[&str]); 2] = [
            (
                "extract and prepare",
                &["--nobuild", "--nodeps", "--noconfirm", "--cleanbuild"],
            ),
            (
                "build and package",
                &["--noextract", "--nodeps", "--noconfirm"],
            ),
        ];
        for (phase, args) in steps {
            let mut command = vec!["makepkg".to_string()];
            command.extend(args.iter().map(|a| a.to_string()));
            let status = sandbox::run(&spec, &command)
                .map_err(|err| Error::CommandSpawn("bwrap".to_string(), err))?;
            match status.code() {
                Some(0) => {}
                Some(MAKEPKG_ALREADY_BUILT) if phase == "build and package" => {}
                _ => return Err(failed(phase, &status, &hint)),
            }
        }
        Ok(())
    }

    fn sandbox_spec(&self, dir: &Path, network: bool) -> SandboxSpec {
        let host_env: Vec<(String, String)> = env::vars().collect();
        let home = env::var("HOME").map(PathBuf::from).unwrap_or_default();
        let mut extra_rw: Vec<PathBuf> = sandbox::makepkg_dirs();
        extra_rw.retain(|d| d != dir);
        SandboxSpec {
            dir: dir.to_path_buf(),
            extra_rw,
            home,
            network,
            hide_run_user: Path::new("/run/user").is_dir(),
            env: sandbox::clean_env(&host_env),
        }
    }

    /// The package files `build` produced (a split package makes several),
    /// per `makepkg --packagelist` — so a custom `PKGDEST` is honoured —
    /// keeping those that exist (the list also names an unbuilt `-debug`
    /// package).
    pub fn built_files(&self, pkg: &str) -> Result<Vec<PathBuf>> {
        self.built_files_in(&self.build_dir(pkg)?)
    }

    /// `built_files` for a PKGBUILD directory.
    pub fn built_files_in(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let output = Command::new("makepkg")
            .arg("--packagelist")
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("makepkg".to_string(), err))?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                command: format!("makepkg --packagelist (in {})", dir.display()),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let files: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| PathBuf::from(l.trim()))
            .filter(|p| !l_is_empty(p) && p.is_file())
            .collect();
        if files.is_empty() {
            return Err(Error::NoBuiltPackage(dir.display().to_string()));
        }
        Ok(files)
    }

    /// Whether emrac already has a git checkout of `pkg`'s AUR repo.
    pub fn has_checkout(&self, pkg: &str) -> bool {
        self.build_dir(pkg)
            .map(|dir| dir.join(".git").exists())
            .unwrap_or(false)
    }

    pub fn build_dir(&self, pkg: &str) -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoHomeDir)?;
        Ok(PathBuf::from(home).join(".cache/emrac/build").join(pkg))
    }

    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<()> {
        self.git_output(dir, args).map(|_| ())
    }

    fn git_output(&self, dir: Option<&Path>, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new("git");
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
        cmd.args(args).stdin(Stdio::null());

        let output = cmd
            .output()
            .map_err(|err| Error::CommandSpawn("git".to_string(), err))?;

        if !output.status.success() {
            return Err(Error::CommandFailed {
                command: format!("git {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Default for AurBuildBackend {
    fn default() -> Self {
        Self::new()
    }
}

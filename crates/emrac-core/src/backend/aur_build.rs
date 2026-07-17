use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

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

    /// Builds and installs via `makepkg -si`, with stdio inherited so the
    /// real build output, dependency-sync prompts, and sudo password prompt
    /// go straight to the terminal. Never runs as root — matches emrac's
    /// existing privilege model, and makepkg refuses root anyway.
    pub fn build_and_install(&self, pkg: &str) -> Result<()> {
        let dir = self.build_dir(pkg)?;
        let args = ["-si", "--noconfirm", "--needed"];

        let status = Command::new("makepkg")
            .args(args)
            .current_dir(&dir)
            .status()
            .map_err(|err| Error::CommandSpawn("makepkg".to_string(), err))?;

        if !status.success() {
            return Err(Error::CommandFailed {
                command: format!("makepkg {} (in {})", args.join(" "), dir.display()),
                stderr: format!("exited with {status}"),
            });
        }

        Ok(())
    }

    fn build_dir(&self, pkg: &str) -> Result<PathBuf> {
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

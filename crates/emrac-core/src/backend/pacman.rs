use std::process::{Command, Output, Stdio};

use crate::error::{Error, Result};

/// The only place in emrac that shells out to `pacman`/`sudo pacman`.
/// Resolution (`-S`/`-R --print`) needs no privileges and never mutates
/// anything; execution always goes through `sudo`, prompting interactively
/// when needed, so emrac itself never has to run as root.
pub struct PacmanBackend;

impl PacmanBackend {
    pub fn new() -> Self {
        Self
    }

    /// Resolved install target names (explicit packages + any new
    /// dependencies pacman would pull in), via `pacman -S --print`. No root.
    pub fn resolve_install(&self, pkgs: &[String]) -> Result<Vec<String>> {
        let mut args = vec!["-S".to_string()];
        args.extend(pkgs.iter().cloned());
        args.push("--print".to_string());
        args.push("--print-format".to_string());
        args.push("%n".to_string());

        let output = self.spawn_captured("pacman", &args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let missing = not_found_targets(&stderr);
            if !missing.is_empty() {
                return Err(not_found_in_official(missing));
            }
            return Err(pacman_failed("pacman", &args, &stderr));
        }

        Ok(parse_names(&output.stdout))
    }

    /// Resolved removal target names (explicit packages + cascade/recursive
    /// dependents/dependencies pacman would also remove), via
    /// `pacman -R --print`. No root.
    pub fn resolve_remove(
        &self,
        pkgs: &[String],
        cascade: bool,
        recursive: bool,
    ) -> Result<Vec<String>> {
        let mut args = vec!["-R".to_string()];
        if cascade {
            args.push("--cascade".to_string());
        }
        if recursive {
            args.push("--recursive".to_string());
        }
        args.extend(pkgs.iter().cloned());
        args.push("--print".to_string());
        args.push("--print-format".to_string());
        args.push("%n".to_string());

        let output = self.spawn_captured("pacman", &args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let missing = not_found_targets(&stderr);
            if !missing.is_empty() {
                return Err(not_installed(missing));
            }
            return Err(pacman_failed("pacman", &args, &stderr));
        }

        Ok(parse_names(&output.stdout))
    }

    /// Resolved full-upgrade target names, via `pacman -Su --print`
    /// (deliberately no `-y`/`--refresh`: verified live that pacman still
    /// enforces the root check for `-y` even under `--print`, unlike `-S
    /// <pkg> --print`, which `man pacman` suggested but which doesn't hold
    /// in practice for the refresh step specifically). No root, but works
    /// off whatever's already in the local sync db cache rather than a
    /// truly fresh one — same "may be a little stale" caveat previews
    /// already carry elsewhere. The real upgrade (`execute_full_upgrade`)
    /// still does a genuine `-Syu`.
    pub fn resolve_full_upgrade(&self) -> Result<Vec<String>> {
        let args = vec![
            "-Su".to_string(),
            "--print".to_string(),
            "--print-format".to_string(),
            "%n".to_string(),
        ];

        let output = self.spawn_captured("pacman", &args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(pacman_failed("pacman", &args, &stderr));
        }

        Ok(parse_names(&output.stdout))
    }

    /// Actually installs. Runs under `sudo`, with stdio inherited so
    /// pacman's real progress output goes straight to the terminal.
    pub fn execute_install(&self, pkgs: &[String]) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-S".to_string()];
        args.extend(pkgs.iter().cloned());
        args.push("--noconfirm".to_string());

        self.run_inherited("sudo", &args)
    }

    /// Actually syncs and upgrades everything. Runs under `sudo`, with
    /// stdio inherited — a real, single combined `-Syu` transaction, same
    /// as `execute_install` lets pacman own conflict/currency logic.
    pub fn execute_full_upgrade(&self) -> Result<()> {
        let args = vec![
            "pacman".to_string(),
            "-Syu".to_string(),
            "--noconfirm".to_string(),
        ];

        self.run_inherited("sudo", &args)
    }

    /// Actually removes. Runs under `sudo`, with stdio inherited.
    pub fn execute_remove(&self, pkgs: &[String], cascade: bool, recursive: bool) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-R".to_string()];
        if cascade {
            args.push("--cascade".to_string());
        }
        if recursive {
            args.push("--recursive".to_string());
        }
        args.extend(pkgs.iter().cloned());
        args.push("--noconfirm".to_string());

        self.run_inherited("sudo", &args)
    }

    fn spawn_captured(&self, program: &str, args: &[String]) -> Result<Output> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn(program.to_string(), err))
    }

    fn run_inherited(&self, program: &str, args: &[String]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|err| Error::CommandSpawn(program.to_string(), err))?;

        if !status.success() {
            return Err(Error::CommandFailed {
                command: format!("{program} {}", args.join(" ")),
                stderr: format!("exited with {status}"),
            });
        }

        Ok(())
    }
}

impl Default for PacmanBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_names(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Pulls package names out of pacman's `error: target not found: <name>`
/// lines, one per missing package, so callers can give a clean message
/// instead of relaying pacman's raw stderr.
fn not_found_targets(stderr: &str) -> Vec<String> {
    const PREFIX: &str = "error: target not found: ";
    stderr
        .lines()
        .filter_map(|line| line.trim().strip_prefix(PREFIX))
        .map(|name| name.trim().to_string())
        .collect()
}

fn not_found_in_official(mut missing: Vec<String>) -> Error {
    if missing.len() == 1 {
        Error::PackageNotFoundInOfficial(missing.remove(0))
    } else {
        Error::PackagesNotFoundInOfficial(missing.join(", "))
    }
}

fn not_installed(mut missing: Vec<String>) -> Error {
    if missing.len() == 1 {
        Error::PackageNotInstalled(missing.remove(0))
    } else {
        Error::PackagesNotInstalled(missing.join(", "))
    }
}

fn pacman_failed(program: &str, args: &[String], stderr: &str) -> Error {
    Error::CommandFailed {
        command: format!("{program} {}", args.join(" ")),
        stderr: stderr.trim().to_string(),
    }
}

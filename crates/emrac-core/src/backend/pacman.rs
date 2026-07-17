use std::process::{Command, Stdio};

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

        self.run_captured("pacman", &args)
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

        self.run_captured("pacman", &args)
    }

    /// Actually installs. Runs under `sudo`, with stdio inherited so
    /// pacman's real progress output goes straight to the terminal.
    pub fn execute_install(&self, pkgs: &[String]) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-S".to_string()];
        args.extend(pkgs.iter().cloned());
        args.push("--noconfirm".to_string());

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

    fn run_captured(&self, program: &str, args: &[String]) -> Result<Vec<String>> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::PacmanSpawn(program.to_string(), err))?;

        if !output.status.success() {
            return Err(Error::PacmanFailed {
                command: format!("{program} {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn run_inherited(&self, program: &str, args: &[String]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|err| Error::PacmanSpawn(program.to_string(), err))?;

        if !status.success() {
            return Err(Error::PacmanFailed {
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

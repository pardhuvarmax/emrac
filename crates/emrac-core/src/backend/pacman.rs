use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::error::{Error, Result};

/// The only place in emrac that shells out to `pacman`/`sudo pacman`.
/// Resolution (`-S`/`-R --print`) needs no privileges and never mutates
/// anything; execution always goes through `sudo`, prompting interactively
/// when needed, so emrac itself never has to run as root.
pub struct PacmanBackend {
    /// Let pacman ask its own questions (conflicts, provider choices,
    /// replacements, key imports) instead of taking `--noconfirm` defaults.
    interactive: bool,
}

impl PacmanBackend {
    /// Non-interactive, as `--yes` means: every question takes its default.
    pub fn new() -> Self {
        Self { interactive: false }
    }

    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
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
    pub fn resolve_full_upgrade(
        &self,
        ignore: &[String],
        dbpath: Option<&std::path::Path>,
    ) -> Result<Vec<String>> {
        let mut args = vec!["-Su".to_string()];
        args.extend(ignore_args(ignore));
        // A freshly synced throwaway database (`FreshDb`) instead of pacman's own.
        if let Some(path) = dbpath {
            args.extend(["--dbpath".to_string(), path.display().to_string()]);
        }
        args.extend([
            "--print".to_string(),
            "--print-format".to_string(),
            "%n".to_string(),
        ]);

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
        self.run_inherited("sudo", &self.install_args(pkgs))
    }

    fn install_args(&self, pkgs: &[String]) -> Vec<String> {
        let mut args = vec!["pacman".to_string(), "-S".to_string()];
        args.extend(pkgs.iter().cloned());
        self.finish(args)
    }

    /// Downloads `pkgs` and their dependencies into the cache without
    /// installing (`pacman -Sw`), so they can be inspected first.
    pub fn execute_download(&self, pkgs: &[String]) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-Sw".to_string()];
        args.extend(pkgs.iter().cloned());
        self.run_inherited("sudo", &self.finish(args))
    }

    /// Syncs the databases and downloads every pending upgrade into the cache
    /// without installing (`pacman -Syuw`).
    pub fn execute_download_upgrade(&self, ignore: &[String]) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-Syuw".to_string()];
        args.extend(ignore_args(ignore));
        self.run_inherited("sudo", &self.finish(args))
    }

    /// Actually syncs and upgrades everything. Runs under `sudo`, with
    /// stdio inherited — a real, single combined `-Syu` transaction, same
    /// as `execute_install` lets pacman own conflict/currency logic.
    pub fn execute_full_upgrade(&self, ignore: &[String]) -> Result<()> {
        self.run_inherited("sudo", &self.full_upgrade_args(ignore))
    }

    fn full_upgrade_args(&self, ignore: &[String]) -> Vec<String> {
        let mut args = vec!["pacman".to_string(), "-Syu".to_string()];
        args.extend(ignore_args(ignore));
        self.finish(args)
    }

    /// Actually removes. Runs under `sudo`, with stdio inherited. `nosave`
    /// is `pacman -Rn`: modified config files are deleted, not kept as
    /// `.pacsave`.
    pub fn execute_remove(
        &self,
        pkgs: &[String],
        cascade: bool,
        recursive: bool,
        nosave: bool,
    ) -> Result<()> {
        self.run_inherited("sudo", &self.remove_args(pkgs, cascade, recursive, nosave))
    }

    fn remove_args(
        &self,
        pkgs: &[String],
        cascade: bool,
        recursive: bool,
        nosave: bool,
    ) -> Vec<String> {
        let mut args = vec!["pacman".to_string(), "-R".to_string()];
        if nosave {
            // Delete modified config files instead of saving them as .pacsave.
            args.push("--nosave".to_string());
        }
        if cascade {
            args.push("--cascade".to_string());
        }
        if recursive {
            args.push("--recursive".to_string());
        }
        args.extend(pkgs.iter().cloned());
        self.finish(args)
    }

    /// Actually installs local package files (`pacman -U`) — how a rollback
    /// reinstalls a removed package or downgrades to an older version.
    /// Runs under `sudo`, with stdio inherited.
    pub fn execute_install_files(&self, files: &[PathBuf]) -> Result<()> {
        let mut args = vec!["pacman".to_string(), "-U".to_string()];
        args.extend(files.iter().map(|f| f.display().to_string()));
        self.run_inherited("sudo", &self.finish(args))
    }

    /// `--noconfirm` unless pacman has been told to ask: with it, pacman takes
    /// the default answer to every question (and the default for a package
    /// conflict is "no", so a conflicting install can only fail).
    fn finish(&self, mut args: Vec<String>) -> Vec<String> {
        if !self.interactive {
            args.push("--noconfirm".to_string());
        }
        args
    }

    /// pacman's `NoExtract` patterns (`pacman-conf NoExtract`).
    pub fn no_extract(&self) -> Result<Vec<String>> {
        let output = Command::new("pacman-conf")
            .arg("NoExtract")
            .output()
            .map_err(Error::PacmanConfSpawn)?;

        if !output.status.success() {
            return Err(Error::PacmanConfStatus(format!(
                "pacman-conf NoExtract failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(parse_names(&output.stdout))
    }

    /// pacman's database directory (`pacman-conf DBPath`).
    pub fn db_path(&self) -> Result<PathBuf> {
        let output = Command::new("pacman-conf")
            .arg("DBPath")
            .output()
            .map_err(Error::PacmanConfSpawn)?;
        if !output.status.success() {
            return Err(Error::PacmanConfStatus(format!(
                "pacman-conf DBPath failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        parse_names(&output.stdout)
            .into_iter()
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| Error::PacmanConfStatus("pacman-conf DBPath printed nothing".into()))
    }

    /// pacman's package cache directories (`pacman-conf CacheDir`).
    pub fn cache_dirs(&self) -> Result<Vec<PathBuf>> {
        let output = Command::new("pacman-conf")
            .arg("CacheDir")
            .output()
            .map_err(Error::PacmanConfSpawn)?;

        if !output.status.success() {
            return Err(Error::PacmanConfStatus(format!(
                "pacman-conf CacheDir failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(parse_names(&output.stdout)
            .into_iter()
            .map(PathBuf::from)
            .collect())
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

/// `--ignore a,b` (nothing for an empty list).
fn ignore_args(ignore: &[String]) -> Vec<String> {
    if ignore.is_empty() {
        Vec::new()
    } else {
        vec!["--ignore".to_string(), ignore.join(",")]
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

#[cfg(test)]
mod prompt_tests {
    use super::*;

    fn backend(interactive: bool) -> PacmanBackend {
        let mut b = PacmanBackend::new();
        b.set_interactive(interactive);
        b
    }

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn non_interactive_takes_noconfirm_and_interactive_lets_pacman_ask() {
        let pkgs = names(&["foo", "bar"]);
        for (interactive, expect) in [(false, true), (true, false)] {
            let b = backend(interactive);
            let all = [
                b.install_args(&pkgs),
                b.full_upgrade_args(&names(&["held"])),
                b.remove_args(&pkgs, true, false, true),
            ];
            for args in all {
                assert_eq!(
                    args.contains(&"--noconfirm".to_string()),
                    expect,
                    "interactive={interactive}: {args:?}"
                );
            }
        }
    }

    #[test]
    fn the_rest_of_each_command_is_identical_either_way() {
        let pkgs = names(&["foo"]);
        let strip = |mut a: Vec<String>| {
            a.retain(|x| x != "--noconfirm");
            a
        };
        assert_eq!(
            strip(backend(false).install_args(&pkgs)),
            backend(true).install_args(&pkgs)
        );
        assert_eq!(
            strip(backend(false).remove_args(&pkgs, true, true, true)),
            backend(true).remove_args(&pkgs, true, true, true)
        );
        assert_eq!(
            backend(true).full_upgrade_args(&names(&["a", "b"])),
            ["pacman", "-Syu", "--ignore", "a,b"]
        );
    }

    #[test]
    fn the_default_is_the_old_non_interactive_behaviour() {
        assert!(PacmanBackend::new()
            .install_args(&names(&["x"]))
            .contains(&"--noconfirm".to_string()));
    }
}

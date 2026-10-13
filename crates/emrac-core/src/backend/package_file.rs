use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Reads built package files (`*.pkg.tar.*`) with `bsdtar` — libarchive, which
/// pacman already depends on. Reading only: nothing is extracted to disk or
/// executed.
pub struct PackageFile;

impl PackageFile {
    /// `bsdtar -tvf`: every member with mode, owner and size.
    pub fn list(path: &Path) -> Result<String> {
        let output = Command::new("bsdtar")
            .arg("-tvf")
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("bsdtar".to_string(), err))?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                command: format!("bsdtar -tvf {}", path.display()),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// One member's contents as text, or `None` if it isn't there or can't
    /// be read.
    pub fn read(path: &Path, member: &str) -> Option<String> {
        let output = Command::new("bsdtar")
            .arg("-xOf")
            .arg(path)
            .arg(member)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

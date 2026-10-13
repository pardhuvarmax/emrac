use std::fs;
use std::path::{Component, Path, PathBuf};
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

/// Selected members of a package, extracted once into a private temporary
/// directory so many small files can be read without decompressing the
/// archive for each. Nothing in it is executed; it is removed on drop.
pub struct Extracted {
    dir: PathBuf,
}

impl Extracted {
    /// Extracts `members` (exact names; anything with a glob character or a
    /// `..` component is skipped). Ownership and special mode bits are not
    /// restored.
    pub fn new(package: &Path, members: &[String]) -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("emrac-pkg-{}-{nonce}", std::process::id()));
        let shown = dir.display().to_string();
        let io_err = |context: &str, source| Error::Io {
            context: format!("{context} '{shown}'"),
            source,
        };
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(|e| {
                    io_err(
                        "couldn't create a private directory to read the package in",
                        e,
                    )
                })?;
        }
        let this = Self { dir };

        let safe: Vec<&String> = members
            .iter()
            .filter(|m| {
                !m.contains(['*', '?', '[', '\\', '\n'])
                    && Path::new(m)
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
            })
            .collect();
        if safe.is_empty() {
            return Ok(this);
        }
        let list = this.dir.join(".members");
        let text: String = safe.iter().map(|m| format!("{m}\n")).collect();
        fs::write(&list, text).map_err(|e| io_err("couldn't write the member list in", e))?;

        let output = Command::new("bsdtar")
            .arg("-xf")
            .arg(package)
            .arg("-C")
            .arg(&this.dir)
            .args(["--no-same-owner", "--no-same-permissions", "-T"])
            .arg(&list)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| Error::CommandSpawn("bsdtar".to_string(), err))?;
        let _ = fs::remove_file(&list);
        if !output.status.success() {
            return Err(Error::CommandFailed {
                command: format!("bsdtar -xf {}", package.display()),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(this)
    }

    /// An extracted member's text (lossy), or `None` if it wasn't extracted
    /// or isn't a regular file.
    pub fn read(&self, member: &str) -> Option<String> {
        let path = self.dir.join(member);
        let meta = fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        fs::read(&path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }
}

impl Drop for Extracted {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

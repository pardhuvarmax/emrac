use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to run pacman-conf: {0}")]
    PacmanConfSpawn(#[source] io::Error),

    #[error("pacman-conf exited with a non-zero status: {0}")]
    PacmanConfStatus(String),

    #[error("failed to initialize libalpm handle: {0}")]
    AlpmInit(#[source] alpm::Error),

    #[error("failed to register sync database '{repo}': {source}")]
    RegisterSyncDb {
        repo: String,
        #[source]
        source: alpm::Error,
    },

    #[error("package '{0}' not found")]
    PackageNotFound(String),

    #[error("package '{0}' not found in official repos or the AUR")]
    PackageNotFoundAnywhere(String),

    #[error("package '{0}' not found in official repos (AUR not checked: offline)")]
    PackageNotFoundOffline(String),

    #[error("AUR request failed: {0}")]
    Aur(String),
}

pub type Result<T> = std::result::Result<T, Error>;

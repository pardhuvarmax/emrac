use std::io;

/// Every message emrac shows the user is voiced as "emrac <verb>: ...".
/// The verb is chosen per error to match what actually happened:
/// - `says` — something genuinely went wrong (infrastructure/spawn/IO failures)
/// - `found` — a lookup completed and this is its outcome (including "not found")
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("emrac says: failed to run pacman-conf: {0}")]
    PacmanConfSpawn(#[source] io::Error),

    #[error("emrac says: pacman-conf exited with a non-zero status: {0}")]
    PacmanConfStatus(String),

    #[error("emrac says: failed to initialize libalpm handle: {0}")]
    AlpmInit(#[source] alpm::Error),

    #[error("emrac says: failed to register sync database '{repo}': {source}")]
    RegisterSyncDb {
        repo: String,
        #[source]
        source: alpm::Error,
    },

    #[error("emrac found: package '{0}' not found")]
    PackageNotFound(String),

    #[error("emrac found: package '{0}' not found in official repos or the AUR")]
    PackageNotFoundAnywhere(String),

    #[error("emrac found: package '{0}' not found in official repos (AUR not checked: offline)")]
    PackageNotFoundOffline(String),

    #[error("emrac says: AUR request failed: {0}")]
    Aur(String),

    #[error("emrac says: failed to run {0}: {1}")]
    PacmanSpawn(String, #[source] io::Error),

    #[error("emrac says: {command} failed: {stderr}")]
    PacmanFailed { command: String, stderr: String },

    #[error(
        "emrac found: package '{0}' not found in official repositories — try `emrac search {0} --aur` to check the AUR"
    )]
    PackageNotFoundInOfficial(String),

    #[error(
        "emrac found: packages not found in official repositories: {0} — try `emrac search <name> --aur` to check the AUR"
    )]
    PackagesNotFoundInOfficial(String),

    #[error("emrac found: package '{0}' is not installed")]
    PackageNotInstalled(String),

    #[error("emrac found: packages not installed: {0}")]
    PackagesNotInstalled(String),
}

pub type Result<T> = std::result::Result<T, Error>;

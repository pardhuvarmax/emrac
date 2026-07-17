use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't run `pacman-conf` to read your repository configuration: {0}")]
    PacmanConfSpawn(#[source] io::Error),

    #[error("`pacman-conf` didn't run cleanly: {0}")]
    PacmanConfStatus(String),

    #[error("couldn't set up access to your local package database: {0}")]
    AlpmInit(#[source] alpm::Error),

    #[error("couldn't read the '{repo}' repository — is it enabled in pacman.conf? ({source})")]
    RegisterSyncDb {
        repo: String,
        #[source]
        source: alpm::Error,
    },

    #[error("no package named '{0}' in the official repositories")]
    PackageNotFound(String),

    #[error("couldn't find '{0}' anywhere — not in the official repositories, not in the AUR")]
    PackageNotFoundAnywhere(String),

    #[error(
        "couldn't find '{0}' in the official repositories, and I skipped the AUR since you're offline"
    )]
    PackageNotFoundOffline(String),

    #[error("the AUR didn't cooperate: {0}")]
    Aur(String),

    #[error("couldn't run `{0}`: {1}")]
    CommandSpawn(String, #[source] io::Error),

    #[error("something went wrong running `{command}`: {stderr}")]
    CommandFailed { command: String, stderr: String },

    #[error(
        "couldn't find '{0}' in the official repositories — want to try `emrac search {0} --aur` to check the AUR?"
    )]
    PackageNotFoundInOfficial(String),

    #[error(
        "couldn't find these in the official repositories: {0} — try `emrac search <name> --aur` to check the AUR"
    )]
    PackagesNotFoundInOfficial(String),

    #[error("'{0}' isn't installed, so there's nothing to remove")]
    PackageNotInstalled(String),

    #[error("none of these are installed, so there's nothing to remove: {0}")]
    PackagesNotInstalled(String),

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },

    #[error(
        "couldn't determine your home directory (`$HOME` isn't set) — needed to cache AUR builds under `~/.cache/emrac/build`"
    )]
    NoHomeDir,

    #[error("'{0}' isn't installed, so there's nothing to upgrade — did you mean `emrac install {0}`?")]
    PackageNotInstalledForUpgrade(String),
}

impl Error {
    /// Which conversational voice the CLI should use to introduce this
    /// error — see `emrac-cli`'s `main.rs`. Kept here rather than
    /// duplicated at the presentation layer, since it's a property of what
    /// the error *is*, not how it's displayed.
    pub fn voice(&self) -> &'static str {
        match self {
            Error::PackageNotFound(_)
            | Error::PackageNotFoundAnywhere(_)
            | Error::PackageNotFoundOffline(_)
            | Error::PackageNotFoundInOfficial(_)
            | Error::PackagesNotFoundInOfficial(_)
            | Error::PackageNotInstalled(_)
            | Error::PackagesNotInstalled(_)
            | Error::PackageNotInstalledForUpgrade(_) => "found",
            Error::PacmanConfSpawn(_)
            | Error::PacmanConfStatus(_)
            | Error::AlpmInit(_)
            | Error::RegisterSyncDb { .. }
            | Error::Aur(_)
            | Error::CommandSpawn(_, _)
            | Error::CommandFailed { .. }
            | Error::Io { .. }
            | Error::NoHomeDir => "says",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

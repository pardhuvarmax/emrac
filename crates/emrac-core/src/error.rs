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

    #[error("the Arch Linux Archive didn't cooperate: {0}")]
    Archive(String),

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

    #[error(
        "'{0}' isn't installed, so there's nothing to upgrade — did you mean `emrac install {0}`?"
    )]
    PackageNotInstalledForUpgrade(String),

    #[error(
        "couldn't determine your home directory (`$HOME` isn't set) — needed to locate ~/.config/emrac"
    )]
    NoConfigDir,

    #[error("your config at {path} doesn't parse: {source}")]
    ConfigParse {
        path: String,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("couldn't write your config to {path}: {source}")]
    ConfigWrite {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error(
        "couldn't determine your home directory (`$HOME` isn't set) — needed to locate ~/.local/share/emrac"
    )]
    NoDataDir,

    #[error("couldn't write to your transaction history at {path}: {source}")]
    HistoryWrite {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("'{0}' isn't a date I understand — use YYYY-MM-DD, like 2026-10-11")]
    InvalidDate(String),

    #[error("'{0}' isn't a transaction id — they look like tx-12 (see `emrac history`)")]
    InvalidTransactionId(String),

    #[error("there's no transaction {0} in your history")]
    UnknownTransaction(String),

    #[error("there's nothing in your history to roll back")]
    NoTransactionToRollBack,

    #[error("'{0}' isn't installed, so there's nothing to downgrade — did you mean `emrac install {0}`?")]
    PackageNotInstalledForDowngrade(String),

    #[error(
        "{pkg} {requested} isn't older than the {installed} you have — downgrade only goes back (use `emrac upgrade` or `emrac install` to go forward)"
    )]
    NotOlder {
        pkg: String,
        installed: String,
        requested: String,
    },

    #[error(
        "I can't find an older version of '{0}' to go back to — try `emrac downgrade {0} --list`"
    )]
    NoOlderVersion(String),

    #[error(
        "{pkg} {version} isn't on this machine and isn't available from the Arch Linux Archive — try `emrac downgrade {pkg} --list`"
    )]
    VersionUnavailable { pkg: String, version: String },

    #[error("not installed, so there's nothing to reinstall: {0} — did you mean `emrac install`?")]
    NotInstalledForReinstall(String),

    #[error("can't reinstall: {0}")]
    CannotReinstall(String),

    #[error("not installed, so there's nothing to verify: {0}")]
    NotInstalledForVerify(String),

    #[error(
        "{count} {noun} {verb} problems — `emrac reinstall {names}` puts the files back",
        noun = if *.count == 1 { "package" } else { "packages" },
        verb = if *.count == 1 { "has" } else { "have" }
    )]
    VerificationFailed { count: usize, names: String },

    #[error("'{0}' isn't a valid snapshot name — use letters, digits, `.`, `_` and `-` (up to 64, not starting with a dot)")]
    InvalidSnapshotName(String),

    #[error("a snapshot named '{0}' already exists")]
    SnapshotExists(String),

    #[error("there's no snapshot named '{0}' — `emrac snapshot list` shows what exists")]
    UnknownSnapshot(String),

    #[error("the snapshot '{name}' can't be read: {why}")]
    SnapshotCorrupt { name: String, why: String },

    #[error("couldn't write the snapshot at {path}: {source}")]
    SnapshotWrite {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("can't restore: no package file for {0} — not cached, and not in the Arch Linux Archive (or --offline)")]
    RestoreFilesMissing(String),

    #[error("there's no rollback in your history to redo")]
    NoTransactionToRedo,

    #[error("can't roll back {id}: {why}")]
    NotRollbackable { id: String, why: String },

    #[error(
        "can't roll back — these package files aren't on this machine (pacman cache, AUR build cache) and aren't available from the Arch Linux Archive: {0}"
    )]
    RollbackFilesMissing(String),

    #[error(
        "'{0}' isn't a config key emrac knows about — see `emrac config show` for the full list"
    )]
    UnknownConfigKey(String),

    #[error("'{value}' isn't a valid value for '{key}'")]
    InvalidConfigValue { key: String, value: String },
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
            | Error::PackageNotInstalledForUpgrade(_)
            | Error::UnknownConfigKey(_)
            | Error::InvalidConfigValue { .. }
            | Error::InvalidDate(_)
            | Error::InvalidTransactionId(_)
            | Error::UnknownTransaction(_)
            | Error::NoTransactionToRollBack
            | Error::NotInstalledForVerify(_)
            | Error::VerificationFailed { .. }
            | Error::NotInstalledForReinstall(_)
            | Error::CannotReinstall(_)
            | Error::PackageNotInstalledForDowngrade(_)
            | Error::NotOlder { .. }
            | Error::NoOlderVersion(_)
            | Error::VersionUnavailable { .. }
            | Error::InvalidSnapshotName(_)
            | Error::SnapshotExists(_)
            | Error::UnknownSnapshot(_)
            | Error::SnapshotCorrupt { .. }
            | Error::RestoreFilesMissing(_)
            | Error::NoTransactionToRedo
            | Error::NotRollbackable { .. }
            | Error::RollbackFilesMissing(_) => "found",
            Error::PacmanConfSpawn(_)
            | Error::PacmanConfStatus(_)
            | Error::AlpmInit(_)
            | Error::RegisterSyncDb { .. }
            | Error::Aur(_)
            | Error::Archive(_)
            | Error::CommandSpawn(_, _)
            | Error::CommandFailed { .. }
            | Error::Io { .. }
            | Error::NoHomeDir
            | Error::NoConfigDir
            | Error::ConfigParse { .. }
            | Error::ConfigWrite { .. }
            | Error::NoDataDir
            | Error::SnapshotWrite { .. }
            | Error::HistoryWrite { .. } => "says",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

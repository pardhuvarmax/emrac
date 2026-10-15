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

    /// A refusal or finding about the history log, already worded for the user.
    #[error("{0}")]
    History(String),

    /// A refusal or failure about the transaction journal, already worded.
    #[error("{0}")]
    Journal(String),

    /// A refusal or failure of `emrac self ...`, already worded.
    #[error("{0}")]
    SelfService(String),

    /// The pre-flight checks stopped a transaction (u17); the text lists why.
    #[error("pre-flight stopped this: {0}")]
    Preflight(String),

    /// The plan the user approved is not the plan that would run now (u18).
    #[error("{0}")]
    PlanChanged(String),

    /// `emrac rebuild` cannot rebuild what was named; the text says why.
    #[error("{0}")]
    Rebuild(String),

    /// The Arch news could not be read, or a critical item stops an upgrade.
    #[error("{0}")]
    News(String),

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

    #[error(
        "'{0}' isn't installed and isn't in the repositories, so it has no dependencies to show"
    )]
    NoSuchPackage(String),

    #[error("'{0}' isn't installed")]
    NotInstalled(String),

    #[error("'{0}' isn't pinned")]
    NotPinned(String),

    #[error("'{name}' is pinned{detail} — `emrac unpin {name}` first if you want to upgrade it")]
    PackagePinned { name: String, detail: String },

    #[error("your pins at {path} can't be read: {why}")]
    PinsCorrupt { path: String, why: String },

    #[error("couldn't write your pins to {path}: {source}")]
    PinWrite {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("won't build '{package}': the audit found critical problems — {findings}. Read the PKGBUILD yourself with `emrac audit {package}`; emrac won't build it")]
    AuditBlocked { package: String, findings: String },

    #[error("won't install '{package}': the built package has critical problems — {findings}. The built file is kept at {file}; emrac won't install it")]
    ManifestBlocked {
        package: String,
        findings: String,
        file: String,
    },

    #[error("didn't install '{package}': you declined after the package manifest. The built file is kept at {file} (`sudo pacman -U` installs it)")]
    InstallDeclined { package: String, file: String },

    #[error("can't build in a sandbox: {0}. Install it, or build unsandboxed with `--no-sandbox` (the build then runs with your full access)")]
    SandboxUnavailable(String),

    #[error("the build failed in the {phase} step ({status}){hint}")]
    BuildFailed {
        phase: String,
        status: String,
        hint: String,
    },

    #[error("makepkg finished but left no package file for '{0}'")]
    NoBuiltPackage(String),

    #[error("couldn't work out what the transaction would remove: {0}")]
    Simulation(String),

    #[error("'{value}' isn't a {what} emrac understands: {hint}")]
    InvalidCacheValue {
        what: &'static str,
        value: String,
        hint: &'static str,
    },

    #[error("`cache purge` with no scope removes every cache; add --force to say that is what you mean, or name a scope (source, build, binary, package)")]
    CacheNeedsForce,

    #[error("won't put the new {file} in place: {message}")]
    PacnewInvalid { file: String, message: String },

    #[error("{file} was changed after emrac last changed it, so reverting would overwrite those edits; type its path to revert anyway")]
    PacnewDrift { file: String },

    #[error("there is no change of emrac's to revert{0}")]
    PacnewNothingToRevert(String),

    #[error("{0} still has merge conflict markers (<<<<<<<, |||||||, >>>>>>>); resolve every one")]
    PacnewMarkers(String),

    #[error("{0}")]
    DependencyConflict(Box<crate::backend::DependencyConflict>),

    #[error(
        "'{package}' was not built: you declined the swap. Run it yourself, then upgrade again"
    )]
    DependencySwapDeclined { package: String },

    #[error("'{removed}' was removed but '{by}' did not install ({cause}). Install one of them before restarting anything that uses the library: `sudo pacman -S {by}`")]
    DependencySwapIncomplete {
        removed: String,
        by: String,
        cause: String,
    },

    #[error("couldn't refresh the package list: {0}")]
    FreshSync(String),

    #[error("{0} in the report above — fix them in the order listed")]
    DoctorProblems(String),

    #[error("high-risk or critical findings in: {0}")]
    AuditFlagged(String),

    #[error("can't read a PKGBUILD at '{0}'")]
    NoPkgbuild(String),

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

/// Marks a sandboxed build failure that may only be the missing network.
pub const NETWORK_HINT: &str = "run again with `--allow-network`";

impl Error {
    /// A sandboxed build failed with the network off, so retrying with it on
    /// might help.
    pub fn may_need_network(&self) -> bool {
        matches!(self, Error::BuildFailed { hint, .. } if hint.contains(NETWORK_HINT))
    }

    /// Which conversational voice the CLI should use to introduce this
    /// error — see `emrac-cli`'s `main.rs`. Kept here rather than
    /// duplicated at the presentation layer, since it's a property of what
    /// the error *is*, not how it's displayed.
    /// A failed build whose dependency step is the one that stopped.
    pub fn stopped_on_dependencies(&self) -> bool {
        matches!(self, Error::BuildFailed { phase, .. } if phase == "dependencies and sources")
    }

    pub fn voice(&self) -> crate::voice::Voice {
        match self {
            // The explanation carries a verdict ("the swap looks safe"): a judgment.
            Error::DependencyConflict(_) => crate::voice::Voice::Thinks,
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
            | Error::History(_)
            | Error::Journal(_)
            | Error::SelfService(_)
            | Error::Preflight(_)
            | Error::PlanChanged(_)
            | Error::Rebuild(_)
            | Error::News(_)
            | Error::NoTransactionToRollBack
            | Error::NotInstalledForVerify(_)
            | Error::VerificationFailed { .. }
            | Error::NotInstalledForReinstall(_)
            | Error::CannotReinstall(_)
            | Error::PackageNotInstalledForDowngrade(_)
            | Error::NotOlder { .. }
            | Error::NoOlderVersion(_)
            | Error::VersionUnavailable { .. }
            | Error::AuditBlocked { .. }
            | Error::NoPkgbuild(_)
            | Error::AuditFlagged(_)
            | Error::DoctorProblems(_)
            | Error::FreshSync(_)
            | Error::Simulation(_)
            | Error::InvalidCacheValue { .. }
            | Error::CacheNeedsForce
            | Error::PacnewInvalid { .. }
            | Error::PacnewDrift { .. }
            | Error::PacnewNothingToRevert(_)
            | Error::PacnewMarkers(_)
            | Error::DependencySwapDeclined { .. }
            | Error::DependencySwapIncomplete { .. }
            | Error::ManifestBlocked { .. }
            | Error::InstallDeclined { .. }
            | Error::NoBuiltPackage(_)
            | Error::SandboxUnavailable(_)
            | Error::BuildFailed { .. }
            | Error::NotPinned(_)
            | Error::PackagePinned { .. }
            | Error::PinsCorrupt { .. }
            | Error::NoSuchPackage(_)
            | Error::NotInstalled(_)
            | Error::InvalidSnapshotName(_)
            | Error::SnapshotExists(_)
            | Error::UnknownSnapshot(_)
            | Error::SnapshotCorrupt { .. }
            | Error::RestoreFilesMissing(_)
            | Error::NoTransactionToRedo
            | Error::NotRollbackable { .. }
            | Error::RollbackFilesMissing(_) => crate::voice::Voice::Found,
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
            | Error::PinWrite { .. }
            | Error::SnapshotWrite { .. }
            | Error::HistoryWrite { .. } => crate::voice::Voice::Says,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod network_hint_tests {
    use super::*;

    fn failure(hint: &str) -> Error {
        Error::BuildFailed {
            phase: "build and package".into(),
            status: "exit status: 4".into(),
            hint: hint.into(),
        }
    }

    #[test]
    fn only_the_no_network_failure_suggests_a_retry_with_network() {
        assert!(failure(&format!(". blocked, {NETWORK_HINT}")).may_need_network());
        assert!(!failure(". raise `build.timeout_minutes`").may_need_network());
        assert!(!failure("").may_need_network());
        assert!(!Error::NoBuiltPackage("x".into()).may_need_network());
    }
}

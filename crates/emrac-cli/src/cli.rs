use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "emrac",
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\nbuilt by ",
        env!("CARGO_PKG_AUTHORS"),
        "\n",
        env!("CARGO_PKG_HOMEPAGE")
    ),
    about = "A hybrid package management platform for Arch Linux",
    after_help = concat!("Built by ", env!("CARGO_PKG_AUTHORS"), " — ", env!("CARGO_PKG_HOMEPAGE"))
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Emit machine-readable JSON instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

    /// Increase verbosity
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Don't touch the network — skip the AUR
    #[arg(long, global = true)]
    pub offline: bool,

    /// Automatically answer yes to confirmation prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Show what would happen without doing it
    #[arg(short = 'n', long, global = true)]
    pub dry_run: bool,
}

/// Shared by `install` and `upgrade` — anything that builds an AUR package
/// goes through the same PKGBUILD/diff review, so the opt-out flags are
/// identical for both.
#[derive(Args)]
pub struct AurReviewFlags {
    /// Never show an AUR package's PKGBUILD/diff, or ask the extra review
    /// confirmation — just go straight to the overall confirm
    #[arg(long)]
    pub skip_pkgbuild: bool,

    /// On a rebuild, show the full new PKGBUILD instead of a diff against
    /// what was reviewed last time
    #[arg(long)]
    pub skip_diff: bool,

    /// Don't stop to ask when the audit finds high-risk problems in a
    /// PKGBUILD (critical ones are always refused)
    #[arg(long)]
    pub accept_risks: bool,

    /// Download official packages first and show what they would change
    /// before installing (only critical findings stop the install)
    #[arg(long)]
    pub inspect: bool,

    /// Build AUR packages without the sandbox (they then run with your
    /// full access)
    #[arg(long)]
    pub no_sandbox: bool,

    /// Let the sandboxed build use the network, without asking
    #[arg(long)]
    pub allow_network: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search official repositories and the AUR by package name or description
    Search {
        query: String,

        /// Limit the number of results shown
        #[arg(long)]
        limit: Option<usize>,

        /// Only search official repositories
        #[arg(long)]
        official: bool,

        /// Only search the AUR
        #[arg(long, conflicts_with = "offline")]
        aur: bool,
    },

    /// Show detailed metadata for a package, official repos or the AUR
    Info { pkg: String },

    /// Install one or more packages from official repositories or the AUR
    Install {
        #[arg(required = true)]
        pkgs: Vec<String>,

        #[command(flatten)]
        review: AurReviewFlags,
    },

    /// Remove one or more installed packages
    Remove {
        #[arg(required = true)]
        pkgs: Vec<String>,

        /// Also remove packages that depend on the given packages
        #[arg(long)]
        cascade: bool,

        /// Also remove dependencies that become orphaned as a result
        #[arg(long)]
        recursive: bool,
    },

    /// Remove packages and delete their config files instead of keeping
    /// modified ones as .pacsave
    Purge {
        #[arg(required = true)]
        pkgs: Vec<String>,

        /// Also remove packages that depend on the given packages
        #[arg(long)]
        cascade: bool,

        /// Also remove dependencies that become orphaned as a result
        #[arg(long)]
        recursive: bool,
    },

    /// Upgrade installed packages, official repos and the AUR alike
    Upgrade {
        /// Packages to upgrade; omitted = everything that's out of date
        pkgs: Vec<String>,

        #[command(flatten)]
        review: AurReviewFlags,
    },

    /// View or change persisted defaults (~/.config/emrac/config.toml)
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// See, clean, trim and purge what emrac and pacman keep on disk
    /// (no subcommand: list)
    Cache {
        #[command(subcommand)]
        command: Option<CacheCommand>,
    },

    /// Shortcut for `cache clean`
    #[command(name = "cc")]
    CacheClean(CacheArgs),

    /// Shortcut for `cache stats`
    #[command(name = "cs")]
    CacheStats(CacheScopes),

    /// Find, explain and resolve .pacnew / .pacsave files (no subcommand: list them)
    Pacnew {
        /// Also fetch old package versions missing from the local caches
        /// from the Arch Linux Archive, to tell your edits from upstream's
        #[arg(long)]
        fetch: bool,

        #[command(subcommand)]
        command: Option<PacnewCommand>,
    },

    /// Hold installed packages back from `upgrade` (no packages: list pins)
    Pin {
        pkgs: Vec<String>,

        /// Why it is held (shown whenever the hold applies)
        #[arg(long)]
        reason: Option<String>,

        /// The hold ends on this date, YYYY-MM-DD (UTC)
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
    },

    /// Let held packages upgrade again
    Unpin {
        #[arg(required = true)]
        pkgs: Vec<String>,
    },

    /// Read a PKGBUILD for risky code without running it (AUR package or path)
    Audit {
        #[arg(required = true)]
        targets: Vec<String>,
    },

    /// Check this system for common problems (read-only)
    Doctor {
        /// Print a markdown report for a bug report, with your home
        /// directory, user name and host name scrubbed out
        #[arg(long)]
        report: bool,

        /// Check for pending upgrades against freshly synced repositories
        /// (uses the network; otherwise the age of the local list is reported)
        #[arg(long)]
        online: bool,
    },

    /// Show a package's dependencies
    #[command(visible_alias = "depends")]
    Deps {
        pkg: String,

        /// The whole transitive closure of required dependencies
        #[arg(short, long)]
        recursive: bool,
    },

    /// Show which installed packages need a package
    #[command(visible_alias = "required-by")]
    Rdeps { pkg: String },

    /// Explain why an installed package is on this system
    Why { pkg: String },

    /// List dependencies nothing needs any more
    Orphans,

    /// Save, list, show or delete named restore points of the installed
    /// package set (no subcommand: save a new one)
    Snapshot {
        #[command(subcommand)]
        command: Option<SnapshotCommand>,
    },

    /// Make the installed packages match a snapshot
    Restore {
        name: String,

        /// Also remove installed packages that are not in the snapshot
        #[arg(long)]
        prune: bool,
    },

    /// Check that installed packages' files still match pacman's record
    #[command(visible_alias = "check")]
    Verify {
        #[arg(required = true)]
        pkgs: Vec<String>,
    },

    /// Put the installed version of one or more packages back (repair)
    Reinstall {
        #[arg(required = true)]
        pkgs: Vec<String>,

        /// Don't stop to ask when a rebuilt AUR package's manifest shows
        /// high-risk changes (critical ones are always refused)
        #[arg(long)]
        accept_risks: bool,

        /// Rebuild AUR packages without the sandbox
        #[arg(long)]
        no_sandbox: bool,

        /// Let the sandboxed rebuild use the network, without asking
        #[arg(long)]
        allow_network: bool,
    },

    /// Build AUR packages or a local PKGBUILD directory in the sandbox and
    /// show what the result would change, without installing anything
    Build {
        /// AUR package names, or directories containing a PKGBUILD
        #[arg(required = true)]
        targets: Vec<String>,

        #[command(flatten)]
        review: AurReviewFlags,
    },

    /// Go back to an older version of an installed package
    Downgrade {
        pkg: String,

        /// Version to go back to; omitted = the newest version older than
        /// the one installed
        version: Option<String>,

        /// List the versions available instead of downgrading
        #[arg(long, conflicts_with = "version")]
        list: bool,
    },

    /// Show past install/remove/upgrade transactions, newest first
    #[command(visible_alias = "transactions")]
    History {
        /// Show at most this many transactions
        #[arg(long)]
        limit: Option<usize>,

        /// Only transactions on or after this date (YYYY-MM-DD, UTC)
        #[arg(long, value_name = "DATE")]
        since: Option<String>,

        /// Include transactions that were archived
        #[arg(long)]
        archive: bool,

        #[command(subcommand)]
        command: Option<HistoryCommand>,
    },

    /// Reverse a recorded transaction (default: the most recent one)
    Rollback {
        /// Transaction id from `emrac history`, like tx-12
        tx: Option<String>,
    },

    /// Reverse the most recent transaction that can still be rolled back
    Undo,

    /// Re-apply the most recent rollback
    Redo,

    /// Look at the ledger: what touched it, and the audit watch on it
    Ledger {
        #[command(subcommand)]
        command: LedgerCommand,
    },

    /// (Internal) append history lines read from stdin to the root-owned ledger;
    /// run through sudo by emrac itself
    #[command(hide = true)]
    LedgerAppend,

    /// Print a shell completion script to stdout
    Completion { shell: CompletionShell },
}

/// The shells `SPEC.md` promises completion for — deliberately not
/// `clap_complete::Shell`, which would also advertise elvish/powershell.
#[derive(Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

/// Which caches: the words `source`, `build`, `binary`, `package` or their
/// letters `s`, `b`, `n`, `p`, or the flags below. None named: the command's
/// own default.
#[derive(Args, Clone, Default)]
pub struct CacheScopes {
    /// Cache categories, by word or letter (source|s build|b binary|n package|p)
    pub scope: Vec<String>,

    /// Downloaded sources
    #[arg(long)]
    pub source: bool,

    /// Extraction and staging trees of AUR builds
    #[arg(long)]
    pub build: bool,

    /// Built and fetched package files
    #[arg(long)]
    pub binary: bool,

    /// pacman's package cache
    #[arg(long)]
    pub package: bool,

    /// Keep only what undoing the latest transaction needs and what can't
    /// be downloaded again (config: cache.policy = "lean")
    #[arg(long)]
    pub lean: bool,
}

#[derive(Args, Clone)]
pub struct CacheArgs {
    #[command(flatten)]
    pub scopes: CacheScopes,

    /// Only entries at least this old (30d, 12h, 2w)
    #[arg(short = 'o', long, value_name = "DURATION")]
    pub older_than: Option<String>,

    /// Confirm an operation on every cache
    #[arg(short = 'f', long)]
    pub force: bool,
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Sizes per category and the largest packages
    #[command(visible_alias = "l")]
    List(CacheScopes),

    /// Totals, and how much `clean` would free
    #[command(visible_alias = "s")]
    Stats(CacheScopes),

    /// Remove what is safe to lose (default: build, binary and package)
    #[command(visible_alias = "c")]
    Clean(CacheArgs),

    /// Remove everything in the scope (no scope needs --force)
    #[command(visible_alias = "p")]
    Purge(CacheArgs),

    /// Remove the oldest removable entries until under a size limit
    #[command(visible_alias = "t")]
    Trim {
        /// The most a category may keep (500M, 10G)
        #[arg(short = 'm', long, value_name = "SIZE")]
        size_limit: String,

        #[command(flatten)]
        args: CacheArgs,
    },
}

#[derive(Subcommand)]
pub enum PacnewCommand {
    /// Show what you changed, what upstream changed, and how a merge comes out
    Diff {
        /// The live file (/etc/…) or its .pacnew / .pacsave
        file: String,
    },

    /// Go through the pending files and keep, take, merge or edit each
    /// (asks every time; --yes does not answer)
    Resolve {
        /// Only these files (default: all of them)
        files: Vec<String>,
    },

    /// Put back the file as it was before emrac's latest change to it
    /// (asks; --yes does not answer)
    Revert {
        /// The live file (default: emrac's latest change to any file)
        file: Option<String>,
    },

    /// Pending files and what emrac has changed so far
    Status,
}

#[derive(Subcommand)]
pub enum HistoryCommand {
    /// Move old transactions out of the live history into archive files
    /// (asks every time; --yes does not answer)
    Archive {
        /// Archive what is older than this: 90d, 12w, … (not less than cache.keep_days)
        #[arg(long, value_name = "AGE")]
        older_than: String,
    },

    /// Check that nothing in the history was edited, removed or added, and
    /// that it agrees with the ledger
    Verify {
        /// Also check the history against an anchor code from `history anchor`
        #[arg(long, value_name = "CODE")]
        against: Option<String>,

        /// Rebuild the history from the ledger when they disagree (asks;
        /// --yes does not answer; the old file is kept as history.jsonl.broken)
        #[arg(long)]
        repair: bool,
    },

    /// Send the history lines the ledger does not hold yet (asks for sudo)
    Sync,

    /// Print a short code for the newest history line, to keep somewhere
    /// else (a note, a repository): `verify --against` checks it later
    Anchor,

    /// Mark failed transactions as seen: they stay, and doctor stops counting them
    Ack {
        /// Transaction ids, like tx-3
        #[arg(required_unless_present = "failed", conflicts_with = "failed")]
        ids: Vec<String>,

        /// Every failed transaction not yet acknowledged
        #[arg(long)]
        failed: bool,
    },
}

#[derive(Subcommand)]
pub enum LedgerCommand {
    /// List what the audit system saw done to the ledger, and by which program
    Events,

    /// The kernel audit rules that watch the ledger
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
}

#[derive(Subcommand)]
pub enum WatchCommand {
    /// Install the audit rules (run as root: sudo emrac ledger watch install;
    /// asks first, --yes does not answer)
    Install,
}

#[derive(Subcommand)]
pub enum SnapshotCommand {
    /// Save the installed packages as a restore point
    Create {
        /// Name (default: snap-N)
        name: Option<String>,

        /// A note to remember it by
        #[arg(long)]
        note: Option<String>,
    },

    /// List snapshots, newest first
    List,

    /// Show the packages in a snapshot
    Show { name: String },

    /// Delete a snapshot
    Delete { name: String },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Print the resolved config
    Show,

    /// Print a single key's value
    Get { key: String },

    /// Set a key to a value
    Set { key: String, value: String },

    /// Reset a key to its default
    Unset { key: String },

    /// Open the config file in $EDITOR
    Edit,

    /// Check that the config file parses
    Validate,
}

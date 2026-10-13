use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "emrac",
    version,
    about = "A source-first package management platform for Arch Linux"
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
    #[arg(long, global = true)]
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

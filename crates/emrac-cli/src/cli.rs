use clap::{Args, Parser, Subcommand, ValueEnum};

/// Said when emrac is started as root where that is not what it is for.
pub const ROOT_WARNING: &str =
    "DON'T RUN AS ROOT UNLESS NECESSARY (emrac asks for sudo itself when a step needs it)";

/// The credit every `--help` ends with.
pub const CREDIT: &str = concat!(
    "Built by ",
    env!("CARGO_PKG_AUTHORS"),
    " — ",
    env!("CARGO_PKG_HOMEPAGE"),
    "\nSic Mundus Creatus Est."
);

/// The command tree with the credit on every (sub)command's help, not only the top one.
pub fn command() -> clap::Command {
    use clap::CommandFactory;
    fn credit(cmd: clap::Command, tail: &str) -> clap::Command {
        let names: Vec<String> = cmd
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        let mut cmd = cmd.after_help(tail.to_string());
        for name in names {
            cmd = cmd.mut_subcommand(name, |c| credit(c, tail));
        }
        cmd
    }
    credit(Cli::command(), CREDIT)
}

#[derive(Parser)]
#[command(
    name = "emrac",
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\nbuilt by ",
        env!("CARGO_PKG_AUTHORS"),
        "\nSic Mundus Creatus Est.\n",
        env!("CARGO_PKG_HOMEPAGE")
    ),
    about = "A hybrid package management platform for Arch Linux",
    after_help = concat!("Built by ", env!("CARGO_PKG_AUTHORS"), " — ", env!("CARGO_PKG_HOMEPAGE"), "\nSic Mundus Creatus Est.")
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

        /// Go ahead although other installed packages have upgrades waiting
        /// (a partial upgrade, which Arch does not support); for this run only
        #[arg(long)]
        allow_partial: bool,

        /// Run only the plan with this id (printed under every plan, and
        /// `digest` in --json); refuses if the plan is anything else
        #[arg(long, value_name = "ID")]
        expect_plan: Option<String>,
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

        /// Run only the plan with this id (printed under every plan, and
        /// `digest` in --json); refuses if the plan is anything else
        #[arg(long, value_name = "ID")]
        expect_plan: Option<String>,
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

        /// Run only the plan with this id (printed under every plan, and
        /// `digest` in --json); refuses if the plan is anything else
        #[arg(long, value_name = "ID")]
        expect_plan: Option<String>,
    },

    /// Upgrade installed packages, official repos and the AUR alike
    Upgrade {
        /// Packages to upgrade; omitted = everything that's out of date
        pkgs: Vec<String>,

        #[command(flatten)]
        review: AurReviewFlags,

        /// Go ahead although other installed packages have upgrades waiting
        /// (a partial upgrade, which Arch does not support); for this run only
        #[arg(long)]
        allow_partial: bool,

        /// Run only the plan with this id (printed under every plan, and
        /// `digest` in --json); refuses if the plan is anything else
        #[arg(long, value_name = "ID")]
        expect_plan: Option<String>,

        /// Go on although there is unread critical Arch news (read it first:
        /// `emrac news`)
        #[arg(long)]
        ack_news: bool,
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

    /// Look after emrac itself: check how it is installed and that its own
    /// files are in order (no pacman, network or root needed)
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
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

    /// Show a package's dependencies as a tree (installed packages)
    Tree {
        pkg: String,

        /// Show only this many levels
        #[arg(long, value_name = "N")]
        depth: Option<usize>,

        /// Include optional dependencies that are installed
        #[arg(long)]
        optional: bool,
    },

    /// The dependency graph of a package, as text, Graphviz `dot` or JSON
    Graph {
        pkg: String,

        /// Show only this many levels
        #[arg(long, value_name = "N")]
        depth: Option<usize>,

        /// Include optional dependencies that are installed
        #[arg(long)]
        optional: bool,

        /// text (default), dot (for `dot -Tsvg`) or json
        #[arg(long, value_enum, default_value = "text")]
        format: GraphFormat,
    },

    /// Source or binary? A recommendation from stated rules, with the reasons
    Recommend {
        /// The package; or `source` / `binary` and then the package, to see
        /// that side only
        #[arg(required = true, num_args = 1..=2, value_name = "[source|binary] PKG")]
        args: Vec<String>,
    },

    /// What happened to the system, day by day: transactions, builds,
    /// snapshots and config file actions
    Timeline {
        /// A period back from now (30d, 12h, 8w) or a date (YYYY-MM-DD, UTC);
        /// default 7d
        #[arg(long, value_name = "PERIOD|DATE")]
        since: Option<String>,

        /// Only what touched this package
        #[arg(long, value_name = "NAME")]
        pkg: Option<String>,
    },

    /// What emrac has built and installed here, and how long builds took
    Stats {
        /// Only the last period: a number with m, h, d or w (for example 30d)
        #[arg(long, value_name = "PERIOD")]
        since: Option<String>,
    },

    /// Measure what an installed package costs to start (runs its programs
    /// with `--version`, after asking)
    Benchmark {
        /// The installed package
        pkg: String,

        /// Only this executable of the package
        #[arg(long, value_name = "NAME")]
        exe: Option<String>,

        /// Runs after the first one (1 to 50)
        #[arg(long, default_value_t = 5)]
        runs: u32,

        /// Show earlier results for the package instead of running
        #[arg(long)]
        history: bool,
    },

    /// How the recommendation for a package was reached, rule by rule, and
    /// what would change it
    Explain {
        /// The package to explain the recommendation for
        pkg: String,
    },

    /// What a change to a package would affect (default: removing it)
    Impact {
        pkg: String,

        /// What removing it would break, leave unneeded and free (default)
        #[arg(long, conflicts_with_all = ["install", "upgrade"])]
        remove: bool,

        /// What installing it would do (the install plan)
        #[arg(long, conflicts_with_all = ["remove", "upgrade"])]
        install: bool,

        /// What upgrading it would do (the upgrade plan)
        #[arg(long, conflicts_with_all = ["remove", "install"])]
        upgrade: bool,
    },

    /// Find installed packages that require each other in a ring
    Cycles,

    /// What the last upgrade left to do: a reboot, services to restart, new
    /// failures, broken AUR packages, new .pacnew files
    Report,

    /// The Arch Linux news: which items you haven't seen, and which are
    /// critical (a breaking change, manual intervention, or a change that
    /// touches the kernel, bootloader, C library, systemd, pacman or drivers)
    News {
        #[command(subcommand)]
        command: Option<NewsCommand>,

        /// Every cached item, not only the newest 10 and the unread ones
        #[arg(long)]
        all: bool,
    },

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

    /// Build installed AUR packages again from source (after a library, Python
    /// or Qt update broke them): fetches and audits the sources like `upgrade`.
    /// For source packages only: rebuilding a -bin package fixes nothing
    Rebuild {
        /// The packages to build again (not with --check or --stale)
        pkgs: Vec<String>,

        /// List the installed AUR packages that need building again, or may,
        /// and why (reads files and pacman's log; changes nothing)
        #[arg(long, conflicts_with_all = ["pkgs", "stale"])]
        check: bool,

        /// Build again every package that is broken now (a missing library, an
        /// old Python or Perl folder); `--check` lists them
        #[arg(long, conflicts_with = "pkgs")]
        stale: bool,

        /// Never show the PKGBUILD/diff for review (the audit still runs)
        #[arg(long)]
        skip_pkgbuild: bool,

        /// Show the full PKGBUILD instead of a diff against the last review
        #[arg(long)]
        skip_diff: bool,

        /// Don't stop to ask when the audit finds high-risk problems
        /// (critical ones are always refused)
        #[arg(long)]
        accept_risks: bool,

        /// Build without the sandbox (the build then runs with your full access)
        #[arg(long)]
        no_sandbox: bool,

        /// Let the sandboxed build use the network, without asking
        #[arg(long)]
        allow_network: bool,
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

    /// Keep the mode and owner of a packaged file across upgrades
    Override {
        #[command(subcommand)]
        command: OverrideCommand,
    },

    /// Keep your own version of a packaged file across upgrades
    Divert {
        #[command(subcommand)]
        command: DivertCommand,
    },

    /// The local file policy (overrides and diversions): is it in effect?
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },

    /// (Internal) change or apply the local file policy as root; run through
    /// sudo by emrac itself, and by the pacman hook it installs
    #[command(hide = true)]
    PolicyHelper {
        #[command(subcommand)]
        verb: PolicyHelperVerb,
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

    /// Find a transaction that was cut off (power loss, killed terminal), say
    /// what it did from pacman's log and put it on record
    Recover,

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

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum GraphFormat {
    Text,
    Dot,
    Json,
}

#[derive(Subcommand)]
pub enum NewsCommand {
    /// Mark all the news so far as seen
    Ack,
}

#[derive(Subcommand)]
pub enum SelfCommand {
    /// Check emrac's own installation: the binary, how it was installed, its
    /// configuration, its history, the journal and the ledger helper
    Check,

    /// A deeper look than `check`: what an upgrade needs (tools, space,
    /// permissions), the release index in detail, and with --online whether
    /// the source answers. Repairs nothing; home directory paths are hidden
    Diagnose {
        /// Also ask the release source whether it answers
        #[arg(long)]
        online: bool,
    },

    /// Upgrade emrac to the newest release of your channel: downloads it,
    /// checks it against the release index, builds a package and installs it
    /// with pacman. Only for a copy a pacman package owns. --dry-run shows the plan
    #[command(visible_alias = "up")]
    Upgrade {
        /// Don't run the package's test suite while building
        #[arg(long)]
        skip_tests: bool,
    },

    /// Go back to an older release (same way as upgrade). Refuses if the
    /// older release could not read your files; nothing is converted back
    #[command(visible_alias = "dn")]
    Downgrade {
        /// The version to go back to, like 0.2.0 (needed when several older
        /// releases exist)
        #[arg(long, conflicts_with = "tag")]
        version: Option<String>,

        /// Or the release tag, like r2
        #[arg(long)]
        tag: Option<String>,

        /// Don't run the package's test suite while building
        #[arg(long)]
        skip_tests: bool,
    },

    /// Fetch emrac's release index and news from the configured source
    /// (`self.source`); never upgrades anything. --dry-run shows the plan
    Sync,

    /// Release news and announcements, from what the last sync fetched
    News {
        /// Every item, not only the newest 10
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum LedgerCommand {
    /// List what the audit system saw done to the ledger, and by which program
    Events {
        /// Mark other programs' events as seen (all of them, or the ids
        /// given, like ev-1a2b3c4d5e6f): they stay listed, and doctor
        /// stops counting them. Recorded in the history and the ledger.
        #[arg(long, value_name = "ID", num_args = 0..)]
        ack: Option<Vec<String>>,
    },

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

    /// Rewrite config.toml in the current style (banner, descriptions, the
    /// changed flags), keeping every value; a file that is already in that
    /// style is left as it is
    Refresh,

    /// Open the config file in $EDITOR
    Edit,

    /// Check that the config file parses
    Validate,
}

#[derive(Subcommand)]
pub enum OverrideCommand {
    /// Set the mode and/or owner a packaged file keeps after every upgrade
    Set {
        /// The file, as an absolute path
        path: String,

        /// Octal, like 755 or 4750
        #[arg(long)]
        mode: Option<String>,

        /// A user name or number
        #[arg(long)]
        owner: Option<String>,

        /// A group name or number
        #[arg(long)]
        group: Option<String>,

        /// Why (shown in the list)
        #[arg(long)]
        note: Option<String>,

        /// Allow a mode with the setuid or setgid bit without asking
        #[arg(long)]
        allow_setuid: bool,
    },

    /// The overrides and whether each is in effect now
    List,

    /// Stop keeping a file's mode and owner
    Remove { path: String },
}

#[derive(Subcommand)]
pub enum DivertCommand {
    /// Keep the file as it is now: upgrades set the package's version aside
    Add {
        /// The file, as an absolute path
        path: String,

        /// Why (shown in the list)
        #[arg(long)]
        note: Option<String>,

        /// Divert without asking: you know the package's later versions of
        /// this file, security fixes included, will not take effect
        #[arg(long)]
        allow_stale: bool,
    },

    /// The diversions and whether each is in effect now
    List,

    /// Stop keeping your version (the file is left as it is)
    Remove { path: String },

    /// Your version against the package's version that an upgrade set aside
    Diff { path: String },

    /// You have looked at the package's changes and keep your version:
    /// warn again only if the package changes the file again
    Reviewed { path: String },
}

#[derive(Subcommand)]
pub enum PolicyCommand {
    /// What is configured, and whether it is in effect and will stay so
    Status,

    /// Apply the policy now (the pacman hook does this after each transaction)
    Apply,
}

#[derive(Subcommand)]
pub enum PolicyHelperVerb {
    Apply,
    Capture,
    SetOverride {
        path: String,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        allow_setuid: bool,
    },
    RemoveOverride {
        path: String,
    },
    AddDivert {
        path: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        allow_stale: bool,
    },
    Reviewed {
        path: String,
    },
    RemoveDivert {
        path: String,
    },
    Diff {
        path: String,
    },
}

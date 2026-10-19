use clap::{Args, Parser, Subcommand, ValueEnum};

/// Said when emrac is started as root where that is not what it is for.
pub const ROOT_WARNING: &str =
    "DON'T RUN AS ROOT UNLESS NECESSARY (emrac asks for sudo itself when a step needs it)";

fn command_with(tail: &str) -> clap::Command {
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
    credit(Cli::command(), tail)
}

/// One SGR parameter string (`1;38;2;90;210;240`) as a clap style.
fn sgr_style(code: Option<&str>) -> clap::builder::styling::Style {
    use clap::builder::styling::{Ansi256Color, Color, Effects, RgbColor, Style as S};
    let mut style = S::new();
    let Some(code) = code else {
        return style;
    };
    let nums: Vec<u32> = code.split(';').filter_map(|n| n.parse().ok()).collect();
    let mut i = 0;
    while i < nums.len() {
        match nums[i] {
            1 => style = style.effects(style.get_effects() | Effects::BOLD),
            2 => style = style.effects(style.get_effects() | Effects::DIMMED),
            4 => style = style.effects(style.get_effects() | Effects::UNDERLINE),
            7 => style = style.effects(style.get_effects() | Effects::INVERT),
            n @ 30..=37 => style = style.fg_color(Some(Color::Ansi256(Ansi256Color(n as u8 - 30)))),
            n @ 90..=97 => {
                style = style.fg_color(Some(Color::Ansi256(Ansi256Color(n as u8 - 90 + 8))))
            }
            38 if nums.get(i + 1) == Some(&5) && i + 2 < nums.len() => {
                style = style.fg_color(Some(Color::Ansi256(Ansi256Color(
                    nums[i + 2].min(255) as u8
                ))));
                i += 2;
            }
            38 if nums.get(i + 1) == Some(&2) && i + 4 < nums.len() => {
                style = style.fg_color(Some(Color::Rgb(RgbColor(
                    nums[i + 2].min(255) as u8,
                    nums[i + 3].min(255) as u8,
                    nums[i + 4].min(255) as u8,
                ))));
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// The help pages in emrac's own colours: headings and usage in the brand
/// colour, commands and flags green, placeholders cyan, errors red.
fn help_styles(st: &emrac_core::style::Style) -> clap::builder::styling::Styles {
    use emrac_core::style::Role;
    clap::builder::styling::Styles::styled()
        .header(sgr_style(st.sgr(Role::Brand)))
        .usage(sgr_style(st.sgr(Role::Brand)))
        .literal(sgr_style(st.sgr(Role::Command)))
        .placeholder(sgr_style(st.sgr(Role::Note)))
        .error(sgr_style(st.sgr(Role::Problem)))
        .valid(sgr_style(st.sgr(Role::Ok)))
        .invalid(sgr_style(st.sgr(Role::Warn)))
        .context(sgr_style(st.sgr(Role::Dim)))
        .context_value(sgr_style(st.sgr(Role::Version)))
}

/// The credit line in colour (plain when the stream has none).
fn styled_credit(st: &emrac_core::style::Style) -> String {
    use emrac_core::style::Role;
    format!(
        "{} {} {} {}\n{}",
        st.paint(Role::Dim, "Built by"),
        st.paint(Role::Package, env!("CARGO_PKG_AUTHORS")),
        st.paint(Role::Dim, "\u{2014}"),
        st.paint(Role::Path, env!("CARGO_PKG_HOMEPAGE")),
        st.paint(Role::Accent, "Sic Mundus Creatus Est.")
    )
}

/// The command tree for parsing: every help page, with `emrac` and
/// `emrac --help`, in colour when the stream has it.
pub fn command_styled(st: &emrac_core::style::Style) -> clap::Command {
    command_with(&styled_credit(st))
        .color(if st.color {
            clap::ColorChoice::Always
        } else {
            clap::ColorChoice::Never
        })
        .styles(help_styles(st))
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

    /// When to use colour: auto (a terminal, not NO_COLOR), always or never
    #[arg(long, global = true, value_enum, value_name = "WHEN")]
    pub color: Option<ColorWhen>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// Shared by `install` and `upgrade` — anything that builds an AUR package
/// goes through the same PKGBUILD/diff review, so the opt-out flags are
/// identical for both.
#[derive(Args, Default)]
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

    /// Install one or more packages from official repositories or the AUR, or
    /// package files on this machine (`emrac install ./foo-1.0-1-x86_64.pkg.tar.zst`,
    /// what `pacman -U` does, with the same plan, audit and history)
    Install {
        /// Package names, or paths of package files (`*.pkg.tar.zst`): not both
        #[arg(required = true)]
        pkgs: Vec<String>,

        #[command(flatten)]
        review: AurReviewFlags,

        /// Go ahead although other installed packages have upgrades waiting
        /// (a partial upgrade, which Arch does not support); for this run only
        #[arg(long)]
        allow_partial: bool,

        /// Build the official packages named from Arch's packaging source here
        /// (in the sandbox, audited, inspected before it is installed) instead
        /// of installing the repository's binary. AUR packages are source anyway
        #[arg(long, conflicts_with = "binary")]
        source: bool,

        /// Compile nothing: install only what exists prebuilt. A package that
        /// exists only as source (the AUR's) is refused, naming its `-bin` one
        #[arg(long, conflicts_with = "source")]
        binary: bool,

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

        /// Install the repository's binary of the packages you build from
        /// source and leave their rebuild pending (`emrac from-source rebuild`)
        #[arg(long)]
        skip_source_builds: bool,
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

    /// Your mirrors, measured and ranked from a history, not a one-time test
    ///
    /// `emrac mirrors` lists the active mirrors of the mirror list with what has
    /// been measured; `mirrors test` measures them now (a real `core.db`
    /// download: latency, speed, and how far behind Arch each mirror says it
    /// is); `mirrors rank` shows the order the history supports and can put it
    /// in the list. Ranking only reorders the mirrors already active in the
    /// file; it never adds one.
    #[command(verbatim_doc_comment)]
    Mirrors {
        #[command(subcommand)]
        command: Option<MirrorsCommand>,

        /// The mirror list to read (default /etc/pacman.d/mirrorlist)
        #[arg(long, global = true, value_name = "FILE")]
        file: Option<String>,
    },

    /// Every element of the look in this terminal and theme: colours, marks,
    /// boxes, tables, bars, diffs and the voice
    Ui {
        /// Preview a theme: default, light, mono or high-contrast
        #[arg(long, value_name = "THEME")]
        theme: Option<String>,

        /// Also run a spinner and a progress bar for about three seconds
        #[arg(long)]
        demo: bool,
    },

    /// Packages on a drive for a machine without network: create, verify, apply
    ///
    /// A bundle is a folder of the repositories' own package files and their
    /// signatures, with a manifest of checksums. Make it where the network is;
    /// install it on the machine that has none. AUR packages are not bundled.
    ///
    /// Two kinds of check: the manifest's checksums catch a damaged copy; the
    /// signatures are what make a package trustworthy, and `apply` makes pacman
    /// require a valid one for every package.
    ///
    /// NOTE ON OLD KEYRINGS: a machine that has been offline for months may not
    /// know the keys new packages are signed with and will refuse them. Bring
    /// `archlinux-keyring` over in a bundle of its own first. See
    /// `emrac bundle apply --help` for the details.
    #[command(verbatim_doc_comment)]
    Bundle {
        #[command(subcommand)]
        command: BundleCommand,
    },

    /// Describe this machine in one small file: packages, a safe subset of the
    /// settings, holds and repository names (to the screen, or with -o)
    Export {
        /// Only the installed packages
        #[arg(long)]
        packages: bool,

        /// Only the settings that can be exchanged
        #[arg(long)]
        config: bool,

        /// Only the repository names
        #[arg(long)]
        repos: bool,

        /// Only the holds (pins)
        #[arg(long)]
        pins: bool,

        /// Write to this file instead of the screen (never overwrites one)
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<String>,
    },

    /// Compare an exchange file with this machine, then apply each part you
    /// agree to (--dry-run: the comparison only; --json: the comparison only)
    Import {
        /// The file `emrac export` wrote
        file: String,

        /// Only the packages
        #[arg(long)]
        packages: bool,

        /// Only the settings
        #[arg(long)]
        config: bool,

        /// Only the repository names (reported, never applied)
        #[arg(long)]
        repos: bool,

        /// Only the holds (pins)
        #[arg(long)]
        pins: bool,
    },

    /// The manual page of a command (a subcommand as a second word), or all of them
    Man {
        /// The command, for example `install` or `snapshot create`; `emrac`
        /// for the main page
        #[arg(
            required_unless_present = "generate",
            num_args = 1..=2,
            value_name = "COMMAND",
            conflicts_with = "generate"
        )]
        command: Vec<String>,

        /// Print the roff source instead of showing the page
        #[arg(long, conflicts_with = "generate")]
        roff: bool,

        /// Write emrac.1 and a page for every command (emrac-install.1,
        /// emrac-snapshot-create.1 ...) into this folder, for a package to
        /// install under /usr/share/man/man1
        #[arg(long, value_name = "DIR")]
        generate: Option<String>,
    },

    /// Where the documentation is
    Docs {
        /// Open the documentation site in the browser
        #[arg(long)]
        open: bool,
    },

    /// The licence (GPL-3.0-or-later)
    License {
        /// The whole text of the licence
        #[arg(long)]
        full: bool,
    },

    /// Who made emrac and what it is built on
    Credits,

    /// Where to get help, and what to run first
    Support {
        /// Open the issues page in the browser
        #[arg(long)]
        open: bool,
    },

    /// Where to send an idea or a complaint
    Feedback {
        /// Open the new-issue page in the browser
        #[arg(long)]
        open: bool,
    },

    /// A report to paste into an issue: the doctor report and the last
    /// failed transactions, with your user name, host name and home scrubbed.
    /// It is printed, never sent
    BugReport {
        /// Open the new-issue page (only a title is filled in)
        #[arg(long)]
        open: bool,
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

    /// A report card for a package: performance, build time, maintenance,
    /// security, popularity and source worthiness, each from a stated rule
    ///
    /// A dimension with no data is shown as unknown, never as a made-up number.
    /// The overall number covers only the known dimensions and says so; a
    /// security gate is shown apart from it. `--details` shows every rule and
    /// the points it moved.
    #[command(verbatim_doc_comment)]
    Score {
        /// The package (official or AUR)
        pkg: String,

        /// Show the rules behind every number
        #[arg(long)]
        details: bool,

        /// Weights for the overall number, for this run: `security=5,popularity=0`
        /// (0 leaves a dimension out, 10 is the most, 1 the default; the config
        /// has `[score]` for the standing ones)
        #[arg(long, value_name = "LIST")]
        weights: Option<String>,

        /// Dimensions you care about: they count three times as much
        /// (`security,maintenance`)
        #[arg(long, value_name = "LIST")]
        care: Option<String>,
    },

    /// Installed packages against the Arch Security Tracker
    ///
    /// Only entries that look current count as confirmed. An entry with no
    /// recorded fix and an old CVE, or for another major version, is shown as
    /// possibly stale and never blocks anything. `emrac vulns ignore AVG-...`
    /// sets one aside; it lapses if the entry later records a fix.
    #[command(verbatim_doc_comment)]
    Vulns {
        /// Also show possibly stale and ignored entries
        #[arg(long)]
        all: bool,

        /// Only entries whose fix is already in the repositories
        #[arg(long)]
        fixable: bool,

        /// Exit with an error when a confirmed High or Critical entry exists
        #[arg(long)]
        check: bool,

        #[command(subcommand)]
        command: Option<VulnsCommand>,
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

    /// Find the transaction that broke something: put the system back into states
    /// it really had, halve the range each time, and ask (or run your test) in
    /// each. Whatever happens, it ends by putting the system back as it was
    Bisect {
        /// A state that worked: a transaction id (tx-12, the system was fine
        /// after it), a date (2026-10-10, the end of that day) or a time back (7d)
        #[arg(long, value_name = "WHEN")]
        good: String,

        /// A state that is broken (default: now)
        #[arg(long, value_name = "WHEN")]
        bad: Option<String>,

        /// A command that decides: exit 0 works, 125 cannot tell, anything else
        /// is broken. Without it you are asked in each state
        #[arg(long, value_name = "COMMAND")]
        test: Option<String>,
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

    /// The packages you build from source (`install --source`): which they are,
    /// whether the installed one was built here, and what waits to be rebuilt.
    /// An upgrade rebuilds them from Arch's packaging source instead of
    /// installing the repository's binary
    #[command(name = "from-source", visible_alias = "fs")]
    FromSource {
        #[command(subcommand)]
        command: Option<FromSourceCommand>,
    },

    /// Build installed AUR packages again from source (after a library, Python
    /// or Qt update broke them): fetches and audits the sources like `upgrade`.
    /// For source packages only: rebuilding a -bin package fixes nothing
    Rebuild {
        /// The packages to build again (with --ack or --unack: the packages to
        /// acknowledge or take back)
        pkgs: Vec<String>,

        /// List the installed AUR packages that need building again, or may,
        /// and why (reads files and pacman's log; changes nothing)
        #[arg(long, conflicts_with_all = ["pkgs", "stale"])]
        check: bool,

        /// Build again every package that is broken now (a missing library, an
        /// old Python or Perl folder); `--check` lists them
        #[arg(long, conflicts_with = "pkgs")]
        stale: bool,

        /// Acknowledge that these packages may need building again and you leave
        /// them: `doctor` and `--check` stop listing them. It lapses when the
        /// package is built again or its note says something new. A source package
        /// that is broken now cannot be acknowledged (rebuild it); a binary
        /// package can (only its vendor can mend it)
        #[arg(long, conflicts_with_all = ["check", "stale", "unack"], requires = "pkgs")]
        ack: bool,

        /// Take an acknowledgement back
        #[arg(long, conflicts_with_all = ["check", "stale", "ack"], requires = "pkgs")]
        unack: bool,

        /// Why, for your own memory (with --ack)
        #[arg(long, requires = "ack", value_name = "TEXT")]
        reason: Option<String>,

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
    /// Drawn in the terminal with box-drawing characters, by emrac itself
    Boxes,
    /// Drawn by `graph-easy` (installed separately) from the dot form
    Easy,
    Dot,
    Json,
}

#[derive(Subcommand)]
pub enum MirrorsCommand {
    /// Measure the active mirrors now and add the results to the history
    Test {
        /// Measure at most this many (the first ones in the file)
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// How many at a time
        #[arg(long, default_value_t = 6)]
        jobs: usize,
    },

    /// The order the history supports, and why; optionally write or apply it
    ///
    /// The order: good (fastest first), then stale (more than a day behind
    /// Arch), then unreliable (fewer than 8 of the last 10 worked, or the last
    /// one failed), then untested, in the order of your file.
    #[command(verbatim_doc_comment)]
    Rank {
        /// Keep only the best N active; the rest are commented out
        #[arg(long, value_name = "N")]
        keep: Option<usize>,

        /// Write the proposed list to this file (never overwrites one)
        #[arg(long, value_name = "FILE")]
        write: Option<String>,

        /// Replace the mirror list: asks, saves the old one, uses sudo
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub enum BundleCommand {
    /// Make a bundle in a new folder: repository packages with their
    /// signatures and a manifest of checksums (no AUR packages)
    ///
    /// Without --for the bundle holds the named packages and their whole
    /// dependency closure. With --for (the target's `emrac export --packages`
    /// file) only what the target lacks or has older goes in; with --for and no
    /// names, every upgrade the target is waiting for.
    ///
    /// If the target has been offline for months, make a bundle of
    /// `archlinux-keyring` alone first and have it applied before the real one:
    /// pacman checks every signature before it installs anything, so a newer
    /// keyring in the same bundle cannot help the other packages. The details
    /// are in `emrac bundle apply --help`.
    #[command(verbatim_doc_comment)]
    Create {
        /// The new folder
        dir: String,

        /// The packages; their dependencies come along. With --for and none
        /// named: every upgrade the target is waiting for
        pkgs: Vec<String>,

        /// The target's `emrac export --packages` file: only what it lacks or
        /// has older goes in
        #[arg(long = "for", value_name = "EXPORT_FILE")]
        target: Option<String>,
    },

    /// Check a bundle's files against its manifest (no root, no network)
    Verify {
        /// The bundle folder
        dir: String,
    },

    /// Install a bundle: verify, show what changes, ask, then `pacman -U`
    /// with signatures required (never a downgrade)
    ///
    /// Order of work: the files are checked against the manifest; each package is
    /// compared with what is installed (install, upgrade, already there and
    /// skipped, or older than installed and refused: a bundle never downgrades);
    /// the pre-flight runs with its network checks left out; the plan is shown and
    /// you are asked (--yes answers); then it is journaled, installed with
    /// `pacman -U` under a copy of pacman.conf that sets LocalFileSigLevel to
    /// Required, and recorded in the history. The sync databases are not in a
    /// bundle, so the machine's own are not refreshed.
    ///
    /// The keyring, and why a bundle can be refused although it is genuine
    ///
    ///   `apply` has pacman check every package's signature against the keyring of the
    ///   machine it runs on: the list of Arch packagers' keys that machine trusts. Arch
    ///   rotates those keys over time. A machine that has been offline for months may
    ///   not know the key a new package was signed with, and pacman then stops with
    ///   "invalid or corrupted package (PGP signature)" even though the bundle is fine.
    ///
    ///   The keyring is itself a package, `archlinux-keyring`, but pacman checks ALL the
    ///   signatures before it installs ANYTHING. So putting a newer keyring into the same
    ///   bundle as the other packages does not help: the other packages are checked
    ///   against the old keyring first, and fail.
    ///
    ///   What to do, for a machine that has been offline for a while:
    ///     1. on the machine with the network, make a small bundle of the keyring alone:
    ///          emrac bundle create /mnt/usb/keyring archlinux-keyring
    ///     2. on the offline machine, apply that one first (the package refreshes the
    ///        keys itself when it is installed):
    ///          emrac bundle apply /mnt/usb/keyring
    ///     3. then make and apply the real bundle.
    ///
    ///   If you see "invalid or corrupted package (PGP signature)", suspect the keyring
    ///   before you suspect the bundle: `emrac bundle verify` passing only means the
    ///   files are the ones that were put in, not that this machine trusts who signed
    ///   them. If even the keyring bundle is refused, the machine is too far behind to
    ///   trust it; emrac does not turn the signature check off. Bring the keyring in
    ///   by another route you trust (newer install media, or check the key fingerprints
    ///   yourself against archlinux.org and run `pacman-key --populate archlinux`).
    #[command(verbatim_doc_comment)]
    Apply {
        /// The bundle folder
        dir: String,
    },
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
pub enum VulnsCommand {
    /// Set an entry aside (it lapses if the entry later records a fix)
    Ignore {
        /// The entry, like AVG-2765
        id: String,

        /// Why, for your own memory
        #[arg(long)]
        reason: Option<String>,
    },

    /// Count an ignored entry again
    Unignore { id: String },

    /// List what is ignored
    Ignored,
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

#[derive(Subcommand, Debug, Clone)]
pub enum FromSourceCommand {
    /// List the packages with a source-build policy and what is known about each
    List,
    /// Build these installed official packages from source from now on (the
    /// build waits for `rebuild` or the next upgrade)
    Add {
        #[arg(required = true)]
        pkgs: Vec<String>,
    },
    /// End the policy: these packages follow the repository's binary again
    /// (the installed one is left as it is)
    Drop {
        #[arg(required = true)]
        pkgs: Vec<String>,
    },
    /// Build the pending ones (or the named ones) from source now
    Rebuild {
        pkgs: Vec<String>,

        /// Never show the PKGBUILD/diff for review (the audit still runs)
        #[arg(long)]
        accept_risks: bool,
    },
}

use clap::{Parser, Subcommand};

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

    /// Install one or more packages from official repositories
    Install {
        #[arg(required = true)]
        pkgs: Vec<String>,
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
}

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
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search official repositories by package name or description
    Search {
        query: String,

        /// Limit the number of results shown
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Show detailed metadata for a package in the official repositories
    Info { pkg: String },
}

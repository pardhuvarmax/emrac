mod cli;
mod commands;
mod output;

use anyhow::Result;
use clap::Parser;
use emrac_core::Sources;

use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(&cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    let sources = Sources::init()?;

    match &cli.command {
        Commands::Search {
            query,
            limit,
            official,
            aur,
        } => commands::search::run(
            &sources, query, *limit, *official, *aur, cli.offline, cli.json, cli.quiet,
        ),
        Commands::Info { pkg } => commands::info::run(&sources, pkg, cli.offline, cli.json),
    }
}

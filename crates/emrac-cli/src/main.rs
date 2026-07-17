mod cli;
mod commands;
mod output;

use anyhow::Result;
use clap::Parser;
use emrac_core::AlpmBackend;

use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(&cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    let backend = AlpmBackend::init()?;

    match &cli.command {
        Commands::Search { query, limit } => {
            commands::search::run(&backend, query, *limit, cli.json)
        }
        Commands::Info { pkg } => commands::info::run(&backend, pkg, cli.json),
    }
}

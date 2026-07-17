mod cli;
mod commands;
mod output;
mod prompt;

use clap::Parser;
use emrac_core::{Result, Sources};

use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(&cli) {
        eprintln!("emrac {}: {err}", err.voice());
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
        Commands::Install {
            pkgs,
            skip_pkgbuild,
            skip_diff,
        } => commands::install::run(
            &sources,
            pkgs,
            cli.dry_run,
            cli.yes,
            cli.offline,
            *skip_pkgbuild,
            *skip_diff,
            cli.json,
        ),
        Commands::Remove {
            pkgs,
            cascade,
            recursive,
        } => commands::remove::run(
            &sources, pkgs, *cascade, *recursive, cli.dry_run, cli.yes, cli.json,
        ),
    }
}

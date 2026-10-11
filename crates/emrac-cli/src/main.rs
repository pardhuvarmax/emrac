mod cli;
mod commands;
mod output;
mod prompt;

use clap::Parser;
use emrac_core::{Config, Result, Sources};

use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(&cli) {
        eprintln!("emrac {}: {err}", err.voice());
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    if let Commands::Completion { shell } = &cli.command {
        commands::completion::run(*shell);
        return Ok(());
    }

    if let Commands::History { limit, since } = &cli.command {
        return commands::history::run(*limit, since.as_deref(), cli.json);
    }

    // `config` doesn't need a working pacman/alpm setup — handle it before
    // paying for `Sources::init()`.
    if let Commands::Config { command } = &cli.command {
        return commands::config::run(command);
    }

    let config = Config::load()?;
    // Config only ever raises the effective default (a flag on the CLI has
    // no way to turn one of these back off) — see docs/plan/u1-config.md.
    let offline = cli.offline || config.flags.offline;
    let yes = cli.yes || config.flags.yes;
    let dry_run = cli.dry_run || config.flags.dry_run;
    let json = cli.json || config.flags.json;
    let quiet = cli.quiet || config.flags.quiet;

    let sources = Sources::init()?;

    match &cli.command {
        Commands::Search {
            query,
            limit,
            official,
            aur,
        } => commands::search::run(
            &sources,
            query,
            limit.or(config.search.limit),
            *official || config.search.official,
            *aur || config.search.aur,
            offline,
            json,
            quiet,
        ),
        Commands::Info { pkg } => commands::info::run(&sources, pkg, offline, json),
        Commands::Install { pkgs, review } => commands::install::run(
            &sources,
            pkgs,
            dry_run,
            yes,
            offline,
            review.skip_pkgbuild || config.review.skip_pkgbuild,
            review.skip_diff || config.review.skip_diff,
            json,
        ),
        Commands::Remove {
            pkgs,
            cascade,
            recursive,
        } => commands::remove::run(&sources, pkgs, *cascade, *recursive, dry_run, yes, json),
        Commands::Upgrade { pkgs, review } => commands::upgrade::run(
            &sources,
            pkgs,
            dry_run,
            yes,
            offline,
            review.skip_pkgbuild || config.review.skip_pkgbuild,
            review.skip_diff || config.review.skip_diff,
            json,
            quiet,
        ),
        Commands::Config { .. } | Commands::Completion { .. } | Commands::History { .. } => {
            unreachable!("handled above")
        }
    }
}

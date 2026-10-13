mod cli;
mod commands;
mod output;
mod prompt;

use clap::Parser;
use emrac_core::{Config, Result, Sources};

use cli::{Cli, Commands, SnapshotCommand};
use commands::rollback::Which;

fn main() {
    // Rust ignores SIGPIPE by default, which turns `emrac … | head` into a
    // panic inside `println!`. Restore the default so the process just ends,
    // like any other Unix tool.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

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

    // Listing, showing and deleting snapshots only touch emrac's own files.
    if let Commands::Snapshot {
        command:
            Some(
                command @ (SnapshotCommand::List
                | SnapshotCommand::Show { .. }
                | SnapshotCommand::Delete { .. }),
            ),
    } = &cli.command
    {
        return commands::snapshot::run_stored(command, cli.json);
    }

    // Listing and removing pins only touch emrac's own file.
    if let Commands::Pin { pkgs, .. } = &cli.command {
        if pkgs.is_empty() {
            return commands::pin::list(cli.json);
        }
    }
    if let Commands::Unpin { pkgs } = &cli.command {
        return commands::pin::unpin(pkgs, cli.json);
    }

    // Auditing files on disk needs no pacman or libalpm.
    if let Commands::Audit { targets } = &cli.command {
        if targets.iter().all(|t| commands::audit::is_path(t)) {
            return commands::audit::run(None, targets, cli.json);
        }
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
            review.accept_risks,
            json,
        ),
        Commands::Remove {
            pkgs,
            cascade,
            recursive,
        } => commands::remove::run(&sources, pkgs, *cascade, *recursive, dry_run, yes, json),
        Commands::Purge {
            pkgs,
            cascade,
            recursive,
        } => commands::purge::run(&sources, pkgs, *cascade, *recursive, dry_run, yes, json),
        Commands::Upgrade { pkgs, review } => commands::upgrade::run(
            &sources,
            pkgs,
            dry_run,
            yes,
            offline,
            review.skip_pkgbuild || config.review.skip_pkgbuild,
            review.skip_diff || config.review.skip_diff,
            review.accept_risks,
            json,
            quiet,
            config.snapshot.before_upgrade,
        ),
        Commands::Pin {
            pkgs,
            reason,
            until,
        } => commands::pin::pin(&sources, pkgs, reason.as_deref(), until.as_deref(), json),
        Commands::Unpin { .. } => unreachable!("handled above"),
        Commands::Audit { targets } => commands::audit::run(Some(&sources), targets, json),
        Commands::Deps { pkg, recursive } => commands::deps::deps(&sources, pkg, *recursive, json),
        Commands::Rdeps { pkg } => commands::deps::rdeps(&sources, pkg, json),
        Commands::Why { pkg } => commands::deps::why(&sources, pkg, json),
        Commands::Orphans => commands::deps::orphans(&sources, json),
        Commands::Snapshot { command } => {
            commands::snapshot::create(&sources, command.as_ref(), json)
        }
        Commands::Restore { name, prune } => {
            commands::restore::run(&sources, name, *prune, dry_run, yes, offline, json)
        }
        Commands::Verify { pkgs } => commands::verify::run(&sources, pkgs, json),
        Commands::Reinstall { pkgs } => {
            commands::reinstall::run(&sources, pkgs, dry_run, yes, offline, json)
        }
        Commands::Downgrade { pkg, version, list } => {
            if *list {
                commands::downgrade::list(&sources, pkg, offline, json)
            } else {
                commands::downgrade::run(
                    &sources,
                    pkg,
                    version.as_deref(),
                    dry_run,
                    yes,
                    offline,
                    json,
                )
            }
        }
        Commands::Rollback { tx } => commands::rollback::run(
            &sources,
            Which::Rollback(tx.as_deref()),
            dry_run,
            yes,
            offline,
            json,
        ),
        Commands::Undo => {
            commands::rollback::run(&sources, Which::Rollback(None), dry_run, yes, offline, json)
        }
        Commands::Redo => {
            commands::rollback::run(&sources, Which::Redo, dry_run, yes, offline, json)
        }
        Commands::Config { .. } | Commands::Completion { .. } | Commands::History { .. } => {
            unreachable!("handled above")
        }
    }
}

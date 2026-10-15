mod cli;
mod commands;
mod output;
mod prompt;

use emrac_core::{Config, Limits, Result, Sources};

use cli::{Cli, Commands, HistoryCommand, LedgerCommand, SnapshotCommand, WatchCommand};
use commands::aur_install::BuildChoice;
use commands::rollback::Which;

fn main() {
    // Rust ignores SIGPIPE by default, which turns `emrac … | head` into a
    // panic inside `println!`. Restore the default so the process just ends,
    // like any other Unix tool.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    warn_if_root();

    let cli = {
        use clap::FromArgMatches;
        Cli::from_arg_matches(&cli::command().get_matches()).unwrap_or_else(|e| e.exit())
    };

    if let Err(err) = run(&cli) {
        emrac_core::say_err!(err.voice(), "{err}");
        std::process::exit(1);
    }
}

/// emrac asks for sudo itself when a step needs it, so running the whole
/// program as root is almost never what you want. Said before the arguments
/// are parsed, so `sudo emrac --help` hears it too. The commands that are
/// meant for root (the audit watch, the ledger helper) and `completion` stay quiet.
fn warn_if_root() {
    let first = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let meant_for_root = matches!(
        first.as_deref(),
        Some("ledger" | "ledger-append" | "completion")
    );
    if !meant_for_root && emrac_core::ledger::running_as_root() {
        emrac_core::say_err!(emrac_core::Voice::Warns, "{}", cli::ROOT_WARNING);
    }
}

fn run(cli: &Cli) -> Result<()> {
    if let Commands::Completion { shell } = &cli.command {
        commands::completion::run(*shell);
        return Ok(());
    }

    if let Commands::Ledger { command } = &cli.command {
        return match command {
            LedgerCommand::Events { ack } => commands::ledger::events(ack.as_deref(), cli.json),
            LedgerCommand::Watch {
                command: WatchCommand::Install,
            } => commands::ledger::watch_install(cli.dry_run),
        };
    }

    if let Commands::LedgerAppend = &cli.command {
        return commands::ledger::append_from_stdin();
    }

    if let Commands::History {
        limit,
        since,
        archive,
        command,
    } = &cli.command
    {
        return match command {
            None => commands::history::run(*limit, since.as_deref(), *archive, cli.json),
            Some(HistoryCommand::Archive { older_than }) => {
                commands::history::archive(older_than, cli.dry_run, cli.json)
            }
            Some(HistoryCommand::Verify { repair, against }) => {
                commands::history::verify(*repair, against.as_deref(), cli.json)
            }
            Some(HistoryCommand::Recover) => commands::history::recover(cli.dry_run, cli.json),
            Some(HistoryCommand::Anchor) => commands::history::anchor(cli.json),
            Some(HistoryCommand::Sync) => commands::ledger::sync_interactive(cli.json),
            Some(HistoryCommand::Ack { ids, failed }) => {
                commands::history::ack(ids, *failed, cli.json)
            }
        };
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
    // no way to turn one of these back off) — see docs/plan/r1/u1-config.md.
    let offline = cli.offline || config.flags.offline;
    let yes = cli.yes || config.flags.yes;
    let dry_run = cli.dry_run || config.flags.dry_run;
    let json = cli.json || config.flags.json;
    let quiet = cli.quiet || config.flags.quiet;

    let cache_settings = || commands::cache::Settings {
        dry_run,
        yes,
        json,
        verbose: cli.verbose,
        lean: config.cache.is_lean(),
        keep: config.cache.keep_days,
    };

    let mut sources = Sources::init()?;
    // Without --yes, pacman asks its own questions (conflicts, provider
    // choices, replacements); with it, every question takes its default.
    sources.set_interactive(!yes);

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
            BuildChoice::from_flags(review, &config.build, !yes),
            review.inspect || config.review.inspect_official,
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
            BuildChoice::from_flags(review, &config.build, !yes),
            review.inspect || config.review.inspect_official,
            json,
            quiet,
            config.snapshot.before_upgrade,
        ),
        Commands::Cache { command } => {
            commands::cache::run(&sources, command.as_ref(), cache_settings())
        }
        Commands::CacheClean(args) => commands::cache::run(
            &sources,
            Some(&cli::CacheCommand::Clean(args.clone())),
            cache_settings(),
        ),
        Commands::CacheStats(scopes) => commands::cache::run(
            &sources,
            Some(&cli::CacheCommand::Stats(scopes.clone())),
            cache_settings(),
        ),
        Commands::Pacnew { fetch, command } => commands::pacnew::run(
            &sources,
            *fetch && !offline,
            command.as_ref(),
            dry_run,
            json,
        ),
        Commands::Pin {
            pkgs,
            reason,
            until,
        } => commands::pin::pin(&sources, pkgs, reason.as_deref(), until.as_deref(), json),
        Commands::Unpin { .. } => unreachable!("handled above"),
        Commands::Doctor { report, online } => {
            commands::doctor::run(&sources, *report, *online && !offline, json)
        }
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
        Commands::Reinstall {
            pkgs,
            accept_risks,
            no_sandbox,
            allow_network,
        } => {
            let choice = BuildChoice {
                accept_risks: *accept_risks,
                no_sandbox: *no_sandbox,
                allow_network: *allow_network,
                interactive: !yes,
                limits: Limits::from_config(&config.build),
            };
            commands::reinstall::run(&sources, pkgs, dry_run, yes, offline, choice, json)
        }
        Commands::Build { targets, review } => commands::build::run(
            &sources,
            targets,
            review.skip_pkgbuild || config.review.skip_pkgbuild,
            review.skip_diff || config.review.skip_diff,
            yes,
            BuildChoice::from_flags(review, &config.build, !yes),
            json,
        ),
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
        Commands::Config { .. }
        | Commands::Completion { .. }
        | Commands::History { .. }
        | Commands::Ledger { .. }
        | Commands::LedgerAppend => {
            unreachable!("handled above")
        }
    }
}

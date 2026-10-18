# src/

`lib.rs` is the crate root: it declares the modules and re-exports the public API, and nothing else. [`sources.rs`](./sources.rs) is the intended entry point; the backends in [`backend/`](./backend) are not re-exported.

**Core model**
- [`sources.rs`](./sources.rs) — `Sources`: merges the official repositories and the AUR behind one `search`/`info` surface, plans and runs install, remove, upgrade, rollback, downgrade, reinstall, purge, restore, builds, `doctor`, and the `pacnew` and `cache` scans.
- [`plan.rs`](./plan.rs) — `Plan` and friends: the preview shown before anything changes, including removals, provider choices and warnings, and the plan's digest (its id).
- [`package.rs`](./package.rs) — `PackageSummary` / `PackageDetails`.
- [`error.rs`](./error.rs) — the error type; each error is voiced `found` (something the user can fix) or `says` (something failed underneath).
- [`config.rs`](./config.rs) — the TOML config (`flags`, `review`, `search`, `snapshot`, `build`, `cache`, `safety`, `self`); [`config_style.rs`](./config_style.rs) writes it framed and documented.
- [`voice.rs`](./voice.rs) — the twelve verbs (`found`, `says`, `warns`, …) and the only place that prints their prefix.

**Safety of AUR builds**
- [`audit.rs`](./audit.rs) — static analysis of a PKGBUILD, its scriptlets and the files shipped beside it; findings are info, warning, high or critical.
- [`manifest.rs`](./manifest.rs) — inspects a built package file: setuid files, hooks, sudoers entries, accounts, units, scripts, and the installed packages it would remove.
- [`sandbox.rs`](./sandbox.rs) — the bubblewrap argument list, the probe that checks it works, resource limits (cgroup scope, timeout, size), and the run that keeps the tail of the build's output.

- [`aur_trust.rs`](./aur_trust.rs) — trust signals for an AUR package: lookalike names, the AUR's own data, a changed maintainer, sources or keys changed since you last built it.
- [`stale_builds.rs`](./stale_builds.rs) — AUR packages a library, Python or Qt update has broken or may have broken: a small ELF reader, interpreter folders, pacman's log.

**Recovery and history**
- [`history.rs`](./history.rs) — the append-only transaction log (`~/.local/share/emrac/history.jsonl`).
- [`journal.rs`](./journal.rs) — the journal written before a transaction and locked during it: a cut-off one is found, assessed from pacman's log and recorded.
- [`history_log.rs`](./history_log.rs) — its SHA-256 chain, `verify`, the archive and the acknowledgement markers.
- [`ledger_watch.rs`](./ledger_watch.rs) — reading the kernel audit events about the ledger, the rules that produce them, and telling emrac's own appends from anything else.
- [`ledger.rs`](./ledger.rs) — the root-owned second copy: the append the helper does, reading and checking it, comparing it with the history, rebuilding the history from it.
- [`rollback.rs`](./rollback.rs) — pure planning for `rollback` / `undo` / `redo`: the inverse of a record, the post-state check, finding package files.
- [`downgrade.rs`](./downgrade.rs), [`reinstall.rs`](./reinstall.rs), [`purge.rs`](./purge.rs) — the version choice, the source choice, and the config-file classification behind those commands.
- [`snapshot.rs`](./snapshot.rs) — named restore points of the installed package set.
- [`pins.rs`](./pins.rs) — holds with a reason and an expiry.
- [`verify.rs`](./verify.rs) — compares a package's mtree with the filesystem.
- [`deps.rs`](./deps.rs) — dependency, reverse-dependency, "why" and orphan reports; the tree, graph, cycles and removal impact.

**Before and after a transaction**
- [`config_preview.rs`](./config_preview.rs) — which config files a plan leaves as `.pacnew` (new version beside your edits) or `.pacsave` (kept on removal), by pacman's three-way rule.
- [`plan_preview.rs`](./plan_preview.rs) — what a plan does beyond its packages (hooks, config files), shown under it and kept out of the plan id.
- [`hooks.rs`](./hooks.rs) — reading pacman hooks and working out which ones a plan will run, in pacman's order, and which would stop the transaction.
- [`build_tools.rs`](./build_tools.rs) — which packages an AUR build installed only to build and nothing needs any more (the fixpoint that decides).
- [`policy.rs`](./policy.rs) — the local file policy (`override`, `divert`): its file, validation, the three-checksum decision for a diverted file, applying it, the pacman hooks, the trust and location guards.
- [`policy_ops.rs`](./policy_ops.rs) — what the root helper does: change the policy, keep the hooks in step, apply, status, review.
- [`policy_preview.rs`](./policy_preview.rs) — what a plan does to files under your policy (a diversion the package changes is a warning).
- [`reconcile.rs`](./reconcile.rs) — after a transaction that failed or was cut off: packages pacman still lists whose files are missing, and the command that mends each.
- [`preflight.rs`](./preflight.rs) — space, `/boot`, power, mirrors, the kernel safety net, the partial-upgrade and older-version gates, pacman's lock.
- [`pacman_db.rs`](./pacman_db.rs) — the backup of pacman's local database and `pacman -Dk`.
- [`repo_trust.rs`](./repo_trust.rs) — repository databases older than one seen; an older version than installed.
- [`arch_news.rs`](./arch_news.rs) — the Arch news feed, unread items and what makes one critical.
- [`upgrade_report.rs`](./upgrade_report.rs) — update risk before an upgrade and the report after it.

**emrac itself**
- [`self_check.rs`](./self_check.rs) — `self check` and `self diagnose`: one engine, a fast and a deep set of checks.
- [`self_meta.rs`](./self_meta.rs) — the release index: validation, staging, the gpg check, sync, news.
- [`self_ops.rs`](./self_ops.rs) — the journal of emrac's operations on itself.
- [`self_upgrade.rs`](./self_upgrade.rs) — planning and doing `self upgrade` and `self downgrade`: through `makepkg` and `pacman -U` for the packaged copy, in place (cargo build, the old file kept, put back on failure) for a copy nobody owns.

**Housekeeping**
- [`doctor.rs`](./doctor.rs) — the health checks, each with a fix, and the sanitized report.
- [`pacnew.rs`](./pacnew.rs) — pure logic for `.pacnew` files: classifying by comparing the old shipped file, the live file and the new one, three-way merging, validators for the files that matter.
- [`pacnew_log.rs`](./pacnew_log.rs) — the record of changes `pacnew` made (`~/.local/share/emrac/pacnew-log.jsonl`).
- [`pacnew_ops.rs`](./pacnew_ops.rs) — applying and reverting a change through `sudo`, with copies kept.
- [`cache.rs`](./cache.rs) — scanning the source, build, binary and pacman caches and deciding what is safe to remove.

Persisted state lives in `~/.config/emrac/`, `~/.local/share/emrac/` and `~/.cache/emrac/`.

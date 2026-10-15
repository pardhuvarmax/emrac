# src/

`lib.rs` is the crate root: it declares the modules and re-exports the public API, and nothing else. [`sources.rs`](./sources.rs) is the intended entry point; the backends in [`backend/`](./backend) are not re-exported.

**Core model**
- [`sources.rs`](./sources.rs) — `Sources`: merges the official repositories and the AUR behind one `search`/`info` surface, plans and runs install, remove, upgrade, rollback, downgrade, reinstall, purge, restore, builds, `doctor`, and the `pacnew` and `cache` scans.
- [`plan.rs`](./plan.rs) — `Plan` and friends: the preview shown before anything changes, including removals, provider choices and warnings.
- [`package.rs`](./package.rs) — `PackageSummary` / `PackageDetails`.
- [`error.rs`](./error.rs) — the error type; each error is voiced `found` (something the user can fix) or `says` (something failed underneath).
- [`config.rs`](./config.rs) — the TOML config (`flags`, `review`, `search`, `snapshot`, `build`, `cache`).

**Safety of AUR builds**
- [`audit.rs`](./audit.rs) — static analysis of a PKGBUILD, its scriptlets and the files shipped beside it; findings are info, warning, high or critical.
- [`manifest.rs`](./manifest.rs) — inspects a built package file: setuid files, hooks, sudoers entries, accounts, units, scripts, and the installed packages it would remove.
- [`sandbox.rs`](./sandbox.rs) — the bubblewrap argument list, the probe that checks it works, resource limits (cgroup scope, timeout, size), and the run that keeps the tail of the build's output.

**Recovery and history**
- [`history.rs`](./history.rs) — the append-only transaction log (`~/.local/share/emrac/history.jsonl`).
- [`history_log.rs`](./history_log.rs) — its SHA-256 chain, `verify`, the archive and the acknowledgement markers.
- [`ledger_watch.rs`](./ledger_watch.rs) — reading the kernel audit events about the ledger, the rules that produce them, and telling emrac's own appends from anything else.
- [`ledger.rs`](./ledger.rs) — the root-owned second copy: the append the helper does, reading and checking it, comparing it with the history, rebuilding the history from it.
- [`rollback.rs`](./rollback.rs) — pure planning for `rollback` / `undo` / `redo`: the inverse of a record, the post-state check, finding package files.
- [`downgrade.rs`](./downgrade.rs), [`reinstall.rs`](./reinstall.rs), [`purge.rs`](./purge.rs) — the version choice, the source choice, and the config-file classification behind those commands.
- [`snapshot.rs`](./snapshot.rs) — named restore points of the installed package set.
- [`pins.rs`](./pins.rs) — holds with a reason and an expiry.
- [`verify.rs`](./verify.rs) — compares a package's mtree with the filesystem.
- [`deps.rs`](./deps.rs) — dependency, reverse-dependency, "why" and orphan reports.

**Housekeeping**
- [`doctor.rs`](./doctor.rs) — the health checks, each with a fix, and the sanitized report.
- [`pacnew.rs`](./pacnew.rs) — pure logic for `.pacnew` files: classifying by comparing the old shipped file, the live file and the new one, three-way merging, validators for the files that matter.
- [`pacnew_log.rs`](./pacnew_log.rs) — the record of changes `pacnew` made (`~/.local/share/emrac/pacnew-log.jsonl`).
- [`pacnew_ops.rs`](./pacnew_ops.rs) — applying and reverting a change through `sudo`, with copies kept.
- [`cache.rs`](./cache.rs) — scanning the source, build, binary and pacman caches and deciding what is safe to remove.

Persisted state lives in `~/.config/emrac/`, `~/.local/share/emrac/` and `~/.cache/emrac/`.

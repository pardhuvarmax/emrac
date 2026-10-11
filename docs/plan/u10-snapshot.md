# Slice u10 — `emrac snapshot` / `emrac restore`

## Context

`SPEC.md` lists snapshots as named restore points for the installed package
set. History (u3) already records every change, and rollback (u4) can reverse
one transaction; a snapshot is the complementary tool for "put the system
back to how it was on that day" without replaying transactions one by one.
This is a **package-set** snapshot only: no filesystem (btrfs/timeshift)
snapshots and no configuration-file contents.

## Design

- `emrac snapshot [create] [NAME] [--note TEXT]`: saves every installed
  package (name, version, explicit/dependency, official/foreign) to
  `<data dir>/emrac/snapshots/<name>.json`. Default name `snap-N` (next free
  number). Names are `[A-Za-z0-9._-]+`; an existing name is refused.
  Read-only toward the system; no root.
- `emrac snapshot list` / `show <name>` / `delete <name>`; `--json` on all.
- `emrac restore <name> [--prune]`: compares the snapshot with what is
  installed now and plans:
  - **install** packages in the snapshot that are missing now,
  - **downgrade / upgrade** packages whose version differs,
  - with `--prune` only: **remove** installed packages absent from the
    snapshot. Without it they are listed as "left alone" — restoring never
    removes something unless asked.
  Install/up/downgrades are done with one `pacman -U` using package files
  located exactly as rollback does (pacman cache, AUR build dir, emrac's
  archive cache, then the Arch Linux Archive after confirmation, never
  `--offline`); the removals follow as one `pacman -R`. If any needed file
  can't be found, the whole restore aborts before prompting, listing all of
  them (same stance as rollback).
- Packages' explicit/dependency *reason* is not restored (pacman `-U` marks
  them explicit); noted in the output. Foreign (AUR) packages missing from
  disk can only come from a cached build; otherwise they're reported.
- History: each step is recorded like any transaction (Install, Upgrade,
  Downgrade, Remove, with `previous_version`), so `emrac undo` works on the
  last step and `history` shows what a restore did.
- `--dry-run`, `--yes`, `--json`, `--offline` behave as elsewhere.

Out of scope: filesystem snapshots, config contents, automatic snapshots
before upgrades (a later `config` key), restoring pacman repo state.

## Files

- `snapshot.rs` (new, core): `Snapshot`, `SnapshotStore` (path-taking),
  name validation, pure `diff` -> `RestoreDiff`.
- `alpm.rs` — `installed_packages()`.
- `sources.rs` — `create_snapshot`, `plan_restore`, `execute_restore`.
- `error.rs` — snapshot errors.
- CLI: `commands/snapshot.rs`, `commands/restore.rs`, output printers.

# Slice u11 — automatic snapshot before `upgrade`

## Context

u10 added `snapshot`/`restore`. The natural use is "snapshot, then upgrade, then
restore if it went badly". This slice does the first step automatically, but
only when asked: a new config key.

## Design

- Config key `snapshot.before_upgrade` (bool, default `false`; the config file
  can only turn it on, like the other flags). `config get/set/unset` support it.
- When on, `emrac upgrade` saves a snapshot named `pre-upgrade-<unix time>`
  (note "before emrac upgrade") after every confirmation and review, right
  before anything is changed. Not for `--dry-run`, a declined prompt or an
  empty plan.
- Only the newest 5 `pre-upgrade-*` snapshots are kept; older automatic ones
  are deleted. Snapshots with any other name are never touched.
- A failure to save the snapshot is a warning on stderr; the upgrade goes on
  (the user can see it and ctrl-c at the next prompt only if not `--yes`, so
  the warning is printed before pacman starts).
- The line `emrac notes: saved snapshot 'pre-upgrade-…' — emrac restore <name>
  returns to it` is printed (JSON mode: stderr only, so stdout stays a plan).

Out of scope: snapshots before install/remove, configurable retention,
a flag to override the config for one run.

## Files

- `config.rs` — `Snapshot { before_upgrade }`, key plumbing, tests.
- `snapshot.rs` — `SnapshotStore::prune_prefixed`, tests.
- `sources.rs` — `auto_snapshot`.
- CLI: `main.rs`, `commands/upgrade.rs`.

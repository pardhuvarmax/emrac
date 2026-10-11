# Slice u8 — `emrac purge`

## Context

`SPEC.md` (Part V `remove / purge`): `emrac purge <pkg>...` — "also deletes
config/state files remove leaves behind". On Arch, what `remove` leaves
behind is pacman's `.pacsave` copies: when a package is removed, any file in
its `backup=()` array that the user modified is saved as `<file>.pacsave`
instead of being deleted. `pacman -Rn` (`--nosave`) skips that and deletes
them outright. That is the whole mechanism; there is no "state" pacman knows
about beyond it.

## Design

- `emrac purge <pkg>... [--cascade] [--recursive]` = `emrac remove` with
  `pacman -Rn`. Same resolution (so the plan lists dependents/orphans exactly
  as `remove` would), same `--dry-run` / `--yes` / `--json`.
- **What is shown before it happens** (the part worth building): for every
  package that will be removed, its `backup` entries are checked against the
  local db hash. A file whose current md5 differs from the recorded one is
  *modified* — `remove` would keep it as `.pacsave`, `purge` destroys it. The
  preview lists those files (per package); unmodified and already-missing
  ones are counted, not listed.
- **Extra confirmation when something would be lost.** If any modified
  config file will be deleted, a dedicated `emrac asks:` confirmation names
  the count, separate from the overall "Proceed?" (same pattern as the AUR
  PKGBUILD review). `--yes` answers both, as it does everywhere. No modified
  files → no extra prompt.
- **Not touched, deliberately**: files under `$HOME` (`~/.config/...`), which
  pacman has no record of and emrac can't safely guess; and `.pacsave` files
  left by *earlier* removals (the package is gone from the db, so which files
  belonged to it is unknowable). The docs say so.
- **History**: new `purge` action. Reversing it reinstalls the package (same
  as a `remove`, same post-state check) but the deleted configuration is
  gone — the rollback preview says so in a note.
- `--keep-config` from the spec is `remove`'s default (pacman saves modified
  configs as `.pacsave`), so it is not a flag; `remove` is unchanged.

## Files

- `backend/alpm.rs` — `config_files(names)`, a pure `classify` for
  unmodified / modified / missing.
- `backend/pacman.rs` — `execute_remove` gains `nosave`.
- `plan.rs` — `PlanAction::Purge`; `purge.rs` (new): `PurgePlan`,
  `ConfigFile`, `ConfigState`.
- `sources.rs` — `plan_purge`, `purge`.
- `rollback.rs` — treat `Purge` as `Remove` for the inverse, plus a note.
- CLI: `cli.rs`, `commands/purge.rs`, `output.rs`, `main.rs`.
- Docs: READMEs, `FEATURE_STATUS.rst`, `CHANGELOG.md`, plan index.

## Verification

Unit: classification (matches / differs / missing / unreadable), hash check
against a temp root, plan assembly, rollback of a purge (inverse install
+ note). Host: `--dry-run` on installed packages, `--json`; nothing removed.
Container (real pacman): modify a package's config file, `remove` leaves
`.pacsave`, `purge` of a modified-config package shows the file, asks the
extra confirm (declined once = nothing removed), `--yes` removes it and no
`.pacsave` remains; purge of a package with only unmodified config; cascade
listing; history shows `purge`; `undo` reinstalls with the note.

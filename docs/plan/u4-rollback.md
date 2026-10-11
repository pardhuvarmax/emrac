# Slice u4 — rollback and undo

## Context

`u3` records every executed `install`/`remove`/`upgrade` in
`~/.local/share/emrac/history.jsonl`. `SPEC.md` (Part V, `transactions /
history / rollback / undo / redo / snapshot / restore`; lifecycle diagram
3: "Failed → RolledBack") asks for `emrac rollback [transaction]` and
`emrac undo`. This slice reverses a recorded transaction. Not in scope:
`redo` (falls out of the same machinery later), `snapshot`/`restore`
(point-in-time package sets, Btrfs/ZFS), automatic rollback on failure, and
fetching old packages from the Arch Linux Archive.

## Semantics

A rollback applies the **inverse** of a recorded transaction:

| Recorded | Inverse | Mechanism |
|---|---|---|
| `install` (packages new to the system) | remove them | `sudo pacman -R <names>` |
| `remove` (packages that were installed at version V) | reinstall at V | `sudo pacman -U <files>` |
| `upgrade` (package went P → N) | downgrade to P | `sudo pacman -U <files>` |

`rollback` with no argument and `undo` both mean "the most recent
transaction that can still be rolled back"; `rollback tx-N` names one.
`undo` is a separate subcommand (the spec lists both) that behaves exactly
like a bare `rollback`.

### Post-state check (the safety rule)

A rollback only touches a package if it is **still in the state the
transaction left it in**: for install/upgrade the installed version must
equal the recorded new version; for remove the package must be absent.
Anything else (upgraded again later, already removed, reinstalled) is
listed as *skipped, with the reason*, and left alone. This one rule covers
three cases at once: later transactions that built on this one, a user who
changed things outside emrac, and **failed transactions** — a `failed`
record may have applied only part of its plan, and the check reverses
exactly the part that landed. If nothing is left to reverse, say so and
exit 0.

### Availability check

Old versions must come from files already on disk, found by exact
`<name>-<version>-<arch>.pkg.tar.*` match in (a) pacman's `CacheDir` (read
via `pacman-conf CacheDir`, the same tool `emrac` already shells out to) and
(b) emrac's own AUR build dir `~/.cache/emrac/build/<pkg>/` (makepkg leaves
its output there). The match is on the full name/version/arch shape, so
`foo` never matches `foo-bar`. If **any** needed file is missing, the whole
rollback aborts before prompting, listing each missing `name version` — a
half-applied downgrade is worse than none. Removal needs no files.

### Recording and refusing repeats

A rollback is itself a transaction in the history: new field
`rollback_of: Option<String>` on `TransactionRecord` (absent on old
records via `#[serde(default)]`). A record is *rolled back* when a
`committed` record with `rollback_of == its id` exists. Rolling back an
already-rolled-back transaction, or a rollback record itself (that is
`redo`, deferred), is refused with an explanation. `emrac history` marks
such records (`rolled back by tx-N` / `rollback of tx-N`).

### Data the log needs (the reason this is a schema change)

To downgrade, the log must know the **previous** version.
`PlannedPackage` gains `previous_version: Option<String>`
(`#[serde(default, skip_serializing_if = "Option::is_none")]`), filled at
plan time from the local db for upgrades; `None` for installs. Records
written by `u3` have no previous version, so **rolling back an old
`upgrade` record is refused** ("recorded before rollback support"); old
`install`/`remove` records work, since they don't need it. `PlanAction`
gains `Downgrade` so a rollback's own record/plan reads honestly (an
upgrade's inverse is a downgrade); install's inverse is `Remove`, remove's
is `Install`.

## CLI

`emrac rollback [TX]` and `emrac undo`, honoring the existing global
`--dry-run`, `--yes`, `--json`. Flow identical to the other mutating
commands: build the rollback plan, print it (plus the skipped list), stop on
empty/`--dry-run`, confirm unless `--yes`, execute, record. Invalid TX
syntax (not `tx-<number>`) and unknown ids are `found`-voiced errors. Sizes
are not computed for rollback plans and are omitted from the printed plan.
Dispatch needs libalpm (installed versions), so unlike `config`/`history`
this runs after `Sources::init()`.

## Code layout

- `crates/emrac-core/src/rollback.rs` — **pure planning logic**, no I/O of
  its own: `plan_rollback(record, history, installed_version, find_file)
  -> RollbackPlan { plan, files, skipped, missing }` taking the two
  lookups as closures so every rule above is unit-testable without alpm or
  pacman. Plus `find_package_file(dirs, name, version)` (filesystem scan,
  tested against a temp dir) and `eligible_last(history)`.
- `crates/emrac-core/src/history.rs` — `rollback_of`, helpers
  `rolled_back_by(id)`, `find(id)`; record API takes an optional
  `rollback_of`.
- `plan.rs` — `previous_version`, `PlanAction::Downgrade`.
- `sources.rs` — fill `previous_version` in upgrade planning;
  `installed_version(name)`; `plan_rollback(tx)` /`execute_rollback(...)`.
- `backend/pacman.rs` — `execute_install_files(&[PathBuf])` (`-U`),
  reuse `execute_remove` (no cascade/recursive), `cache_dir()`.
- `error.rs` — `UnknownTransaction`, `InvalidTransactionId`,
  `NotRollbackable { id, why }`, `RollbackFilesMissing(Vec<..>)`.
- CLI: `cli.rs`, `commands/rollback.rs`, `output.rs` (Downgrade label,
  skipped list, history markers), `main.rs`.
- Docs: READMEs, `FEATURE_STATUS.rst` #1, `CHANGELOG.md`, plan index.

## Open decisions (defaults chosen, easy to flip)

- `--cascade`-style removal on install-rollback is **off**: if something now
  depends on the packages being removed, pacman refuses and the failure is
  recorded. Safer than silently removing the user's newer packages.
- Skipped packages do not make the command fail; it exits 0 and reports.
- Dependencies pulled in by an install are removed too (they are in the
  recorded plan with `explicit: false`), unless post-state says they changed.

## Verification

Unit (host, no pacman): every row of the inverse table; post-state skip for
each case (upgraded again, already removed, missing); failed-record partial
application; old `upgrade` record refused, old `install`/`remove` allowed;
double rollback refused; rollback-of-rollback refused; `eligible_last`
ordering; `find_package_file` exact-match incl. `foo` vs `foo-bar`, `.sig`
ignored; missing files aggregated; TX id parsing; serde compatibility with
u3-format lines.

Container (`dev/container/run.sh` build, real pacman): (1) `install` a
package then `undo` → gone, history shows `rollback_of`. (2) `remove` then
`rollback` → back, from the pacman cache. (3) Put an older package file in
the cache (fetched once from the Archive inside the container), install it
with `pacman -U`, `emrac upgrade`, then `undo` → old version restored.
(4) `undo` twice → second refused. (5) Upgrade again after (3), then try to
roll back the first upgrade → skipped with reason. (6) Delete the cached
file, roll back → aborts before prompting, naming the missing file. (7)
`--dry-run` changes nothing and records nothing.

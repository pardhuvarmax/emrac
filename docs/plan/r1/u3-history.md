# Slice u3 — transaction history

## Context

`FEATURE_STATUS.rst` #1 lists "transaction history" as not started, and
`SPEC.md` (Part IV #1, Part V `transactions / history / rollback`, lifecycle
diagram 3: "Log to transaction history") says every mutating operation is
an explicit, recorded transaction. Today `install`/`remove`/`upgrade` leave
no trace beyond pacman's own log. History is also the prerequisite for
`rollback`, `undo` and resume-interrupted, so it comes first.

This slice only **records and shows** history. `rollback`/`undo`/`redo`,
snapshots, and resume are later slices that build on the log.

## Design

- **Storage**: append-only JSON Lines at
  `$HOME/.local/share/emrac/history.jsonl` (same `$HOME`-only resolution as
  config and the build cache; no `XDG_DATA_HOME`). One record per line,
  written with a single `write_all` in append mode, so a crash can lose at
  most the last line and never corrupts earlier ones. `serde_json` is added
  to `emrac-core`.
- **Record**: `{ id: "tx-<N>", timestamp (unix secs, UTC), action
  (install|remove|upgrade), packages [{name, version, repo, explicit}],
  status (committed|failed), error? }`. `N` = highest existing id + 1.
  The packages are the *plan* that was executed, so history shows what was
  intended; for a partly-failed run (official part succeeded, AUR build
  failed) the record is `failed` with the error text — finer per-package
  status is future work.
- **What is recorded**: only executed transactions. `--dry-run`, declined
  confirmations and empty plans execute nothing and write nothing (the
  spec's "Aborted" state is deliberately not logged).
- **Recording never fails the command.** If the history file can't be
  written, the CLI prints a warning to stderr and returns the transaction's
  own result unchanged.
- **Reading is tolerant**: lines that don't parse are skipped, so one bad
  line (or a record from a future version) can't brick `emrac history`. A
  missing file is an empty history.
- **Command**: `emrac history [--limit N] [--since YYYY-MM-DD]`, with
  `transactions` as a visible alias (spec lists both). Newest first.
  `--json` prints the records. Dispatched before `Sources::init()`.
  `--since` is a UTC calendar date; times are displayed in UTC (no
  `chrono`/`time` dependency: a small civil-date conversion lives in
  `history.rs`, unit-tested against known timestamps).
- **Testability**: like `Config`, path-taking `append_to`/`read_from`
  helpers, with the `$HOME`-resolving methods as thin wrappers.

## Files

- `crates/emrac-core/Cargo.toml` — `serde_json = "1"`.
- `crates/emrac-core/src/history.rs` — new: `TransactionRecord`,
  `TxStatus`, `History::{record, list}`, date helpers.
- `crates/emrac-core/src/plan.rs` — derive `Deserialize` on `PlanAction`,
  `PlannedPackage`.
- `crates/emrac-core/src/error.rs` — `NoDataDir`, `HistoryWrite`,
  `InvalidDate`.
- `crates/emrac-core/src/lib.rs` — export.
- `crates/emrac-cli/src/cli.rs`, `main.rs`, `commands/history.rs`,
  `commands/mod.rs`, `output.rs` — the command and its table output.
- `commands/{install,remove,upgrade}.rs` — record after execution.
- Docs: READMEs, `FEATURE_STATUS.rst` #1, `CHANGELOG.md`, plan index.

## Verification

- Unit tests: append/read round trip, newest-first + `--limit`, `--since`
  filtering, id increment, tolerant read of a corrupt line, missing file,
  date conversion against known timestamps, bad `--since` rejected.
- Host (scratch `$HOME`, no pacman state touched): `emrac history` on an
  empty history; `--dry-run` install writes nothing; `--json` output.
- Container (`dev/container/run.sh`): a real install and remove each add a
  record; a failing install (nonexistent AUR build) adds a `failed` one;
  `emrac history` and `--since`/`--limit` show them correctly.

# Slice u10 — a transaction that was cut off is not lost

Everything emrac records (u3 history, u7 chain, u8 ledger, u9 watch) is written **after** a
transaction ends. If the power goes, the terminal is closed, the process is killed or the
machine hangs in the middle of `pacman -Syu`, nothing is written: the history has no line, the
ledger has none, `rollback` has nothing to reverse, and the user does not know what state the
system is in. `FEATURE_STATUS.rst` has said "no resume of an interrupted transaction" since
r1. This slice does not resume anything (pacman's own database is the authority on what
happened); it makes sure that **a cut-off transaction is found, described from evidence, and
put on record**.

## The journal

A small file, `~/.local/share/emrac/journal.json`, written **before** emrac hands a plan to
pacman (or builds an AUR package) and removed **after** the result is recorded:

- `started` (unix seconds), `action`, the planned packages with their versions, what was
  installed before (name and version of each planned package), emrac's version.
- `pacman_log_offset`: the size of `/var/log/pacman.log` when the transaction started. Whatever
  pacman wrote after that point belongs to this run (or to a person running pacman by hand at
  the same time, which the report says).
- Written under the lock and `fsync`ed, mode `0600`. A file cut off while it was being written
  cannot be read as a journal; it is still reported as an interruption, with "nothing is known".
- **Liveness is a lock, not a process id.** emrac holds an exclusive `flock` on the file for
  the whole transaction. The kernel releases it when the process dies for any reason, a
  power loss included, and a process id can be reused. So "the file exists and can be locked"
  means *the run that wrote it is gone and never finished*; "the file exists and is locked"
  means *another emrac is running a transaction right now*.
- A second emrac that finds the lock held refuses to start a mutating command ("another emrac
  is changing packages: wait for it"). pacman has its own lock (`db.lck`), but that is held
  only while pacman itself runs, not while emrac builds an AUR package in between.

## What happens next time

Every mutating command (`install`, `remove`, `purge`, `upgrade`, `reinstall`, `downgrade`,
`rollback`/`undo`/`redo`, `restore`) starts with a check. A leftover journal that can be locked
is an interrupted transaction, and emrac:

1. Reads `/var/log/pacman.log` from the saved offset and finds what pacman actually did:
   `installed`, `upgraded`, `downgraded`, `removed` lines, and any `transaction started` with no
   matching `transaction completed`.
2. Compares the installed versions now with the plan and with the "before" list. Each planned
   package ends up **done** (the planned version is installed or the package is gone), **not
   reached** (still the old version) or **other** (a version that is neither: something else
   changed it).
3. Says what it found in the `suspects` voice, listing the three groups, and notes a stale
   `/var/lib/pacman/db.lck` if present (pacman will refuse to run until it is removed; emrac
   never removes it for the user, `doctor` already gives the command).
4. **Records it in the history** as a failed transaction whose error starts with
   `interrupted:` and carries the three groups. It goes through the same chain and, with
   sudo, into the ledger, so the cut-off is part of the tamper-evident record. No schema
   change: `status` stays `failed`, so `history ack`, `doctor`'s failed-transaction count and
   `rollback` behave as they do for any other failure.
5. Deletes the journal and carries on with the command the user asked for.

If nothing happened (the plan was cut off while emrac was still building, before pacman was
called), the record says "nothing was installed or removed" and is still written: it is a
fact about the run.

`emrac history recover` does steps 1 to 5 on demand, without any other command, and with
`--dry-run` only prints the findings. `doctor` has an "Interrupted transaction" check:
a warning with `emrac history recover` as the fix; read-only, it never records.

## Limits (said in the docs)

- It cannot tell a half-written package file from a good one. When the log says pacman was in
  the middle of a package, the report says so and recommends `emrac verify <pkg>` for it
  (u9); it does not guess.
- It is not a resume. Nothing is retried. The plan was a prediction made on one date; the
  report tells the user what is left, and the user decides.
- A journal on a disk that was itself lost in the crash is gone with it. The ledger cannot
  help: it only holds what was recorded.
- Two things on the same machine are not told apart: pacman run by hand while emrac was
  working shows up in the same log range.

## Code

- `crates/emrac-core/src/journal.rs`: `begin(plan) -> Guard` (writes the file, takes the
  lock, stores itself for `finish`), `finish()` (removes it, called by `history::record`),
  `find_interrupted() -> Option<Interrupted>` (the leftover, lockable), `Interrupted::assess()`
  (pacman log + installed versions → done / not reached / other), `parse_pacman_log(text,
  from_offset)`. `Error::Journal(String)`.
- `History::record_interrupted(&Interrupted)` writes the failed record.
- CLI: `commands/history.rs` gains `begin`, a `check_interrupted()` that every mutating command
  calls first, and `recover`; `cli.rs` gets `history recover`.
- `doctor.rs`: the "Interrupted transaction" check.
- Docs: a section in `docs/design/` (what a cut-off leaves, what is recovered), README line,
  `FEATURE_STATUS.rst`, `CHANGELOG.md`.

## Verification

- Unit tests: the pacman log parser (start/completed, installed/upgraded/removed/downgraded,
  a log cut in the middle of a transaction, a log shorter than the offset after rotation); the
  assessment (done / not reached / other); the lock (a held journal is "running", a free one
  is "interrupted"); the journal is atomic and `0600`; `record_interrupted` keeps the chain
  valid and the id sequence.
- Container: run `emrac install` of a real package under `timeout -s KILL` at several points
  (before pacman, during the download, during the install), then check that the next emrac
  command reports and records exactly what pacman's log shows, that `history verify` is
  clean, and that a normal transaction leaves no journal behind.

## What was built, and where it differs

- As planned: `journal.rs`, the lock-as-liveness, the pacman-log offset, `history recover` (with
  `--dry-run` and `--json`), the automatic record by the next mutating command, the `doctor`
  check, `History::record_interrupted` (a failed record whose error starts `interrupted:`).
- The journal is begun by `history::begin`, which every mutating command calls just before it
  runs (`install`, `upgrade`, `remove`, `purge`, `reinstall`, `downgrade`, `rollback`/`undo`/`redo`,
  `restore`). A restore is one journal for all its steps; each step is still recorded as it ends.
- Failing to write the journal never blocks a transaction (a warning says that a cut-off would not
  be known); a lock held by another emrac does.
- `doctor` opens the journal read-only and takes no write path.
- Container run: `emrac install` killed with `kill -9` after the journal appeared; `doctor` named
  it, `history recover` recorded it (pacman's child had finished the install: "done: tree"),
  `history verify` stayed clean, and the next transaction recorded a second cut-off by itself.

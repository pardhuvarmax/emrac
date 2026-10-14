# Slice u6 — history that can be archived, acknowledged and checked

`emrac history` is an append-only file, `~/.local/share/emrac/history.jsonl`, written by
emrac and nothing else. "Append-only" is a convention: the file is the user's own, so
the user or any program running as the user can edit or delete it, and emrac would not
notice. `undo`, `redo` and the cache policy all trust what is in it. This slice adds
three things at the user level; u7 adds a second, tamper-resistant copy.

## What stays true

A single transaction is never edited or deleted by emrac. `undo` and `redo` check the
system against each record, the cache policy protects what recent records name, and a
failure is information. Nothing here offers "delete this transaction".

## 1. Archive: `emrac history archive --older-than 90d`

Moves old records out of the live log into `~/.local/share/emrac/history-archive/<from>-<to>.jsonl`
so the live log stays small.

- **Never moves what is still needed:** anything newer than the window, and anything
  `undo` or the cache policy would use (the records `cache clean --lean` keeps, anything
  `emrac undo` would step through next). `--older-than` below the cache's own window is
  refused with the reason.
- **Leaves a marker record in the live log** (`type: "archive"`): how many records, the
  first and last id and time, the archive file and its SHA-256, and the highest id used so
  far. Ids keep counting from that number, so an id is never reused even if every record
  moved.
- **Asks first** (default no); `--yes` does not answer; a closed stdin is no; `--dry-run`
  lists what would move. The archive is written and verified (read back, hash compared)
  before the live log is rewritten, and the rewrite is a rename, so a crash leaves either
  the old log or the new one, never half of each.
- `history` shows the marker as one line ("12 transactions, tx-3 to tx-14, archived") and
  `history --archive` reads the archived files.

## 2. A hash chain

Every record carries `seq` (1, 2, 3, …) and `prev`: the SHA-256 of the previous line, byte
for byte as written. A hand edit changes a line's hash and breaks the next record's
`prev`; a deleted, inserted or reordered line breaks the chain at that point.

- Markers (archive, acknowledge) are chained like transactions.
- **Existing logs:** records written before this slice have no chain. The first new
  record starts it with a `chain-start` marker that names how many earlier lines there are
  and the hash of the last of them; `doctor` says "N earlier records predate the chain".
- A SHA-256 implementation is needed in the core library (it is shelled out to
  `sha256sum` in `pacnew`, which is fine for one file and wrong for thousands of lines):
  the permissively licensed `sha2` crate, added to `Cargo.lock`.
- `emrac history verify` walks the live log and the archive files and reports the first
  break, with the ids on either side. `doctor` gets a "History integrity" check: ok, or a
  warning naming the break. A broken chain does not stop any command: it is evidence, not
  a lock.

## 3. Acknowledge instead of delete: `emrac history ack`

`emrac history ack tx-3 tx-5` (or `--failed` for every failed transaction not yet
acknowledged) appends an acknowledgement marker (`type: "ack"`, the ids, the time). The
records stay; `doctor`'s "N transactions failed in the last 7 days" stops counting them
and `history` shows `FAILED (acknowledged)`. An acknowledgement is a chained record
like any other, so hiding a failure leaves a trace.

## Not covered

A second copy that the user cannot edit (u7); signing records with a key; editing the
archive files (they are hashed in the marker, so a change is detected, but not prevented);
`history ack` of anything but failed transactions.

## Verification

Unit tests: the chain (a changed field, a deleted, inserted and reordered line, a
truncated tail, a legacy log joined by `chain-start`), id continuity after archiving the
whole log, the archive window never touching a record `undo` needs, `ack` hiding a failure
from the doctor count and not from `history`. Container, against a real pacman: a few real
transactions, then `archive` (dry run, declined, accepted), `undo` of a recent transaction
and `cache clean --lean` giving the same answers before and after, an edit to the log
caught by `history verify` and `doctor`, and `ack` followed by `doctor`.

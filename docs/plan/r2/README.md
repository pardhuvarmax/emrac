# plan/r2/

Plans for the slices of release cycle **r2**: the updates made after the `r2` release, numbered
again from `u1`. They go into the next release (`r3`). `u1` to `u5` had no plan of their own
(tests, license, install script). Newest last.

- [`u6-voice.md`](./u6-voice.md) — emrac's voice: twelve verbs (says, found, notes, plans, asks,
  warns, suspects, thinks, suggests, refuses, confirms, remembers) that tell the reader how sure
  emrac is and what to do with a line.
- [`u7-history-integrity.md`](./u7-history-integrity.md) — history you can archive, acknowledge and
  check: `history archive`, a hash chain, `history ack`.
- [`u8-ledger.md`](./u8-ledger.md) — a second, root-owned append-only copy of the history that the
  user's own programs cannot rewrite.
- [`u9-ledger-watch.md`](./u9-ledger-watch.md) — the kernel audit system watches the ledger, emrac
  reports any write that is not its own, and chain anchors catch a rewrite.

- [`u10-journal.md`](./u10-journal.md) — a journal written before a transaction and a lock held
  during it, so a transaction cut off by a crash or a power loss is found next time, described
  from pacman's log and the installed versions, and put on record (`history recover`).
- [`u11-self-check.md`](./u11-self-check.md) — `emrac self check`: a read-only look at emrac's own installation that needs no pacman, config or network.

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
- [`u12-self-service.md`](./u12-self-service.md) — u12–u15: the release index, `self sync`/`news` (u12), `self diagnose` (u13), `self upgrade` (u14), `self downgrade` (u15).
- [`u16-dependency-views.md`](./u16-dependency-views.md) — `tree`, `graph` (text, dot, json), `impact` (remove, install, upgrade) and `cycles`.
- [`u17-preflight.md`](./u17-preflight.md) — checks before a transaction changes anything (space, `/boot`, power, network, the kernel safety net) and the partial-upgrade gate.
- [`u18-plan-integrity.md`](./u18-plan-integrity.md) — plan ids, `--expect-plan`, and a re-check of the plan before the commit.

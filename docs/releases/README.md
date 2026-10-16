# releases/

One document per release, named after its tag (`r1.md`, `r2.md`, `r2-aur.md`, `r3.md`). Each is the full release notes — what is new, what changes for someone updating, requirements, how the release was checked, known limits — and is also the message of the annotated tag, so it travels with the tag:

```sh
git tag -a r2-aur -F docs/releases/r2-aur.md --cleanup=verbatim
```

- [`r3.md`](./r3.md) — "Emrac Third Release : R3" (tag `r3`, 2026-10-15, version 0.3.0): r2 plus the update slices u1 to u20 built after it — a checkable history and ledger, a journal for cut-off transactions, pre-flight checks and plan ids, AUR trust signals and the Arch news gate, `rebuild`, and emrac updating itself through pacman.
- [`r2.md`](./r2.md) — release r2 (2026-10-14): r1 plus the update slices u1 to u30.
- [`r2-aur.md`](./r2-aur.md) — "Emrac First AUR Release : R2-AUR" (tag `r2-aur`, 2026-10-14): the r2 source plus a license, the tracked test list and the READMEs, for the first AUR package.
- [`r1.md`](./r1.md) — the first release (tag `r1`, 2026-10-13, version 0.1.0): the core loop over the official repositories and the AUR, plus undo, snapshots, pins and dependency commands (u1–u14). Written afterwards; the tag itself carries a one-line message.

The day-to-day record is the root [`CHANGELOG.md`](../../CHANGELOG.md); a release document summarizes a release from it.

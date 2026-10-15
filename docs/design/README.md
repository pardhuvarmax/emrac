# design/

Design documents that settle how a part of emrac works before or while it is built.

- [`source-build.md`](./source-build.md) — the source-build path for AUR packages: the build/install split, the audit and its severity policy (info and warning are shown; high needs an explicit answer that `--yes` does not give; critical is refused), the sandbox, and the owner's decisions on each. The slices u15 to u20 implement it; their plans are in [`../plan/`](../plan).
- [`journal.md`](./journal.md) — what emrac does when a transaction is cut off: the journal, the lock, `history recover`.
- [`release-index.md`](./release-index.md) — how emrac learns about its own releases: the index, its checks, signatures, publishing.
- [`self-upgrade.md`](./self-upgrade.md) — `emrac self upgrade` and `self downgrade`: what they do, what they refuse, what a cut-off leaves.

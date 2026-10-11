# plan/

One markdown file per slice/feature increment, written before
implementation starts — see the root [`SPEC.md`](../../SPEC.md) Part X for
what a "slice" is (`i<N>`/`r<N>`/`u<N>`). A plan records the design
decisions and the concrete file-by-file diff *before* code changes them,
so it stays useful as a record even after the slice ships.

Naming: `<slice>-<short-name>.md`, e.g. [`u1-config.md`](./u1-config.md).

Current:

- [`u1-config.md`](./u1-config.md) — persisted config file (`emrac config
  ...`), the first increment after the `r1` core CLI loop milestone.
- [`u2-completion.md`](./u2-completion.md) — `emrac completion
  bash|zsh|fish`, static shell completion generated from the clap definition.
- [`u3-history.md`](./u3-history.md) — persisted transaction log and
  `emrac history`/`transactions`; the groundwork for rollback.
- [`u4-rollback.md`](./u4-rollback.md) — `emrac rollback [tx]` / `emrac undo`:
  inverse of a recorded transaction, guarded by a post-state check.
- [`u5-archive-fetch.md`](./u5-archive-fetch.md) — rollback fetches
  missing official package versions from the Arch Linux Archive.
- [`u6-downgrade.md`](./u6-downgrade.md) — `emrac downgrade <pkg> [version]`,
  built on the rollback file/Archive machinery.
- [`u7-reinstall.md`](./u7-reinstall.md) — `emrac reinstall <pkg>...`: the
  same installed version again, without a partial upgrade.
- [`u8-purge.md`](./u8-purge.md) — `emrac purge <pkg>...`: remove without
  `.pacsave`, with a preview of the modified configs it will destroy.

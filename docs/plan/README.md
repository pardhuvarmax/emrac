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

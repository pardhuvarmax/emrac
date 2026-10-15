# plan/

One markdown file per slice/feature increment, written before implementation starts — see the
root [`SPEC.md`](../SPEC.md) Part X for what a "slice" is (`i<N>`/`r<N>`/`u<N>`). A plan records the
design decisions and the concrete file-by-file diff *before* code changes them, so it stays
useful as a record even after the slice ships.

Plans are grouped by the release cycle they belong to, because the `u` counter starts again at
`u1` after every release:

- [`r1/`](./r1) — `u1` to `u30`, the updates between `r1` and `r2`.
- [`r2/`](./r2) — `u6` to `u20` after `r2` (`u1`–`u5` had no plan), the updates that went into `r3`.
- [`r3/`](./r3) — the updates after `r3` (none yet).

Naming: `<slice>-<short-name>.md`, e.g. [`r1/u1-config.md`](./r1/u1-config.md).

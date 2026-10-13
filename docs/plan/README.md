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
- [`u9-verify.md`](./u9-verify.md) — `emrac verify <pkg>...`: installed files
  vs. pacman's mtree record.
- [`u10-snapshot.md`](./u10-snapshot.md) — `emrac snapshot` / `emrac restore`:
  named package-set restore points and a diff-driven restore.
- [`u11-auto-snapshot.md`](./u11-auto-snapshot.md) — `snapshot.before_upgrade`: automatic
  restore point before `emrac upgrade`.
- [`u12-deps.md`](./u12-deps.md) — `emrac deps`/`rdeps`/`why`/`orphans`: read-only
  dependency inspection.
- [`u13-pin.md`](./u13-pin.md) — `emrac pin`/`unpin`: holds with reasons and expiry,
  honoured by `upgrade`.
- [`u14-hardening.md`](./u14-hardening.md) — broken-pipe fix, CLI integration tests, CI,
  exit codes and quickstart ahead of r1.

## Design

- [`../design/source-build.md`](../design/source-build.md) — proposed design for the source-first
  build path (audit, fetch/build split, bubblewrap sandbox, manifest, cache).
- [`u15-audit.md`](./u15-audit.md) — `emrac audit`: static PKGBUILD analysis with a
  severity policy in the AUR review.
- [`u16-audit-coverage.md`](./u16-audit-coverage.md) — audit reads every file in an AUR
  repo and catches more obfuscation.
- [`u17-manifest.md`](./u17-manifest.md) — build/install split and a manifest of what a built
  package changes, with the audit's severity policy.
- [`u18-close-gaps.md`](./u18-close-gaps.md) — packaged scripts, cross-file exfiltration, opt-in
  inspection of official packages, and the verification gaps.

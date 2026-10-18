# Slice u6 (after r3) — what pacman leaves behind when a transaction fails

Found while failing pacman on purpose (`research/pacman-commit-order.md`): a package whose extraction failed is
still recorded as installed, its install script ran, and the run says "no packages were upgraded". emrac's journal
covers a run that was cut off; nothing covered a run that *ended* with a lie.

Idea source: dpkg has `--audit` and a "half-installed" status for exactly this (`Status: install ok half-installed`)
and rpm marks the transaction incomplete; pacman has neither. This is a native, report-only version: it names
the package and the command, and changes nothing.

## Design

- `reconcile::after_failure(&Plan) -> Vec<Damaged>`: for each package of the plan that pacman's database lists,
  the files it records that are not on disk (not directories, not `backup=`, not `NoExtract`). Read-only.
- `AlpmBackend::recorded_files` and `root`; the check is a pure function (`missing_files`) with unit tests.
- Called from the CLI's `log_step` when the outcome is an error, and from `history recover`.
- Wording: `Damaged::headline` and `Damaged::fix` (reinstall for an install or upgrade, remove again for a removal).

## Not covered

Truncated or corrupt files (`emrac check`), packages the failed plan did not name, AUR builds that failed before
pacman ran (nothing is recorded then), automatic repair.

## Verification

Unit tests (missing-file rules, wording); `dev/e2e/recovery.sh` with real pacman and an injected ENOSPC.

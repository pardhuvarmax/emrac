# Slice u6 — `emrac downgrade`

## Context

`SPEC.md` lists "Package downgrades" as a feature (Part III §1, and the
conflict resolver's `[1] Downgrade`) but defines no command for it. `u4`/`u5`
can already downgrade as the *inverse of a recorded upgrade*, including
fetching the old file from the Arch Linux Archive. `downgrade` exposes that
machinery directly: go back to a specific version, or one step back,
without needing a prior emrac upgrade on record.

## Design

```
emrac downgrade <pkg> [<version>]     # one step back, or to <version>
emrac downgrade <pkg> --list          # versions available to go back to
```

- **One installed package per call.** Not installed → `found`-voiced error
  pointing at `install`.
- **Only older versions.** `<version>` must compare older than what's
  installed (libalpm `vercmp`, so epochs/pkgrel are right); equal or newer
  is refused with a message naming both. Going forward is `upgrade`.
- **Where versions come from.** Package files already on disk (pacman
  `CacheDir`, the AUR build dir, `~/.cache/emrac/archive/<pkg>`) and, unless
  `--offline` or the package is foreign/AUR, the Archive's listing. Local
  files win when a version is in both, so nothing is downloaded needlessly.
  Version-less `downgrade` picks the **newest version older than the
  installed one** from that union.
- **Same plan → confirm → execute → record flow** as the other mutating
  commands: preview (`downgrade 1 package`, `2.3.2-1 → 1.8.0-2`, plus the
  "Will download from the Arch Linux Archive" section when applicable),
  `--dry-run`, `--yes`, `--json`. Executed through the existing fetch +
  `sudo pacman -U` path. pacman itself refuses a downgrade that breaks a
  dependent, and that failure is recorded.
- **History**: recorded as a normal `downgrade` transaction with the
  previous version filled in, so `emrac rollback`/`undo` can reverse it (the
  inverse of a recorded downgrade is an upgrade — already implemented in
  u4's redo work) and `history` shows it.
- **After success** one note: the next `emrac upgrade` will bring the
  package forward again; add it to `IgnorePkg` in `pacman.conf` to hold it.
  emrac does not edit `pacman.conf` (that is a separate, deliberate
  feature).
- **AUR (foreign) packages** work only from local files (no Archive); a
  missing version is reported as unavailable.
- **Version parsing from file names**: `<name>-<pkgver>-<pkgrel>-<arch>.pkg.tar.<ext>`
  with `pkgver` never containing `-`, so `foo` does not pick up `foo-bar`
  files. Same exact-shape matcher the rollback scan uses, extended to
  return the version.

Out of scope: multi-package downgrades, `reinstall` (separate, small),
pinning/holding, interactive version picker.

## Files

- `rollback.rs` — `package_file_version(file_name, name)`.
- `backend/archive.rs` — `versions(name)` sharing the listing fetch with
  `find`.
- `downgrade.rs` (new, pure): `choose_target(installed, requested,
  available)`, `VersionList` helpers; unit tested.
- `sources.rs` — `plan_downgrade`, `list_versions`, shared
  fetch-before-install helper used by `execute_rollback` and
  `execute_downgrade`.
- `error.rs` — `PackageNotInstalledForDowngrade`, `NotOlder`,
  `NoOlderVersion`, `VersionUnavailable`.
- CLI: `cli.rs`, `commands/downgrade.rs`, `output.rs`, `main.rs`.
- Docs: READMEs, `FEATURE_STATUS.rst`, `CHANGELOG.md`, plan index.

## Verification

Unit: file-name version parsing (epochs, `foo` vs `foo-bar`, `.sig`/`.part`
ignored); `choose_target` (previous version, explicit version, newer/equal
refused, nothing older, unavailable, vercmp ordering such as `1.10` vs
`1.9` and epochs); plan building keeps `previous_version`.

Host (read-only): `--list` and a `--dry-run` downgrade of a real installed
package; nothing written. Container (real pacman): upgrade-free downgrade of
`tree` by one step and to an explicit version, with files absent locally
(Archive) and present locally; newer/equal version refused; history shows
it; `undo` reverses it; unknown version reported; pacman refusing a
dependency-breaking downgrade is recorded as `failed`.

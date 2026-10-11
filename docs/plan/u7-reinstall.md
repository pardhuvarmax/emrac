# Slice u7 — `emrac reinstall`

## Context

`SPEC.md` (Part V `install / reinstall / upgrade`) lists
`emrac reinstall <pkg>...`. Typical reasons: repair a package whose files
were damaged or deleted, or re-apply a package's install scripts. Build
options in the spec (`--source`, `--clean`, `--profile`, ...) belong to the
build system, which doesn't exist yet and is out of scope; this slice is
the base command only.

## Design

`emrac reinstall <pkg>...` puts the **same version that is installed** back,
for installed packages only. It must not silently upgrade: with a plain
`pacman -S pkg` a package whose repo version has moved on would be
upgraded on its own, which is a partial upgrade (unsupported on Arch). So
each package picks its source in this order:

| Case | Source | How |
|---|---|---|
| a package file for exactly the installed version is on disk (pacman cache, AUR build dir, emrac archive cache) | `file` | `sudo pacman -U <files>` |
| official, repo version == installed version | `repository` | `sudo pacman -S <names>` |
| official, repo has moved on, not on disk, Archive has it (not `--offline`) | `archive` | download after confirmation, then `-U` |
| foreign/AUR, no file, an emrac checkout exists | `rebuild` | `makepkg -si` on the existing checkout (already reviewed; no new PKGBUILD is fetched or shown) |
| anything else | — | the whole command aborts before prompting, naming each package and why |

- **Installed only.** Any name not installed aborts, listing all of them,
  and points at `install`.
- Same plan → preview → `--dry-run` / confirm / `--yes` → execute → record
  flow as the other mutating commands, `--json` supported. The preview
  shows each package's source.
- **History**: a new `reinstall` action. It changes no versions, so there is
  nothing to roll back: `rollback tx-N` on one is refused with that
  explanation, and bare `undo` skips reinstall records (it undoes the last
  transaction that actually changed something).
- Execution order: files (`-U`), then repository (`-S`), then AUR rebuilds;
  it stops at the first failure and the record is `failed`.

Out of scope: `--source`/`--binary`/`--clean`/`--rebuild` and other build
flags, `--force`, reinstalling at a different version (that's `downgrade`
or `install`), and re-fetching an AUR PKGBUILD.

## Files

- `plan.rs` — `PlanAction::Reinstall`.
- `reinstall.rs` (new, pure): `ReinstallSource`, `choose_source`, the
  plan types; unit tested.
- `backend/aur_build.rs` — `has_checkout`.
- `sources.rs` — `plan_reinstall`, `execute_reinstall`.
- `rollback.rs` — refuse/skip reinstall records.
- `error.rs` — `NotInstalledForReinstall`, `CannotReinstall`.
- CLI: `cli.rs`, `commands/reinstall.rs`, `output.rs`, `main.rs`.
- Docs: READMEs, `FEATURE_STATUS.rst`, `CHANGELOG.md`, plan index.

## Verification

Unit: `choose_source` for every row above; rollback refusal and
`candidates` skipping. Host (read-only): `--dry-run` on installed packages
(file / repository / archive / unknown names / `--json`). Container (real
pacman): reinstall of a package with a deleted file repairs it (via the
cache file), via the repository when the cache is wiped, via the Archive
when the repo has moved past the installed version (install old `tree`,
wipe cache, reinstall), AUR rebuild path, mixed multi-package run, history
entry, `undo` skipping it, `rollback` refusing it.

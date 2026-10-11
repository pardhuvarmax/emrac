# Slice u5 — fetch old versions from the Arch Linux Archive

## Context

`u4` rollback/undo/redo only work while the needed package file is still in
pacman's `CacheDir` or emrac's AUR build dir; after `pacman -Scc` (or a
paccache run, or a package never cached on this machine) a rollback aborts
with "files missing". The Arch Linux Archive (`archive.archlinux.org`)
keeps every official package version ever shipped, so a missing file for an
official package can be fetched instead.

## Design

- **Only when needed.** Local files are always tried first (pacman cache,
  AUR build dir, and emrac's own archive cache below). The Archive is
  consulted only for a package whose file wasn't found, and never for
  `repo == "aur"` packages (the Archive doesn't carry them).
- **Resolve at plan time, download after confirmation.** Planning stays free
  of side effects on disk: it makes read-only GETs of the package's Archive
  directory listing (`/packages/<first letter>/<name>/`), finds the exact
  `<name>-<version>-<arch>.pkg.tar.<ext>` entry (same exact-shape matcher as
  the local scan, so `foo` never matches `foo-bar`; percent-decoded, so an
  epoch version like `1:2.0-1` matches `1%3A2.0-1`; `.sig` and unknown
  extensions ignored; `.zst` preferred), and records it as a download in the
  plan. The preview lists "will download from the Arch Linux Archive". Only
  after the user confirms does execution download, then run `pacman -U`.
- **Where files go**: `~/.cache/emrac/archive/<name>/` (user-writable; the
  pacman cache needs root). It is added to the local search dirs, so a
  second rollback reuses the file and nothing is fetched twice. Downloads
  stream to `<file>.part` and are renamed on success, so an interrupted
  download is never mistaken for a package.
- **Trust**: HTTPS to the Archive, plus the detached `.sig` fetched next to
  the package (best effort). `pacman -U` verifies a signature when one sits
  next to the file, against the user's own keyring, so the check is
  pacman's, not ours; a package that fails it makes the rollback fail
  loudly and is recorded as `failed`. No hash is invented on our side.
- **`--offline`** disables the Archive entirely: behavior is exactly u4's
  (missing files abort before prompting, naming each one).
- **Errors**: a listing that 404s means "never in the Archive" and counts as
  missing (aggregated with other missing files as today). A listing that
  can't be reached (network, 5xx, timeout) is a hard error naming the
  Archive, rather than pretending the version doesn't exist.
- **Out of scope**: a general `emrac downgrade <pkg> [version]` command
  (it would reuse this fetcher, separate slice), mirrors of the Archive,
  retrying/resume of partial downloads, size/progress display, AUR history.

## Files

- `crates/emrac-core/src/backend/archive.rs` — new, the only module that
  talks to the Archive: `list_files(name)` (`Ok(None)` on 404), `download`
  (stream to `.part`, rename), plus pure helpers `listing_hrefs`,
  `percent_decode`, `pick_file` that are unit-tested.
- `crates/emrac-core/src/rollback.rs` — `plan_rollback` takes a
  `find_remote` closure; `RollbackPlan` gains `downloads: Vec<Download>`
  (`name`, `version`, `url`, `dest`); `files` includes the destinations.
- `crates/emrac-core/src/sources.rs` — `plan_rollback`/`plan_redo` take
  `offline`; `execute_rollback` downloads first.
- `error.rs` — `Archive(String)`.
- CLI: pass the global `--offline`; `print_rollback` lists downloads.
- Docs: READMEs, `FEATURE_STATUS.rst`, `CHANGELOG.md`, plan index.

## Verification

Unit: href extraction and percent decoding (incl. `%3A`, `%2B`); exact-shape
selection incl. `foo` vs `foo-bar`, `.sig` ignored, `.zst` over `.xz`;
`plan_rollback` uses remote only when local is missing, never for `aur`,
reports missing when remote has nothing, propagates a remote error, and
puts destinations into `files`/`downloads`.

Host (read-only network): listing lookup for a real package and a bogus one;
`rollback --dry-run` against a hand-written history showing the download
plan; nothing written to disk.

Container (real pacman): upgrade an Archive-fetched old `tree`, `pacman
-Scc`, delete the AUR-dir copy, then `undo` → file downloaded, signature
checked by pacman, downgrade succeeds; the file now sits under
`~/.cache/emrac/archive`; a second rollback reuses it with no download;
`--offline` reproduces the u4 "files missing" abort; a bogus version
reports missing.

# Slice u9 — `emrac verify`

## Context

`SPEC.md` (Part V) lists `emrac verify <pkg>` (alias `check`) and the
"Package verification" feature. This slice checks that an installed
package's files on disk still match what pacman recorded — the equivalent of
`pacman -Qkk`, with a clearer report and a pointer to `emrac reinstall` for
repair. The spec's other "verify" commands (`repo`, `cache`, `index`) and
package-*signature* verification belong to features that don't exist yet.

## Design

- `emrac verify <pkg>...` (alias `check`), read-only, no root, `--json`.
  Names that aren't installed abort together, listed in one error.
- **Source of truth**: the package's `mtree` in the local db
  (`<dbpath>/local/<name>-<version>/mtree`, gzip). It is parsed here
  (`flate2` for gzip, ~60 lines of mtree parsing: `/set`/`/unset` defaults,
  `\NNN` octal escapes in names, `type`/`mode`/`size`/`sha256digest`/`link`),
  rather than through libalpm's libarchive-backed iterator, which doesn't
  expose the digests. The parser is pure and unit-tested.
- **Per entry** (relative to pacman's `RootDir`; the package's own metadata
  files `.PKGINFO`/`.BUILDINFO`/`.INSTALL`/`.CHANGELOG`/`.MTREE` skipped):
  missing; wrong type (file/dir/symlink); regular file size and SHA-256
  contents; permission bits (incl. setuid/setgid/sticky); symlink target.
  Deliberately **not** checked: mtime (changes harmlessly), owner uid/gid
  (an unprivileged user can't be expected to match, and it's noisy).
- **Config files** (`backup=()`) are expected to be edited: a content/size
  difference there is reported as *modified config*, not a problem — same
  stance as pacman. Missing or wrong-type config files are still problems.
- **Unreadable** files (root-only) are counted separately ("couldn't be
  checked — run as root for a complete check"), never as problems.
- **Exit status**: after the report, any real problem makes the command fail
  (exit 1, `found`-voiced summary) so scripts can use it; the summary points
  at `emrac reinstall <pkgs>`. Unreadable and modified-config alone → success.
- Not a transaction: nothing is written to the history.

Out of scope: `--all` (every installed package; large and slow, a later
flag), mtime/owner checks, repairing automatically, signature checks.

## Refinement found while testing

Two sources of false "missing" reports had to be handled, both visible in the
stock Arch container image: the mtree describes the whole package, but files
excluded at install time by `NoExtract` (docs, locales, man pages in that
image) are not on disk. Pacman's `-Qk` skips them; so does `verify`, by
matching each entry against `pacman-conf NoExtract` (`fnmatch`-style globs
where `*` crosses `/`; a `!` pattern re-includes; last match wins; directories
matched with a trailing `/`), and by dropping entries absent from libalpm's
installed file list.

## Files

- `verify.rs` (new, core): mtree parser, `check_entry`, report types,
  `verify_package`.
- `backend/alpm.rs` — `verify_inputs(name)` (version, mtree path, root,
  backup set).
- `sources.rs` — `verify(pkgs)`.
- `error.rs` — `NotInstalledForVerify`, `VerificationFailed`.
- `Cargo.toml` — `flate2`.
- CLI: `cli.rs`, `commands/verify.rs`, `output.rs`, `main.rs`.
- Docs: READMEs, `FEATURE_STATUS.rst`, `CHANGELOG.md`, plan index.

## Verification

Unit: mtree parsing (defaults, unset, escapes, links, dirs, metadata
files), each issue kind against a temp root (missing, wrong type, size,
content, mode, link, unreadable), config-modified vs problem, clean package.
Host (read-only): verify real packages; compare problem/ok verdicts with
`pacman -Qkk`. Container: tamper with files (delete, edit, chmod, replace
symlink), edit a config; verify reports each, exit status, `--json`;
`reinstall` fixes it and verify passes.

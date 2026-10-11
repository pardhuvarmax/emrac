# Slice u12 — `deps`, `rdeps`, `why`, `orphans`

## Context

`SPEC.md` Part V "Dependency Intelligence". These four are read-only, use only
libalpm, and need no root or network. (`tree`, `graph`, `impact`, `cycles` are
left for later.)

## Design

- `emrac deps <pkg> [--recursive]` (alias `depends`): direct dependencies of an
  installed package, or if not installed, of the sync-db package. Each shows
  the dependency string (`glibc>=2.40`), what satisfies it (installed
  package + version, or "not installed"). Optional dependencies are listed
  separately with their descriptions. `--recursive` lists the whole
  transitive closure of required dependencies (installed packages only,
  each once, with the depth).
- `emrac rdeps <pkg>` (alias `required-by`): installed packages that require
  it, and those that list it as optional. Installed packages only.
- `emrac why <pkg>`: why an installed package is there. Explicitly installed →
  says so (and also lists who requires it, if anyone). Installed as a
  dependency → the shortest chain from an explicitly installed package to it
  (`foo → bar → pkg`, breadth-first over required-by, ties alphabetical), or
  "no explicit package needs it (orphan)". Chains follow hard dependencies
  only.
- `emrac orphans`: packages installed as dependencies that nothing requires
  or optionally depends on (like `pacman -Qdtq`), with sizes. Removal is left
  to `emrac remove`; nothing is suggested automatically.
- All support `--json`. Unknown names → `found`-voiced error.

Chain search and orphan rules are pure functions in `deps.rs`.

Out of scope: `tree`, `graph`, `impact`, `cycles`, AUR/sync-only reverse deps,
`--recursive` for packages that aren't installed.

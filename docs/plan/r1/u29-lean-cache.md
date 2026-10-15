# Slice u29 — a lean cache: keep one rollback set, not every file pacman downloaded

After u28's first real `cache clean`, pacman's cache still held 7.3 GiB. Counting what
the files are for:

| Kept as | Size | What it is for |
|---|---|---|
| the installed version | 6.95 GiB | almost nothing: pacman keeps whatever it downloaded; only an offline `reinstall` or `redo` reads it |
| the previous version | 2.17 GiB | `undo` / `downgrade` — but of every package that ever had one, not of the last upgrade |
| named by a recent transaction | 1.9 MiB | `undo` / `redo` of the last 20 transactions |

(An earlier answer called the whole 7.3 GiB a rollback net. Most of it is not.)

`clean` today is "standard": the installed and the previous version of every package.
This slice adds a second policy, **lean**, that keeps one canonical rollback set.

## What lean keeps (binary and package scopes)

A package file is kept only if:

1. **It is needed to undo something from the last N days** (`cache.keep_days`, default
   14; the newest change counts however old it is): for each package a record really
   changed (its installed version is the one the record installed, or the package is
   gone), the `previous_version` of an upgrade or downgrade, and the version of anything
   removed. Installs need nothing (undoing one removes the package).

   (The first version of this plan said "the last K committed transactions". The first
   real data showed that is the wrong unit: one `emrac upgrade` session left ten
   records, the 365-package official upgrade sits in a record marked FAILED because an
   AUR build failed afterwards, and the newest record was a one-package AUR build. A
   time window over records of any status, counting only what each really changed, is
   what a person means by "I can undo the last upgrade".)
2. **It cannot be fetched again.** Official packages can: the current version from a
   mirror, any version from the Arch Linux Archive. So also kept:
   - a **foreign/AUR** package's installed and previous build (the Archive does not have
     them; a rebuild is the only other way);
   - the installed file of any package whose installed version is **no longer in the
     repositories** (held back by a pin, or not yet upgraded), since the mirrors have
     moved on;
   - any version **newer than the installed one** (a pending upgrade already downloaded).

Everything else is removable, including the installed-version copy of every up-to-date
official package. Sources and build trees follow the same rules as in standard.

## Using it

- `emrac cache clean --lean` (also for `trim`, `stats` and `list`, so the numbers match).
- Config: `cache.policy = "lean"` (default `"standard"`) and `cache.keep_days = N`.
  A flag on the command line wins over the config file, as with the other settings.
- The dry run shows the reason each file stays: `the last transaction's previous
  version`, `an AUR build`, `no longer in the repositories`, `a pending upgrade`.

## What lean costs

- `undo` of a transaction older than the last K downloads the old versions from the
  Archive: official packages, online only. AUR packages would have to be rebuilt.
- `redo` and `reinstall` of an up-to-date official package download it from the mirror.
  (This needed a change: rollback only knew the Archive, which can lag behind the
  mirrors. It now looks in the repositories first, for exactly the version they offer,
  and a `file://` mirror is copied rather than fetched.)
- Nothing else changes: `downgrade`, `verify`, `snapshot` and `restore` already fetch what
  is missing.

## Not covered

Per-package overrides; deduplicating or compressing what is kept; changing what
`purge` does; pacman's own `paccache` hook.

## Verification

Unit tests: each keep rule alone (what undo needs, removed packages, AUR installed and
previous, not-in-repositories installed, newer than installed), a failed record counting
for exactly what it changed, the standard policy unchanged, config parsing. Container, against a real pacman and a local repository: upgrade a
package A→B, then `cache clean --lean` removes B's file and keeps A's; `emrac undo`
installs A from the cache and `emrac redo` fetches B from the repository; a pinned
package's installed file is kept; a foreign package keeps its installed and previous
build; a second upgrade makes the first transaction's files removable.

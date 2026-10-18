# pacman: what it locks, writes and syncs during a transaction

Subject: pacman 7.1.0.r9.g54d9411-3 / libalpm 16.0.1 (Arch container, a throwaway copy).
Method: **dynamic** `strace -f` of `pacman -S cowsay` (which pulled perl and db5.3 as
dependencies; 3 packages were installed in total) and `pacman -R cowsay`,
run as root in a fresh `archlinux:latest` container with the network on only to fetch
packages. This is a deviation from the requested Ghidra trace: the binary is stripped and
the commit path is huge, and a syscall trace answers "which file operations, in what order"
directly and without reconstruction. The static side (hooks) is in `alpm-hooks.md`.

All findings are from these two runs only. Cases not exercised are listed under Gaps.

## Install (`-S`), order of operations in the pacman process

1. Read `/etc/pacman.conf`, the mirrorlist, and the local db version marker
   (`local/ALPM_DB_VERSION`).
2. Open `/var/log/pacman.log` for append (created if missing, mode 0644) and write the
   `Running 'pacman ...'` line. The log is open **before** the lock.
3. **Lock:** create `/var/lib/pacman/db.lck` with `O_CREAT|O_EXCL`, mode 000. No `flock`,
   no `fcntl` lock. The lock is a plain file; exclusion is only "create fails if present".
4. Read each sync db (`sync/core.db`, `sync/extra.db`) and load the whole local db
   (`local/*/desc`). Sync dbs are not modified.
5. Create a private download directory `/var/cache/pacman/pkg/download-XXXXXX` (mode 0700)
   for fetches.
6. Verify signatures by spawning `gpgconf` and `gpg` repeatedly (many short processes)
   before any package is extracted.
7. Per package, in transaction order:
   1. `mkdir /var/lib/pacman/local/<name>-<ver>/`
   2. `unlink` then create `local/<pkg>/mtree` with `O_EXCL`
   3. Extract payload files (below)
   4. Write `local/<pkg>/desc` then `local/<pkg>/files`, both with `O_TRUNC` in place
      (no temp file, no rename)
8. After all packages: post-transaction hooks run (in the container: `ldconfig`,
   `systemd-hook update`, `touch -c /usr`, `detect-old-perl-modules.sh`), each a child of
   pacman. No PreTransaction hook was triggered in this run.
9. **Unlock:** `unlink /var/lib/pacman/db.lck` as the last filesystem action before exit.

## Payload file extraction

For every file: `unlink(path)` (usually ENOENT on a fresh install), then
`open(path, O_WRONLY|O_CREAT|O_EXCL)` at the **final path**, then write, with the mode from
the archive (e.g. 0444 for perl modules). Directories use `mkdir` with archive mode.
Symlinks use `unlink` + `symlink`. Ordinary files have **no temp file and no rename**, so a
file is briefly partial at its final name and an interrupted extraction leaves a truncated
file in place. The one exception is backup files (`backup=`), see "Backup files" below.

## Remove (`-R`), order

1. Open log, create `db.lck` exactly as above.
2. Read `local/<pkg>/desc` and `files`.
3. Delete payload: files with `unlink`, directories with `rmdir` after their contents,
   children before parents (`/usr/share/cowsay/site-cows/` before `cows/` before the root).
   Directories that are not empty are left.
4. Only after all payload: delete the db entry: `unlink desc`, `unlink mtree`,
   `unlink files`, then `rmdir local/<pkg>/`. Note the order: `desc` goes first, so a crash
   in this window leaves a directory without `desc`.
5. Post-transaction hooks (`ldconfig`, `systemd-hook`, `touch -c /usr`).
6. `unlink db.lck`.

## Syncing to disk

pacman made **no** `fsync`, `fdatasync` or `sync` calls in any run. The only ones in the
install/remove traces (`fsync`, `fdatasync`, two `rename`s of `/etc/ld.so.cache~` and
`/var/cache/ldconfig/aux-cache~`) come from the `ldconfig` hook. Its own `rename`s occur only
for backup files (`.pacnew` / `.pacsave`). So package data and the
local db rely on the kernel writeback and the filesystem's journaling alone.

## Failure paths

Not exercised: no failure was injected. What the traces do show:
- Lock is non-atomic against crashes: if pacman dies between steps 3 and 9, `db.lck` stays
  and the next run fails to create it. Nothing in the trace removes a stale lock.
- File extraction is non-atomic (see above); db entries are updated in place after payload.
- Remove deletes payload before metadata, install writes payload before metadata. On both
  paths the local db is the last thing to change, so an interruption leaves "files present
  but package recorded absent / half-present" rather than the reverse.

## Experiments with a purpose-built test package

A tiny package `tp` (v1 and v2) with `backup=(etc/tp.conf)` and an `.INSTALL` that prints
each scriptlet name, built with makepkg in the container. Hooks were dropped into
`/etc/pacman.d/hooks`. Raw traces are in the scratchpad; findings below are what was observed.

### Scriptlets
- Order for a fresh install: `pre_install <new>`, payload, `post_install <new>`.
  Upgrade: `pre_upgrade <new> <old>`, ..., `post_upgrade <new> <old>`. Remove:
  `pre_remove <ver>` before any file is deleted, `post_remove <ver>` after payload and
  `.pacsave` handling but **before** the db entry is deleted.
- Each scriptlet runs as `bash -c '. /tmp/alpm_XXXXXX/.INSTALL; <function> <args>'`, in a
  fresh `mkdtemp` (0700) under `/tmp` inside the root, with `.INSTALL` copied there and
  deleted along with the directory after the run. `pre_*` copies from the package archive;
  `post_*` copies from the stored `local/<pkg>/install` file. A scriptlet's output is shown
  and logged (see `alpm-hooks.md` for the shared run helper).

### Upgrade (`pre_upgrade` -> `post_upgrade`)
Order in the pacman process (v1 to v2, verified with an unmodified conf):
1. Load hooks, then `pre_upgrade` scriptlet.
2. **Delete the old package's payload files** (here `a.txt`, even though v2 ships the same
   path), then delete the **entire old db entry** (`desc`, `install`, `mtree`, `files`,
   `rmdir`).
3. `mkdir local/tp-2-1/`, create `install` and `mtree`, extract new payload, write `desc`
   and `files`, run `post_upgrade`.
Consequence: between steps 2 and 3 the old files are gone and neither version has a db
entry. A crash there leaves files missing and the package unrecorded.

### Backup files
For a path in `backup=` pacman extracts the new file **as `<path>.pacnew`** first
(`unlink` + `O_EXCL` create), then reads the new file, the current file and the stored hash:
- **Current file unmodified** (hash equals the package's stored hash): `rename(path.pacnew,
  path)`. No `.pacnew` remains, no warning. Confirmed: conf became `conf 2`.
- **Current file modified**: `.pacnew` is kept, the live file is untouched, and pacman prints
  `warning: /etc/tp.conf installed as /etc/tp.conf.pacnew`. Confirmed.
- **Remove with a modified backup file**: after payload deletion, `rename(path,
  path.pacsave)` and `warning: ... saved as ... .pacsave`. An unmodified backup file is
  removed normally. Confirmed.
- Existing `.pacnew` is overwritten (`unlink` first).

### Pre-transaction hooks
- Hooks are read right after the local db loads, before any scriptlet or payload write.
- `AbortOnFail` + non-zero exit: output `error: command failed to execute correctly`,
  `error: failed to commit transaction (failed to run transaction hooks)`, exit 1. **Zero
  payload or db writes happened.** The lock was released. `pacman -Q` confirmed the package
  was not installed.
- Same hook without `AbortOnFail`: still prints `command failed to execute correctly`,
  but the transaction continues, the next hook (`10-second`) runs, scriptlets run, package
  installs, exit 0.
- `NeedsTargets`: the hook received the package name (`tp`) on stdin.
- `Depends` on a missing package with `AbortOnFail`: `error: unable to run hook 05-dep.hook:
  could not satisfy dependencies`, transaction aborted, package left installed (remove did
  not happen). Not run: the same case without `AbortOnFail` (the binary suggests it
  continues, unconfirmed by execution).

### Failure injection
- **ENOSPC injected on creating the payload file** (`openat` of `/usr/share/tp/a.txt`
  forced to fail): pacman reports `could not extract /usr/share/tp/a.txt`, `problem occurred
  while installing tp`, **still runs `post_install`**, ends with `failed to commit
  transaction (transaction aborted)` and `Errors occurred, no packages were upgraded.`,
  exit 1. Yet afterwards the package **is recorded as installed**: `local/tp-1-1/` exists
  with `desc`, `files`, `install`, `mtree`, `pacman -Q tp` prints `tp 1-1`, while the payload
  file is missing. The message says nothing was installed but the db says otherwise.
  The lock was released.
- **`kill -9` during a `post_install` scriptlet:** `db.lck` remains. The db entry is
  already complete (`desc`, `files`, `install`, `mtree`). The next `pacman -U` fails with
  `error: could not lock database: File exists` and the "if you're sure a package manager
  is not already running, remove db.lck" hint. `pacman -Q` (read-only) still works. The
  scriptlet's child `sleep` was orphaned and kept running.

## Still not covered

- Upgrade with the file layout changed (files removed in the new version), file conflict
  and ownership checks, `--needed`, `-Syu` against real mirrors, `Replaces`/`Conflicts`.
- Hooks with a trigger on `Type = Path`, and `Depends` unsatisfied without `AbortOnFail`.
- Failure injection for ENOSPC on the db files and for `kill` at other points (mid-extract).
- A static Ghidra trace of the failure branches (this document is dynamic only).

## Rust-port consequences

- `db.lck` is `O_EXCL` create, mode 000, removed last. Reproduce exactly and keep the
  "stale lock" behavior unless deliberately improved; decide whether emrac adds an `flock`.
- Mirror the install order: db dir + `mtree` first, payload, then `desc` and `files`.
  Mirror the remove order: payload deepest first, db entry last, `desc` first within it.
- Failed extraction must not leave a db entry: pacman's behavior (entry written, payload
  missing, "no packages were upgraded") is a bug-for-bug trap. Decide explicitly.
- Upgrade removes old files and the old db entry before the new package is written; keep
  that order for parity or reverse it for safety, but not by accident.
- Backup files: extract as `.pacnew`, compare hashes, `rename` over the live file only when
  unmodified; on remove, `rename` modified ones to `.pacsave`.
- If emrac should be safer than pacman, the traces show where: temp+rename for payload
  and db files, `fsync` before unlock. That is a deliberate divergence, not a port.

# Slice u27 — `emrac pacnew`: find, explain and resolve .pacnew / .pacsave files

`emrac doctor` says "5 pending .pacnew/.pacsave files" and stops. After the first
real upgrade they had to be sorted by hand: which ones hold the user's own edits,
which are plain upstream changes, and what to do with each.

## What pacman guarantees, and what it does not

A `.pacnew` appears when the live config file differs from the version the *old*
package shipped **and** the new package ships something different. If the live
file was untouched, pacman replaces it silently and no `.pacnew` is made.

So a pending `.pacnew` does not by itself say what changed. Comparing the live file
with the `.pacnew` only shows the two differ, mixing the user's edits with the
upstream's. (In the session that prompted this slice, `services` and the two
`tpm2-tss` profiles were judged "never edited" from their timestamps and a short
diff. That was an inference. pacman's log shows both had `.pacnew` files made, so
the live files were not what the old packages shipped, or the `.pacnew` is a
leftover from an earlier upgrade. The slice exists to replace such guesses.)

The honest comparison is three-way:

- **base** — the file as the *old* package shipped it;
- **ours** — the live file;
- **theirs** — the `.pacnew`.

## 1. Finding them

For every installed package, each `backup=` file (libalpm's backup list) is checked
for `<file>.pacnew` and `<file>.pacsave`. This is exact and fast, and ownership is
known. Any other `*.pacnew` / `*.pacsave` found under `/etc` is listed as "not owned
by a package".

## 2. Finding the base

`/var/log/pacman.log` records `upgraded <pkg> (<old> -> <new>)`. The earlier
versions of the owning package (newest first, at most six) give candidate files,
read from the pacman cache, the AUR build folder or emrac's archive cache, and
with `--fetch` from the Arch Linux Archive through the existing `ArchiveBackend`
(u5), extracting only that one path with `bsdtar`.

(The first version of this plan took the version just before the log's `installed
as .pacnew` warning. Real data showed that is not reliable: a `.pacnew` can be left
by an older upgrade than the log's latest warning, and the live file can be older
than every recent version. So every candidate is tried.)

- If the live file is exactly what one candidate shipped, it was never edited: it is
  just older than the new file (`untouched`).
- Otherwise the base is the newest candidate whose file differs from the new one.
- A `.pacnew` whose md5 differs from the one pacman recorded for the installed
  package is older than that package, says nothing about what changed, and is
  `unknown`.

If no candidate can be read, the file is `unknown` and only the live-vs-new diff is
shown, with the reason and the hint about `--fetch`. Nothing is guessed.

## 3. Classification

| ours vs base | theirs vs base | class | default offer |
|---|---|---|---|
| same | differs | `untouched` — only upstream changed | take the new file |
| differs | same | `mine-only` — the `.pacnew` is a leftover | keep yours, drop the `.pacnew` |
| differs | differs | `both-changed` | three-way merge if clean, else show the conflicts |
| same as theirs | | `identical` | drop the `.pacnew` |
| no base | | `unknown` | keep yours |

The merge uses `git merge-file -p` (git is already required for AUR builds). A clean
merge is shown as a diff against the live file before anything is written; a merge
with conflicts is never written.

## 4. Commands

- `emrac pacnew [--fetch]` — list every pending file with its package, class, age, and
  a one-line reason. `--json` is machine-readable. Without `--fetch`, only package
  files already on this machine are used; nothing is downloaded.
- `emrac pacnew diff <file>` — what the user changed, what upstream changed (both
  against the base), and the result of a merge.
- `emrac pacnew resolve [file…]` — for each file, offer keep / take / merge / skip.
  `emrac doctor`'s warning points here.

## 5. Safety

- emrac never runs as root. Writes go through `sudo`; the replaced live file is first
  copied to `~/.local/share/emrac/config-backups/<unix-time>/<path>` (private, mode 0600).
- Every change is asked about; an empty answer, an unknown one, `--yes` and a closed
  stdin all skip. The suggested action is named, never pre-selected.
- A short list of files whose change can lock the user out (`sudoers`, `sudoers.d/*`,
  `passwd`, `shadow`, `group`, `fstab`, `pam.d/*`, `ssh/sshd_config`) is marked and
  needs the user to type the file's name to take or merge it. Keep and skip stay
  one keystroke.
- After a change, a note says what to do next (for `mkinitcpio.conf`: `sudo mkinitcpio -P`;
  for `pacman.conf`/`sshd_config`: nothing runs by itself).
- `.pacsave` files (made when a package with edited config is removed) are listed and
  can be removed; they have no "new" side.

## Not covered

Semantic judgement (whether a new setting matters to the user); `.pacorig`; config
in `$HOME`; binary files (reported, offered keep/take only); restoring a file from
the backup copy (the path is printed, the copy is a plain file).

## Verification

Container, against a real pacman. A package with `backup=(etc/x.conf)` in two
versions, built and installed from a local repository:

- live edited in a line upstream does not touch → `both-changed`, clean merge;
- live edited in a line upstream also changes → conflict, nothing written;
- live restored to the shipped text with a leftover `.pacnew` → `untouched`;
- old package removed from the cache and the archive unreachable → `unknown`, and
  the message says why;
- take / keep / merge each leave the expected file, a backup copy, and no `.pacnew`;
- `--yes` and a closed stdin both decline.

Unit tests: parsing the pacman.log lines, picking the right `<old>` version, the
classification table, the critical-file list, and the backup path.

# Slice u8 — an immutable place for the history

u7 makes tampering with `history.jsonl` visible, but the file is still in the user's home,
so whatever shows the tampering can be edited too. This slice keeps a second copy where
the user's own programs cannot rewrite it: a root-owned, append-only file. emrac's history
stays the working copy; the ledger is the record to compare it against.

## What it protects against, and what it does not

It protects against edits and deletions by the user and by anything running as the user:
an accident, a script, malware without root. It does **not** protect against root: a
person with `sudo` can remove the append-only flag and rewrite the file. That is
inherent, and the documentation says so; the point is that rewriting it takes a
deliberate, privileged act that does not look like anything emrac does.

## The ledger

- **Where:** `/var/lib/emrac/ledger.jsonl`, in a directory root owns (`0755`), the file
  `0644` root, so everyone can read it and only root can write it. `/var/lib` is where
  pacman keeps its own state.
- **Append-only:** on creation the file gets the append-only attribute
  (`chattr +a`), so even a root process can only add lines until the flag is cleared.
  Where the filesystem has no such attribute (overlayfs, tmpfs, some network mounts) it
  is left out and `doctor` says "root-owned, but not append-only here".
- **What goes in:** the same chained lines as the user's log (u7): every transaction that
  reached `sudo pacman`, and the markers (archive, acknowledge). A transaction that failed
  before touching the system (an AUR build that never got to `pacman -U`) stays in the
  user's log only: nothing system-side happened.

## How a line gets in

Writing needs root, and emrac runs as the user. emrac already calls `sudo` for `pacman`,
so the append uses it too, right after the change, while the sudo credential is fresh:

- `sudo emrac ledger-append` is a hidden subcommand of the same, root-owned binary that
  the package installs. It reads lines from standard input and does nothing else:
  it checks that each is a JSON object of a known shape under a size limit, that the
  chain continues from the ledger's own last line (`prev` is the hash of it), and then
  appends with `O_APPEND`, flushes it to disk and sets the attribute on first creation.
  Anything else is refused.
- **Catching up:** if a call is skipped (no sudo, a declined password), the next one sends
  every user-log record the ledger does not have yet, in order. The chain makes the gap
  checkable, and `doctor` says "N transactions are not in the ledger yet".
- **No sudo, or the user says no:** emrac carries on with the user's log, says so once
  per run, and never blocks a transaction on the ledger.

## Checking, and repairing

- `emrac history verify` and `doctor` compare three things: the ledger's own chain (it
  should never be broken), the user's log against it (a changed, missing or extra record
  is named by id), and the archive files against the markers in the ledger.
- `emrac history verify --repair` rebuilds the user's log from the ledger after asking
  (default no, `--yes` does not answer), keeping the broken file as `history.jsonl.broken`.
  Archived records are not restored into the live log; the markers say where they went.
- `history archive` (u7) changes only the user's log. The ledger keeps every record for
  good; at about 500 bytes each, ten thousand transactions is a few megabytes. Rotating it
  is an administrator's job and is not offered.

## Packaging

The package ships a `systemd-tmpfiles` entry that creates `/var/lib/emrac`; without it
(a build from source) the first append creates the directory. The AUR package, the
install script and the README mention the ledger and how to read it: it is a plain text
file anyone can `cat`.

## What was built, and where it differs

- **Line format.** A ledger line wraps one history line byte for byte:
  `{"at","line","lprev","lseq","sha","tx"|"type"}`. `lseq`/`lprev` are the ledger's own
  chain, `sha` is the SHA-256 of `line` (how the two copies are matched, so an archive that
  renames nothing still matches), `at` is root's clock. The history's own `seq`/`prev` ride
  inside `line`.
- **sudo.** After a change emrac runs `sudo -n -- <this binary> ledger-append`: `-n` so a
  transaction never ends with a password prompt. `emrac history sync` is the version that may
  ask. The binary is only run as root if it is owned by root and not group/other-writable.
- **Which lines.** Every recorded line goes (transactions, acknowledgements, archive and
  chain-start markers). The "failed before pacman" distinction is not made: those are not
  recorded in the history at all.
- **`EMRAC_LEDGER`.** For anyone who is not root it points the ledger somewhere else and
  emrac writes that file directly (tests, a copy). Root always uses `/var/lib/emrac`.
- **Not built:** the `systemd-tmpfiles` entry (the first append creates the directory).
- **Append-only was not exercised on a real filesystem.** `chattr +a` needs
  CAP_LINUX_IMMUTABLE, which a rootless container (and this session) does not have: it fails
  in the container, so the file there is root-owned but not append-only and `doctor` says so.
  What is tested: that the helper calls `chattr` best-effort and that `lsattr` output is read
  correctly. The flag itself should be tried once on a real machine with `sudo`.

## Not covered

Signing the ledger with a key; a separate privileged daemon (the helper is one subcommand
and a `sudo` call, on purpose: nothing runs in the background); shipping the ledger off the
machine.

## Verification

Container, as root and as a user with the usual `sudo` rule: the helper appends and
refuses malformed, oversized and chain-breaking input; the user cannot delete, truncate or
overwrite the ledger; with `chattr +a` supported even root's `>` is refused (and
`doctor`'s message where it is not supported); a skipped append is caught up by the
next transaction; deleting the user's log and `history verify --repair` bring it back;
an edited record is named. Unit tests: the line validation and the chain check in the
helper, and the catch-up selection.

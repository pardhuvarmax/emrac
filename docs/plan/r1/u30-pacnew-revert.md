# Slice u30 — `pacnew`: validate before applying, record what was done, take it back

u27 finds, compares and resolves `.pacnew` files, and saves the live file before
changing it. Three things it does not do, found while using it for real:

- It never checks that the file it is about to activate **works**. The stock
  `mirrorlist.pacnew` has every `Server` line commented out; taking it would leave
  pacman with no mirrors.
- The backups exist but nothing uses them: no way to put a file back, and no list of
  what emrac changed.
- A merge that conflicts (the user's `mkinitcpio.conf` against upstream's `HOOKS`
  change) can only be kept or skipped; settling it by hand means leaving emrac.

Proposals for a bigger "configuration lifecycle manager" were weighed. Not taken: a
two-way merge (without the old shipped file nothing separates the user's edit from
upstream's, so any automatic result is a guess — u27 shows the diff instead), a
snapshot store under `/var/lib/emrac` (emrac runs as the user, and the old package
versions already give a reliable base), and multi-file transactions.

## 1. Validate

Before `take`, `merge` or an edited result is written, the **candidate** is checked:

| File | Check |
|---|---|
| `/etc/sudoers`, `/etc/sudoers.d/*` | `visudo -cf <candidate>` |
| `/etc/ssh/sshd_config` | `sudo sshd -t -f <candidate>` (host keys need root) |
| `/etc/pacman.conf` | `pacman-conf --config <candidate>` parses |
| `/etc/fstab` | `findmnt --verify --tab-file <candidate>` |
| `/etc/pacman.d/mirrorlist` | at least one uncommented `Server` line |
| `*.json` | parses as JSON |
| shell-sourced files (`/etc/mkinitcpio.conf`, `/etc/makepkg.conf`, `/etc/*.conf` that begin `#!` or are known to be sourced) | `bash -n` |
| anything else | no validator: the message says so |

A failed check refuses that file's take/merge and shows the tool's message; keep and
skip stay available. There is no override flag: the way past a check is to edit the
file, which is then checked again.

## 2. Record, and revert

Every change appends to `~/.local/share/emrac/pacnew-log.jsonl`: an id (`cfg-N`), the
time, the live path, the package, the action (`keep`, `take`, `merge`, `edit`,
`revert`), the class, SHA-256 of the live file before and after, its mode and owner,
and the paths of the copies. A record is written as `started` before anything changes
and `done` (or `failed`) after, so a crash leaves a visible half-finished entry.

Both the live file **and the `.pacnew`** are copied to
`~/.local/share/emrac/config-backups/<unix-time>/` before the change, so a reverted
file becomes pending again exactly as it was. The replacement is written beside the
live file and renamed over it, so the live file is never half-written.

- `emrac pacnew revert [file]` — undoes the newest change to that file (no file: the
  newest change of all): puts back the saved live file with its mode and owner, and the
  `.pacnew` if the change consumed it. It first compares the live file's hash with the
  one recorded after the change; if someone edited it since, it refuses and says so
  (typing the path overrides). Default-no prompt; `--yes` does not answer; a closed
  stdin is no. The revert is itself recorded.
- `emrac pacnew status` — pending files, then the recorded changes with their state:
  `applied`, `reverted`, `changed since` (the live file no longer matches) or
  `interrupted`. `--json`.

## 3. Edit by hand

For a file whose merge has conflicts (or a clean one the user wants to adjust),
`resolve` offers **[e]dit**: the three-way result with `diff3` conflict markers is
written to a private temp file and opened in `$VISUAL`/`$EDITOR`. Afterwards it must
have no marker lines left and pass the validation above; the diff against the live file
is shown and the usual confirmation (typed path for risky files) applies. Nothing is
offered when there is no base (`unknown`): the diff is all emrac can honestly show.

## Not covered

Several files as one transaction; reverting a change that a later change to the same
file already replaced (the older one is refused until the newer is reverted); watching
for new `.pacnew` files; checks for files beyond the table.

## Verification

Container, against a real pacman and packages with `backup=()` files as in u27:
a `take` whose candidate is an all-commented mirrorlist, a sudoers file with a syntax
error and invalid JSON are each refused and change nothing; a `take`, a `merge` and a
`keep` are each reverted and the live file (bytes, mode, owner) and the `.pacnew` are
back as before; editing the live file after a change makes `revert` refuse until the
path is typed; `status` shows applied, reverted and changed-since; an edit with a
script as `$EDITOR` that resolves the markers is applied, and one that leaves a marker
is refused. Unit tests: the log's round trip and the state of each record, the
validators on good and bad input, the marker check, and the diff3 output.

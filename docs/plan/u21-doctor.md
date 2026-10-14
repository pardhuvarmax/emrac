# Slice u21 — `emrac doctor`

`ideas.md` ("`emrac doctor`: one command that checks for common problems … and
produces a sanitized report suitable for a bug report") and SPEC's Health
Center. Read-only: it never changes anything, needs no root, and says what to
run when it finds something.

## Checks

Each check has a status — **ok**, **note** (worth knowing), **warning**,
**problem** — a one-line finding, optional detail lines and a fix hint.

| Check | Looks at | Status |
|---|---|---|
| unsatisfied dependencies | every installed package's dependencies against what is installed (what `pacman -Dk` finds) — the visible signature of a partial upgrade or a forced removal | problem |
| pacman lock | `db.lck` present: with no pacman running it is stale | problem if stale, note if pacman is running |
| sync databases | age of the newest `sync/*.db` — installing from a stale database is how partial upgrades start | warning after 7 days |
| pending upgrades | how many packages `pacman -Su` would upgrade (from the current database) | note |
| `.pacnew` / `.pacsave` | files under `/etc` left by upgrades that need merging | warning |
| failed units | `systemctl --failed` for the system and the user | warning |
| running kernel | `/usr/lib/modules/<uname -r>` exists; if not, the kernel was upgraded under you | warning (reboot) |
| disk space | free space on `/` and on pacman's cache filesystem | warning under 10 % or 5 GiB, problem under 1 GiB |
| caches | pacman package cache; emrac's build and archive caches | note, warning for pacman's over 10 GiB |
| orphans | dependencies nothing needs (u12) | note |
| pins | expired pins (u13) | warning |
| recent failures | failed transactions in the history in the last 7 days | note |
| config | `config.toml` parses and isn't world-writable | problem / warning |
| tools | `makepkg`, `git`, `bsdtar`, `fakeroot`, `sudo`, `pacman-conf` present | problem for the ones AUR builds can't do without |
| build sandbox | `bwrap` actually works here (the real probe); whether memory/process limits are available | warning / note |

Not checked: anything that needs the network or root (AUR packages removed
upstream, keyring expiry), or that would be slow and noisy (a full library
scan) — `emrac verify` covers file integrity per package.

## Output and exit status

- Grouped list, problems first, each with its fix hint. `--json` returns the
  same data.
- Exit `1` when there is any **problem**, `0` otherwise (warnings and notes
  don't fail a script).
- **`--report`**: a markdown block for a bug report — emrac and pacman versions,
  kernel, package counts, and every check's result — with `$HOME`, the user
  name and the host name scrubbed, so it can be pasted publicly.

## Design

Pure functions decide each status from plain inputs (an age, a file list, the
text of `systemctl --failed`, free/total bytes, …) and are unit-tested;
`Sources::doctor` gathers the inputs. The scrubber is pure too.

## Files

`doctor.rs` (new, core), `backend/alpm.rs` (`unsatisfied_dependencies`),
`backend/pacman.rs` (`db_path`), `sources.rs`, `error.rs`; CLI
`commands/doctor.rs`, `output.rs`, `cli.rs`, `main.rs`.

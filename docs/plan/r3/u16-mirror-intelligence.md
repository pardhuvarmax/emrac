# Slice u16 (after r3) — Mirror Intelligence

`SPEC.md` Part IV "additional ideas": *"chooses mirrors based on latency, historical reliability, and current download
performance, not just a one-time ranking."*

```
emrac mirrors                          # your active mirrors, in order, with what emrac has measured
emrac mirrors test                     # measure them now (adds to the history)
emrac mirrors rank                     # the order the history supports, and why
emrac mirrors rank --keep 5 --apply    # put it in /etc/pacman.d/mirrorlist (asks; keeps a backup)
emrac mirrors rank --write ranked.txt  # or write the proposal to a file and look at it
```

## What is measured

For each active `Server` line, emrac downloads the real thing pacman would: `<server>/core/os/<arch>/core.db` (about a hundred
KiB), and records, with the time:

- whether it worked, and the error if not;
- **latency**: time until the headers arrive; **total time** and the **throughput** that gives;
- **freshness**: the mirror's own `lastsync` file (the Unix time it last synchronised with Arch); the age is recorded, and a
  mirror that is more than a day behind is *stale*. A mirror with no `lastsync` is simply unknown.

Every run is appended to `~/.local/share/emrac/mirrors.jsonl` (the newest 3000 samples are kept). That history is what makes
it "not a one-time ranking": a mirror that was fast once and fails half the time is not trusted.

## The ranking, in words (no hidden weights)

Mirrors are put in four groups, in this order, and sorted inside a group as stated:

1. **good**: its last attempt worked, at least 80% of its last 10 attempts worked, and it is not stale. By median total time,
   fastest first; a tie goes to the name.
2. **stale**: works but is more than a day behind Arch. By median total time.
3. **unreliable**: its last attempt failed or fewer than 80% of its last 10 worked. By success rate, then median total time.
4. **untested**: no samples yet. In the order your file has them.

`rank` prints the groups with the numbers, so every position can be checked.

## Changing the mirror list is careful

- `rank` never changes anything. `--write FILE` writes the proposal to a file you choose and refuses to overwrite one.
- `--apply` asks (default no; `--yes` answers it), saves the current file to `~/.local/share/emrac/mirrorlist-backups/` first, and
  installs the new one with `sudo install`. It prints how to put the old one back.
- **Only reorders.** The proposal contains exactly the mirrors that were active in the file, never one that was not there
  (choosing who to download packages from is yours); `--keep N` keeps the best N and comments the rest out with `#`. Comments and the
  header of the file are kept. A result with no active `Server` line is refused.
- It does not refresh the databases: run `sudo pacman -Sy` yourself (or `emrac upgrade`) after a change.
- Signatures protect the packages whatever the mirror; ranking is about speed, freshness and availability, not trust.

## Not covered

Choosing a country, adding mirrors from the Arch mirror status page (use `reflector` for that), per-repository ranking, measuring a
real package download, ranking during an upgrade, and doing it in the background.

## Design

`emrac-core/src/mirrors.rs`: `parse`, `probe_url`, `measure` (ureq), `summarize`, `rank`, `render_mirrorlist` (pure and
tested), the history file. `commands/mirrors.rs`: `mirrors`, `mirrors test`, `mirrors rank`. No libalpm.

## Verification

Unit tests for the parser, the URLs, the summary, each rule of the ranking, the rewritten file (header kept, only active mirrors,
`--keep`, never adds, no active server refused), the history cap, and `measure` against a local server (a good one, a
failing one, one with a stale `lastsync`, one that is closed). A container suite `dev/e2e/mirrors.sh` against the real mirrors
and `--apply` against a copy of the list.

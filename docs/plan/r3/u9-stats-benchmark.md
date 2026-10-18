# Slice u9 (after r3) — `emrac stats` and `emrac benchmark`

`SPEC.md` Part IV #18 (Build Statistics) and #7 (Build Benchmark). Both read or measure data emrac can
already see, and neither changes the system.

## `emrac stats`

*"What has emrac built and installed on this machine, and how long did it take?"*

```
emrac stats                # builds and transactions, all time
emrac stats --since 30d    # the last 30 days (also 12h, 8w)
emrac stats --json
```

Sources, both already on disk:

| Source | Gives |
|---|---|
| `~/.local/share/emrac/build-times.jsonl` (written by every build since u2; newest 500 kept) | builds: package, version, seconds, cores, success |
| the transaction history (`TransactionRecord`) | committed installs, upgrades, removals, failed and rolled-back transactions; packages by repository |

Output: builds (total, failed), distinct packages, total and average build time, the longest build, the
packages built most often with their average time, and the transactions by kind with the share of
packages that came from the AUR (repository `aur`) against the official repositories.

**What it does not claim.** The spec also lists "hours saved (source versus binary time deltas)" and "storage
saved". Those need a measured binary-install time next to a measured build, and emrac measures only
the build. They are left out and `stats` says so in one line, rather than guessing a figure. The build
file keeps the newest 500 builds, so "all time" means "all kept" and the report states the period it covers.

## `emrac benchmark <pkg>`

*"What does this installed package cost to run?"* Measured, stored, and read back by `recommend`/`explain`
later (not wired in this slice: the record format is what is settled here).

```
emrac benchmark ripgrep                  # its executables in /usr/bin
emrac benchmark ripgrep --exe rg         # one executable
emrac benchmark ripgrep --runs 10
emrac benchmark ripgrep --history        # earlier results for it
```

Per executable (files the package owns directly in `/usr/bin` or `/usr/sbin`, regular and executable; `--exe`
names one):

- **size**: the file, and the package's installed size;
- **first run and warm runs**: wall time of `<exe> --version` with stdin closed and output discarded: the
  first run after the command starts, then the median of `--runs` further runs (default 5, at most 50).
  "Cold" in the strict sense (an empty page cache) needs root and is not claimed: the first run is
  reported as "first run";
- **peak memory**: the largest resident set seen among the runs (`ru_maxrss` from `wait4`).

Safeguards, since this runs a program:

1. It runs only an executable of the **named installed package**, never a path from the command line, and only after
   showing which files it is about to run; it asks (default no, `--yes` answers it) because running a package's
   binary executes its code.
2. It runs as you, never through `sudo`, with the working directory set to a fresh empty temporary directory and
   stdin closed.
3. A run is killed after 10 seconds (a program that waits for input or opens a window is reported as "did not
   finish", with no number made up). A non-zero exit still gives a time, flagged, since many tools reject
   `--version`.
4. If `--version` makes no sense for the program the timing is still the cost of starting it; the report says
   what was run so it is not mistaken for a workload benchmark.

Results are appended to `~/.local/share/emrac/benchmarks.jsonl` (newest 200 kept): package, version, executable, time
taken, runs, sizes, memory, cores.

## Design

- `emrac-core/src/build_stats.rs`: `BuildStats`, `TransactionStats`, `Stats`, `compute(builds, history, since,
  now)`, `parse_since`. Pure, tested.
- `emrac-core/src/recommend.rs`: `read_builds()` (all lines of the build file) beside the existing writer.
- `emrac-core/src/benchmark.rs`: `Measurement`, `BenchRecord`, `median`, `measure(exe, runs)` (spawn, `wait4`,
  timeout), file store with the same atomic write as the build file. Needs `libc` in `emrac-core`.
- `emrac-core/src/sources.rs`: `stats()`, `package_executables(pkg)` (from the file list pacman recorded).
- CLI: `commands/stats.rs`, `commands/benchmark.rs`, `Stats` and `Benchmark` in `cli.rs`, dispatch in `main.rs`.

## Not covered

Workload benchmarks, CPU time over a longer run, FPS, a cold-cache measurement, source-versus-binary comparison
of the same package (`diff-build`), and feeding the results into `recommend`. The last is the next step
once records exist.

## Verification

Unit tests for the aggregation (empty input, failed builds excluded from averages, `--since` windows, the AUR
share), `parse_since`, `median`, the store (written, capped, read back), and `measure` against `/bin/true` and a
sleeping program (timeout). A container check in `dev/e2e/stats.sh`: build times written by a real build,
`stats` output, `benchmark` of a real installed package with `--yes`, and the refusal of a package with no
executables.

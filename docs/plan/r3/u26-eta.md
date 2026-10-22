# Slice u26 (after r3) — ETA prediction

`SPEC.md` III.4. A plan says what will change; this says how long it will take, how sure that is, and what the
number rests on. An estimate that cannot be made is said to be unmade.

```
emrac eta                 # the upgrade that is waiting
emrac eta firefox ripgrep # installing these
emrac install / upgrade   # the plan ends with an "Estimated time" line
```

```
Estimated time   : about 2 min 14 s (between 1 min 40 s and 3 min 50 s), medium confidence
                   download and install 41 s; build brave-bin 1 min 33 s
                   no estimate for: build foo (a first build here)
                   (estimates so far: within a quarter of the real time in 7 of the last 9)
```

## What each part rests on

| Part | From | Confidence |
|---|---|---|
| Build of a package built here before | its successful builds in `build-times.jsonl` (last five), each scaled to the cores there are now (85 % of the work taken as parallel, never more than ×4 either way); the median, with the fastest and the slowest as the range | high with three or more close builds, otherwise medium |
| Build of a `-bin` package never built here | the median of at least three other `-bin` builds (they are a download and a repackaging) | low |
| Build of anything else never built here | nothing: **no number**, listed under "no estimate for" | none |
| Download and install of official packages | `ops.jsonl`, the transactions emrac ran: seconds per package from the small ones, bytes per second from the big ones (≥ 30 MiB) | medium with two big ones, low with one |
| the same with no history | the speed `emrac mirrors` measured, ×0.6 (it measured a small file) | low |
| a big download with nothing to go on | nothing | none |

The total adds the parts, adds the ranges, and takes the weakest confidence; anything that could not be estimated
caps it at low and is named. Builds that failed are not counted, and the AUR source download inside a build is part
of the build's time, so a slow network shows in the next estimate, not in this one.

## Refinement while it runs

The build screen's live line shows the usual time beside the clock: `~1 min 40 s left`, and when it is taking
longer, `longer than the usual 2 min`. It says nothing in the first seconds of a build. (Downloads are pacman's own
screen and keep pacman's own rates.)

## Calibration

When a plan's estimate is shown and the transaction then finishes, the estimate and the real time go to
`eta-accuracy.jsonl`; after three, the plan shows how close past estimates were (within a quarter of the real time:
n of the last m). Every finished official transaction goes to `ops.jsonl` and teaches the next estimate. Failed
transactions teach nothing.

## Not here

Per-phase estimates for the installation steps of a source build that has no history; network-aware estimates beyond
the measured rates; estimates for `reinstall`, `downgrade`, `restore` plans (they print none); estimating before
`emrac mirrors` or any transaction has run on a fresh machine (the answer is "cannot say" for a big download).

## Design

`emrac-core/src/eta.rs` (pure, tested): `build_part`, `transfer_part`, `Estimate::total`, `scale_cores`,
`accuracy_summary`, `live_note`, and the two files. `Sources::estimate`, `Sources::build_expected_secs`;
`BuildOptions::expected_secs`; `BuildScreen::set_expected`; `commands/eta.rs` prints and records.

## Verification

Unit tests (`eta.rs` 7, `build_ui.rs` 1). `dev/e2e/eta.sh` (11 checks): history seeded into the data directory,
the figure and its range for a package built three times (the failed build left out), hardware scaling, a first
build said to be unknown, `--json`, and a real transaction that is noted and compared.

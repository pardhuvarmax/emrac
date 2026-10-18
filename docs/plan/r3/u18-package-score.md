# Slice u18 (after r3) — `emrac score`: a report card for a package

`SPEC.md` Part IV #3: *"Every package gets a report card across Performance, Build Time, Maintenance, Security, Popularity, and
Source Worthiness, rendered as bar meters."*

```
emrac score neovim
emrac score neovim --details     # every rule behind every number
emrac score some-aur-package --json
```

```
neovim 0.10.2-1 (extra): a report card, 4 of 6 known
  Performance    ▇▇▇▇▇▇▇▇▇░   92   starts in 4.3 ms (warm), 2 programs, benchmarked 2026-10-18
  Build time     ▇▇▇▇░░░░░░   40   about 28 min on this machine (estimated)
  Maintenance    ▇▇▇▇▇▇▇▇▇▇  100   rebuilt in the repository 6 days ago
  Security       ▇▇▇▇▇▇▇▇▇░   95   official package, signed by an Arch packager
  Popularity     -                 the official repositories keep no popularity data (212 packages depend on it)
  Source         ▇▇░░░░░░░░   22   the source/binary advice is "binary" (see `emrac explain neovim`)
```

## The principle

Every number comes from a stated input and a stated rule, like `recommend`: no hidden weights, no guessing, and **a dimension with no
data is shown as unknown (`-`), never as a made-up number**.

**The overall number** (added after the first version, at your request) is the weighted mean of the dimensions that are known and have a
weight, rounded half up, and it always says what it is made of: *"from 4 of 6 dimensions, 67% of the weight"*, which ones were left out
and why (unknown, or a weight of 0), and the weights when they are not the default. An unknown dimension is never counted as zero. The
weights are yours: every dimension counts 1 by default; `[score]` in the config sets standing weights (`score.security = 5`, 0 to 10, 0
leaves a dimension out); `--weights security=5,popularity=0` sets them for one run; `--care security,maintenance` makes the named ones
count three times as much. A mean can hide a serious problem, so read the dimensions, not only the number.

## The six, and what each is made of

Each dimension is a score from 0 to 100 (100 best) found in a table of limits, with the rule shown by `--details`.

| Dimension | Input | Rule |
|---|---|---|
| **Performance** | the stored `emrac benchmark` results of the package (installed programs only) | the median warm start of its programs: up to 10 ms 100, 50 ms 85, 200 ms 65, 1 s 40, slower 15; minus 10 if the biggest peak memory is over 256 MiB, 25 if over 1 GiB. No benchmark: unknown, with the command to run |
| **Build time** | your own earlier builds of it (median), else the `recommend` estimate | up to 1 min 100, 5 min 85, 15 min 65, 1 h 40, 3 h 20, longer 5. Says which: measured or estimated |
| **Maintenance** | official: the build date in the repository. AUR: when the page last changed | up to 30 days 100, 90 days 90, 180 days 75, a year 55, two years 30, older 15. AUR: minus 30 when flagged out of date; at most 40 when it has no maintainer |
| **Security** | official: the signature in the database, the Arch news, and the **Arch Security Tracker** (security.archlinux.org). AUR: the PKGBUILD audit and the trust signals | official: 95 if signed, 50 if not; minus 15 for each Arch news item of the last 90 days that names it (at most 30); minus, for each open tracker entry that applies to the version (installed, else the repository's): critical 40, high 25, medium 12, low or unknown 5, at most 60 in all. An entry applies when its status is `Vulnerable`, the package is named, the version is not older than `affected` and older than `fixed` (or no fix is recorded), the rule `arch-audit` uses; every applying entry is listed by id with its CVEs and whether a fix exists. AUR: 80 as the starting point (community-uploaded, not reviewed by Arch); a critical audit finding makes it 0; each high finding minus 30; each warning minus 10 (at most 30); each high trust signal minus 25, each warning minus 10; the tracker covers official packages only and the card says so. Offline: not audited / not checked, and it says so |
| **Popularity** | AUR: votes and popularity. Official: nothing | votes: none 10, under 10 30, 50 50, 200 70, 1000 85, more 100; popularity: under 0.01 20, 0.1 40, 1 60, 5 80, more 100; the average of the two. Official: unknown (the repositories keep no vote data), with the number of packages that depend on it as a fact |
| **Source worthiness** | the `recommend` engine's score | that score as it is; unknown when there is no choice (no binary to compare with) |

## The Arch Security Tracker

`security_tracker.rs` fetches `https://security.archlinux.org/issues/all.json` (about 900 KB, 2400 groups), keeps it in
`~/.local/share/emrac/security-tracker.json` and uses the saved copy for 24 hours; older, it asks again; offline or failing, it uses the saved
copy and says it is old; with nothing saved, it says the tracker was not checked and takes nothing off for what it could not look at. The
tracker does not date its entries, so an old open entry with no recorded fix can be stale (a 5.15-era `linux` entry still applies to
a 7.x kernel by the rule): that is why each applying entry is listed with its id, severity, `affected` version and CVEs, so you
can judge it. A clean answer is never called a clean bill: the tracker lists only what its maintainers have recorded.

## Not covered

An overall number that blocks anything (the score is advice); data from sources other than the tracker (OSV, upstream release
history, OpenSSF Scorecard); a score for a package that is neither in the repositories nor on the AUR.

## Design

`emrac-core/src/score.rs`: `Facts`, `Dimension`, `Rule`, `Card`, `card` (pure, table-driven, tested at every limit).
`backend/alpm.rs`: `repo_meta` (build date, signature, how many repository packages depend on it).
`Sources::score_facts` gathers the inputs. `commands/score.rs` prints; `view.rs` draws the bars on a terminal.

## Verification

Unit tests at both sides of every limit, for the adjustments and caps, for unknowns, for the AUR and the official paths and for the
JSON; a container check `dev/e2e/score.sh` (an official package, an unknown package, an AUR package's offline note, `--json`,
`--details`, the benchmark feeding Performance).

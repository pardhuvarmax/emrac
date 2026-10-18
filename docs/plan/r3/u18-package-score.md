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
data is shown as unknown (`-`), never as a made-up number**. There is deliberately **no overall score**: the six answer different
questions (a package can be a fine program and a slow build), and an average would hide which is which. The header says how many of the
six are known.

## The six, and what each is made of

Each dimension is a score from 0 to 100 (100 best) found in a table of limits, with the rule shown by `--details`.

| Dimension | Input | Rule |
|---|---|---|
| **Performance** | the stored `emrac benchmark` results of the package (installed programs only) | the median warm start of its programs: up to 10 ms 100, 50 ms 85, 200 ms 65, 1 s 40, slower 15; minus 10 if the biggest peak memory is over 256 MiB, 25 if over 1 GiB. No benchmark: unknown, with the command to run |
| **Build time** | your own earlier builds of it (median), else the `recommend` estimate | up to 1 min 100, 5 min 85, 15 min 65, 1 h 40, 3 h 20, longer 5. Says which: measured or estimated |
| **Maintenance** | official: the build date in the repository. AUR: when the page last changed | up to 30 days 100, 90 days 90, 180 days 75, a year 55, two years 30, older 15. AUR: minus 30 when flagged out of date; at most 40 when it has no maintainer |
| **Security** | official: whether the database carries its signature, and the Arch news. AUR: the PKGBUILD audit and the trust signals | official: 95 if signed, 50 if not; minus 15 for each Arch news item of the last 90 days that names it (at most 30). AUR: 80 as the starting point (community-uploaded, not reviewed by Arch); a critical audit finding makes it 0; each high finding minus 30; each warning minus 10 (at most 30); each high trust signal minus 25, each warning minus 10. Offline: not audited, and it says so |
| **Popularity** | AUR: votes and popularity. Official: nothing | votes: none 10, under 10 30, 50 50, 200 70, 1000 85, more 100; popularity: under 0.01 20, 0.1 40, 1 60, 5 80, more 100; the average of the two. Official: unknown (the repositories keep no vote data), with the number of packages that depend on it as a fact |
| **Source worthiness** | the `recommend` engine's score | that score as it is; unknown when there is no choice (no binary to compare with) |

## Not covered

An overall number; vulnerability databases (CVEs): emrac keeps none, and says "not checked" rather than imply a clean bill; a score for
a package that is neither in the repositories nor on the AUR; weighting by what you care about.

## Design

`emrac-core/src/score.rs`: `Facts`, `Dimension`, `Rule`, `Card`, `card` (pure, table-driven, tested at every limit).
`backend/alpm.rs`: `repo_meta` (build date, signature, how many repository packages depend on it).
`Sources::score_facts` gathers the inputs. `commands/score.rs` prints; `view.rs` draws the bars on a terminal.

## Verification

Unit tests at both sides of every limit, for the adjustments and caps, for unknowns, for the AUR and the official paths and for the
JSON; a container check `dev/e2e/score.sh` (an official package, an unknown package, an AUR package's offline note, `--json`,
`--details`, the benchmark feeding Performance).

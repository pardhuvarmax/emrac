# Slice u2 (after r3) — `emrac recommend`: source or binary, from rules

The first of five slices that build the source-or-binary engine (`SPEC.md` Part III #3 and #4,
Part IV #2, #3 and #7):

| Slice | What |
|---|---|
| **u2** | `emrac recommend <pkg>`: the engine and its verdict, and a record of how long builds take |
| u3 | `emrac explain <pkg>`: the full reasoning, input by input, and what would change it |
| u4 | `emrac score <pkg>`: the report card (performance, build time, maintenance, security, popularity, source worthiness) |
| u5 | `emrac estimate` / `eta`: build and install time, refined while a build runs |
| u6 | `emrac benchmark <pkg>`: measures a package after installation and feeds the engine |

`recommend` only advises. It changes nothing, needs no root, and `install` and `upgrade` do not
use it yet (the `--source` / `--binary` / `--auto` flags are a later slice). The SPEC's rule
applies: no AI and no hidden weights; every number comes from a stated input and every point
from a stated rule.

## What it answers

*"Is it worth building this from source on this machine, or should I install the binary?"*

```
emrac recommend ripgrep
emrac recommend source ripgrep      # the source side only: cost and gain
emrac recommend binary ripgrep      # the binary side only
emrac recommend firefox --json
```

Output: the verdict (**binary**, **source**, **either** or **source (the only option)**), the Source
Suitability Score (0 to 100) where there is a choice, the confidence (low, medium, high) with the
reason for it, and the reasons in order of weight, one line each. `explain` (u3) shows all of them.

## What a "source" and a "binary" are here

- **Binary:** the package in an official repository, installed with pacman. Or, for an AUR package
  that has a `-bin` sibling, that sibling.
- **Source:** building from a PKGBUILD. Today emrac builds AUR packages and local PKGBUILD folders.
  For an official package the source is the distribution's own PKGBUILD; `recommend` can judge it,
  but `install --source` for it is not built yet and the report says so.
- An AUR package with no `-bin` sibling has **no binary**: the verdict is *source (the only
  option)* and the report gives the cost, not a score.

## The inputs, and where each comes from

| Input | Source | Used for |
|---|---|---|
| Does the build differ from the repository's? | `/etc/makepkg.conf` and `~/.makepkg.conf` (`CFLAGS`, `-march`, `-O`, `LTO`), `pacman.conf` (a `-v3` or `-v4` repository) and the CPU's instruction-set level from `/proc/cpuinfo` | the **gain**: with Arch's default flags a local build produces the same code as the repository |
| Package size and shape | the sync database (installed size, download size, architecture `any`, dependencies) or the AUR RPC (`MakeDepends`, votes, popularity) | the **cost** and the toolchain class |
| Dependency weight | the transitive dependency closure (`deps`) | build and install cost, risk |
| Build time | `~/.local/share/emrac/build-times.jsonl`: every build emrac runs is timed from this slice on; with none, a stated estimate from size and class | the **cost**, and the **confidence** |
| How often it changes | emrac's own history (installs and upgrades of this package in 90 days), the AUR's last-modified time | each update is another build: the **churn** |
| This machine | cores, memory, free disk | whether a build would fit |

## The rules

The Source Suitability Score starts at 50.

1. **Gain.** No difference from the repository's flags: the score is capped at 30 and the first
   reason says why ("your makepkg flags are Arch's defaults: a local build gives the same code").
   `-march=native`, an `x86-64-v3` or `-v4` target the CPU supports: +25. `-O3` or LTO: +5 each.
   An architecture-independent package (`any`): nothing to compile, score 0, binary.
2. **Cost** (build time on this machine): under 2 minutes 0; to 10 minutes −10; to 30 −20; to 60
   −30; over 60 −40.
3. **Churn:** 6 or more updates in 90 days −15; 3 to 5 −8.
4. **Trust:** a binary in an official repository is signed by Arch's keys; a source build is only as
   trusted as its audit: −5 for source when an official binary exists.
5. **Fit:** not enough free memory or disk for the estimated build: −20, and the reason says what is
   short.

Verdict: 65 or more **source**; 35 or less **binary**; between, **either** (binary is the default
if `install` is ever asked to choose). Rules are listed in `docs/design/recommend.md` with the
constants, so each is one line to change.

## Time estimates, honestly

With no measured build the estimate is `installed size × seconds-per-MiB(class) ÷ usable cores`.
The class comes from `MakeDepends` (AUR) or from the dependencies (official: `gcc-libs` means
C++, `rust` or `cargo` means Rust, and so on). The constants are rough and the estimate is shown
as a range (half to double) with **low** confidence. One measured build of the package makes it
**medium**, three or more **high**. `benchmark` (u6) and the build log are how this gets better.

## Recording build times

`emrac build`, an AUR `install` and `upgrade`, and `rebuild` append one line per finished build to
`build-times.jsonl` (time, package, version, seconds, cores, outcome). Kept to the newest 500
lines. Nothing leaves the machine.

## Not in this slice

`explain` (u3), `score` (u4), ETA and live refinement (u5), `benchmark` (u6); using the
recommendation in `install`/`upgrade` (`--auto`, `--source`, `--binary`); building official
packages from source; profiles and recipes; fetching the official PKGBUILD.

## Verification

Unit tests for every rule and the cap, the verdict boundaries, the confidence levels, the class
guess, the makepkg.conf and CPU parsing, the history count and the build-times file (written,
capped, read back). Container: a fixture repository (a package with `arch=any`, a C-like and a
C++-like one) and a fixture AUR answer; the output of `recommend` for each, and a build timed by
`emrac build` showing up in the next `recommend`.

# `emrac recommend`: source or binary

For people who use emrac and for whoever changes the rules. The plan is
[`../plan/r3/u2-recommend.md`](../plan/r3/u2-recommend.md); the code is `crates/emrac-core/src/recommend.rs`.

`emrac recommend <pkg>` answers one question: **is it worth building this package from source on this
machine, or should the binary be installed?** It only advises. It changes nothing, needs no root, and
`install` and `upgrade` do not use it yet. There is no learning and no hidden weight: every number
comes from a named input, every point from a rule below, and the output says which.

```
emrac recommend ripgrep           # both sides and the verdict
emrac recommend source ripgrep    # the source side only
emrac recommend binary ripgrep    # the binary side only
emrac recommend ripgrep --json    # the inputs and the whole report
```

## What "source" and "binary" mean

- **Binary:** the package in an official repository, or, for an AUR package, a `<name>-bin` sibling.
- **Source:** building from a PKGBUILD. Today emrac builds AUR packages and local PKGBUILD folders; for
  an official package the source is the distribution's own PKGBUILD, which `recommend` can judge but
  `install --source` cannot yet build.
- No binary exists (an AUR package without a `-bin` sibling): the verdict is *source (the only option)*,
  the cost is shown and nothing is scored.

## The inputs

| Input | Where it comes from |
|---|---|
| Makepkg flags | `CFLAGS` and `MAKEFLAGS` of `/etc/makepkg.conf`, then `~/.makepkg.conf`, then `~/.config/pacman/makepkg.conf` (the last assignment wins; a line ending in `\` continues) |
| A tuned repository | a section of `/etc/pacman.conf` whose name has `-v3`, `-v4` or `x86-64-v` |
| CPU level | the flags line of `/proc/cpuinfo`: v2 (`cx16 popcnt ssse3 sse4_1 sse4_2`), v3 (`avx avx2 bmi1 bmi2 fma movbe f16c abm`), v4 (`avx512f/bw/cd/dq/vl`) |
| Size, architecture, dependencies | the sync database (installed and download size, `any`, the dependency list); the transitive count follows the sync databases |
| Toolchain | AUR: `MakeDepends` from the AUR RPC (`rust`/`cargo` is Rust, `go`, `jdk*`, `cmake`/`meson`/`ninja`/`clang`/`qt*` C++, `make`/`gcc` C, `python-build` and the like nothing to compile). Official: the architecture (`any`: nothing to compile) and the dependencies (`gcc-libs`, Qt, Boost, LLVM: C++ or Rust; otherwise unknown). A `-bin` package: nothing to compile |
| Build time | `~/.local/share/emrac/build-times.jsonl`, one line per build emrac ran (`build`, AUR `install` and `upgrade`, `rebuild`), only successful builds on a machine with this many cores; without one, the estimate below |
| How often it changes | emrac's own history: committed installs and upgrades of the package in the last 90 days |
| This machine | cores, `MemTotal`, free space under `~/.cache` |

## The rules

The score is the Source Suitability Score, 0 to 100. It starts at **50**.

| Rule | Condition | Points |
|---|---|---|
| nothing-to-compile | architecture `any`, or a `-bin` package | the verdict is binary, score 0, confidence high |
| gain-tuned | `-march=` is `native` or a CPU name, or an x86-64 level above v1 that the CPU supports and no repository already ships | +25 |
| gain-none | otherwise: the flags are the repository's | 0, and the score is **capped at 30** |
| gain-unsupported | `-march=` asks for a level the CPU does not run | 0, capped at 30 |
| gain-opt | `-O3` (the repository builds with `-O2`) | +5 |
| cost | build time on this machine: under 2 min; to 10; to 30; to 60; over 60 | 0; −10; −20; −30; −40 |
| churn | updates in 90 days: 0–2; 3–5; 6 or more | 0; −8; −15 |
| trust | an official binary exists (signed by Arch's keys; a source build is as trusted as its audit) | −5 |
| fit | memory below the toolchain's peak (4 GiB for C++ and Rust, 2 for Java and unknown, 1 otherwise) or under 2 GiB of disk | −20 |

Verdict: **65 or more: source; 35 or less: binary; in between: either** (the binary is the default
if a command ever has to choose). The dependency tree is shown as a note, not scored.

`emrac explain <pkg>` shows all of this at once: every rule with its points, its cap and the source of its input, the total before and after the cap, and *what would change this* (below). It reads the same `evaluate` result; nothing is computed twice.

The gain is a possibility, not a measurement: it says the local build *can* use instructions the
repository's generic build cannot. `emrac benchmark` (u6) is what will measure it.

## Build time

With no measured build: `installed size × seconds per MiB for the toolchain ÷ usable cores`, never less
than a floor for the toolchain, shown as a range (half to double):

| Toolchain | s per MiB (one core) | floor |
|---|---|---|
| nothing to compile | 0.05 | 1 s |
| C | 5 | 20 s |
| C++ | 60 | 60 s |
| Rust | 25 | 90 s |
| Go | 6 | 40 s |
| Java | 8 | 60 s |
| unknown | 12 | 45 s |

Usable cores are `1 + 0.5 × (n − 1)` for `n` cores, or `-j` from `MAKEFLAGS` if smaller. The AUR carries
no sizes, so for an AUR package without a measured build the estimate is a typical time for the
toolchain. These constants are rough, which is why the estimate is a range with **low** or **medium**
confidence; one measured build makes it **medium**, three **high**.

## Confidence

- **High:** the verdict follows from the flags (the cap) or from there being nothing to compile, or
  three or more builds of the package were measured here.
- **Medium:** one build was measured; or the build time is only estimated but the verdict is the same
  at half and at double the estimate.
- **Low:** the verdict changes within the estimate's range. The report says to build it once.

## Limits

An estimate from size cannot know that a small Rust program pulls in hundreds of crates; that is why
the floors exist and why a measured build replaces the estimate at once. Official packages have no
`makedepends` in the sync database, so their toolchain is a guess from the dependencies. The update
rate counts only what emrac itself did. None of this reads the package's own PKGBUILD yet.

## `emrac explain`

`emrac explain <pkg>` (u3, [`../plan/r3/u3-explain.md`](../plan/r3/u3-explain.md)) prints the reasoning behind `recommend` (real output, on a machine with `-march=native` in `makepkg.conf`):

```
Recommendation for ripgrep: source    confidence: medium — the build time is only estimated, but the verdict is the same at half and at double the estimate

  ✓ start                                   50
  ✓ your flags target this CPU             +25
      your makepkg flags target native: a local build can use instructions the repository's generic build cannot (a possible gain, not a measured one; `emrac benchmark` will measure it)
      from: CFLAGS `-march=native -mtune=native -O2 -pipe -fno-plt -fexceptions -Wp,-D_FORTIFY_SOURCE=3 -Wforma…
  ✓ build time                               0
      a build here takes about 45 s, between 22 s and 2 min (estimated: its dependencies say only that it is a native program (C, Rust or Go))
      from: its dependencies say only that it is a native program (C, Rust or Go), 24 cores
  ✗ official binary is signed               -5
      the official binary is signed by Arch's keys; a source build is as trusted as its audit
      from: the package comes from an official repository

  Score 70 of 100 → source (65 or more)
  Note: 3 direct and 13 transitive dependencies

What would change this (the engine run again with only that changed):
  - `-O3` in makepkg.conf: the score would be 75 (source)
  - three builds of this package measured here: the confidence would be high
```

Every rule that applied is listed, in the order the engine applied it; ✓ marks a rule that adds or is neutral, ✗ one that
costs points or caps the score. **What would change this** is not a second set of rules: for each input that is not at its
best, a copy of the facts with that one input changed goes through `evaluate`, and only the changes that alter the score, the
verdict or the confidence are shown (so `-O3` is not offered while the cap holds the score where it is). The alternatives are
`-march=native`, `-O3`, a quick build, a package that changes rarely, enough memory and disk, and three measured builds.
`--json` carries the facts, the rules and the alternatives. `emrac recommend` ends its list with a pointer to `explain`.

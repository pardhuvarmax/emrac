# Slice u15 (after r3) — the Package Advisor

`SPEC.md` Part IV "additional ideas": *"warns before installing packages with unusually large dependency trees or disk impact."*

Under the plan of an `install` or `upgrade`, before the question, emrac now adds **advice** when the plan is
unusually big. It is only advice: nothing is blocked, the plan id is unchanged, and it is not part of the plan (it depends on the
machine).

```
emrac install texlive-meta
...
Advice:
  Large download: this install downloads 2.1 GiB (advice starts at 1.0 GiB). Biggest: texlive-fontsextra 1.2 GiB, texlive-latexextra 301 MiB, texlive-pictures 212 MiB.
  Many new packages: it pulls in 61 packages you did not ask for (for 1 you did).
    to see what it pulls in: emrac tree texlive-meta --depth 2
  A lot of your free space: it needs 4.8 GiB more, 31% of the 15 GiB free on /.
```

## The rules

Each is a fact worked out from the plan and the repository databases, with a stated threshold; none is a guess.

| Advice | When | Setting |
|---|---|---|
| Large download | the plan downloads at least this much | `advisor.large_download_mib` (1024) |
| Large disk growth | the installed size grows by at least this much | `advisor.large_growth_mib` (2048) |
| A lot of your free space | the growth is at least a quarter of the free space on `/` and at least 256 MiB | none |
| Many new packages | at least this many dependencies come along | `advisor.many_dependencies` (40) |
| One package is most of it | one package is at least 60% of the installed size of a plan of two or more, and 300 MiB or more | none |

`advisor.enabled false` turns it all off. Only official repository packages have a known size; an AUR package is not
counted until it is built, and the advice says so when the plan has some. Removals and downgrades get none.

## Where it shows

Under the plan in `install` and `upgrade`, with the hooks and `.pacnew` previews, and as `advice` in `--json`.
`emrac config` gets an `[advisor]` section.

## Design

`emrac-core/src/advisor.rs`: `Input`, `Thresholds`, `Advice`, `advise` (pure, tested). `Sources::advice` gathers the sizes
from the sync databases and the free space. `PlanPreview.advice`. `Config.advisor` with the four settings.

## Verification

Unit tests for each rule on both sides of its threshold, for the ordering and wording, for the settings; a container check
(`dev/e2e/advisor.sh`) that lowers the thresholds to see the advice on a small package, sees none by default, and sees none with
`advisor.enabled false`.

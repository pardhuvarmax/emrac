# Source-based installation: the system plan (against what exists)

Principle, unchanged: **emrac owns source-build orchestration and policy; pacman/libalpm owns installation.** Nothing is copied into the filesystem by emrac; a source build produces a standard `.pkg.tar.zst` that `pacman -U` installs.

## The target-state invariant (new, core, for binary and source alike)

> Every planned package operation is resolved against one coherent target system state. Emrac must not knowingly commit a transaction that leaves an unsupported mixture of package versions or incompatible dependencies. Binary and source-built packages follow the same planner and the same validation.

It is a transaction-engine requirement, not a feature of source builds: rebuilding from source does not fix an inconsistent plan, it can reproduce it. Registered as invariant 11 in `docs/design/safety-invariants.md`.

## Where the plan stands today

| Plan part | State |
|---|---|
| Official package from Arch's packaging source, audited, sandboxed, inspected, `pacman -U` | built (u23) |
| `emrac build` without installing | exists for AUR and local PKGBUILD directories; not for an official package by `--source` |
| Persistent source intent, separate from pins, reconciled with the installed version | built (u23b); reconcile = "unknown build intent"; no external-removal detection beyond "not installed" |
| Upgrade honours the policy, never a silent binary fallback | built, **two-phase** (binary first, rebuild after, failure = pending) |
| Partial-upgrade guard | built (pre-flight `partial-upgrade`, fresh syncs, plan id re-check) |
| Clean chroot (`devtools`) builds at the target state | not built (bubblewrap over the host; `mkarchroot`/`makechrootpkg` exist on this machine) |
| Provenance record (recipe revision, source checksums, build deps, package hash) | partial: recipe revision only |
| Artifact store, `source status / inspect / rebuild` | not built (`from-source list/rebuild` is the start) |
| Structured events shared by CLI and TUI | not built |

## Decision: the order of a source-policy upgrade is chosen per run

1. **Default: two-phase** (as built). `pacman -Syu` installs the binaries, each policy package is rebuilt after, a failure leaves the binary, the policy and a pending mark; the exit is non-zero. This keeps rule 23: a failing recipe never blocks the rolling upgrade.
2. **`upgrade --build-first`** (new): every policy package is built first, in a clean chroot synced to the **target** repository state (the databases the plan was made from), each artifact validated; only then the single transaction installs repository packages and the built archives together (`pacman -U` of the archives alongside `-Syu` is not one pacman call, so the plan is: `-Syu` with the built packages ignored not possible; the transaction is `pacman -U <archives>` after `-Syu`-equivalent resolution — see open question 1). Any build failure stops **before** the host changes: nothing is installed, the plan is shown again with the cause. Mixed state cannot result from a build failure.

## Slices

- **S1, `emrac build --source <official pkg>`** (build and validate, no install; log and provenance kept). Uses today's sandbox. Small.
- **S2, provenance + artifact store**: `source-built.json` gains recipe hash, source checksums, build dependency versions, package hash, artifact path; `emrac source status | inspect | rebuild` (folded with `from-source`, which stays an alias). External replacement detected by comparing the installed package's build date/packager with the record.
- **S3, clean-chroot builder** (`devtools`, a chroot kept at the target state, resource limits, no host mounts): replaces bubblewrap-on-host for official-package builds; this is the isolation of Build Profiles & Recipes (u28).
- **S4, `upgrade --build-first`** on top of S3, with the target-state check before and after the builds.
- **S5, dependency order** among policy packages and ABI-impact decisions (rebuild dependents only when the change affects them).
- **S6, structured events** for CLI and TUI. Later: system-wide source policy.

## Open questions

1. How the built archives and the repository upgrade become one transaction: pacman cannot take `-Syu` and local `-U` files in one call. Options: (a) a local repository of the built archives added to a temporary pacman config, so one `pacman -Syu` resolves both (preferred: one transaction, standard resolution); (b) `-U` of the archives after `-Syu` (back to two commits).
2. Whether `build-first` may use the chroot's dependency set when the host has not yet been upgraded to the target (the chroot is at the target state by construction; the host is not).

Nothing here changes what is built today. Order of work: S1, S2 with the safety P0 items, then S3 and S4 inside u28/u29.

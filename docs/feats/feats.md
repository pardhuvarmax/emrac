# Emrac: every feature, in one file

One list of everything emrac has, is specified to have, or has been proposed: what is built, what is partly built, what is specified or only an idea, what is research, and what was decided against. It collects the other documents instead of replacing them; each row says where the detail lives.

State of the tree: release `r3` plus the update slices `u1` to `u8` after it (commit `9b51a2d`). Built things are checked against the code and `CHANGELOG.md`; the older status tags inside `ideas.md` and `additions.md` are out of date, so this file follows the code.

## Status legend

| Status | Meaning |
|---|---|
| Built | Works today, with a test or a container check. |
| Partial | Part of it works; the row says what does not. |
| Specified | Described in `SPEC.md`, not started. |
| Idea | Proposed in an idea document, not decided or started. |
| Research | A research task, not a product feature. |
| Not planned | Decided against, or deliberately far future. |

## Counts

| Section | Built | Partial | Specified | Idea | Research | Not planned | Total |
|---|---|---|---|---|---|---|---|
| 1. Built: what works today | 56 | 0 | 0 | 0 | 0 | 0 | 56 |
| 2. The specification (SPEC.md): catalogue and signature features | 0 | 32 | 24 | 0 | 0 | 0 | 56 |
| 3. Ideas from additions.md (A, B, C) | 13 | 8 | 0 | 18 | 0 | 0 | 39 |
| 4. Ideas from ideas.md | 10 | 7 | 1 | 38 | 0 | 0 | 56 |
| 5. Wandrover: cross-distribution package resolution | 0 | 0 | 0 | 9 | 0 | 0 | 9 |
| 6. Self-management service (Emrac-SMS) | 5 | 0 | 0 | 0 | 0 | 0 | 5 |
| 7. AUR defense (planned for r4) | 1 | 4 | 0 | 5 | 0 | 0 | 10 |
| 8. Package forensics (proposed) | 0 | 2 | 0 | 7 | 0 | 0 | 9 |
| 9. Plugin system (proposed) | 0 | 0 | 1 | 16 | 0 | 0 | 17 |
| 10. Research programme (REA, containers, other package managers) | 1 | 1 | 0 | 20 | 9 | 0 | 31 |
| 11. Newer ideas (not yet in the other documents) | 0 | 1 | 0 | 26 | 0 | 0 | 27 |
| 12. Decided against, or far future | 0 | 0 | 0 | 0 | 0 | 3 | 3 |
| **All** | **86** | **55** | **26** | **139** | **9** | **3** | **318** |

Some features appear in more than one section (soak time, the offline bundle, Wandrover's migration mode); each section counts its own rows.

## Contents

- [1. Built: what works today](#1-built-what-works-today)
- [2. The specification (SPEC.md): catalogue and signature features](#2-the-specification-specmd-catalogue-and-signature-features)
- [3. Ideas from additions.md (A, B, C)](#3-ideas-from-additionsmd-a-b-c)
- [4. Ideas from ideas.md](#4-ideas-from-ideasmd)
- [5. Wandrover: cross-distribution package resolution](#5-wandrover-cross-distribution-package-resolution)
- [6. Self-management service (Emrac-SMS)](#6-self-management-service-emrac-sms)
- [7. AUR defense (planned for r4)](#7-aur-defense-planned-for-r4)
- [8. Package forensics (proposed)](#8-package-forensics-proposed)
- [9. Plugin system (proposed)](#9-plugin-system-proposed)
- [10. Research programme (REA, containers, other package managers)](#10-research-programme-rea-containers-other-package-managers)
- [11. Newer ideas (not yet in the other documents)](#11-newer-ideas-not-yet-in-the-other-documents)
- [12. Decided against, or far future](#12-decided-against-or-far-future)
- [Where each thing is documented](#where-each-thing-is-documented)

## 1. Built: what works today

Everything below runs in the current tree (r3 plus the updates after it). Slice names: plain `uN` is update N between r1 and r2; `r2 uN` is update N after r2 (shipped in r3); `r3 uN` is update N after r3; `i1` is the first incubation. `FEATURE_STATUS.rst` and `CHANGELOG.md` carry the detail.

| Feature | Status | What it is | Where |
|---|---|---|---|
| search / info | Built | Official repositories (libalpm) and the AUR (RPC v5) behind one interface; `--official`/`--aur`, `--offline`, `--json`; AUR trust signals shown by `info`. | i1, r1; `FEATURE_STATUS.rst` |
| install / remove / upgrade | Built | Official repos and the AUR: preview, confirmation, `--dry-run`, `--yes`, `--json`, AUR PKGBUILD or diff review; mutations go through `sudo pacman`, libalpm only reads. | i1, r1 |
| emrac's voice | Built | Twelve message verbs (says, found, notes, plans, asks, warns, suspects, ...) so a line says what kind of statement it is. | r2 u6; `docs/plan/r2/u6-voice.md` |
| exit codes, broken pipe | Built | Documented exit codes pinned by tests; `emrac ... \| head` ends quietly. | u14 |
| config | Built | `config show\|get\|set\|unset\|edit\|validate` over `~/.config/emrac/config.toml` (flags, review, search, snapshot, build limits, cache, safety, self, `build.remove_make_deps`). | u1; later keys per slice |
| shell completion | Built | bash, zsh and fish scripts generated from the real command definitions. | u2 |
| Plan preview that matches reality | Built | Plans come from freshly synced repositories (not a stale list), list what conflicts and replacements will remove, provider choices and built packages, with sizes. | u22, u24, u25 |
| pacman's own prompts pass through | Built | Without `--yes` pacman asks its questions; silence is never consent (a conflict's default is no). | u23 |
| Plan ids and `--expect-plan` | Built | A SHA-256 digest of what a plan does; `--expect-plan` runs only that plan; a plan that waited 30 s is made again before the commit; drift is shown as a diff. | r2 u18; `docs/plan/r2/u18-plan-integrity.md` |
| Pre-flight checks | Built | Before a transaction: space on `/`, the cache, `/boot`, the EFI partition; battery; mirrors; the kernel safety net; pacman's lock. | r2 u17 |
| Partial-upgrade gate | Built | Refuses installing or upgrading only some packages while upgrades wait (`--allow-partial` for one run; a pin does not count); refuses an older version than installed. | r2 u17, u20 |
| Update risk and post-upgrade report | Built | LOW/MEDIUM/HIGH under an upgrade plan; after it (and `emrac report`): reboot needed, services to restart, new failures, broken AUR packages, new `.pacnew` files. | r2 u20; `docs/design/trust.md` |
| Arch news gate | Built | `news` reads the Arch news and tracks what is unread; critical items (breaking, manual intervention, kernel, bootloader, C library, systemd, pacman, drivers, filesystem) stop an upgrade until acknowledged. | r2 u20 |
| Hook preview | Built | Plans list the pacman hooks that will run, in pacman's order, and which would stop the transaction (`AbortOnFail`, missing `Depends`); hooks a package brings are named. | r3 u4; `docs/design/hook-preview.md` |
| Config-file preview | Built | Plans list the config files you edited that will get a `.pacnew` or `.pacsave`, by pacman's three-way rule (certain when the new package is cached, else "may"); `NoUpgrade` included. | r3 u5; `docs/design/config-preview.md` |
| Policy preview | Built | Plans list the overridden and diverted files they touch and warn when a package changes a file you diverted. | r3 u8 |
| history | Built | Append-only log of executed transactions, failures and conflict removals (`transactions` alias); `--limit`, `--since`, `--json`. | u3 |
| History integrity | Built | Each record chained by SHA-256 to the one before; `history verify`, `archive --older-than`, `ack`. | r2 u7 |
| Ledger | Built | A second copy of every history line in a root-owned append-only file (`chattr +a`); compare and rebuild. | r2 u8; `docs/design/ledger.md` |
| Ledger watch | Built | Kernel audit rules that log any change to the ledger, and telling emrac's own appends from anything else. | r2 u9 |
| Journal | Built | A write-ahead journal and lock for every mutating command: a transaction that was cut off is found, assessed from pacman's log and recorded (`history recover`). | r2 u10; `docs/design/journal.md` |
| rollback / undo / redo | Built | Reverse and re-apply recorded transactions from the pacman cache, the AUR build folder, or the Arch Linux Archive. | u4, u5 |
| downgrade | Built | One package back to an older version (`--list` shows cache, archive, installed). | u6 |
| reinstall | Built | The installed version again (repair, re-run scripts), never a partial upgrade. | u7 |
| purge | Built | Remove with its modified config files (`pacman -Rn`), with a report of what is lost. | u8 |
| verify / check | Built | Installed files against pacman's mtree record; a mode you overrode and a file you diverted are shown as expected. | u9, r3 u8 |
| snapshot / restore | Built | Named restore points of the installed package set; optional automatic one before `upgrade`. | u10, u11 |
| pacman database protection | Built | A backup of the local database before each transaction, `pacman -Dk` before and after, the restore command. | r2 u20 |
| Repository trust | Built | An older repository database than one seen is a warning; an older version than installed is refused in pre-flight. | r2 u20 |
| After a failed transaction | Built | Packages pacman still lists with files missing (what a failed install leaves behind) are reported with the command that mends each; also after `history recover`. | r3 u6; `docs/design/reconcile.md` |
| deps / rdeps / why / orphans | Built | Read-only dependency views with what satisfies each dependency. | u12 |
| tree / graph / impact / cycles | Built | Dependency tree, Graphviz and JSON graph, what a change affects, rings of packages that need each other. | r2 u16 |
| pin / unpin | Built | Hold a package back from `upgrade` with a reason and an expiry. | u13 |
| audit | Built | Static reading of a PKGBUILD, scriptlets and shipped files (nothing executed) with info/warning/high/critical tiers; high needs `--accept-risks`, critical is refused. | u15, u16, u18 |
| Build/install split and package manifest | Built | AUR builds make the package without installing it; the built package is inspected (setuid files, hooks, sudoers, accounts, units, scripts) before `pacman -U`. | u17 |
| Sandboxed AUR builds | Built | Dependencies and verified sources on the host, extraction and build in bubblewrap with the network off (warn-and-ask to allow it). | u19; `docs/design/source-build.md` |
| Build resource limits | Built | Time, memory, process and size limits for the sandboxed steps, with the reason a build was killed. | u20 |
| `emrac build` | Built | Build an AUR package or a local PKGBUILD folder without installing. | u19 |
| AUR trust signals | Built | Lookalike names, a claim on an official package, orphaned/new/few users, changed maintainer, sources changed under the same version, a new source host, changed signing keys. | r2 u20 |
| rebuild | Built | Rebuild installed AUR packages through the audited path; find the ones a library, Python or Qt update broke or may have broken (`--check`, `--stale`). | r2 u19 |
| Build tool removal | Built | Packages an AUR build installed only to build are offered for removal at the end of the run (`build.remove_make_deps` = ask/always/never). | r3 u7; `docs/design/build-tools.md` |
| recommend | Built | Source or binary from stated rules: score, verdict, confidence, build time here (measured or estimated). | r3 u2; `docs/design/recommend.md` |
| explain | Built | How `recommend` was reached, rule by rule, with where each input came from and what would change the answer. | r3 u3 |
| pacnew | Built | Lists `.pacnew`/`.pacsave`, tells your edits from upstream's by a three-way comparison, keeps/takes/merges/edits after validating the result, records and can revert every change. | u27, u30 |
| cache | Built | Source, build, binary and pacman caches: list, stats, clean, purge, trim, `--lean`. | u28, u29 |
| override | Built | Keep the mode and owner of a packaged file across upgrades and reinstalls, by a pacman hook (dpkg's `statoverride`, for pacman). | r3 u8; `docs/design/local-file-policy.md` |
| divert | Built | Keep your own version of a packaged file across upgrades; a diversion freezes the file, so it asks first, tracks what it diverted from, and warns (STALE) in pacman's output, list, status, plans and `doctor`. | r3 u8 |
| policy status / apply | Built | Is the local file policy in effect and will it stay so; apply it now. | r3 u8 |
| doctor | Built | Read-only health checks with a fix hint each (dependencies, database age, pending upgrades, kernel, disk, caches, orphans, `.pacnew`, failed units, pins, tools, config, sandbox, history, ledger, stale AUR builds, stale diversions, policy); `--report` makes a sanitized bug report. | u21 and later |
| self check / diagnose | Built | Read-only checks of emrac's own installation and what an upgrade needs; work without pacman, network or root. | r2 u11, u13 |
| self sync / news | Built | A validated, staged copy of the project's release index and news (gpg signature checked if you require it). | r2 u12; `docs/design/release-index.md` |
| self upgrade / downgrade | Built | Download, check against the index, build and install with pacman; a copy no package owns is updated in place (`cargo install` copies). Aliases `up`, `dn`. | r2 u14, u15; r3 u1; `docs/design/self-upgrade.md` |
| install.sh | Built | Installs emrac from a pinned, checksum-verified release tarball without the AUR. | r2 u5 |
| Release pipeline | Built | `dev/release.sh` prepare/publish, a checked and reviewed release workflow, `releases.json` and news. | r3 small changes; `docs/design/releasing.md` |
| Test infrastructure | Built | A generated list of every test (`docs/tests/emrac-tests.rst`), CI as an ordinary user, and container end-to-end suites against real pacman (`dev/e2e/`). | r2 u1 and each slice |
| License | Built | GPL-3.0-or-later. | r2 u2 |

## 2. The specification (SPEC.md): catalogue and signature features

From `SPEC.md` Parts III to V, with the status from `FEATURE_STATUS.rst`. "Specified" means described in the spec and not started; the spec scopes Parts III-beyond-MVP and IV for after the core loop, so these are on schedule, not slippage.

| Feature | Status | What it is | Where |
|---|---|---|---|
| III.1 Unified Package Management | Partial | install/upgrade/downgrade/remove/purge/reinstall/verify, dry-run, plans, history and rollback exist. Not yet: manual source/binary override on install, binary-vs-source install choice. | `SPEC.md` |
| III.2 Source Build System | Partial | AUR fetch, review, audit, sandboxed two-step build, caches. Not yet: incremental/parallel builds, build queue, background builds, pause/resume, log collection, replay. |  |
| III.3 Intelligent Recommendation Engine | Partial | `recommend` (score, verdict, confidence, build time) and `explain`. Not yet: performance-gain estimate by measurement, historical learning, personalized recommendations. |  |
| III.4 ETA Prediction Engine | Specified | Source, download, install and total ETA with live refinement and confidence. |  |
| III.5 Package Discovery | Partial | Name/description search only. Not yet: fuzzy, regex, file-ownership, maintainer, category, popular and recent searches. |  |
| III.6 Package Inspection | Partial | `info`, `rdeps`, `why`, `audit`, `verify`, `--inspect` together; not the single aggregated view (see IV.13). |  |
| III.7 Dependency Intelligence | Partial | Everything except an interactive explorer (needs the TUI). |  |
| III.8 Repository Management | Partial | Official and AUR only. Not yet: local, custom and git repositories, priorities, mirror management, repository health. |  |
| III.9 News & Advisory System | Partial | Arch news with read/unread and a critical gate. Not yet: package-specific news, security advisories, severity filtering. |  |
| III.10 Build Profiles | Specified | Built-in and custom profiles (compiler, linker, optimization, LTO, PGO, tuning, debug, strip, jobs, cache policy). |  |
| III.11 Compiler & Build Optimization | Specified | GCC/Clang, linker choice, CPU tuning, LTO, PGO, custom flags. `recommend` already reads makepkg flags, but nothing sets them. |  |
| III.12 Build Monitoring | Partial | Live sandbox output and why a build was killed. Not yet: progress, stage tracking, summaries, reports. |  |
| III.13 Cache Management | Partial | `cache` over four categories with lean policy. Not yet: browsable explorer, selective-by-name removal. |  |
| III.14 History & Recovery | Partial | Transaction history (chained, ledgered), rollback, snapshots, journal. Not yet: build/installation/upgrade histories as separate views, package timelines. |  |
| III.15 Security & Integrity | Partial | Audit, manifest, sandbox, limits, trust signals, repository trust, database protection. Signature checks themselves stay pacman's and makepkg's. |  |
| III.16 Package Health | Partial | `doctor` and `report`. Not yet: duplicate package/library detection, mirror health. |  |
| III.17 Analytics | Specified | Build statistics, success rates, downloads, cache usage, resource reports. |  |
| III.18 Unified Interfaces | Partial | CLI with `--json`, `--dry-run`, `--yes`, completion. No TUI at all (browser, inspector, queue and build monitors, log viewer, news reader, history browser, command palette). |  |
| III.19 Configuration | Partial | TOML user config. Not yet: system-wide config, repository and profile config, themes, keybindings, aliases, environment profiles. |  |
| III.20 Performance | Partial | Fast by construction. Not yet: a deliberate index/cache layer, parallel downloads. |  |
| III.21 Extensibility (future) | Specified | Plugin system, public API, remote build workers, distributed compilation, reproducible-build verification, binary cache servers, team build profiles, benchmark database. The plugin system is WASM-sandboxed with enforcement and security checks (see section 9). AI-assisted items in the spec are excluded by the no-AI decision. | `SPEC.md` |
| IV.1 Install Planner | Partial | Packages, sizes, removals, provider choices, risk, plan id, hooks, config files, policy. Not yet: compile time, services to restart before the commit, estimated time. |  |
| IV.2 Explain Decisions | Partial | `explain` for the source/binary decision only. | r3 u3 |
| IV.3 Package Score | Specified | A report card per package: performance, build time, maintenance, security, popularity, source worthiness. |  |
| IV.4 Build Profiles with Inheritance | Specified | Profiles that extend other profiles. |  |
| IV.5 Build Recipes | Specified | `recipe save\|apply`: a concrete, shareable snapshot of compiler, flags, environment, hooks, profile. |  |
| IV.6 Build Diff | Partial | PKGBUILD diff on rebuild exists; `diff-build` (binary vs source runtime comparison) does not. |  |
| IV.7 Build Benchmark | Partial | `benchmark <pkg>` (u9): first run, warm median, size and memory above the launcher floor, stored in `benchmarks.jsonl`. No CPU time, no cold-cache run, not yet read by `recommend`. | `docs/plan/r3/u9-stats-benchmark.md` |
| IV.8 Build Cache Explorer | Partial | Per-package sizes in `cache list`; no browsing or selective deletion by name. |  |
| IV.9 Timeline | Partial | `timeline` (u10): transactions, builds, snapshots and config file actions by UTC day, with `--since` and `--pkg`. No TUI form; `pacman.log` and file-change events belong to package forensics. | `docs/plan/r3/u10-timeline.md` |
| IV.10 Rollback Snapshots | Partial | Package-set snapshots and per-transaction rollback; no Btrfs/ZFS filesystem snapshots. |  |
| IV.11 Interactive Conflict Resolver | Partial | Conflicts and providers listed in plans; an AUR-build dependency conflict is explained with a swap offered. No interactive `[1] Downgrade [2] Rebuild [3] Replace` for the rest. | u24-u26 |
| IV.12 Dependency Graph Explorer | Partial | Text/dot/JSON views; the interactive explorer needs the TUI. |  |
| IV.13 Package Inspector | Partial | The pieces exist separately; not the single aggregated view. |  |
| IV.14 Repository Analytics | Specified | Per-repository totals, pending updates, bandwidth saved, mirror latency. |  |
| IV.15 Build Dashboard | Specified | Live compilation view; needs the TUI. |  |
| IV.16 Health Center | Partial | `doctor` and `report`; not an interactive center. |  |
| IV.17 Smart Updates | Partial | Pins with reason and expiry; no rule-based `smart-update` skip logic. |  |
| IV.18 Build Statistics | Partial | `stats` (u9): builds, failures, time, longest, most built; transactions by kind and the AUR share. Hours and storage saved are not shown: they would be guesses. | `docs/plan/r3/u9-stats-benchmark.md` |
| IV.19 Package Collections | Specified | `collection install\|save\|export`: reusable bundles. |  |
| IV.20 Workspace Mode | Specified | A workspace bundles a profile, repositories, collections and mirrors. |  |
| IV+ Package Advisor | Specified | Warn before packages with unusually large dependency trees or disk impact. |  |
| IV+ Mirror Intelligence | Specified | Mirrors chosen by latency, reliability and current performance. |  |
| IV+ Package Impact Preview | Partial | `impact` shows removal/install/upgrade effects; services, desktop entries and reclaimed space are not yet all included. | u16 |
| IV+ Compile Farm Support | Specified | Distribute source builds to builder machines over SSH. |  |
| IV+ Build Artifact Library | Specified | Reuse compatible local builds by profile, compiler and flags. |  |
| IV+ TUI Command Palette | Specified | `Ctrl+P`-style launcher; needs the TUI. |  |
| IV+ Policy Engine | Specified | Rules such as "always build Rust packages from source" or "binary when on battery". |  |
| V Shell integration (`alias`, `integrate`) | Partial | `completion` exists; `alias` and `integrate` do not. |  |
| V System (`env`, `paths`, `about`, `diagnostics`) | Partial | `--version`, `--help`; `doctor --report` covers the bug-report part. |  |
| V Development (`dev`, `sandbox`, `test`, `trace`, `debug`) | Specified | Internal tooling. |  |
| V Export / Import (`export`, `import`, `bundle`) | Specified | See also the offline bundle idea in section 11. |  |
| V Help (`man`, `docs`, `license`, `credits`, `bug-report`) | Specified | Only clap's `--help` exists. |  |
| V `lock` / `unlock` | Specified | `pin`/`unpin` are built; `lock`/`unlock` are not. |  |
| Integrated Modal Editor (`emrac edit <resource>`) | Specified | A built-in modal editor for PKGBUILDs, `.SRCINFO`, patches, profiles and config, with `--external` for `$EDITOR`. | `SPEC.md` Part I |
| TUI (whole interface) | Specified | Package browser, inspector, dependency explorer, queue and build monitors, log viewer, news reader, history browser, cache manager, rollback interface, themes, keybindings. | `SPEC.md` III.18 |

## 3. Ideas from additions.md (A, B, C)

Numbers are the ones in `additions.md`. Part C items are the ones a package manager is judged on: the worst day.

| Feature | Status | What it is | Where |
|---|---|---|---|
| A1 Typosquat and lookalike warnings | Built | AUR names near-copies of popular packages. | r2 u20 |
| A2 Source-changed-without-bump alert | Built | Sources or checksums change under the same version, or move to another host. | r2 u20 |
| A3 New-maintainer and first-publish warnings | Built | Orphaned/new/few users, changed maintainer, name shadows an official package. | r2 u20 |
| A4 Signing-key pinning | Built | Warn when the PGP keys that sign a package's sources change. | r2 u20 |
| A5 Update risk rating | Built | LOW/MEDIUM/HIGH by what the transaction touches and unread news. | r2 u20 |
| A6 Staged upgrades | Idea | Split a big upgrade into ordered groups so a failure stops early. | `additions.md` |
| A7 Restart and reboot advisor | Built | After a transaction: reboot needed, services to restart. | r2 u20 (`report`) |
| A8 Rollback rehearsal | Idea | `doctor --rollback-test`: prove a restore would work before you need it. |  |
| A9 Stray file scanner | Idea | Files under `/usr` and `/opt` no package owns, with who put them there. |  |
| A10 Config drift report | Partial | `.pacnew` handling and previews exist; a report of how `/etc` differs from package defaults does not. | u27, r3 u5 |
| A11 Orphan lifecycle | Idea | Grace period, recorded reason and restore instead of immediate delete (`orphans` lists today). |  |
| A12 Install reasons ledger | Idea | Record why each package is installed (explicit, dependency of X, project Y). |  |
| A13 Build failure classifier | Partial | Network failures, resource limits and AUR dependency conflicts are recognised; a general pattern classifier is not. | u19, u20, u26 |
| A14 Soname rebuild planner | Built | Which source-built packages a library bump broke; queue the rebuild. | r2 u19 |
| A15 Clean-chroot build option | Idea | Build in a devtools-style clean chroot. |  |
| A16 Machine diff | Idea | Compare two machines' package sets or one against a lockfile. |  |
| A17 Remote apply | Idea | Review a plan locally, apply it on another machine over SSH. |  |
| A18 Container and image export | Idea | A package set as a `pacstrap` list or Containerfile. |  |
| A19 Post-upgrade "what changed" report | Built | New failures, broken AUR packages, new `.pacnew`, reboot and services; per-package dependency/file/service changes are not yet listed. | r2 u20 |
| A20 Prompt and status-bar output | Idea | `emrac status --short` for updates and unread news. |  |
| A21 Notifications | Idea | Desktop notifications or a webhook when a long build or scheduled update ends. |  |
| A22 Foreign-package inventory | Idea | Flatpaks, pip/cargo/npm globals and AppImages beside system packages. |  |
| B1 `emrac bisect` | Idea | Find which update broke something using the transaction log and cached packages. |  |
| B2 `emrac try <pkg>` | Idea | Run a package in the sandbox overlay without installing it. |  |
| B3 Pre-flight checks and kernel safety net | Built | Space, `/boot`, EFI, battery, mirrors, a second kernel and fallback initramfs. | r2 u17 |
| B4 Conflict explainer | Partial | Conflict removals and providers in plans; AUR-build conflicts explained. The shortest chain and ranked ways out are not built. | u24-u26 |
| B5 Driver and firmware doctor | Idea | Detected hardware against installed drivers, DKMS modules and firmware. |  |
| B6 Machine manifest and bootstrap | Idea | One file describing a machine; `bootstrap` rebuilds it. |  |
| B7 Downgrade assistant | Partial | `downgrade` finds versions in the cache and the Archive; working out which dependents must move too is not built. | u6 |
| B8 Link-check for software built outside the manager | Partial | Installed AUR packages broken by a library update are found; unpackaged binaries under `~/.local`, `/opt`, `/usr/local` are not scanned. | r2 u19 |
| B9 Metered and flaky-network mode | Idea | Resumable downloads, mirror fallback, bandwidth cap, deferred large packages. |  |
| C1 Crash-safe transaction journal | Built | Write-ahead journal, stale lock check, recover or record. | r2 u10 |
| C2 Plan integrity | Built | What was previewed is what runs: plan ids, `--expect-plan`, re-plan on drift. | r2 u18 |
| C3 Fail-closed verification chain | Partial | Older-database and older-version refusals exist; keyring-first upgrade and a clock-skew check do not. | r2 u20 |
| C4 Partial-upgrade hard gate | Built | A planner rule, with the exact packages named. | r2 u17 |
| C5 Mirror and repository trust | Partial | Rollback protection and the downgrade refusal exist; comparing database hashes across two mirrors does not. | r2 u20 |
| C6 Protecting pacman's own database | Built | Backup before every transaction, `-Dk` before and after, lock respected. | r2 u20 |
| C7 Rescue binary (`emrac-rescue`) | Idea | A small static binary that reads the journal, restores the database backup and rolls back when the main binary cannot start. |  |
| C8 Privilege separation | Partial | Root-only hidden helpers exist for the ledger and the local file policy (validated arguments, trusted executable); the planner, builds and the rest still run as the user, but the commit goes through `sudo pacman`, not a helper that accepts a saved plan. | r2 u8, r3 u8 |

## 4. Ideas from ideas.md

Grouped as in `ideas.md`. Tags there are older than the code, so the status here follows what is built.

| Feature | Status | What it is | Where |
|---|---|---|---|
| PKGBUILD AST parser | Partial | Static analysis without running bash exists (`audit`); a full shell AST is the stated direction. | u15 |
| Config presets | Idea | Shareable, signed presets extending build profiles and recipes. |  |
| Landlock + OverlayFS together in the staging layer | Idea | Defense in depth inside the sandbox. |  |
| Declarative profile file | Idea | pacdef/decman style: diff a plain-text desired set against the machine. |  |
| Rollback history in the TUI | Idea | CLI history and rollback are built; the TUI view is not. |  |
| Persistent build cache keyed by PKGBUILD hash + flags | Idea | Identical builds are not repeated. |  |
| Optional local repository of verified builds | Idea | Reuse across machines. |  |
| Multi-source backends | Idea | pacman repos, AUR, Flatpak, systemd units behind one interface. |  |
| Split fetch and build phases | Built | Sources fetched and verified first; the build runs with no network. | u17, u19 |
| Reproducible-build check | Idea | Build twice in separate sandboxes and compare hashes. |  |
| Signed, tamper-evident transaction log | Built | SHA-256 chain, root-owned ledger, audit watch (chained and witnessed, not cryptographically signed). | r2 u7-u9 |
| SBOM / provenance output | Idea | What source, flags and toolchain went into each build. |  |
| Post-commit health check with auto-rollback | Partial | The post-upgrade report and the after-failure check exist; automatic rollback on failed checks does not. | r2 u20, r3 u6 |
| Per-package rollback from cached packages | Built | `rollback`, `undo`, `redo`, `downgrade`. | u4-u6 |
| Bootable pre-transaction snapshots | Idea | Register the snapshot as a boot entry. |  |
| Snapshot backends behind one trait | Idea | Btrfs, ZFS, snapper/timeshift; package-set snapshots are built. |  |
| Lockfile (`emrac.lock`) | Idea | Replay a transaction with exact versions and hashes on another machine. |  |
| Headless JSON API | Partial | `--json` on most commands; no long-running API. |  |
| `.pacnew` / config-merge handling | Built | CLI merge and resolve with validation and revert (TUI integration is not). | u27, u30 |
| Plan diff view | Partial | A drifted plan shows what changed; there is no command to diff two saved plans. | r2 u18 |
| Pacman hook compatibility | Built | pacman runs its hooks as always, and plans now preview them. | r3 u4 |
| Offline / air-gapped mode | Partial | `--offline` for lookups and plans from local data; no verified bundle (see section 11). |  |
| Mirror ranking and parallel downloads | Idea | Built into the engine. |  |
| Plugin system (WASM-sandboxed, with security checks) and backend plugin interface | Specified | `emrac plugin list\|search\|install\|remove\|update\|enable\|disable\|verify`. Plugins are compiled WebAssembly modules, never scripts. Enforcement: deny-by-default host calls, capabilities declared in a manifest and granted by the user, memory/time/call limits, every host call logged. Security checks: hash and signature verified before install, requested capabilities shown in the install preview and audited like a package manifest, versions pinned, an update that asks for more capabilities needs a new approval. Also the way new sources (Flatpak and others) are added without touching core transaction logic. | `SPEC.md` III.21, Part V Plugins; `ideas.md`; `plugin-feat.md` |
| Preset sharing | Idea | Signed optimization-flag presets. |  |
| Smoke tests in the sandbox | Idea | Run `check()` and "does the binary start" before commit. |  |
| Binary vs source benchmarking | Idea | Measure whether a source build was worth it (`recommend` predicts, `benchmark` would measure). |  |
| Vulnerability-aware planning | Idea | Arch security advisory data in the plan. |  |
| Safe unattended updates | Idea | Scheduled runs that commit only if checks pass. |  |
| "Why" explanations | Built | `why` for installed packages and `explain` for recommendations. | u12, r3 u3 |
| Arch news gate | Built | See section 1. | r2 u20 |
| DKMS / kernel-module test | Idea | Build modules against the new kernel in the sandbox first. |  |
| Boot test for risky transactions | Idea | Boot the result in a throwaway VM. |  |
| Install-time capability manifest | Built | Everything a package changes beyond files, before `pacman -U`. | u17 |
| Config tracking tied to transactions | Idea | etckeeper-style git history of `/etc` per transaction. |  |
| Time-travel installs | Idea | Resolve a plan "as of a date" from the Arch Linux Archive (the Archive is used today only to fetch files for rollback). | u5 |
| Holds with reasons and expiry | Built | `pin --reason --until`. | u13 |
| Cleanup simulation | Partial | `impact` and `orphans`; no bloat view. | u12, u16 |
| Shared build acceleration (ccache, sccache, distcc) | Idea | Pairs with the persistent build cache. |  |
| `doctor` | Built | See section 1. | u21 |
| Migration mode | Idea | Import a package list from another distribution into a reviewed Arch plan. | Wandrover |
| `maintain check` (pre-submit pipeline) | Idea | Lint, regenerate `.SRCINFO`, clean-sandbox build and `check()`, conflict, install and upgrade tests, AUR guideline checks, then the push. | ideas.md section 4 |
| Upstream watcher | Idea | Detect a new release, bump `pkgver`, test-build, prepare a commit for approval. |  |
| Maintainer dashboard (TUI) | Idea | Out-of-date flags, failing builds, soname rebuilds. |  |
| Capability-based workflow files | Idea | Each pipeline step declares needs (network, paths, secrets); enforced by Landlock or namespaces. |  |
| Dry-run for automation | Idea | Every workflow run shown as a plan first. |  |
| Bring-your-own SSH key for publishing | Idea | Reference the key by path or agent, never embed; only the push step sees it; host key pinning; confirm before push; multiple identities; other secrets by reference. |  |
| Verified provenance indicator, CI template generation, local test repo, orphan adoption | Idea | Maintainer extras. |  |
| `emrac setup` | Idea | First-run wizard that detects what the machine supports and writes the config. |  |
| Config extras (layered config, `migrate`, `preset`, `diff`) | Idea | Beyond show/get/set/unset/edit/validate. |  |
| `emrac self-test` | Idea | Prove each sandbox and snapshot layer works here. |  |
| `emrac sandbox shell` | Idea | Drop into the sandbox of a failed build. |  |
| `plan` / `apply` | Partial | Dry-run, plan ids and `--expect-plan` exist; saving and applying a plan later does not. | r2 u18 |
| Snapshot retention policy | Idea | Keep last N or N days. |  |
| Man pages | Idea | Generated from the command definitions. |  |
| Structured logging | Idea | One log per transaction ID, JSON on request. |  |

## 5. Wandrover: cross-distribution package resolution

Designed in `cross-distro-feat.md`; nothing is built. Deterministic, no AI.

| Feature | Status | What it is | Where |
|---|---|---|---|
| `wandrover search` | Idea | Find, by name or description, a package of another distribution (Debian, Ubuntu, Fedora, openSUSE, ...). | `cross-distro-feat.md` |
| `wandrover resolve` | Idea | The Arch package that provides the same software, with the evidence. |  |
| `wandrover alternatives` | Idea | Compatible replacements and the closest alternatives, ranked. |  |
| `wandrover map` | Idea | Show how a foreign package maps (clear rename, ambiguous, no match). |  |
| `wandrover audit` | Idea | Check the explicit mapping database. |  |
| Cross-distribution package index | Idea | Records with a schema, stored and updated locally. |  |
| Explicit mapping database | Idea | Curated mappings that override the matching rules. |  |
| Capability taxonomy and ranking | Idea | Intent-aware matching and a deterministic ranking model. |  |
| Migration mode | Idea | Import a foreign package list into a reviewed Arch plan. |  |

## 6. Self-management service (Emrac-SMS)

Specified in `self-service-feat.md`; its five phases are built.

| Feature | Status | What it is | Where |
|---|---|---|---|
| `self check` | Built | Phase 1. | r2 u11 |
| `self sync`, `self news` | Built | Phase 2: validated state and metadata. | r2 u12 |
| `self diagnose` | Built | Phase 3: diagnostics (journals and locking arrived with the journal). | r2 u13 |
| `self upgrade` (`up`) | Built | Phase 4: selection, installation-method detection, verification. | r2 u14; r3 u1 |
| `self downgrade` (`dn`) | Built | Phase 5: explicit target, compatibility check. | r2 u15 |

## 7. AUR defense (planned for r4)

`aur-defense-feat.md`: layered malware protection. Decision: plan it in the r4 cycle, not before. Several layers overlap what is already built.

| Feature | Status | What it is | Where |
|---|---|---|---|
| Layer 1 Metadata and trust | Partial | Lookalikes, maintainer signals, shadowing exist; the signed blocklist and upload-burst detection do not. | r2 u20 |
| Layer 2 PKGBUILD, `.install` and source analysis | Partial | Most rules exist (`audit`); patch scanning and a few rules are the spec's additions. | u15, u16, u18 |
| Layer 3 Diff review and soak time | Partial | The diff is shown on every update; soak time, an approval hash ledger and trusted/blocked maintainer lists are not built. |  |
| Layer 4 Isolated build with behavior tracing | Partial | Sandbox, no-network build and limits exist; syscall tracing, canary files, the sinkhole and the ecosystem network tier do not. | u19, u20 |
| Layer 5 Built-package inspection | Built | The manifest. | u17 |
| Layer 6 Install detonation | Idea | Run scriptlets and hooks in a throwaway root and report undeclared changes. |  |
| Layer 7 Observe mode (`try --observe`) | Idea | Run the program in the sandbox with a sinkhole and canaries. |  |
| Layer 8 Post-install audit and incident response | Idea | `audit --persistence`, `incident <id>` against a signed advisory, using the history. |  |
| Signed blocklist and advisory feed | Idea | Open question: who maintains and signs it. |  |
| VM isolation tier | Idea | A microVM for high-risk packages. |  |

## 8. Package forensics (proposed)

`pkgforensics.md`: answers to "what changed, where did it come from, was it tampered with, what is still on the system", from local data, read-only. Commands are proposed under `emrac forensics`; the document's suggested order is `persistence` and `verify`, then `timeline`, then `baseline`.

| Feature | Status | What it is | Where |
|---|---|---|---|
| `forensics timeline --since <date> [--pkg <name>]` | Idea | One ordered timeline merging transactions, `pacman.log`, file change times and enabled units, each event showing its source. | `pkgforensics.md` |
| `forensics provenance <pkg or file>` | Idea | Where it came from: repository or AUR, PKGBUILD hash and git commit, sources and checksums, build environment, signature status, the installing transaction and its review record. |  |
| `forensics verify [--deep]` | Partial | `verify`/`check` compares installed files with pacman's record (hashes, sizes, modes). `--deep` (re-extract the cached package and compare, which catches a tampered metadata record), ownership and extra-file checks are not built. | u9 |
| `forensics persistence` | Idea | Every persistence point (units, timers, cron, udev, `ld.so.preload`, shell startup files, SSH keys, PAM, sudoers, autostart, kernel modules, initramfs hooks) and which package owns each; unowned ones flagged. Same ground as the AUR defense `audit --persistence` and the stray file scanner (A9). |  |
| `forensics delta <pkg> <v1> <v2>` | Idea | Files added or removed, hash and permission changes, scriptlet and hook changes, dependency changes, ELF imports and embedded URLs. |  |
| `forensics baseline create \| diff` | Idea | Save a hash baseline of package files and the persistence set at a known-good time and show what differs now. |  |
| `forensics capture` | Idea | An evidence bundle (package list, transaction log, hashes, persistence inventory, timeline) with a hash manifest, an optional signature and a redaction option. |  |
| Offline analysis (`--root /mnt`) | Idea | Analyze a mounted disk from a live USB or the rescue binary (C7) without trusting the running kernel. |  |
| Tamper-evident history with an off-machine anchor | Partial | The SHA-256 chain, the root-owned ledger and the audit watch are built; storing the latest hash somewhere else (a git remote or a printed code) is not. | r2 u7-u9 |

## 9. Plugin system (proposed)

`plugin-feat.md`: WebAssembly modules, never scripts, with the enforcement and security checks built in. Nothing is built. The design is a proposal; the open choices (runtime, signing, whether there is a registry at all) are listed on the page.

| Feature | Status | What it is | Where |
|---|---|---|---|
| `emrac plugin list \| search \| install \| remove \| enable \| disable \| update` | Specified | The command surface in `SPEC.md` Part V. The design adds `info`, `verify` and `test`. | `SPEC.md` Part V; `plugin-feat.md` section 8 |
| Plugin runtime and enforcement | Idea | The WASM runtime runs in a child process inside the same bubblewrap sandbox as builds (no network, empty home); host calls are deny-by-default and logged; plugins run unprivileged and are never loaded by a root helper or a pacman hook. | `plugin-feat.md` section 2 |
| Capability model | Idea | Capabilities declared in the manifest and granted by you: `fs.read:<path>`, `net.get:<host>`, `net.post:<host>`, `events:<kinds>`, `time`, and output kinds. There is no host call to write, read the environment or reach other hosts; running an external tool is a separate, later capability (section 15). | `plugin-feat.md` sections 3 and 15 |
| Resource limits | Idea | Per call: memory, instruction budget (fuel), wall time (the host process is killed), output size and number of findings, host-call counts. A limit hit switches the plugin off for that run and never blocks or passes a transaction. | `plugin-feat.md` section 4 |
| Add-only plugin findings | Idea | Plugin output is merged and labelled with the plugin; it cannot suppress or edit a built-in finding, and its severity is capped below `critical` by default. | `plugin-feat.md` section 2 |
| Plugin manifest and module inspection | Idea | `plugin.toml` (id, version, ABI, extension points, capabilities, limits, hash) validated strictly; before install the module must validate as WebAssembly and its imports must be a subset of the declared capabilities (no WASI, no threads). | `plugin-feat.md` sections 5-7 |
| Plugin install security checks | Idea | Hash and signature verified; key trust explicit on first use; an install preview in words with a default-no question `--yes` does not answer; pinning in `plugins.lock`; an update that asks for more capabilities needs a new approval; downgrades refused; history records; `plugin verify` and a `doctor` row. | `plugin-feat.md` section 7 |
| E1 Audit-rule plugins | Idea | Extra checks over a PKGBUILD, `.install` and patches, returning findings in the shape `audit` already has. The first extension point to build. | `plugin-feat.md` section 1 |
| E2-E4 Health-check, plan-note and formatter plugins | Idea | Extra `doctor` rows, extra notes under a plan, and new output formats for existing JSON reports. | `plugin-feat.md` section 1 |
| E5 Package-source plugins (Flatpak and others) | Idea | New sources without touching core transaction logic; needs its own design because it involves external tools and the transaction machinery. | `plugin-feat.md` section 1; `ideas.md` |
| E6-E7 Event-observer and policy-gate plugins | Idea | Observers receive structured events and report them (notification, webhook, log). Gates can veto a transaction under a policy you configured; they only ever make it stricter, never approve what a built-in check stopped, and never apply to recovery. | `plugin-feat.md` sections 1-3 |
| E8-E9 Read-only command and plan-proposal plugins | Idea | A namespaced read-only command from the data a plugin was granted, and plugins that propose operations that emrac turns into a normal previewed plan. | `plugin-feat.md` section 1 |
| Execution by proxy (`exec:<tool>`) | Idea | emrac runs a named, hash-pinned tool in the build sandbox on a plugin's behalf, from declared argument templates only; read-only by default, writes need a separate capability that `--yes` does not answer; never privileged, output treated as untrusted, every run logged. Phase 4 or later. | `plugin-feat.md` section 15 |
| Industry-tool plugin families | Idea | Adapters for Git and forges, containers, infrastructure as code, Kubernetes, CI/CD, observability, supply chain and security, build systems, artifact registries, notifications, editors and fleet tools. The tool stays a separately installed program; installs, removals and commits stay with emrac. | `plugin-feat.md` sections 13-15 |
| Core-versus-plugin boundary | Idea | Protections for the transaction and for recovery stay built in so removing a plugin never removes one; a suggested plugin list was reviewed and roughly a third to a half duplicates the core. | `plugin-feat.md` section 14 |
| Signed plugin index, `search`, revocation | Idea | A signed index with rollback protection and a revocation list, in phase 3 only. Shares the governance question of the AUR defense feed. | `plugin-feat.md` section 10 |
| Plugin SDK and template | Idea | A crate and template so writing a rule is not an exercise in the raw ABI. | `plugin-feat.md` section 12 |

## 10. Research programme (REA, containers, other package managers)

`rea-research-plan.md` and `rea-portage.md`. Research informs design; nothing here runs inside emrac. Findings so far are in `research/` and drove u4 to u8.

| Feature | Status | What it is | Where |
|---|---|---|---|
| R1 Version comparison (pacman, dpkg, rpm) | Research | Done: `research/vercmp.md`, `dpkg-versions.md`, `rpm-versions.md`, a truth table, and the pacman rows pinned as a test. | `research/` |
| R2 Transaction commit order and locking | Research | Done (by `strace` in a container): `research/pacman-commit-order.md`, with upgrade, backup files, scriptlets, hooks and failure injection. | `research/` |
| R3 Hooks and scriptlets | Research | Done: `research/alpm-hooks.md`. | `research/` |
| R4 Solver explanations and conflict resolution (aptitude, libsolv) | Research | Planned. | `rea-research-plan.md` |
| R5 Selective undo and generations (dnf history) | Research | Planned. |  |
| R6 Temporary build dependencies (apk `--virtual`) | Research | Idea taken; built as u7 without a research pass. |  |
| R7 Closure size and generation storage (nix) | Research | Planned. |  |
| R8 Permission display model (flatpak) | Research | Planned. |  |
| Bin-package audit (network endpoints, self-updaters, bundled libraries) | Research | Needs a real target package. |  |
| P1 Subslot-style rebuilds | Idea | Record the library ABI each source build linked against; rebuild plans on change (Portage `:=`). | `rea-portage.md` |
| P2 Preserved libraries | Idea | Keep an old `.so` that programs still need until dependents are rebuilt. |  |
| P3 Rebuild on changed inputs | Idea | Rebuild when flags or dependencies changed since the build. |  |
| P4 Saved build environment | Idea | Store each package's exact build inputs. |  |
| P5 `--keep-going` | Idea | Continue a build queue past a failure and report what was skipped. |  |
| P6 Resume an interrupted run | Idea | A saved list lets a run continue. |  |
| P7 Separate phases | Idea | Fetch, build and install each run alone. |  |
| P8 Jobs and load-average limits | Idea | Limit parallel builds by load. |  |
| P9 `--oneshot` | Idea | Install without marking it wanted. |  |
| P10 `--depclean` | Idea | Remove unneeded packages after checking reverse dependencies. |  |
| P11 Per-package build environment overrides | Idea | Different flags or toolchain for one package. |  |
| P12 User patches applied on every build | Idea | A patch directory per package. |  |
| P13 License acceptance groups | Idea | Filter by license class. |  |
| P14 Autounmask-style suggestions | Idea | When resolution fails, propose the exact change and write it after confirmation. |  |
| P15 `REQUIRED_USE`-style option rules | Idea | Valid option combinations. |  |
| P16 Install message log (`elog`) | Idea | Keep the messages a package prints and show them afterwards. |  |
| P17 `emerge --info`-style environment dump | Idea | One command for bug reports (`doctor --report` covers part). |  |
| P18 Package integrity check | Partial | Built as `verify` (hashes, modes, sizes from the mtree); modification times are not compared. | u9 |
| P19 Build sandbox and network sandbox | Built | The bubblewrap sandbox with the network off. | u19 |
| P20 Signed sync and signed binary packages | Idea | Verify signed commits and package signatures. |  |
| P21 Binary packages with matching rules | Idea | Reuse a prebuilt package only if its inputs match. |  |
| P22 USE-flag-like per-package toggles | Idea | Per-package feature toggles that change build options and dependencies. |  |

## 11. Newer ideas (not yet in the other documents)

Collected after the documents above. Everything here is deterministic and local; no AI, no scripting engine.

| Feature | Status | What it is | Where |
|---|---|---|---|
| Pacman hook editor: `emrac pachooks list \| show \| new --template \| edit \| test \| enable \| disable` | Idea | A form-assisted, validating editor for pacman's `*.hook` files (declarative files, not scripts): live validation (missing sections, unknown keys, an `Exec` that does not exist, `AbortOnFail` where it is not allowed), a "when would this fire?" preview against a plan (the matching engine of the hook preview already exists), a sandboxed test run of `Exec` with a fake target list, templates (snapshot before upgrade, notify after, clear a cache, rebuild after a kernel update), safe overrides (copy a package-owned hook to `/etc/pacman.d/hooks/` under the same name or mask it with a `/dev/null` symlink, never edit in place), owner display (package or user), history with diffs and one-command enable/disable/undo, saving through the privileged helper with a diff and a confirmation, lint for risky patterns, and package-added hooks shown in the install preview. The group is named `pachooks` because `SPEC.md` already uses `emrac hooks` for emrac's own event hooks (the schedule/cron/hooks/events group). `edit` uses the same editor as `emrac edit`. CLI in the style of `visudo` (validates on save, refuses to install an invalid file) and TUI (hook list, form, preview). | `hooks.rs` (u4), `policy.rs` (u8) |
| Update cooldown (soak time) | Idea | Do not install a new AUR version until it is N hours old unless it is flagged as a security fix; also in the AUR defense document. | section 7 |
| PKGBUILD review ledger | Idea | Record the hash you last reviewed; show only the diff since and ask to re-approve; later import others' signed review records. | section 7 |
| Lockstep groups | Idea | User-defined sets (GPU driver, utilities, kernel module) that upgrade together or not at all, with the reason; extends the partial-upgrade gate. |  |
| Provider preferences | Idea | Remember your answer to "there are N providers available" as a named preference, show it in every plan, change it with one command. |  |
| File history: `emrac blame <path>` | Idea | Which package installed a file, in which transaction, and what changed it since; with `emrac diff <date\|tx> <date\|tx>` for packages, files and services between two points. |  |
| Transaction notes and known-good markers | Idea | `-m "before the demo"` on a transaction; mark a state known good; bisect, rollback and diff can target it. |  |
| Build failure bundle | Idea | A sanitized bundle (PKGBUILD, toolchain versions, environment summary, relevant log lines) to attach to an AUR comment or bug; pairs with the failure classifier (A13). |  |
| Offline bundles: `bundle create \| apply` | Idea | Export verified packages, databases and a reviewed plan to a drive; import and verify them on an air-gapped machine through the normal preview. |  |
| Git-style `log` / `show` | Idea | Every transaction gets a short ID, a parent and a message; `log --oneline --graph`, filters by package or date; `show <tx>` lists what changed. |  |
| `diff <tx> <tx>` and `revert <tx>` | Idea | Changes between two points, including tags; `revert` undoes one past transaction by creating a new one, after checking later transactions for conflicts (unlike `rollback`, which resets). |  |
| Tags | Idea | `emrac tag known-good`; bisect, diff and restore can target it. |  |
| `emrac status` (a git status for the system) | Idea | Packages behind, held packages, orphans, stray files, unmerged `.pacnew`, drift from a manifest or lockfile. |  |
| Ignore file | Idea | A `.gitignore`-style list of paths the stray-file scanner and drift reports skip. |  |
| Experiment branches | Idea | `branch new` snapshots and tracks changes; `discard` returns to the branch point, `keep` accepts them. |  |
| Content-addressed plans and manifests | Partial | Plan ids are already SHA-256 digests of what a plan does; manifest ids are not. | r2 u18 |
| Config repo in git | Idea | Keep config, manifest, lockfile, profiles, recipes, patches and hooks in a git repo; commit each change with the transaction ID; `bootstrap <git-url>` rebuilds a machine. |  |
| `/etc` history (etckeeper-style) | Idea | Commit `/etc` after every transaction to see which package changed which config and revert one file. |  |
| AUR history as evidence | Idea | Commit log, author and email changes and PKGBUILD diffs since your last review; a force-pushed history is a trust warning. |  |
| VCS package pinning | Idea | For `-git` packages record the built commit, show "3 commits behind" and the commits between your build and the update in the preview; build at a chosen commit; verify signed tags. |  |
| Signed git data feeds | Idea | The Wandrover mapping repository, the blocklist and advisories as git repositories fetched with signature verification and fast-forward only. |  |
| Patch workflow: `emrac patch new <pkg>` | Idea | Open the unpacked source as a git worktree, generate the patch into your patch directory, rebase it when upstream changes and report conflicts. |  |
| Maintainer pre-push checks | Idea | Clean worktree, regenerated and committed `.SRCINFO`, and the `master` branch the AUR requires. |  |
| `emrac vercmp` | Idea | Compare two versions with pacman's rules (the semantics are documented in `research/vercmp.md`). | from dpkg `--compare-versions` |
| `emrac query --format` | Idea | Filter and format installed packages by size, install date, reason, repository (rpm `-q --qf`, `dpkg-query -W -f`). |  |
| Optional-dependency suggestions | Idea | After a transaction, which optional dependencies are unmet and what each adds (rpm weak dependencies). |  |
| `.pacnew` resolution offered right after an upgrade | Idea | Offer `emrac pacnew resolve` for the files the upgrade just created, using the u5 prediction. |  |

## 12. Decided against, or far future

Recorded so nobody re-proposes them by accident.

| Feature | Status | What it is | Where |
|---|---|---|---|
| AI or LLM features, telemetry, cloud lookups | Not planned | Decided against: emrac is deterministic and local. (`SPEC.md` III.21 lists AI-assisted items; they are excluded.) | `additions.md`, `rea-research-plan.md` |
| Scripting of any kind | Not planned | Decided: no scripting engine, no embedded interpreter, no user-written scripts run by emrac, and no scripting layer for hooks or workflows. Everything is declarative files and fixed rules. Plugins are compiled WASM modules under enforcement, not scripts. (The restricted scripting layer idea was removed from `ideas.md`.) | `additions.md`, `ideas.md` |
| Committing transactions through libalpm instead of the `pacman` binary | Not planned | A far-future idea of yours, deliberately not planned or scheduled. The research documents hold what dpkg and rpm do for atomic file replacement and journaling, which is where it would start. | far future |

## Where each thing is documented

| Document | What it holds |
|---|---|
| [`FEATURE_STATUS.rst`](../../FEATURE_STATUS.rst) | The spec's categories against the code, with the commands available today. |
| [`CHANGELOG.md`](../../CHANGELOG.md) | What each slice changed and why, newest first. |
| [`SPEC.md`](./SPEC.md) | The product specification: vision, the feature catalogue, signature features, the command catalogue. |
| [`additions.md`](./additions.md) | Ideas A1-A22, B1-B9 and the critical features C1-C8. |
| [`ideas.md`](./ideas.md) | Ideas from other projects, safety, workflow, the maintainer workflow, emrac's own tooling, alignment with the spec. |
| [`cross-distro-feat.md`](./cross-distro-feat.md) | Wandrover, cross-distribution package resolution. |
| [`self-service-feat.md`](./self-service-feat.md) | The self-management service, `emrac self`. |
| [`aur-defense-feat.md`](./aur-defense-feat.md) | Layered AUR malware defense, planned for r4. |
| [`plugin-feat.md`](./plugin-feat.md) | The plugin system: WebAssembly modules with enforcement and security checks (proposed). |
| [`pkgforensics.md`](./pkgforensics.md) | Package forensics: timeline, provenance, persistence, baseline, evidence capture (proposed). |
| [`rea-research-plan.md`](./rea-research-plan.md), [`rea-portage.md`](./rea-portage.md) | The research programme and what to learn from Portage. |
| [`../plan/`](../plan) | One plan per slice, written before the code (`r1/`, `r2/`, `r3/`). |
| [`../design/`](../design) | Design pages for users and maintainers of built features. |
| [`../../research/`](../../research) | Findings from reading and running other package managers (versions, hooks, commit order, dpkg and rpm databases). |
| [`../tests/emrac-tests.rst`](../tests/emrac-tests.rst) | Every automated test and container check, generated. |

## Keeping this file current

When a slice ships, change its rows here from Idea or Specified to Built or Partial and name the slice; when a new idea is proposed, add a row to the section it belongs to (or section 11) with a one-line description and where it is written up. `FEATURE_STATUS.rst` stays the per-category status of the spec; this file is the union of everything.

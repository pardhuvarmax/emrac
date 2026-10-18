# REA x Portage: Features to Learn From

Research doc for Emrac. Portage is Gentoo's source-based package manager, which makes it the closest existing relative of Emrac's source-aware design. This document lists the Portage features worth learning from, the Emrac version of each, and how to research each one, including where REA (Reverse Engineer Anything) helps.

Emrac stays deterministic with no AI in its runtime. REA is a research tool only. See `rea-research-plan.md` for the general REA setup, safety rules, workflow, and research log template. Status tags: `[idea]` not started, `[planned]` decided, `[confirmed]` behavior verified by a test.

## 1. How to research Portage

**Read first.** Portage's core (`emerge`, the `portage` Python modules, the `ebuild` command) is open-source Python. Its behavior is documented in `man emerge`, `man portage`, `man make.conf`, `man ebuild`, the Gentoo Handbook and wiki, and the ebuild specifications (PMS). For these, reading the docs and source is faster and more accurate than reverse engineering. REA adds little there.

**Where REA does help.** Portage's ecosystem includes compiled components. These are the REA targets (see section 4):

- `sandbox` and `libsandbox` (build sandboxing)
- `portage-utils` (`q` and the `qcheck` applet)
- `pax-utils` (`scanelf`), an ELF scanner (verify which tool Portage actually uses for linkage data)

**Environment.** Use a Gentoo stage3 container or VM with no personal keys or credentials. Install REA only from its official sources. Use `emerge --info` output as your record of the exact environment.

**Rules.** Portage is GPL-2. Study behavior and write the Emrac version in your own words. Never copy or port reconstructed code. Record where each idea came from.

**Confirming tests.** For every feature, run the scenario in the container, capture the output, and write a test that compares Portage's behavior with Emrac's. Keep the test.

## 2. Feature catalog

Details of each Portage feature should be checked against its manual before relying on it. Cross-references (A, B, C numbers) point to `additions.md`.

### A. Rebuild correctness (the biggest wins)

| # | Portage feature | Emrac version | Research method |
|---|---|---|---|
| P1 | **Subslot dependencies (`:=`).** A package depending on a library with `:=` is rebuilt automatically when the library's ABI (subslot) changes. | Record the library ABI each source build linked against, and generate a rebuild plan automatically when a soname changes. Builds on A14 (soname rebuild planner) and B8 (link-check). | Docs: PMS slot operators, `man ebuild`. Test: bump a library subslot in a container and observe which packages are scheduled. |
| P2 | **Preserved libraries (`preserve-libs`, `@preserved-rebuild`).** Keeps the old `.so` that programs still need after a library upgrade, then removes it after the dependents are rebuilt. | A managed preserved-libs directory: keep old library files that installed binaries still need, rebuild the consumers, then delete the leftovers. Closes the Arch gap where a soname bump breaks binaries. | Docs and Python source (`portage/util/_dyn_libs`). Test: upgrade a library with a dependent binary, check what is kept and when it is removed. |
| P3 | **Rebuild on changed inputs (`--newuse`, `--changed-use`, `--changed-deps`).** Rebuilds packages whose build options or dependencies changed since they were built. | Record each source build's inputs (profile, flags, toolchain, dependency versions) and show a "stale builds" report with a rebuild plan. | Docs. Test: change a setting and compare Portage's rebuild list with Emrac's. |
| P4 | **Saved build environment.** Each installed package keeps its USE flags, CFLAGS, and a saved build environment in `/var/db/pkg`. | Store the exact build inputs per package, so "how was this built?" always has an answer and stale builds can be detected. | Inspect `/var/db/pkg/<category>/<package>/` in the container and list every file and field. |

### B. Build queue and recovery

| # | Portage feature | Emrac version | Research method |
|---|---|---|---|
| P5 | **`--keep-going`.** If one package fails to build, continue with the rest and skip only what depended on it. | Build queue behavior: finish everything that can finish, and report exactly what was skipped and why. | Docs. Test: force a failure in the middle of a merge list and record the resulting order and report. |
| P6 | **`emerge --resume`.** The saved merge list lets an interrupted run continue where it stopped. | Resume from the transaction journal (C1) with a verified plan (C2). | Docs and source (resume list storage). Test: kill a running merge and resume it. |
| P7 | **Separate phases (`--fetchonly`, `--buildpkgonly`, `--onlydeps`, `ebuild ... unpack compile install`).** Fetch, build, and install can each be run alone. | `emrac build --stop-after <phase>` for debugging, and a download-only mode that matches the fetch and build split. | Docs, `man ebuild`. |
| P8 | **`--jobs` and `--load-average`.** Limits parallel builds by system load. | Queue scheduling by CPU load, and by memory using each package's recorded peak memory to avoid out-of-memory failures. Memory-awareness is an Emrac improvement over Portage. | Docs. Test: run parallel builds under load and observe scheduling. |
| P9 | **`--oneshot`.** Installs a package without marking it as wanted. | Temporary installs that are cleaned up later. Pairs with `emrac try` (B2). | Docs. |
| P10 | **`--depclean`.** Removes unneeded packages after checking reverse dependencies. | Orphan removal that verifies reverse dependencies and records the reason (A11). | Docs. Test: depclean a set with shared dependencies. |

### C. Configuration and policy

| # | Portage feature | Emrac version | Research method |
|---|---|---|---|
| P11 | **`package.env`.** Per-package environment overrides: different compiler flags, `MAKEOPTS`, even another toolchain for one package. | Per-package build profile overrides ("build ffmpeg with LTO, install Firefox as the binary"). | Docs, `man portage`. |
| P12 | **User patches (`/etc/portage/patches`).** A patch placed in a package's directory is applied on every future build. | A per-package patch directory that survives upgrades. This is the clean foundation for the variants design (section 3). | Docs: the user patches section of PMS and `man portage`. Test: add a patch, rebuild, then upgrade. |
| P13 | **`ACCEPT_LICENSE` and license groups.** Filter by license class and block everything else. | A license policy with groups. Blocks or warns in the plan before anything installs. | Docs. Inspect the license group definitions that ship with the tree. |
| P14 | **Autounmask.** When resolution fails, proposes the exact configuration change needed and can write it after confirmation. | The conflict explainer (B4) offers ready-to-apply pin, hold, or profile changes, written only on confirmation. | Docs (`--autounmask`, `--autounmask-write`). Test: request a masked package and record the proposals. |
| P15 | **`REQUIRED_USE`.** Declares valid option combinations ("exactly one of", "at most one of"). | If variants are ever added, validate combinations in the plan and reject invalid ones with a clear reason. | PMS, REQUIRED_USE syntax. |

### D. Visibility and integrity

| # | Portage feature | Emrac version | Research method |
|---|---|---|---|
| P16 | **`elog`.** Saves the messages a package prints during installation and shows them afterwards. | Capture scriptlet output and package notes into the transaction report and history. | Docs (`PORTAGE_ELOG_*` in `man make.conf`). |
| P17 | **`emerge --info`.** One command dumps the build environment for bug reports. | `emrac info`: toolchain, flags, CPU features, repositories, active profile, in a form suitable for bug reports. | Run it in the container and list every field it prints. |
| P18 | **Package integrity check (`qcheck`, `equery check`).** Verifies installed files against recorded hashes and modification times. | `emrac verify`: detect modified or missing package files, extending `pacman -Qkk`. | Docs. REA candidate: `qcheck` (section 4). |
| P19 | **Sandbox and network sandbox.** Builds are blocked from writing outside their directories and from using the network. | Matches the Layer 2 design. Worth copying: log denied accesses and show them in the failure report. | Docs. REA candidate: `sandbox` and `libsandbox` (section 4). |
| P20 | **Signed sync and signed binary packages.** Repository sync can verify signed commits, and the binary package format can carry signatures. | Verification of repository state and cached build artifacts (C3 and C5). | Docs (sync verification settings in `man portage`; the binary package format specification). |
| P21 | **Binary packages with matching rules.** A prebuilt package is reused only if its build inputs match. | Cache key from PKGBUILD hash, flags, and toolchain, shareable over a LAN. | Docs. Test: build with different settings and see when Portage reuses or rejects a binary package. |
| P22 | **USE flags.** Per-package feature toggles that change both build options and dependencies. | See section 3. Hardest to port. | Docs. Lowest priority. |

## 3. Why USE flags are hard on Arch, and the staged alternative

**Why it is hard**

1. PKGBUILDs have no flag mechanism. An ebuild's conditionals change both configure options and dependencies. A PKGBUILD is a plain script with fixed `depends`, `makedepends`, and configure arguments.
2. Flags change the dependency graph, and Arch repositories and `.SRCINFO` carry no flag-conditional dependency metadata.
3. pacman does not know about flags. A custom build of `foo` would be replaced by the stock binary on the next `pacman -Syu` unless Emrac tracks it.
4. Official binary packages are built once with one feature set, so any non-default option forces a source build.
5. Rebuilding a library with different features can break dependents, so rebuild tracking is required.
6. Flag combinations multiply, and Arch maintainers do not support custom builds.

**Staged alternative**

1. **Build profiles** (already in `SPEC.md`): compiler flags, LTO, PGO, CPU tuning. The safe 80%.
2. **Per-package overrides (P11) and user patches (P12).**
3. **Curated variants:** a small reviewed set of overlay recipes for popular packages, applied as patches to the PKGBUILD. Emrac builds the result under a distinct name (for example `foo-emrac`) with `provides=(foo)` and `conflicts=(foo)`, so pacman treats it as separate and will not overwrite it.
4. **Variant tracking** in the install reasons ledger, with a rebuild report when upstream changes (P3).
5. **Full USE-style flags:** only later, and only if variants prove popular and the metadata can be maintained.

## 4. REA investigations

Command format (confirm each argument's meaning by running the prompt with no arguments):

```
/mcp__rea__investigate_feature [feature] [target_path] [document] [procedure] [provider_id]
```

Use `command -v` and `readlink -f` in the Gentoo container to get real paths. The paths below are examples.

```
/mcp__rea__investigate_feature "Which operations the build sandbox intercepts, how it decides to deny them, and what it logs on a violation" /usr/lib64/libsandbox.so research/portage-sandbox.md "Locate the interception layer, list covered calls, record the deny decision and log format, do not reproduce code" ghidra

/mcp__rea__investigate_feature "How qcheck verifies installed files against recorded hashes and modification times, and what it reports" /usr/bin/q research/portage-qcheck.md "Document the checked fields, the comparison logic, and the report format as behavior" ghidra

/mcp__rea__investigate_feature "Which ELF fields scanelf extracts (needed libraries, soname, rpath) and how output is formatted" /usr/bin/scanelf research/portage-scanelf.md "Document extracted fields and format, record edge cases for stripped and static binaries" ghidra
```

Use these results to inform:

- the binary inspector idea (ELF linkage extraction, for P1, P2, and B8)
- `emrac verify` (P18)
- sandbox denial reporting (P19)

Everything else in the catalog is Python or documented behavior and should be researched by reading and testing, not with REA.

## 5. Suggested order

1. **P1, P2, P4 together** (ABI tracking, preserved libraries, saved build inputs). The highest-value group: it addresses breakage after soname bumps, a common Arch problem. Confirm with a container scenario that upgrades a library under a dependent binary.
2. **P12 (patches) and P11 (per-package overrides).** Small features that make Emrac a real source-based tool for people who customize packages, and the base for variants.
3. **P5 and P6 (keep-going and resume).** Keeps a long upgrade from being lost to one failure. Needs the journal and plan integrity features from `additions.md` Part C.
4. **P14 (autounmask-style fixes) with the conflict explainer (B4).**
5. The rest as time allows. Leave P22 (USE flags) for last.

## 6. Research log

Use the template in `rea-research-plan.md` (section 5). Keep one file per feature in `research/`, and record for each: Portage version, the docs read, container scenario, REA command if used, confirming test, the behavior in your own words, the Emrac design, and the status (`[idea]`, `[planned]`, or `[confirmed]`).

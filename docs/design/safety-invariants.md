# Safety invariants

Safety rules are system invariants, not warnings: each one is a property the code keeps and a test that shows it under failure. This page is the register. It states what is **kept today**, what is **a known gap**, and where the gap is scheduled. It promises only what the implementation enforces; for the rest it detects and gives a recovery path. No package manager can promise an untouched system through power loss, filesystem errors or an irreversible hook.

Status words: **kept** (code + a test), **partly** (kept in the main path, a gap named), **gap** (not yet), **n/a yet** (the feature does not exist).

## The ten invariants

| # | Invariant | Status | Where / test |
|---|-----------|--------|--------------|
| 1 | A failed preflight does not modify installed package state | kept | `commands/preflight.rs` only reads; pre-flight e2e (`preflight.sh`) |
| 2 | A failed source rebuild after a successful upgrade leaves the repository binary installed and the preference pending | kept | `upgrade.rs::rebuild_policy`; `sourceinstall.sh` ("a rebuild that fails leaves the binary, keeps the policy") |
| 3 | A source preference never silently becomes a pin or a repository policy | kept | policy is its own file, `drop` and `install --binary` are the only ways out and are explicit; `sourceinstall.sh` |
| 4 | A system upgrade is resolved and committed as one pacman transaction (`-Syu`) | kept | `upgrade_official`; the source rebuild is a second, separate step by design |
| 5 | No signature, integrity, dependency or conflict check is silently bypassed (`--yes` included) | partly | pacman verifies signatures; `--yes` answers pacman's questions with their defaults and never skips emrac's gates (news, audit blocks, preflight). Gap: `--yes` with a HIGH audit still needs `--accept-risks`, which is tested only for AUR |
| 6 | Intended policy and actual installed origin are different facts | partly | `source_built::state_of` separates intent from the installed version; origin is inferred from the recorded version only. Gap: the installed package's `Packager`/build date is not compared |
| 7 | After an interrupted operation emrac inspects the real state before retrying or claiming success | kept for transactions | journal (`docs/design/journal.md`), `recover_found`. Gap: a rebuild interrupted mid-build leaves no `building` record (see S-2) |
| 8 | A failed transaction is never said to have left the system untouched unless verified | kept | the recovery report reads the database and ledger |
| 9 | A pending rebuild never blocks unrelated upgrades or causes a downgrade | kept | pending is a flag in `source-built.json`, not a pin and not an ignore |
| 10 | Every guarantee has a failure-condition test | partly | this register's last column; gaps below |

## Source-build lifecycle (rules 16-34)

Kept: intent persisted across upgrades and emrac updates; failure keeps binary, intent and pending; `drop` is explicit; a package whose installed version differs from the one built is reported **unknown build intent**; the policy file is written atomically (temp file, fsync, rename); a rebuild is idempotent (a clean checkout every time; `record_to` replaces the entry).

Gaps:
- **S-1 States.** Only `built here`, `pending` and `unknown` exist. Missing: `building`, `failed` (with the reason and the log path), `blocked`. A failure is a pending flag plus a message in the terminal; the reason is not recorded (rule 128).
- **S-2 Operation record.** A rebuild is a history entry, but a crash mid-build leaves the policy unchanged without saying a build was running (rules 79, 81).
- **S-3 Recipe change.** The packaging revision is recorded but not compared: a changed recipe at the same version does not mark the build stale (rules 27, 28, 65).
- **S-4 Dependency order.** Several policy packages are rebuilt in plan order, not in dependency order; a policy package depending on another is not detected (rules 38-40).
- **S-5 Verified install.** "Built here" is recorded after `pacman -U` returns success; it is not re-read from the database (rule 20).
- **S-6 Schema.** `source-built.json` has no version field; the new fields are optional, so old files still read, but there is no migration path (rule 121).
- **S-7 Locking.** Read-modify-write of the policy has no lock; two emrac processes can lose an update. Transactions are already serialised by the journal state (rules 123, 124).
- **S-8 Locally modified packages.** Nothing marks a package built here as not the repository's file in `pacman -Qi` (it shows `Unknown Packager`); `from-source list` should show origin from the database (rule 33).

## Build isolation (rules 46-58)

Kept: builds run unprivileged in bubblewrap with the network off unless allowed, limits from `[build]`, the audit of the recipe before the build and of the built package before the install, downloads checked against the recipe's checksums by `makepkg`.

Gaps: the sandbox is bubblewrap over the host's installed packages, not a clean chroot from `devtools` (rules 47, 55: the build sees the host's library versions, which for a rebuild after `-Syu` is the intended target, but is not a clean-room); credentials/agents are not stripped beyond the sandbox's own mounts (rule 49: to be audited); the artifact's hash and the source checksums are not stored (rule 63). Scheduled: Build Profiles (u28) and the hardening slice (u29).

## Transactions and consistency (rules 1-15, 91-100)

Kept: one pacman transaction; pacman's own locking; no concurrent emrac transaction (journal `Running` refuses); the plan id is verified again immediately before the commit (`verify_before_commit`); databases are synced fresh for the plan (u22); `--expect-plan`; removals and replacements are simulated by libalpm and shown; commit failure and hook failure are reported by pacman's own words, and the recovery report says what landed.

Gaps: a package pacman fails to download aborts the transaction (kept) but the message is not classified as network/mirror/signature (rule 98); an offline plan from an old list is labelled stale, not refused for commit (rule 14: commit through `upgrade` needs the network anyway).

## Failure, recovery, state (rules 78-90, 121-130)

Kept: the journal, the ledger, `history reconcile`, `rollback`, atomic writes of emrac's JSON files, optional features (scores, stats) failing without stopping the package manager.

Gaps: the schema version of the JSON state files (S-6); a lock for policy files (S-7); "installed state and metadata disagree" is surfaced for the source policy (`unknown build intent`) but is not a general reconcile for all metadata (rule 90).

## Consent and CLI (rules 111-120)

Kept: the plan before any change; planned and done are different words; policy changes only by explicit commands; `--yes` does not pass the news gate, the audit's Block, or preflight refusals; noninteractive mode fails where a decision has no safe default.

Gaps: a pending rebuild is shown by `upgrade` when it happens and by `from-source list`, but not by `emrac doctor` or at the next `upgrade` plan (rule 119): to add.

## Tests required (rules 131-144)

Container checks exist for: failed rebuild keeping the binary (`sourceinstall.sh`), the policy surviving an upgrade, recovery after an injected pacman failure (`recovery.sh`), preflight, concurrent journal state. To add: crash during a rebuild (S-2), a changed recipe (S-3), two emrac processes on the policy (S-7), a corrupted/truncated policy file, an unreadable `source-built.json` (reads as empty today: should be reported as unknown, not as "no policy"), non-TTY runs of the rebuild step.

## Priority

- **P0** (before trusting it with real upgrades): S-1 `failed` with a reason, the unreadable-policy-file case (treated as unknown intent, never as "none"), S-5, the doctor/plan line for pending rebuilds, S-7.
- **P1**: S-2, S-3, S-4, S-6, S-8, artifact hashes and provenance, clean chroot.
- **P2**: snapshot integration for rollback of a source step, ABI-impact analysis, provenance reports.

Slice: these go into u29 (safety hardening). The unreadable-policy-file case is the one that most contradicts rule 32 today and is the first to fix.

## Audit of the code against the register (2026-10-19)

Read from the code, not assumed. Invariants 1, 2, 4, 8 and 9 hold; the others have these breaches.

- **A-1 (breaks 3, 32).** `source_built::read_from` returns an empty list for a file it cannot parse. `apply_source_policy` then plans no rebuild, so the next upgrade installs the binary without a word; and `record_to` after that writes a file with only the new entry, losing every other policy. Corruption must read as "unknown intent" and stop writes.
- **A-2 (breaks 12, 108, 79).** `upgrade::rebuild_policy` calls `history::begin` and, if it fails (another emrac running, a recovery that failed), builds and installs anyway: `if started.is_ok()` guards only the history record. It must stop for that package and mark it pending.
- **A-3 (breaks 120, 117).** `rebuild_policy` returns `Ok` when a rebuild failed, so `emrac upgrade` exits 0 although part of what was asked did not happen. It should exit non-zero (a distinct code for "upgraded, rebuild pending").
- **A-4 (rule 5, 116, 59).** `--yes` passes `--noconfirm`. pacman's default for "import this PGP key?" is yes, so a non-interactive run can import a key. Conflicts default to no and no `--force`, `--overwrite`, `--nodeps` or `SigLevel` override exists in the code (checked), but key import under `--yes` needs a decision: refuse, or require `--accept-keys`.
- **A-5 (rule 119).** A package pending since an earlier failed rebuild is not in the next upgrade plan unless the repository moves again, and `doctor` does not list it.
- **A-6 (rule 11).** The plan is checked again before commit only if more than 30 s passed since it was shown (`RECHECK_AFTER`). That is a deliberate cost trade-off; the strict reading is "always, immediately before".

Everything else in the sections above stands as written. Fix order: A-1, A-2, A-3, A-4, A-5, then the P0 list.

### Fixed after the audit

- **A-1 fixed.** `source_built::read_from` returns an error for a file it cannot read or parse; upgrade, `from-source` and every write refuse and leave the file untouched; `doctor` calls it a PROBLEM. Tests: `a_damaged_record_is_an_error_and_is_never_written_over`, three container checks.
- **A-2 fixed in code**: `rebuild_policy` builds nothing when `history::begin` fails; the package is marked pending. No automated test yet (needs an injected lock; add with the S-2 fault-injection set).
- **A-3 fixed.** `emrac upgrade` exits non-zero (`SourceRebuildPending`) after reporting the upgrade, when a rebuild failed. `--skip-source-builds` is a request and exits 0. Container check.
- **A-5 fixed.** Every upgrade plan (also an empty one) and `doctor` list pending rebuilds. Container checks.
- **A-4 open:** `--yes` and PGP key import needs a decision (see the conversation); **A-6** stays a deliberate trade-off.

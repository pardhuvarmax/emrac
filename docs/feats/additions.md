# Emrac: Additions

Further ideas and features for Emrac, collected in two batches. Everything here is deterministic and local: no AI or LLMs, and no scripting layer (decided against).

Status tags: `[idea]` not started, `[planned]` decided, `[building]` in progress, `[dropped]`.

- **Part A** (A1 to A22): trust, upgrade safety, hygiene, builds, multi-machine, usability.
- **Part B** (B1 to B9): higher-value features.
- **Part C** (C1 to C8): critical features. A package manager is judged on the worst day (a failed upgrade, a power cut, a bad mirror), so these are about correctness and survival.

---

## Part A: 22 ideas

### Supply-chain trust (mostly AUR)

- `[idea]` **A1. Typosquat and lookalike warnings.** Flag AUR packages whose names are near-copies of popular packages (one character off, swapped words, a `-bin` suffix on a name that already exists). Rules only, no guessing.
- `[idea]` **A2. Source-changed-without-bump alert.** Warn when a PKGBUILD's source URL or checksum changes but `pkgver` does not, or when sources move to a different host.
- `[idea]` **A3. New-maintainer and first-publish warnings.** Highlight recently adopted or very new packages in the preview, plus a "name shadows an official package" check.
- `[idea]` **A4. Signing-key pinning.** Remember which PGP keys signed a package's sources, and warn when the key changes.

### Upgrade safety

- `[idea]` **A5. Update risk rating.** A deterministic label per transaction (low, medium, high) based on what it touches: kernel, bootloader, glibc, graphics drivers, init, soname bumps, and unread Arch news. High risk suggests a snapshot first, or a reboot check afterward.
- `[idea]` **A6. Staged upgrades.** Split a big upgrade into ordered groups (core libraries, then desktop, then applications) so a failure stops early.
- `[idea]` **A7. Restart and reboot advisor.** After a commit, list services and processes still running old libraries, and say whether a reboot or logout is needed (like `needrestart`).
- `[idea]` **A8. Rollback rehearsal.** `emrac doctor --rollback-test` checks that a snapshot restore would actually work, before you need it.

### System hygiene

- `[idea]` **A9. Stray file scanner.** Find files under `/usr` and `/opt` that no package owns (left behind by `make install`, `pip --break-system-packages`, or manual installs), with a clear "who put this here" report.
- `[idea]` **A10. Config drift report.** Show how `/etc` differs from package defaults, including leftover `.pacnew` files, and what a package change would affect.
- `[idea]` **A11. Orphan lifecycle.** Orphans get a grace period, a recorded reason for removal, and a "restore" option instead of an immediate delete.
- `[idea]` **A12. Install reasons ledger.** Record why each package is installed (explicit, dependency of X, installed for project Y), and let `why` read from it.

### Build experience

- `[idea]` **A13. Build failure classifier.** Pattern rules that recognize common failures (missing dependency, network error, out of memory, failing tests, checksum mismatch) and print the likely cause and a fix.
- `[idea]` **A14. Soname rebuild planner.** When a library's soname bumps, list exactly which of your source-built packages need rebuilding, and offer to queue them.
- `[idea]` **A15. Clean-chroot build option.** Optionally build in a devtools-style clean chroot, so a build cannot depend on whatever happens to be installed.

### Multi-machine

- `[idea]` **A16. Machine diff.** Compare two machines' package sets (or one machine against a lockfile) and show what is missing or different.
- `[idea]` **A17. Remote apply.** Review a plan locally, then apply it on another machine over SSH with its own preview and confirmation.
- `[idea]` **A18. Container and image export.** Export a package set as a `pacstrap` list or a Containerfile, for reproducing a dev environment.

### Everyday usability

- `[idea]` **A19. Post-upgrade "what changed" report.** Notable changes per package: new dependencies, removed files, changed services, new config options, plus relevant news.
- `[idea]` **A20. Prompt and status-bar output.** A fast `emrac status --short` for updates available and unread news counts, for shell prompts and bars.
- `[idea]` **A21. Notifications.** Optional desktop notifications or a local webhook when a long build finishes or a scheduled update succeeds or fails.
- `[idea]` **A22. Foreign-package inventory.** Read-only listing of Flatpaks, language-global installs (pip, cargo, npm), and AppImages next to system packages, so conflicts are visible.

**Suggested starting set from Part A:** update risk rating (A5), restart advisor (A7), stray file scanner (A9), and the typosquat and source-change warnings (A1, A2). They reuse the plan preview and give users something concrete to trust.

---

## Part B: 9 higher-value features

- `[idea]` **B1. `emrac bisect`: find which update broke something.** Rolling releases fail in a way that is hard to trace: something worked last week and does not now. Bisect uses the transaction log and cached or archived packages. The user gives a "good" date or transaction and a test command (or judges manually). Emrac rolls back half of the changed packages, the user checks, and it narrows down to the culprit package and version in a few steps. It only touches packages in the changed set and respects dependencies, so it never leaves a partial upgrade. A git bisect for the system.
- `[idea]` **B2. `emrac try <pkg>`: run a package without installing it.** Build or fetch the package and run it inside the sandbox overlay, so nothing touches the real system. When the user exits, everything is discarded. Useful for testing an AUR program, a Wandrover suggestion, or a risky tool with no cleanup. It reuses the Layer 2 sandbox, so it is cheap to add.
- `[idea]` **B3. Pre-flight checks, including a kernel safety net.** Before any commit, check the things that break updates midway: free space on `/`, `/var/cache`, `/boot`, and the EFI partition; battery and AC power for long builds; network reachability. For kernel, initramfs, or bootloader transactions, also confirm that a fallback kernel and fallback initramfs exist, that the bootloader entry will be generated, and that the EFI partition has room. A full `/boot` mid-upgrade is a common way Arch systems end up unbootable.
- `[idea]` **B4. Conflict explainer.** When a transaction has conflicting or unsatisfiable dependencies, show the shortest chain that causes it ("A needs B>=2, but C pins B<2, and you asked for both"), then list the valid ways out: remove one, hold one, or swap an alternative. Derived directly from the dependency graph, so it is exact and repeatable.
- `[idea]` **B5. Driver and firmware doctor.** Compare detected hardware (PCI and USB IDs, loaded modules, missing-firmware kernel messages) with installed packages. Report mismatches such as a GPU driver that does not match the running kernel, DKMS modules not built for the new kernel, or firmware packages missing for a device. A rule-based mapping, not guessing.
- `[idea]` **B6. Machine manifest and bootstrap.** Export one file describing a machine: explicit packages, repository and `pacman.conf` settings, enabled services, build profiles, and pointers to config. `emrac bootstrap manifest.toml` then rebuilds that setup on a fresh install, showing the plan first. Turns a reinstall or new laptop into a reviewed, repeatable step.
- `[idea]` **B7. Downgrade assistant.** `emrac downgrade <pkg>` finds earlier versions in the cache or the Arch Linux Archive and works out which dependents also need to move, so the system is not left half-downgraded. It then offers a hold with a reason and an expiry.
- `[idea]` **B8. Link-check for software built outside the package manager.** Before an upgrade removes or changes a shared library, scan locally built or unpackaged binaries (the stray-file scanner's output, `~/.local`, `/opt`, `/usr/local`) for the sonames they need. If the upgrade would break one, warn in the preview with the binary's path and the package change that causes it. Package-level soname checks miss exactly this case.
- `[idea]` **B9. Metered and flaky-network mode.** Resumable downloads, automatic mirror fallback, a bandwidth cap, and a "metered" mode that defers large packages and downloads only what the user approves.

**Suggested starting set from Part B:** pre-flight and kernel safety net (B3), `bisect` (B1), and `try` (B2). Pre-flight prevents the worst failure, `bisect` is the standout feature, and `try` is nearly free given the sandbox.

---

## Part C: 8 critical features

- `[idea]` **C1. Crash-safe transaction journal.**
  - Failure it prevents: a power cut, kernel panic, or killed process mid-commit leaves a half-applied system and a stale lock.
  - How: before touching the system, write the plan to an fsynced write-ahead journal and record each step as it completes. On the next start, Emrac detects the unfinished transaction and offers to finish it or roll it back to the snapshot. Stale locks are checked (is the owning process alive?) and never deleted blindly.
- `[idea]` **C2. Plan integrity: what was previewed is what runs.**
  - Failure it prevents: the repositories change between preview and commit, or a saved plan is altered, so the executed transaction differs from the approved one.
  - How: bind each plan to a hash of the repository state, package versions, and checksums. Before commit, re-verify the hash and refuse to apply a stale or modified plan, then re-plan and re-preview.
- `[idea]` **C3. Fail-closed verification chain.**
  - Failure it prevents: installing something unverified, or the common Arch breakage where an old system cannot verify new packages.
  - How: verify database signatures, package signatures, and source checksums, and stop if any check fails, with no "continue anyway" shortcut. Update the keyring packages first, in their own step, before the rest of an upgrade. Check for large clock skew first, since a wrong clock makes signatures look expired.
- `[idea]` **C4. Partial-upgrade hard gate.**
  - Failure it prevents: the classic way Arch systems break, installing from fresh databases onto a system that was not fully upgraded.
  - How: make it a planner rule, not just a `doctor` warning. If a transaction would leave the system in an unsupported partial state (stale databases, split dependency sets), refuse it and explain the exact packages involved and the correct fix.
- `[idea]` **C5. Mirror and repository trust.**
  - Failure it prevents: stale, frozen, or malicious mirrors, and silent downgrades.
  - How: check database freshness against a timestamp and reject databases older than the one already seen (rollback protection). Compare database hashes across two mirrors. Refuse to install a package version older than the installed one unless the user explicitly asked for a downgrade.
- `[idea]` **C6. Protecting pacman's own database.**
  - Failure it prevents: Emrac corrupting `/var/lib/pacman`, or running at the same time as pacman.
  - How: take a small backup of the local package database before every transaction (it is tiny and cheap), run an integrity check before and after, and share pacman's lock so the two never run concurrently.
- `[idea]` **C7. Rescue binary (`emrac-rescue`).**
  - Failure it prevents: a library upgrade (for example glibc) breaks the package manager itself, so the system cannot be repaired with it.
  - How: ship a small statically linked recovery binary that does not depend on system libraries. It can read the journal, restore the pacman database backup, and roll back to a snapshot even when the main binary cannot start. Document how to run it from a live USB or a recovery shell.
- `[idea]` **C8. Privilege separation, decided early.**
  - Why critical: the largest and most complex parts of Emrac (resolver, TUI, builds) should never run as root. A small privileged helper accepts only a verified, saved plan (see C2) and executes it. It is much easier to design this in at the start than to retrofit it. This is the open question already recorded in `ideas.md`.

**Dependencies between them:** C1, C2, and C8 depend on each other. The journal records the plan, the plan hash is checked before the privileged helper executes it. Design them as one unit.

**Suggested order:** journal (C1), plan integrity (C2), and the partial-upgrade gate (C4) stop the most common and most serious failures. After that, the rescue binary (C7) and the pacman database protection (C6), because both are cheap and could save a system.

---

## Notes

- Part C items should be designed first and as a group, since later features (B1 bisect, B3 pre-flight, A5 risk rating, rollback in general) rely on the journal, plan hash, and privilege split.
- Several items relate to existing designs: B2 reuses the Layer 2 sandbox from `emrac.md`, B6 and A16 build on the lockfile idea in `ideas.md`, A9 and B8 share the same stray-file scan, and B7 pairs with holds-with-expiry in `ideas.md`.
- Before building any of these, check them against `SPEC.md`, since a few (downgrade, rollback testing) may overlap with its history and rollback commands.

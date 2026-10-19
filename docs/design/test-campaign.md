# Test campaign: what to challenge next

Each row is a way the system can be wrong that a passing suite does not show. Status is what exists in the tree
today (unit tests, `dev/e2e/*.sh`), not what is hoped.

| Pri | Campaign | Covered today | Not covered |
|---|---|---|---|
| P0 | State corruption | A malformed or deleted source-policy record is an error and is never written over (unit + container); JSON state is written by temp file, fsync, rename | Truncated or malformed `history.jsonl`, journal, ledger, pins, snapshots, build-times, rebuild acks; a write that fails halfway (disk full, read-only filesystem); every state file needs the same "unreadable is an error, never empty" rule |
| P0 | External pacman activity | pre-flight refuses while pacman runs or its lock is held (tested with a hook that keeps pacman busy); a second emrac started during a transaction is refused and the first finishes untouched (tested); six emrac processes changing the policy record at once lose nothing (lock, tested); a direct `pacman -Syu`/`-R` is reconciled afterwards | a package changed by pacman during a long source build (the plan is checked again at the commit, not during the build) |
| P0 | Crash recovery | A rebuild cut off at each of 9 boundaries (journal begin, before and after the build, after `pacman -U`, after the policy record, before and after the history record, after the ledger sync, after the journal is cleared); after each the record, the installed package, the journal, the history and the ledger are compared and the next run must finish the job; plus kill -9 mid-build and after the binary upgrade | `install` and `upgrade` are swept the same way (`crash.sh`, 11 checks) and a power cut in the middle of `pacman -U` is simulated by killing emrac and pacman together; the same sweep for `remove`, `restore` and `downgrade`; the snapshot and pin files |
| P1 | Reproducibility | `score` has a reproducible-build dimension from the Arch rebuilders; the reproducible-build check is partial | Building the same source twice here and comparing artifacts (file lists, hashes, mtimes, build paths) with nondeterministic metadata accounted for |
| P1 | Supply-chain changes between review and build | `makepkg --verifysource` checks source checksums; AUR review shows the diff; **what was reviewed is what is built** (done, `recipe_digest.rs`, 5 container checks): a digest of the recipe files taken at the review or audit and checked before `makepkg` and after each host step that sources the PKGBUILD | the unsandboxed path (`--no-sandbox`) has no check after the build starts; source *archives* are covered by the checksums in the PKGBUILD, not by this |
| P1 | Multi-package failures | unknown name in a list; second of two failing says "installed X; not installed Y" | one rebuild failing after several packages were already upgraded (upgrade with two policy packages); mixed outcomes in the history and the exit code |
| P1 | Security boundaries | bubblewrap: read-only root, empty home, cleared environment, no network | escape attempts (writes outside the build tree, `/proc` tricks, symlinks in `$srcdir` and `$pkgdir`, setuid/capabilities in the built package, path traversal in package file lists, hostile `.pkg.tar` names) |
| P2 | Long-term consistency | none | hundreds of upgrade/rebuild/rollback/policy cycles on one machine, then `history verify`, ledger compare, `from-source`, `doctor` must all agree |

## The three to do first

1. **Concurrent pacman activity** (P0). Start an emrac operation and, during it, change packages through pacman or a
   second emrac; assert the second is refused or the first stops before committing, and that state is reconciled
   afterwards. Needs: the policy-file lock (S-7), and a way to hold an emrac operation open in the container (a
   fault point that waits, test build only, like `EMRAC_FAULT`).
2. **Crash at every transition** (P0). Generalise the fault points into a list of named durable boundaries; a test
   loop runs the same scenario once per point, restarts emrac, and compares four views: installed packages, pending
   rebuild marks, the journal/history, the ledger. One assertion function, one run per point.
3. **Review-to-build integrity** (P1) — **done**. A digest of the whole packaging directory (PKGBUILD, `.install`, patches,
   everything in the checkout but `.git`) taken when it is reviewed or audited and checked again immediately before
   `makepkg`; any change stops the build with the changed files named and asks for a new review. The same digest is
   kept in the build's provenance record (S2). Test: review, alter PKGBUILD / add a file / change a patch, build →
   refused.

State corruption comes with 2 (a corruption sweep over every state file) and is cheap once the shared reader exists.

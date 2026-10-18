Package forensics would answer the questions you ask after something goes wrong, from local data only and read-only by default: what changed, where did it come from, was it tampered with, and what is still on the system.

**Commands** (proposed, under `emrac forensics`)

| Command | What it answers |
|---|---|
| `timeline --since <date> [--pkg <name>]` | One ordered timeline merging transactions, `pacman.log`, file change times, and enabled units. Each event shows its source |
| `provenance <pkg or file>` | Where it came from: repository or AUR, PKGBUILD hash and git commit, source URLs and checksums, build environment, signature status, the transaction that installed it, and its review record |
| `verify [--deep]` | Compare installed files to package metadata: hashes, permissions, ownership, setuid bits, missing and extra files. With `--deep`, re-extract the cached package and compare, which catches a tampered metadata record |
| `persistence` | List every persistence point (systemd units and timers, cron, udev, `ld.so.preload`, shell startup files, SSH keys, PAM, sudoers, autostart, kernel modules, initramfs hooks) and which package owns each. Unowned ones are flagged |
| `delta <pkg> <v1> <v2>` | Compare two versions: files added or removed, hash and permission changes, scriptlet and hook changes, dependency changes, and ELF imports and embedded URLs (from the binary inspector idea) |
| `baseline create / diff` | Save a hash baseline of package files and the persistence set at a known-good time, then show exactly what differs now |
| `capture` | Write an evidence bundle (package list, transaction log, hashes, persistence inventory, timeline), with a hash manifest and an optional signature, ready to hand to a responder. Redaction option for paths and usernames |

**Existing features it builds on:** the transaction journal, snapshots (compare live files against a snapshot), pacman database backups (C6), stored build inputs, AUR git history, the dynamic analysis event logs (stored per transaction, so you can view what a build did long afterward), the review ledger, and `emrac incident`.

**Design points**
- **Offline mode:** `--root /mnt` analyzes a mounted disk, so you can run it from a live USB or the rescue binary (C7), without trusting the running system's kernel.
- **Tamper-evident history:** hash-chain the transaction log so edits or deletions show up. An attacker with root can still rewrite local data, so optionally store the chain's latest hash somewhere else (a git remote or a printed code).
- **Read-only:** forensics never repairs or deletes anything. Remediation goes through the normal plan and preview.

**Limits**
- A rootkit can hide files from a running system, so the offline mode matters.
- If the cached package is gone, deep verification needs the Arch Linux Archive (official packages only). AUR rebuilds may not be reproducible.
- Static analysis can't prove a binary is harmless.

**Build first:** `persistence` and `verify`, then `timeline`, then `baseline`. Those give the most answers with the least new machinery.

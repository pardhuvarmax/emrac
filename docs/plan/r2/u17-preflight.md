# Slice u17 — pre-flight checks and the partial-upgrade gate

From `docs/feats/additions.md` B3 (pre-flight, with the kernel safety net) and C4 (partial-upgrade
hard gate). Both run **after the plan is shown and before anything is asked or changed**, so a
transaction that is going to fail halfway, or leave the machine unbootable, is stopped while
stopping is free.

## Where it runs

`install`, `upgrade`, `reinstall`, `downgrade`, `rollback`/`undo`/`redo`, `restore`, `remove`
and `purge`, after the plan is printed. `--dry-run` runs it too and prints what would block, but
never fails (a preview is allowed to say "this would not run"). An empty plan skips it.

## The checks (`preflight.rs`: facts gathered, then a pure `evaluate`)

| Id | Applies to | Rule |
|---|---|---|
| `space-root` | anything that installs | free space on `/` is at least the plan's installed-size growth plus 512 MiB |
| `space-cache` | anything that downloads | free space where pacman keeps its cache is at least the download size plus 256 MiB (when the cache and `/` are one filesystem the two are added) |
| `space-boot` | a kernel is touched | free space on `/boot` is at least 1.5 × the biggest kernel or initramfs file plus 16 MiB; 128 MiB when the files can't be read |
| `space-esp` | a kernel or a bootloader is touched | the EFI system partition (`/efi`, `/boot/efi`, or `/boot` when it is FAT) has at least 64 MiB free |
| `power` | a kernel, an AUR build or a download over 500 MiB | on battery under 20 %: **problem**; under 50 %: warning; otherwise a note. Machines without a battery say nothing |
| `network` | a download, unless `--offline` | the first three mirrors of the first repository are tried (name lookup and a TCP connection, 4 s each); none answering is a **problem**, some failing a note |
| `kernel-fallback` | a kernel is touched | another kernel is installed, so a bad one can be booted around: if not, a warning with the command to install `linux-lts` |
| `kernel-initramfs` | a kernel is touched | `/boot/initramfs-<kernel>-fallback.img` exists (a note when `/boot` can't be read) |
| `kernel-bootloader` | a kernel is touched | the bootloader's configuration (GRUB, systemd-boot, Limine) names the kernel image; a warning when it exists and does not; a note when it isn't recognised or can't be read |
| `kernel-removal` | `remove`, `purge` | removing every installed kernel is a **problem**; removing one of several is fine |
| `partial-upgrade` | `install` of repository packages, `upgrade <pkg>…` | see below |

A kernel counts as touched when the plan upgrades, reinstalls or downgrades an installed kernel
package (found from `/usr/lib/modules/*/pkgbase`, which any user can read). `/boot` is often
root-only (it is here); every check that needs it says so and degrades to a note instead of
guessing.

## The partial-upgrade gate (C4)

Installing a package from the repositories, or upgrading only some packages, while other
installed packages have upgrades waiting, is the classic way to break an Arch system: the new
package was built against newer libraries than the ones installed. pacman does not stop it.
emrac now does: the packages that would be upgraded by `emrac upgrade` (against the databases
pacman would use now), minus the packages already in this plan and minus the ones you **pinned on
purpose**, must be empty. Otherwise the check is a **problem** that lists them (up to 12), names
the cause and gives the fix, `emrac upgrade`, or `--allow-partial` for this one run.
AUR-only installs, reinstalls, downgrades, rollbacks and restores are not affected: they do not
take new versions out of the repositories.

## Settings and flags

```
[safety]
preflight       = "block"    # block | warn | off
partial_upgrade = "refuse"   # refuse | warn | off
```

`block` stops on a problem, `warn` shows problems as warnings and goes on, `off` skips the
checks. `refuse | warn | off` is the same for the gate. `--allow-partial` on `install` and
`upgrade` lets one run through; there is no flag to skip the rest, so that habit isn't formed
(change the setting if you mean it).

Output is a short block under the plan: nothing but a one-line "pre-flight passed" when all is
well, and the findings, worst first, when not. With `--json` nothing extra is printed (the plan's
JSON stays clean); a blocked run still fails with the reasons on stderr.

## Code

- `preflight.rs`: `Facts`, `gather`, `evaluate` (pure), helpers for kernels, bootloader text,
  `/proc/mounts` and power supplies.
- `sources.rs`: `partial_upgrade_facts` (uses the same resolver as `upgrade`), `preflight`.
- `config.rs` / `config_style.rs`: `[safety]`. `Error::Preflight`.
- CLI: `commands/preflight.rs` and one call in each of the commands above.

## Verification

Unit tests for every rule in `evaluate`, the kernel and bootloader parsers, the power rules, the
mode mapping. Container: install with an upgrade pending is refused, `--allow-partial` lets it
through, a pinned package doesn't count, the same install after `upgrade` is fine; a full
`upgrade --dry-run` prints the pre-flight; removing the only kernel is refused (a fake kernel
package, not a real one).

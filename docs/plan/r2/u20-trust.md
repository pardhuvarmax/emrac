# Slice u20 — trust and the safety net

The last feature slice before r3. Five parts, each its own commit with its own tests, built in
this order. Chosen from `additions.md` (A1–A5, A7, A19, C3–C6) and `ideas.md` (AUR trust
signals, Arch news gate, post-commit health check).

## 1. AUR trust signals (A1–A4, "AUR trust signals")

Shown in the review before an AUR package is built, in `emrac info` for AUR packages, and kept
as rules only, with no guessing. Each signal has a level: `note`, `warning` or `high`.

| Signal | Rule |
|---|---|
| lookalike name | not an official name, but one character from one (insert, delete, substitute, swap of neighbours), or the same words in another order, or an official name with `-`/`_` swapped; also near an installed package |
| `-bin`/`-git` of an official name | an official package `x` exists and this is `x-bin` (note) |
| claims an official package | its `provides`, `conflicts` or `replaces` name an official package (warning) |
| orphaned | no maintainer (warning) |
| new | first submitted less than 30 days ago (warning) |
| few users | under 5 votes (note, only when also new) |
| flagged out of date | the AUR flag is set (note) |
| maintainer changed | differs from the one recorded when you last built it (**high**) |
| sources changed, version not | `pkgver` and `pkgrel` are the same as in the last reviewed `.SRCINFO` but a source address or checksum differs (**high**) |
| sources moved host | a source now comes from a host not used before (warning) |
| signing keys changed | `validpgpkeys` differs from the pinned set: a new key, or a key gone (**high**) |

`high` signals use the same default-no confirmation as a high audit finding (`--accept-risks`
passes it; `--yes` does not). State is `~/.local/share/emrac/aur-trust.json` (maintainer, source
hosts, pinned keys per package), written when you go ahead with a build. Data: the AUR RPC
(maintainer, first submitted, last modified, votes, out-of-date, provides/conflicts/replaces) and
the `.SRCINFO` of the old and new revision.

## 2. The Arch news gate

`emrac news [--all] [--json]` reads `https://archlinux.org/feeds/news/` (cached in
`~/.local/share/emrac/news.json`, labelled stale offline), lists items newest first, and marks
the ones you have not seen. `emrac news ack` marks all as seen. In `upgrade` the unread items are
shown inside the plan. **Warn by default; block only when critical.**

*Critical* is about how risky the change is, not about whether it names a package in this
upgrade. An item is critical when its text shows any of:

- a **breaking change or manual intervention**: "manual intervention", "action required",
  "breaking change", "will fail", "will break", "no longer", "before upgrading", "must be",
  "incompatible", "requires you to";
- a change that touches a **critical part of the system**: the kernel (`linux`, `initramfs`,
  `mkinitcpio`, modules, `dkms`), the bootloader (`grub`, `systemd-boot`, `limine`, `efi`,
  `secure boot`), the C library and toolchain (`glibc`, `gcc`, `libstdc++`), init and boot
  (`systemd`), `pacman` and its keyring, `openssl`/`ca-certificates`, graphics and firmware
  drivers (`nvidia`, `mesa`, `linux-firmware`), the filesystem layout (`/usr`, `/lib`, `usrmerge`,
  `filesystem`).

An unread critical item stops the run (not a `--dry-run`), listing why it counts ("touches the
kernel", "asks for manual intervention"), until you have read it and run `emrac news ack`, or pass
`--ack-news` for this run. Every other unread item is only shown. A feed that can't be read is a
warning, never a block. Setting `safety.arch_news = gate | warn | off` (`warn` never blocks).
The same classes feed the update risk rating in part 5, so the two agree on what is risky.

## 3. Protecting pacman's own database (C6)

Before each transaction a backup of `/var/lib/pacman/local` (a compressed tar, kept in
`~/.local/share/emrac/pacman-db/`, newest 3); `pacman -Dk` before (warns if it is already
inconsistent) and after (warns and names the backup if the transaction left it so). A new
pre-flight check: pacman's lock file present while a pacman process runs is a problem, present
with none running is a warning that says how to remove it. `safety.db_backup = on | off`.
`doctor` shows the last backup. The restore recipe is in `docs/design/`.

## 4. Repository trust, a slice of C5

- A plan that would install an older version than the installed one (not a downgrade you asked
  for) is a pre-flight problem naming the packages.
- A high-water mark per repository (the newest modification time of its sync database ever seen,
  `~/.local/share/emrac/repo-marks.json`): a database older than the mark by over a day is a
  warning in the pre-flight and `doctor` (a mirror serving old or rolled-back data).
Not built: comparing database hashes across two mirrors.

## 5. After the upgrade, and before it (A5, A7, A19, the health check)

- **Update risk** (before an upgrade, under the plan): `low`, `medium` or `high` from rules: high
  for a kernel, bootloader, `glibc`, `systemd`, `pacman`, `openssl` minor+, or a critical news item;
  medium for graphics drivers, the toolchain, a Python, Perl or Qt minor bump, an AUR rebuild, or
  over 150 packages. It names the reasons and, for high, suggests a snapshot.
- **After an upgrade** (and `emrac report`): what changed (notable versions: the kernel, glibc,
  the desktop's toolkits), the running kernel against the newest installed one, services started
  before an upgraded package owning their unit (`systemctl restart …`), core libraries upgraded
  (reboot advised), failed units, AUR packages broken (u19), new `.pacnew` files, and the kernel
  and boot checks again. It suggests `emrac undo` when something failed. It does not roll back by
  itself.
Rootless: services and processes of other users are not visible without root, and the report
says so.

## Not in this slice

C7 (rescue binary), C8 (privilege separation), the TUI, `bisect`, `try`, the hygiene tools
and the multi-machine ideas; the SPEC's source/binary/recommend engine.

## Verification

Pure rules get unit tests (each signal, the news parser and classifier on a fixture feed, the
risk rules, the report). Container: a local AUR-like repository for the trust signals, a served
feed for the news gate (a `file://` feed address for tests), a corrupted database for the backup
check, and a fixture upgrade for the report.

## What was built, and where it differs

- All five parts, in five commits. `aur_trust.rs`, `arch_news.rs`, `pacman_db.rs`, `repo_trust.rs`,
  `upgrade_report.rs`; settings `safety.arch_news` and `safety.db_backup`; commands `emrac news`,
  `emrac news ack`, `emrac report`; flag `--ack-news`.
- The news gate's "critical" follows the developer's definition (risk: breaking, manual
  intervention, or a critical part of the system), not "names a package in this upgrade". Parts of
  the system are matched in the **title** only: a first version matched the body, and an item that
  merely told you to run `pacman` was flagged as touching pacman.
- "Unread" before any acknowledgement means since your last upgrade (two weeks without one): the
  first version called the whole feed unread, which would have blocked the first upgrade.
- The database backup leaves out the `mtree` files: 6 MiB instead of 43 MiB, and `pacman` doesn't
  need them.
- `x-git` providing and conflicting with `x` is not flagged as claiming an official package (the
  first version flagged `quickshell-git`).
- Service start times come from `systemctl show --timestamp=unix`; the boot-time arithmetic that
  was considered drifts after every suspend.
- A running kernel whose modules folder has gone is reported only when an installed kernel of the
  same kind is there (the kind comes from the release's suffix: `-zen`, `-lts`, `-hardened`,
  `-arch`): "linux-zen was upgraded to X and you are still running Y". A first version said "the
  running kernel has been removed" whenever the folder was missing; the container test showed it
  on a container (which runs the host's kernel and has no package for it), and the developer
  rightly asked what it meant on a machine where nothing was removed. With no installed kernel of
  that kind nothing is said.
- Container runs: `final-check.sh` (50 checks) and `preflight.sh` (24); the fixture repository
  of the second now also drives the news, backup, lock, downgrade and rollback cases.

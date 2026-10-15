# Trust and the safety net

What emrac checks about the AUR, the Arch news, pacman's own database and the repositories, and
what it tells you after an upgrade. For people who use emrac. (Plan: `docs/plan/r2/u20-trust.md`.)

## AUR trust signals

Shown in the review before an AUR package is built, and in `emrac info <aur package>`. Rules, not
scores. A **high** signal asks the same default-no question as a high audit finding (`--accept-risks`
passes it, `--yes` does not).

| Signal | Meaning |
|---|---|
| lookalike | the name is one letter from an official or installed package, or its words are the same in another order |
| variant | `x-bin`, `x-appimage` of an official `x` (note) |
| claims official | it provides, conflicts with or replaces an official package other than its own base (`x-git` providing `x` is expected) |
| orphaned, new, few users, out of date | what the AUR says (less than 30 days old is "new") |
| **maintainer changed** | differs from the one recorded when you last built it |
| **sources changed under the same version** | `pkgver` and `pkgrel` are the same in the last reviewed `.SRCINFO` but a source address or checksum differs |
| sources moved host | a source comes from a host not seen before |
| **signing keys changed** | `validpgpkeys` differs from the keys pinned at your last build |

What emrac remembers (maintainer, source hosts, signing keys per package) is in
`~/.local/share/emrac/aur-trust.json`, written when a build you went ahead with is installed.

## The Arch news gate

`emrac news` lists the Arch Linux news (`https://archlinux.org/feeds/news/`, cached), `emrac news
ack` marks it all as seen. Before an upgrade the unread items are shown under the plan. They only
warn, **except** a critical one, which stops the upgrade. *Critical* is about risk, not about whether
the item names a package in this upgrade:

- a breaking change, or manual intervention ("requires manual intervention", "action required",
  "before upgrading", "breaking changes", "will fail", …); or
- a title that touches a critical part of the system: the kernel or initramfs, the bootloader, the
  C library and toolchain, systemd, pacman and its keyring, OpenSSL, graphics drivers and firmware,
  the filesystem layout.

Read it, run `emrac news ack`, and upgrade; or pass `--ack-news` once. Before you have acknowledged
anything, "unread" means "since your last upgrade" (two weeks if there was none). Setting
`safety.arch_news = gate | warn | off`. A feed that can't be read is a warning, never a block.

## Update risk and the report

Under every upgrade plan: **LOW, MEDIUM or HIGH**, with the reasons. High: a new kernel, glibc,
pacman, GRUB, mkinitcpio, OpenSSL or filesystem (minor or major step), a new systemd (new first
number), or critical news; medium: a kernel patch release, graphics drivers, the toolchain, Python,
Perl or Qt steps, AUR rebuilds, or over 150 packages. High suggests a snapshot first.

After an upgrade (and any time with `emrac report`, for the last upgrade in the history): notable
changes; a reboot when the running kernel is older than the installed one or was removed, or glibc,
systemd or the firmware changed; services started before an upgraded package that owns their unit
(`sudo systemctl restart …`); units that failed; AUR packages the new libraries broke (`emrac
rebuild --check`); new `.pacnew` files. As a normal user it sees systemd as a user does; processes
of other users are not inspected.

## Protecting pacman's database

Before each transaction emrac saves `/var/lib/pacman/local` to `~/.local/share/emrac/pacman-db/` (the
newest 3, without the large `mtree` files, about 6 MiB) and runs `pacman -Dk` before and after.
If the database was already inconsistent it says so; if the transaction left it inconsistent it says
so and prints the command that puts the backup back:

    sudo mv /var/lib/pacman/local /var/lib/pacman/local.broken \
      && sudo tar -xzf ~/.local/share/emrac/pacman-db/local-<time>.tar.gz -C /var/lib/pacman \
      && pacman -Dk

(The `mtree` files are only used by `pacman -Qkk`; `emrac reinstall` brings them back.) A pacman
that is running, or a lock file left behind, is a pre-flight finding. `safety.db_backup = on | off`.

## Repository trust

A plan that would install an older version than the one you have (without being a downgrade you
asked for) is a pre-flight problem naming the packages. Each repository's database time is
remembered (`~/.local/share/emrac/repo-marks.json`); one more than a day older than the newest seen
is a warning in the pre-flight and in `doctor`: a mirror serving old or rolled-back data. Not done:
comparing database hashes across mirrors.

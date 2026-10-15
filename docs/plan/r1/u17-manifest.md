# Slice u17 — split build from install; manifest of what a package changes

Implements §3.5 of `docs/design/source-build.md` and closes the last limit named
in u15/u16: the audit reads *source*, but what actually reaches the system is
the **built package** — setuid files, systemd units, pacman hooks, sudoers
drop-ins, users, an `.INSTALL` scriptlet generated during `package()`. Only the
built package can be inspected for that.

## Design

- **Split `makepkg -si`.** AUR builds become `makepkg -s --noconfirm --needed`
  (builds, installing makedepends through the existing `sudo pacman -S
  --asdeps` path, but not the package), then emrac inspects the result, then
  installs the built files with `sudo pacman -U`. makepkg's "already built"
  exit (13) is accepted: the existing package file is used, as `-si` did.
  The built files come from `makepkg --packagelist` (exact names, split
  packages included).
- **Inspect with `bsdtar`** (libarchive; already a pacman dependency): `-tv`
  for the file list with modes and owners, `-xO` for `.PKGINFO`, `.INSTALL` and
  a few small files whose contents matter. Reading only; the package is never
  extracted or executed.
- **The manifest** (`manifest.rs`, pure over a file list and a reader closure):
  a short summary (files, installed size, scriptlet yes/no, systemd units,
  users/groups created, pacman hooks) plus findings in the audit's severity
  vocabulary, so the same policy applies:

  | What | Severity |
  |---|---|
  | `etc/ld.so.preload` shipped | critical |
  | setuid file | high; setgid: warning |
  | pacman hook (`usr/share/libalpm/hooks`) — runs as root on later transactions | warning; its `Exec` line is run through the script rules, which is what escalates it (6% of official packages ship a hook, so a blanket high would just train people to say yes) |
  | sudoers drop-in, cron entries | high |
  | files under `home/`, `root/` | high |
  | `etc/profile.d` script (runs for every login shell) | warning, content scanned |
  | service enabled by default (`*.wants/` symlink) | warning |
  | files outside `usr`, `etc`, `opt`, `var`, `srv`, `boot` | warning |
  | systemd units, sysusers/tmpfiles, `.INSTALL`, replaces/conflicts | info (listed) |
  | the packaged `.INSTALL` scriptlet | scanned with the scriptlet rules |

- **Policy**, same as the audit: critical → emrac refuses to install (the built
  file stays in the build directory, and the message says so); high → the
  default-no prompt that `--yes` does not answer, `--accept-risks` does;
  otherwise it installs without a further question (the user already confirmed
  the install). The manifest is printed to stderr for every AUR build.
- **`emrac audit <file.pkg.tar.*>`** inspects any package file the same way —
  useful before `pacman -U` on something downloaded, and the way this slice is
  tested without installing anything.
- **Reinstall's AUR rebuild** goes through the same build → manifest → install
  path.
- A declined or refused install returns an error naming the built file; the
  history records it as a failed transaction like any other.

Out of scope: inspecting official-repo packages before install (pacman has
already verified their signatures; `audit <file>` works on them if wanted),
signature checks of the built package, the sandbox (u18 onward).

## Files

- `manifest.rs` (new, core); `backend/archive_reader.rs`-style helper for
  `bsdtar`; `audit.rs` — expose the script/scriptlet scanners;
  `backend/aur_build.rs` — `build`, `built_files`; `sources.rs` —
  `build_aur_package`, `install_built`, `inspect_package`;
  `error.rs`.
- CLI: `commands/aur_install.rs` (shared build → manifest → install),
  `install.rs`, `upgrade.rs`, `reinstall.rs`, `audit.rs`, `output.rs`.

## Found while testing (all fixed, all pinned by tests)

- The script rules were quote-aware, so a payload passed as a string —
  `sh -c 'echo k >> ~/.ssh/authorized_keys'`, `bash -c "curl … | sh"`,
  `eval '…'` — was never read. This applies to PKGBUILDs and shipped scripts as
  much as to hooks. The scanner now recurses into `sh -c`/`eval` strings (three
  levels deep).
- Calibration over the 932 official packages in a pacman cache: no critical
  finding; the only high findings were 17 legitimate setuid helpers
  (`fusermount`, `mount.cifs`, `chrome-sandbox`, …), and 44 packages ship a
  pacman hook. A blanket "hook = high" would have made the prompt noise, so a
  plain hook is a warning and only what it *runs* (scanned as code) escalates it.

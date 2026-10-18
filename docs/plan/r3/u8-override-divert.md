# Slice u8 (after r3) — `override` and `divert`: local file policy that survives upgrades

pacman puts a package's files back exactly as the package ships them, every time: on an upgrade **and** on a
reinstall of the same version. Two things you may want to keep are lost silently:

- the **mode and owner** of a packaged file (a `4750 nobody:users` helper, a `640` log directory entry);
- your **own version of a packaged file** that is not a `backup=()` config file (a patched library, a replaced
  script). pacman overwrites it even when upstream did not change it.

Both were checked on real pacman first (container, pacman 7.1): an upgrade turned `4750 nobody:users` into
`755 root:root` although the file's content was identical; a local edit of a plain file was replaced by an
upgrade whose version of the file was identical; a post-transaction hook runs as root in `/`, sees the new file
already in place, and also runs on a reinstall; removal deletes a locally modified plain file.

Idea source: dpkg's `dpkg-statoverride` (persistent owner/mode of a packaged file) and `dpkg-divert` (keep your file,
the package's goes elsewhere). Neither exists for pacman, whose only tools are `NoUpgrade` (always a `.pacnew`, for
`backup=` files) and `NoExtract` (never extract). The mechanism here is Arch's own: a pacman hook that re-applies
the policy after every transaction.

```
emrac override set /usr/bin/helper --mode 4750 --owner root --group wheel
emrac override list
emrac override remove /usr/bin/helper

emrac divert add /usr/lib/libfoo.so.1     # your current file is kept; upgrades set the package's version aside
emrac divert diff /usr/lib/libfoo.so.1    # yours against the package's new version
emrac divert list | remove

emrac policy status                        # what is configured and whether it is in effect
emrac policy apply                         # apply now
```

## Design

- **Policy file** `/etc/emrac/policy.toml` (root, 0644; admin-readable, hand-editable): `[[override]]` with `path`, `mode`,
  `owner`, `group`, `note`; `[[divert]]` with `path`, `note`. Your copy of a diverted file lives in
  `/var/lib/emrac/divert/<id>/local` (0700 directory, the file keeps its own mode and owner), the package's version
  that an upgrade replaced in `.../package`.
- **Hook** `zz-emrac-policy.hook` in the last `HookDir` of `pacman.conf`: a `Path` trigger per policy path (Install,
  Upgrade; Remove too for diversions), `PostTransaction`, `Exec = /usr/bin/emrac policy-helper apply`. Written by the
  helper, removed when the policy is empty. emrac's hook preview (u4) lists it like any other.
- **Apply** (root, from the hook or by hand):
  - override: `chmod`/`chown` through an open descriptor (no symlink following) when the file differs from the policy;
    a missing file or a symlink is reported, not touched;
  - divert: compares three sha256 sums: the package's (from its `mtree`, recorded by pacman), the file now, and your
    stored copy. **Now = package's**: pacman just overwrote yours, so the package's version is set aside and your copy
    is put back (written next to the file, then `rename`d, with your mode and owner). **Now = yours**: nothing to do.
    **Now = neither**: you edited it since; the stored copy is refreshed from it. File gone because the package was
    removed: **nothing is put back** (a file it ships would then already exist and the package could not be installed
    again: found by the container run); your copy waits in the store and comes back when the package is installed again.
  - a second, **pre-transaction** hook (`zz-emrac-policy-pre.hook`, never `AbortOnFail`) saves an edit of a diverted file
    before pacman overwrites or removes it: an edit made between transactions would otherwise be lost, because the
    post-transaction hook only sees what pacman left.
- **Risk: a diversion freezes the file** (see the section below). Each diversion records the sha256 and the version of
  the package's file at the time (`base`), so emrac can tell when the package has changed it since.
- **Refusals** (at the time you add them): a path that is not absolute, not normal (`..`, `//`, trailing `/`), under
  `/proc /sys /dev /run`; a mode that is not 3 or 4 octal digits; a divert of a file no package owns, of a
  `backup=` file (use `NoUpgrade`), of a symlink or directory, or of a file whose package recorded no sha256;
  a setuid or setgid mode asks for a confirmation (default no) because it makes the program run with the owner's rights.
- **Location guard**: root acts on a policy file only if every directory from `/` down to it is owned by root and
  writable by no one else, checked when the policy is written and again at every apply; otherwise a swapped directory
  or link could redirect root's write.
- **Trust of the hook**: root runs `emrac` from a pacman hook, so the helper writes the hook only if the executable
  is `/usr/bin/emrac` or `/usr/local/bin/emrac`, owned by root and writable by no one else, with root-owned,
  non-writable parent directories. A user-writable copy would be a way to run code as root and is refused.
- **Privilege**: user commands change nothing themselves; they run the hidden `emrac policy-helper <verb>` (through
  `sudo` unless already root), which validates every argument again and takes a lock. The hook calls the same helper.
- **Where emrac knows about it**:
  - plans (`install`, `remove`, `upgrade`) list the policy entries their packages touch and what will happen
    (`PlanPreview.policy`), and warn when the hook is missing so the policy would **not** be re-applied;
  - `verify`/`check` treat a matching override as the expected mode/owner and a diversion as the expected content;
  - `doctor` has a row for the policy (not in effect, hook missing or untrusted, package version set aside).

## Risk: what a diversion costs, and what emrac does about it

A diverted file keeps your version through every upgrade, so it stops getting that package's fixes, **security fixes
included**, and pacman reports an ordinary successful upgrade. An override only changes mode and owner, so this does not
apply to it (but a setuid mode makes a program run with extra rights: also a question). emrac treats this as part of the
feature, not as a footnote:

- `divert add` says so and asks (default no; `--yes` does not answer it; `--allow-stale` is the explicit opt-in), with
  extra warnings for setuid/setgid files and for compiled programs and libraries; the root helper refuses without the
  consent as well;
- the package's version of the file when you diverted it (`base`) is recorded; when the package ships a different file,
  the diversion is **stale**. The hook says so in pacman's own output after the transaction ("the package changed this
  file and YOUR old version is back ... NOT in use"), every time;
- `divert list` and `policy status` mark it `STALE`; `doctor` has a row; plans show it beforehand (exact when the new
  package is in the cache, otherwise "maybe") and warn by name when the package changes a file you diverted;
- `emrac divert diff` shows what you are missing; `divert reviewed` records that you looked and keep yours (it warns again
  only if the package changes the file again); `divert remove` lets go;
- a diversion nobody reviews is exactly what `doctor` is for: check them from time to time.

## Not covered

Diverting a directory or a symlink, overrides by glob, ACLs and extended attributes, `NoExtract` interplay, merging the
package's change into your file for you (`divert diff` only shows it), and an expiry on diversions.

## Verification

Unit tests for validation, the three-hash decision, the apply executor (real files; chmod/chown to oneself), the
passwd/group lookup, the hook text, the trust check; `dev/e2e/policy.sh` against real pacman as root: an override
survives an upgrade and a reinstall, a diversion survives an upgrade with and without upstream changes, a later
edit is kept, removal puts your copy back, the refusals, the untrusted-executable refusal, the plan preview, `check`.

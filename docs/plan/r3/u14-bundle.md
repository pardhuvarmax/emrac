# Slice u14 (after r3) — `emrac bundle`: offline bundles for a machine with no network

`ideas.md`/`feats.md`: "Offline bundles: `bundle create | apply`". A machine that never touches the network (or only
sometimes) can still be installed on and upgraded, from a folder carried on a drive, with the same checks as any other
transaction.

```
# On the machine with the network, for a target that wrote `emrac export --packages -o target.toml`:
emrac bundle create /mnt/usb/bundle firefox --for target.toml     # firefox and what the target lacks of its dependencies
emrac bundle create /mnt/usb/bundle --for target.toml             # every upgrade the target is waiting for, and what they need

# Without --for: the whole dependency closure of the named packages (nothing assumed about the target)
emrac bundle create /mnt/usb/bundle firefox

# On the target (no network needed):
emrac bundle verify /mnt/usb/bundle
emrac bundle apply  /mnt/usb/bundle
```

## What a bundle is

A **directory** (tar it yourself if you like):

```
bundle/manifest.toml          what is in it, with a SHA-256 for every file
bundle/pkg/<name>-<ver>-<arch>.pkg.tar.zst
bundle/pkg/<name>-<ver>-<arch>.pkg.tar.zst.sig
```

The packages are the repositories' own files and their detached signatures, downloaded by `pacman -Sw --noconfirm` into the bundle's
folder (so pacman's own checks apply while downloading; the files are root's inside a folder that is yours, so you can copy and delete them). `create` refuses a folder that already exists.

## Two kinds of check, because a drive is not trusted

- **The manifest's hashes** catch a damaged or half-copied bundle (`bundle verify`, and again at `apply`).
- **The signatures** are what makes a package trustworthy: a hash in a file next to the package proves nothing against
  someone who can change both. `apply` installs with `pacman -U` under a temporary copy of `pacman.conf` that sets
  `LocalFileSigLevel = Required`, so pacman verifies every package against the machine's own keyring and **refuses an unsigned or
  wrongly signed file**, whatever the manifest says. (pacman's default for local files is `Optional`, which would accept an
  unsigned file.) A package without a `.sig` file in the bundle is rejected before pacman is even asked.

## `apply` goes through the normal path

Verify, compare with what is installed (install, upgrade, already there and skipped, **older than installed and refused**,
never a downgrade), the pre-flight (network checks skipped, disk space and the kernel checks kept), the plan shown, a question
(`--yes` answers it), then the transaction is journaled and recorded in the history like any other. The bundle also carries
no sync databases, so pacman's own database on the target is not refreshed: the target's next online `-Sy` does that.

## What `--for` does

`--for FILE` takes the exchange file of the target (`emrac export --packages`, u12). With it, a package is put in the bundle only
if the target has no copy of it or an older one; with no package names it means "every upgrade the target is waiting for".
Without `--for` an upgrade bundle cannot be made (the creator does not know what the target has), so naming no packages is
refused.

## Limits, said plainly

- **Official repository packages only.** An AUR package in the plan is listed as "not included" with the reason: it has to
  be built and its PKGBUILD reviewed, which is not something to do blindly from a drive.
- The bundle is as fresh as the creator's databases; a package replaced in the repositories since is not in it.
- The architecture must match (`apply` refuses a bundle for another one).
- A dependency cycle or a name no repository carries is reported at `create` and the bundle is not made.

## A machine that has been offline for months: the keyring

`apply` makes pacman check every signature against the target's own keyring. Arch rotates its packagers' keys, so a machine that
has been offline for a long time may not know the key a new package was signed with, and pacman stops with "invalid or corrupted
package (PGP signature)" although the bundle is genuine. The keyring is itself a package (`archlinux-keyring`), but pacman checks
**all** signatures **before** it installs anything, so a newer keyring in the same bundle cannot help the other packages. The way
through is two bundles: `emrac bundle create DIR archlinux-keyring`, applied first (its install script loads the new keys), then the real
one. If even the keyring bundle is refused the machine is too far behind to trust it, and emrac does not turn the check off:
bring the keyring in another way you trust. This is written in the long help of `bundle`, `bundle create` and `bundle apply`, and
`apply` repeats the hint when pacman fails. It was not tested against a really old keyring (that needs an old image); the
container suite checks that the help says it and that a keyring-only bundle can be made.

## Design

`emrac-core/src/bundle.rs`: `Manifest`, `BundlePackage`, `render` / `parse` (every name and file name validated; no path
separator, no `..`, a file must start with `<name>-<version>-`), `needed` (what the target lacks, by version), `verify_dir`
(sizes, hashes streamed, signature present, nothing unlisted, no symlinks), `compare` (the install / upgrade / same / older
decision). `backend/alpm.rs`: `sync_closure`. `backend/pacman.rs`: `download_packages` and `install_files_signed`.
`commands/bundle.rs` prints and asks.

## Verification

Unit tests for the manifest (round trip, hostile names and files), `needed`, `verify_dir` against real files (a changed byte, a
missing file, a missing signature, an extra file, a symlink), `compare`, and the temporary config. A container suite
`dev/e2e/bundle.sh`: create a bundle of a real package, verify, damage it and see `verify` fail, apply on a state without
the package, `--for` leaving out what the target already has, an unsigned file refused by pacman itself, a downgrade bundle
refused.

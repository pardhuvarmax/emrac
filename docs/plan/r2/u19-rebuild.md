# Slice u19 — `emrac rebuild` and finding what needs it

From a real case on the developer's machine and `additions.md` A14. After a Qt update the
`quickshell-check` pacman hook said Quickshell, built against Qt 6.11.2, had to be rebuilt for Qt
6.12.0. emrac had nothing for it: `reinstall` puts back the same package file (the old build),
`install` of an installed AUR package plans a different, stale version, and a `-git` package never
looks outdated to `upgrade`.

## `emrac rebuild <pkg>...`

Builds installed AUR (foreign) packages again from source and installs the result.
Official packages and names that aren't installed are refused with what to use instead. It goes
through the same steps as `upgrade` for an AUR package: the sources are fetched again, audited,
and shown for review (`--skip-pkgbuild`, `--skip-diff`, `--accept-risks`, `--no-sandbox`,
`--allow-network` as elsewhere), then one confirmation, the pre-flight checks, the plan id
re-check, the journal, the sandboxed build with **every makepkg phase forced** (makepkg refuses to
start while a package file for the version exists), the manifest check of the built package and
`sudo pacman -U`. The history records the versions actually installed afterwards, as an upgrade if
they changed (a `-git` package gets a new version) and as a reinstall if not.

## `emrac rebuild --check`, `--stale`

`--check` lists the installed AUR packages that need attention and why; nothing is changed. Two
levels:

- **broken now**: a program or library in a system folder (`usr/bin`, `usr/lib`, `usr/lib32`)
  needs a shared library that nothing provides (the ELF headers are read directly, no program is
  run; the package's own files, `RUNPATH` and `$ORIGIN` are honoured), or the package installs into
  the folder of another Python or Perl version than the one now installed;
- **may break**: a bundled program or plugin (deeper folders, `/opt`) needs a missing library
  (often optional), or the package depends on Qt and Qt's minor version changed in pacman's log
  after the package was built (`qt6-base 6.11 → 6.12`, the Quickshell case).

`--stale` builds again the ones that are broken now. `doctor` has a row for each level; after an
`upgrade` that changed official packages, a line names the AUR packages the new libraries broke.

## Binary packages

A `-bin`, `-appimage` or `-prebuilt` package repackages a vendor's finished program: rebuilding it
downloads the same files and fixes nothing. So everywhere a rebuild could be expected to help,
this is said: "rebuild is intended for source packages only: rebuilding a binary package (-bin)
won't fix anything... see `emrac doctor` or the package maintainer's news". `--check` marks them
and leaves them out of the suggested command, `--stale` leaves them out and says so, naming one
explicitly warns first, and `doctor` lists them in a row of their own with no fix. (Asked for by
the developer after the first version listed `cloudflare-warp-bin` as something to rebuild.)

## Not done

- A package that was broken by a change that leaves no trace in a library name, a path or the
  log (a protocol change, a plugin API) is not found; the `may break` level only covers Qt.
- No VCS check: whether a `-git` package's upstream moved on. (`rebuild` handles the case by
  building the current HEAD.)

## Verification

Unit tests: the ELF reader against `readelf` on a real binary; the missing-library rule
(system, own files, `RUNPATH`, `$ORIGIN`, a needed name that is a path); the system and bundled
folders; interpreter folders; log time and upgrade parsing; the Qt rule including patch-level bumps
and a package built after the bump; binary package names; the doctor rows. Container
(`dev/final-check.sh`): `rebuild` builds an installed AUR package again (the build date changes)
and records it; official and missing names are refused; `--check` is quiet for a healthy package.
The container run found that makepkg refuses even the source-fetch phase while a package file
exists. Read-only on the developer's machine: `--check` finds Quickshell as "may break", with the
Qt versions and the date, which is what the pacman hook had said.

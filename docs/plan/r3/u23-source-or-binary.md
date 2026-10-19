# Slice u23 (after r3) — source or binary on install

**Status: built.**

Until now the choice was made by the name: an official package was the repository's binary, an AUR package was built from source, and `recommend` only advised. This slice makes the choice yours, and lets the setting or `recommend` make it when you do not.

## What it does

- **`emrac install --source <pkg>`**: an official package is built from **Arch's packaging source** here. The packaging repository (`gitlab.archlinux.org/archlinux/packaging/packages/<base>`) is fetched at the tag of the repository's version, which is what the binary was built from; the same audit as for an AUR PKGBUILD reads it first (critical is refused, high asks), it is built in the sandbox with the limits and the build screen of u22, the built package is inspected (setuid files, hooks, scriptlets, ...) and only then installed with `pacman -U`. Only the package asked for is installed when the base builds several. A version mismatch between the build and the repository is said, not hidden.
- **`emrac install --binary <pkg>`**: nothing is compiled. A package that exists only as source (the AUR's) is refused, with its prebuilt `-bin` counterpart named when the AUR has one.
- **`install.prefer`** (`[install]`): `repository` (the default, as before) or `recommend`, which builds an official package from source where `emrac recommend` says source with medium or high confidence. `--source` and `--binary` win over it.
- The plan says which are built here (`extra+source/sl`, and a warning that explains it: the sandbox, the build tools `makepkg` installs, and that the next upgrade replaces the result with the repository's binary unless pinned). The choice is part of the plan id, so `--expect-plan` binds it.

## Decisions

- Arch's packaging repositories are fetched over https only, one tag, shallow, a clean checkout every time; the name that goes into the address is checked.
- The default does not change: a build is never started because of a score unless you set `install.prefer = recommend`.
- `upgrade` is unchanged: it replaces a source-built package with the repository's binary at the next upgrade, as the plan says. Keeping it built from source is what a pin is for (or `install --source` again).

## Not covered

`upgrade --source`; building a repository package with other flags than the user's `makepkg.conf` (that is Build Profiles, u28); `reinstall --source`; official packages whose tag is missing from the packaging repository (an error says so).

## Verification

Unit tests of the choice (flag, setting, verdict, confidence), of the plan entry, of the built file's package name (split packages), of the tag for an epoch and of the name allowed in an address; `dev/e2e/sourceinstall.sh` (12 checks, in the container, with a real build of `sl` from the packaging source in the sandbox, the installed package shown to be built here and not the repository's file).

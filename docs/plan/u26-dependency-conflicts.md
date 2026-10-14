# Slice u26 — explain dependency conflicts that stop an AUR build

Found on a real upgrade. The installed `mangowm-git` depends on the package
*name* `scenefx0.5` (an old AUR package). The new `mangowm-git` depends on
`scenefx` from `extra`, which is now 0.5-1 and provides the same
`libscenefx-0.5.so`. The two conflict. makepkg's dependency step asked pacman to
install `scenefx`; pacman asked "Remove scenefx0.5?", then refused because the
old `mangowm-git` still requires that name. emrac reported only
"the build failed in the dependencies and sources step (exit status: 8)".

The fix by hand (`pacman -Rdd scenefx0.5 && pacman -S scenefx`) was safe only
because the replacement provides the same library. emrac should say that
itself, and must not do it silently.

## 1. Diagnose the failure

When the host dependency step fails, emrac reads what stopped it. If pacman
reported "removing X breaks dependency 'X' required by Y", emrac says, in its own
words: which package would be removed, which installed package requires it, which
package needs the conflicting one, and whether the replacement provides everything
the removed package provided (the sonames and names from the sync and local
databases, via libalpm — not by parsing pacman's text for anything but the names).

## 2. Warn in the plan

The u24/u25 simulation already runs for repository packages. For each AUR package
to build, it is also run for the new PKGBUILD's missing dependencies (from the
checked-out `.SRCINFO`/PKGBUILD, as `audit` already reads them). A dependency that
would remove an installed package other things still require is a plan warning
(`dependency-conflict`), shown before anything is built. If the simulation cannot
run (a dependency is itself an uninstalled AUR package), the plan says so.

## 3. Offer the fix, never apply it silently

emrac prints the command it would run and why it is, or is not, likely to be safe:

- **likely safe**: the replacement provides every name and soname the removed
  package provided. The message says the dependent package still works, since the
  library is the same, until it is upgraded itself.
- **unclear**: it does not. The message says what is missing and leaves the
  decision to the user.

The offer is a default-no prompt that `--yes` does not answer, and it needs the
replacement to be an official-repo package. Declining leaves the system untouched
and the package is skipped, with the command shown for the user to run themselves.
Under `--json` nothing is prompted: the finding and the suggested command are in
the output.

## Not covered

- Applying the fix across several packages at once.
- Proving that the files of the old and new package are the same. A matching
  `provides` is a strong hint, not proof; the message says so.
- Providers that arise only at `pacman -U` of a built package (still open from u25).

## Verification

Container: the same shape as the real case. A local repository holds `libnew`
(provides `libx.so=1-64`, conflicts `libold`) and an installed `libold` with an
installed `app` that depends on the name `libold`. A second AUR-style package needs
`libnew`. The plan must warn, the build must fail with the diagnosis rather than a
bare exit status, and the suggested command must succeed and let the rebuild run.
Unit tests: the plain-language message, "likely safe" versus "unclear", and
`--json` carrying the finding without prompting.

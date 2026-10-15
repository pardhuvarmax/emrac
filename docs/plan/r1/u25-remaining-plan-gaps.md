# Slice u25 — close the three gaps left by u24

u24 made plans list what conflict resolution will remove, and named three things
it did not cover. Each is covered here, with a real test.

## 1. Provider choices

When a dependency can be met by several packages (`jack` → `jack2` or
`pipewire-jack`), pacman asks "Enter a number (default=1)". With `--yes` it
silently takes the first. The plan now says which dependencies have a choice,
who the candidates are, and which one pacman takes by default. Same simulation as
u24: the question callback records the dependency and the providers (and still
answers with the default, so the rest of the plan is what the default produces).

## 2. Replacement removals

Code written but never seen firing. A real `replaces` case is built for the test:
a local repository holds `newpkg` with `replaces=(oldpkg)`, `oldpkg` is installed,
and `emrac upgrade --dry-run` must show `oldpkg — replaced by newpkg` and match
what the real upgrade then does.

## 3. AUR (built) packages

An AUR package's conflicts are only known once it is built. After the build the
**built package file** is loaded into the same libalpm simulation as an install
(`pacman -U`), so the manifest can say, before anything is installed, which
installed packages it would remove (`conflict-removal`, a warning) — in
`emrac audit <file.pkg.tar.*>` too. If the simulation can't run (typically
because a runtime dependency is itself an AUR package that isn't installed yet),
the manifest says so rather than staying silent.

## Verification

Container: a package with a provider choice (`jack`); the `replaces` repository
above, plan vs. real upgrade; a locally built package that `conflicts` with an
installed one, audited as a file and then installed through `emrac build`'s
manifest path.

# Build tools left behind by AUR builds

The plan is [`../plan/r3/u7-build-tools.md`](../plan/r3/u7-build-tools.md); the code is
`crates/emrac-core/src/build_tools.rs` and `crates/emrac-cli/src/commands/aur_install.rs`.

`makepkg -s` installs an AUR package's `makedepends` and `checkdepends` (and what they need) as plain dependencies
and leaves them. emrac records the installed packages before an AUR build and, when the run is over, says what the
build added:

```
emrac notes: 2 packages were installed only to build 'pfetch', and nothing needs them now: cmake, libuv (18.2 MiB).
Remove them now? [y/N]
```

## What counts

A package qualifies only if all hold:

- it was **not installed** before the first build of the run;
- it was installed **as a dependency** (install reason), so something you installed on purpose is never offered;
- every installed package that requires it or lists it as an optional dependency is **also being offered**. This is
  a fixpoint: `cmake` has no dependents and goes; `libuv` is needed only by `cmake`, so it goes with it; `python` is
  needed by `cmake` and by the package just built, so it stays. The package just built is installed by then, so what
  it needs is protected without a special case.

If the installed set before the build cannot be read, nothing is offered (never "everything").

## The setting

`build.remove_make_deps`:

- `ask` (default): list, size, ask (default no). `--yes` does not answer the question: it prints the list and
  `emrac remove ...` instead.
- `always`: remove without asking.
- `never`: say nothing.

The removal is an ordinary `emrac remove`, so it shows a plan, runs pre-flight, is journaled and recorded, and
`emrac undo` puts the tools back.

## Why at the end of the run

A first version offered right after each install and was refused: the running command is itself a journaled
transaction and the journal allows one at a time ("another emrac is changing packages right now"). The offer is
now made once, after the command has finished everything it changed; several builds in one run share the earliest
snapshot and give one list. The removal also needs a fresh libalpm handle, because the handle used by the run read
pacman's database before the build and does not know what the build installed.

## Not covered

`emrac build` (it installs nothing itself), a build tool you want to keep for the next build (use `never` or answer
no; the next build installs it again), and dependencies pacman pulls for official packages.

## Checked against pacman

`dev/e2e/buildtools.sh` builds a local checkout posing as an AUR package (`makedepends=(figlet)`,
`depends=(tree)`) in all three modes: `never` says nothing, `ask --yes` lists and removes nothing, `always` removes
figlet and keeps the package and `tree`, the removal is in the history and undoable, and a package that was
installed before the build is not touched. 9 checks.

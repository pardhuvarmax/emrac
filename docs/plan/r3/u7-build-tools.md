# Slice u7 (after r3) — remove the tools that were only installed to build

`makepkg -s` installs an AUR package's build dependencies (`makedepends`, `checkdepends`, and what they need)
as ordinary dependencies and never removes them: after a few builds the system carries compilers, `cmake`,
`rust`, `python-setuptools` that nothing runs. `makepkg -r` would remove them, but it also removes runtime
dependencies it installed and cannot be used with emrac's sandboxed two-step build. `pacman -Rs` after the fact
cannot tell which dependencies were installed for the build.

Idea source: Alpine's `apk add --virtual .build-deps` groups packages added for a build so one command removes
them (`research/` notes on R6 in `docs/rea-research-plan.md`); Debian's `build-essential`/`apt-mark auto` and
Nix's `nix-shell` are variations. Native version here: emrac knows what was installed before the build, so it
can name exactly what the build added.

## Design

- Before the build, `Sources::installed_names_set()`; after `pacman -U` succeeded, `Sources::build_tools_since(before)`
  returns the packages that appeared since, were installed **as dependencies** (install reason), and are not needed
  by anything outside that set: libalpm's `required_by` and `optional_for` of each, then a fixpoint (`cmake` goes, then
  the `libuv` only it needed; `python` stays if the built package, now installed, requires it). The built package
  itself, anything you installed explicitly, and anything that was already there are never candidates.
- If the set of installed names cannot be read before the build, nothing is proposed (never "everything").
- The offer is made **at the end of the run**, not right after each install: the command is itself a journaled
  transaction and the journal allows one at a time (found when the first version, a removal started from inside
  `build_and_install`, was refused with "another emrac is changing packages right now"). Several builds in one run
  share the earliest snapshot and give one list. The removal also uses a fresh `Sources`: the one the run used read
  pacman's database before the build and did not know the build tools.
- `build.remove_make_deps` = `ask` (default) | `always` | `never`. `ask` shows the list with its size and asks (default
  no); `--yes` never answers it, it prints the list and the command. `always` removes without asking; `never` says
  nothing. The removal is an ordinary `emrac remove` (plan, pre-flight, journal, history), so it can be undone with
  `emrac undo`.
- Works for `install`, `upgrade`, `rebuild` and `reinstall`: all go through `build_and_install`.

## Not covered

`emrac build` (it does not install), dependencies pacman itself pulled for an official package, and tools you want
to keep for the next build (that is what `never` and the prompt are for).

## Verification

Unit tests for the fixpoint and the config key (`build_tools.rs` 6, `config.rs` 1); `dev/e2e/buildtools.sh` (9 checks): a local AUR-like package built by makepkg with a makedepend
that is not installed beforehand: it is removed afterwards, the runtime dependency stays, an explicitly installed
package stays, `never` and `--yes` do nothing.

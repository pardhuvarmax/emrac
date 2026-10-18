# Slice u4 (after r3) — hook preview: which pacman hooks a plan will run

pacman runs hooks (`/usr/share/libalpm/hooks`, `/etc/pacman.d/hooks`) as root before and after a
transaction and shows nothing about them until they run. A failing `AbortOnFail` hook cancels the
transaction after the user has confirmed; a hook whose `Depends` is missing fails the same way.
`install`, `remove` and `upgrade` plans now list the hooks that will run, in the order pacman runs
them, so the surprise comes before the confirmation.

Idea source: no package manager previews its hooks; dpkg's `apt-listchanges` and `needrestart` show
similar "what else will happen" lists. The matching rules were taken from `alpm-hooks(5)` and then
confirmed by running real hooks in a container (`research/alpm-hooks.md`, `research/pacman-commit-order.md`).
Nothing was copied from libalpm's source.

```
Hooks that will run (12):
  before the transaction
    1. 05-dkms-check          "Checking DKMS modules"                 AbortOnFail
  after the transaction
    1. 11-glibc-ldconfig      "Updating the dynamic linker cache…"    (usr/lib/libfoo.so)
    2. 30-systemd-daemon-reload-system  "Reloading system manager configuration…"
    ...
  ! 05-dkms-check needs dkms, which is not installed: pacman would stop here.
```

## Rules (all confirmed against real pacman; see `research/alpm-hooks.md`)

- **Directories:** `pacman-conf HookDir` entries plus the system directory
  `/usr/share/libalpm/hooks`. A file name present in a custom directory replaces the system one
  (observed: the custom `50-prec.hook` ran, the system one did not). A hook is a file ending in
  `.hook`; other files and directories are skipped. A symlink to `/dev/null` disables a hook.
- **Order:** by file name with the `.hook` suffix removed, byte-wise (`10-A` < `10-a` <
  `10-a-b` < `10-a_b`, and `9-x` sorts after `65-…`). Pre and post hooks are separate passes.
- **Trigger:** matches if **any** `[Trigger]` section matches. Within one, the `Operation` list, a
  `Type` of `Package` or `Path` (`File` is an alias), and `Target`s. Targets are evaluated in order
  and the last one that matches decides; a leading `!` makes a match mean "no". A trigger whose
  only matches are negations does not fire.
- **Glob:** `*`, `?` and `[...]`; `*` also matches `/` (`usr/share/*.txt` matches
  `usr/share/tp/a.txt`). Path targets have no leading `/`; directories appear with a trailing `/`.
- **Package trigger:** operation Install if the package is not installed, Upgrade if it is, Remove
  for removals. Matches on the package name.
- **Path trigger:** looks at the files of every package in the transaction. For a new or upgraded
  package a file is an Install when it was not in the old version and not on disk, otherwise an
  Upgrade; for a removed package every file it owned is a Remove. Pre-transaction hooks see the
  new packages' files too.
- **Hooks a package brings:** pacman reads post-transaction hooks again after extraction, so a hook a
  package puts in `/usr/share/libalpm/hooks` runs in the same transaction (seen: `perl` brings
  `detect-old-perl-modules.hook`). Its content is inside the package, so the preview lists it by name
  (`shipped`) and does not claim to know what it does.
- **Consequences shown:** `AbortOnFail` (pre only), `NeedsTargets`, `Depends` not satisfied by any
  installed package (and not in this transaction).

## Where the data comes from

- Hooks: read from disk with the rules above (pure parsing in a new `hooks.rs`).
- Package files: removals and the old version of an upgrade come from the local database. The files
  of a package that is about to be installed come from libalpm's sync file lists, which exist only
  after `pacman -Fy`, read through a second handle whose database extension is `.files` (as `pacman -F`
  does); with no list the preview says so (`path triggers could not be checked for
  <pkg>: no file list`) and shows only the hooks that need nothing else. It never guesses a match.

## Files

- `crates/emrac-core/src/hooks.rs` (new): `Hook`, `parse_hook`, `load_hooks`, glob matching,
  `plan_hooks(&[Hook], &Facts) -> HookPreview`. Pure except `load_hooks`.
- `crates/emrac-core/src/backend/alpm.rs`: `AlpmBackend::hook_facts(&Plan)` builds `Facts` from
  libalpm (installed or not, file lists).
- `crates/emrac-core/src/sources.rs`: `Sources::hook_preview(&Plan) -> HookPreview`.
- `crates/emrac-cli/src/output.rs`: `print_plan` shows the preview; `--json` gets a `hooks` key.
- `install`, `remove`, `upgrade` call it. `Plan` and its digest are unchanged: the hooks depend on
  the system, not on the plan, so they must not change a plan id.
- Docs: `docs/SPEC.md` is left alone; `FEATURE_STATUS.rst`, `CHANGELOG.md`, `docs/tests/…`.

## Verification

- Unit tests in `hooks.rs` for each rule above (parsing, the sample hooks from the container run,
  override, order, negation, install vs upgrade, missing `Depends`).
- Container: the same probe hooks as the research run; the list emrac prints must equal the lines
  pacman prints for pre and post hooks, in the same order, for an install, an upgrade and a remove.
- Not covered by this slice: hooks that run for files removed by an upgrade (not observed), hook
  output, running anything.

## What was built (result)

`hooks.rs` (14 unit tests), `AlpmBackend::{hook_dirs, hook_facts, hook_dependency_met, root_has}`,
`Sources::hook_preview`, `output::print_plan_with_preview (u5 widened it from hooks to hooks and config)`, `dev/e2e/preview.sh` (hook checks: 10 of 10 passed:
install, remove and a full upgrade each match what pacman ran, in order). The only difference seen in
the first comparison was the hook `perl` brings, which is why `shipped` exists.

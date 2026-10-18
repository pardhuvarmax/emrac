# Hook preview: what pacman will run

For people who use emrac and for whoever changes the rules. The plan is
[`../plan/r3/u4-hook-preview.md`](../plan/r3/u4-hook-preview.md); the code is
`crates/emrac-core/src/hooks.rs`; the research behind the rules is in `research/alpm-hooks.md` and
`research/pacman-commit-order.md`.

pacman runs hooks as root before and after a transaction and says nothing about them until they run.
A hook that fails with `AbortOnFail`, or whose `Depends` is not installed, cancels the transaction
**after** the user confirmed it. `install`, `remove` and `upgrade` therefore list the hooks of the plan
under it:

```
Hooks pacman will run (9):
  before 10-upg-pre  Checking something first
  after  11-glibc-ldconfig  Updating the dynamic linker cache...
  ...
emrac warns: hook 05-dkms-check.hook needs dkms, which is not installed: pacman will stop the transaction
emrac notes: packages in this plan bring hooks of their own, which also run afterwards: x.hook (pkg)
```

`--json` adds a `hooks` object (`hooks`, `unchecked`, `unusable`, `shipped`). The hooks are **not** part of
the plan or its id: they depend on the system, not on what the plan does.

## What is checked

The same rules pacman uses, confirmed by running probe hooks next to the system ones:

- hook directories: `/usr/share/libalpm/hooks`, then the `HookDir` entries of `pacman.conf`; the same file
  name in a later directory replaces the earlier one, a symlink to `/dev/null` disables it;
- order: by file name without `.hook`, byte by byte;
- a hook runs if any of its triggers matches; targets are globs where `*` also matches `/`, a leading `!`
  negates, the last matching target decides;
- package triggers by name and operation; path triggers by the files of the packages in the transaction
  (an install when the file was not in the old version and is not on disk, otherwise an upgrade; every file
  of a removed package is a removal);
- `AbortOnFail`, `NeedsTargets` and `Depends` are read; `Depends` is checked against what is installed
  and what comes with the plan.

## What it cannot know, and says so

- **File lists of packages not yet installed** come from the repositories' `.files` databases
  (`pacman -Fy`). Without them a hook that watches files cannot be checked for those packages; the
  output names them (`unchecked`) and never guesses.
- **Hooks a package brings** are read by pacman after the files are extracted, so they run in the same
  transaction. Their content is inside the package; they are listed by name (`shipped`).
- **AUR packages** are built first, so their files are unknown until then (also `unchecked`).
- A hook pacman would refuse (`unusable`) is shown with the reason. Hooks run for files an upgrade removes
  were not observed and are not predicted.

## Checked against pacman

`dev/e2e/preview.sh` (`dev/e2e/run.sh preview`) puts probe hooks beside the system ones and compares the hooks
emrac predicts, per pass and in order, with what pacman prints for a real install, removal and full
upgrade. Last run: 22 of 22 (the same script checks the config files, see [`config-preview.md`](./config-preview.md)).

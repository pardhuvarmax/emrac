# Local file policy: `override` and `divert`

The plan is [`../plan/r3/u8-override-divert.md`](../plan/r3/u8-override-divert.md); the code is
`crates/emrac-core/src/policy.rs` (rules), `policy_ops.rs` (what the root helper does), `policy_preview.rs` (plans) and
`crates/emrac-cli/src/commands/policy.rs`.

pacman puts a package's files back exactly as shipped on **every upgrade and every reinstall**. Two things you may want to
keep are lost without a word: the mode and owner of a packaged file, and your own version of a packaged file that is not a
`backup=()` config file (those have `.pacnew`). This keeps them, by a pacman hook that re-applies the policy.

```
emrac override set /usr/bin/helper --mode 4750 --owner root --group wheel
emrac override list | remove
emrac divert add /usr/lib/libfoo.so.1        # asks first, see below
emrac divert list | diff | reviewed | remove
emrac policy status | apply
```

## What it keeps

- **Override:** mode, owner and group of a file or directory. After each transaction that installs, upgrades or reinstalls
  the owning package, the hook sets them back through an open descriptor (never following a link). A missing file or a link
  is reported, not touched.
- **Divert:** your version of a regular file a package owns. Three sha256 sums decide after a transaction: the package's
  (from pacman's own `mtree`), the file now, and your stored copy.

| the file now is | meaning | emrac does |
|---|---|---|
| your copy | nothing happened | nothing |
| the package's | pacman just overwrote yours | sets the package's aside, puts yours back (with your mode and owner, written beside it and renamed, never half written) |
| neither | you edited it since | updates your stored copy from it |
| gone, package still installed | someone deleted it | puts your copy back |
| gone, package removed | the package is not installed | **nothing is put back**: your copy waits in the store |

A pre-transaction hook saves an edit before pacman can overwrite or remove the file, because afterwards it is gone. Putting a
file back after its package is removed would make the package impossible to install again (the file it ships would already
exist), which the container run showed; so it returns on the next install instead.

Files: `/etc/emrac/policy.toml` (root, 0644, readable and hand-editable), `/var/lib/emrac/divert/<id>/local` (your copy, in a
0700 directory, keeping its own mode and owner) and `.../package` (the package's version an upgrade set aside), the hooks
`zz-emrac-policy.hook` and `zz-emrac-policy-pre.hook` in the last `HookDir`.

## The risk: a diversion freezes the file

A diverted file keeps your version through every upgrade, so it **stops getting that package's fixes, security fixes
included**, and pacman reports a normal successful upgrade. An override only changes mode and owner, so this does not apply to
it (a setuid mode is a different risk: the program runs with extra rights, and that is also a question). What is done:

- **Consent.** `divert add` states it and asks (default no; `--yes` does not answer it; `--allow-stale` is the explicit
  opt-in). Extra warnings for setuid/setgid files and for compiled programs and libraries, the kinds of file security fixes
  are shipped in. The root helper refuses without the consent too.
- **A base.** The package's checksum and version when you diverted it are recorded. When the package ships a different file,
  the diversion is **stale**.
- **It does not stay quiet.** After a transaction that changes it, pacman's own output says: the package changed this file,
  your old version is back, the package's changes are NOT in use. `divert list` and `policy status` mark `STALE`; `doctor`
  has a row; plans warn by name beforehand (exact when the new package is cached, otherwise "may").
- **A way out.** `divert diff` shows what you are missing; merge it, then `divert reviewed` (warn again only if the package
  changes the file again), or `divert remove`.

## Root runs a hook: the guards

- **Trust of the executable.** The hook is written only for `/usr/bin/emrac` or `/usr/local/bin/emrac`, owned by root and
  writable by no one else, with root-owned, unwritable parent directories. A copy anyone can write would be a way to run
  their code as root, and is refused.
- **Location.** Root acts on a policy file only if every directory from `/` down to it is owned by root and closed to others,
  checked when the policy is written and at every apply; otherwise a swapped directory or link could redirect a root write.
- **Arguments.** User commands change nothing themselves; they run the hidden `emrac policy-helper <verb>` through `sudo`
  (or directly as root), which validates every argument again. Paths must be absolute and normal, not under `/proc /sys /dev
  /run`, with no `* ? [ ] \` or control characters (a hook would read them as patterns); modes are 3 or 4 octal digits.
- **Refusals for a diversion:** a file no package owns, a `backup=` file (use `NoUpgrade` or `.pacnew`), a link or
  directory, a file whose package recorded no checksum.

## In emrac

`check`/`verify` treat an overridden mode and a diverted file's content as expected ("by your policy"); plans list the policy
they touch and warn when a diverted file is about to be changed by the package; `doctor` warns about stale diversions and a
policy that is not in effect or whose hook is missing. All of it is read-only.

## Not covered

Diverting directories or links, overrides by glob, ACLs and extended attributes, merging the package's change into your file
(`divert diff` shows it), expiry of a diversion.

## Checked against pacman

`dev/e2e/policy.sh`: a small package upgraded, reinstalled and removed by real pacman with the policy set (47 checks).

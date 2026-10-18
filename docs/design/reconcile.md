# After a failed transaction: a package pacman lists but is not whole

The plan is [`../plan/r3/u6-reconcile.md`](../plan/r3/u6-reconcile.md); the code is
`crates/emrac-core/src/reconcile.rs`.

## What was found

Failing pacman on purpose (a file of the package made to fail with "no space left on device", strace fault
injection, `research/pacman-commit-order.md`): pacman printed `could not extract ...`, `problem occurred while
installing`, **ran the package's `post_install` script**, ended with `failed to commit transaction` and `Errors
occurred, no packages were upgraded`, and exited 1. Afterwards `pacman -Q` listed the package and its database
entry (`desc`, `files`, `mtree`) was complete, while the file was not on disk. Nothing in pacman's output says so.
A package pacman kills halfway through a removal is the mirror image: files gone, entry still there.

## What emrac does

After any transaction that returns an error, and after `history recover` puts a cut-off one on record, emrac looks
at the packages **that transaction named**, compares the files pacman records for each with the disk, and for each
package with files missing says:

```
emrac warns: pacman lists phantom 1-1 as installed, but 1 of its 2 files is missing (usr/share/phantom/a.txt)
emrac suggests: to mend it: emrac reinstall phantom
```

For a removal: `pacman still lists X although N of its M files are already gone: the removal did not finish`, fix
`emrac remove X`. Directories, `backup=()` config files (a missing one may be deliberate) and anything `NoExtract`
leaves out are not counted. A symlink counts as present even if it dangles. A successful transaction is not
examined at all.

## What it does not do

- It does not repair anything by itself; it names the command (`reinstall` re-runs the install scripts too).
- It looks only at the packages of the failed plan, not at the whole system (`emrac check` does that).
- A file that exists but is truncated is not noticed (that is `emrac check`, which compares the mtree).
- It cannot tell a half-written *upgrade* from a half-written install beyond the missing files: an upgrade that
  died after deleting the old files and before writing the new ones shows up the same way, which is right.

## Checked against pacman

`dev/e2e/recovery.sh` fails a real `pacman -S` with an injected ENOSPC and checks: pacman exits 1 and still lists
the package, emrac names the missing file and the fix, `emrac check` agrees, the fix mends it, and a transaction
that worked says nothing. 9 checks.

# Slice u25 (after r3) — `emrac strays` and `emrac linkcheck`

`additions.md` A9 (stray file scanner) and B8 (link-check for software built outside the package manager). The
two belong together: the stray scanner finds what no package owns; the link-check asks what that would lose when
a package changes.

```
emrac strays                      # files under /usr and /opt that no package owns, by what they look like
emrac strays --local --all        # also /usr/local, and the files tools generate
emrac strays --path usr/share     # one place
emrac linkcheck                   # programs nobody packaged that cannot find a library now
emrac linkcheck --upgrade         # ... that would lose one to the upgrade that is waiting
emrac upgrade / remove --dry-run  # the same lines appear under the plan, with the package change that causes them
```

## `strays`

Ownership is pacman's own record, the `files` list of every package under `/var/lib/pacman/local`. The walk goes
through `/usr` and `/opt` without following symlinks; an owned directory is entered, an unowned one is a single
stray with its file count, size and newest modification, and is not entered further.

Not strays (left out unless `--all`): the files hooks and tools create under package directories (`info/dir`,
`locale-archive`, MIME and icon caches, `gschemas.compiled`, `loaders.cache`, kernel module indexes under
`/usr/lib/modules`, byte-compiled Python). `/usr/local` is where manual installs belong, so it is shown only
with `--local`.

"Who put this here" is a clue read from where the file is, never a claim: a Python package whose `dist-info`
says it was installed by `pip`; a global npm package; a Ruby gem; a file inside a directory that exactly one
package owns (a directory every package shares, like `/usr/bin`, says nothing); `/usr/local`; otherwise nothing
says. Read only: nothing is removed or moved.

## `linkcheck`

Candidates are ELF files that no package owns: `/usr/local/{bin,sbin,lib,lib64,libexec}`, `~/.local/{bin,lib,lib64}`
and unowned files under `/opt`. Only the ELF headers are read (the same reader as the stale AUR build check); nothing
is run.

An upgrade or a removal changes a library when a package's installed version provides a soname
(`libfoo.so=1-64`, from the package's `provides`, which is how Arch records sonames) that the new version in the
synced databases does not, or when the package is removed or taken out by a conflict. The soname's file name is
`libfoo.so.1`. A candidate that needs that file, while no other installed package still provides it, is named in the
plan's warnings:

```
/usr/local/bin/usesfix is not from a package and needs libfixlib.so.1: fixlib 1-1 → 2-1 no longer provides libfixlib.so.1
```

At most five lines, then a count. The plan id is unchanged (warnings are not part of it). `linkcheck` without a flag
reports programs that cannot find a library now, using the system library directories and the program's own
search paths.

## Not here

Libraries a program loads at run time with `dlopen`, plugins, and interpreted programs (Python, Perl modules that
link a library) are not seen; a wrong answer in the other direction is possible when a library is provided under
another name. Packages whose files db is needed to compare file lists (`pacman -Fy`) are not used: the soname
provides are. Candidates in `~/.local` are only those of the user running emrac.

## Verification

Unit tests (`strays.rs` 3, `linkcheck.rs` 5). `dev/e2e/strays.sh` (16 checks): a real program compiled against a
library from a fixture package, installed in `/usr/local` and `/opt`; the package then upgraded to a version
that no longer provides that soname and removed in a dry run; the plan names the programs; after the upgrade the
program really fails and `linkcheck` says which library is gone. The strays are planted files of every kind above.

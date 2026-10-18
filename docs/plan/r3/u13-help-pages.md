# Slice u13 (after r3) — `man`, `docs`, `license`, `credits`, `support`, `feedback`, `bug-report`

`SPEC.md` Part V, "help / man / docs / license / credits / support / bug-report / feedback". `emrac help [command]` already
works (clap). The rest:

```
emrac man install              # the manual page of a command (a real man page when `man` is there and this is a terminal)
emrac man snapshot create      # a subcommand
emrac man emrac --roff > emrac.1   # the roff source, for a packager
emrac docs [--open]            # where the documentation is
emrac license [--full]         # the notice; the whole GPL text with --full
emrac credits                  # who made it and what it is built on
emrac support [--open]         # where to ask for help, and what to run first
emrac feedback [--open]        # where to send an idea or a complaint
emrac bug-report [--open]      # a scrubbed report to paste into an issue
```

## What each does

- **`man`**: renders the command's help as a man page with `clap_mangen`. On a terminal, with `man` installed, it is shown
  by `man -l -`; otherwise the long help is printed as plain text. `--roff` prints the roff source. An unknown command lists
  the known ones.
- **`docs`, `support`, `feedback`**: print addresses (the documentation site, the repository, its issue page) and what to run
  first (`emrac doctor`, `emrac bug-report`). `--open` opens the address with `xdg-open`; nothing is opened without it.
- **`license`**: GPL-3.0-or-later. The licence file is built into the program, so `--full` works offline.
- **`credits`**: the author, the licence, the repository, the programs it relies on at run time (pacman, libalpm, makepkg,
  bubblewrap, git) and the main libraries it is built with and their licences. A test keeps the list of libraries true to
  `Cargo.toml`.
- **`bug-report`**: the `doctor --report` markdown (home directory, user name and host name already scrubbed), the emrac
  version and the last three **failed** transactions with the first line of each error, all scrubbed again as one text. It
  is printed, **never sent**: you read it, then paste it into an issue. `--open` opens the new-issue page with only a
  title filled in (a body in a web address would be long and would leave the machine before you saw it).

## `man emrac` for real: `emrac man --generate DIR`

`emrac man --generate DIR` writes `emrac.1` and one page per command and subcommand (`emrac-install.1`, `emrac-snapshot-create.1`,
and so on; about 100 files) into `DIR`, with `clap_mangen`. The main page also has DESCRIPTION, FILES, EXIT STATUS, ENVIRONMENT, SEE ALSO
(every command page, pacman(8), makepkg(8), pacman.conf(5), PKGBUILD(5), bwrap(1)), BUGS, VERSION, AUTHOR and COPYRIGHT; a command page
points back to its parent. Hidden helpers get no page. The packaging calls it: `install.sh` and the AUR `PKGBUILD` run
`emrac man --generate man-pages` and install the files to `/usr/share/man/man1` (guarded, so a tag from before this existed builds as before),
which is what makes `man emrac` and `man emrac-install` work after `pacman -S emrac`. All pages pass `groff -man -ww` without a warning.

## Not covered

Sending reports from emrac, a pager of our own, translations, and a page for the hidden root helpers.

## Verification

Unit tests for the man lookup (a command, a subcommand, an unknown one), for the credits list against `Cargo.toml`, and for the
bug report (scrubbing, three failures at most); `dev/e2e/final-check.sh`-style container checks in `dev/e2e/helppages.sh`.

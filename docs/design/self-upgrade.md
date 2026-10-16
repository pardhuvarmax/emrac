# Upgrading and downgrading emrac itself

How `emrac self upgrade` and `emrac self downgrade` work, for people who use emrac.

**The rule.** pacman owns `/usr/bin/emrac` when emrac was installed as a package, and emrac never
overwrites a file a package owns. The method follows who owns the copy:

- **The `emrac` package owns it:** build a package from the release and install it with
  `sudo pacman -U`.
- **Nobody owns it** (`cargo install`, or a copy put in `~/.local/bin` or `/usr/local/bin` by hand)
  **and you can write to its folder:** update it **in place** (below): no root, no pacman, the old
  file kept.
- **Anything else** (another package's file, a cargo `target/` build, a copy whose owner can't be
  told, a link, a folder you can't write to) is refused with what to do instead. emrac never
  guesses and never asks for root to replace a file.

`--dry-run` names the method (`Method:` line) and the file it would replace.

**What `self upgrade` does** (`--dry-run` prints this and does nothing; `--yes` is passed to pacman as
`--noconfirm`; `--skip-tests` leaves out the package's test run):

1. If the release index is more than a day old it fetches a fresh one (`--offline`: uses it as it is).
2. Plans: the installed version, the newest release your channel offers, the checks below, every
   step. Already newest: says so and stops. Anything blocking is listed in full.
3. Takes a lock and writes a journal (phase by phase), so a cut-off run is found next time.
4. Copies your configuration and `~/.local/share/emrac` (except the `self` folder) to
   `~/.local/share/emrac/self/backup-<time>/`.
5. Downloads the tarball and checks its SHA-256 against the release index; unpacks the list to see
   that the top folder is `emrac-<tag>` and that its `Cargo.toml` says the version the index promised.
6. Builds the package with `makepkg` from the recipe that `install.sh` and the AUR package use (a
   test keeps the three equal), then installs it with `sudo pacman -U`.
7. Checks that `/usr/bin/emrac --version` shows the new version and that `emrac self check` passes.
8. Removes the journal and writes a line to `~/.local/share/emrac/self/log.jsonl`.

A failure before step 6 changes nothing on the system. If the new version installed but did not pass
the checks of step 7, emrac says so and points to `self downgrade` and the backup.

**What an in-place update does differently** (steps 1 to 5 above are the same; no `makepkg`,
`pacman`, `sudo` or `fakeroot` is used or needed):

6. Builds the binary with `cargo` in `~/.cache/emrac/self/` with the package recipe's flags (the test
   run is left out by `--skip-tests`).
7. Runs the new binary from there: `--version` must say the target version.
8. Copies it next to the old file (same folder, so the last step is one rename), runs it once more
   from there, and keeps the old file in `~/.local/share/emrac/self/previous/` (the newest 3).
9. Renames the new file over the old one: one atomic step, never a half-written emrac; a running
   emrac keeps working.
10. Checks `<path> --version` and `self check`. If the new file does not report the target version,
    the kept file is **put back automatically** and the run says so. If only `self check` reports a
    problem nothing is rolled back (it may be an old problem).

Before step 9 nothing outside `~/.cache/emrac` and the backup has changed. Only that one file is
replaced: completions or docs you installed by hand are not touched, and for a `cargo install` copy
`cargo install --list` goes on showing the old version, because cargo does not know about the
update. `self downgrade` takes the same path with an older release.

**Checks before anything starts:** the copy is owned by the `emrac` package, or is a regular file nobody owns in a folder you can write to; a required signature was
verified when the index was synced; for a package: `makepkg`, `pacman`, `sudo`, `cargo`, a C compiler,
`fakeroot` and `tar` are installed (for an in-place update only `cargo`, a C compiler and `tar`); about 2 GiB are free where the build runs; no other self operation is running.
`emrac self diagnose` shows each one.

**`self downgrade`** is the same machinery with an older target: `--version 0.2.0` or `--tag r2`, and
without either only if exactly one older release exists (otherwise it lists them and stops).
It refuses when the older release understands emrac's files in an older format than yours: nothing is
converted backwards or deleted. When it goes ahead it says that settings your current version added
may be ignored by the old one, and the backup keeps your files exactly as they are.

**If it is cut off** (power loss, a killed terminal): the journal stays. `emrac self check` and
`self diagnose` show it with what it means: before the package install nothing on the system changed;
during it, `pacman -Q emrac` says whether the package was replaced.

**What it does not do:** no background or automatic updates; no `--force` over a refusal; it does not
touch the release index's trust (that is `self sync` and the signature settings).

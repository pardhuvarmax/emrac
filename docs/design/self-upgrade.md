# Upgrading and downgrading emrac itself

How `emrac self upgrade` and `emrac self downgrade` work, for people who use emrac.

**The rule.** pacman owns `/usr/bin/emrac` when emrac was installed as a package, and emrac never
overwrites a file a package owns. So the commands work only for such a copy, and they do the
change the way you would: build a package from the release and install it with `sudo pacman -U`.
A copy installed with `cargo install`, put in place by hand, run from a cargo `target/` folder, or
whose owner can't be told is refused with what to do instead. emrac never guesses.

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

**Checks before anything starts:** the copy is owned by the `emrac` package; a required signature was
verified when the index was synced; `makepkg`, `pacman`, `sudo`, `cargo`, a C compiler, `fakeroot` and
`tar` are installed; about 2 GiB are free where the build runs; no other self operation is running.
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

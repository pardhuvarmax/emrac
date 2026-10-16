# Slice u1 (after r3) — emrac updates a copy pacman does not own, in place and safely

`emrac self upgrade` and `self downgrade` work only when the `emrac` pacman package owns the binary.
A copy from `cargo install`, or one put in `~/.local/bin` or `/usr/local/bin` by hand, is refused
today. This slice lets emrac update **those** copies itself, without pacman, and keeps the pacman
path exactly as it is. The owner's request: keep the pacman way, and also let emrac update itself,
the safe way.

## The rule that does not change

emrac never overwrites a file a package owns, and never guesses. The method follows the owner:

| Owner | Method |
|---|---|
| the `emrac` package | build a package, `sudo pacman -U` (unchanged) |
| `cargo install` (`~/.cargo/bin`), or a copy by hand (`~/.local/bin`, `/usr/local/bin`) | **in place** (this slice), only if the file is a regular file and its folder is writable by you |
| another package, a cargo `target/` build, unknown | refused, as before |

No `sudo`, no root, no `--force`. A copy in a folder you cannot write to (`/usr/local/bin` as a
normal user) is refused with the reason; emrac does not ask for root to do this.

## What an in-place upgrade does

1. Lock and journal, backup of configuration and data, download checked against the index's
   SHA-256, the tarball's top folder and `Cargo.toml` version checked: the same first steps.
2. Unpack and build with `cargo` in `~/.cache/emrac/self/<tag>-<time>/` (needs `cargo`, a C
   compiler, `tar`; not `makepkg`, `pacman`, `sudo` or `fakeroot`). The same flags as the package
   recipe (`--frozen --release`, build paths remapped); the test run is skipped by `--skip-tests`.
3. Run the **new binary from its staging folder** with `--version`: it must say the target version.
4. Copy it next to the old one (same folder, so the final step is a rename on one filesystem), set
   its mode, flush it to disk, and run it once more from there (catches a `noexec` mount).
5. Keep the old binary in `~/.local/share/emrac/self/previous/emrac-<version>-<time>` (newest 3).
6. `rename` the new file over the old one: one atomic step, so there is never a half-written
   emrac, and a running emrac keeps working.
7. Check `<path> --version` and `self check`. If the new file does not report the target version,
   the kept copy is **put back automatically** and the run says so. If only `self check` reports a
   problem, nothing is rolled back (it may be an old problem), and the report says what to do.
8. Remove the staging folder and the journal; one line in the self log.

Before step 6 nothing outside `~/.cache/emrac` and the backup changed. A cut-off run leaves the
journal, and `self check` says what that means for the in-place path as well.

## Plan and report

`self upgrade --dry-run` names the method and the file it would replace, with the steps. The plan
warns that the copy is not tracked by pacman (completions and docs installed by hand are not
touched) and, for `cargo install`, that `cargo install --list` will go on showing the old version.
`self downgrade` takes the same path with an older release, with the same schema refusal as now.
`self check` and `self diagnose` tell a cargo or hand-placed copy that `self upgrade` can update it
in place, and what is missing if it cannot.

## Not in this slice

A prebuilt binary (every update still builds from source: a published binary needs a signing
decision first); updating a pacman-owned copy without pacman (never); an instant `self rollback`
command from the kept copies (the report names the kept file and how to put it back).

## Verification

Unit tests: the method per owner, refusals (a link, a folder that cannot be written, a missing
tool), the steps, the swap (the new file in place, the old one kept, only 3 kept, a binary that
reports another version refused before anything changes, the rollback restoring the old bytes).
Container: install the r8 tarball's binary by hand into `~/.local/bin`, upgrade it to r9 and
downgrade back, check the kept copies, and that a tarball that does not match the index changes
nothing.

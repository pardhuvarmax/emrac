# Emrac First AUR Release : R2-AUR

Tag `r2-aur`, 2026-10-14. Built by pardhuvarmax — https://github.com/pardhuvarmax/emrac

This is the source release the first Arch User Repository package is built from. The program is **release r2**: its behaviour is unchanged and the version is still `0.2.0`. Compared with the `r2` tag, the Rust code differs only in tests (and a test-only switch in an output helper). What this tag adds is what a distribution package needs.

## What is new compared with the `r2` tag

- **A license.** emrac is free software under the GNU General Public License, version 3 or (at your option) any later version (`GPL-3.0-or-later`). The standard text is in `LICENSE`, and both crates carry the license in their package metadata. The choice follows from the dependencies: the `alpm` crates are GPL-3.0 and libalpm is GPL-2.0-or-later. The `r2` tag has no license file, which is why a package is built from this tag instead.
- **A tracked list of tests.** `docs/tests/emrac-tests.rst` lists every passing automated test (271) and the 26 end-to-end checks. `dev/gen-tests-doc.sh` regenerates it and CI fails if it is stale.
- **The end-to-end checks in the repository** (`dev/e2e/`), run in a disposable container against a real pacman, and a release build script (`dev/release-build.sh`) that embeds none of the builder's paths.
- **A README in every folder**, the stale ones rewritten.
- **Tests that hold up in a clean build.** Building the package in a clean chroot showed that two sandbox tests failed where bubblewrap cannot mount `/proc`: their check for "is the sandbox usable?" left out the `/proc` mount that emrac's own check performs. They now use the same arguments and skip where the sandbox cannot work, and run for real where it can. A third test no longer writes 5,000 lines into every test log.
- **Credit and metadata:** `emrac --version` and `--help` name the builder; the images carry no author metadata.

## What emrac r2 is

A hybrid package management platform for Arch Linux, built over `pacman`, `makepkg`, `libalpm` and the AUR. Plans that match what pacman will do, AUR packages that are reviewed, audited, built in a sandbox and inspected, history with rollback, `doctor`, `pacnew` for `.pacnew` files with validation and revert, and `cache` with a lean policy. The full notes are in [`r2.md`](./r2.md), including what changes for someone updating from r1 and the known limits.

## For packagers

- **Source:** `https://github.com/pardhuvarmax/emrac/archive/refs/tags/r2-aur.tar.gz`.
- **Build:** `cargo build --release --locked` (the lock file is committed). To keep the builder's home directory out of the binary, pass `--remap-path-prefix` as `dev/release-build.sh` does. The program is the single binary `target/release/emrac`; the crate producing it is named `emrac-cli`.
- **Build dependencies:** a Rust toolchain, `clang` (the `alpm` crate generates its bindings with bindgen), and the `libalpm` headers that come with `pacman`.
- **Runtime dependencies:** `pacman`, `git`, `sudo`, `libarchive` (`bsdtar`) and `fakeroot`. `bubblewrap` is needed to build AUR packages in the sandbox; without it emrac asks you to install it or to pass `--no-sandbox`. A systemd user session gives memory and process limits for builds.
- **License file:** `LICENSE`, GPL-3.0-or-later.
- **Tests:** `cargo test --workspace`; the tests that need bubblewrap or a systemd session skip themselves when the machine has none.

## Known limits

The same as r2, in [`r2.md`](./r2.md): no terminal interface, build profiles, build queue, news check, `locate`, `lock`, filesystem snapshots or Landlock layer. It has been used on one real machine so far, which is why the upgrade, `pacnew` and cache commands ask before they change anything.

# dev/

Development/test infrastructure — not part of the shipped `emrac` product.

## release-build.sh

A release build (`target/release/emrac`) that does not embed the builder's paths: it passes `--remap-path-prefix` for the home directory, the cargo registry and the checkout, since `profile.release.trim-paths` is not stable yet. Run it from anywhere: `dev/release-build.sh`.

## e2e/

End-to-end checks of the release binary against a real pacman, run in the container below with `dev/e2e/run.sh`. See [`e2e/README.md`](./e2e/README.md).

## pin-install-script.sh

Points the root `install.sh` at a release tag: `dev/pin-install-script.sh <tag>` downloads the tag's tarball, computes its SHA-256, reads the version and rewrites the three pinned values at the top of the script. Run it after pushing a new tag and commit the result.

## gen-tests-doc.sh

Runs the tests and rewrites [`docs/tests/emrac-tests.rst`](../docs/tests/emrac-tests.rst), the list of every passing test; `--check` fails if the committed file is stale, and CI runs it. See [`docs/tests/README.md`](../docs/tests/README.md).

## container/

A disposable Arch Linux [podman](https://podman.io/) container for testing emrac's mutating commands (`install`, `remove`, `upgrade`, AUR building) without ever touching this host's real pacman state. The container gets its own independent, freshly-synced package database at build time — it's never given access to the host's `/var/lib/pacman`. It also carries `git` and `base-devel` (compiler toolchain, `fakeroot`, etc.), needed for `makepkg` to actually build AUR packages. Only the compiled `emrac` binary is mounted in (read-only), from `target/debug/emrac`.

```sh
dev/container/run.sh
```

This builds emrac on the host, builds the container image (cached after the first run), and drops you into a shell inside it as a non-root user (`tester`, in `wheel`, with passwordless `sudo` scoped to `pacman` only — matching the real privilege model: emrac shells out to `sudo pacman`/`makepkg`, prompting interactively, never running as root itself).

Once inside:

```sh
emrac install ripgrep --yes
pacman -Q ripgrep      # confirm it actually installed, in the container
emrac remove ripgrep --yes

emrac install <some-aur-package>   # builds via makepkg, after showing the PKGBUILD
emrac upgrade --yes                # official pacman -Syu, plus any AUR packages behind
```

Verified (2026-07-17): a real install + remove cycle inside the container leaves the host's own `ripgrep` installation completely untouched.

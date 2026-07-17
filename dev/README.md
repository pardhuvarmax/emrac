# dev/

Development/test infrastructure — not part of the shipped `emrac` product.

## container/

A disposable Arch Linux [podman](https://podman.io/) container for testing emrac's mutating commands (`install`, `remove`, and future `upgrade`/build commands) without ever touching this host's real pacman state. The container gets its own independent, freshly-synced package database at build time — it's never given access to the host's `/var/lib/pacman`. Only the compiled `emrac` binary is mounted in (read-only), from `target/debug/emrac`.

```sh
dev/container/run.sh
```

This builds emrac on the host, builds the container image (cached after the first run), and drops you into a shell inside it as a non-root user (`tester`, in `wheel`, with passwordless `sudo` scoped to `pacman` only — matching the real privilege model: emrac shells out to `sudo pacman`, prompting interactively, never running as root itself).

Once inside:

```sh
emrac install ripgrep --yes
pacman -Q ripgrep      # confirm it actually installed, in the container
emrac remove ripgrep --yes
```

Verified (2026-07-17): a real install + remove cycle inside the container leaves the host's own `ripgrep` installation completely untouched.

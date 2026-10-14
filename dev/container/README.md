# container/

A disposable Arch Linux container for trying emrac's mutating commands (`install`, `remove`, `upgrade`, AUR builds) without touching the host's pacman state.

- [`Containerfile`](./Containerfile) — `archlinux:latest` with `sudo`, `git`, `base-devel` and `bubblewrap`, and a non-root user `tester` whose only passwordless `sudo` right is `pacman`. The image has its own freshly synced package database and never sees the host's `/var/lib/pacman`.
- [`run.sh`](./run.sh) — builds emrac on the host with the system toolchain, builds the image (cached after the first run) and opens a shell in it with the new binary mounted read-only at `/usr/local/bin/emrac`. It passes `--security-opt unmask=/proc/*` so bubblewrap can mount a fresh `/proc` for the build sandbox; the container is not privileged.

Needs podman. The scripted checks in [`../e2e/`](../e2e) use the same image.

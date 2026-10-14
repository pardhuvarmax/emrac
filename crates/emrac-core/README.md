# emrac-core

The library behind emrac. It owns all interaction with libalpm, pacman, makepkg, git and the AUR, so that the command line (and a future terminal interface) call one shared implementation instead of duplicating it. Nothing here prints to the terminal or reads from it; that is the front end's job.

Where things are (details in [`src/README.md`](./src/README.md)):

- **Sources and plans** — `Sources` is the entry point; it merges the official repositories and the AUR, builds plans (what an install, removal or upgrade will do, including what it will also remove), and runs them.
- **AUR builds** — `audit` (static reading of PKGBUILDs, scriptlets and shipped files), `manifest` (what a built package changes), `sandbox` (bubblewrap and resource limits), and the `backend/aur_build` pipeline that ties them to `makepkg`.
- **Recovery** — `history`, `rollback`, `downgrade`, `reinstall`, `purge`, `snapshot`, `pins`, and `verify`.
- **Housekeeping** — `doctor` (health checks), `pacnew` (`.pacnew` files), `cache`, `config`, `deps`.
- **Backends** — [`src/backend/`](./src/backend): libalpm, pacman, the AUR RPC, `git`/`makepkg`, the Arch Linux Archive, a freshly synced throwaway database, and package-file reading.

Run its tests with `cargo test -p emrac-core` (244 tests, listed in [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst)). Commands that change the system are tried in a container instead: see [`dev/`](../../dev).

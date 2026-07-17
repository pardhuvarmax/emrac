# backend/

Concrete package-source backends, one module per source. None of these are meant to be used directly by the CLI/TUI — see [`../sources.rs`](../sources.rs) for the aggregator that orchestrates them.

- [`alpm.rs`](./alpm.rs) — official repositories, read-only, via libalpm FFI (the `alpm` crate, built with its `generate` feature so bindings match whatever libalpm version is actually installed).
- [`aur.rs`](./aur.rs) — the AUR, read-only, via the aurweb RPC v5 API (`https://aur.archlinux.org/rpc/`) over `ureq`.
- [`pacman.rs`](./pacman.rs) — the only module that shells out to `pacman`/`sudo pacman`. Resolution (`--print`) is read-only and needs no root; execution (`install`/`remove`) runs under `sudo`.
- [`aur_build.rs`](./aur_build.rs) — the only module that shells out to `git`/`makepkg`. Caches each AUR package's git checkout under `~/.cache/emrac/build/<pkg>`, reused across runs so a rebuild only needs to review what changed rather than the whole PKGBUILD again. Building runs as the invoking user, never root — `makepkg -s` handles syncing any missing *official* dependencies itself (via `pacman`, prompting for `sudo` as needed); an AUR-only transitive dependency isn't resolved automatically and surfaces as a build failure instead.

`upgrade` will land here in a later increment.

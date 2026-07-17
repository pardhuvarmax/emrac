# backend/

Concrete package-source backends, one module per source. None of these are meant to be used directly by the CLI/TUI — see [`../sources.rs`](../sources.rs) for the aggregator that orchestrates them.

- [`alpm.rs`](./alpm.rs) — official repositories, read-only, via libalpm FFI (the `alpm` crate, built with its `generate` feature so bindings match whatever libalpm version is actually installed).
- [`aur.rs`](./aur.rs) — the AUR, read-only, via the aurweb RPC v5 API (`https://aur.archlinux.org/rpc/`) over `ureq`.
- [`pacman.rs`](./pacman.rs) — the only module that shells out to `pacman`/`sudo pacman`. Resolution (`--print`) is read-only and needs no root; execution (`install`/`remove`) runs under `sudo`.

The `makepkg`-subprocess backend used for AUR building, and `upgrade`, will land here in a later increment.

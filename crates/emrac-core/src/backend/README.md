# backend/

Concrete package-source backends, one module per source. Neither is meant to be used directly by the CLI/TUI — see [`../sources.rs`](../sources.rs) for the aggregator that merges them.

- [`alpm.rs`](./alpm.rs) — official repositories, read-only, via libalpm FFI (the `alpm` crate, built with its `generate` feature so bindings match whatever libalpm version is actually installed).
- [`aur.rs`](./aur.rs) — the AUR, read-only, via the aurweb RPC v5 API (`https://aur.archlinux.org/rpc/`) over `ureq`.

The mutating `pacman`/`makepkg`-subprocess backend used for installs/removes/builds will land here in a later increment.

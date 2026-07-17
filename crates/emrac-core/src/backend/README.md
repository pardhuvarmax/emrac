# backend/

Concrete package-source backends, one module per source.

- [`alpm.rs`](./alpm.rs) — official repositories, read-only, via libalpm FFI (the `alpm` crate, built with its `generate` feature so bindings match whatever libalpm version is actually installed).

An AUR backend and the mutating `pacman`/`makepkg`-subprocess backend used for installs/removes/builds will land here in later slices.

# crates/

Emrac's Rust workspace members.

- [`emrac-core/`](./emrac-core) — backend library. Owns all interaction with libalpm/pacman/makepkg. Neither interface talks to libalpm directly.
- [`emrac-cli/`](./emrac-cli) — the `emrac` binary. A thin layer over `emrac-core`: argument parsing, command dispatch, output formatting.

A future `emrac-tui` crate is expected to sit alongside these and depend on `emrac-core` the same way `emrac-cli` does — see the root [`SPEC.md`](../SPEC.md) Part II ("CLI and TUI expose the same core functionality").

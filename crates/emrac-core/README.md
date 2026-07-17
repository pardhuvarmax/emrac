# emrac-core

Backend library crate. Owns all interaction with libalpm/pacman/makepkg so that both the CLI and (later) the TUI call into one shared implementation instead of duplicating it.

**Current contents (Slice 1):**

- [`backend/alpm.rs`](./src/backend/alpm.rs) — read-only libalpm queries against the official repo sync databases (`search`, `info`). No network calls, no root required.
- [`package.rs`](./src/package.rs) — `PackageSummary` / `PackageDetails` data models.
- [`error.rs`](./src/error.rs) — crate error type.

**Not yet implemented:** AUR backend, and the `pacman`/`makepkg` subprocess wrappers for mutating operations (install/remove/upgrade/build). See the root [`SPEC.md`](../../SPEC.md) Part X for the planned integration strategy.

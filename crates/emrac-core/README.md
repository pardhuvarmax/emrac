# emrac-core

Backend library crate. Owns all interaction with libalpm/pacman/makepkg/AUR so that both the CLI and (later) the TUI call into one shared implementation instead of duplicating it.

**Current contents (Slice i1):**

- [`backend/alpm.rs`](./src/backend/alpm.rs) — read-only libalpm queries against the official repo sync databases (`search`, `info`). No network calls, no root required.
- [`backend/aur.rs`](./src/backend/aur.rs) — read-only queries against the AUR's RPC API (`search`, `info`). Outbound HTTPS GET only, no root required.
- [`sources.rs`](./src/sources.rs) — `Sources`: the aggregator that merges official repos and the AUR behind one `search`/`info` surface. This is what the CLI/TUI should actually call, not the two backends directly.
- [`package.rs`](./src/package.rs) — `PackageSummary` / `PackageDetails` data models (AUR-only fields like `maintainer`/`votes`/`popularity` are `None` for official-repo packages).
- [`error.rs`](./src/error.rs) — crate error type.

**Not yet implemented:** the `pacman`/`makepkg` subprocess wrappers for mutating operations (install/remove/upgrade/build). See the root [`SPEC.md`](../../SPEC.md) Part X for the planned integration strategy.

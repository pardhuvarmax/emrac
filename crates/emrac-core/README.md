# emrac-core

Backend library crate. Owns all interaction with libalpm/pacman/makepkg/AUR so that both the CLI and (later) the TUI call into one shared implementation instead of duplicating it.

**Current contents (Slice i1):**

- [`backend/alpm.rs`](./src/backend/alpm.rs) — read-only libalpm queries against the official repo sync databases (`search`, `info`, and resolved-package lookups used to build install/remove plans). No network calls, no root required.
- [`backend/aur.rs`](./src/backend/aur.rs) — read-only queries against the AUR's RPC API (`search`, `info`). Outbound HTTPS GET only, no root required.
- [`backend/pacman.rs`](./src/backend/pacman.rs) — the only place that shells out to `pacman`/`sudo pacman`. Dependency/upgrade resolution (`--print`) needs no root; actual execution (`install`/`remove`/`upgrade`) runs under `sudo`, prompting interactively.
- [`backend/aur_build.rs`](./src/backend/aur_build.rs) — the only place that shells out to `git`/`makepkg`. Clones/fetches each AUR package into a persistent cache (`~/.cache/emrac/build/<pkg>`) and builds via `makepkg -si`, never as root. Reused as-is for AUR upgrades — a rebuild is just another sync/review/build cycle.
- [`sources.rs`](./src/sources.rs) — `Sources`: the aggregator that merges official repos and the AUR behind one `search`/`info` surface, classifies `install`/`upgrade` targets by source, and orchestrates `pacman.rs`/`alpm.rs`/`aur_build.rs` accordingly. This is what the CLI/TUI should actually call, not the backends directly.
- [`plan.rs`](./src/plan.rs) — `Plan`/`PlannedPackage`/`PlanAction`: the transaction preview shown before any install/remove/upgrade executes.
- [`package.rs`](./src/package.rs) — `PackageSummary` / `PackageDetails` data models (AUR-only fields like `maintainer`/`votes`/`popularity` are `None` for official-repo packages).
- [`error.rs`](./src/error.rs) — crate error type.

This completes the "core CLI loop" milestone (`search`/`info`/`install`/`remove`/`upgrade` against official repos + AUR) from `SPEC.md` Part X. See [`../../dev/README.md`](../../dev/README.md) for how mutating commands are tested without touching the host.

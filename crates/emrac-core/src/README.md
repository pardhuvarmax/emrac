# src/

`lib.rs` is the crate root — it re-exports the public API (`Sources`/`SearchScope`/`SearchResults`, `Plan`/`PlannedPackage`/`PlanAction`, `Error`/`Result`, `Config`, `PackageSummary`/`PackageDetails`) and nothing else. `Sources` (in [`sources.rs`](./sources.rs)) is the intended entry point; the source-specific backends in [`backend/`](./backend) aren't re-exported. Data models live in [`package.rs`](./package.rs) and [`plan.rs`](./plan.rs), the crate error type in [`error.rs`](./error.rs), and the persisted TOML config (`~/.config/emrac/config.toml`) in [`config.rs`](./config.rs).

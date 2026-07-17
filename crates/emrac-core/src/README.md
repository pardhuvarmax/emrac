# src/

`lib.rs` is the crate root — it re-exports the public API (`Sources`/`SearchScope`/`SearchResults`, `Error`/`Result`, `PackageSummary`/`PackageDetails`) and nothing else. `Sources` (in [`sources.rs`](./sources.rs)) is the intended entry point; the source-specific backends in [`backend/`](./backend) aren't re-exported. Data models live in [`package.rs`](./package.rs), the crate error type in [`error.rs`](./error.rs).

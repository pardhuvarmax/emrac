# src/

`lib.rs` is the crate root — it re-exports the public API (`AlpmBackend`, `Error`/`Result`, `PackageSummary`/`PackageDetails`) and nothing else. Implementation lives in [`backend/`](./backend), [`package.rs`](./package.rs), and [`error.rs`](./error.rs).

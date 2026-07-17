# commands/

One file per subcommand. Each exposes a thin `run()` that calls into `emrac-core` and hands the result to `output.rs` — no business logic lives here.

Currently: [`search.rs`](./search.rs), [`info.rs`](./info.rs).

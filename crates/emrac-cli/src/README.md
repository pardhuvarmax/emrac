# src/

`main.rs` parses `Cli` (defined in [`cli.rs`](./cli.rs)) and dispatches to [`commands/`](./commands). [`output.rs`](./output.rs) holds the human-readable/`--json` formatting shared across commands. [`prompt.rs`](./prompt.rs) is the plain stdin yes/no confirmation used before mutating commands execute.

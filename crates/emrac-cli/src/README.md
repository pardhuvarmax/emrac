# src/

- [`main.rs`](./main.rs) — parses `Cli`, loads the config, works out the effective flags, and dispatches to [`commands/`](./commands). Commands that need no pacman setup (`config`, `completion`, `history`, `audit` on a path, `pin` list / `unpin`, snapshot list / show / delete) are handled before `Sources::init()`.
- [`cli.rs`](./cli.rs) — the command-line definition (`clap` derive): commands, flags, the version and credit line.
- [`output.rs`](./output.rs) — human-readable and `--json` formatting shared by the commands.
- [`prompt.rs`](./prompt.rs) — confirmations: `[Y/n]`, a default-no `[y/N]`, a choice among letters, and a typed confirmation for risky changes. Input is read one byte at a time so piped answers reach the right program, and **end of input is never consent**.
- [`commands/`](./commands) — one module per subcommand.

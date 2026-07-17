# emrac-cli

The `emrac` binary. A thin layer over [`emrac-core`](../emrac-core): parses arguments ([`cli.rs`](./src/cli.rs), via `clap`), dispatches to [`commands/`](./src/commands), and formats output ([`output.rs`](./src/output.rs)) as human-readable text or `--json`.

**Current commands (Slice i1):** `search` (official repos + AUR, `--official`/`--aur` to scope), `info` (same, with `--offline` to skip the AUR). See the root [`SPEC.md`](../../SPEC.md) Part V and Part VIII for the full planned command surface.

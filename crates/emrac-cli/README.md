# emrac-cli

The `emrac` binary. A thin layer over [`emrac-core`](../emrac-core): parses arguments ([`cli.rs`](./src/cli.rs), via `clap`), dispatches to [`commands/`](./src/commands), formats output ([`output.rs`](./src/output.rs)) as human-readable text or `--json`, and prompts for confirmation ([`prompt.rs`](./src/prompt.rs)) before mutating anything.

**Current commands (Slice i1):**
- `search` — official repos + AUR, `--official`/`--aur` to scope.
- `info` — same, with `--offline` to skip the AUR.
- `install` / `remove` — official repos only. Always show a plan first; `--dry-run` stops there, otherwise confirms (skippable with `-y`/`--yes`) before running the real `sudo pacman` command. See [`../../dev/README.md`](../../dev/README.md) for how these are tested without touching the host.

See the root [`SPEC.md`](../../SPEC.md) Part V and Part VIII for the full planned command surface.

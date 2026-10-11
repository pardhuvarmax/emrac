# Slice u2 — shell completion

## Context

`u1` shipped `emrac config`. [`FEATURE_STATUS.rst`](../../FEATURE_STATUS.rst)
category #18 "Unified Interfaces" lists shell completion as missing, and
`SPEC.md` (Part V, `completion / alias / integrate`) specifies
`emrac completion bash|zsh|fish`. It is small, self-contained, and needs
no pacman/AUR access, so it fits an update slice.

## Design

- **Static generation via `clap_complete`** (`clap_complete = "4"` in
  `emrac-cli`). The completion script is derived from the `Cli` definition
  with `CommandFactory`, so it can't drift from the real flags and
  subcommands.
- **`emrac completion <shell>` prints the script to stdout**, matching the
  spec's example (`emrac completion zsh > ~/.zsh/completions/_emrac`).
  `<shell>` is `bash | zsh | fish`, exactly the spec's three — a
  `clap::ValueEnum` of our own rather than `clap_complete::Shell`, so
  `elvish`/`powershell` don't silently appear as supported.
- **Handled before `Sources::init()`**, alongside `config`: it needs no
  libalpm.
- **Descoped**: dynamic completion of package names (needs a runtime
  callback into libalpm/AUR and per-shell glue), `emrac alias`, and
  `emrac integrate [--install]` (writes to the user's rc files; separate
  slice, warrants its own safety design).

## Files

- `crates/emrac-cli/Cargo.toml` — add `clap_complete = "4"`.
- `crates/emrac-cli/src/cli.rs` — `Commands::Completion { shell }`,
  `enum CompletionShell { Bash, Zsh, Fish }`.
- `crates/emrac-cli/src/commands/completion.rs` — new, `run(shell)`.
- `crates/emrac-cli/src/commands/mod.rs`, `main.rs` — register and
  dispatch before `Sources::init()`.
- Docs: `commands/README.md`, root `README.md`, `FEATURE_STATUS.rst` #18,
  `CHANGELOG.md`, `docs/plan/README.md`.

## Verification

- `emrac completion bash|zsh|fish` each exit 0 and print a non-empty script
  mentioning every subcommand (including `completion` and `config`).
- `emrac completion powershell` is rejected by clap with a clean error.
- `bash -n` parses the bash script; `zsh -n` / `fish --no-execute` the
  others when available.
- A unit test asserts the generated script for each shell contains all
  top-level subcommand names.

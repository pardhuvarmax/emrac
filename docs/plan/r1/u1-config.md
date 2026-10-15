# Slice u1 — config file

## Context

`r1` (the core CLI loop — `search`/`info`/`install`/`remove`/`upgrade`,
official repos + AUR) is done as of `196141a`. Per
[`FEATURE_STATUS.rst`](../../../FEATURE_STATUS.rst), category #19
"Configuration" is Not started — every run reads live `pacman-conf` and
every flag defaults to `false`/unset, with no way to persist a preference.
This is the first `u1` increment: small, self-contained, and it unblocks
later Partial items (Build Profiles, Cache Management tuning, AUR review
defaults) that would otherwise each reinvent their own settings file.

`SPEC.md` #19 and the `### Configuration` command group (Part V) describe
a much bigger surface than this slice attempts — TOML config, theming,
keybindings, aliases, environment profiles, system-wide config, per-repo
config. Per the standing project stance that the spec is a brainstorm and
not a locked contract, this plan only covers what's actually load-bearing
right now: persisting defaults for flags that already exist on the CLI
today. Theming/keybindings need the TUI (not started); aliases and
environment profiles have no concrete use case yet. Those stay in
`SPEC.md` as future scope, not in this slice.

## Design

- **Format**: TOML, via a new `toml = "0.8"` dependency in `emrac-core`
  (`serde` + `derive` are already a dependency, so this is the only
  addition). Matches `SPEC.md`'s own choice ("TOML configuration") and
  Rust-ecosystem convention (Cargo, rustfmt, clippy all use TOML).
- **Location**: `$HOME/.config/emrac/config.toml`, resolved the same way
  `aur_build.rs` already resolves the build cache dir (`env::var("HOME")`,
  `Error::NoHomeDir` on failure) — no `XDG_CONFIG_HOME` handling, no new
  `dirs` dependency, matching the existing cache-dir precedent
  (`crates/emrac-core/src/backend/aur_build.rs:109`) rather than
  introducing a second, more thorough convention just for this.
- **Missing file = all defaults.** Every command already works with zero
  config today; that has to keep being true. `Config::load()` returns
  `Config::default()` if the file doesn't exist, and only errors if the
  file exists but doesn't parse.
- **Scope of settings — only what's already a CLI flag today**, grouped
  into three TOML tables that double as the `config get/set` key
  namespace:

  ```toml
  [flags]
  offline = false
  yes = false
  dry_run = false
  json = false
  quiet = false

  [review]
  skip_pkgbuild = false
  skip_diff = false

  [search]
  limit = 20        # optional; unset = no limit
  official = false
  aur = false
  ```

  (`verbose` is excluded — it's declared on `Cli` today but nothing reads
  it; not this slice's job to wire up a dead flag.)

- **Merge model: OR, not override.** Every one of these is a bool flag
  that only ever turns a behavior *on* (there's no `--offline=false`) —
  clap gives no way to distinguish "user passed `--offline`" from "user
  didn't," so there's no way to have an explicit CLI flag override a
  config value that's already `true`. The correct semantics fall out of
  that for free: `effective = cli_flag || config.flags.x`. Config sets a
  persisted floor, the flag can only raise it. `search.limit` merges as
  `cli.limit.or(config.search.limit)`. This needs zero changes to `Cli`'s
  field types (still plain `bool`/`Option<usize>`) — the merge is a few
  `||`/`.or()` calls in `main.rs::run()`, right where the flags are
  already being unpacked before dispatch to `commands::*::run()`.
  `commands/*.rs` and `sources.rs` signatures don't change at all.
- **`config set`/`get`/`unset` are a fixed 9-key allowlist**
  (`flags.offline`, `flags.yes`, `flags.dry_run`, `flags.json`,
  `flags.quiet`, `review.skip_pkgbuild`, `review.skip_diff`,
  `search.limit`, `search.official`, `search.aur`), matched directly onto
  `Config` struct fields — not a generic `toml::Value` tree edit. With
  only 9 keys, a `match` is simpler and keeps type checking (bool vs.
  usize) for free; an unrecognized key is `Error::UnknownConfigKey`.
  `unset <key>` resets that one field to its struct default and rewrites
  the whole file (all fields have `#[serde(default)]`, so this is exact).
- **`config edit`** opens `$EDITOR` (fallback `vi`) on the config path,
  creating the parent dir and an empty-but-valid file first if neither
  exists yet, then re-parses on close and reports a validation error
  without discarding the edit (the file stays as the user left it; they
  re-run `config edit` or `config validate` to fix it).
- **`config validate`** just calls `Config::load()` and reports ok or the
  `toml::de::Error` (which already carries line/column) — no separate
  validation logic to write.
- **Descoped for this slice**: `config reset` (== `rm` the file, trivial
  enough to not need a subcommand yet), `config export`/`import`
  (redundant — the file itself is already a portable, human-editable
  artifact; `cp ~/.config/emrac/config.toml backup.toml` does the same
  thing with no new code). Both stay in `SPEC.md` as later, cheap
  additions if a real need shows up.

## Files

- `crates/emrac-core/Cargo.toml` — add `toml = "0.8"`.
- `crates/emrac-core/src/config.rs` — new. `Config` struct (three nested
  structs for the `[flags]`/`[review]`/`[search]` tables, all
  `#[derive(Debug, Clone, Deserialize, Serialize, Default)]` with
  `#[serde(default)]`), `Config::path()`, `Config::load()`,
  `Config::save(&self)`, `Config::get(key)`/`set(key, value)`/`unset(key)`
  over the 9-key allowlist.
- `crates/emrac-core/src/error.rs` — add `Error::NoConfigDir` (reuses the
  `$HOME`-missing case but with config-specific wording, doesn't overload
  `NoHomeDir`'s AUR-cache message), `Error::ConfigParse(toml::de::Error)`,
  `Error::ConfigWrite { context, source }`, `Error::UnknownConfigKey`,
  `Error::InvalidConfigValue { key, value }` (bad `config set` value for
  the key's type); extend `voice()` for all five.
- `crates/emrac-core/src/lib.rs` — `mod config;` +
  `pub use config::Config;`.
- `crates/emrac-cli/src/cli.rs` — new `Commands::Config(ConfigCommand)`
  with a nested `#[derive(Subcommand)] enum ConfigCommand { Show, Get {
  key: String }, Set { key: String, value: String }, Unset { key: String
  }, Edit, Validate }`.
- `crates/emrac-cli/src/commands/config.rs` — new, `run()` for the six
  subcommands. `Show` pretty-prints the resolved `Config` back as TOML
  (`toml::to_string_pretty`).
- `crates/emrac-cli/src/commands/mod.rs` — register the module.
- `crates/emrac-cli/src/main.rs` — load `Config` once in `run()` right
  after `Sources::init()`, OR-merge it into each command's flags before
  dispatch, add the `Commands::Config` match arm (this one doesn't touch
  `Sources` at all).
- `crates/emrac-cli/src/commands/README.md`,
  `crates/emrac-core/src/README.md`, root `README.md` — update to mention
  `config` and close out the "Configuration: not started" note.
- `CHANGELOG.md` — new entry once committed.

## Verification

All host-safe — config is pure file I/O, doesn't touch pacman/AUR state,
no podman container needed:

- `emrac config show` with no file present — prints all-defaults TOML,
  doesn't create the file.
- `emrac config set flags.offline true`, then `emrac search ripgrep`
  (no `--offline` flag) — confirm it actually skips the AUR now.
- `emrac config set search.limit 5`, then `emrac search e` with no
  `--limit` — confirm 5 results; `emrac search e --limit 10` — confirm the
  flag still wins (raises above the config floor).
- `emrac config get search.limit` / `config unset search.limit` /
  `config get search.limit` again — confirm it round-trips and resets to
  unset.
- `emrac config set flags.offline nonsense` — confirm a clean
  `InvalidConfigValue` error, not a panic.
- Hand-edit the file with broken TOML, run `emrac config validate` —
  confirm the parse error (with line/column) is surfaced; run any other
  command — confirm it fails the same way rather than silently ignoring
  the broken file.
- `emrac config edit` with `$EDITOR=true` (a no-op editor) on a missing
  file — confirm it creates a valid empty-defaults file rather than
  erroring on "file not found."

`cargo build --workspace` and `cargo clippy --workspace` clean throughout.

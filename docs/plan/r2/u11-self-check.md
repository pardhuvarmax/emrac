# Slice u11 — `emrac self check`

First step of the self-management service in `docs/feats/self-service-feat.md` (its Phase 1,
§15): the `emrac self` namespace and its `check` command, a quick read-only look at emrac's own
installation. The other `self` commands (`sync`, `news`, `diagnose`, `up`/`upgrade`,
`dn`/`downgrade`) are **not** part of this slice and are not in the help: a command that is
listed must work.

## Why it must not need the rest of emrac

`doctor` looks at the system and needs libalpm. `self check` is for the day something about
emrac itself is wrong, which may be the day pacman or the config is broken. So it runs before
`Config::load` and `Sources::init`, reads files, and starts at most one process: `pacman -Qo` /
`-Q` / `-Qm`, which only query the database and whose absence is a finding, not a failure.

## What it checks (spec §3.4)

| Check | How |
|---|---|
| Executable | the running binary can be identified and opened; a ` (deleted)` mark means it was replaced while running |
| Version | the build's version is `x.y.z` |
| Installation | `pacman -Qo` names the owning package (and whether it is foreign: AUR or built locally); otherwise the place decides: `target/` is a development build, `~/.cargo/bin` is cargo, `/usr/local` or `~/.local/bin` is by hand; anything else is **UNKNOWN**, never guessed |
| Updating | what follows from the owner; at u11 it said emrac did not update itself yet, since `self upgrade` came in u14 it points at `emrac self upgrade` and says when a newer release is out |
| Configuration | `config.toml` parses, is not world-writable |
| Local state | the history reads and its chain verifies |
| Data folder | `~/.local/share/emrac` can be written |
| Operation journal | a cut-off transaction (u10) is a warning with `emrac history recover` |
| Ledger helper | `/usr/bin/emrac`, which `sudo emrac ledger-append` runs as root, is owned by root and not group- or world-writable (a problem otherwise, with the command to fix it) |

Not in this slice because there is nothing to check yet: release metadata and its cache, a
release source (there is no release channel; the spec's `sync`/`news` come with it).

## Output and exit codes

The same report as `doctor` (`ok`/`note`/`warning`/`PROBLEM`, a fix line for each finding,
`--json`), problems first. Exit 1 when a check found a problem, 0 otherwise. It repairs nothing.

## Code

- `crates/emrac-core/src/self_check.rs`: `SelfFacts`, `gather()`, `evaluate()` (pure), `Owner`,
  `classify_path`.
- `crates/emrac-cli/src/commands/self_cmd.rs`, `cli.rs` (`SelfCmd`/`SelfCommand`), `main.rs`.

## Verification

Unit tests for each check's status and fix (healthy, unknown owner, version mismatch, each
failure, missing helper); a CLI integration test (runs with no pacman state, `--json`, exit 1
on a broken config); a container check.

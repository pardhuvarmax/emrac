# Slice u14 — hardening ahead of r1

## Context

The core CLI loop is done and many commands sit on top of it, but the CLI crate
has one test, there is no CI, piping output into `head` panics, and exit codes
and getting started are undocumented. The CHANGELOG says `i1 → r1` ships once
the core loop is "complete and verified"; this slice is that verification work.
No new features.

## Design

- **Broken pipe**: restore the default `SIGPIPE` disposition at startup
  (`libc::signal`), so `emrac … | head` ends quietly like any Unix tool
  instead of panicking in `println!`.
- **Exit codes**, documented in the README and pinned by tests: `0` success
  (including "nothing to do" and a declined prompt), `1` any emrac error
  (including `verify` finding damage), `2` command-line usage error (clap).
- **CLI integration tests** (`crates/emrac-cli/tests/cli.rs`): run the real
  binary with a scratch `$HOME`, only through commands that don't need pacman
  or libalpm (`--help`, `--version`, `completion`, `config`, `history`,
  `snapshot list/show/delete`, `pin` list/`unpin`, usage errors), so they run
  anywhere, including CI. Covers stdout/stderr split, `--json`, error voice
  and exit codes.
- **CI** (`.github/workflows/ci.yml`): in an `archlinux` container (libalpm
  is needed to build): `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`.
- **README**: current status, a short quickstart, exit codes.
- After this, the label moves to r1 in the CHANGELOG.

Out of scope: tests that drive real pacman (those stay as the container
runs under `dev/container`), man pages, packaging.

# Changelog

All notable changes to Emrac are recorded here, newest first. Each entry lists the date, the commit it corresponds to, and what changed and why.

Emrac releases/updates are named **Slice \<N\>** (Slice 1, Slice 2, ...) rather than semver — each slice is one working, verified vertical increment of the tool (see `SPEC.md` Part X for the milestone philosophy this follows).

## Slice 1 — 2026-07-17 — `86b9ded` — Read-only search/info via libalpm

First working code. Added a Cargo workspace with two crates:

- **`emrac-core`** — the libalpm backend. Initializes an `Alpm` handle using `RootDir`/`DBPath`/`--repo-list` from `pacman-conf` (rather than hand-parsing `/etc/pacman.conf`), registers each official repo as a sync database, and exposes `search()`/`info()` over the local sync db cache.
- **`emrac-cli`** — the `emrac` binary. `clap`-derive CLI exposing `emrac search <query>` and `emrac info <pkg>`, with human-readable and `--json` output.

Notable implementation detail: the `alpm` crate's published bindings target libalpm v15.x, but this system runs libalpm 16.0.1 (a git/dev `pacman` build). Enabled the crate's `generate` feature so `alpm-sys` runs `bindgen` against the actually-installed headers at build time instead of using the mismatched pregenerated bindings.

Scope is deliberately narrow: official repos only, read-only (no network calls, no root required, no chroot needed). AUR, install/remove/upgrade, build profiles, and the TUI are not implemented yet.

Verified: `cargo build --workspace` succeeds, `search`/`info` return real data, looking up a nonexistent package exits cleanly (code 1, no panic), `cargo clippy --workspace` is clean, and none of it required elevated privileges.

## 2026-07-17 — `be0b34d` — Add Emrac specification, README, and gitignore

Initial commit. Added `SPEC.md`: a complete, internally-consistent specification consolidated from the original design brainstorm (`emrac.md`, kept local-only via `.gitignore`) — vision and philosophy, the full feature catalog, signature/advanced features, a command catalog, a completed formal grammar (EBNF), global and per-command option references, resolved-ambiguity editorial notes, implementation notes, and architecture/process flowcharts (Mermaid). Added `README.md` summarizing the project and pointing to `SPEC.md`.

No code yet at this point — design phase only.

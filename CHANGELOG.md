# Changelog

All notable changes to Emrac are recorded here, newest first. Each entry lists the date, the commit it corresponds to, and what changed and why.

Emrac uses **Slices** instead of semver, in three tiers that cycle per release:

- **Slice i\<N\>** — incubation for release `r<N>`: work-in-progress leading up to that release (code, docs, planning — not code-only).
- **Slice r\<N\>** — major release versions (`r1`, `r2`, ...).
- **Slice u\<N\>** — update cycle versions within a release; resets to `u1` at the start of each new `r<N>`.

The cycle repeats every release: `i1 → r1 → u1 → u2 → ... → i2 → r2 → u1 → u2 → ... → i3 → r3 → ...`. `i1 → r1` ships once the core CLI loop MVP (`search`/`info`/`install`/`remove`/`upgrade` against official repos + AUR, per `SPEC.md` Part X) is complete and verified; later `i<N> → r<N>` transitions are a per-release judgment call.

(See `SPEC.md` Part X for the milestone philosophy this follows.)

## Slice i1 — 2026-07-17 — `41eeddb` — Clean, actionable not-found errors

Bug found by hand-testing (`emrac install golly --dry-run`): pacman's raw stderr was leaking straight through as `pacman -S golly --print --print-format %n failed: error: target not found: golly` — exposing the underlying shelled-out command instead of saying anything useful.

`PacmanBackend` now recognizes pacman's `error: target not found: <name>` lines during resolution and translates them into dedicated errors: `install` points at the AUR (`package 'golly' not found in official repositories — try 'emrac search golly --aur' to check the AUR`), `remove` says the package just isn't installed. Genuinely unexpected pacman failures still fall back to the raw command/stderr, since that's still useful when it isn't a plain not-found case. Handles multiple missing packages in one command too (`packages not found in official repositories: golly, nonexistent-xyz`).

## Slice i1 — 2026-07-17 — `1cde3ae` — install/remove for official repos, with transaction preview

New `PacmanBackend` (`crates/emrac-core/src/backend/pacman.rs`) — the only place emrac shells out to `pacman`/`sudo pacman`. Dependency resolution (`pacman -S/-R --print`) needs no root and mutates nothing; execution runs under `sudo`, prompting interactively. emrac itself never runs as root, matching `yay`/`paru`.

`Plan`/`PlannedPackage`/`PlanAction` (`plan.rs`) model the transaction preview shown before anything executes — resolved package list (explicit vs. dependency), total download size, total installed-size delta. Sizes come from `AlpmBackend`'s libalpm FFI (already-synced local metadata), not parsed out of pacman's text output — `pacman --print-format %s` proved unreliable for size fields specifically during testing.

CLI: new `install`/`remove` commands, plus global `-y`/`--yes` and `--dry-run` (both already defined in `SPEC.md` Part VII, unused until now). `remove` supports `--cascade`/`--recursive`, passed straight through to pacman. The plan is always shown first; `--dry-run` stops there, otherwise a plain stdin prompt confirms unless `--yes`.

Added `dev/container/`: a disposable podman container with its own independent pacman database (never given access to the host's `/var/lib/pacman`) for testing install/remove for real, since preview needs no root but execution genuinely mutates system state. Verified a full install → remove cycle inside it, and confirmed the host's own `ripgrep` installation was untouched throughout.

Still official repos only — AUR building (`makepkg`) and `upgrade` are still deferred to later increments.

## Slice i1 — 2026-07-17 — `9eec4e7` — AUR search/info merged with official repos

Added `AurBackend` (`crates/emrac-core/src/backend/aur.rs`), querying the aurweb RPC v5 API (`https://aur.archlinux.org/rpc/`) over `ureq` — read-only, no root, no async runtime. Added the `Sources` aggregator (`sources.rs`) as the single entry point the CLI now calls instead of `AlpmBackend` directly: it merges official-repo and AUR results for `search`, and falls back official → AUR for `info`.

`PackageDetails` gained `maintainer`/`votes`/`popularity`/`out_of_date` (AUR-only, `None` for official packages); `installed_size` became `Option<u64>` (`None` for AUR packages, which carry no such metadata pre-build).

CLI: global `--offline` skips the AUR everywhere; `search` gained `--official`/`--aur` scope flags, with `--aur --offline` rejected via clap's `conflicts_with` rather than silently ignored. AUR failures degrade gracefully during `search` (official results still returned, warning on stderr) but are a hard error during `info` (a network failure means we genuinely can't answer about one specific package).

Verified live against the real AUR endpoint (not a mock): `search yay`/`search yay --official`, `info yay --json` (AUR fields populated), `info neovim --json` (regression check, official-only path unchanged), `info yay --offline` (clean not-found error), `search foo --offline --aur` (clap rejects the contradiction).

## Slice i1 — 2026-07-17 — `86b9ded` — Read-only search/info via libalpm

First working code. Added a Cargo workspace with two crates:

- **`emrac-core`** — the libalpm backend. Initializes an `Alpm` handle using `RootDir`/`DBPath`/`--repo-list` from `pacman-conf` (rather than hand-parsing `/etc/pacman.conf`), registers each official repo as a sync database, and exposes `search()`/`info()` over the local sync db cache.
- **`emrac-cli`** — the `emrac` binary. `clap`-derive CLI exposing `emrac search <query>` and `emrac info <pkg>`, with human-readable and `--json` output.

Notable implementation detail: the `alpm` crate's published bindings target libalpm v15.x, but this system runs libalpm 16.0.1 (a git/dev `pacman` build). Enabled the crate's `generate` feature so `alpm-sys` runs `bindgen` against the actually-installed headers at build time instead of using the mismatched pregenerated bindings.

Scope is deliberately narrow: official repos only, read-only (no network calls, no root required, no chroot needed). AUR, install/remove/upgrade, build profiles, and the TUI are not implemented yet.

Verified: `cargo build --workspace` succeeds, `search`/`info` return real data, looking up a nonexistent package exits cleanly (code 1, no panic), `cargo clippy --workspace` is clean, and none of it required elevated privileges.

## 2026-07-17 — `be0b34d` — Add Emrac specification, README, and gitignore

Initial commit. Added `SPEC.md`: a complete, internally-consistent specification consolidated from the original design brainstorm (`emrac.md`, kept local-only via `.gitignore`) — vision and philosophy, the full feature catalog, signature/advanced features, a command catalog, a completed formal grammar (EBNF), global and per-command option references, resolved-ambiguity editorial notes, implementation notes, and architecture/process flowcharts (Mermaid). Added `README.md` summarizing the project and pointing to `SPEC.md`.

No code yet at this point — design phase only.

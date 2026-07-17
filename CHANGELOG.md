# Changelog

All notable changes to Emrac are recorded here, newest first. Each entry lists the date, the commit it corresponds to, and what changed and why.

Emrac uses **Slices** instead of semver, in three tiers that cycle per release:

- **Slice i\<N\>** — incubation for release `r<N>`: work-in-progress leading up to that release (code, docs, planning — not code-only).
- **Slice r\<N\>** — major release versions (`r1`, `r2`, ...).
- **Slice u\<N\>** — update cycle versions within a release; resets to `u1` at the start of each new `r<N>`.

The cycle repeats every release: `i1 → r1 → u1 → u2 → ... → i2 → r2 → u1 → u2 → ... → i3 → r3 → ...`. `i1 → r1` ships once the core CLI loop MVP (`search`/`info`/`install`/`remove`/`upgrade` against official repos + AUR, per `SPEC.md` Part X) is complete and verified; later `i<N> → r<N>` transitions are a per-release judgment call.

(See `SPEC.md` Part X for the milestone philosophy this follows.)

## Slice i1 — 2026-07-17 — `6c1d2b2` — AUR building for `install`

`install` now handles the AUR, not just official repos, closing the gap `backend/README.md` had flagged since the install/remove increment. New `AurBuildBackend` (`crates/emrac-core/src/backend/aur_build.rs`) is the only module that shells out to `git`/`makepkg`: each AUR package gets a persistent checkout under `~/.cache/emrac/build/<pkg>`, reused across runs so a rebuild only needs reviewing what changed rather than the whole PKGBUILD again. Building always runs as the invoking user (never root, matching the existing privilege model) via `makepkg -si`, which syncs any missing *official* dependencies itself through `pacman` — an AUR-only transitive dependency isn't resolved automatically and surfaces as a clear build failure instead, a deliberate scope cut for this increment (same philosophy as the deferred conflict/disk-space checks in the install/remove increment).

Since building means running a PKGBUILD's own shell code locally, `install` shows it by default before building: the full file on a first build, or a diff against the last build on a rebuild, each gated behind its own `emrac asks:` confirmation distinct from the overall install confirm — declining aborts the whole install, no partial-install-the-rest. Two opt-outs: `--skip-pkgbuild` (skip the review and its confirm entirely) and `--skip-diff` (show the full new file instead of a diff on rebuilds).

Also renamed `Error::PacmanSpawn`/`Error::PacmanFailed` to `Error::CommandSpawn`/`Error::CommandFailed` — they were already program-name-generic, just misleadingly named for a variant now shared with `git`/`makepkg` failures.

Verified inside the podman container (dev/container/, now with `git`/`base-devel` added): a first build of a real AUR package (`downgrade`) showed the full PKGBUILD, required the dedicated confirm, declined-then-accepted correctly, built and installed via `makepkg -si` (which pulled its own official deps through `pacman` as expected); removing and reinstalling with no upstream changes correctly reported up-to-date and skipped straight to the overall confirm; `--skip-pkgbuild --yes` ran with zero prompts. Host's own (unrelated) `downgrade` installation was confirmed unchanged throughout, and no build cache was ever created on the host.

## Slice i1 — 2026-07-17 — `fb43285` — Conversational messages, voiced by category

Follow-up to the not-found fix below, after more hand-testing feedback: every user-facing message should read like an actual explanation, not terse technical text, and different message *types* should sound different rather than one generic prefix everywhere.

Landed in two passes: first a blanket `"emrac says: {err}"` wrapper in `main.rs`, then split into a real per-category voice — `emrac says:` for genuine failures, `emrac found:` for lookup outcomes (including not-found), `emrac warns:` for non-fatal AUR degradation, `emrac notes:` for neutral status (aborted, nothing to do). That second pass introduced a real bug: baking the prefix into each error's own `Display` meant the AUR warning in `search.rs` (which reuses an error's `.to_string()` as its own message) doubled up into `"emrac warns: emrac says: ..."`.

Fixed by moving the voicing decision out of `Display` entirely and into a new `Error::voice()` method (`emrac-core/src/error.rs`), so the same underlying message text can be reused in different contexts without dragging a prefix along. `main.rs` now matches on the concrete error and picks the verb via `err.voice()` — which also meant dropping `anyhow` from `emrac-cli` altogether, since every error the CLI ever produces was already `emrac_core::Error` under the hood and didn't need anyhow's type erasure. Also rewrote the actual message text to read naturally and suggest a next step wherever there's a sensible one (`"couldn't find 'golly' in the official repositories — want to try `emrac search golly --aur` to check the AUR?"`).

## Slice i1 — 2026-07-17 — `8d4fdf9` — Clean, actionable not-found errors

Bug found by hand-testing (`emrac install golly --dry-run`): pacman's raw stderr was leaking straight through as `pacman -S golly --print --print-format %n failed: error: target not found: golly` — exposing the underlying shelled-out command instead of saying anything useful.

`PacmanBackend` now recognizes pacman's `error: target not found: <name>` lines during resolution and translates them into dedicated errors: `install` points at the AUR, `remove` says the package just isn't installed. Genuinely unexpected pacman failures still fall back to the raw command/stderr, since that's still useful when it isn't a plain not-found case. Handles multiple missing packages in one command too.

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

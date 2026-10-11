# Changelog

All notable changes to Emrac are recorded here, newest first. Each entry lists the date, the commit it corresponds to, and what changed and why.

Emrac uses **Slices** instead of semver, in three tiers that cycle per release:

- **Slice i\<N\>** — incubation for release `r<N>`: work-in-progress leading up to that release (code, docs, planning — not code-only).
- **Slice r\<N\>** — major release versions (`r1`, `r2`, ...).
- **Slice u\<N\>** — update cycle versions within a release; resets to `u1` at the start of each new `r<N>`.

The cycle repeats every release: `i1 → r1 → u1 → u2 → ... → i2 → r2 → u1 → u2 → ... → i3 → r3 → ...`. `i1 → r1` ships once the core CLI loop MVP (`search`/`info`/`install`/`remove`/`upgrade` against official repos + AUR, per `SPEC.md` Part X) is complete and verified; later `i<N> → r<N>` transitions are a per-release judgment call.

(See `SPEC.md` Part X for the milestone philosophy this follows.)

## Slice u8 — 2026-10-11 — purge

New `emrac purge <pkg>... [--cascade] [--recursive]`: `emrac remove` with `pacman -Rn`. On Arch the thing `remove` "leaves behind" is the `.pacsave` copy pacman makes of a package's *modified* `backup=()` config files; `purge` deletes them instead. Resolution, `--dry-run`, `--yes` and `--json` are exactly `remove`'s. What's new is the safety net: before anything happens each removed package's config files are checked against the md5 recorded in the local db, and every file that was modified (or couldn't be read — root-only files can't be checked by an unprivileged user, so they are treated as possibly modified) is listed by path and package, with a dedicated `emrac asks:` confirmation separate from the overall one when any exist; `--yes` answers both. Nothing at risk, no extra prompt. `remove` is unchanged and already is the "keep config" behaviour, so the spec's `--keep-config` is not a flag. Files under `$HOME` (pacman has no record of them) and `.pacsave` files left by *earlier* removals (the package is gone from the db) are deliberately not touched. Design in `docs/plan/u8-purge.md`.

Logged as a new `purge` transaction. `undo` reinstalls the packages like it does for a `remove`, and its preview says the deleted configuration files stay deleted.

Verified with 4 new unit tests (config state classification, at-risk rules, hashing against a temp root incl. md5 of a known file / modified / missing, rollback of a purge with its note) and in the podman container with real pacman: `remove` of `nano` with an edited `/etc/nanorc` left `/etc/nanorc.pacsave`; `purge --dry-run` listed the edited file and listed nothing for an unmodified one; declining the second confirmation removed nothing (`nano` and `/etc/nanorc` intact); accepting removed `nano` with no `.pacsave` left; `--yes` did it in one shot; history shows `purge` and `undo --dry-run` shows the note. On the host, read-only: a real package with root-only configs listed them as "couldn't be read", an unconfigured package listed none, and `--json` carries `config_files`.

## Slice u7 — 2026-10-11 — reinstall

New `emrac reinstall <pkg>...` puts the **installed version** of each named package back (repair a damaged or deleted file, re-run install scripts). It deliberately never reinstalls the repositories' *newer* version: a plain `pacman -S pkg` would turn a reinstall into a partial upgrade, so each package picks a source in order — a package file for exactly the installed version already on disk (`pacman -U`), else the repositories if they still carry that version (`pacman -S`), else the Arch Linux Archive (downloaded after confirmation, then `-U`), and for an AUR package a rebuild (`makepkg -si`) of the existing, already-reviewed checkout, with no new PKGBUILD fetched. If a package can't be had at the installed version the whole command aborts before prompting, naming each package and why (including "the Archive wasn't checked (--offline)"); names that aren't installed are listed together and pointed at `install`. The preview shows each package's source; `--dry-run`, `--yes`, `--json`, `--offline` behave as elsewhere. Build options from the spec (`--source`, `--clean`, `--profile`, ...) need the build system and are not part of this slice. Design in `docs/plan/u7-reinstall.md`.

Logged as a new `reinstall` transaction. Since no version changes, `emrac rollback tx-N` on one is refused with that explanation and a bare `undo` skips reinstall records (it undoes the last transaction that actually changed something).

Verified with 7 new unit tests (source choice for every case incl. the Archive not being consulted unless needed or allowed, rollback refusal, `undo` skipping) and in the podman container with real pacman: deleting `/usr/bin/tree` and reinstalling repaired it from the cached file; after `pacman -Scc` it repaired from the repositories; with an old `tree` installed, the repo ahead and the cache wiped, the reinstall came from the Archive (and `--offline` aborted with the reason instead of upgrading), a multi-package run mixed sources, an unknown name aborted the lot, an AUR package (`downgrade`) whose built files were removed was rebuilt from its checkout, the history shows `reinstall` entries, `undo` targeted the earlier install, and `rollback` refused the reinstall. On the host, read-only: `--dry-run` for repository/file/AUR-without-checkout/unknown cases and `--json`.

## Slice u6 — 2026-10-11 — downgrade

New `emrac downgrade <pkg> [version]` goes back to an older version of an installed package, and `emrac downgrade <pkg> --list` shows what's available (newest first, with whether each version is `local`, `archive` or both, and which is installed). `SPEC.md` lists downgrades as a feature but defines no command, so the shape is new: one package per call; the target must compare older than what's installed (libalpm `vercmp`, so `1.10` > `1.9` and epochs are right) — equal or newer is refused and points at `upgrade`/`install`; without a version it picks the newest version older than the installed one, from the union of files on this machine and the Arch Linux Archive listing (local wins, so nothing is downloaded needlessly). It is the u4/u5 machinery exposed directly: same preview (`downgrade 1 package`, `from 2.3.2-1`, "Will download from the Arch Linux Archive"), `--dry-run`/`--yes`/`--json`/`--offline`, same fetch + signature-checked `sudo pacman -U`, and it is logged as a normal `downgrade` transaction with the previous version recorded — so `emrac undo` reverses it (as an upgrade) and `history` shows it. After success one note says the next `emrac upgrade` will bring the package back and that `IgnorePkg` holds it; emrac does not edit `pacman.conf`. AUR (foreign) packages work from local files only. Design in `docs/plan/u6-downgrade.md`.

Also: package versions are now parsed out of `<name>-<pkgver>-<pkgrel>-<arch>.pkg.tar.*` file names (a pkgver never contains `-`, so `foo` doesn't claim `foo-bar` files; `.sig`/`.part` ignored), shared by the local scan and the Archive listing.

Verified with 6 new unit tests (file-name version parsing, newest-older choice incl. `1.10` vs `1.9` and epochs/pkgrel, nothing-older error, explicit version must be older, merged list order/sources/installed marker). On the host, read-only: `--list` (13 real versions), one-step and explicit `--dry-run`, newer/equal/not-installed refusals, `--offline` with no local older file, `--json`, and nothing written. In the podman container with real pacman: one-step downgrade of `tree` 2.3.2-1 → 2.3.1-1 and an explicit one to 1.8.0-2 (both fetched from the Archive), `undo` of a downgrade upgraded it back, a repeat downgrade then found the file locally with no download section, `--list --offline` showed local files only, an unavailable version was reported, and history shows the chain. A very old `readline` downgrade was rejected by pacman (`invalid or corrupted package (PGP signature)` — the 2020 signing key isn't trusted) and recorded as a `failed` downgrade with nothing installed. Not exercised: pacman refusing a downgrade because a dependent requires the newer version (pacman's own check; the failed-record path is the same one the signature failure used).

Not built: multi-package downgrades, `reinstall`, holding/pinning a package, an interactive version picker.

## Slice u5 — 2026-10-11 — Arch Linux Archive fetch for rollback

`rollback`/`undo`/`redo` no longer need the package file to still be on disk: when a needed official-package version isn't in pacman's cache, the AUR build dir or emrac's own archive cache, it is looked up in the Arch Linux Archive (`archive.archlinux.org`) and downloaded after you confirm. Planning stays free of side effects: it only makes read-only GETs of the package's Archive directory listing, finds the exact `<name>-<version>-<arch>.pkg.tar.*` file (same exact-shape matcher as the local scan; `%3A`-encoded epochs decoded; `.zst` preferred; `.sig` ignored), and the preview gains a "Will download from the Arch Linux Archive" section (also in `--json` as `downloads`). Files land in `~/.cache/emrac/archive/<pkg>/` via a `.part` file renamed on success, are reused by later rollbacks, and the detached `.sig` is fetched beside each package (best effort). AUR packages are never looked up there. `--offline` turns the Archive off entirely, restoring u4's "files missing" abort. A package the Archive doesn't have counts as missing (aggregated, aborts before prompting); an Archive that can't be reached is a hard `Archive` error rather than a claim that the version doesn't exist. Design in `docs/plan/u5-archive-fetch.md`.

Trust: HTTPS plus pacman's own signature check — `pacman -U` verifies the `.sig` beside a file against your keyring. Verified by flipping a byte in a downloaded package: pacman refused it (`invalid or corrupted package (PGP signature)`), nothing was installed and the failure was recorded as a `failed` rollback.

Verified with 9 new unit tests (listing hrefs + percent decoding incl. `%3A`/`%2B`, exact/preferred file selection, URL building and path-injection refusal, and `plan_rollback` using the Archive only for locally-missing official files, never for AUR, never for removals, and surfacing an unreachable Archive) and, read-only on the host, a real listing lookup that produced the download plan and wrote nothing to disk. In the podman container with real pacman: after `pacman -Scc`, `undo --dry-run` showed the download plan, `--offline` aborted naming the file, the real `undo` downloaded `tree 1.8.0-2` (+ `.sig`) and downgraded, `redo` fetched `2.3.2-1` the same way, and a later `undo --dry-run` found the file in the archive cache and showed no download.

Not built: an `emrac downgrade <pkg> [version]` command (it would reuse this), Archive mirrors, resuming partial downloads, progress output.

## Slice u4 — 2026-10-11 — redo

`emrac redo` re-applies the most recent rollback. It is the same code path as `rollback`: reversing a rollback record is a redo, so `rollback_of` now reads as an *active* reversal (a committed rollback that hasn't itself been reversed), and reversing an undo makes the original transaction rollbackable again — undo/redo/undo/redo toggles consistently, and a second `redo` in a row reports there is nothing to redo instead of undoing the first. A downgrade record's inverse is an upgrade (from the newer file the original upgrade left in the pacman cache). `emrac rollback tx-<id of a rollback>` does the same as `redo` for that one. Output reads `redo tx-1 (by reversing tx-2)`; a rollback attempt that failed is neither redoable nor counts as reversing, so re-running `undo` finishes it. Design addendum in `docs/plan/u4-rollback.md`.

Verified with 4 new unit tests (redo of an undo, redo of a downgrade incl. the package-moved-on skip, the undo/redo/undo candidate toggle, failed rollbacks ignored) and in the podman container with real pacman: upgrade → undo (downgrade) → redo (upgrade) → redo again (nothing to redo) → undo → redo, plus install ripgrep → undo → redo; `redo --dry-run` changed nothing; `history` shows the chain.

## Slice u4 — 2026-10-11 — rollback and undo

New `emrac rollback [tx-N]` and `emrac undo` reverse a recorded transaction: an `install` is undone by removing what it installed (`pacman -R`), a `remove` by reinstalling the recorded version, an `upgrade` by downgrading to the previous version (both via `pacman -U` on files already on disk — pacman's `CacheDir` and `~/.cache/emrac/build/<pkg>`; nothing is downloaded). A bare `rollback` and `undo` pick the most recent transaction that is neither a rollback nor already rolled back. Honors `--dry-run`, `--yes` and `--json`.

Safety rules: a package is only touched if it is still as the transaction left it (same version after an install/upgrade, absent after a remove); everything else is listed as left alone with the reason, which also makes rolling back a *failed* transaction reverse exactly the part that landed. If any needed package file is missing the whole rollback aborts before prompting and names every one. Rollbacks are themselves logged (`rollback_of`), `emrac history` shows `rollback of`/`rolled back by`, and rolling back a transaction twice, is refused (reversing a rollback is `redo`, added next). Undoing an install does not cascade: if something now depends on the packages, pacman refuses and the failure is recorded. AUR versions are not compared for the post-state check since RPC metadata can differ from what a VCS package's build produced.

Schema: `PlannedPackage.previous_version` (filled for upgrades) and `TransactionRecord.rollback_of` are new, both optional so u3 logs still parse. Upgrade records written by u3 lack the previous version and are refused with an explanation; old install/remove records work. New `PlanAction::Downgrade`. Design in `docs/plan/u4-rollback.md`.

Verified with 16 new unit tests (inverse for each action, post-state skips, failed-partial, AUR version handling, missing files aggregated, old-format and repeat refusals, candidate ordering, id parsing, exact package-file matching) and inside the podman container with real pacman: upgrade of an Archive-fetched old `tree` then `undo` restored 1.8.0-2 (file found in the AUR build dir); a second `undo` and `rollback tx-1` were refused; `remove` then `rollback` reinstalled from the pacman cache; an `install` of `ripgrep` was undone; rolling back an upgrade whose package was since removed was skipped with a reason; after `pacman -Scc` a rollback aborted naming `tree 2.3.2-1`; `--dry-run` recorded nothing; `history` shows the markers. Not covered by a live test: a rollback of a `failed` transaction (unit-tested only) and `redo`.

## Slice u3 — 2026-10-11 — transaction history

Executed `install`/`remove`/`upgrade` transactions are now recorded in an append-only JSON Lines log at `~/.local/share/emrac/history.jsonl`, and `emrac history` (alias `emrac transactions`, per `SPEC.md`) shows them newest-first with `--limit N`, `--since YYYY-MM-DD` and `--json`. Each record holds an id (`tx-N`), a UTC timestamp, the action, the planned packages, a `committed`/`failed` status and, on failure, the error text. This is groundwork for `rollback`/`undo`/resume, which are not built yet.

Decisions: only transactions that actually ran are logged (dry runs, declined confirmations and empty plans are not); a failed history write only warns on stderr and never changes the transaction's own result; a record is the *plan* that was executed, so a run that failed halfway (official part done, AUR build failed) is one `failed` record rather than per-package status; reading skips unparsable lines so one bad line can't brick the command. Dates use a small tested civil-date conversion instead of adding `chrono`; all times are UTC. Design in `docs/plan/u3-history.md`.

Verified on the host with a scratch `$HOME`: empty history, `--json`, `--limit`, `--since` (including a day with no matches), invalid dates like `2026-02-30` rejected, a hand-written log containing a corrupt line, and `install --dry-run` writing nothing. 10 new unit tests cover the log and date logic.

Verified inside the podman container (real execution): a real `install` and `remove` of `ripgrep` each wrote a `committed` record (`tx-1`, `tx-2`, correct JSON incl. `--json --limit 1`), `remove --dry-run` wrote nothing, and with a stale `/var/lib/pacman/db.lck` forced, `install` failed in `sudo pacman` and wrote a `FAILED` record with the error text while the command still reported its own error. (A removal pacman rejects at the `--print` planning stage, e.g. `remove ncurses`, never executes and so is correctly not recorded.)

Build note: the debug binary was linked against nix's glibc loader because `~/.nix-profile/bin/cargo` precedes `/usr/bin` on this machine's `PATH`, so it can't run in the Arch container or even directly on the host. Build with `PATH=/usr/bin /usr/bin/cargo build` for a binary with the standard `/lib64` loader.

## Slice u2 — 2026-10-11 — shell completion

New `emrac completion bash|zsh|fish` prints a completion script to stdout (`emrac completion zsh > ~/.zsh/completions/_emrac`), generated by `clap_complete` from the real `Cli` definition so it can't drift from the actual flags and subcommands. The shell argument is our own `ValueEnum` limited to the three shells `SPEC.md` promises, so `powershell`/`elvish` are rejected by clap rather than silently supported. Dispatched before `Sources::init()`, so it needs no libalpm setup. Static only: no dynamic package-name completion, and `emrac alias`/`integrate` stay unbuilt. Design in `docs/plan/u2-completion.md`. `Cargo.lock` also moved clap's patch versions forward when `clap_complete` was added.

Verified: all three shells exit 0 with non-empty scripts; `powershell` exits 2 with a clean error; `bash -n`, `zsh -n` and `fish --no-execute` accept their scripts; a unit test asserts each script mentions every top-level subcommand.

## Slice u1 — 2026-10-11 — config file

New `emrac config` command group (`show`/`get`/`set`/`unset`/`edit`/`validate`) backed by `~/.config/emrac/config.toml` (new `emrac_core::Config`, `toml` dependency). It persists defaults for flags that already exist: `[flags]` (`offline`, `yes`, `dry_run`, `json`, `quiet`), `[review]` (`skip_pkgbuild`, `skip_diff`) and `[search]` (`limit`, `official`, `aur`) — a fixed 10-key allowlist, type-checked on `set`. A missing file means all defaults; a file that exists but doesn't parse fails every command with the TOML line/column rather than being silently ignored.

Merge is OR, not override: every setting is a bool flag that can only turn behavior on (clap can't tell "not passed" from "false"), so the effective value is `cli || config`, and `search.limit` is `cli.or(config)`. `config` is dispatched before `Sources::init()`, so it works without a pacman setup. Design in `docs/plan/u1-config.md`; descoped `config reset`/`export`/`import` (the file is already portable). `FEATURE_STATUS.rst` #19 moves to Partial.

Verified on the host against a scratch `$HOME` (file I/O only, no pacman state touched): `show` with no file doesn't create one; `set`/`get`/`unset` round-trip; bad value and unknown key give clean errors; broken TOML is reported by `validate` and by other commands; `edit` with a no-op editor creates a valid defaults file; `flags.offline` and `search.limit` set in config take effect on `search`, and `--limit` still raises above the config floor.

## Slice i1 — 2026-07-17 — `db75c7a` — `upgrade`, completing the core CLI loop milestone

Last piece of the `i1 → r1` milestone bar (`search`/`info`/`install`/`remove`/`upgrade` against official repos + AUR, `SPEC.md` Part X). Built almost entirely by reusing what `install` already had rather than a parallel implementation: the AUR PKGBUILD/diff review flow (`review_aur_package`, extracted from `commands/install.rs` into shared `commands/aur_review.rs`, since "upgrade an AUR package" and "build it for the first time" are literally the same `AurBuildBackend::sync` outcome check) and `install_official`/`build_aur` execution paths.

Official packages: `emrac upgrade` (no args) runs a real combined `sudo pacman -Syu`; `emrac upgrade <pkg>` reuses the same `pacman -S <pkg>` path `install` already uses (accepted quirk inherited from pacman: this resolves regardless of whether a newer version is actually available). AUR packages: full-upgrade enumerates installed "foreign" packages via new `AlpmBackend::foreign_package_names` (local db packages absent from every sync db — the `pacman -Qm` definition, done via libalpm instead of a subprocess), batch-checks their latest versions in one AUR RPC call (new `AurBackend::info_multi`, repeated `arg[]=` params), and compares with the newly-confirmed-exported `alpm::vercmp`; only the ones actually behind go through the review/build sequence.

Found and fixed two bugs during host verification before this ever touched the container:
- `man pacman`'s wording ("`--print`: only print the targets instead of performing the actual operation (sync, remove or upgrade)") suggested `-Syu --print` would stay root-free like `-S <pkg> --print` already is. It doesn't — `-y`/`--refresh` enforces its root check even under `--print`, confirmed live (`pacman -Syu --print ...` → `you cannot perform this operation unless you are root`). Fixed by dropping `-y` from the preview (`resolve_full_upgrade` now runs `-Su --print`, working off whatever's already cached — same "may be a little stale" caveat every other preview already carries); the real upgrade still does a genuine `-Syu`.
- A named package that isn't installed was reusing `remove`'s `PackageNotInstalled` error, so `emrac upgrade nonexistent` said *"nothing to **remove**"* — wrong verb entirely. New dedicated `Error::PackageNotInstalledForUpgrade`, pointing at `install` instead.

`Plan` gained a third `PlanAction::Upgrade`; sizing needed its own small assembly step in `sources.rs` (`official_upgrade_packages`) since upgrade needs the *net* delta (new size − old size, can be negative) rather than install/remove's flat sign-based approach. A full upgrade degrades gracefully if the AUR is unreachable (`UpgradePlan.aur_warning`, same precedent as `search`, not `info`'s hard-fail) — a flaky connection shouldn't block `pacman -Syu` from running; a targeted AUR upgrade still hard-fails, same as targeted `install`/`info`.

Verified on the host (dry-run only): full upgrade correctly listed real outdated AUR packages already on this machine; targeted official and targeted AUR upgrades previewed correctly; not-installed now gives the correct upgrade-specific message.

Verified inside the podman container (real execution): installed an AUR package, then artificially rewound its *local git checkout* (not the installed version) by one commit to force a divergence. A full upgrade correctly reported nothing to do — full-upgrade's AUR check compares the *actually installed* version against the AUR's latest, which hadn't diverged, so this was the right call, not a miss. A **targeted** upgrade of that same package (`emrac upgrade <pkg>`) doesn't consult installed-version freshness at all — it just re-syncs, which did see the rewound checkout diverge from upstream, and for the first time exercised the `Changed` review path end-to-end: a real PKGBUILD diff was printed, the dedicated confirm gated it, and accepting rebuilt and reinstalled correctly. Also verified a targeted official upgrade (`pacman-contrib`) and confirmed the host's own `downgrade`/`pacman-contrib` installations were untouched throughout.

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

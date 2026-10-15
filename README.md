# Emrac

<img align="left" width="250" src="/assets/emrac.png" alt="emrac">

A hybrid next generation package management platform for Arch Linux — an orchestration and UX layer over `pacman`, `makepkg`, `libalpm`, PKGBUILD, and the AUR, unifying official repos, the AUR, and local/custom repos behind one CLI and TUI.

---

Status: **Slice r2** (released 2026-10-14; the release notes are in [`docs/releases/r2.md`](./docs/releases/r2.md)). The core loop (`search`, `info`, `install`, `remove`, `upgrade` against the official repos and the AUR) works, with a transaction preview and confirmation before every change, audited and sandboxed AUR builds, history with rollback, health checks, and tools for config files and caches (see Highlights). Still missing: the TUI, build profiles and the source-or-binary recommendation, and most of `SPEC.md` Part IV.

| Area | Commands |
|---|---|
| Find & inspect | `search`, `info`, `deps`, `rdeps`, `why`, `orphans` |
| Change | `install`, `remove`, `purge`, `upgrade`, `reinstall`, `downgrade`, `build` (AUR/PKGBUILD dir in the sandbox, no install) |
| Undo | `history` (`archive`, `verify`, `ack`, `sync`, `anchor`), `ledger` (`events`, `watch install`), `rollback [tx]` / `undo`, `redo`, `snapshot`, `restore <name>` |
| Protect | `doctor` (read-only system health checks), `audit` (read a PKGBUILD or inspect a built package for risky code; runs automatically before AUR builds and again on the built package before it is installed), `pin` / `unpin` (holds with a reason and expiry), `verify` (files vs. pacman's record), `cache` (sizes of the source, build, binary and pacman caches; `clean`, `trim`, `purge` keep what `undo` needs), `pacnew` (tells your edits from upstream's in `.pacnew` files, then keeps, takes, merges or edits each after checking it; every change is recorded and revertible) |
| Setup | `config`, `completion bash\|zsh\|fish` |

Global flags: `--dry-run`, `--yes`, `--offline`, `--json`, `--quiet`. Defaults can be set with `emrac config`.

### Quickstart

```sh
emrac search ripgrep            # official repos + AUR
emrac install ripgrep           # preview, confirm, install
emrac snapshot                  # save the installed set as a restore point
emrac upgrade                   # full upgrade (with `snapshot.before_upgrade`, snapshots first)
emrac undo                      # reverse the last transaction
emrac verify ripgrep            # are its files still intact?
```

Every mutating command supports `--dry-run`, and the first thing it does is show you the plan.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success, including "nothing to do" and a declined confirmation |
| `1` | emrac reported an error (including `verify` finding damaged files) |
| `2` | Command-line usage error (unknown command, missing argument, bad value) |

emrac says what kind of statement each line is with a verb, so you can tell how sure it is and what to do about it:

| Line begins | Means |
|---|---|
| `emrac found:` | a fact it discovered that you can act on (also the heading of a result) |
| `emrac says:` | something failed underneath emrac, not your doing |
| `emrac plans:` / `emrac asks:` | what it is about to do / a question that needs your answer |
| `emrac notes:` | plain information |
| `emrac warns:` | a real risk |
| `emrac suspects:` | a conclusion from weak evidence; it may be wrong |
| `emrac thinks:` | a judgment with its reasons; good evidence, not proof |
| `emrac suggests:` | the next step, usually a command to run |
| `emrac refuses:` | a refusal that nothing overrides (a critical finding, or no answer at all) |
| `emrac confirms:` | something was checked or done, and holds |
| `emrac remembers:` | something recalled from what you did before |

The verb is coloured on a terminal (never when piped, with `NO_COLOR` set, or under `TERM=dumb`). Problems and doubts (`says`, `warns`, `suspects`, `refuses`) go to stderr; results and information go to stdout; `--json` is unchanged.

Errors go to stderr as `emrac found: …` (something you can fix) or `emrac says: …` (something failed underneath); normal output, including `--json`, goes to stdout.

> Emrac uses "Slice i/r/u\<N\>" instead of semver — see `CHANGELOG.md`.

---


## Highlights

- **Emrac is a hybrid package manager.** It builds from PKGBUILD locally when it's worthwhile, installs binaries when it isn't, and tells you why.
- **Plans that match what pacman will do.** The upgrade preview is built from repositories synced just now, and it lists what conflicts and replacements will also remove and which provider pacman will pick, so what you confirm is what happens.
- **AUR builds you can trust a little more.** Each package is reviewed (PKGBUILD or diff), audited for risky code, built in a bubblewrap sandbox (read-only root, empty home, no network unless you allow it, memory, process and time limits), and its built package is inspected before it is installed. Findings are info, warning, high (needs an explicit answer that `--yes` does not give) or critical (refused).
- **Silence is never consent.** pacman asks its own questions unless you pass `--yes`, and a closed terminal or empty stdin is always a no.
- **A history you can check.** Every record links to the one before it by SHA-256, so `history verify` finds the line where one was edited, removed, added or moved. `history archive --older-than 90d` moves old transactions into archive files (never the newest 20, never one a kept rollback names), `history ack` marks a failure as seen without deleting it. This catches accidents and casual edits; someone who can rewrite the whole file can rebuild the chain, which is what the planned root-owned ledger is for.
- **A second copy you cannot edit.** After each change emrac copies the new history lines to `/var/lib/emrac/ledger.jsonl`, a root-owned append-only file (a plain text file anyone can `cat`), through `sudo emrac ledger-append`, which checks every line and does nothing else. `history verify` compares the two, names a record that was changed or deleted, and `history verify --repair` rebuilds your history from the ledger. It stops you, a script or malware without root; it does not stop root, who can lift the flag. `sudo emrac ledger watch install` adds kernel audit rules that record who else touches it (`emrac ledger events`, and `doctor` reports it), and `history anchor` prints a short code to keep elsewhere so that `history verify --against` can show a rewrite or a cut-off tail. What each layer does and does not give is in [`docs/design/ledger.md`](./docs/design/ledger.md). Without sudo emrac carries on and `history sync` catches the ledger up.
- **A cut-off upgrade is not lost.** emrac writes a journal before it hands a plan to pacman and holds a lock on it. After a power loss or a killed terminal, the next run (or `emrac history recover`) reads pacman's log, says which packages were done and which were not reached, and puts it in your history. Design in [`docs/design/journal.md`](./docs/design/journal.md).
- **A check of emrac itself.** `emrac self check` looks at how emrac was installed, its configuration, its history and journal, and the root-run ledger helper, without needing pacman, the network or root.
- **It can update itself, through pacman.** `emrac self upgrade` and `self downgrade` fetch a release named in the checked index, verify its SHA-256, build a package and install it with `sudo pacman -U`; they never touch a copy a package doesn't own, take a backup first, and a cut-off run is found next time. Design in [`docs/design/self-upgrade.md`](./docs/design/self-upgrade.md).
- **It knows its own releases.** `emrac self sync` keeps a checked copy of the project's release index (optionally verified against a gpg key you trust) and `emrac self news` shows what is new, from that copy; nothing is updated behind your back. Design in [`docs/design/release-index.md`](./docs/design/release-index.md).
- **Undo that works after a real upgrade.** `history`, `undo` / `redo`, `downgrade`, `reinstall`, `snapshot` and `restore` reverse changes using files from the pacman cache, the AUR build folder, the repositories or the Arch Linux Archive, after checking the system is still in the state they expect.
- **Conflicts are explained, not dumped.** When an AUR build stops because one package conflicts with another that something still requires, emrac says which, why, and whether the swap looks safe, and offers it (default no).
- **`.pacnew` files without guesswork.** `emrac pacnew` compares the file the old package shipped, your file and the new one, so your edits are told from upstream's. It merges what is safe, checks the result before applying it (sudoers, sshd_config, pacman.conf, fstab, the mirrorlist, JSON, shell-sourced files), saves both files, records every change and can revert it.
- **Caches you can trim safely.** `emrac cache` shows the source, build, binary and pacman caches; `clean` keeps what `undo` needs, and `--lean` keeps one rollback set instead of every file pacman downloaded.
- **Health checks with a fix for each finding.** `emrac doctor` checks dependencies, databases, pending upgrades against fresh repositories, the running kernel, disk, caches, orphans, `.pacnew` files, failed units, pins and the sandbox, and can write a sanitized bug report.
- **Holds that remember why.** `pin` holds a package back from `upgrade` with a reason and an expiry date.
- **Tested against a real pacman.** 271 tests run on every change, and each command that changes the system was also run against a real pacman in a container; the changelog says what was seen for each slice.
- **No telemetry.** emrac talks only to your mirrors, the AUR and the Arch Linux Archive, and only when a command needs them.
- **Source Suitability Score & ETA engine** *(planned)* — estimates build time, download size, and performance gain per package before you commit, refined by local build history.
- **One interface, every source** — official repos and the AUR are searched, inspected, and installed the same way today; local/custom repos and locally built packages *(planned)*.
- **CLI and TUI parity** *(planned)* — no capability is exclusive to either interface.
- **Build profiles with inheritance** *(planned)* — reusable compiler/linker/LTO/PGO/march-mtune configurations instead of long flag lists per install.
- **Transactional and reversible** — dry-run previews, rollback, and snapshots for every mutating operation.
- **Integrated modal editor** *(planned)* — edit PKGBUILDs and configs without leaving the TUI, or fall back to `$EDITOR`.

## Documentation

[`SPEC.md`](./SPEC.md) is the complete specification:

- Vision & philosophy
- Full feature catalog
- Signature features (Install Planner, Explain, Package Score, Build Diff, Conflict Resolver, etc.)
- Command catalog, formal grammar (EBNF), and global/per-command option reference
- Architecture and process flowcharts (Mermaid)
- Implementation notes (language, integration strategy, milestone scope)

The automated tests that pass are listed in [`docs/tests/emrac-tests.rst`](./docs/tests/emrac-tests.rst), generated and kept current by `dev/gen-tests-doc.sh`. Release notes are in [`docs/releases/`](./docs/releases).

## Architecture (planned)

- **Language:** Rust
- **System integration:** hybrid — `libalpm` via FFI for fast read-only queries/indexing; shells out to the real `pacman` and `makepkg` binaries for anything that mutates system state, so pacman's own safety logic stays authoritative.
- **First milestone:** a core CLI loop (`search` / `info` / `install` / `remove` / `upgrade` against official repos + AUR) before the TUI, recommendation engine, build profiles, or integrated editor.
- **Testing:** real install/remove/build flows are exercised in a container or chroot, not against the live host, until the tool has proven itself trustworthy.

See `SPEC.md` Part X for details.

## Installing

On Arch Linux or an Arch-based system, the install script builds a pacman package from the release tarball, after checking its SHA-256 against the value pinned in the script, and installs it with `sudo pacman -U`, so pacman tracks every file and `sudo pacman -Rns emrac` removes it. It never runs as root, asks only for `sudo` where pacman needs it, and cleans up its temporary folder. Read it before you run it:

```sh
curl -fsSLO https://pardhuvarmax.github.io/emrac/install.sh
less install.sh
sh install.sh          # asks before it does anything; --yes skips the questions
```

Or in one step: `curl -fsSL https://pardhuvarmax.github.io/emrac/install.sh | sh`. `sh install.sh --dry-run` shows what it would do, and `sh install.sh --uninstall` removes the package. The AUR package is not published yet.

## Building

Requirements:

- A recent Rust toolchain (`cargo`, `rustc`)
- `libalpm` development headers (`pacman.pc`/`alpm.h` — already present on any Arch system with `pacman` installed)
- `clang`/`libclang` — the `alpm` crate generates its FFI bindings at build time (via `bindgen`) against whatever libalpm version is actually installed, rather than shipping bindings pinned to one version

```sh
cargo build --workspace
cargo run --bin emrac -- search ripgrep
cargo run --bin emrac -- info ripgrep --json
cargo run --bin emrac -- install ripgrep --dry-run   # preview only, safe on any machine
cargo run --bin emrac -- upgrade --dry-run           # preview only, safe on any machine
```

For a release build that does not embed your home directory or the cargo registry path in the binary, use `dev/release-build.sh` (it passes `--remap-path-prefix` for you).

`install`/`remove`/`upgrade` preview (`--dry-run`) is read-only and safe to run directly. Actually running them mutates real system state via `sudo pacman`/`makepkg` — see [`dev/README.md`](./dev/README.md) for the disposable container used to test that without touching your host.

## License

emrac is free software, licensed under the GNU General Public License, version 3 or (at your option) any later version (`GPL-3.0-or-later`); the full text is in [`LICENSE`](./LICENSE). It links `libalpm` and the `alpm` crates, which are GPL, so the program as a whole has to be distributed under GPL-compatible terms.

Copyright (C) 2026 Emrac contributors. This program comes with ABSOLUTELY NO WARRANTY; see the license for details.

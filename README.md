# Emrac

<img align="left" width="250" src="/assets/emrac.png" alt="emrac">

A hybrid next generation package management platform for Arch Linux — an orchestration and UX layer over `pacman`, `makepkg`, `libalpm`, PKGBUILD, and the AUR, unifying official repos, the AUR, and local/custom repos behind one CLI and TUI.

---

Status: **Slice r1.** The core loop (`search`, `info`, `install`, `remove`, `upgrade` against the official repos and the AUR) works, with a transaction preview and confirmation before every change and a PKGBUILD review for AUR builds. Still missing: the TUI, build profiles, and everything else in `SPEC.md` Part IV.

| Area | Commands |
|---|---|
| Find & inspect | `search`, `info`, `deps`, `rdeps`, `why`, `orphans` |
| Change | `install`, `remove`, `purge`, `upgrade`, `reinstall`, `downgrade`, `build` (AUR/PKGBUILD dir in the sandbox, no install) |
| Undo | `history`, `rollback [tx]` / `undo`, `redo`, `snapshot`, `restore <name>` |
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

Errors go to stderr as `emrac found: …` (something you can fix) or `emrac says: …` (something failed underneath); normal output, including `--json`, goes to stdout.

> Emrac uses "Slice i/r/u\<N\>" instead of semver — see `CHANGELOG.md`.

---


## Highlights

- **Source-first, not source-only** — builds from PKGBUILD locally when it's worthwhile, installs binaries when it isn't, and tells you why.
- **Source Suitability Score & ETA engine** — estimates build time, download size, and performance gain per package before you commit, refined by local build history.
- **One interface, every source** — official repos, AUR, local/custom repos, and locally built packages all searched, inspected, and installed the same way.
- **CLI and TUI parity** — no capability is exclusive to either interface.
- **Build profiles with inheritance** — reusable compiler/linker/LTO/PGO/march-mtune configurations instead of long flag lists per install.
- **Transactional and reversible** — dry-run previews, rollback, and snapshots for every mutating operation.
- **Integrated modal editor** — edit PKGBUILDs and configs without leaving the TUI, or fall back to `$EDITOR`.

## Documentation

[`SPEC.md`](./SPEC.md) is the complete specification:

- Vision & philosophy
- Full feature catalog
- Signature features (Install Planner, Explain, Package Score, Build Diff, Conflict Resolver, etc.)
- Command catalog, formal grammar (EBNF), and global/per-command option reference
- Architecture and process flowcharts (Mermaid)
- Implementation notes (language, integration strategy, milestone scope)

## Architecture (planned)

- **Language:** Rust
- **System integration:** hybrid — `libalpm` via FFI for fast read-only queries/indexing; shells out to the real `pacman` and `makepkg` binaries for anything that mutates system state, so pacman's own safety logic stays authoritative.
- **First milestone:** a core CLI loop (`search` / `info` / `install` / `remove` / `upgrade` against official repos + AUR) before the TUI, recommendation engine, build profiles, or integrated editor.
- **Testing:** real install/remove/build flows are exercised in a container or chroot, not against the live host, until the tool has proven itself trustworthy.

See `SPEC.md` Part X for details.

## Building

Requirements:

- A recent Rust toolchain (`cargo`, `rustc`)
- `libalpm` development headers (`pacman.pc`/`alpm.h` — already present on any Arch system with `pacman` installed)
- `clang`/`libclang` — the `alpm` crate generates its FFI bindings at build time (via `bindgen`) against whatever libalpm version is actually installed, rather than shipping bindings pinned to one version

```sh
cargo build --workspace
cargo run -p emrac-cli -- search ripgrep
cargo run -p emrac-cli -- info ripgrep --json
cargo run -p emrac-cli -- install ripgrep --dry-run   # preview only, safe on any machine
cargo run -p emrac-cli -- upgrade --dry-run           # preview only, safe on any machine
```

`install`/`remove`/`upgrade` preview (`--dry-run`) is read-only and safe to run directly. Actually running them mutates real system state via `sudo pacman`/`makepkg` — see [`dev/README.md`](./dev/README.md) for the disposable container used to test that without touching your host.

## License

Not yet decided.

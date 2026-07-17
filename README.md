# Emrac

<img align="right" width="350" src="https://github.com/user-attachments/assets/3419b48f-a2c8-4a0f-b4f4-f47f0ec3988e" alt="emrac">

A source-first package management platform for Arch Linux — an orchestration and UX layer over `pacman`, `makepkg`, `libalpm`, PKGBUILD, and the AUR, unifying official repos, the AUR, and local/custom repos behind one CLI and TUI.

Status: **Slice i1.** `search`/`info` work against official repositories (via libalpm) and the AUR (via its RPC API). `install` works against both, with a transaction preview, a PKGBUILD/diff review for AUR packages before building, and a confirmation prompt before anything real happens; `remove` works against installed packages regardless of source. Still missing: `upgrade`, the TUI, build profiles. (Emrac uses "Slice i/r/u\<N\>" instead of semver — see `CHANGELOG.md`.)

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
```

`install`/`remove` preview (`--dry-run`) is read-only and safe to run directly. Actually running them mutates real system state via `sudo pacman` — see [`dev/README.md`](./dev/README.md) for the disposable container used to test that without touching your host.

## License

Not yet decided.

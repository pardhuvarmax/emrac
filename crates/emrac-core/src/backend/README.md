# backend/

One module per source of packages or per outside tool. None is meant to be used directly by the command line: [`../sources.rs`](../sources.rs) orchestrates them.

- [`alpm.rs`](./alpm.rs) — the official repositories and the local database through libalpm FFI (the `alpm` crate, built with `generate` so the bindings match the installed libalpm). Read-only queries, plus simulated transactions: it prepares (never commits) an install or a full upgrade to learn what conflicts, replacements and provider choices will do, and what a dependency conflict is. Needs no root.
- [`pacman.rs`](./pacman.rs) — the only module that runs `pacman` / `sudo pacman` (and the few other `sudo` commands emrac needs). Resolution with `--print` needs no root; changes run under `sudo`, with pacman's own prompts unless `--yes`.
- [`fresh_db.rs`](./fresh_db.rs) — a private, throwaway copy of the sync databases refreshed just now (the `checkupdates` technique), so an upgrade preview matches what `pacman -Syu` will do.
- [`aur.rs`](./aur.rs) — the AUR through the aurweb RPC v5 API, read-only, with retries for transient failures.
- [`aur_build.rs`](./aur_build.rs) — the only module that runs `git` and `makepkg`. It keeps each package's checkout under `~/.cache/emrac/build/<pkg>` (a split package links to its base's), and builds in two steps: dependencies and sources on the host, then extraction, build and packaging inside the sandbox. Always as the invoking user, never root.
- [`archive.rs`](./archive.rs) — the Arch Linux Archive (listing and download); used for old package versions. A `file://` mirror is copied.
- [`package_file.rs`](./package_file.rs) — reads package files (`*.pkg.tar.*`) with `bsdtar` without installing or executing anything.

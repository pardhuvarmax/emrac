# emrac-cli

The `emrac` binary. A thin layer over [`emrac-core`](../emrac-core): parses arguments ([`cli.rs`](./src/cli.rs), via `clap`), dispatches to [`commands/`](./src/commands), formats output ([`output.rs`](./src/output.rs)) as human-readable text or `--json`, and prompts for confirmation ([`prompt.rs`](./src/prompt.rs)) before mutating anything.

**Current commands (Slice i1):**
- `search` — official repos + AUR, `--official`/`--aur` to scope.
- `info` — same, with `--offline` to skip the AUR.
- `install` — official repos and the AUR. Always shows a plan first; `--dry-run` stops there. AUR packages additionally show their PKGBUILD (first build) or a diff against the last build (rebuilds), gated behind their own confirmation — skip with `--skip-pkgbuild`, or force the full file instead of a diff with `--skip-diff`. Then the overall confirm (skippable with `-y`/`--yes`) before running the real `sudo pacman`/`makepkg` commands.
- `remove` — official repos only (removal doesn't distinguish source). Same plan-first, confirm-before-mutating shape.
- `upgrade [pkg...]` — everything out of date (official repos via a real `pacman -Syu`, plus any AUR-sourced package with a newer version upstream) when no packages are named, or just the named targets otherwise (each of which must already be installed). AUR upgrades go through the exact same PKGBUILD/diff review as `install` (`--skip-pkgbuild`/`--skip-diff` apply here too) — a rebuild with no upstream changes is detected and skips the review automatically.

See [`../../dev/README.md`](../../dev/README.md) for how `install`/`remove`/`upgrade` are tested without touching the host.

See the root [`SPEC.md`](../../SPEC.md) Part V and Part VIII for the full planned command surface.

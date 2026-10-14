# commands/

One module per subcommand. Each exposes a thin `run()` that calls into `emrac-core` and hands the result to `../output.rs`; the plan, confirm and execute flow of the commands that change the system lives here too (via `../prompt.rs`), since that is orchestration, not business logic.

**Find and inspect**
- [`search.rs`](./search.rs), [`info.rs`](./info.rs) — official repositories plus the AUR; `--official`, `--aur`, `--offline`.
- [`deps.rs`](./deps.rs) — `deps`, `rdeps`, `why`, `orphans`.
- [`verify.rs`](./verify.rs) — `verify` (alias `check`): exits non-zero if any package has real problems.
- [`audit.rs`](./audit.rs) — `audit` of a PKGBUILD, a folder, a built package file or an AUR package.
- [`doctor.rs`](./doctor.rs) — `doctor`, `--online`, `--report`; the exit status is the number of problems.

**Change the system**
- [`install.rs`](./install.rs), [`remove.rs`](./remove.rs), [`upgrade.rs`](./upgrade.rs), [`purge.rs`](./purge.rs), [`reinstall.rs`](./reinstall.rs), [`downgrade.rs`](./downgrade.rs) — plan, show, confirm, execute, then log to the history.
- [`aur_review.rs`](./aur_review.rs), [`aur_install.rs`](./aur_install.rs), [`official_inspect.rs`](./official_inspect.rs) — shared helpers, not subcommands: the PKGBUILD review and its confirmation, the build-and-install flow (sandbox, manifest, the high-risk prompt, the retry with the network on, the dependency-conflict swap), and `--inspect` for official packages.
- [`build.rs`](./build.rs) — `build`: an AUR package or a local PKGBUILD folder, no install.
- [`rollback.rs`](./rollback.rs) — `rollback [tx]`, `undo`, `redo`.
- [`snapshot.rs`](./snapshot.rs), [`restore.rs`](./restore.rs) — package-set restore points.
- [`pin.rs`](./pin.rs) — `pin` (no packages: list) and `unpin`.
- [`history.rs`](./history.rs) — `history` (alias `transactions`), and `record()`, which the commands above call after executing; a failed write only warns.

**Housekeeping**
- [`pacnew.rs`](./pacnew.rs) — `pacnew`, `pacnew diff`, `resolve` (keep, take, merge, edit), `revert`, `status`; asks every time and never takes `--yes` as an answer.
- [`cache.rs`](./cache.rs) — `cache` list, stats, clean, trim, purge (and the `cc` / `cs` shortcuts).

**Setup**
- [`config.rs`](./config.rs), [`completion.rs`](./completion.rs) — handled before `Sources::init()`, since they need no pacman setup.

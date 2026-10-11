# commands/

One file per subcommand. Each exposes a thin `run()` that calls into `emrac-core` and hands the result to `output.rs` — no business logic lives here.

Currently: [`search.rs`](./search.rs), [`info.rs`](./info.rs), [`install.rs`](./install.rs), [`remove.rs`](./remove.rs), [`upgrade.rs`](./upgrade.rs). `install`/`remove`/`upgrade` also handle the plan/confirm/execute flow (via `../prompt.rs`), since that's still just command orchestration, not business logic. [`aur_review.rs`](./aur_review.rs) is a shared helper, not a subcommand — `install` and `upgrade` both call it to show an AUR package's PKGBUILD/diff and get its own confirmation before a build.

[`config.rs`](./config.rs) handles `emrac config ...` (persisted flag defaults, see `emrac-core`'s `config.rs`); it's dispatched before `Sources::init()` since it needs no pacman/alpm setup.

[`completion.rs`](./completion.rs) prints a `clap_complete`-generated bash/zsh/fish script for `emrac completion <shell>`; like `config`, it's dispatched before `Sources::init()`.

[`history.rs`](./history.rs) handles `emrac history` (alias `transactions`) and exports `record()`, which `install`/`remove`/`upgrade` call after executing to append to the transaction log; a failed write there only warns.

[`rollback.rs`](./rollback.rs) handles `emrac rollback [tx]`, `emrac undo` and `emrac redo` (same code path; redo is just reversing the latest undo): plan, show what will change and what is left alone, confirm, execute, then log the result with `history::record_rollback`.

[`downgrade.rs`](./downgrade.rs) handles `emrac downgrade <pkg> [version]` and `--list`: plan, preview, confirm, execute via the same fetch + `pacman -U` path rollback uses, then log it as a `downgrade` transaction (so `undo` can reverse it).

[`reinstall.rs`](./reinstall.rs) handles `emrac reinstall <pkg>...`: plan (each package's source is shown), confirm, execute, log as a `reinstall` transaction (which `undo` skips and `rollback` refuses, since no version changes).

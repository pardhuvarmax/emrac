# commands/

One file per subcommand. Each exposes a thin `run()` that calls into `emrac-core` and hands the result to `output.rs` — no business logic lives here.

Currently: [`search.rs`](./search.rs), [`info.rs`](./info.rs), [`install.rs`](./install.rs), [`remove.rs`](./remove.rs), [`upgrade.rs`](./upgrade.rs). `install`/`remove`/`upgrade` also handle the plan/confirm/execute flow (via `../prompt.rs`), since that's still just command orchestration, not business logic. [`aur_review.rs`](./aur_review.rs) is a shared helper, not a subcommand — `install` and `upgrade` both call it to show an AUR package's PKGBUILD/diff and get its own confirmation before a build.

[`config.rs`](./config.rs) handles `emrac config ...` (persisted flag defaults, see `emrac-core`'s `config.rs`); it's dispatched before `Sources::init()` since it needs no pacman/alpm setup.

[`completion.rs`](./completion.rs) prints a `clap_complete`-generated bash/zsh/fish script for `emrac completion <shell>`; like `config`, it's dispatched before `Sources::init()`.

# commands/

One file per subcommand. Each exposes a thin `run()` that calls into `emrac-core` and hands the result to `output.rs` — no business logic lives here.

Currently: [`search.rs`](./search.rs), [`info.rs`](./info.rs), [`install.rs`](./install.rs), [`remove.rs`](./remove.rs), [`upgrade.rs`](./upgrade.rs). The latter three also handle the plan/confirm/execute flow (via `../prompt.rs`), since that's still just command orchestration, not business logic. [`aur_review.rs`](./aur_review.rs) is a shared helper, not a subcommand — `install` and `upgrade` both call it to show an AUR package's PKGBUILD/diff and get its own confirmation before a build.

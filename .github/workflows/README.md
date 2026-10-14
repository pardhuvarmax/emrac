# workflows/

GitHub Actions workflows.

- [`ci.yml`](./ci.yml) — runs on every push to `main` and on pull requests, inside an `archlinux:latest` container (libalpm and the pacman tools emrac calls only exist on Arch). It installs the toolchain (`base-devel`, `git`, `clang`, `rust`, `bubblewrap`), then runs, in order:
  1. `cargo fmt --all -- --check` — formatting;
  2. `cargo clippy --workspace --all-targets -- -D warnings` — lints, warnings are errors;
  3. `cargo test --workspace` — the automated tests;
  4. `sh -n` on `install.sh` and the scripts in `dev/` — they must at least parse;
  5. `sh dev/gen-tests-doc.sh --check` — fails if [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst) no longer matches the tests that pass, so the list of tests is always kept current.

The end-to-end checks in [`dev/e2e/`](../../dev/e2e) need podman and are not run here.

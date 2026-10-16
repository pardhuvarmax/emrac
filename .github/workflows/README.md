# workflows/

GitHub Actions workflows.

- [`ci.yml`](./ci.yml) — runs on every push to `main` and on pull requests, inside an `archlinux:latest` container (libalpm and the pacman tools emrac calls only exist on Arch). It installs the toolchain (`base-devel`, `git`, `clang`, `rust`, `bubblewrap`), then runs, in order:
  1. `cargo fmt --all -- --check` — formatting;
  2. `cargo clippy --workspace --all-targets -- -D warnings` — lints, warnings are errors;
  3. `cargo test --workspace` — the automated tests;
  4. `sh -n` on `install.sh` and the scripts in `dev/` — they must at least parse;
  5. `sh dev/gen-tests-doc.sh --check` — fails if [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst) no longer matches the tests that pass, so the list of tests is always kept current.
- [`release.yml`](./release.yml) — the release pipeline, run when a tag `r*` is pushed. Job `check`: the tag is annotated, reachable from `main`, has its notes in `docs/releases/`, and its version is higher than any in `releases.json`; then format, clippy, tests and the test list. Job `review` writes `dev/release-review.sh`'s report into the run summary. Job `publish` waits for approval (environment `release`, with a required reviewer) and then: `dev/release.sh publish <tag> --gh` — the GitHub release from the notes, the `releases.json` entry and news item, the `install.sh` pin (stable only) — then a commit to `main` by `github-actions[bot]`. The channel comes from the `Channel:` line of the notes. Start it with `dev/release.sh prepare <tag> <version> "summary" --push`. If it fails before `publish`, nothing is published; fix, delete the tag (`git push origin :<tag>`), and prepare again.

The end-to-end checks in [`dev/e2e/`](../../dev/e2e) need podman and are not run here.

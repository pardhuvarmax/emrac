#!/bin/sh
# The checks CI runs, in one place: format, lints, tests, that the shell
# scripts parse, and that the list of tests is current. Run it as an ordinary
# user: some tests expect one (emrac warns and behaves differently as root),
# so CI runs it through `su` (see .github/workflows/ci.yml).
set -eu
cd "$(dirname "$0")/.."

if [ "$(id -u)" = 0 ]; then
    echo "dev/ci.sh: run this as an ordinary user, not root (the tests assume one)" >&2
    exit 2
fi

echo "==> format"
cargo fmt --all -- --check
echo "==> clippy"
cargo clippy --workspace --all-targets -- -D warnings
echo "==> tests"
cargo test --workspace
echo "==> shell scripts parse"
sh -n install.sh
for f in dev/*.sh; do sh -n "$f"; done
echo "==> the test list is current"
sh dev/gen-tests-doc.sh --check

#!/bin/sh
# A release build that does not embed the builder's own paths.
#
# Rust writes source paths into a binary (panic messages, debug info), and
# without this they include the builder's home directory, the cargo registry
# and the checkout. `profile.release.trim-paths` would do it but is not stable
# yet, so the same is done with --remap-path-prefix, computed for whoever runs
# this. The more specific prefixes come later because the last match wins.
#
#   dev/release-build.sh            # builds target/release/emrac
#
# Extra arguments go to `cargo build` (for example `--locked`).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cargo_home=${CARGO_HOME:-$HOME/.cargo}

flags="--remap-path-prefix=$HOME=~"
flags="$flags --remap-path-prefix=$cargo_home=/cargo"
flags="$flags --remap-path-prefix=$root=."

cd "$root"
RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$flags" cargo build --release "$@"

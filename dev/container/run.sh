#!/usr/bin/env bash
# Builds emrac on the host, then drops into the disposable test container
# with the freshly-built binary mounted in read-only. Nothing this container
# does can touch the host's real pacman state (see Containerfile).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

echo "==> Building emrac (host)"
cargo build --workspace

echo "==> Building test container image"
podman build -t emrac-test -f dev/container/Containerfile dev/container

echo "==> Starting container (emrac mounted at /usr/local/bin/emrac, read-only)"
exec podman run --rm -it \
    -v "$repo_root/target/debug/emrac:/usr/local/bin/emrac:ro" \
    emrac-test

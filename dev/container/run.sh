#!/usr/bin/env bash
# Builds emrac on the host, then drops into the disposable test container
# with the freshly-built binary mounted in read-only. Nothing this container
# does can touch the host's real pacman state (see Containerfile).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Use the system toolchain explicitly: a cargo/rustc earlier on PATH (e.g.
# nix) links the binary against a loader that doesn't exist in the Arch
# container, so the mounted binary would fail to start.
echo "==> Building emrac (host)"
PATH=/usr/bin /usr/bin/cargo build --workspace

echo "==> Building test container image"
podman build -t emrac-test -f dev/container/Containerfile dev/container

echo "==> Starting container (emrac mounted at /usr/local/bin/emrac, read-only)"
# unmask=/proc/* lets bubblewrap mount a fresh /proc inside the container, which
# emrac's build sandbox needs (no extra capabilities, not --privileged).
exec podman run --rm -it \
    --security-opt 'unmask=/proc/*' \
    -v "$repo_root/target/debug/emrac:/usr/local/bin/emrac:ro" \
    emrac-test

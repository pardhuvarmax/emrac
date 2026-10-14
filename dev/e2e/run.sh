#!/usr/bin/env bash
# Builds the release binary (without the builder's paths), then runs the
# end-to-end checks in dev/e2e/final-check.sh in the disposable test container.
# Needs podman and network access; nothing here touches the host's pacman.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

echo "==> Building the release binary"
PATH=/usr/bin:$PATH sh dev/release-build.sh

echo "==> Building the test container image"
podman build -t emrac-test -f dev/container/Containerfile dev/container

echo "==> Running the end-to-end checks"
exec podman run --rm \
    --security-opt 'unmask=/proc/*' \
    -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
    -v "$repo_root/dev/e2e/final-check.sh:/final-check.sh:ro" \
    emrac-test bash /final-check.sh

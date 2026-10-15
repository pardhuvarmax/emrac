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

# dev/e2e/run.sh          the end-to-end checks of the binary
# dev/e2e/run.sh install  the checks of the root install.sh
# dev/e2e/run.sh self     builds two releases and runs self upgrade / downgrade for real
case "${1:-}" in
self)
    echo "==> Preparing two releases (0.8.0 and 0.9.0) from the committed source"
    fx="$(mktemp -d)"
    trap 'rm -rf "$fx"' EXIT
    for rel in 8 9; do
        mkdir "$fx/src$rel"
        git archive --prefix="emrac-r$rel/" HEAD | tar -x -C "$fx/src$rel"
        python3 - "$fx/src$rel/emrac-r$rel" "0.$rel.0" <<'PY'
import re, sys
root, version = sys.argv[1:3]
toml = root + "/Cargo.toml"
t = open(toml).read()
t = re.sub(r'(?m)^version = "[^"]*"', 'version = "%s"' % version, t, count=1)
open(toml, "w").write(t)
lock = root + "/Cargo.lock"
l = open(lock).read()
for name in ("emrac-core", "emrac-cli"):
    l = re.sub(r'(name = "%s"\nversion = )"[^"]*"' % name, r'\1"%s"' % version, l)
open(lock, "w").write(l)
PY
        tar -czf "$fx/emrac-r$rel.tar.gz" -C "$fx/src$rel" "emrac-r$rel"
    done
    echo "==> Running the self-upgrade checks (builds emrac three times; it takes a while)"
    podman run --rm \
        -v "$fx:/fx:ro" \
        -v "$repo_root/dev/e2e/self-upgrade.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
install)
    echo "==> Running the install script checks"
    exec podman run --rm \
        -v "$repo_root/install.sh:/install.sh:ro" \
        -v "$repo_root/dev/e2e/install-script.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
*)
    echo "==> Running the end-to-end checks"
    exec podman run --rm \
        --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/final-check.sh:/final-check.sh:ro" \
        emrac-test bash /final-check.sh
    ;;
esac

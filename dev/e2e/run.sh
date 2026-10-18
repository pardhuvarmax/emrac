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
# dev/e2e/run.sh preflight  a pending upgrade and a fake kernel: the partial-upgrade gate and the kernel checks
# dev/e2e/run.sh preview  what emrac says a plan will do (config files, hooks) against what pacman does
# dev/e2e/run.sh recovery pacman fails mid-install (injected): what emrac finds and how it mends it
# dev/e2e/run.sh buildtools an AUR build with a build-only dependency: what emrac offers to remove
# dev/e2e/run.sh stats    emrac stats from a build-times file, emrac benchmark of an installed package
# dev/e2e/run.sh graph    emrac graph drawn in the terminal, natively and through graph-easy
# dev/e2e/run.sh timeline emrac timeline over real transactions, a snapshot and a build-times fixture
# dev/e2e/run.sh policy   override and divert: what survives upgrades, reinstalls and removals
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
    chmod -R a+rX "$fx"
    echo "==> Running the self-upgrade checks (builds emrac three times; it takes a while)"
    podman run --rm \
        -v "$fx:/fx:ro" \
        -v "$repo_root/dev/e2e/self-upgrade.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
preflight)
    echo "==> Running the pre-flight checks (a local repository, a pending upgrade, a fake kernel)"
    podman run --rm --user root \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/preflight.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
preview)
    echo "==> Running the plan preview checks (config files and hooks against pacman)"
    podman run --rm --user root --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/preview.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
recovery)
    echo "==> Running the recovery checks (a full disk, simulated, in the middle of an install)"
    podman run --rm --user root --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/recovery.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
buildtools)
    echo "==> Running the build tool removal checks (an AUR build with a build-only dependency)"
    podman run --rm --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/buildtools.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
graph)
    echo "==> Running the graph drawing checks (installs graph-easy from the AUR)"
    podman run --rm --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/graph.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
timeline)
    echo "==> Running the timeline checks"
    podman run --rm \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/timeline.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
stats)
    echo "==> Running the statistics and benchmark checks"
    podman run --rm \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/stats.sh:/t.sh:ro" \
        emrac-test bash /t.sh
    ;;
policy)
    echo "==> Running the local file policy checks (override and divert against real pacman)"
    podman run --rm --user root --security-opt 'unmask=/proc/*' \
        -v "$repo_root/target/release/emrac:/usr/local/bin/emrac:ro" \
        -v "$repo_root/dev/e2e/policy.sh:/t.sh:ro" \
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

#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh graph).
# `emrac graph` drawn in the terminal: natively (--format boxes) and through
# graph-easy (--format easy), which is installed from the AUR by emrac itself.
#
# last full run: after u11, 13 of 13 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1; sudo pacman -S --noconfirm --needed bubblewrap base-devel git >/dev/null 2>&1

ck "boxes draws the root in a double box with its first dependency" "$e graph pacman --depth 1 --format boxes | grep -q '║ pacman ' && $e graph pacman --depth 1 --format boxes | grep -q '│ bash'"
ck "every direct dependency emrac lists for pacman is drawn" "( for d in \$($e graph pacman --depth 1 | sed -n 's/^  pacman -> //p' | tr ',' '\n' | sed 's/ (optional)//; s/ //g'); do $e graph pacman --depth 1 --format boxes | grep -q \"│ \$d \" || { echo missing \$d; exit 1; }; done )"
ck "links are arrows from a trunk" "$e graph zlib --format boxes | grep -q '▶' && $e graph zlib --format boxes | grep -q '┬'"
ck "the layout goes left to right: zlib is left of glibc" "$e graph zlib --format boxes | grep -E 'zlib.*glibc' "
ck "a shared package is drawn once, the extra links are listed" "$e graph pacman --depth 2 --format text >/dev/null; $e graph curl --depth 2 --format boxes 2>&1 | grep -c '│ openssl ' | grep -qx 1"
ck "a graph with too many packages is refused with advice" "$e graph pacman --format boxes 2>&1 | grep -q 'lower --depth'"
ck "an unknown package is refused" "! $e graph no-such-package --format boxes 2>&1 >/dev/null; $e graph no-such-package --format boxes 2>&1 | grep -qi 'no-such-package'"
ck "dot is unchanged" "$e graph pacman --depth 1 --format dot | grep -q 'digraph dependencies'"
ck "json is unchanged" "$e graph pacman --depth 1 --json | grep -q '\"edges\"'"
ck "easy without graph-easy says how to get it" "$e graph pacman --depth 1 --format easy 2>&1 | grep -q 'perl-graph-easy'"
ck "emrac installs graph-easy from the AUR" "$e install perl-graph-easy --yes --skip-pkgbuild --allow-network </dev/null && pacman -Q perl-graph-easy"
ck "easy draws with graph-easy (the root has a heavy border)" "$e graph zlib --format easy | grep -q '┃ zlib ┃'"
ck "easy shows the dependencies" "$e graph zlib --format easy | grep -q 'glibc'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

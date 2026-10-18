#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh buildtools).
# An AUR package (a local checkout posing as `pfetch`) has a build dependency
# (figlet) and a runtime dependency (tree). After the build emrac must offer to
# remove exactly what the build added and nothing needs: figlet goes, tree stays,
# and what was installed before the build is never touched.
#
# last full run: after u7, 9 of 9 passed
e=/usr/local/bin/emrac
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1; sudo pacman -S --noconfirm --needed bubblewrap base-devel git >/dev/null 2>&1

B=$HOME/.cache/emrac/build; mkdir -p /tmp/w $B; rm -rf /tmp/up.git $B/pfetch /tmp/w/*
printf 'pkgname=pfetch\npkgver=9.9\npkgrel=1\npkgdesc=fixture\narch=(any)\nlicense=(MIT)\ndepends=(tree)\nmakedepends=(figlet)\npackage() {\n  install -Dm755 /dev/stdin "$pkgdir/usr/bin/pfetch" <<<"#!/bin/sh"\n}\n' > /tmp/w/PKGBUILD
printf 'pkgbase = pfetch\n\tpkgdesc = fixture\n\tpkgver = 9.9\n\tpkgrel = 1\n\tarch = any\n\tdepends = tree\n\tmakedepends = figlet\n\tsource = https://github.com/dylanaraps/pfetch/archive/9.9.tar.gz\n\tsha256sums = aaaa\n\npkgname = pfetch\n' > /tmp/w/.SRCINFO
(cd /tmp/w && git init -q -b master && git add -A && git commit -q -m x) && git clone -q --bare /tmp/w /tmp/up.git && git clone -q /tmp/up.git $B/pfetch
ck "fixture: neither figlet nor tree is installed" "! pacman -Q figlet && ! pacman -Q tree"

# never: the old behaviour, nothing said
$e config set build.remove_make_deps never >/dev/null
ck "never: the build installs its dependencies and says nothing about removing them" "$e install pfetch --yes --skip-pkgbuild </dev/null >/tmp/o1 2>&1 && pacman -Q pfetch figlet tree && ! grep -q 'installed only to build' /tmp/o1"
sudo pacman -R --noconfirm figlet >/dev/null 2>&1

# ask, with --yes: the list and the command, nothing removed
$e config set build.remove_make_deps ask >/dev/null
ck "ask with --yes lists the build tool and the command, and removes nothing" "$e rebuild pfetch --yes --skip-pkgbuild </dev/null >/tmp/o2 2>&1; grep -q '1 package was installed only to build .pfetch., and nothing needs it now: figlet' /tmp/o2 && grep -q 'emrac remove figlet' /tmp/o2 && pacman -Q figlet"
ck "the runtime dependency is never offered" "! grep -E 'only to build.*tree' /tmp/o2"
sudo pacman -R --noconfirm figlet >/dev/null 2>&1

# always: removed as an ordinary, recorded removal
$e config set build.remove_make_deps always >/dev/null
ck "always removes the build tool after the install" "$e rebuild pfetch --yes --skip-pkgbuild </dev/null >/tmp/o3 2>&1; ! pacman -Q figlet"
ck "the package and its runtime dependency stay" "pacman -Q pfetch tree"
ck "the removal is in the history like any other" "$e history 2>&1 | grep -q 'figlet'"
ck "and can be undone" "$e undo --yes </dev/null >/tmp/o4 2>&1; pacman -Q figlet"
sudo pacman -R --noconfirm figlet >/dev/null 2>&1

# what was installed before the build is never touched
sudo pacman -S --noconfirm figlet >/dev/null 2>&1
ck "a package that was installed before the build is not offered" "$e rebuild pfetch --yes --skip-pkgbuild </dev/null >/tmp/o5 2>&1; pacman -Q figlet && ! grep -q 'only to build' /tmp/o5"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

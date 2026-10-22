#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh strays). `emrac strays` finds
# what no package owns under /usr and /opt (and says what it looks like it came from),
# and `emrac linkcheck` / the upgrade and removal plans name programs nobody packaged
# that would lose a shared library: a real program is compiled against a library from
# a fixture package, installed in /usr/local and /opt, and the package is then
# upgraded to a version that no longer provides that soname.
#
# last full run: after u25, 16 of 16 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkdir -p /srv/fixrepo
# fixlib N provides libfixlib.so=N-64 and ships libfixlib.so.N
mklib() {
  local v=$1 d=/tmp/mk-fixlib-$1; rm -rf $d; mkdir -p $d; chown tester $d
  printf 'int fixlib_answer(void){ return %s; }\n' "$v" > $d/lib.c
  cat > $d/PKGBUILD <<PB
pkgname=fixlib
pkgver=$v
pkgrel=1
pkgdesc=fixture
arch=(x86_64)
license=(MIT)
provides=('libfixlib.so=$v-64')
build() { gcc -shared -fPIC -Wl,-soname,libfixlib.so.$v -o libfixlib.so.$v "\$startdir/lib.c"; }
package() { install -Dm755 libfixlib.so.$v "\$pkgdir/usr/lib/libfixlib.so.$v"; }
PB
  chown -R tester $d
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/fixlib-$v-1-x86_64.pkg.tar.zst /srv/fixrepo/
}
mkdir_pkg() { # a package that owns the directory /usr/lib/fixdir
  local d=/tmp/mk-fixdir; rm -rf $d; mkdir -p $d; chown tester $d
  printf "pkgname=fixdir\npkgver=1\npkgrel=1\npkgdesc=fixture\narch=(any)\nlicense=(MIT)\npackage() {\n  install -Dm644 /dev/null \"\$pkgdir/usr/lib/fixdir/own.txt\"\n}\n" > $d/PKGBUILD
  chown -R tester $d
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/fixdir-1-1-any.pkg.tar.zst /srv/fixrepo/
}
publish() {
  (cd /srv/fixrepo && rm -f fixrepo.db* fixrepo.files* && repo-add fixrepo.db.tar.gz "$@" >/dev/null 2>&1)
  pacman -Sy --noconfirm >/dev/null 2>&1
}
mklib 1; mklib 2; mkdir_pkg
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
as_tester $e config set safety.signatures warn >/dev/null 2>&1
as_tester $e config set safety.preflight warn >/dev/null 2>&1
publish fixlib-1-1-x86_64.pkg.tar.zst fixdir-1-1-any.pkg.tar.zst
pacman -S --noconfirm fixlib fixdir >/dev/null 2>&1

# a program built outside the package manager, linked against the packaged library
mkdir -p /tmp/prog && printf 'int fixlib_answer(void);\nint main(void){ return fixlib_answer() == 1 ? 0 : 3; }\n' > /tmp/prog/main.c
gcc -o /usr/local/bin/usesfix /tmp/prog/main.c /usr/lib/libfixlib.so.1
mkdir -p /opt/strayapp/bin && cp /usr/local/bin/usesfix /opt/strayapp/bin/run
# things nobody owns
printf 'made by make install\n' > /usr/bin/handmade
mkdir -p /usr/share/handmade-docs && printf 'docs\n' > /usr/share/handmade-docs/readme
printf 'dropped in by a script\n' > /usr/lib/fixdir/plugin.so
printf 'generated\n' > /usr/share/applications/mimeinfo.cache
py=$(ls -d /usr/lib/python3*/site-packages 2>/dev/null | head -1)
if [ -n "$py" ]; then mkdir -p $py/leftpad $py/leftpad-1.0.dist-info; echo x > $py/leftpad/__init__.py; echo pip > $py/leftpad-1.0.dist-info/INSTALLER; fi

ck "the program built outside the package manager runs" "/usr/local/bin/usesfix"

# --- strays
as_tester $e strays >/tmp/s1.out 2>&1; rc=$?
ck "STRAYS lists what no package owns and changes nothing" "[ $rc -eq 0 ] && grep -q '/usr/bin/handmade' /tmp/s1.out && grep -q '/usr/share/handmade-docs/' /tmp/s1.out && grep -q '/opt/strayapp/' /tmp/s1.out && grep -q 'Nothing was changed' /tmp/s1.out && [ -e /usr/bin/handmade ]"
ck "STRAYS does not list what a package owns" "! grep -q '/usr/bin/bash' /tmp/s1.out && ! grep -q 'libfixlib.so.1' /tmp/s1.out"
ck "STRAYS says a file was put into a directory a package owns, and which package" "grep -q 'Put into a directory that fixdir owns' /tmp/s1.out && grep -q '/usr/lib/fixdir/plugin.so' /tmp/s1.out"
ck "STRAYS leaves out the files tools generate, unless asked" "! grep -q 'mimeinfo.cache' /tmp/s1.out && as_tester $e strays --all 2>&1 | grep -q 'mimeinfo.cache'"
ck "STRAYS keeps /usr/local out unless asked" "! grep -q '/usr/local/bin/usesfix' /tmp/s1.out && as_tester $e strays --local 2>&1 | grep -q '/usr/local/bin/usesfix'"
ck "STRAYS --path looks only there" "as_tester $e strays --path usr/share 2>&1 | grep -q 'handmade-docs' && ! as_tester $e strays --path usr/share 2>&1 | grep -q '/usr/bin/handmade'"
ck "STRAYS --json is data" "as_tester $e strays --json 2>/dev/null | grep -q '\"path\": \"/usr/bin/handmade\"'"
if [ -n "$py" ]; then ck "STRAYS names pip as the installer of a Python package outside pacman" "grep -q 'Python packages installed outside pacman (pip)' /tmp/s1.out && grep -q 'leftpad' /tmp/s1.out"; fi

# --- linkcheck now
as_tester $e linkcheck >/tmp/l1.out 2>&1
ck "LINK linkcheck looks at the programs outside the package manager and finds none broken" "grep -q 'outside the package manager looked at' /tmp/l1.out && grep -q 'none of them is missing a library now' /tmp/l1.out"

# --- plans: an upgrade and a removal of the library
publish fixlib-2-1-x86_64.pkg.tar.zst fixdir-1-1-any.pkg.tar.zst
as_tester $e upgrade fixlib --dry-run >/tmp/l2.out 2>&1
ck "LINK the upgrade plan names the program in /usr/local, the library and the package change" "grep -q '/usr/local/bin/usesfix is not from a package and needs libfixlib.so.1: fixlib 1-1 → 2-1 no longer provides libfixlib.so.1' /tmp/l2.out"
ck "LINK it names the unowned program under /opt too" "grep -q '/opt/strayapp/bin/run is not from a package and needs libfixlib.so.1' /tmp/l2.out"
as_tester $e remove fixlib --dry-run >/tmp/l3.out 2>&1
ck "LINK the removal plan names them as well" "grep -q '/usr/local/bin/usesfix is not from a package and needs libfixlib.so.1: removing fixlib 1-1 takes libfixlib.so.1 away' /tmp/l3.out"
as_tester $e linkcheck --upgrade >/tmp/l4.out 2>&1
ck "LINK linkcheck --upgrade says the same without opening a plan" "grep -q '/usr/local/bin/usesfix is not from a package and needs libfixlib.so.1' /tmp/l4.out"
ck "LINK an upgrade of something else says nothing about them" "! as_tester $e upgrade fixdir --dry-run 2>&1 | grep -q 'is not from a package'"

# --- after the upgrade: the program really cannot find its library
as_tester $e upgrade fixlib --yes --ack-news >/tmp/l5.out 2>&1
as_tester $e linkcheck >/tmp/l6.out 2>&1
ck "LINK after the upgrade the program really fails, and linkcheck says which library is gone" "! /usr/local/bin/usesfix 2>/dev/null; grep -q '/usr/local/bin/usesfix cannot find libfixlib.so.1' /tmp/l6.out && grep -q '/opt/strayapp/bin/run cannot find libfixlib.so.1' /tmp/l6.out"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

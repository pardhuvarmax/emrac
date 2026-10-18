#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh recovery). Makes pacman
# fail the way a full disk does (strace fault injection on creating one file of a
# package) and checks that emrac notices what pacman leaves behind: a package
# recorded as installed with a file missing; that the suggested command mends
# it; and that a transaction that worked says nothing.
#
# last full run: after u6, 9 of 9 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel strace >/dev/null 2>&1
mkpkg() { # name  : a package with two files
  local n=$1 d=/tmp/mk-$1; rm -rf $d; mkdir -p $d; chown tester $d
  printf "pkgname=$n\npkgver=1\npkgrel=1\npkgdesc='fixture'\narch=(any)\nlicense=(MIT)\npackage() {\n  echo a | install -Dm644 /dev/stdin \"\$pkgdir/usr/share/$n/a.txt\"\n  echo b | install -Dm644 /dev/stdin \"\$pkgdir/usr/share/$n/b.txt\"\n}\n" > $d/PKGBUILD
  chown tester $d/PKGBUILD
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$n-1-1-any.pkg.tar.zst /srv/fixrepo/
}
mkdir -p /srv/fixrepo
mkpkg phantom; mkpkg fine
(cd /srv/fixrepo && repo-add fixrepo.db.tar.gz phantom-1-1-any.pkg.tar.zst fine-1-1-any.pkg.tar.zst >/dev/null 2>&1)
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
pacman -Syu --noconfirm >/dev/null 2>&1

ck "a transaction that worked says nothing about missing files" \
   "as_tester $e install fine --yes >/tmp/ok.out 2>&1 && pacman -Q fine && ! grep -q 'files are missing' /tmp/ok.out"

# the disk is "full" for one file of the package: a stand-in for pacman that adds the fault
# only when installing "phantom" (strace cannot sit in front of sudo, which needs its setuid bit)
mv /usr/bin/pacman /usr/bin/pacman.real
cat > /usr/bin/pacman <<'W'
#!/bin/sh
case " $* " in
*" -S "*phantom*) exec strace -f -P /usr/share/phantom/a.txt -e trace=openat -e inject=openat:error=ENOSPC -o /dev/null /usr/bin/pacman.real "$@";;
esac
exec /usr/bin/pacman.real "$@"
W
chmod 755 /usr/bin/pacman
as_tester $e install phantom --yes >/tmp/fail.out 2>&1
rc=$?
mv -f /usr/bin/pacman.real /usr/bin/pacman
tail -15 /tmp/fail.out | sed "s/^/    | /"
ck "the install fails" "[ $rc -ne 0 ]"
ck "pacman still lists the package (this is what it leaves behind)" "pacman -Q phantom && [ ! -e /usr/share/phantom/a.txt ]"
ck "emrac says the package is recorded but its file is missing" "grep -q 'phantom 1-1 as installed, but' /tmp/fail.out && grep -q 'usr/share/phantom/a.txt' /tmp/fail.out"
ck "emrac names the command that mends it" "grep -q 'emrac reinstall phantom' /tmp/fail.out"
ck "the failure is in the history as before" "as_tester $e history 2>&1 | grep -q FAILED"
ck "emrac check also sees the damage (it exits non-zero)" "! as_tester $e check phantom"
ck "that command mends it" "as_tester $e reinstall phantom --yes >/tmp/fix.out 2>&1; [ -f /usr/share/phantom/a.txt ] && [ -f /usr/share/phantom/b.txt ]"
ck "and the next check finds nothing wrong" "as_tester $e check phantom"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

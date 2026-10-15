#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh preflight). Builds a
# small local repository so that an upgrade is really waiting, then checks that
# emrac refuses a partial upgrade, lets --allow-partial and a pin through, and
# refuses to remove the only kernel (a fake one).
#
# last full run: u17, 8 of 8 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }

# the image is older than the repositories: bring it fully up to date first, so that
# only the fixture is waiting
pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1

# fixture packages (any architecture, no dependencies)
mk() { # name version [extra file commands]
  local d=/tmp/mk-$1-$2; rm -rf $d; mkdir -p $d; chown tester $d
  cat > $d/PKGBUILD <<PKG
pkgname=$1
pkgver=$2
pkgrel=1
pkgdesc='fixture'
arch=(any)
license=(MIT)
package() {
  install -Dm644 /dev/null "\$pkgdir/usr/share/$1/$2"
  $3
}
PKG
  chown tester $d/PKGBUILD
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$1-$2-1-any.pkg.tar.zst /srv/fixrepo/
}
mkdir -p /srv/fixrepo
mk fixa 1.0 ''
mk fixnew 1.0 ''
mk fixnew2 1.0 ''
mk fakekernel 1.0 'install -Dm644 /dev/null "$pkgdir/usr/lib/modules/1.0-fake/vmlinuz"; echo fakekernel | install -Dm644 /dev/stdin "$pkgdir/usr/lib/modules/1.0-fake/pkgbase"'
cd /srv/fixrepo && repo-add fixrepo.db.tar.gz fixa-1.0-1-any.pkg.tar.zst fixnew-1.0-1-any.pkg.tar.zst fixnew2-1.0-1-any.pkg.tar.zst fakekernel-1.0-1-any.pkg.tar.zst >/dev/null 2>&1
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
pacman -Sy --noconfirm >/dev/null 2>&1
pacman -S --noconfirm fixa fakekernel >/dev/null 2>&1
# a newer fixa appears in the repository: an upgrade is now waiting
mk fixa 2.0 ''
cd /srv/fixrepo && rm -f fixa-1.0-1-any.pkg.tar.zst && repo-add fixrepo.db.tar.gz fixa-2.0-1-any.pkg.tar.zst >/dev/null 2>&1
pacman -Sy --noconfirm >/dev/null 2>&1
ck "the fixtures are in place: fixa 1.0 installed, 2.0 waiting" "pacman -Q fixa | grep -q '1.0-1' && pacman -Qu | grep -q fixa"

ck "installing a new package while an upgrade waits is refused and names it" "! as_tester $e install fixnew --yes 2>&1 | tee /tmp/o; grep -q 'partial' /tmp/o && grep -q 'fixa' /tmp/o && ! pacman -Q fixnew"
ck "--dry-run says the same and changes nothing" "as_tester $e install fixnew --dry-run 2>&1 | grep -q PROBLEM; ! pacman -Q fixnew"
ck "--allow-partial lets one run through" "as_tester $e install fixnew --yes --allow-partial && pacman -Q fixnew"
ck "a pinned package does not count" "as_tester $e pin fixa --reason test >/dev/null && as_tester $e install fixnew2 --yes && pacman -Q fixnew2 && as_tester $e unpin fixa >/dev/null"
ck "the setting safety.partial_upgrade=warn goes on with a warning" "as_tester $e config set safety.partial_upgrade warn >/dev/null && pacman -R --noconfirm fixnew2 >/dev/null && as_tester $e install fixnew2 --yes >/tmp/o 2>&1; grep -q 'warning' /tmp/o && pacman -Q fixnew2; as_tester $e config unset safety.partial_upgrade >/dev/null"
ck "removing the only kernel is refused" "! as_tester $e remove fakekernel --yes 2>&1 | tee /tmp/o; grep -q 'only\\|every installed kernel' /tmp/o && pacman -Q fakekernel"
ck "after a full upgrade the same install passes the partial-upgrade check" "as_tester $e upgrade --yes >/tmp/u 2>&1; pacman -Q fixa | grep -q 2.0 && pacman -R --noconfirm fixnew >/dev/null && as_tester $e install fixnew --dry-run >/tmp/o 2>&1; ! grep -q PROBLEM /tmp/o && (grep -q 'pre-flight passed.*Partial upgrade' /tmp/o || grep -q 'ok  *Partial upgrade' /tmp/o)"
echo; echo "$n checks, $bad failed"

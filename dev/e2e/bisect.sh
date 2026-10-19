#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh bisect). Five real emrac
# transactions with fixture packages, the third of which brings the breakage
# (fixb 3.0 ships /usr/share/fixb/broken); `emrac bisect` has to find it by putting
# the system back into the states it had, and then put it back as it was.
#
# last full run: after u24, 11 of 11 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkdir -p /srv/fixrepo
mkpkg() { # name version [file]  : a package whose version is in /usr/share/<name>/version
  local n=$1 v=$2 f=${3:-} d=/tmp/mk-$1-$2; rm -rf $d; mkdir -p $d; chown tester $d
  {
    printf "pkgname=$n\npkgver=$v\npkgrel=1\npkgdesc='fixture'\narch=(any)\nlicense=(MIT)\npackage() {\n"
    printf "  echo $v | install -Dm644 /dev/stdin \"\$pkgdir/usr/share/$n/version\"\n"
    [ -n "$f" ] && printf "  echo x | install -Dm644 /dev/stdin \"\$pkgdir$f\"\n"
    printf "}\n"
  } > $d/PKGBUILD
  chown tester $d/PKGBUILD
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$n-$v-1-any.pkg.tar.zst /srv/fixrepo/
}
publish() { # the repository holds exactly these files
  (cd /srv/fixrepo && rm -f fixrepo.db* fixrepo.files* && repo-add fixrepo.db.tar.gz "$@" >/dev/null 2>&1)
  pacman -Sy --noconfirm >/dev/null 2>&1
}
mkpkg fixb 1.0; mkpkg fixb 2.0; mkpkg fixb 3.0 /usr/share/fixb/broken
mkpkg fixc 1.0; mkpkg fixc 2.0; mkpkg fixd 1.0
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
as_tester $e config set safety.signatures warn >/dev/null 2>&1
as_tester $e config set safety.preflight warn >/dev/null 2>&1

# tx-1 install fixb 1.0   tx-2 install fixc 1.0   tx-3 fixb 2.0   tx-4 fixc 2.0   tx-5 fixb 3.0 (broken)   tx-6 install fixd
publish fixb-1.0-1-any.pkg.tar.zst fixc-1.0-1-any.pkg.tar.zst fixb-2.0-1-any.pkg.tar.zst fixc-2.0-1-any.pkg.tar.zst fixb-3.0-1-any.pkg.tar.zst fixd-1.0-1-any.pkg.tar.zst
# installs by repository would have cached every version; a file install does not, so the cache is filled
cp /srv/fixrepo/*.pkg.tar.zst /var/cache/pacman/pkg/
as_tester $e install /srv/fixrepo/fixb-1.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e install /srv/fixrepo/fixc-1.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e install /srv/fixrepo/fixb-2.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e install /srv/fixrepo/fixc-2.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e install /srv/fixrepo/fixb-3.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e install /srv/fixrepo/fixd-1.0-1-any.pkg.tar.zst --yes >/dev/null 2>&1
as_tester $e history 2>&1 | tail -8 >/tmp/hist.out
good=$(as_tester $e history --json 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(sorted([r['id'] for r in d], key=lambda s:int(s.split('-')[1]))[1])" 2>/dev/null)
[ -z "$good" ] && good=tx-2

ck "the fixtures are in place: fixb 3.0 installed and broken" "[ \"\$(cat /usr/share/fixb/version)\" = 3.0 ] && [ -e /usr/share/fixb/broken ] && [ \"\$(cat /usr/share/fixc/version)\" = 2.0 ] && [ -e /usr/share/fixd/version ]"
before=$(pacman -Q | sha256sum)
ck "--dry-run lists the window, says every version can be found, and changes nothing" "as_tester $e bisect --good $good --dry-run >/tmp/b0.out 2>&1; grep -q 'transactions between good' /tmp/b0.out && grep -q 'can be found' /tmp/b0.out && [ \"\$(pacman -Q | sha256sum)\" = \"$before\" ]"
ck "a state the history does not know is refused with what was asked" "! as_tester $e bisect --good tx-999 --dry-run >/tmp/b1.out 2>&1; grep -q 'tx-999' /tmp/b1.out"
ck "good must come before bad" "! as_tester $e bisect --good tx-5 --bad tx-3 --dry-run >/tmp/b2.out 2>&1; grep -q 'good state must come before' /tmp/b2.out"

as_tester $e bisect --good $good --test 'test ! -e /usr/share/fixb/broken' --yes >/tmp/b3.out 2>&1; rc=$?
ck "the search names the transaction that brought fixb 3.0" "[ $rc -eq 0 ] && grep -q 'first state where it is broken is after' /tmp/b3.out && grep -q 'fixb 2.0-1 → 3.0-1' /tmp/b3.out"
ck "it did not blame the transactions around it" "! grep -q 'fixc' <(sed -n '/first state where it is broken/,\$p' /tmp/b3.out) && ! grep -q 'fixd' <(sed -n '/first state where it is broken/,\$p' /tmp/b3.out)"
ck "the system is back exactly as it was" "grep -q 'back as it was' /tmp/b3.out && [ \"\$(pacman -Q | sha256sum)\" = \"$before\" ] && [ -e /usr/share/fixb/broken ]"
ck "a safety snapshot was saved and named" "grep -q \"saved snapshot 'bisect-\" /tmp/b3.out && as_tester $e snapshot list 2>&1 | grep -q bisect-"
ck "the steps are in the history, none of them failed" "as_tester $e history 2>&1 | grep -c restore >/dev/null; ! as_tester $e history 2>&1 | grep -qi 'failed'"
ck "a test that cannot tell (125) in every state ends with the range, not a guess" "as_tester $e bisect --good $good --bad tx-6 --test 'exit 125' --yes >/tmp/b4.out 2>&1; grep -q 'could not be judged' /tmp/b4.out && [ \"\$(pacman -Q | sha256sum)\" = \"$before\" ]"

# a version no cache, repository or archive has: refused before anything changes
rm -f /var/cache/pacman/pkg/fixb-1.0-1-any.pkg.tar.zst /srv/fixrepo/fixb-1.0-1-any.pkg.tar.zst /home/tester/.cache/emrac/archive/fixb/* 2>/dev/null
ck "a missing package file is refused up front, before the system is touched" "! as_tester $e bisect --good $good --test true --yes --offline >/tmp/b5.out 2>&1; grep -q 'fixb 1.0' /tmp/b5.out && [ \"\$(pacman -Q | sha256sum)\" = \"$before\" ]"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

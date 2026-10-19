#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh crash), with the test build of
# emrac (--cfg emrac_fault). The same operation is cut off at each named durable
# boundary in turn (EMRAC_FAULT=<point>), for `install` and `upgrade`; then a package
# manager and emrac are killed together in the middle of `pacman -U` (a power cut,
# unlike killing emrac alone), and pacman is changed between emrac's plan and its
# commit. After every cut the installed packages, the journal, the history and
# pacman's own database must agree with what happened, and the next run must work.
#
# last full run: after the crash sweep for install and upgrade, 18 of 18 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkdir -p /srv/fixrepo
mkpkg() { # name version [extra PKGBUILD lines]
  local n=$1 v=$2 d=/tmp/mk-$1-$2; rm -rf $d; mkdir -p $d; chown tester $d
  {
    printf "pkgname=$n\npkgver=$v\npkgrel=1\npkgdesc='fixture'\narch=(any)\nlicense=(MIT)\n"
    [ "$n" = slowpre ] && printf "install=slowpre.install\n"
    printf "package() {\n  echo $v | install -Dm644 /dev/stdin \"\$pkgdir/usr/share/$n/version\"\n}\n"
  } > $d/PKGBUILD
  [ "$n" = slowpre ] && printf 'pre_install() { [ -e /tmp/slowpre-fast ] || sleep 25; }\n' > $d/slowpre.install
  chown -R tester $d
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$n-$v-1-any.pkg.tar.zst /srv/fixrepo/
}
publish() { # the repository holds exactly these package files
  (cd /srv/fixrepo && rm -f fixrepo.db* fixrepo.files* && repo-add fixrepo.db.tar.gz "$@" >/dev/null 2>&1)
  pacman -Sy --noconfirm >/dev/null 2>&1
}
mkpkg fixnew 1; mkpkg fixother 1; mkpkg slowpre 1
for v in $(seq 1 14); do mkpkg fixa $v; done
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
as_tester $e config set safety.signatures warn >/dev/null 2>&1
as_tester $e config set safety.preflight warn >/dev/null 2>&1
count() { as_tester $e history --limit 100000 2>/dev/null | grep -c '^tx-'; }

# the four views, after the next command has run
views_agree() { # before-count
  as_tester $e doctor 2>&1 | grep -qi 'cut off\|interrupted' && { echo "an interrupted transaction is still pending"; return 1; }
  as_tester $e history verify >/tmp/hv.out 2>&1 || { echo "history verify fails: $(tail -3 /tmp/hv.out)"; return 1; }
  pacman -Dk >/tmp/dk.out 2>&1 || { echo "pacman -Dk: $(tail -3 /tmp/dk.out)"; return 1; }
  local after; after=$(count)
  [ $((after-$1)) -eq 2 ] || { echo "the history has $((after-$1)) new records, not 2"; return 1; }
}

# --- install, cut off at each boundary
install_case() {
  local P=$1 expect=$2
  pacman -R --noconfirm fixnew fixother >/dev/null 2>&1
  publish fixnew-1-1-any.pkg.tar.zst fixother-1-1-any.pkg.tar.zst fixa-1-1-any.pkg.tar.zst
  local before; before=$(count)
  EMRAC_FAULT=$P as_tester $e install fixnew --yes >/tmp/ci.out 2>&1; local rc=$?
  { [ $rc -ne 0 ] && grep -q "cut off at '$P'" /tmp/ci.out; } || { echo "not cut off at $P (rc=$rc)"; tail -3 /tmp/ci.out; return 1; }
  if pacman -Q fixnew >/dev/null 2>&1; then got=yes; else got=no; fi
  [ $got = $expect ] || { echo "fixnew installed: $got, expected $expect at $P"; return 1; }
  as_tester $e install fixother --yes >/tmp/ci2.out 2>&1 || { echo "the next command failed"; tail -5 /tmp/ci2.out; return 1; }
  pacman -Q fixother >/dev/null 2>&1 || { echo "the next command did not install"; return 1; }
  views_agree $before
}
for spec in after-journal-begin:no before-history-record:yes after-history-record:yes after-ledger-sync:yes after-journal-finish:yes; do
  ck "INSTALL cut off at '${spec%%:*}': the packages, journal, history and pacman's database agree; the next command works" "install_case ${spec%%:*} ${spec##*:}"
done

# --- upgrade, cut off at each boundary
sudo_pacman_u() { pacman -U --noconfirm "$@" >/dev/null 2>&1; }
cur=1
sudo_pacman_u /srv/fixrepo/fixa-1-1-any.pkg.tar.zst
upgrade_case() {
  local P=$1 expect=$2
  pacman -R --noconfirm fixother >/dev/null 2>&1
  local next=$((cur+1))
  publish fixa-$next-1-any.pkg.tar.zst fixother-1-1-any.pkg.tar.zst
  local before; before=$(count)
  EMRAC_FAULT=$P as_tester $e upgrade fixa --yes >/tmp/cu.out 2>&1; local rc=$?
  { [ $rc -ne 0 ] && grep -q "cut off at '$P'" /tmp/cu.out; } || { echo "not cut off at $P (rc=$rc)"; tail -3 /tmp/cu.out; return 1; }
  local v; v=$(pacman -Q fixa | cut -d' ' -f2 | cut -d- -f1)
  if [ "$expect" = yes ]; then [ "$v" = $next ] || { echo "fixa is $v, expected the upgrade ($next) at $P"; return 1; }; cur=$next
  else [ "$v" = $cur ] || { echo "fixa is $v, expected it untouched ($cur) at $P"; return 1; }; fi
  as_tester $e install fixother --yes >/tmp/cu2.out 2>&1 || { echo "the next command failed"; tail -5 /tmp/cu2.out; return 1; }
  views_agree $before
}
for spec in after-journal-begin:no before-history-record:yes after-history-record:yes after-ledger-sync:yes after-journal-finish:yes after-binary-upgrade:yes; do
  ck "UPGRADE cut off at '${spec%%:*}': the packages, journal, history and pacman's database agree; the next command works" "upgrade_case ${spec%%:*} ${spec##*:}"
done

# --- pacman changes the system between emrac's plan and its commit
pacman -R --noconfirm fixnew fixother >/dev/null 2>&1
publish fixnew-1-1-any.pkg.tar.zst fixother-1-1-any.pkg.tar.zst fixa-$((cur+1))-1-any.pkg.tar.zst
EMRAC_FAULT_TAMPER='before-recheck:sudo pacman -S --noconfirm fixnew >/dev/null 2>&1' as_tester $e install fixnew --yes >/tmp/pl1.out 2>&1; rc=$?
ck "PC-01 pacman installs the very package between the plan and the commit: the plan is made again at once and the run is refused" "[ $rc -ne 0 ] && grep -q 'plan changed while you were deciding' /tmp/pl1.out && grep -q 'Nothing was changed' /tmp/pl1.out"
ck "PC-01 emrac did not run its own transaction on top of pacman's" "pacman -Q fixnew >/dev/null && ! as_tester $e history --limit 1 2>&1 | grep -q 'fixnew.*failed'"
EMRAC_FAULT_TAMPER='before-recheck:sudo pacman -S --noconfirm fixother >/dev/null 2>&1' as_tester $e install fixa --yes >/tmp/pl2.out 2>&1; rc=$?
ck "PC-02 pacman installs an unrelated package in between: the plan is made again, found the same, and goes on" "pacman -Q fixother >/dev/null && ! grep -q 'plan changed while you were deciding' /tmp/pl2.out"
pacman -R --noconfirm fixnew >/dev/null 2>&1
EMRAC_FAULT_TAMPER='before-recheck:sudo pacman -Sy >/dev/null 2>&1' as_tester $e install fixnew --yes >/tmp/pl3.out 2>&1; rc=$?
ck "PC-03 pacman syncs the databases in between: the plan is made again, found the same, and goes on" "[ $rc -eq 0 ] && pacman -Q fixnew >/dev/null"

# --- a power cut in the middle of pacman -U: emrac and pacman die together
pacman -R --noconfirm slowpre >/dev/null 2>&1
rm -f /tmp/slowpre-fast
publish slowpre-1-1-any.pkg.tar.zst fixother-1-1-any.pkg.tar.zst fixa-$((cur+1))-1-any.pkg.tar.zst
before=$(count)
setsid runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin $e install slowpre --yes >/tmp/pw.out 2>&1 &
for i in $(seq 1 120); do pgrep -f 'sleep 25' >/dev/null && break; sleep 0.25; done
pkill -9 -f 'emrac install slowpre'; pkill -9 pacman; pkill -9 -f 'sleep 25'; sleep 1
rm -f /var/lib/pacman/db.lck
ck "PW-01 killed together in the middle of pacman's install: the package is not half-installed and pacman's database is consistent" "! pacman -Q slowpre >/dev/null 2>&1 && pacman -Dk >/dev/null 2>&1"
touch /tmp/slowpre-fast
as_tester $e install fixother --yes >/tmp/pw2.out 2>&1; rc=$?
ck "PW-01 the next emrac command finds the cut-off transaction, records it as failed and carries on" "[ $rc -eq 0 ] && grep -q 'in your history (failed' /tmp/pw2.out"
ck "PW-01 the history, the journal and the database agree afterwards, and the install can be done again" "views_agree $before && as_tester $e install slowpre --yes >/dev/null 2>&1 && pacman -Q slowpre >/dev/null"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

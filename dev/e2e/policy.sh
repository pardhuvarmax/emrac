#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh policy). A small package
# (a program, a plain data file, a backup= config file) is built in three versions;
# pacman is made to upgrade and reinstall it while a local file policy is set
# (emrac override / emrac divert), and what pacman leaves is checked against what
# the policy says. Without the policy pacman resets the mode, owner and content
# on every one of these (shown first).
#
# last full run: after u8, 47 of 47 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }
st() { stat -c '%a %U:%G' "$1"; }

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkpkg() { # version data-content
  local d=/tmp/mk-tp-$1; rm -rf $d; mkdir -p $d; chown tester $d
  cat > $d/PKGBUILD <<P
pkgname=tp
pkgver=$1
pkgrel=1
pkgdesc='fixture'
arch=(any)
license=(MIT)
backup=(etc/tp.conf)
package() {
  install -Dm755 /dev/stdin "\$pkgdir/usr/bin/tp-tool" <<<"#!/bin/sh"
  install -Dm644 /dev/stdin "\$pkgdir/usr/share/tp/data" <<<"$2"
  install -Dm644 /dev/stdin "\$pkgdir/etc/tp.conf" <<<"conf"
}
P
  chown tester $d/PKGBUILD
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/tp-$1-1-any.pkg.tar.zst /tmp/
}
mkpkg 1 "package data"; mkpkg 2 "package data"; mkpkg 3 "package data, new"
V1=/tmp/tp-1-1-any.pkg.tar.zst; V2=/tmp/tp-2-1-any.pkg.tar.zst; V3=/tmp/tp-3-1-any.pkg.tar.zst
pacman -U --noconfirm $V1 >/dev/null 2>&1

# -------------------------------------------------- the problem, without a policy
chmod 4750 /usr/bin/tp-tool; chown nobody:users /usr/bin/tp-tool; echo mine > /usr/share/tp/data
pacman -U --noconfirm $V2 >/dev/null 2>&1
ck "without a policy pacman resets the mode and owner and replaces the edited file (the problem)" "[ \"\$(st /usr/bin/tp-tool)\" = '755 root:root' ] && [ \"\$(cat /usr/share/tp/data)\" = 'package data' ]"

# ------------------------------------------------------------------- override
ck "override set applies at once and records the policy and a hook" "$e override set /usr/bin/tp-tool --mode 4750 --owner nobody --group users --allow-setuid --note 'helper' >/tmp/o1 2>&1; [ \"\$(st /usr/bin/tp-tool)\" = '4750 nobody:users' ] && grep -q 'path = \"/usr/bin/tp-tool\"' /etc/emrac/policy.toml && grep -q 'Target = usr/bin/tp-tool' /etc/pacman.d/hooks/zz-emrac-policy.hook && grep -q 'Exec = /usr/local/bin/emrac policy-helper apply' /etc/pacman.d/hooks/zz-emrac-policy.hook"
pacman -U --noconfirm $V3 >/tmp/up.out 2>&1
ck "the mode and owner survive an upgrade" "[ \"\$(st /usr/bin/tp-tool)\" = '4750 nobody:users' ]"
ck "pacman showed the hook and it said what it did" "grep -q \"Applying emrac's local file policy\" /tmp/up.out"
chmod 755 /usr/bin/tp-tool; chown root:root /usr/bin/tp-tool; pacman -U --noconfirm $V3 >/dev/null 2>&1
ck "and a reinstall of the same version" "[ \"\$(st /usr/bin/tp-tool)\" = '4750 nobody:users' ]"
ck "override list says it is in effect" "$e override list 2>&1 | grep -q 'in effect.*tp-tool'"
ck "check (verify) does not call the overridden mode damage" "$e check tp >/tmp/chk 2>&1"
chmod 755 /usr/bin/tp-tool
ck "list and status say when it is not in effect" "$e override list 2>&1 | grep -q 'NOT IN EFFECT' && ! $e policy status >/dev/null 2>&1 || $e policy status 2>&1 | grep -q 'NOT IN EFFECT'"
ck "policy apply sets it right again" "$e policy apply >/dev/null 2>&1; [ \"\$(st /usr/bin/tp-tool)\" = '4750 nobody:users' ]"

# ------------------------------------------------------------------ refusals
ck "a relative path is refused" "! $e override set usr/bin/tp-tool --mode 755 2>&1 | grep -q . || ! $e override set usr/bin/tp-tool --mode 755 >/dev/null 2>&1"
ck "a path with .. is refused" "! $e override set /usr/../etc/shadow --mode 777 >/dev/null 2>&1"
ck "a bad mode is refused" "! $e override set /usr/bin/tp-tool --mode 99999 >/dev/null 2>&1"
ck "an override that sets nothing is refused" "! $e override set /usr/bin/tp-tool >/dev/null 2>&1"
ck "an unknown user is refused" "! $e override set /usr/bin/tp-tool --mode 755 --owner no-such-user >/dev/null 2>&1"
ck "a setuid mode without consent changes nothing" "$e override set /usr/bin/tp-tool --mode 4755 </dev/null >/tmp/o2 2>&1; grep -q 'mode = \"4750\"' /etc/emrac/policy.toml"
ck "a file in /proc is refused" "! $e override set /proc/1/mem --mode 777 >/dev/null 2>&1"
ck "emrac run from a place root may not trust refuses to write a hook" "cp $e /tmp/emrac-copy && ! /tmp/emrac-copy override set /usr/share/tp/data --mode 600 >/tmp/o3 2>&1; grep -q 'not where root may run emrac from' /tmp/o3; ! grep -q 'usr/share/tp/data' /etc/emrac/policy.toml"
ck "diverting a backup= file is refused and points to pacnew" "! $e divert add /etc/tp.conf --allow-stale >/tmp/o4 2>&1; grep -q 'backup= file' /tmp/o4 && grep -q pacnew /tmp/o4"
ck "diverting a file no package owns is refused" "echo x > /usr/local/share/loose; ! $e divert add /usr/local/share/loose --allow-stale >/tmp/o5 2>&1; grep -q 'no installed package owns' /tmp/o5"
ck "diverting a directory is refused" "! $e divert add /usr/share/tp --allow-stale >/tmp/o6 2>&1"

# ------------------------------------------------------------------- divert
pacman -U --noconfirm $V2 >/dev/null 2>&1   # the package at "package data" (version 2): what the diversion is made against
echo "my local data" > /usr/share/tp/data; chmod 640 /usr/share/tp/data
ck "diverting asks first, says it freezes the file and security fixes, and changes nothing without a yes" "$e divert add /usr/share/tp/data </dev/null >/tmp/d0 2>&1; grep -q 'security fixes' /tmp/d0 && grep -q 'NOT take effect' /tmp/d0 && ! grep -q 'usr/share/tp/data' /etc/emrac/policy.toml 2>/dev/null"
ck "--yes does not answer that question" "$e --yes divert add /usr/share/tp/data </dev/null >/tmp/d0b 2>&1; ! grep -q 'usr/share/tp/data' /etc/emrac/policy.toml 2>/dev/null"
ck "the root helper itself refuses without the consent" "! $e policy-helper add-divert /usr/share/tp/data >/tmp/d0c 2>&1; grep -q 'explicit consent' /tmp/d0c"
ck "a compiled program is called out as the kind security fixes ship in" "$e divert add /usr/bin/ls </dev/null >/tmp/d0d 2>&1; grep -q 'compiled program or library' /tmp/d0d"
ck "divert add keeps your file and records it" "$e divert add /usr/share/tp/data --note 'patched' --allow-stale >/tmp/d1 2>&1; grep -q 'path = \"/usr/share/tp/data\"' /etc/emrac/policy.toml && [ \"\$(stat -c %a /var/lib/emrac/divert)\" = 700 ]"
ck "an unchanged-upstream upgrade keeps your file, content and mode" "pacman -U --noconfirm $V2 >/tmp/up2.out 2>&1; [ \"\$(cat /usr/share/tp/data)\" = 'my local data' ] && [ \"\$(stat -c %a /usr/share/tp/data)\" = 640 ]"
ck "and the package's version is set aside" "pacman -U --noconfirm $V3 >/tmp/up3.out 2>&1; [ \"\$(cat /usr/share/tp/data)\" = 'my local data' ] && $e divert diff /usr/share/tp/data 2>&1 | grep -q 'package data, new'"
ck "pacman's own output warns that the package's change is NOT in use" "grep -q 'package changed this file' /tmp/up3.out && grep -q 'security fixes' /tmp/up3.out"
ck "divert list marks it STALE and says what to do" "$e divert list 2>&1 | grep -q 'STALE.*usr/share/tp/data' && $e divert list 2>&1 | grep -q 'divert reviewed'"
ck "doctor warns about the stale diversion" "$e doctor 2>&1 | grep -q 'Diverted files'"
ck "policy status is not all clear while it is stale" "$e policy status 2>&1 | grep -q STALE"
ck "reviewed clears it, and the next transaction does not warn again" "$e divert reviewed /usr/share/tp/data >/dev/null 2>&1; ! $e divert list 2>&1 | grep -q STALE && pacman -U --noconfirm $V3 >/tmp/up3b.out 2>&1; ! grep -q 'package changed this file' /tmp/up3b.out"
ck "divert list says it is in effect" "$e divert list 2>&1 | grep -q 'in effect.*usr/share/tp/data'"
echo "edited again" > /usr/share/tp/data
ck "a later edit of the diverted file is kept as yours" "pacman -U --noconfirm $V1 >/dev/null 2>&1; [ \"\$(cat /usr/share/tp/data)\" = 'edited again' ]"
ck "check does not call a diverted file damaged" "$e check tp >/dev/null 2>&1"
ck "removing the package leaves no stray file, and says your copy waits in the store" "pacman -R --noconfirm tp >/tmp/rm.out 2>&1; ! test -e /usr/share/tp/data && grep -q 'waits in the store' /tmp/rm.out && test -f \$(ls -d /var/lib/emrac/divert/*/ | head -1)local"
ck "reinstalling the package works (no conflicting file) and brings your copy back" "pacman -U --noconfirm $V1 >/tmp/re.out 2>&1 && [ \"\$(cat /usr/share/tp/data)\" = 'edited again' ]"
ck "divert remove lets go: the next reinstall replaces the file" "$e divert remove /usr/share/tp/data >/dev/null 2>&1; pacman -U --noconfirm $V1 >/dev/null 2>&1; [ \"\$(cat /usr/share/tp/data)\" = 'package data' ] && ! grep -q 'usr/share/tp/data' /etc/emrac/policy.toml"

# ---------------------------------------------------------------------- hook
ck "a deleted hook is reported" "rm -f /etc/pacman.d/hooks/zz-emrac-policy.hook; $e policy status 2>&1 | grep -qi 'hook is missing'"
ck "policy apply writes it again" "$e policy apply >/dev/null 2>&1; test -f /etc/pacman.d/hooks/zz-emrac-policy.hook"
ck "removing the last entry removes both hooks" "$e override remove /usr/bin/tp-tool >/dev/null 2>&1; ! test -e /etc/pacman.d/hooks/zz-emrac-policy.hook && ! test -e /etc/pacman.d/hooks/zz-emrac-policy-pre.hook && ! grep -q '\[\[override\]\]' /etc/emrac/policy.toml"

# ---------------------------------------------------------- plans show it (needs a repository)
mkdir -p /srv/fixrepo
mkpkg 10 "package data, much newer"
cp /tmp/tp-10-1-any.pkg.tar.zst /srv/fixrepo/ && (cd /srv/fixrepo && repo-add fixrepo.db.tar.gz tp-10-1-any.pkg.tar.zst >/dev/null 2>&1)
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
pacman -Sy --noconfirm >/dev/null 2>&1
pacman -U --noconfirm $V1 >/dev/null 2>&1
$e override set /usr/bin/tp-tool --mode 4750 --allow-setuid >/dev/null 2>&1
echo "kept data" > /usr/share/tp/data; $e divert add /usr/share/tp/data --allow-stale >/dev/null 2>&1
$e upgrade tp --dry-run --yes >/tmp/pv1 2>&1
ck "the upgrade plan lists the override it will re-apply" "grep -q 'Your local file policy' /tmp/pv1 && grep -E 'override /usr/bin/tp-tool.*sets mode 4750 back' /tmp/pv1"
ck "and the diverted file, saying the package's changes may not take effect" "grep -E 'divert /usr/share/tp/data.*NOT take effect' /tmp/pv1"
pacman -Sw --noconfirm tp >/dev/null 2>&1
$e upgrade tp --dry-run --yes >/tmp/pv2 2>&1
ck "with the package downloaded it is exact, and a warning names the file" "grep -E 'DIFFERENT version' /tmp/pv2 && grep -E 'the package changes a file you diverted' /tmp/pv2"
rm -f /etc/pacman.d/hooks/zz-emrac-policy.hook
$e upgrade tp --dry-run --yes >/tmp/pv3 2>&1
ck "a missing hook is flagged in the plan: the policy would not be re-applied" "grep -q 'will NOT be re-applied' /tmp/pv3"
$e policy apply >/dev/null 2>&1
ck "--json carries the policy" "$e upgrade tp --dry-run --yes --json 2>/dev/null | grep -q '\"policy\"' && $e upgrade tp --dry-run --yes --json 2>/dev/null | grep -q 'divert_misses_changes'"
ck "upgrading for real keeps the override and your file, and warns" "pacman -Su --noconfirm >/tmp/up10.out 2>&1; [ \"\$(st /usr/bin/tp-tool)\" = '4750 root:root' ] && [ \"\$(cat /usr/share/tp/data)\" = 'kept data' ] && grep -q 'package changed this file' /tmp/up10.out"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

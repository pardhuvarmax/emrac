#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh preview). Checks what
# emrac says a plan will do beyond its packages against what pacman then really
# does: the config files left as .pacnew or .pacsave (a small local repository
# with real backup= files), and the hooks (probe hooks placed next to the system
# ones: an install, a removal and a full upgrade, in order, and the hook that
# would stop a transaction).
#
# last full run: after u5, 22 of 22 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
H=/etc/pacman.d/hooks; mkdir -p $H


# ---------------------------------------------------------------------- hooks (first: the image still has many upgrades waiting)
# file lists for path triggers (`pacman -Fy`), and a system that is up to date
# except for what the upgrade check below needs: leave it as the image is
pacman -Sy --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed diffutils >/dev/null 2>&1
pacman -Fy >/dev/null 2>&1

mk() { # file when ops type description extra-action-line targets...
  local f=$1 when=$2 ops=$3 type=$4 desc=$5 extra=$6; shift 6
  { echo "[Trigger]"; for o in ${ops//,/ }; do echo "Operation = $o"; done; echo "Type = $type"
    for t in "$@"; do echo "Target = $t"; done
    echo "[Action]"; echo "Description = $desc"; echo "When = $when"; echo "Exec = /usr/bin/true"
    [ -n "$extra" ] && echo "$extra"; } > $H/$f
}
# the hooks emrac predicts, split by pass, from its text output
predict() { # emrac args...
  : > /tmp/e.pre; : > /tmp/e.post
  $e "$@" --dry-run --yes >/tmp/emrac.out 2>&1
  grep -E '^  before ' /tmp/emrac.out | sed -E 's/^  before +[^ ]+ +//; s/ +\[stops.*//' > /tmp/e.pre
  grep -E '^  after  ' /tmp/emrac.out | sed -E 's/^  after +[^ ]+ +//' > /tmp/e.post
}
# the hooks pacman then runs, split by pass, from its output
real() { # pacman args...
  : > /tmp/p.pre; : > /tmp/p.post
  pacman "$@" --noconfirm >/tmp/pacman.out 2>&1
  awk '/Running pre-transaction/{m="pre";next} /Running post-transaction/{m="post";next} /^:: /{m=""}
       m!="" && /^\( *[0-9]+\/[0-9]+\) /{sub(/^\( *[0-9]+\/[0-9]+\) /,""); print > "/tmp/p." m}' /tmp/pacman.out
}

# probes: package and path triggers, both passes, a negation that must not fire
mk 01-pkg-pre.hook  PreTransaction  Install         Package "PROBE pkg-pre"  "" figlet
mk 02-path-pre.hook PreTransaction  Install         Path    "PROBE path-pre" "" 'usr/bin/figlet'
mk 03-pkg-post.hook PostTransaction Install,Upgrade Package "PROBE pkg-post" "" 'fig*'
mk 04-neg.hook      PostTransaction Install         Package "PROBE negated"  "" '*' '!figlet'
mk 06-path-post.hook PostTransaction Install        Path    "PROBE path-post" "" 'usr/bin/fig*'
mk 07-rm-pkg.hook   PostTransaction Remove          Package "PROBE remove-pkg" "" figlet
mk 08-rm-path.hook  PostTransaction Remove          Path    "PROBE remove-path" "" 'usr/bin/fig*'
mk 09-upg-pkg.hook  PostTransaction Upgrade         Package "PROBE upgrade-pkg" "" '*'
mk 10-upg-pre.hook  PreTransaction  Upgrade         Package "PROBE upgrade-pre" "" '*'
mk 05-dep-abort.hook PreTransaction Install         Package "PROBE needs-missing" $'Depends = no-such-pkg\nAbortOnFail' figlet

ck "an AbortOnFail hook whose Depends is missing is flagged as stopping the transaction" \
   "$e install --dry-run --yes figlet 2>&1 | grep -q 'pacman will stop the transaction'"
ck "the flagged hook is marked in the list" \
   "$e install --dry-run --yes figlet 2>&1 | grep -E 'before .*05-dep-abort' | grep -q 'stops the transaction if it fails'"
ck "--json carries the hooks, in order, with missing_depends" \
   "$e install --dry-run --yes --json figlet 2>/dev/null | grep -q '\"missing_depends\": \\[' && $e install --dry-run --yes --json figlet 2>/dev/null | grep -q '\"hooks\"'"
rm -f $H/05-dep-abort.hook

predict install figlet
ck "install: a negated target does not fire" "! grep -q negated /tmp/e.post"
ck "install: the path hook fires on a file of the package" "grep -q 'PROBE path-pre' /tmp/e.pre && grep -q 'PROBE path-post' /tmp/e.post"
real -S figlet
ck "install: emrac predicted the hooks pacman ran before the transaction, in order" "diff /tmp/e.pre /tmp/p.pre"
ck "install: emrac predicted the hooks pacman ran after the transaction, in order" "diff /tmp/e.post /tmp/p.post"

predict remove figlet
real -R figlet
ck "remove: the hooks pacman ran are the hooks emrac predicted" "diff /tmp/e.pre /tmp/p.pre && diff /tmp/e.post /tmp/p.post"

predict upgrade
real -Su
ck "upgrade: the hooks pacman ran are the hooks emrac predicted" "[ -s /tmp/p.post ] && diff /tmp/e.pre /tmp/p.pre && diff /tmp/e.post /tmp/p.post"
ck "upgrade: more than one hook fired (the comparison means something)" "[ \$(wc -l < /tmp/p.post) -gt 2 ]"

# ---------------------------------------------------------------- config files
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }
# (the hook part above ended with a full upgrade, so the system is current)
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkpkg() { # name version conf-content [drop]   a package whose etc/<name>.conf is a backup file
  local n=$1 v=$2 c=$3 drop=${4:-} d=/tmp/mk-$1-$2; rm -rf $d; mkdir -p $d; chown tester $d
  { echo "pkgname=$n"; echo "pkgver=$v"; echo "pkgrel=1"; echo "pkgdesc='fixture'"; echo "arch=(any)"; echo "license=(MIT)"
    [ -z "$drop" ] && echo "backup=(etc/$n.conf)"
    echo "package() {"; echo "  install -Dm644 /dev/null \"\$pkgdir/usr/share/$n/$v\""
    [ -z "$drop" ] && echo "  echo '$c' | install -Dm644 /dev/stdin \"\$pkgdir/etc/$n.conf\""
    echo "}"; } > $d/PKGBUILD
  chown tester $d/PKGBUILD
  (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$n-$v-1-any.pkg.tar.zst /srv/fixrepo/
}
publish() { (cd /srv/fixrepo && repo-add fixrepo.db.tar.gz "$@" >/dev/null 2>&1); pacman -Sy --noconfirm >/dev/null 2>&1; pacman -Fy >/dev/null 2>&1; }
mkdir -p /srv/fixrepo
mkpkg cfgchg 1 base; mkpkg cfgsame 1 base; mkpkg cfggone 1 base
(cd /srv/fixrepo && repo-add fixrepo.db.tar.gz cfgchg-1-1-any.pkg.tar.zst cfgsame-1-1-any.pkg.tar.zst cfggone-1-1-any.pkg.tar.zst >/dev/null 2>&1)
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
pacman -Sy --noconfirm >/dev/null 2>&1
pacman -S --noconfirm cfgchg cfgsame cfggone >/dev/null 2>&1
# version 2: one changes its file, one does not, one stops shipping it
mkpkg cfgchg 2 newer; mkpkg cfgsame 2 base; mkpkg cfggone 2 base drop
(cd /srv/fixrepo && rm -f cfgchg-1-1-any.pkg.tar.zst cfgsame-1-1-any.pkg.tar.zst cfggone-1-1-any.pkg.tar.zst)
publish cfgchg-2-1-any.pkg.tar.zst cfgsame-2-1-any.pkg.tar.zst cfggone-2-1-any.pkg.tar.zst
ck "fixtures: three packages installed at 1 with a version 2 waiting" "pacman -Qu | grep -c cfg | grep -q 3"

# untouched files: nothing to tell
ck "untouched config files are not listed" "! as_tester $e upgrade cfgchg --dry-run --yes 2>&1 | grep -q 'Config files you changed'"
for f in cfgchg cfgsame cfggone; do echo "mine" > /etc/$f.conf; done
# the new packages are not downloaded yet: an edited file is a maybe
$e upgrade --dry-run --yes >/tmp/up1 2>&1
ck "edited files are listed before the download as maybes" "grep -q 'Config files you changed' /tmp/up1 && grep -E 'cfgchg.*may get a <file>.pacnew' /tmp/up1 && grep -E 'cfgsame.*may get a <file>.pacnew' /tmp/up1"
ck "a file the new package stops shipping is listed as kept (.pacsave)" "grep -E 'cfggone.*pacsave' /tmp/up1"
ck "--json carries them" "$e upgrade --dry-run --yes --json 2>/dev/null | grep -q '\"config\"' && $e upgrade --dry-run --yes --json 2>/dev/null | grep -q 'maybe_pacnew'"
# with the packages in the cache the answer is exact
pacman -Sw --noconfirm cfgchg cfgsame cfggone >/dev/null 2>&1
$e upgrade --dry-run --yes >/tmp/up2 2>&1
ck "with the packages cached, a changed file is a certain .pacnew" "grep -E 'cfgchg.*will be written as <file>.pacnew' /tmp/up2"
ck "with the packages cached, an unchanged file is not listed" "! grep -E 'cfgsame' /tmp/up2 | grep -q 'pacnew'"
pacman -Su --noconfirm >/dev/null 2>&1
ck "pacman did exactly that: .pacnew only for the changed file, yours kept" "[ -f /etc/cfgchg.conf.pacnew ] && [ ! -f /etc/cfgsame.conf.pacnew ] && [ \"\$(cat /etc/cfgchg.conf)\" = mine ] && [ \"\$(cat /etc/cfgsame.conf)\" = mine ]"
ck "pacman saved the dropped file as .pacsave" "[ -f /etc/cfggone.conf.pacsave ] && [ ! -f /etc/cfggone.conf ]"
# removal
echo "mine" > /etc/cfgsame.conf
ck "a removal lists an edited file as .pacsave" "$e remove cfgsame --dry-run --yes 2>&1 | grep -E 'cfgsame.*pacsave'"
pacman -R --noconfirm cfgsame >/dev/null 2>&1
ck "pacman kept it as .pacsave" "[ -f /etc/cfgsame.conf.pacsave ]"
# NoUpgrade
sed -i '/^\[options\]/a NoUpgrade = etc/cfgchg.conf' /etc/pacman.conf
mkpkg cfgchg 3 newer3; publish cfgchg-3-1-any.pkg.tar.zst
(cd /srv/fixrepo && rm -f cfgchg-2-1-any.pkg.tar.zst)
cp /etc/cfgchg.conf.pacnew /tmp/keep 2>/dev/null; rm -f /etc/cfgchg.conf.pacnew
ck "NoUpgrade makes an existing file a certain .pacnew" "$e upgrade --dry-run --yes 2>&1 | grep -E 'cfgchg.*will be written as <file>.pacnew.*NoUpgrade'"
sed -i '/^NoUpgrade = etc\/cfgchg.conf/d' /etc/pacman.conf


echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

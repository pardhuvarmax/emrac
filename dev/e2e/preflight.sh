#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh preflight). Builds a
# small local repository so that an upgrade is really waiting, then checks that
# emrac refuses a partial upgrade, lets --allow-partial and a pin through, and
# refuses to remove the only kernel (a fake one).
#
# last full run: after u20, 24 of 24 passed
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

# plan integrity (u18): what was previewed is what runs
newer() { # name version: put a newer version of a fixture package in the repository
  mk $1 $2 ''
  (cd /srv/fixrepo && rm -f $1-*-1-any.pkg.tar.zst.old; for f in $1-*-1-any.pkg.tar.zst; do case $f in $1-$2-1-any.pkg.tar.zst) ;; *) rm -f $f;; esac; done; repo-add fixrepo.db.tar.gz $1-$2-1-any.pkg.tar.zst >/dev/null 2>&1)
  pacman -Sy --noconfirm >/dev/null 2>&1
}
ck "every plan shows its id, and --json has the digest" "as_tester $e install fixnew --dry-run 2>&1 | grep -q 'Plan id' && as_tester $e install fixnew --dry-run --json 2>/dev/null | grep -q '\"digest\": \"[0-9a-f]\\{64\\}\"'"
D=$(as_tester $e install fixnew --dry-run --json 2>/dev/null | sed -n 's/.*"digest": "\([0-9a-f]*\)".*/\1/p' | head -1)
ck "--expect-plan with the right id runs the plan" "as_tester $e install fixnew --yes --expect-plan ${D:0:12} && pacman -Q fixnew"
ck "the history record carries the plan's digest" "as_tester $e history --json | grep -q '\"plan_digest\": \"${D}\"'"
pacman -R --noconfirm fixnew >/dev/null 2>&1
newer fixnew 2.0
ck "--expect-plan with the old id is refused after the repository changed" "! as_tester $e install fixnew --yes --expect-plan ${D:0:12} >/tmp/o 2>&1; grep -q 'not the plan you expected' /tmp/o && ! pacman -Q fixnew"
ck "a malformed plan id is refused" "! as_tester $e install fixnew --dry-run --expect-plan zz >/tmp/o 2>&1; grep -q 'not a plan id' /tmp/o"
# the repository changes while the plan waits for an answer (more than 30 s)
( sleep 36; echo y ) | as_tester $e install fixnew > /tmp/drift 2>&1 &
sleep 8
newer fixnew 3.0
wait
ck "a plan that changed while it waited is refused at the commit, with the difference" "cat /tmp/drift; grep -q 'changed while you were deciding' /tmp/drift && grep -q 'changed: fixnew 2.0-1 → 3.0-1' /tmp/drift && ! pacman -Q fixnew"

# the Arch news gate (u20): critical news stops an upgrade, other news is only shown
feed() { # title text: a one-item feed published now
  cat > /tmp/feed.xml <<XML
<?xml version="1.0"?><rss><channel><item><title>$1</title><link>https://archlinux.org/news/test/</link><pubDate>$(date -R -u)</pubDate><description>$2</description></item></channel></rss>
XML
}
newer fixnew2 2.0
feed "Kernel and initramfs changes need manual intervention" "Regenerate the initramfs before rebooting."
ck "an upgrade is stopped by unread critical news and nothing changes" "! as_tester env EMRAC_NEWS_FEED=file:///tmp/feed.xml $e upgrade --yes >/tmp/o 2>&1; grep -q 'critical' /tmp/o && grep -q 'emrac news ack' /tmp/o && pacman -Q fixnew2 | grep -q '1.0'"
ck "--ack-news goes ahead" "as_tester env EMRAC_NEWS_FEED=file:///tmp/feed.xml $e upgrade --yes --ack-news >/tmp/o 2>&1; pacman -Q fixnew2 | grep -q '2.0'"
newer fixnew2 3.0
feed "A new mirror in Iceland" "A new mirror is available."
ck "news that is not critical is shown and does not stop the upgrade" "as_tester env EMRAC_NEWS_FEED=file:///tmp/feed.xml $e upgrade --yes >/tmp/o 2>&1; grep -q \"haven't seen\" /tmp/o && pacman -Q fixnew2 | grep -q '3.0'"
ck "an upgrade shows its update risk and ends with the report" "grep -q 'Update risk' /tmp/o && grep -q -e 'after the upgrade' -e 'reboot is advised' /tmp/o"
ck "emrac report works on the last upgrade in the history" "as_tester $e report >/tmp/r 2>&1; grep -q 'report for tx-' /tmp/r"

# protecting pacman's database (u20): a leftover lock, a database that is already broken
touch /var/lib/pacman/db.lck
ck "a leftover pacman lock is a pre-flight warning with the command" "as_tester $e install fixnew --dry-run >/tmp/o 2>&1; grep -q 'db.lck' /tmp/o && grep -q 'sudo rm' /tmp/o"
rm -f /var/lib/pacman/db.lck
mkdir -p /var/lib/pacman/local/zzbroken-1.0-1 && echo '%NAME%
zzbroken' > /var/lib/pacman/local/zzbroken-1.0-1/desc
ck "a database that already has problems is reported before a transaction, and a backup is made" "pacman -Dk >/dev/null 2>&1; as_tester $e install fixnew --yes >/tmp/o 2>&1; grep -q 'already reports problems' /tmp/o && ls /home/tester/.local/share/emrac/pacman-db/local-*.tar.gz >/dev/null"
rm -rf /var/lib/pacman/local/zzbroken-1.0-1
ck "the backup is a real archive of the database without the mtree files" "f=\$(ls -t /home/tester/.local/share/emrac/pacman-db/local-*.tar.gz | head -1); tar -tzf \$f | grep -q 'local/fixa-.*/desc' && ! tar -tzf \$f | grep -q mtree"

# repository trust (u20): an older version than the installed one, a database older than one seen
newer fixnew2 1.0
ck "installing an older version than the installed one is a pre-flight problem naming it" "as_tester $e install fixnew2 --dry-run >/tmp/o 2>&1; grep -q 'Older version' /tmp/o && grep -q 'fixnew2: installed 3.0-1, this plan has 1.0-1' /tmp/o"
mkdir -p /home/tester/.local/share/emrac && echo '{"fixrepo": 9999999999}' > /home/tester/.local/share/emrac/repo-marks.json && chown tester /home/tester/.local/share/emrac/repo-marks.json
ck "a repository database older than one seen is a warning" "as_tester $e install fixnew --dry-run >/tmp/o 2>&1; grep -q 'older than one you have seen' /tmp/o && grep -q 'fixrepo:' /tmp/o"
echo; echo "$n checks, $bad failed"

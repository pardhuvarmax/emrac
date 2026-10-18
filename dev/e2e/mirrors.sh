#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh mirrors): `--apply` uses
# sudo, which only root may use without a password here. Real mirrors over the
# network, plus one address that is certain to fail.
#
# last full run: after u16, 23 of 23 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
W=/tmp/mw; rm -rf $W; mkdir -p $W; cd $W
export HOME=/root; rm -f $HOME/.local/share/emrac/mirrors.jsonl

cat > $W/list <<'EOT'
##
## Test mirror list
##

## Mixed
Server = https://127.0.0.1:1/$repo/os/$arch
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
#Server = https://commented.example/$repo/os/$arch
Server = https://fastly.mirror.pkgbuild.com/$repo/os/$arch
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch
EOT

ck "the list shows the active mirrors, none measured yet" "$e mirrors --file $W/list 2>&1 | grep -q '4 active mirrors' && $e mirrors --file $W/list 2>&1 | grep -q untested"
ck "rank before any measurement says to test first" "$e mirrors rank --file $W/list 2>&1 | grep -q 'run .emrac mirrors test. first'"
ck "a file with no active mirror is refused" "printf '#Server = https://x/\$repo\n' > $W/none; $e mirrors --file $W/none 2>&1 | grep -q 'no active Server line'"
ck "a missing file is refused" "$e mirrors --file $W/nope 2>&1 | grep -q 'No such file'"
ck "test measures every active mirror and records them" "$e mirrors test --file $W/list >$W/t.out 2>&1; grep -q '3 of 4 answered' $W/t.out && wc -l < $HOME/.local/share/emrac/mirrors.jsonl | grep -qx 4"
ck "the one that cannot be reached is shown as FAILED" "grep -q 'FAILED' $W/t.out && grep -c 'ok ' $W/t.out | grep -qx 3"
ck "a working mirror reports its freshness when it has lastsync" "grep -E 'pkgbuild.com' $W/t.out | grep -qE '(min|h|d)\$|unknown\$'"
ck "--limit measures only the first ones" "$e mirrors test --file $W/list --limit 1 2>&1 | grep -q '0 of 1 answered' && wc -l < $HOME/.local/share/emrac/mirrors.jsonl | grep -qx 5"
ck "the list now shows groups and numbers" "$e mirrors --file $W/list 2>&1 | grep -q 'unreliable' && $e mirrors --file $W/list 2>&1 | grep -q 'good'"
ck "rank puts the unreachable mirror last" "$e mirrors rank --file $W/list 2>&1 | awk '/^ +[0-9]+ /{print \$2}' | tail -1 | grep -q '127.0.0.1'"
ck "rank puts the working ones first and says the rule" "$e mirrors rank --file $W/list 2>&1 | awk '/^ +[0-9]+ /{print \$4}' | head -3 | grep -qv unreliable && $e mirrors rank --file $W/list 2>&1 | grep -q 'fewer than 8 of the last 10'"
ck "rank alone changes nothing" "cp $W/list $W/list.before; $e mirrors rank --file $W/list >/dev/null 2>&1; cmp $W/list $W/list.before"
ck "--write saves the proposal and keeps the header and the comments" "$e mirrors rank --file $W/list --write $W/ranked >/dev/null 2>&1; grep -q 'Test mirror list' $W/ranked && grep -q '^#Server = https://commented.example' $W/ranked && grep -q '^## Mixed' $W/ranked"
ck "the proposal has the same active mirrors and no others" "diff <(grep '^Server' $W/list | sort) <(grep '^Server' $W/ranked | sort)"
ck "the unreachable mirror comes last in the proposal" "grep '^Server' $W/ranked | tail -1 | grep -q '127.0.0.1'"
ck "--write will not overwrite a file" "$e mirrors rank --file $W/list --write $W/ranked 2>&1 | grep -q 'File exists'"
ck "--keep comments out the rest" "$e mirrors rank --file $W/list --keep 2 --write $W/kept >/dev/null 2>&1; [ \$(grep -c '^Server' $W/kept) = 2 ] && grep -q '^#Server = https://127.0.0.1:1' $W/kept"
ck "--json gives the ranking" "$e mirrors rank --file $W/list --json | grep -q '\"group\": \"unreliable\"'"

# apply (root, so sudo needs no password)
cp $W/list $W/live
ck "--apply without an answer changes nothing" "$e mirrors rank --file $W/live --apply </dev/null >/dev/null 2>&1; cmp $W/live $W/list"
ck "--apply --yes replaces the list with the ranked one" "$e mirrors rank --file $W/live --apply --yes </dev/null >$W/apply.out 2>&1; cmp $W/live $W/ranked && grep -q 'done. The old list is saved' $W/apply.out"
ck "the old list was saved first, unchanged" "ls $HOME/.local/share/emrac/mirrorlist-backups/ | grep -q '^mirrorlist-' && cmp \$(ls -d $HOME/.local/share/emrac/mirrorlist-backups/* | head -1) $W/list"
ck "ranking the ranked list again has nothing to change" "$e mirrors rank --file $W/live --apply --yes </dev/null 2>&1 | grep -q 'already in this order\\|Nothing to change'"
ck "the file mode is world-readable like a mirror list" "[ \$(stat -c %a $W/live) = 644 ]"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh timeline).
# Real transactions, a snapshot and a build-times fixture: `emrac timeline`
# must tell them as one story, by day, newest first, and filter it.
#
# last full run: after u10, 14 of 14 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
D=$HOME/.local/share/emrac; mkdir -p $D; rm -f $D/build-times.jsonl
now=$(date +%s)

ck "an empty history says so" "$e timeline 2>&1 | grep -q 'Nothing recorded'"

$e snapshot create tl-snap >/dev/null 2>&1
$e install sl --yes </dev/null >/dev/null 2>&1
$e install cowsay --yes </dev/null >/dev/null 2>&1
$e remove sl --yes </dev/null >/dev/null 2>&1
$e undo --yes </dev/null >/dev/null 2>&1
echo "{\"time\":$now,\"name\":\"fixture-pkg\",\"version\":\"2\",\"secs\":240,\"cores\":4,\"ok\":true}" > $D/build-times.jsonl
echo "{\"time\":$now,\"name\":\"broken-pkg\",\"version\":\"1\",\"secs\":30,\"cores\":4,\"ok\":false}" >> $D/build-times.jsonl

ck "today's events are under a Today heading" "$e timeline 2>&1 | head -1 | grep -qx 'Today'"
ck "installs and the removal are told as sentences" "$e timeline 2>&1 | grep -q 'Installed sl ' && $e timeline 2>&1 | grep -q 'Installed cowsay ' && $e timeline 2>&1 | grep -q 'Removed sl '"
ck "the undo shows as a rollback of the transaction" "$e timeline 2>&1 | grep -q 'Rolled back '"
ck "the snapshot is on the timeline with its package count" "$e timeline 2>&1 | grep -qE 'Snapshot \"tl-snap\" \([0-9]+ packages\)'"
ck "a build and a failed build are on it with their times" "$e timeline 2>&1 | grep -q 'Built fixture-pkg 2 in 4 min' && $e timeline 2>&1 | grep -q 'Build of broken-pkg 1 failed after 30 s'"
ck "events run newest first within the day" "$e timeline --json | grep -o '\"time\": [0-9]*' | awk '{print \$2}' | sort -rn -c"
ck "--pkg keeps only what names that package" "$e timeline --pkg sl 2>&1 | grep -q 'Installed sl ' && ! $e timeline --pkg sl 2>&1 | grep -q 'cowsay\|fixture-pkg'"
ck "--pkg for a package with no events says so" "$e timeline --pkg no-such-package 2>&1 | grep -q 'Nothing recorded for no-such-package'"
ck "--since 1h still shows today's events" "$e timeline --since 1h 2>&1 | grep -q 'Installed sl '"
ck "a date far in the future shows nothing" "$e timeline --since 2999-01-01 2>&1 | grep -q 'Nothing recorded'"
ck "--json is a list of events with a source" "$e timeline --json | grep -q '\"source\": \"history\"' && $e timeline --json | grep -q '\"source\": \"snapshots\"'"
ck "a bad period is refused" "$e timeline --since banana 2>&1 | grep -q 'neither a period'"
ck "it says the times are UTC" "$e timeline 2>&1 | grep -q 'Times are UTC'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

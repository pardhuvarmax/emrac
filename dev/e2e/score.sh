#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh score).
# `emrac score`: six dimensions from stated rules, unknown when there is no data,
# no overall number; the benchmark and your own builds feed it; an AUR package is
# audited when online.
#
# last full run: after u18, 18 of 18 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
pty() { script -qec "$1" /dev/null | tr -d '\r' | sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g'; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
D=$HOME/.local/share/emrac; mkdir -p $D; rm -f $D/benchmarks.jsonl $D/build-times.jsonl

ck "the card has a header and the six dimensions in order" "$e score pacman >/tmp/s1.out 2>&1; head -1 /tmp/s1.out | grep -q 'a report card, [0-9] of 6 known' && awk '/^  (Performance|Build time|Maintenance|Security|Popularity|Source) /{printf \"%s|\", \$1} END{print \"\"}' /tmp/s1.out | grep -q '^Performance|Build|Maintenance|Security|Popularity|Source|'"
ck "there is no overall score" "! grep -qi 'overall\\|total score' /tmp/s1.out && ! $e score pacman --json | grep -qi 'overall'"
ck "an official package's maintenance is its repository build age" "grep -E '^  Maintenance' /tmp/s1.out | grep -q 'rebuilt in the repository [0-9]* days ago'"
ck "its security is the signature, with the honest note under --details" "grep -E '^  Security' /tmp/s1.out | grep -q 'signed by an Arch packager' && $e score pacman --details | grep -q 'not a clean bill'"
ck "popularity is unknown for an official package, and says why" "grep -E '^  Popularity +- ' /tmp/s1.out | grep -q 'keep no popularity data'"
ck "performance is unknown before a benchmark and says what to run" "grep -E '^  Performance +- ' /tmp/s1.out | grep -q 'not measured' && grep -q 'run .emrac benchmark pacman.' /tmp/s1.out"
ck "after a benchmark performance has a number" "$e benchmark pacman --exe pacman --runs 3 --yes >/dev/null 2>&1; $e score pacman | grep -E '^  Performance +[0-9]+ ' | grep -q 'starts in'"
ck "and no hint is left for it" "! $e score pacman | grep -q 'run .emrac benchmark pacman.'"
CORES=$(nproc)
echo "{\"time\":$(date +%s),\"name\":\"pacman\",\"version\":\"1\",\"secs\":240,\"cores\":$CORES,\"ok\":true}" > $D/build-times.jsonl
ck "your own builds make build time measured" "$e score pacman | grep -E '^  Build time' | grep -q 'about 4 min on this machine (measured: your own builds)'"
ck "240 s scores 85 in the stated bands" "$e score pacman | grep -E '^  Build time +85 '"
ck "--details shows the rule behind a number" "$e score pacman --details | grep -q 'up to 1 min scores 100, 5 min 85'"
ck "--json is the card: six dimensions, unknown as null" "$e score pacman --json | grep -c '\"key\"' | grep -qx 6 && $e score nonexistent-xyz --json 2>&1 | grep -q \"couldn't find\""
ck "an unknown package is refused" "$e score no-such-package-xyz 2>&1 | grep -q \"couldn't find\""
ck "on a terminal there are bars" "pty \"env TERM=xterm LANG=C.UTF-8 $e score pacman\" | grep -E 'Maintenance' | grep -q '▇'"
ck "piped there are none" "! $e score pacman | grep -q '▇'"
ck "an AUR package is scored from the AUR and its PKGBUILD is audited" "$e score yay >/tmp/s2.out 2>&1; head -1 /tmp/s2.out | grep -q '(aur): a report card' && grep -E '^  Security' /tmp/s2.out | grep -q 'audit'"
ck "its popularity comes from the votes" "grep -E '^  Popularity +[0-9]+ ' /tmp/s2.out | grep -q 'votes, popularity'"
ck "its maintenance is when the AUR page last changed" "grep -E '^  Maintenance' /tmp/s2.out | grep -q 'last changed on the AUR [0-9]* days ago'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

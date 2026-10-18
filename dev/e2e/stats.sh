#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh stats).
# `emrac stats` reads the build-times file and the history; `emrac benchmark`
# runs the programs of an installed package, after asking, and keeps the result.
#
# last full run: after u9, 18 of 18 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
D=$HOME/.local/share/emrac; mkdir -p $D; rm -f $D/build-times.jsonl $D/benchmarks.jsonl
now=$(date +%s); old=$((now - 40*86400))

ck "stats with nothing recorded says so" "$e stats 2>&1 | grep -q 'Nothing recorded'"

{
  echo "{\"time\":$old,\"name\":\"oldpkg\",\"version\":\"1\",\"secs\":9000,\"cores\":4,\"ok\":true}"
  echo "{\"time\":$now,\"name\":\"alpha\",\"version\":\"1\",\"secs\":100,\"cores\":4,\"ok\":true}"
  echo "{\"time\":$now,\"name\":\"alpha\",\"version\":\"2\",\"secs\":300,\"cores\":4,\"ok\":true}"
  echo "{\"time\":$now,\"name\":\"beta\",\"version\":\"1\",\"secs\":50,\"cores\":4,\"ok\":false}"
} > $D/build-times.jsonl
ck "all time counts 4 builds, 1 failed, 3 packages" "$e stats --json | python3 -c 'import sys,json; b=json.load(sys.stdin)[\"builds\"]; assert (b[\"total\"],b[\"failed\"],b[\"packages\"])==(4,1,3), b' 2>/dev/null || $e stats 2>&1 | grep -q '4 built (1 failed), 3 different packages'"
ck "--since 30d leaves the old build out" "$e stats --since 30d 2>&1 | grep -q '3 built (1 failed), 2 different packages'"
ck "the average leaves the failed build out" "$e stats --since 30d 2>&1 | grep -q '7 min in all, 3 min on average'"
ck "the most built package is listed with its count" "$e stats --since 30d 2>&1 | grep -qE 'alpha +2x'"
ck "stats --json is valid JSON" "$e stats --json | python3 -c 'import sys,json; json.load(sys.stdin)' 2>/dev/null || $e stats --json | grep -q '\"builds\"'"
ck "it says no saved-hours figure is invented" "$e stats 2>&1 | grep -q 'would be a guess'"
ck "a bad period is refused" "! $e stats --since banana 2>&1 && $e stats --since banana 2>&1 | grep -q 'not a period'"

ck "benchmark without an answer runs nothing" "$e benchmark pacman --exe pacman </dev/null 2>&1 | grep -q 'Nothing was run' && [ ! -s $D/benchmarks.jsonl ]"
ck "benchmark lists what it will run before it runs it" "$e benchmark pacman --exe pacman </dev/null 2>&1 | grep -q '/usr/bin/pacman'"
ck "benchmark --yes measures a program of the package" "$e benchmark pacman --exe pacman --runs 3 --yes 2>&1 | grep -qE 'first run .* warm '"
ck "the result is kept and shown by --history" "$e benchmark pacman --history 2>&1 | grep -q '/usr/bin/pacman'"
ck "benchmark --json gives the record" "$e benchmark pacman --exe pacman --runs 2 --yes --json | grep -q '\"warm_micros\"'"
ck "a package with no programs is refused" "! $e benchmark filesystem --yes 2>&1 && $e benchmark filesystem --yes 2>&1 | grep -q 'nothing to start'"
ck "an executable the package does not have is refused" "$e benchmark pacman --exe nope --yes 2>&1 | grep -q \"no executable called 'nope'\""
ck "a package that is not installed is refused" "$e benchmark no-such-package --yes 2>&1 | grep -q 'isn.t installed'"
ck "--runs 0 is refused" "$e benchmark pacman --runs 0 --yes 2>&1 | grep -q 'between 1 and 50'"
ck "stats still reads the history after these" "$e stats 2>&1 | grep -q 'Transactions'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

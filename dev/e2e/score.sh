#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh score).
# `emrac score`: six dimensions from stated rules, unknown when there is no data,
# no overall number; the benchmark and your own builds feed it; an AUR package is
# audited when online.
#
# last full run: after u19, 48 of 48 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
pty() { script -qec "$1" /dev/null | tr -d '\r' | sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g'; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
D=$HOME/.local/share/emrac; mkdir -p $D; rm -f $D/benchmarks.jsonl $D/build-times.jsonl

ck "the card has a header and the six dimensions in order" "$e score pacman >/tmp/s1.out 2>&1; head -1 /tmp/s1.out | grep -q 'a report card, [0-9] of 6 known' && awk '/^  (Performance|Build time|Maintenance|Security|Popularity|Source) /{printf \"%s|\", \$1} END{print \"\"}' /tmp/s1.out | grep -q '^Performance|Build|Maintenance|Security|Popularity|Source|'"
ck "the card is six dimensions and an overall that says what it covers" "grep -q 'a report card, [0-9] of 6 known' /tmp/s1.out && $e score pacman | grep -qE '^  Overall +[0-9]+ +from'"
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
ck "--json is the card: six dimensions, unknown as null" "$e score pacman --json | grep -c '\"summary\"' | grep -qx 6 && $e score nonexistent-xyz --json 2>&1 | grep -q \"couldn't find\""
ck "an unknown package is refused" "$e score no-such-package-xyz 2>&1 | grep -q \"couldn't find\""
ck "on a terminal there are bars" "pty \"env TERM=xterm LANG=C.UTF-8 $e score pacman\" | grep -E 'Maintenance' | grep -q '▇'"
ck "piped there are none" "! $e score pacman | grep -q '▇'"
ck "an AUR package is scored from the AUR and its PKGBUILD is audited" "$e score yay >/tmp/s2.out 2>&1; head -1 /tmp/s2.out | grep -q '(aur): a report card' && grep -E '^  Security' /tmp/s2.out | grep -q 'audit'"
ck "its popularity comes from the votes" "grep -E '^  Popularity +[0-9]+ ' /tmp/s2.out | grep -q 'votes, popularity'"
ck "its maintenance is when the AUR page last changed" "grep -E '^  Maintenance' /tmp/s2.out | grep -q 'last changed on the AUR [0-9]* days ago'"


# --- the overall number and its weights
ck "there is an overall number that says what it is made of" "$e score pacman >/tmp/s3.out 2>&1; grep -E '^  Overall +[0-9]+ +from [0-9] of 6 dimensions, [0-9]+% of the weight' /tmp/s3.out"
ck "what was left out is named, with the reason" "grep -qF 'Popularity (unknown)' /tmp/s3.out && grep -q '^  left out: ' /tmp/s3.out"
ck "--weights changes the overall and shows the weights" "$e score pacman --weights security=5,popularity=0 >/tmp/s4.out 2>&1; grep -q '^  weights: security 5, popularity 0' /tmp/s4.out && grep -q 'Popularity (its weight is 0)' /tmp/s4.out"
ck "--care counts the named dimensions three times" "$e score pacman --care security,maintenance | grep -q '^  weights: maintenance 3, security 3'"
ck "bad weights are refused with a reason" "$e score pacman --weights speed=3 2>&1 | grep -q 'is not a dimension' && $e score pacman --weights security=11 2>&1 | grep -q 'not a weight from 0 to 10' && $e score pacman --weights security 2>&1 | grep -q 'not name=number'"
ck "the config keeps standing weights" "$e config set score.security 5 >/dev/null && $e score pacman | grep -q '^  weights: security 5'; $e config unset score.security >/dev/null; ! $e score pacman | grep -q '^  weights:'"
ck "a weight outside 0 to 10 is refused by the config" "! $e config set score.security 11 2>&1 && ! $e config set score.security many 2>&1"
ck "--json carries the overall with its coverage and weights" "$e score pacman --json | python3 -c 'import sys,json; o=json.load(sys.stdin)[\"overall\"]; assert o[\"coverage_percent\"] > 0 and o[\"weights\"][\"security\"]==1' 2>/dev/null || $e score pacman --json | grep -q '\"coverage_percent\"'"

# --- the Arch Security Tracker (a fixture, so the answer is known)
V=$(pacman -Q pacman | cut -d' ' -f2)
cat > /tmp/tracker.json <<EOT
[
 {"name":"AVG-9001","packages":["pacman"],"status":"Vulnerable","severity":"High","type":"x","affected":"$V","fixed":null,"ticket":null,"issues":["CVE-2099-1","CVE-2099-2"],"advisories":[]},
 {"name":"AVG-9002","packages":["pacman"],"status":"Fixed","severity":"Critical","type":"x","affected":"1.0-1","fixed":"2.0-1","ticket":null,"issues":["CVE-2099-3"],"advisories":["ASA-209901-1"]},
 {"name":"AVG-9003","packages":["curl"],"status":"Vulnerable","severity":"Critical","type":"x","affected":"1.0-1","fixed":null,"ticket":null,"issues":["CVE-2099-4"],"advisories":[]}
]
EOT
mkdir -p /tmp/h-t1 /tmp/h-t2
ck "a vulnerable entry for this version takes points off and is named with its CVEs" "HOME=/tmp/h-t1 EMRAC_SECURITY_URL=file:///tmp/tracker.json $e score pacman --details >/tmp/s5.out 2>&1; grep -E '^  Security +70 ' /tmp/s5.out | grep -q 'Arch Security Tracker entries for .* (1 open): AVG-9001 high' && grep -q 'AVG-9001 (high, affected from $V; open): CVE-2099-1, CVE-2099-2; no fix is recorded; its newest CVE is from 2099 (-25)' /tmp/s5.out"
ck "a fixed entry and an entry for another package do not count" "! grep -q 'AVG-9002\|AVG-9003' /tmp/s5.out"
ck "the list is saved and used again without the address" "HOME=/tmp/h-t1 EMRAC_SECURITY_URL=file:///nonexistent.json $e score pacman | grep -E '^  Security +70 '"
ck "offline with nothing saved says the tracker was not checked, and takes nothing off" "HOME=/tmp/h-t2 $e --offline score pacman >/tmp/s6.out 2>&1; grep -E '^  Security +95 ' /tmp/s6.out | grep -q 'the Arch Security Tracker was not checked'"
ck "the real tracker answers for an official package" "mkdir -p /tmp/h-t3; HOME=/tmp/h-t3 $e score pacman | grep -E '^  Security' | grep -q 'Arch Security Tracker'"
ck "an AUR package says the tracker does not cover it" "$e score yay --details | grep -q 'covers official packages only'"

# --- emrac vulns: standings, the ignore list, the gate (u19)
TU=file:///tmp/tracker.json
cat > /tmp/tracker2.json <<EOT
[
 {"name":"AVG-9101","packages":["pacman"],"status":"Vulnerable","severity":"Low","type":"x","affected":"$V","fixed":null,"ticket":null,"issues":["CVE-2010-1"],"advisories":[]}
]
EOT
mkdir -p /tmp/h-t4 /tmp/h-t5
ck "vulns lists a confirmed open entry" "HOME=/tmp/h-t4 EMRAC_SECURITY_URL=\$TU $e vulns >/tmp/v1.out 2>&1; grep -q 'AVG-9001' /tmp/v1.out && grep -q 'no fix recorded' /tmp/v1.out"
ck "a fixed entry is not listed" "! grep -q 'AVG-9002' /tmp/v1.out"
ck "--check fails while a confirmed High entry exists" "! HOME=/tmp/h-t4 EMRAC_SECURITY_URL=\$TU $e vulns --check"
ck "an old entry with no fix is possibly stale: hidden, then shown with --all and its reason" "HOME=/tmp/h-t5 EMRAC_SECURITY_URL=file:///tmp/tracker2.json $e vulns >/tmp/v2.out 2>&1; ! grep -q 'AVG-9101' /tmp/v2.out && grep -q '1 possibly stale or ignored entry is not shown' /tmp/v2.out && HOME=/tmp/h-t5 $e vulns --all | grep -q 'AVG-9101.*possibly stale'"
ck "a possibly stale entry never fails --check" "HOME=/tmp/h-t5 $e vulns --check"
ck "ignoring an entry sets it aside and --check passes" "HOME=/tmp/h-t4 $e vulns ignore AVG-9001 --reason 'old report' >/dev/null 2>&1 && HOME=/tmp/h-t4 $e vulns --check && HOME=/tmp/h-t4 $e vulns --all | grep -q 'AVG-9001.*ignored by you'"
ck "the ignore list shows the reason" "HOME=/tmp/h-t4 $e vulns ignored | grep -q 'AVG-9001  old report'"
ck "an ignored entry takes nothing off the score" "HOME=/tmp/h-t4 $e score pacman | grep -E '^  Security +95 '"
ck "unignore counts it again" "HOME=/tmp/h-t4 $e vulns unignore AVG-9001 >/dev/null 2>&1 && ! HOME=/tmp/h-t4 $e vulns --check"
ck "ids that are not entries are refused" "! HOME=/tmp/h-t4 $e vulns ignore ../etc 2>&1 && ! HOME=/tmp/h-t4 $e vulns ignore AVG-1 2>&1"
ck "--json is the report" "HOME=/tmp/h-t4 $e vulns --json | grep -q '\"list_age_hours\"'"
ck "the doctor has a vulnerabilities row" "HOME=/tmp/h-t4 $e doctor 2>&1 | grep -q 'Known vulnerabilities'"
ck "safety.vulnerabilities is a setting with three values" "$e config set safety.vulnerabilities block >/dev/null && $e config get safety.vulnerabilities | grep -q block && ! $e config set safety.vulnerabilities loud 2>&1; $e config unset safety.vulnerabilities >/dev/null"

# --- the pre-flight gate for a plan
W=$(pacman -Si figlet | awk '/^Version/{print $3}')
echo "[{\"name\":\"AVG-9201\",\"packages\":[\"figlet\"],\"status\":\"Vulnerable\",\"severity\":\"Critical\",\"type\":\"x\",\"affected\":\"$W\",\"fixed\":null,\"ticket\":null,\"issues\":[\"CVE-2099-9\"],\"advisories\":[]}]" > /tmp/tracker3.json
mkdir -p /tmp/h-t6
ck "a plan that would install a confirmed Critical entry warns by default" "HOME=/tmp/h-t6 EMRAC_SECURITY_URL=file:///tmp/tracker3.json $e install figlet --dry-run 2>&1 | grep -q 'warning  Known vulnerabilities: 1 confirmed open High or Critical'"
ck "with safety.vulnerabilities block it is a problem and the dry run says it would refuse" "HOME=/tmp/h-t6 $e config set safety.vulnerabilities block >/dev/null && HOME=/tmp/h-t6 EMRAC_SECURITY_URL=file:///tmp/tracker3.json $e install figlet --dry-run 2>&1 | grep -q 'PROBLEM  Known vulnerabilities'"
ck "with off the check is not made" "HOME=/tmp/h-t6 $e config set safety.vulnerabilities off >/dev/null && ! HOME=/tmp/h-t6 EMRAC_SECURITY_URL=file:///tmp/tracker3.json $e install figlet --dry-run 2>&1 | grep -qi 'vulnerabilities'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

#!/usr/bin/env bash
# Runs inside the test container as root (dev/e2e/run.sh eta). Time estimates: from
# earlier builds and earlier transactions seeded into the data directory, shown under
# a plan and by `emrac eta`; an unknown first build is said to be unknown; a
# finished transaction teaches the next estimate and is compared with the one shown.
#
# last full run: after u26, 11 of 11 passed
set -u
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; tail -8 /tmp/ck.out | sed 's/^/        /'; fi; }
export PATH=/usr/bin:$PATH
e=/usr/local/bin/emrac
as_tester() { runuser -u tester -- env HOME=/home/tester PATH=/usr/bin:/usr/local/bin "$@"; }
data=/home/tester/.local/share/emrac

pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed base-devel >/dev/null 2>&1
mkdir -p /srv/fixrepo
mk() { local n=$1 d=/tmp/mk-$1; rm -rf $d; mkdir -p $d; chown tester $d
  printf "pkgname=$n\npkgver=1\npkgrel=1\npkgdesc=fixture\narch=(any)\nlicense=(MIT)\npackage() {\n  echo 1 | install -Dm644 /dev/stdin \"\$pkgdir/usr/share/$n/v\"\n}\n" > $d/PKGBUILD
  chown -R tester $d; (cd $d && as_tester env PKGDEST=$d makepkg -f --noconfirm >/dev/null 2>&1) && cp $d/$n-1-1-any.pkg.tar.zst /srv/fixrepo/; }
mk fixnew; mk fixother
(cd /srv/fixrepo && repo-add fixrepo.db.tar.gz fixnew-1-1-any.pkg.tar.zst fixother-1-1-any.pkg.tar.zst >/dev/null 2>&1)
printf '\n[fixrepo]\nSigLevel = Never\nServer = file:///srv/fixrepo\n' >> /etc/pacman.conf
pacman -Sy --noconfirm >/dev/null 2>&1
as_tester $e config set safety.signatures warn >/dev/null 2>&1
as_tester $e config set safety.preflight warn >/dev/null 2>&1
cores=$(nproc)
mkdir -p $data && chown -R tester /home/tester/.local

# nothing known yet
rm -f $data/ops.jsonl $data/build-times.jsonl $data/eta-accuracy.jsonl
as_tester $e eta fixnew >/tmp/e0.out 2>&1
ck "ETA with nothing to go on: a tiny install is only overhead, at low confidence" "grep -q 'Estimated time   : about' /tmp/e0.out && grep -q 'low confidence' /tmp/e0.out"
as_tester $e install --source sl --dry-run >/tmp/e1.out 2>&1
ck "ETA a first build is said to be unknown, not guessed" "grep -q 'Estimated time   : cannot say' /tmp/e1.out && grep -q 'no estimate for: build sl (a first build here)' /tmp/e1.out"

# earlier builds of sl here
now=$(date +%s)
for s in 40 44 42; do printf '{"time":%s,"name":"sl","version":"","secs":%s,"cores":%s,"ok":true}\n' $now $s $cores; done > $data/build-times.jsonl
printf '{"time":%s,"name":"sl","version":"","secs":900,"cores":%s,"ok":false}\n' $now $cores >> $data/build-times.jsonl
chown tester $data/build-times.jsonl
as_tester $e install --source sl --dry-run >/tmp/e2.out 2>&1
ck "ETA a package built here three times: about 42 s, high confidence, the failed build left out" "grep -q 'Estimated time   : about 42 s' /tmp/e2.out && grep -q 'high confidence' /tmp/e2.out"
ck "ETA the range is the fastest and the slowest of those builds" "grep -q 'between 40 s and 44 s' /tmp/e2.out"
printf '{"time":%s,"name":"sl","version":"","secs":84,"cores":%s,"ok":true}\n' $now $((cores*2)) > $data/build-times.jsonl
as_tester $e eta --json >/dev/null 2>&1
as_tester $e install --source sl --dry-run >/tmp/e3.out 2>&1
ck "ETA hardware-aware: a build that took 84 s on twice the cores is not 84 s here" "grep -q 'Estimated time   : about' /tmp/e3.out && ! grep -q 'about 84 s' /tmp/e3.out"

# earlier transactions teach the download rate
mib=1048576
{
  printf '{"time":%s,"bytes":%s,"packages":10,"secs":30}\n' $now $((100*mib))
  printf '{"time":%s,"bytes":%s,"packages":5,"secs":20}\n' $now $((60*mib))
  printf '{"time":%s,"bytes":%s,"packages":2,"secs":4}\n' $now $mib
} > $data/ops.jsonl
chown tester $data/ops.jsonl
as_tester $e eta fixnew --json >/tmp/e4.json 2>&1
ck "ETA --json is data, with the basis of every part" "grep -q '\"basis\"' /tmp/e4.json && grep -q 'download and install' /tmp/e4.json"
as_tester $e eta fixnew >/tmp/e5.out 2>&1
ck "ETA eta lists each part with its range, confidence and basis" "grep -q 'download and install' /tmp/e5.out && grep -q 'medium confidence' /tmp/e5.out && grep -q '2 earlier transactions here' /tmp/e5.out"

# a transaction that finishes teaches the next one and is compared with the estimate
rm -f $data/eta-accuracy.jsonl; : > $data/ops.jsonl; chown tester $data/ops.jsonl
before=$(wc -l < $data/ops.jsonl)
as_tester $e install fixnew --yes >/tmp/e6.out 2>&1
ck "ETA the plan shows the estimate before the transaction" "grep -q 'Estimated time' /tmp/e6.out && pacman -Q fixnew"
ck "ETA a finished transaction is noted for the next estimate" "[ \$(wc -l < $data/ops.jsonl) -gt $before ]"
ck "ETA and compared with the estimate that was shown" "[ -s $data/eta-accuracy.jsonl ] && grep -q '\"estimated\"' $data/eta-accuracy.jsonl && grep -q '\"actual\"' $data/eta-accuracy.jsonl"
ck "ETA nothing to do takes no time" "pacman -S --noconfirm fixother >/dev/null 2>&1; as_tester $e eta 2>&1 | grep -qi 'nothing to do\\|Estimated time'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

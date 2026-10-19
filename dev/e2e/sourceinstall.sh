#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh sourceinstall).
# Source or binary on install: `emrac install --source` builds an official package
# from Arch's packaging source here (sandboxed, audited, inspected, installed with
# pacman -U), `--binary` compiles nothing, and `install.prefer` decides when you
# do not. Needs the network (the packaging source and the package's own source).
#
# The source-build policy: what was asked to be built from source stays so; an
# upgrade rebuilds it from the new packaging source instead of installing the binary.
#
# The binary here is the test build (--cfg emrac_fault): EMRAC_FAULT=<point> cuts emrac
# off at that point, as a kill or a power cut would. A normal build has no such code.
#
# last full run: after the fault checks, 48 of 48 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1; sudo pacman -S --noconfirm --needed bubblewrap base-devel git >/dev/null 2>&1
sudo pacman -R --noconfirm sl >/dev/null 2>&1

ck "the plan says an official package is built here and what that means" "$e install --source sl --dry-run > /tmp/s1.out 2>&1; grep -q 'extra+source/sl' /tmp/s1.out && grep -q 'built from source here, from Arch.s packaging source' /tmp/s1.out && ! pacman -Q sl"
ck "--binary on an AUR package refuses and names the prebuilt one" "$e install --binary yay --dry-run 2>&1 | grep -q 'prebuilt counterpart is yay-bin'"
ck "--source and --binary together are refused" "! $e install --source --binary sl --dry-run >/dev/null 2>&1"
ck "install.prefer is checked and defaults to the repository" "[ \"\$($e config get install.prefer)\" = repository ] && ! $e config set install.prefer sometimes >/dev/null 2>&1"
ck "by default the repository's binary is what the plan has" "$e install sl --dry-run 2>&1 | grep -q 'extra/sl' && ! $e install sl --dry-run 2>&1 | grep -q '+source'"
ck "a tag that does not exist is an error that says what was asked" "EMRAC_ARCH_PACKAGING_URL=file:///nonexistent/{base}.git $e install --source sl --yes 2>&1 | grep -qi 'git clone'; ! pacman -Q sl"

ck "--source builds sl here in the sandbox and installs the result" "$e install --source sl --yes --skip-pkgbuild > /tmp/s2.out 2>&1; pacman -Q sl"
ck "the packaging source it used is the repository's version" "grep -q '^pkgver=5.05' ~/.cache/emrac/build/arch-sl/PKGBUILD && [ \"\$(pacman -Q sl | cut -d' ' -f2)\" = \"\$(pacman -Si sl | awk '/^Version/{print \$3}')\" ]"
ck "what was installed was built here, not downloaded" "pacman -Qi sl | grep -q 'Unknown Packager'"
ck "it is remembered as built here, so an upgrade that would replace it can say so" "grep -q \"\\\"name\\\": \\\"sl\\\"\" ~/.local/share/emrac/source-built.json"
ck "the transaction is in the history" "$e history 2>&1 | head -3 | grep -q 'sl'"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "a binary install of the same package is the repository's file" "$e install --binary sl --yes >/tmp/s3.out 2>&1; pacman -Q sl && ! pacman -Qi sl | grep -q 'Unknown Packager'"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "install.prefer recommend uses the verdict of recommend, --binary still wins" "$e config set install.prefer recommend >/dev/null && $e install --binary sl --dry-run 2>&1 | grep -q 'extra/sl'; $e config unset install.prefer >/dev/null"

# --- the policy: an upgrade rebuilds from source, it does not install the binary
sudo pacman -R --noconfirm sl >/dev/null 2>&1
rm -f ~/.local/share/emrac/source-built.json
ck "from-source is empty to begin with and says how to start" "$e from-source 2>&1 | grep -q 'install --source'"
sudo pacman -S --noconfirm sl >/dev/null 2>&1
ck "from-source add declares the policy for an installed binary: the rebuild is pending" "$e from-source add sl >/dev/null 2>&1 && $e from-source 2>&1 | grep -q 'rebuild pending'"
ck "from-source drop ends it and leaves the package" "$e from-source drop sl >/dev/null 2>&1 && pacman -Q sl && $e from-source 2>&1 | grep -q 'no package is set'"
ck "from-source rebuild builds the declared package here, then it is built here" "$e from-source rebuild sl --accept-risks --yes >/tmp/s4.out 2>&1; tail -6 /tmp/s4.out; pacman -Qi sl | grep -q 'Unknown Packager' && $e from-source 2>&1 | grep -q 'built here'"

ck "the build is verified against the package database" "$e from-source 2>&1 | grep -q 'source verified'"
sudo pacman -S --noconfirm sl >/dev/null 2>&1
ck "the same version put back from the repository is detected, not claimed as built here" "pacman -Qi sl | grep -q 'Packager' && ! pacman -Qi sl | grep -q 'Unknown Packager' && $e from-source 2>&1 | grep -q 'repository file detected' && ! $e from-source 2>&1 | grep -q 'source verified'"
ck "doctor says the policy is not honoured" "$e doctor >/tmp/d.out 2>&1; grep -q 'not built from source as you asked' /tmp/d.out || { head -15 /tmp/d.out; false; }"
ck "from-source rebuild builds it again and it is verified" "$e from-source rebuild sl --accept-risks --yes >/tmp/s4b.out 2>&1; $e from-source 2>&1 | grep -q 'source verified'"

# an older sl from the archive, installed by hand: the repository is then ahead
cur=$(pacman -Q sl | cut -d' ' -f2)
old=$(curl -fsS https://archive.archlinux.org/packages/s/sl/ | grep -o 'sl-[0-9][^"]*-x86_64.pkg.tar.zst"' | tr -d '"' | grep -v "^sl-$cur-" | head -1)
if [ -n "$old" ]; then
  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  ck "an older version is installed, so the policy mismatch is reported as such" "$e from-source 2>&1 | grep -q 'policy mismatch'"
  ck "the upgrade plan names the rebuild from source, not the binary" "$e upgrade sl --dry-run > /tmp/s5.out 2>&1; grep -q 'extra+source/sl' /tmp/s5.out && grep -q 'source-build policy: sl' /tmp/s5.out"
  ck "upgrade --skip-source-builds installs the binary and leaves the rebuild pending" "$e upgrade sl --yes --skip-source-builds >/tmp/s6.out 2>&1; ! pacman -Qi sl | grep -q 'Unknown Packager' && $e from-source 2>&1 | grep -q 'rebuild pending'"
  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  ck "the upgrade rebuilds the package from source and clears the pending state" "$e upgrade sl --yes --skip-pkgbuild --accept-risks >/tmp/s7.out 2>&1; pacman -Qi sl | grep -q 'Unknown Packager' && $e from-source 2>&1 | grep -q 'built here'"
  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  ck "a rebuild that fails leaves the binary, keeps the policy, says so and exits non-zero" "EMRAC_ARCH_PACKAGING_URL=file:///nonexistent/{base}.git $e upgrade sl --yes >/tmp/s8.out 2>&1; rc=\$?; [ \$rc -ne 0 ] && grep -q 'binary of sl stays installed' /tmp/s8.out && grep -q 'upgrade is done, but sl could not be rebuilt' /tmp/s8.out && pacman -Q sl && $e from-source 2>&1 | grep -q 'rebuild pending'"
  ck "a pending rebuild is in the doctor" "$e doctor 2>&1 | grep -q 'not built from source as you asked'"
  ck "a pending rebuild is in the plan of the next upgrade" "$e upgrade --dry-run 2>&1 | grep -q 'source-build policy is not being honoured for sl'"
fi
# a record that cannot be read is unknown intent, never "no policy"
cp ~/.local/share/emrac/source-built.json /tmp/sb.good
printf '[{"name": "sl", ' > ~/.local/share/emrac/source-built.json
ck "a damaged record stops an upgrade and says why" "! $e upgrade --dry-run >/tmp/s9.out 2>&1 && grep -q \"can't be read\" /tmp/s9.out"
ck "a damaged record is never written over" "! $e from-source add figlet >/dev/null 2>&1; [ \"\$(cat ~/.local/share/emrac/source-built.json)\" = '[{\"name\": \"sl\", ' ]"
ck "the doctor calls a damaged record a problem" "$e doctor 2>&1 | grep -q 'record of your source-build policies'"
cp /tmp/sb.good ~/.local/share/emrac/source-built.json
# a record that is gone while its seal lists policies is unknown intent, not "none"
rm -f ~/.local/share/emrac/source-built.json
ck "a deleted record stops an upgrade and names what it held" "! $e upgrade --dry-run >/tmp/s10.out 2>&1 && grep -q 'is missing, but it held policies for sl' /tmp/s10.out"
ck "a deleted record is not written over either" "! $e from-source add figlet >/dev/null 2>&1; [ ! -e ~/.local/share/emrac/source-built.json ]"
ck "the doctor calls a deleted record a problem" "$e doctor 2>&1 | grep -q 'record of your source-build policies'"
cp /tmp/sb.good ~/.local/share/emrac/source-built.json

# pacman used directly: the policy is reconciled with what is installed
if [ -n "$old" ]; then
  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  ck "a plan with the policy has an id; ending the policy changes it" "$e upgrade sl --dry-run 2>&1 | grep -o 'Plan id *: [0-9a-f]*' | head -1 | awk '{print \$NF}' > /tmp/id1; $e from-source drop sl >/dev/null 2>&1; $e upgrade sl --dry-run 2>&1 | grep -o 'Plan id *: [0-9a-f]*' | head -1 | awk '{print \$NF}' > /tmp/id2; [ -s /tmp/id1 ] && [ -s /tmp/id2 ] && ! cmp -s /tmp/id1 /tmp/id2"
  cp /tmp/sb.good ~/.local/share/emrac/source-built.json
  ck "a plan made before the policy changed is refused by --expect-plan" "$e upgrade sl --dry-run 2>&1 | grep -o 'Plan id *: [0-9a-f]*' | head -1 | awk '{print \$NF}' > /tmp/id3; $e from-source drop sl >/dev/null 2>&1; ! $e upgrade sl --yes --expect-plan \$(cat /tmp/id3 | cut -c1-12) >/tmp/s11.out 2>&1; grep -q 'not the plan you expected' /tmp/s11.out; pacman -Qi sl | grep -q Version"
  cp /tmp/sb.good ~/.local/share/emrac/source-built.json
  ck "pacman -Syu by hand: emrac sees the policy was not honoured" "sudo pacman -Syu --noconfirm >/dev/null 2>&1; $e from-source 2>&1 | grep -qE 'repository file detected|policy mismatch|rebuild pending' && $e doctor 2>&1 | grep -q 'not built from source as you asked'"
fi
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "pacman -R by hand: the record does not claim an installed source build" "$e from-source 2>&1 | grep -q 'not installed' && ! $e from-source 2>&1 | grep -q 'source verified' && ! $e doctor 2>&1 | grep -q 'not built from source as you asked'"
sudo pacman -S --noconfirm sl >/dev/null 2>&1
ck "install.prefer = recommend never overrides --source or --binary" "$e config set install.prefer recommend >/dev/null; $e install --source sl --dry-run 2>&1 | grep -q 'extra+source/sl' && $e install --binary sl --dry-run 2>&1 | grep -q 'extra/sl'; $e config unset install.prefer >/dev/null"
cp /tmp/sb.good ~/.local/share/emrac/source-built.json
ck "install --binary of a policy package ends the policy" "$e install --binary sl --yes >/dev/null 2>&1; $e from-source 2>&1 | grep -q 'no package is set'"
sudo pacman -R --noconfirm sl >/dev/null 2>&1

# --- cut off: the state a crash leaves, and what emrac makes of it
if [ -n "$old" ]; then
  now=$(pacman -Si sl | awk '/^Version/{print $3}')
  sudo pacman -R --noconfirm sl >/dev/null 2>&1; rm -f ~/.local/share/emrac/source-built.json ~/.local/share/emrac/source-built.seal
  $e install --source sl --yes --skip-pkgbuild --accept-risks >/dev/null 2>&1
  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  EMRAC_FAULT=after-binary-upgrade $e upgrade sl --yes --skip-pkgbuild --accept-risks >/tmp/t1.out 2>&1; rc=$?
  ck "TX-01 cut off after the binary upgrade: the binary is installed and the cut-off is real" "[ $rc -ne 0 ] && grep -q 'cut off at' /tmp/t1.out && [ \"\$(pacman -Q sl | cut -d' ' -f2)\" = \"$now\" ] && ! pacman -Qi sl | grep -q 'Unknown Packager'"
  ck "TX-01 the policy survived, and emrac says it is not honoured" "$e from-source 2>&1 | grep -qE 'repository file detected|policy mismatch|rebuild pending' && $e doctor 2>&1 | grep -q 'not built from source as you asked'"
  ck "TX-01 the upgrade is in the history as done (nothing to recover), and the rebuild then completes" "$e history 2>&1 | head -4 | grep -q 'upgrade' && $e from-source rebuild sl --accept-risks --yes >/tmp/t1b.out 2>&1; ! grep -q 'in your history (failed' /tmp/t1b.out && pacman -Qi sl | grep -q 'Unknown Packager' && $e from-source 2>&1 | grep -q 'source verified'"

  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  EMRAC_FAULT=after-build $e from-source rebuild sl --accept-risks --yes >/tmp/t3.out 2>&1; rc=$?
  ck "TX-03 cut off after the build, before the install: the installed package is untouched" "[ $rc -ne 0 ] && grep -q 'cut off at' /tmp/t3.out && [ \"\$(pacman -Q sl | cut -d' ' -f2)\" = \"\$(echo $old | sed 's/^sl-//; s/-x86_64.*//')\" ]"
  ck "TX-03 the built package is kept and the record is still readable" "ls ~/.cache/emrac/build/arch-sl/*.pkg.tar.zst >/dev/null && $e from-source >/dev/null 2>&1"
  ck "TX-03/04 the next run recovers the journal and the package ends up verified" "$e from-source rebuild sl --accept-risks --yes >/tmp/t3b.out 2>&1; grep -q 'in your history (failed' /tmp/t3b.out && $e from-source 2>&1 | grep -q 'source verified'"

  sudo pacman -U --noconfirm "https://archive.archlinux.org/packages/s/sl/$old" >/dev/null 2>&1
  $e from-source rebuild sl --accept-risks --yes >/tmp/t2.out 2>&1 &
  pid=$!
  for i in $(seq 1 240); do pgrep -f 'makepkg' >/dev/null && break; sleep 0.5; done
  kill -9 $pid 2>/dev/null; pkill -9 -f makepkg 2>/dev/null; pkill -9 -f bwrap 2>/dev/null; wait $pid 2>/dev/null; sleep 2
  ck "TX-02 killed in the middle of the build: the installed package is untouched, the record readable" "[ \"\$(pacman -Q sl | cut -d' ' -f2)\" = \"\$(echo $old | sed 's/^sl-//; s/-x86_64.*//')\" ] && $e from-source >/dev/null 2>&1"
  ck "TX-02 the next run recovers the journal and builds cleanly" "$e from-source rebuild sl --accept-risks --yes >/tmp/t2b.out 2>&1; grep -q 'in your history (failed' /tmp/t2b.out && $e from-source 2>&1 | grep -q 'source verified'"
fi

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

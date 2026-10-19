#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh sourceinstall).
# Source or binary on install: `emrac install --source` builds an official package
# from Arch's packaging source here (sandboxed, audited, inspected, installed with
# pacman -U), `--binary` compiles nothing, and `install.prefer` decides when you
# do not. Needs the network (the packaging source and the package's own source).
#
# last full run: after u23, 12 of 12 passed
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
ck "the transaction is in the history" "$e history 2>&1 | head -3 | grep -q 'sl'"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "a binary install of the same package is the repository's file" "$e install --binary sl --yes >/tmp/s3.out 2>&1; pacman -Q sl && ! pacman -Qi sl | grep -q 'Unknown Packager'"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "install.prefer recommend uses the verdict of recommend, --binary still wins" "$e config set install.prefer recommend >/dev/null && $e install --binary sl --dry-run 2>&1 | grep -q 'extra/sl'; $e config unset install.prefer >/dev/null"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

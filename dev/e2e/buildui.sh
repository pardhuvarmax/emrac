#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh buildui).
# An AUR build in emrac's style: a local checkout posing as `pfetch` whose build()
# prints compiler lines (cmake style, a warning, an error), shown as makepkg's
# phases with a live line, the compiler's lines folded away, errors at once and the
# whole output in a log. Both the sandboxed and the unsandboxed path.
#
# last full run: after the package files, 18 of 18 passed
e=/usr/local/bin/emrac
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
pty() { script -qec "$1" /dev/null | tr -d '\r'; }
ptyp() { pty "$1" | sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g'; }
T="env TERM=xterm-256color LANG=C.UTF-8"
sudo pacman -Syu --noconfirm >/dev/null 2>&1; sudo pacman -S --noconfirm --needed bubblewrap base-devel git >/dev/null 2>&1

B=$HOME/.cache/emrac/build
fixture() { # $1 = the body of build()
  rm -rf /tmp/up.git $B/pfetch /tmp/w; mkdir -p /tmp/w $B
  printf 'pkgname=pfetch\npkgver=9.9\npkgrel=1\npkgdesc=fixture\narch=(any)\nlicense=(MIT)\nbuild() {\n%s\n}\npackage() {\n  install -Dm755 /dev/stdin "$pkgdir/usr/bin/pfetch" <<<"#!/bin/sh"\n}\n' "$1" > /tmp/w/PKGBUILD
  printf 'pkgbase = pfetch\n\tpkgdesc = fixture\n\tpkgver = 9.9\n\tpkgrel = 1\n\tarch = any\n\tsource = https://github.com/dylanaraps/pfetch/archive/9.9.tar.gz\n\tsha256sums = aaaa\n\npkgname = pfetch\n' > /tmp/w/.SRCINFO
  (cd /tmp/w && git init -q -b master && git add -A && git commit -q -m x) && git clone -q --bare /tmp/w /tmp/up.git && git clone -q /tmp/up.git $B/pfetch
}
GOOD='  for i in 1 2 3 4 5 6 7 8 9 10; do echo "[$((i*10))%] Building CXX object src/file$i.cpp.o"; sleep 0.4; done
  echo "src/file3.cpp:4:5: warning: unused variable x"
  echo "[100%] Linking CXX executable pfetch"'
BAD='  for i in 1 2 3; do echo "[$((i*10))%] Building CXX object src/file$i.cpp.o"; sleep 0.3; done
  echo "src/file4.cpp:9:2: error: expected semicolon"
  echo "make: *** [Makefile:7: all] Error 2"
  exit 1'

fixture "$GOOD"
ck "unsandboxed, on a terminal: phases, a clock and a summary; the compiler lines are folded away" "ptyp \"$T $e install pfetch --yes --skip-pkgbuild --no-sandbox\" > /tmp/b1.out; grep -q '✔ build()' /tmp/b1.out && ! grep -q 'Building CXX object src/file2' /tmp/b1.out && pacman -Q pfetch"
ck "the summary counts the files and the warning" "grep -E '✔ build\\(\\).*11 files.*1 warnings' /tmp/b1.out"
ck "the whole output is in a log that has every compiler line" "ls ~/.local/share/emrac/build-logs/pfetch-*.log >/dev/null && grep -q 'Building CXX object src/file7.cpp.o' ~/.local/share/emrac/build-logs/pfetch-*.log"
sudo pacman -R --noconfirm pfetch >/dev/null 2>&1; fixture "$GOOD"
ck "sandboxed too: the same phases and a summary" "ptyp \"$T $e install pfetch --yes --skip-pkgbuild\" > /tmp/b2.out; grep -q '✔ build()' /tmp/b2.out && pacman -Q pfetch"
sudo pacman -R --noconfirm pfetch >/dev/null 2>&1; fixture "$GOOD"
ck "piped, the build is makepkg's own output" "$e install pfetch --yes --skip-pkgbuild --no-sandbox >/tmp/b3.out 2>&1; grep -q 'Building CXX object src/file2.cpp.o' /tmp/b3.out && ! grep -q '✔ build()' /tmp/b3.out && pacman -Q pfetch"
sudo pacman -R --noconfirm pfetch >/dev/null 2>&1; fixture "$GOOD"
ck "clui.builds native leaves makepkg's own output on a terminal" "$e config set clui.builds native >/dev/null; ptyp \"$T $e install pfetch --yes --skip-pkgbuild --no-sandbox\" > /tmp/b4.out; $e config unset clui.builds >/dev/null; grep -q 'Building CXX object src/file2.cpp.o' /tmp/b4.out && ! grep -q '✔ build()' /tmp/b4.out"
sudo pacman -R --noconfirm pfetch >/dev/null 2>&1; fixture "$BAD"
ck "a failing build shows its error at once, the last lines and the log" "ptyp \"$T $e install pfetch --yes --skip-pkgbuild --no-sandbox\" > /tmp/b5.out; grep -q '✖ src/file4.cpp:9:2: error: expected semicolon' /tmp/b5.out && grep -q 'last .* lines of the build' /tmp/b5.out && grep -q 'the whole build is in' /tmp/b5.out; ! pacman -Q pfetch"
ck "and the command fails" "! pty \"$T $e install pfetch --yes --skip-pkgbuild --no-sandbox\" >/dev/null 2>&1; true"
ck "the setting is checked" "! $e config set clui.builds loud 2>&1 && [ \"\$($e config get clui.builds)\" = emrac ]"

# --- what is read before a build: the PKGBUILD diff, the audit and the trust signals
sudo pacman -R --noconfirm pfetch >/dev/null 2>&1; fixture "$GOOD"
$e install pfetch --yes --skip-pkgbuild --no-sandbox >/dev/null 2>&1
(cd /tmp/w && sed -i 's/pkgver=9.9/pkgver=9.10/; s/^license=(MIT)/license=(MIT)\nsource=(https:\/\/example.net\/pfetch-$pkgver.tar.gz)\nsha256sums=(SKIP)/' PKGBUILD && git commit -qam bump && git push -q /tmp/up.git master)
ck "on a terminal the PKGBUILD changes are a summary and a numbered diff" "ptyp \"$T $e rebuild pfetch --yes\" > /tmp/r1.out; grep -q '▼ PKGBUILD changes · pfetch · since your last build  +3 −1' /tmp/r1.out && grep -qE 'pkgver +9.9 → 9.10' /tmp/r1.out && grep -qE '^ *2 +│ − pkgver=9.9' /tmp/r1.out && grep -qE '^ +2 │ \\+ pkgver=9.10' /tmp/r1.out"
ck "a change of sources is marked and says what to check" "grep -qE '⚠ +source +added https://example.net' /tmp/r1.out && grep -q 'the sources or their checksums changed' /tmp/r1.out"
ck "the audit's finding is a badge, the rule, the place and the line it found" "grep -qE '(warning|HIGH) +checksum-skipped +PKGBUILD:[0-9]+' /tmp/r1.out && grep -q 'Audit · pfetch' /tmp/r1.out"
ck "piped, the review is the plain text it always was" "(cd /tmp/w && git reset -q --hard HEAD~1 && git push -q -f /tmp/up.git master) ; $e install pfetch --yes --skip-diff >/dev/null 2>&1; (cd /tmp/w && git cherry-pick -q ORIG_HEAD 2>/dev/null || true); $e rebuild pfetch --yes > /tmp/r2.out 2>&1; ! grep -q '▼ PKGBUILD changes' /tmp/r2.out"

# --- a package file on this machine: emrac install ./x.pkg.tar.zst (what pacman -U does)
sudo pacman -R --noconfirm sl >/dev/null 2>&1; sudo pacman -Sw --noconfirm sl >/dev/null 2>&1
PF=$(ls /var/cache/pacman/pkg/sl-*.pkg.tar.zst | head -1); cp "$PF" /tmp/sl-local.pkg.tar.zst
ck "a package file's plan says what it is: a local file, an install, no signature" "$e install /tmp/sl-local.pkg.tar.zst --dry-run > /tmp/f1.out 2>&1; grep -q 'local file/sl' /tmp/f1.out && grep -q 'has no signature next to it' /tmp/f1.out && ! pacman -Q sl"
ck "installing it works, shown in emrac's style on a terminal, and it is in the history" "ptyp \"$T $e install /tmp/sl-local.pkg.tar.zst --yes\" > /tmp/f2.out; grep -q '✔ installing sl' /tmp/f2.out && pacman -Q sl && $e history 2>&1 | grep -q 'sl'"
ck "the same file again is a reinstall" "$e install /tmp/sl-local.pkg.tar.zst --dry-run 2>&1 | grep -qi 'reinstall'"
ck "package names and files cannot be mixed" "! $e install sl /tmp/sl-local.pkg.tar.zst --yes 2>&1 | grep -q 'installing' && $e install sl /tmp/sl-local.pkg.tar.zst --dry-run 2>&1 | grep -q 'not both'"
ck "a missing file or a file that is not a package is refused with a reason" "$e install /tmp/no-such-file.pkg.tar.zst --yes 2>&1 | grep -q 'No such file' && echo x > /tmp/fake.pkg.tar.zst && ! $e install /tmp/fake.pkg.tar.zst --yes >/dev/null 2>&1"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

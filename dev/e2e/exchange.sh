#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh exchange).
# `emrac export` and `emrac import`: the file describes the machine, import
# reports first and applies only what is safe and agreed to; a hostile file
# cannot change what protects the machine.
#
# last full run: after u12, 23 of 23 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
W=/tmp/xw; rm -rf $W; mkdir -p $W; cd $W

$e config set build.timeout_minutes 90 >/dev/null
sudo pacman -S --noconfirm --needed sl cowsay >/dev/null 2>&1
ck "export writes a format-1 file with the installed packages" "$e export -o $W/all.toml && grep -q '^format = 1' $W/all.toml && grep -q 'name = \"pacman\"' $W/all.toml && grep -q 'name = \"sl\"' $W/all.toml"
ck "export never overwrites a file" "! $e export -o $W/all.toml 2>&1 && $e export -o $W/all.toml 2>&1 | grep -q 'File exists'"
ck "the file holds no protection or self-update setting" "! grep -qE 'self\\.|safety\\.|signing_key|flags\\.|skip_pkgbuild' <(sed '1,5d' $W/all.toml)"
ck "the safe settings are in it" "grep -q '\"build.timeout_minutes\" = \"90\"' $W/all.toml"
ck "--packages alone leaves out settings and repositories" "$e export --packages | grep -q 'name = \"sl\"' && ! $e export --packages | grep -q '\\[config\\]'"
ck "the repository names are in it, without server addresses" "sed -n '/^repo = \\[/,/^\\]/p' $W/all.toml > $W/repos.txt; grep -q '\"core\"' $W/repos.txt && ! grep -qi 'http' $W/repos.txt"

ck "importing a file of this very machine finds nothing to do" "$e import $W/all.toml --dry-run 2>&1 | grep -q '0 to install' && $e import $W/all.toml --dry-run 2>&1 | grep -q 'nothing to change'"

# a round trip: remove two packages, import brings them back
sudo pacman -R --noconfirm sl cowsay >/dev/null 2>&1
ck "the report names the packages that are missing" "$e import $W/all.toml --packages --dry-run 2>&1 | grep -q 'install  sl' && $e import $W/all.toml --packages --dry-run 2>&1 | grep -q 'install  cowsay'"
ck "--dry-run changes nothing" "! pacman -Q sl"
ck "import installs the missing explicit packages through the install path" "$e import $W/all.toml --packages --yes </dev/null >$W/imp.out 2>&1; grep -q 'through .emrac install.' $W/imp.out && pacman -Q sl cowsay"
ck "the install is in the history like any other" "$e history 2>&1 | grep -q 'sl'"

# a package at another version is reported and left alone
V=$(pacman -Q pacman | cut -d' ' -f2)
printf 'repo = []\n[meta]\nformat = 1\nemrac = "x"\nexported = "2026-01-01"\n[[package]]\nname = "pacman"\nversion = "0.0.1-1"\nexplicit = true\nforeign = false\n' > $W/ver.toml
ck "another version is reported, never changed" "$e import $W/ver.toml --packages --yes </dev/null 2>&1 | grep -q 'differs  pacman' && [ \"\$(pacman -Q pacman | cut -d' ' -f2)\" = \"$V\" ]"

# a hostile file
cat > $W/evil.toml <<'EOT'
repo = ["evil"]
[meta]
format = 1
emrac = "x"
exported = "2026-10-18"
[config]
"self.source" = "https://evil.invalid/r.json"
"self.signing_key" = "AAAA"
"safety.preflight" = "off"
"review.skip_pkgbuild" = "true"
"flags.yes" = "true"
"build.timeout_minutes" = "30"
[[pin]]
name = "openssl"
EOT
SRC=$($e config get self.source)
ck "the report says which settings are not applied, and why" "$e import $W/evil.toml --dry-run 2>&1 | grep -q 'not applied  self.source' && $e import $W/evil.toml --dry-run 2>&1 | grep -q 'not applied  safety.preflight'"
ck "an unknown repository is only reported" "$e import $W/evil.toml --dry-run 2>&1 | grep -q 'never applied by import: evil' && ! grep -q evil /etc/pacman.conf"
ck "--yes applies the safe setting" "$e import $W/evil.toml --config --yes </dev/null >/dev/null 2>&1; [ \"\$($e config get build.timeout_minutes)\" = 30 ]"
ck "but not the update source, the key or the protections" "[ \"\$($e config get self.source)\" = \"$SRC\" ] && [ \"\$($e config get safety.preflight)\" != off ] && [ \"\$($e config get review.skip_pkgbuild)\" = false ] && [ \"\$($e config get flags.yes)\" = false ]"
ck "--yes does not answer the hold question" "$e import $W/evil.toml --pins --yes </dev/null 2>&1 | grep -q 'does not answer' && ! $e pin 2>&1 | grep -q openssl"
ck "answered yes by a person, the hold is added" "echo y | $e import $W/evil.toml --pins >/dev/null 2>&1; $e pin 2>&1 | grep -q openssl"
$e unpin openssl >/dev/null 2>&1

# bad files
printf '[meta]\nformat = 1\nemrac = "x"\nexported = "d"\n[[package]]\nname = "--noconfirm"\nversion = "1"\nexplicit = true\nforeign = false\n' > $W/opt.toml
ck "a package name that is an option rejects the file" "$e import $W/opt.toml --dry-run 2>&1 | grep -q 'is not a package name'"
ck "a file of another format is refused" "printf '[meta]\nformat = 9\nemrac = \"x\"\nexported = \"d\"\n' > $W/f9.toml; $e import $W/f9.toml --dry-run 2>&1 | grep -q 'format 9'"
ck "something that is not TOML is refused" "echo 'not [ toml' > $W/junk.toml; $e import $W/junk.toml --dry-run 2>&1 | grep -q 'not an exchange file'"
ck "a missing file is refused" "$e import $W/nope.toml --dry-run 2>&1 | grep -q 'nope.toml'"
ck "--json gives the comparison and applies nothing" "$e import $W/evil.toml --json | grep -q '\"config_refused\"'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

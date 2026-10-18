#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh advisor).
# The Package Advisor: advice under a big plan, shown by lowering the limits on a
# small package; none by default, none when turned off, never a blocker.
#
# last full run: after u15, 15 of 15 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
sudo pacman -R --noconfirm vim vim-runtime gpm >/dev/null 2>&1

ck "a small plan has no advice with the default limits" "! $e install vim --dry-run 2>&1 | grep -q '^Advice'"
$e config set advisor.large_download_mib 0 >/dev/null; $e config set advisor.large_growth_mib 0 >/dev/null
ck "the advisor settings are in the config" "$e config get advisor.enabled | grep -qx true && $e config get advisor.many_dependencies | grep -qx 40"
$e config set advisor.large_download_mib 5 >/dev/null
ck "a low download limit gives the large-download advice" "$e install vim --dry-run 2>&1 | grep -q 'Large download: this install downloads'"
ck "the advice names the biggest package" "$e install vim --dry-run 2>&1 | grep -q 'Biggest: '"
ck "the advice says it blocks nothing" "$e install vim --dry-run 2>&1 | grep -q 'it blocks nothing'"
$e config set advisor.large_growth_mib 20 >/dev/null
ck "a low growth limit gives the disk-growth advice" "$e install vim --dry-run 2>&1 | grep -q 'Large disk growth: this install needs'"
$e config set advisor.many_dependencies 1 >/dev/null
ck "a low dependency limit gives the many-packages advice with a command" "$e install vim --dry-run 2>&1 | grep -q 'Many new packages' && $e install vim --dry-run 2>&1 | grep -q 'emrac tree vim --depth 2'"
ck "the advice is in the JSON plan" "$e install vim --dry-run --json | grep -q '\"advice\"' && $e install vim --dry-run --json | grep -q 'large_download'"
ck "the plan id does not change with the advice" "A=\$($e install vim --dry-run --json | grep '\"digest\"'); $e config set advisor.enabled false >/dev/null; B=\$($e install vim --dry-run --json | grep '\"digest\"'); [ -n \"\$A\" ] && [ \"\$A\" = \"\$B\" ]"
ck "advisor.enabled false gives no advice at all" "! $e install vim --dry-run 2>&1 | grep -q '^Advice' && ! $e install vim --dry-run --json | grep -q '\"advice\"'"
$e config set advisor.enabled true >/dev/null
ck "a limit of 0 turns that one rule off" "$e config set advisor.large_download_mib 0 >/dev/null; $e config set advisor.large_growth_mib 0 >/dev/null; $e config set advisor.many_dependencies 0 >/dev/null; ! $e install vim --dry-run 2>&1 | grep -q '^Advice'"
ck "a bad value is refused" "! $e config set advisor.large_download_mib lots 2>&1 && ! $e config set advisor.enabled maybe 2>&1"
ck "advice never stops an install" "$e config set advisor.large_download_mib 5 >/dev/null; $e install vim --yes </dev/null >/tmp/adv.out 2>&1; grep -q '^Advice' /tmp/adv.out && pacman -Q vim"
ck "a removal gets no advice" "$e remove vim --dry-run 2>&1 | grep -qv '^Advice'; ! $e remove vim --dry-run 2>&1 | grep -q '^Advice'"
ck "the config file has an advisor section" "$e config show | grep -q '^\\[advisor\\]'"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

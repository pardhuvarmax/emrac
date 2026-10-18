#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh helppages).
# man, docs, license, credits, support, feedback, bug-report.
#
# last full run: after u13, 22 of 22 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
W=/tmp/hw; rm -rf $W; mkdir -p $W

ck "man prints the help of a command when not on a terminal" "$e man install | grep -q 'Usage: emrac install'"
ck "man takes a subcommand" "$e man snapshot create | grep -q 'Usage: emrac snapshot create'"
ck "man --roff is roff source with a title line" "$e man install --roff | grep -q '^\\.TH emrac-install'"
ck "man of emrac itself lists the commands" "$e man emrac --roff | grep -q 'bug\\\\-report'"
ck "an unknown command lists the known ones" "$e man nope 2>&1 | grep -q 'the commands are: .*install'"
ck "every command renders as roff" "for c in \$($e man emrac | sed -n '/^Commands:/,/^$/p' | awk 'NR>1 && NF {print \$1}'); do [ \"\$c\" = help ] && continue; $e man \$c --roff | grep -q '^\\.TH' || { echo bad \$c; exit 1; }; done"
ck "man emrac is the main page with files, exit status and see-also" "$e man emrac --roff >$W/main.1 2>&1; grep -q '^\\.SH FILES' $W/main.1 && grep -q '^\\.SH EXIT STATUS' $W/main.1 && grep -q '^\\.SH SEE ALSO' $W/main.1"
ck "man --generate writes emrac.1 and a page for every command" "$e man --generate $W/man 2>/dev/null; [ -f $W/man/emrac.1 ] && [ -f $W/man/emrac-install.1 ] && [ -f $W/man/emrac-snapshot-create.1 ] && [ -f $W/man/emrac-bundle-apply.1 ] && [ \$(ls $W/man | wc -l) -gt 60 ]"
ck "every generated page has a title line and a name section" "for f in $W/man/*.1; do grep -q '^\\.TH ' \$f && grep -q '^\\.SH NAME' \$f || { echo bad \$f; exit 1; }; done"
ck "no page is written for the root helpers" "! ls $W/man | grep -q 'policy-helper\\|ledger-append'"
ck "--generate with a command is a usage error" "$e man install --generate $W/x 2>&1; [ \$($e man install --generate $W/x >/dev/null 2>&1; echo \$?) = 2 ]"
ck "docs gives the site and the repository" "$e docs | grep -q 'pardhuvarmax.github.io/emrac' && $e docs | grep -q 'github.com/pardhuvarmax/emrac'"
ck "docs without --open opens nothing" "$e docs | grep -q 'In the terminal'"
ck "license says GPL and offers the full text" "$e license | grep -q 'GNU General Public License' && $e license | grep -q -- '--full'"
ck "license --full is the whole text offline" "[ \$($e license --full | wc -l) -gt 600 ] && $e license --full | grep -q 'Version 3, 29 June 2007'"
ck "credits names the author, the licence and what it runs" "$e credits | grep -q pardhuvarmax && $e credits | grep -q 'GPL-3.0-or-later' && $e credits | grep -q bubblewrap && $e credits | grep -q alpm"
ck "support says what to run first" "$e support | grep -q 'emrac doctor' && $e support | grep -q 'emrac bug-report'"
ck "feedback gives the new-issue page and says nothing is sent" "$e feedback | grep -q 'issues/new' && $e feedback | grep -q 'Nothing is sent'"
ck "bug-report has the doctor report and the version" "$e bug-report | grep -q 'doctor report' && $e bug-report | grep -q \"emrac \$($e --version | awk '{print \$2}')\""
ck "bug-report has a failed-transactions section" "$e bug-report | grep -q 'Recent failed transactions'"
ck "bug-report is scrubbed: no user name, no home path" "! $e bug-report | grep -q \"\$HOME\" && ! $e bug-report | grep -q \"/home/\$USER\""
ck "bug-report says it was not sent" "$e bug-report | grep -q 'only printed'"
echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

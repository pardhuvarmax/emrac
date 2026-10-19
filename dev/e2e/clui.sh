#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh clui).
# How emrac looks, under a real pseudo-terminal (util-linux `script`) and piped:
# marks, boxes and tables on a terminal only; colour by NO_COLOR, --color,
# CLICOLOR_FORCE, TERM and COLORTERM; ASCII when the locale is not UTF-8;
# animations only when `clui.animations active`, never in CI or piped, and the
# cursor always shown again, also after Ctrl-C.
#
# last full run: after the cancel, 65 of 65 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
# run a command line under a pseudo-terminal; carriage returns removed
pty() { script -qec "$1" /dev/null | tr -d '\r'; }
# the same with the colour and cursor sequences removed: for matching text
ptyp() { pty "$1" | sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g'; }
# the same environment as a UTF-8 terminal
T="env TERM=xterm-256color LANG=C.UTF-8"
ESC=$'\x1b'
sudo pacman -Syu --noconfirm >/dev/null 2>&1
sudo pacman -R --noconfirm vim vim-runtime gpm >/dev/null 2>&1
W=/tmp/cw; rm -rf $W; mkdir -p $W
$e config unset clui.animations >/dev/null 2>&1; $e config unset clui.theme >/dev/null 2>&1
# something in the history, the build times and a snapshot for the lists to show
sudo pacman -S --noconfirm --needed sl >/dev/null 2>&1; $e remove sl --yes </dev/null >/dev/null 2>&1
mkdir -p $HOME/.local/share/emrac
echo "{\"time\":$(date +%s),\"name\":\"fixture\",\"version\":\"1\",\"secs\":200,\"cores\":4,\"ok\":true}" > $HOME/.local/share/emrac/build-times.jsonl
$e snapshot ui-snap >/dev/null 2>&1

# --- the settings
ck "the defaults: regular animations, auto colour and unicode, the default theme" "[ \"\$($e config get clui.animations)\" = regular ] && [ \"\$($e config get clui.color)\" = auto ] && [ \"\$($e config get clui.unicode)\" = auto ] && [ \"\$($e config get clui.theme)\" = default ]"
ck "a value that is not one of the words is refused" "! $e config set clui.animations always 2>&1 && ! $e config set clui.theme neon 2>&1 && ! $e config set clui.color sometimes 2>&1 && ! $e config set clui.unicode yes 2>&1"
ck "the config file has a [clui] section" "$e config show | grep -q '^\\[clui\\]'"

# --- layout only on a terminal
ck "doctor on a terminal has marks" "ptyp \"$T $e doctor\" | grep -q '✔'"
ck "doctor piped is the plain text it always was" "$e doctor 2>&1 | grep -q '^  ok ' && ! $e doctor 2>&1 | grep -q '✔'"
ck "a plan on a terminal is a box with a plan id line" "ptyp \"$T $e install vim --dry-run\" | grep -q '╭─ install · ' && ptyp \"$T $e install vim --dry-run\" | grep -q '  plan id '"
ck "a plan piped keeps the old lines" "$e install vim --dry-run 2>&1 | grep -q '^emrac plans: install' && $e install vim --dry-run 2>&1 | grep -q '^Plan id  ' && ! $e install vim --dry-run 2>&1 | grep -q '╭'"
ck "the disk line has a bar and the share of free space" "ptyp \"$T $e install vim --dry-run\" | grep -E '^ +disk ' | grep -q 'of the free space on /'"
ck "history on a terminal is a table" "$e snapshot ui-snap >/dev/null 2>&1; ptyp \"$T $e history\" | grep -q 'time (UTC)'"
ck "the timeline on a terminal has day headings and glyphs" "ptyp \"$T $e timeline\" | grep -qE '^(Today|Yesterday|[0-9-]+) ─' "
ck "stats on a terminal has headings" "ptyp \"$T $e stats\" | grep -q '^Transactions ─'"
ck "mirrors on a terminal is a table" "printf 'Server = https://geo.mirror.pkgbuild.com/\$repo/os/\$arch\n' > $W/ml; ptyp \"$T $e mirrors --file $W/ml\" | grep -q 'last 10'"

# --- colour
ck "on a terminal there is colour" "pty \"$T $e doctor\" | grep -q \"$ESC\\[\""
ck "NO_COLOR removes the colour and keeps the layout" "! pty \"$T NO_COLOR=1 $e doctor\" | grep -q \"$ESC\\[\" && ptyp \"$T NO_COLOR=1 $e doctor\" | grep -q '✔'"
ck "--color never removes the colour" "! pty \"$T $e --color never doctor\" | grep -q \"$ESC\\[\""
ck "--color always colours a pipe" "$e --color always doctor 2>&1 | grep -q \"$ESC\\[\""
ck "CLICOLOR_FORCE colours a pipe" "CLICOLOR_FORCE=1 $e doctor 2>&1 | grep -q \"$ESC\\[\""
ck "piped output has no colour" "! $e doctor 2>&1 | grep -q \"$ESC\\[\""
ck "--json is plain, JSON, with no escapes" "pty \"$T $e install vim --dry-run --json\" | head -1 | grep -q '^{' && ! pty \"$T $e install vim --dry-run --json\" | grep -q \"$ESC\""
ck "a dumb terminal gets plain text" "! pty \"env TERM=dumb LANG=C.UTF-8 $e doctor\" | grep -q \"$ESC\\[\" && ! ptyp \"env TERM=dumb LANG=C.UTF-8 $e doctor\" | grep -q '✔'"
ck "true colour only when COLORTERM says so" "pty \"$T COLORTERM=truecolor $e doctor\" | grep -q '38;2;' && ! pty \"$T $e doctor\" | grep -q '38;2;'"
ck "the mono theme uses no colour codes" "$e config set clui.theme mono >/dev/null; ! pty \"$T $e doctor\" | grep -qE \"$ESC\\[[0-9;]*(3[0-7]|9[0-7]|38)[;m]\"; $e config unset clui.theme >/dev/null"
ck "the light theme differs from the default" "A=\$(pty \"$T $e doctor\" | md5sum); $e config set clui.theme light >/dev/null; B=\$(pty \"$T $e doctor\" | md5sum); $e config unset clui.theme >/dev/null; [ \"\$A\" != \"\$B\" ]"

# --- unicode and ascii
ck "a non-UTF-8 locale gets ASCII only" "! ptyp \"env TERM=xterm LANG=C LC_ALL=C $e doctor\" | grep -qP '[^\\x00-\\x7F]'"
ck "and a box made of + and -" "ptyp \"env TERM=xterm LANG=C LC_ALL=C $e install vim --dry-run\" | grep -q '^+- install - '"
ck "clui.unicode never forces ASCII on a UTF-8 terminal" "$e config set clui.unicode never >/dev/null; ! ptyp \"$T $e doctor\" | grep -qP '[^\\x00-\\x7F]'; $e config unset clui.unicode >/dev/null"
ck "the Linux console gets ASCII" "! ptyp \"env TERM=linux LANG=C.UTF-8 $e doctor\" | grep -q '✔'"

# --- width
ck "COLUMNS narrows the layout" "ptyp \"env TERM=xterm LANG=C LC_ALL=C COLUMNS=60 $e doctor\" | awk '{ if (length(\$0) > 60) bad=1 } END { exit bad }'"

# --- the gallery
ck "emrac ui shows every part on a terminal" "ptyp \"$T $e ui\" > $W/ui.out; for part in 'This terminal' Roles Marks 'A box' 'A table' 'A diff' 'The voice'; do grep -q \"\$part\" $W/ui.out || { echo missing \$part; exit 1; }; done"
ck "emrac ui --theme previews a theme" "ptyp \"$T $e ui --theme high-contrast\" | grep -q 'theme        high-contrast'"
ck "emrac ui piped says it is plain text" "$e ui 2>&1 | grep -q 'plain text'"
ck "emrac ui refuses an unknown theme" "$e ui --theme neon 2>&1 | grep -q 'not a theme'"

# --- animations
printf 'Server = https://127.0.0.1:1/$repo/os/$arch\nServer = https://127.0.0.2:1/$repo/os/$arch\n' > $W/dead
ck "regular (the default) never hides the cursor" "! pty \"$T $e mirrors test --file $W/dead\" | grep -q \"$ESC\\[?25l\""
ck "regular prints the old progress line" "ptyp \"$T $e mirrors test --file $W/dead\" 2>&1 | grep -q 'measured 2 of 2'"
$e config set clui.animations active >/dev/null
ck "active hides the cursor, draws a spinner and shows the cursor again" "pty \"$T $e mirrors test --file $W/dead\" > $W/act.out; grep -q \"$ESC\\[?25l\" $W/act.out && grep -q \"$ESC\\[?25h\" $W/act.out && sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' $W/act.out | grep -q 'measuring mirrors'"
ck "active leaves one result line in the scrollback" "sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' $W/act.out | grep -q '✔ measured 2 mirrors in '"
ck "active does not print the old progress line" "! sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' $W/act.out | grep -q 'measured 2 of 2'"
ck "active piped draws nothing" "! $e mirrors test --file $W/dead 2>&1 | grep -q \"$ESC\\[?25l\""
ck "active under CI draws nothing" "! pty \"$T CI=true $e mirrors test --file $W/dead\" | grep -q \"$ESC\\[?25l\""
ck "active on a dumb terminal draws nothing" "! pty \"env TERM=dumb LANG=C.UTF-8 $e mirrors test --file $W/dead\" | grep -q \"$ESC\\[?25l\""
ck "active with --json draws nothing" "! pty \"$T $e mirrors test --file $W/dead --json\" | grep -q \"$ESC\\[?25l\""
ck "active with --quiet draws nothing" "! pty \"$T $e --quiet mirrors test --file $W/dead\" | grep -q \"$ESC\\[?25l\""
ck "the ASCII spinner is ASCII" "! pty \"env TERM=xterm LANG=C LC_ALL=C $e mirrors test --file $W/dead\" | tr -d '\\033' | grep -qP '[^\\x00-\\x7F]'"
printf 'Server = https://10.255.255.1/$repo/os/$arch\n' > $W/slow
ck "Ctrl-C shows the cursor again" "pty \"$T sh -c '$e mirrors test --file $W/slow & p=\\\$!; sleep 1.5; kill -INT \\\$p; wait \\\$p'\" > $W/int.out; grep -q \"$ESC\\[?25l\" $W/int.out && grep -q \"$ESC\\[?25h\" $W/int.out"
ck "doctor with active animations shows the spinner result and then the report" "ptyp \"$T $e doctor\" > $W/doc.out; grep -q '✔ checked' $W/doc.out && grep -q 'Dependencies' $W/doc.out"
ck "emrac ui --demo runs both animations even when set to regular" "$e config set clui.animations regular >/dev/null; pty \"$T $e ui --demo\" > $W/demo.out; sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' $W/demo.out | grep -q 'counted to 8' && grep -q \"$ESC\\[?25h\" $W/demo.out"
$e config unset clui.animations >/dev/null

# --- nothing else changed
ck "the plan id of a plan does not depend on how it is shown" "A=\$($e install vim --dry-run --json | grep digest); B=\$(pty \"$T $e install vim --dry-run --json\" | grep digest); [ -n \"\$A\" ] && [ \"\$A\" = \"\$B\" ]"
ck "a real install on a terminal still works" "pty \"$T $e install vim --yes\" > $W/inst.out; pacman -Q vim"
ck "--version on a terminal is the EMRAC lettering, both versions and the motto" "ptyp \"$T $e --version\" | grep -q 'emrac v[0-9.]* - libalpm v[0-9.]*' && ptyp \"$T $e --version\" | grep -q '|/____/' && ptyp \"$T $e --version\" | grep -q 'Sic Mundus Creatus Est.'"
ck "--version piped is still the plain lines scripts read" "[ \"\$($e --version | head -1)\" = \"emrac \$($e --version | head -1 | cut -d' ' -f2)\" ] && $e --version | grep -q '^built by ' && ! $e --version | grep -q '|/____/'"

# --- every help page is in colour on a terminal
esc=$(printf '\033')
ck "emrac with no arguments and emrac --help are in colour on a terminal" "pty \"$T $e\" | grep -q \"$esc\\[\" && pty \"$T $e --help\" | grep -q \"$esc\\[\""
ck "a command's and a subcommand's --help, and emrac help <cmd>, are in colour" "pty \"$T $e install --help\" | grep -q \"$esc\\[\" && pty \"$T $e snapshot create --help\" | grep -q \"$esc\\[\" && pty \"$T $e help doctor\" | grep -q \"$esc\\[\""
ck "the help text is the same with the colour taken out" "[ \"\$(ptyp \"$T $e install --help\")\" = \"\$(pty \"env TERM=xterm LANG=C.UTF-8 NO_COLOR=1 $e install --help\")\" ]"
ck "piped, --color never and NO_COLOR give plain help" "! $e --help | grep -q \"$esc\" && ! pty \"$T $e --color never --help\" | grep -q \"$esc\" && ! pty \"env TERM=xterm LANG=C.UTF-8 NO_COLOR=1 $e --help\" | grep -q \"$esc\""
ck "--color always colours piped help" "$e --color always snapshot --help | grep -q \"$esc\\[\""
ck "the other pages (credits, support, docs, license) are in colour too" "for c in credits support docs license feedback; do pty \"$T $e \$c\" | grep -q \"$esc\\[\" || exit 1; done"

# --- pacman's transaction in emrac's style (pacman does the work in a pseudo-terminal)
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "on a terminal the transaction is shown in emrac's style: steps with marks, no pacman bars" "ptyp \"$T $e install sl --yes\" > $W/pu1.out; grep -q '✔ installing sl' $W/pu1.out && grep -q '✔ checking keys in keyring' $W/pu1.out && ! grep -q '\\[####' $W/pu1.out && pacman -Q sl"
ck "pacman's own table, its question's text and its hook lines are still there" "grep -q 'Total Installed Size' $W/pu1.out && grep -q ':: Processing package changes' $W/pu1.out && grep -q 'Arming ConditionNeedsUpdate' $W/pu1.out"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "pacman's question is shown and answered on the terminal" "(sleep 5; echo y; sleep 8; echo y; sleep 5) | script -qec \"$T $e install sl\" /dev/null | tr -d '\\r' | sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' > $W/pu2.out; grep -q ':: Proceed with installation? \\[Y/n\\]' $W/pu2.out && grep -q '✔ installing sl' $W/pu2.out && pacman -Q sl"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "clui.pacman native leaves pacman's own output, bars included" "$e config set clui.pacman native >/dev/null && ptyp \"$T $e install sl --yes\" > $W/pu3.out; $e config unset clui.pacman >/dev/null; grep -q '\\[####' $W/pu3.out && ! grep -q '✔ installing sl' $W/pu3.out && pacman -Q sl"
sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "piped, pacman is not touched: no marks, no restyled lines" "$e install sl --yes > $W/pu4.out 2>&1; ! grep -q '✔ installing' $W/pu4.out && grep -q 'installing sl' $W/pu4.out && pacman -Q sl"
ck "the setting is checked" "! $e config set clui.pacman fancy 2>&1 && [ \"\$($e config get clui.pacman)\" = emrac ]"
ck "a failed transaction still fails the command" "! $e install --yes no-such-package-xyz >/dev/null 2>&1"

sudo pacman -R --noconfirm sl >/dev/null 2>&1
ck "answering no to pacman's own question is a cancel, said as one, not an error" "(sleep 5; echo y; sleep 8; echo n; sleep 5) | script -qec \"$T $e install sl\" /dev/null | tr -d '\\r' | sed -E 's/\\x1b\\[[0-9;?]*[A-Za-z]//g' > $W/no1.out; grep -q 'okay, nothing was changed: you answered no' $W/no1.out && ! grep -q 'something went wrong' $W/no1.out && ! pacman -Q sl"
ck "a cancel at pacman's question is not recorded as a failed transaction" "! $e history 2>&1 | head -4 | grep -qi 'failed'"


echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

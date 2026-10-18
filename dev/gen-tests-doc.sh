#!/bin/sh
# Regenerates docs/tests/emrac-tests.rst: every automated test that passes,
# grouped by suite and module, plus the end-to-end checks.
#
#   dev/gen-tests-doc.sh           run the tests and rewrite the file
#   dev/gen-tests-doc.sh --check   run the tests and fail if the file is stale
#
# CI runs --check, so a test that is added, renamed or removed shows up in the
# diff of the change that did it. A failing test aborts both modes: the list
# only ever holds tests that passed.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target=docs/tests/emrac-tests.rst
e2e=dev/e2e/final-check.sh

mode=write
[ "${1:-}" = "--check" ] && mode=check

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if ! cargo test --workspace >"$work/test.log" 2>&1; then
    cat "$work/test.log"
    echo "dev/gen-tests-doc.sh: the tests fail, so the list is not updated." >&2
    exit 1
fi

# Names come from `--list` (the test lines of a normal run can be garbled by
# child processes writing straight to the terminal). Passing is established by
# the run above: cargo exited 0, nothing was ignored, and the number of names
# equals the number of tests the run reports as passed.
if grep -Eq '[1-9][0-9]* ignored' "$work/test.log"; then
    echo "dev/gen-tests-doc.sh: some tests are ignored; they would be listed as passing." >&2
    exit 1
fi
passed=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$work/test.log" | awk '{ n += $1 } END { print n + 0 }')

cargo test --workspace -- --list >"$work/list.log" 2>&1

# suite|module|test, one line per test. The suite comes from the
# "Running ... (target/debug/deps/<binary>-<hash>)" line above its names.
awk '
    /^ *Running / {
        suite = ""
        if (match($0, /deps\/[A-Za-z0-9_]+-[0-9a-f]+/)) {
            name = substr($0, RSTART + 5, RLENGTH - 5)
            sub(/-[0-9a-f]+$/, "", name)
            suite = name
        }
        next
    }
    /^ *Doc-tests / { suite = ""; next }
    suite != "" && /: test$/ {
        t = $0
        sub(/: test$/, "", t)
        module = "(top level)"
        n = split(t, parts, "::")
        if (n > 1) {
            module = parts[1]
            for (i = 2; i < n; i++) module = module "::" parts[i]
            t = parts[n]
        }
        print suite "|" module "|" t
    }
' "$work/list.log" | LC_ALL=C sort -u >"$work/tests.txt"

listed=$(wc -l <"$work/tests.txt" | tr -d ' ')
if [ "$listed" != "$passed" ]; then
    echo "dev/gen-tests-doc.sh: $listed tests listed but the run passed $passed; not updating." >&2
    exit 1
fi

last_run=$(sed -n 's/^# last full run: *//p' "$e2e" | head -1)
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$e2e" >"$work/e2e.txt"
pre=dev/e2e/preflight.sh
selfup=dev/e2e/self-upgrade.sh
hk=dev/e2e/preview.sh
rc=dev/e2e/recovery.sh
bt=dev/e2e/buildtools.sh
po=dev/e2e/policy.sh
st=dev/e2e/stats.sh
tl=dev/e2e/timeline.sh
gr=dev/e2e/graph.sh
pre_run=$(sed -n 's/^# last full run: *//p' "$pre" | head -1)
self_run=$(sed -n 's/^# last full run: *//p' "$selfup" | head -1)
hk_run=$(sed -n 's/^# last full run: *//p' "$hk" | head -1)
rc_run=$(sed -n 's/^# last full run: *//p' "$rc" | head -1)
bt_run=$(sed -n 's/^# last full run: *//p' "$bt" | head -1)
po_run=$(sed -n 's/^# last full run: *//p' "$po" | head -1)
st_run=$(sed -n 's/^# last full run: *//p' "$st" | head -1)
tl_run=$(sed -n 's/^# last full run: *//p' "$tl" | head -1)
gr_run=$(sed -n 's/^# last full run: *//p' "$gr" | head -1)
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$gr" >"$work/e2e_graph.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$tl" >"$work/e2e_timeline.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$st" >"$work/e2e_stats.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$po" >"$work/e2e_policy.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$bt" >"$work/e2e_buildtools.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$rc" >"$work/e2e_recovery.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$hk" >"$work/e2e_hooks.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$pre" >"$work/e2e_pre.txt"
sed -n 's/^ck "\([^"]*\)".*/\1/p' "$selfup" >"$work/e2e_self.txt"

awk -F'|' -v e2e_file="$work/e2e.txt" -v last_run="$last_run" -v pre_file="$work/e2e_pre.txt" -v pre_run="$pre_run" -v self_file="$work/e2e_self.txt" -v self_run="$self_run" -v hk_file="$work/e2e_hooks.txt" -v hk_run="$hk_run" -v rc_file="$work/e2e_recovery.txt" -v rc_run="$rc_run" -v bt_file="$work/e2e_buildtools.txt" -v bt_run="$bt_run" -v po_file="$work/e2e_policy.txt" -v po_run="$po_run" -v st_file="$work/e2e_stats.txt" -v st_run="$st_run" -v tl_file="$work/e2e_timeline.txt" -v tl_run="$tl_run" -v gr_file="$work/e2e_graph.txt" -v gr_run="$gr_run" '
    function rule(text, ch,   i, line) {
        line = ""
        for (i = 0; i < length(text); i++) line = line ch
        return line
    }
    function title(text, ch) { print text; print rule(text, ch); print "" }
    function label(s) {
        if (s == "emrac_core") return "emrac-core library"
        if (s == "emrac") return "emrac command line, unit tests"
        if (s == "cli") return "emrac command line, integration tests"
        return s
    }
    function where(s) {
        if (s == "emrac_core") return "crates/emrac-core"
        if (s == "emrac") return "crates/emrac-cli/src"
        if (s == "cli") return "crates/emrac-cli/tests"
        return ""
    }
    { total[$1]++; all++; line[NR] = $0 }
    END {
        order[1] = "emrac_core"; order[2] = "emrac"; order[3] = "cli"
        print ".. This file is generated by dev/gen-tests-doc.sh. Do not edit it by hand."
        print ""
        title("emrac tests", "=")
        print "Every automated test that passes, grouped by suite and module, and the"
        print "end-to-end checks that are run by hand. The list is kept current by"
        print "``dev/gen-tests-doc.sh``, which runs the tests and rewrites this file; CI"
        print "runs ``dev/gen-tests-doc.sh --check`` and fails when the committed copy is"
        print "stale, so adding, renaming or removing a test always shows up in the same"
        print "change. A failing test stops the script: only passing tests are listed."
        print ""
        title("Totals", "-")
        print ".. list-table::"
        print "   :header-rows: 1"
        print "   :widths: 40 40 20"
        print ""
        print "   * - Suite"
        print "     - Location"
        print "     - Passing"
        for (k = 1; k <= 3; k++) {
            s = order[k]
            if (total[s] > 0) {
                print "   * - " label(s)
                print "     - ``" where(s) "``"
                print "     - " total[s]
            }
        }
        print "   * - **Automated total**"
        print "     -"
        print "     - **" all "**"
        print ""
        for (k = 1; k <= 3; k++) {
            s = order[k]
            if (total[s] == 0) continue
            title(label(s) " (" total[s] ")", "-")
            prev = ""
            for (i = 1; i <= NR; i++) {
                split(line[i], f, "|")
                if (f[1] != s) continue
                if (f[2] != prev) {
                    mc = 0
                    for (j = 1; j <= NR; j++) {
                        split(line[j], g, "|")
                        if (g[1] == s && g[2] == f[2]) mc++
                    }
                    if (prev != "") print ""
                    title("``" f[2] "`` (" mc ")", "~")
                    prev = f[2]
                }
                print "* ``" f[3] "``"
            }
            print ""
        }
        title("End-to-end checks", "-")
        print "These run the release binary against a real pacman inside the disposable"
        print "test container; they need podman and network access, so CI does not run"
        print "them. Run them with ``dev/e2e/run.sh``. Last full run: " last_run "."
        print ""
        n = 0
        while ((getline t < e2e_file) > 0) { n++; print "* " t }
        close(e2e_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: pre-flight, plan ids, news and trust (dev/e2e/preflight.sh)", "~")
        print "Run as root in the container with a small local repository, so that an"
        print "upgrade is really waiting and a kernel package exists: ``dev/e2e/run.sh"
        print "preflight``. Last full run: " pre_run "."
        print ""
        n = 0
        while ((getline t < pre_file) > 0) { n++; print "* " t }
        close(pre_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: the plan preview against pacman (dev/e2e/preview.sh)", "~")
        print "What emrac says a plan will do beyond its packages is compared with what"
        print "pacman then does: the config files left as .pacnew or .pacsave, and the"
        print "hooks (probe hooks next to the system ones, for an install, a removal and a"
        print "full upgrade, in order): ``dev/e2e/run.sh preview``."
        print "Last full run: " hk_run "."
        print ""
        n = 0
        while ((getline t < hk_file) > 0) { n++; print "* " t }
        close(hk_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: after a transaction pacman did not finish (dev/e2e/recovery.sh)", "~")
        print "pacman is made to fail the way a full disk does, in the middle of an install"
        print "(strace fault injection): ``dev/e2e/run.sh recovery``. Last full run: " rc_run "."
        print ""
        n = 0
        while ((getline t < rc_file) > 0) { n++; print "* " t }
        close(rc_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: build tools left behind by an AUR build (dev/e2e/buildtools.sh)", "~")
        print "A local checkout posing as an AUR package has a build-only dependency and a"
        print "runtime one; emrac must offer to remove exactly what the build added and"
        print "nothing needs: ``dev/e2e/run.sh buildtools``. Last full run: " bt_run "."
        print ""
        n = 0
        while ((getline t < bt_file) > 0) { n++; print "* " t }
        close(bt_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: graphs drawn in the terminal (dev/e2e/graph.sh)", "~")
        print "``emrac graph --format boxes`` (native) and ``--format easy`` (graph-easy,"
        print "installed from the AUR by emrac): ``dev/e2e/run.sh graph``. Last full run: " gr_run "."
        print ""
        n = 0
        while ((getline t < gr_file) > 0) { n++; print "* " t }
        close(gr_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: the timeline (dev/e2e/timeline.sh)", "~")
        print "Real transactions, a snapshot and a build-times fixture, told by"
        print "``emrac timeline``: ``dev/e2e/run.sh timeline``. Last full run: " tl_run "."
        print ""
        n = 0
        while ((getline t < tl_file) > 0) { n++; print "* " t }
        close(tl_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: statistics and benchmark (dev/e2e/stats.sh)", "~")
        print "``emrac stats`` reads a build-times file and the history; ``emrac benchmark``"
        print "runs the programs of an installed package after asking: ``dev/e2e/run.sh stats``."
        print "Last full run: " st_run "."
        print ""
        n = 0
        while ((getline t < st_file) > 0) { n++; print "* " t }
        close(st_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: override and divert against pacman (dev/e2e/policy.sh)", "~")
        print "A small package is upgraded, reinstalled and removed by real pacman while a"
        print "local file policy is set: ``dev/e2e/run.sh policy``. Last full run: " po_run "."
        print ""
        n = 0
        while ((getline t < po_file) > 0) { n++; print "* " t }
        close(po_file)
        print ""
        print "(" n " checks.)"
        print ""
        title("End-to-end: emrac updating itself (dev/e2e/self-upgrade.sh)", "~")
        print "Builds a 0.8.0 and a 0.9.0 release from the committed source and runs"
        print "``emrac self upgrade`` and ``self downgrade`` for real: ``dev/e2e/run.sh"
        print "self``. Last full run: " self_run "."
        print ""
        n = 0
        while ((getline t < self_file) > 0) { n++; print "* " t }
        close(self_file)
        print ""
        print "(" n " checks.)"
    }
' "$work/tests.txt" >"$work/out.rst"

case "$mode" in
write)
    cp "$work/out.rst" "$target"
    echo "wrote $target ($(grep -c '^\* ``' "$target") tests, $(wc -l <"$work/e2e.txt") + $(wc -l <"$work/e2e_pre.txt") + $(wc -l <"$work/e2e_self.txt") + $(wc -l <"$work/e2e_hooks.txt") + $(wc -l <"$work/e2e_recovery.txt") + $(wc -l <"$work/e2e_buildtools.txt") + $(wc -l <"$work/e2e_policy.txt") + $(wc -l <"$work/e2e_stats.txt") + $(wc -l <"$work/e2e_timeline.txt") + $(wc -l <"$work/e2e_graph.txt") end-to-end checks)"
    ;;
check)
    if ! diff -u "$target" "$work/out.rst" >"$work/diff.txt"; then
        cat "$work/diff.txt"
        echo "dev/gen-tests-doc.sh: $target is out of date; run dev/gen-tests-doc.sh and commit it." >&2
        exit 1
    fi
    echo "$target is current"
    ;;
esac

# Slice u22 (after r3) — AUR builds and reviews in emrac's style; the three limits of u21 mended

**Status: built.**

## The three limits of u21, mended

- **A hook that moves the cursor itself.** Cursor-movement and erase sequences are now removed only from the lines that belong to pacman's own bars and download block (those lines are redrawn by emrac); everything else is passed with its sequences intact, so a hook or scriptlet that animates its own line still does. If pacman hid the cursor for its bars and was stopped before showing it, emrac shows it again.
- **A resize in the middle of a run.** The terminal's width is read about ten times a second; when it changes, pacman is told (the pseudo-terminal's size) and the live line is erased wherever the terminal wrapped it (it goes up the rows it took) and drawn again to fit.
- **AUR builds.** Below.

## An AUR build on a terminal

`makepkg`'s own lines become phases: `▸ Retrieving sources...`, `▸ Extracting sources...`. Inside `prepare()`, `build()`, `check()` and `package()` the compiler's lines are folded into **one live line**: a spinner whose colour moves, a clock, a bar when the build says how far it is (cmake `[ 45%]`, ninja and meson `[23/456]`), the verb and the file it is on (`CXX src/a.cpp.o`, `LINK foo`, `RUST serde 1.0`, `GO ...`), and the warnings counted. **Errors are shown the moment they happen** (`✖ a.c:2:2: error: ...`). When a phase ends it stays as `✔ build()  2 min 13 s  342 files · 14 warnings`. Everything outside those phases, above all the dependency step and its questions and the sudo prompt, is passed through untouched; a question is shown at once.

The **whole output is kept** in `~/.local/share/emrac/build-logs/<package>-<time>.log` (the 20 newest are kept). When a step fails, the last 25 lines and the path of the log are shown in a frame, then emrac's own error as before.

It covers the sandboxed steps (through the pipes of the sandbox, which tools see as no terminal) and the host steps and the unsandboxed build (through a pseudo-terminal, the same way as pacman's transaction). `clui.builds = "native"` gives makepkg's own output; so does anything that is not a terminal, with `--json`, or in CI.

## What is read before a build

On a terminal (piped, the plain text of before):

- **The PKGBUILD** (first build): numbered lines, shell coloured by what each word is (assignments, functions, `https://` addresses, `if/then/fi`), long lines wrapped under their own margin instead of cut, and a mark `⚠` beside the lines that fetch or escalate (`curl`, `wget`, `sudo`, `eval`, `base64`, `chmod`, ...). The mark does not judge; the audit does.
- **What changed** since your last build: a summary of the assignments that matter (`pkgver 1.2.3 → 1.2.4`, `source`, the checksums, `depends`, ...), with the part that changed picked out and a `⚠` and a reminder when the sources or checksums changed; then each hunk with the old and the new line numbers, the sign, the function it is in, and the changed lines paired so only what really differs is bold.
- **The audit findings**: a badge for the severity (`CRITICAL`, `HIGH`, `warning`, `info`), the rule, the file and line, what it means wrapped to the terminal, and the line it found.
- **The trust signals**: `✖ HIGH`, `⚠ warning`, `● note`, the text wrapped under its label.

## Tests

`build_ui.rs` (11), the renderer additions in `pacman_ui.rs` (cursor kept, cursor shown again, resize), `review_view.rs` (9), `strip_ansi` taking out character sets and operating-system commands; in the container `dev/e2e/buildui.sh` (13): both build paths, the log, `clui.builds native`, a failing build, and the PKGBUILD diff, the sources mark, the audit badge and the plain text when piped.

## Not covered

`makepkg` output of `emrac self upgrade` (it builds emrac itself and keeps its own output); a build tool that prints its progress only to a terminal through the sandbox's pipes (it prints plain lines instead, which is parsed the same way); the PKGBUILD viewer does not highlight inside quoted strings or `$(...)`.

# Slice u21 (after r3) — pacman's transaction in emrac's style

**Status: built.**

pacman still downloads and installs; emrac shows its transaction in emrac's own style, and pacman's questions stay as they are.

## How

emrac starts `sudo pacman …` with its standard output and error on a **pseudo-terminal**, so pacman believes it is on a terminal and draws its real progress bars; **its input stays on the real terminal**. That keeps three things as they were:

- **Questions** (`:: Proceed with installation? [Y/n]`, key imports, replacing a package) are shown the moment pacman asks and answered where you are; the answer is typed on the real terminal.
- **sudo** asks for its password on the terminal as always and keeps its per-terminal ticket (a pseudo-terminal of its own would ask again).
- **Ctrl-C** reaches pacman exactly as without emrac (emrac installs a handler that does nothing, so it stays to finish the screen; pacman starts with the default).

What pacman writes is read, and **only what is understood is redrawn**:

| pacman | emrac |
|---|---|
| `(2/5) installing foo [####---] 45%` and the checks (`checking keys in keyring`, `loading package files`, …) | one line redrawn in place, a gradient bar, the percentage and `2/5`; a `✔` line stays when it finishes |
| the parallel download block (`Total (1/3) … [###] 50%`, the file rows, cursor movements) | one live `↓ downloading 1/3` line with the bar, size, rate and time; a single file with no total is shown as itself |
| `(1/2) Arming ConditionNeedsUpdate...` | a `✔` line with a small bar |
| `:: Processing package changes...` | the same words in the brand colour |
| **everything else**: the question, the package table, warnings, errors, the output of hooks and scriptlets | **byte for byte**, in order; a half-written line (a prompt) is shown at once |

Cursor-movement and erase sequences of pacman are dropped (emrac does the drawing); colours are kept. If a progress line is not in the exact shape pacman uses (for example with `ILoveCandy`) it is simply passed through.

## When it is used, and when it is not

On a terminal (colour or not), with input from a terminal, for a `pacman` command run by emrac, and `clui.pacman = "emrac"` (the default). Not when piped, in CI, with `--json` or `--quiet`, on `TERM=dumb`, with `clui.pacman = "native"`, or if a pseudo-terminal cannot be opened: then pacman owns the terminal as before. All of emrac's transactions (install, upgrade, remove, downgrade, bundle install, swap) go through the one place that decides.

## Tests

13 unit tests of the renderer with the byte streams pacman writes (steps, parallel downloads with cursor movement, hooks, questions, warnings passed byte for byte, a sequence cut in two by a read, colour only decorating); 3 of the runner; and in the container (`dev/e2e/clui.sh`, bars switched on in the test image): an install in the new style, the question shown and answered with typed input, `clui.pacman native` giving pacman's own bars, a piped run untouched, a failed command still failing. By hand in the container: a Ctrl-C during a 124-package download stops pacman and emrac says so.

## Not covered

AUR builds (`makepkg` keeps its own output); the live line is not redrawn when the terminal is resized mid-run (the next update fits); a hook or scriptlet that moves the cursor itself loses those movements.

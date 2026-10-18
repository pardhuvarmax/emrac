# Slice u17 (after r3) — a better-looking command line: styles, marks, bars, animations

**Status: decided and being built (all of it in this one slice).**

Decisions: one slice for everything (A to E); animations are a setting with two values, `regular` (the default: nothing moves, as
now) and `active`; Unicode by default with an automatic ASCII fallback when the locale is not UTF-8; a built-in width table, no new
dependency; the four themes. The static design (marks, tables, bars, colour) is on in a terminal; only the motion is optional. The
settings are in a `[clui]` section: `clui.animations`, `clui.color`, `clui.unicode`, `clui.theme`. The layout changes show only
on a terminal; piped, redirected, `--json` and on a dumb terminal the output is today's plain text.

## Where it stands today

- One thing is styled: the verb of `emrac <verb>:` is coloured (`voice.rs`), only on a terminal, never with `NO_COLOR`, never on
  `TERM=dumb`.
- Everything else is plain text lined up by hand with `format!` (about 460 `println!` in the command line crate): plans, the
  doctor, history, tables of `stats`, `mirrors`, `timeline`.
- Nothing moves. A wait that emrac does itself (measuring mirrors, hashing a bundle, fetching the Arch news, the AUR, `doctor --online`)
  prints nothing, or one `measured 6 of 6` line on standard error at the end of each batch.
- There is no setting for any of it, no `--color` flag, no terminal width, no Unicode/ASCII choice.
- pacman and makepkg own the terminal while they run, with their own progress bars. That stays: emrac does not touch their
  output.

## What this slice would add

### 1. One place that decides how things look (`style.rs`)

Everything below goes through one module, so a command asks for a *role* (`heading`, `ok`, `warn`, `problem`, `dim`, `package`,
`version`, `size`, `command`, `path`) and never writes an escape sequence itself.

It decides, once per stream, from facts and settings:

| Decision | From |
|---|---|
| colour on or off | a terminal; not `NO_COLOR`; not `TERM=dumb`; `CLICOLOR_FORCE`; `--color auto\|always\|never`; `ui.color` |
| 16 colours, 256 or true colour | `COLORTERM`, `TERM` |
| Unicode marks and box lines, or plain ASCII | `LANG`/`LC_ALL` saying UTF-8; `TERM=linux`; `ui.unicode` |
| the width | the terminal (`ioctl`), `COLUMNS`, else 80 |
| animation on or off | a terminal on standard error; not `CI`, not `TERM=dumb`; not `--quiet`, `--json`; `ui.animations` |

The rule that keeps everything else safe: **when output is piped, redirected, `--json` or on a dumb terminal, it is exactly
today's plain text.** The 600 tests and the container checks read plain text and keep working unchanged.

### 2. Marks, headings, bars, tables

Small, consistent pieces used by every command:

- **Marks** with a word beside them, so colour is never the only signal: `✔ ok`, `⚠ warning`, `✖ problem`, `● note` (ASCII:
  `[ok]`, `[!]`, `[x]`, `[i]`).
- **Aligned tables** that know the width: columns sized to their content, long values cut with `…`, wide characters counted
  correctly, a table that would be too wide dropping its least important column instead of wrapping.
- **Bars** only where the scale is meaningful and stated: a plan's disk growth as a share of the free space on `/`, mirror speed
  against the fastest, build times against the longest; `▇▇▇░░░░░`.
- **Sparklines** for a short history: `▁▃▂▇▅` (last builds, last mirror times).
- **Coloured diffs**: `+`/`-` lines in the `.pacnew` and PKGBUILD diffs (with `+`/`-` still there for the colour-blind).
- **Version changes** `1.2.0 → 1.3.0` with the part that changed highlighted.

### 3. Animations (only where emrac itself is waiting)

- A **spinner** with a label for a wait of unknown length: `⠋ asking the AUR`, and a **progress bar** with `N of M` when the
  count is known: `▰▰▰▱▱▱ 4 of 9 mirrors`.
- It is drawn on **standard error** by a small background thread, about twelve frames a second, with the cursor hidden and
  *always restored* (on finishing, on an error, on Ctrl-C).
- When the work ends the line is **replaced by one result line** that stays in the scrollback: `✔ measured 9 mirrors in 3.1 s`.
- Not on a terminal, in CI, with `--json`, `--quiet`, `TERM=dumb`, or `ui.animations off`: nothing animates, and a wait that took
  more than two seconds prints one plain line ("asking the AUR…") instead.
- Never over pacman, makepkg or the build sandbox output, and never while a question is being asked.

Where it would appear first: `mirrors test`, `bundle create` and `verify` (hashing), `benchmark`, `doctor --online`, `news`,
AUR searches and `info`, `audit`, `verify`, `stats` on a big history.

### 4. Settings

A new `[ui]` section of the config, and a global `--color` flag:

```
[ui]
color      = "auto"      # auto | always | never
unicode    = "auto"      # auto | always | never   (never = ASCII marks and lines)
animations = "auto"      # auto | on | off
theme      = "default"   # default | light | mono | high-contrast
```

Themes only change colours (the roles of section 1). `mono` uses bold and dim only; `high-contrast` avoids the dim greys. No theme
relies on red against green alone.

### 5. What it would look like

Today:

```
emrac plans: install 3 packages

  extra/vim 9.1.1-1
  extra/vim-runtime 9.1.1-1 (dependency)
  extra/gpm 1.20.7-5 (dependency)

Download Size    : 9.4 MiB
Installed Size (+) : 41.0 MiB
Plan id          : 9d0ddb545688   (`--expect-plan 9d0ddb545688` runs only this plan)
```

Proposed (colour not shown here; names bold, repository and "dependency" dim, versions coloured):

```
 install · 3 packages
 ╭──────────────────────────────────────────────────╮
 │ extra   vim           9.1.1-1                    │
 │ extra   vim-runtime   9.1.1-1   dependency       │
 │ extra   gpm           1.20.7-5  dependency       │
 ╰──────────────────────────────────────────────────╯
   download   9.4 MiB
   disk      +41.0 MiB   ▇░░░░░░░░░░░  0.04% of the free space on /
   plan id    9d0ddb545688   (--expect-plan 9d0ddb545688 runs only this plan)
```

The doctor:

```
 ✔ ok       Dependencies      every installed package's dependencies are satisfied
 ⚠ warning  systemd units     1 unit has failed
              vboxdrv.service (system)
 ● note     Orphans           20 dependencies that nothing needs (1.0 GiB)
              emrac remove --recursive ...
```

A wait:

```
 ⠹ measuring mirrors  ▰▰▰▰▱▱▱▱ 4 of 9   in.arch.niranjan.co
 ✔ measured 9 mirrors in 3.1 s
```

## How it would be built, in order

| Part | What | Size |
|---|---|---|
| A | `style.rs`: detection, roles, marks, width, table, bar, sparkline; the `[ui]` section and `--color`; unit tests | foundation |
| B | Plans and previews (`install`, `upgrade`, `remove`, `downgrade`), the pre-flight, the doctor, with the `✔ ⚠ ✖` marks | the part you see most |
| C | Spinner and progress on the waits listed above, with the cursor always restored | the "alive" part |
| D | Tables (`history`, `timeline`, `stats`, `mirrors`, `cache`, `snapshot`), diffs, sparklines, the themes | polish |
| E | `emrac ui` — a gallery that shows every element in every theme (also the self-test for a terminal) | small |

A and B and C are one slice (u17); D and E could be u18 if you would rather keep each slice small.

## How it would be tested

- Pure functions, unit-tested: capability detection as a table of cases (like `color_allowed` today), width and truncation
  (including wide and combining characters), bars and sparklines at their edges, the spinner frame for a given tick, the
  progress line for N of M.
- **The invariant that matters**: for every converted command, the output with colour forced on and the escape sequences stripped is
  *identical* to the output with colour off. So styling can never change the words or the layout, only decorate.
- A test that nothing outside `style.rs` writes an escape sequence (the way a test already scans for hand-built `emrac verb:`).
- Container checks that run emrac under a pseudo-terminal (`script`): colour present on a terminal, absent under `NO_COLOR`, with
  `--color never`, piped, with `--json`; the spinner line is cleared and one result line remains; the cursor is shown again after an
  error and after Ctrl-C.

## What it would *not* do

- Change what any line says: the twelve verbs (`emrac plans:`, `emrac warns:` ...) stay, with their colours.
- Touch pacman, makepkg or sandbox output, or add a full-screen interface (that is the TUI, a later and larger slice).
- Make any output depend on being coloured: every mark has a word, every bar has its number.

## Costs and risks

- About 460 print sites to move over gradually; part B and C cover the commands you see on every upgrade, D the rest. Unconverted
  commands keep working as they are.
- A background thread for animations: it must stop and restore the terminal on every path; that is what the pseudo-terminal checks
  are for.
- Width and Unicode: correct alignment of wide characters needs a table of character widths. Options: the small `unicode-width`
  crate (one new dependency), or a minimal built-in table covering the characters emrac prints (marks, box lines, bars, braille).
  Recommendation: the built-in table, no new dependency; the input is mostly package names and our own symbols.

## Decisions I need from you

1. **Scope**: u17 = A + B + C (foundation, plans/doctor, animations) and u18 = D + E (all tables, themes, gallery)? Or everything in one?
2. **Animations on by default** on a terminal (with `ui.animations off` to turn off)?
3. **Unicode by default** with an automatic ASCII fallback when the locale is not UTF-8?
4. **Themes**: the four listed (`default`, `light`, `mono`, `high-contrast`), or fewer/more?
5. **Dependencies**: built-in width table (recommended), or add `unicode-width`?
6. Anything you want in the look that is not here: a banner when you run plain `emrac`, a different mark set, a different spinner,
   sound-free "done" notifications, a colour you want for the brand?

Package Score (the slice that was u17) moves down by one or two numbers.

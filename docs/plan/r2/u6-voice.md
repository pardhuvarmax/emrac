# Slice u6 — emrac's voice

emrac talks to people in a few places and says almost everything in the same flat way:
57 lines begin `emrac notes:`, 28 `emrac found:`, 10 `emrac warns:`, 8 `emrac plans:`.
`notes` carries facts, guesses, advice and results alike, so the reader cannot tell
whether emrac knows something, believes it, or is only guessing. This slice gives each
kind of statement its own verb, so the verb tells the reader how much to trust the line
and what to do with it. It changes wording and the stream a line goes to, never behaviour,
exit codes or `--json`.

(Independent of the history slices u7 to u9 (archive and chain, ledger, ledger watch). They were planned first and numbered u6 to u8; they moved up by one so that this slice is built first.)

## The voices

| Voice | Means | Example |
|---|---|---|
| **says** | something broke underneath emrac; not the user's doing | `emrac says: pacman exited with status 1` |
| **found** | a fact emrac discovered that the user can act on | `emrac found: 5 config files waiting` |
| **notes** | plain information, nothing to decide | `emrac notes: nothing to remove.` |
| **plans** | what it is about to do | `emrac plans: upgrade 1 package` |
| **asks** | a question; the answer decides what happens | `emrac asks: Retry with network access? [y/N]` |
| **warns** | a real risk, or something that could hurt | `emrac warns: building 'x' WITHOUT the sandbox` |
| **suspects** | a conclusion from weak or indirect evidence; may be wrong | `emrac suspects: the missing network is why 'x' failed (the output mentions a host it could not resolve)` |
| **thinks** | a judgment with its reasons, from evidence that is good but not proof | `emrac thinks: swapping scenefx0.5 for scenefx is safe: it provides the same libscenefx-0.5.so` |
| **suggests** | the next step, usually a command to run | `emrac suggests: emrac orphans, then emrac remove what you don't want` |
| **refuses** | a deliberate refusal, with no override | `emrac refuses: a closed terminal is not an answer; nothing was installed` |
| **confirms** | something was checked, or done, and holds | `emrac confirms: the checksum matches` |
| **remembers** | something it recalls from what you did before | `emrac remembers: you reviewed this PKGBUILD on 2026-10-10; showing only what changed` |

**Rules that keep it honest**
- A hedge (`probably`, `looks like`, `may`) appears only under **suspects** and **thinks**,
  and those two never state a fact flatly. **says**, **found** and **confirms** never hedge.
- **suspects** is for heuristics (a rule that guesses a download failed; an audit warning),
  **thinks** for a judgment that rests on checked facts (a classification, a safety verdict),
  **warns** for a consequence rather than a doubt.
- **refuses** is only for things that cannot be overridden (critical findings, an answer that
  never came); a default-no prompt is **asks**.
- One sentence, sometimes two. Plain words, lower case after the colon, no exclamation marks,
  no jokes, no apologies.

## What each part of emrac says

- **Errors** keep `found` (the user can fix it) and `says` (something failed underneath), word
  for word as the README documents them; scripts that read stderr keep working.
- **Audit and manifest:** info stays a note, a warning becomes **suspects** (static reading can
  only suspect), high is **warns**, critical is **refuses**. The `[warning]` / `[HIGH]` tags
  stay as they are.
- **Plans:** `plans` for the preview, **warns** for removals and risks in it, **thinks** for a
  dependency-conflict verdict ("the swap looks safe"), **suspects** for the network-retry
  guess, **suggests** for the command that follows.
- **`pacnew`:** the class reasons are **thinks** ("you never edited it: the live file is exactly
  what 4.1.3-1 shipped") and, where no earlier version could be read, **found** ("no earlier
  version to compare with"); applied changes **confirm**.
- **`doctor`:** each `fix:` line is **suggests**; checks that pass stay `ok`.
- **Prompts that decline** by silence say **refuses** first, then what was left unchanged.
- Build steps that pass say **confirms** ("checksum matches", "done"); a PKGBUILD that changed
  since the last review is **remembers**.

## Mechanism

- One `Voice` enum in the core library, with the word, the stream (stdout for `plans`, `notes`,
  `confirms`, `found` lists; stderr for `says`, `warns`, `suspects`, `refuses`, and error
  `found`) and the colour. `Error::voice()` returns it. Nothing else builds an `emrac word:`
  prefix by hand.
- Core messages that carry a voice (plan warnings, doctor fixes, `pacnew` reasons) hold a
  `Voice` next to the text, so the front end does not guess it from the wording.
- **Colour** on the voice word only, when the stream is a terminal, `NO_COLOR` is unset and
  `TERM` is not `dumb`; never in `--json` or when piped. Colours come from the palette:
  says and refuses red, warns yellow, suspects magenta, thinks cyan, suggests and confirms
  green, asks blue, plans and found default, notes and remembers dim.
- `--quiet` keeps hiding notes, plans and confirms; warnings and errors always show.

## What was built, and what was not

Built as planned: the twelve verbs, streams and colour; errors unchanged; audit findings
by worst severity; `pacnew` reasons, doctor fixes, the retry and conflict messages;
the refusal when no answer comes; both guard tests. Not built: voices carried by core
messages (plan warnings stay `warns`), and a `remembers` line for a PKGBUILD reviewed
before beyond the one for a changed PKGBUILD.

## Not covered

Translating the messages; changing what the commands decide; the wording inside `--json`
(field names and values stay); colouring anything but the voice word.

## Verification

Unit tests: every voice's word, stream and colour; colour only on a terminal and never under
`NO_COLOR`; the hedging rule (a test that lists every message built for **says**, **found** and
**confirms** and fails on a hedge word); and a source scan that fails the build if an
`"emrac word:"` literal appears outside the voice module, so a new message cannot skip the
vocabulary. The existing integration tests that assert on message text are updated with the
messages. Container, against a real pacman: a plan with a conflict, a refused install with a
closed stdin, a blocked download, an audit with a warning and a high finding, `pacnew`
classes and `doctor`, each read for the right voice and stream.

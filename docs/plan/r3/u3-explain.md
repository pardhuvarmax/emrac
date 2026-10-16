# Slice u3 (after r3) — `emrac explain`: the reasoning, input by input

`emrac explain <pkg>` shows how the recommendation of u2 was reached, not only the verdict (`SPEC.md`
Part IV #2). It uses the same engine; nothing new is computed.

```
Recommendation: binary           confidence: medium (one measured build)

  Rule                         Input                                   Points
  ✓ start                      —                                          50
  ✗ your flags match the repo  CFLAGS "-march=x86-64 -O2" (makepkg.conf)  cap 30
  ✗ build time                 14 min (measured once, 2026-10-16)         −20
  ✗ changes often              5 updates in 90 days (emrac history)        −8
  ✓ ...
  Score 22 of 100  →  binary (35 or less)

What would change this:
  - `-march=native` in makepkg.conf: +25, the score would be 47 (either)
  - three more measured builds: the confidence would be high
```

- Every line says **where the input came from** (read from, measured, estimated) so nothing is
  taken on trust.
- "What would change this" lists each rule that is not at its best and what the score would be if
  it were: the engine is run again with that one input changed. Deterministic; no guessing.
- `--json` carries the same table.
- `recommend` keeps showing the top three reasons and says `emrac explain <pkg>` for the rest.

Verification: unit tests on the "what would change" calculation (each alternative recomputed from
the engine), the table text for each source of input, and the JSON; container: the example above
against fixtures.

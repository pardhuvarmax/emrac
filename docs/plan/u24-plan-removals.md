# Slice u24 — the plan lists what conflict resolution will remove

## The gap

`emrac install pipewire-jack` planned "install 1 package" and said nothing about
`jack2`, which pacman removes because the two conflict. The plan came from
`pacman -S --print`, which neither prints nor asks about removals. Since u23 the
user sees pacman's question when it runs, but a preview that leaves out a
removal is the same kind of fault as the stale upgrade plan (u22): the thing
you confirm is not the thing that happens.

## Design

Ask libalpm itself — the code pacman uses — instead of guessing from metadata:

- Build a throwaway libalpm handle (read-only use: transaction flag `NO_LOCK`,
  nothing is committed or downloaded), add the requested packages (or
  `sysupgrade` for a full upgrade, with pins ignored), register a question
  callback that records and answers **yes** to "conflict: remove X?" and
  "replace X with Y?" (pacman's own defaults for a user who accepts), and
  `prepare` the transaction.
- The transaction's remove list is exactly what pacman would remove. Each
  removal carries its reason: "conflicts with <pkg>" or "replaced by <pkg>".
- A full upgrade is simulated against the freshly synced throwaway database
  (u22), so it matches `-Syu`.
- `Plan` gains `removals` (and the net size change accounts for them). They are
  printed as their own section, included in `--json`, and recorded in the
  history; `undo` says that it cannot restore them.
- If the simulation itself cannot run, the plan says so (`warnings`) instead of
  silently omitting the section.

Not covered: AUR packages (their conflicts surface when `pacman -U` runs, and the
manifest lists what they ship), provider choices (pacman asks at execution).

## Verification

Pure tests for reason wording and size accounting; then the real conflict in the
container (`pipewire-jack` vs `jack2`) with the plan checked against what pacman
then actually does, and a replacement case; the host's full upgrade plan.

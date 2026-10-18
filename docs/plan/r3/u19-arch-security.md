# Slice u19 (after r3) — Arch security data: standings, gates, evidence

**Status: built.**

The Arch Security Tracker keeps some old entries open forever: an entry with no recorded fix is "Vulnerable" until someone
closes it, and nobody always does. Taking those at face value gives false alarms; ignoring them gives false comfort. So an entry
that applies to an installed version gets a **standing**, with the evidence for it, and only a *confirmed* standing can warn or block.

## Standings

| Standing | When | Counts for the score | Can gate |
|---|---|---|---|
| fix available | a fix is recorded and this version is older than it | 100% | yes |
| open | no fix is recorded; the newest CVE is recent and the installed major version is the affected one | 100% | yes |
| possibly stale | no fix is recorded and the newest CVE is more than two years old, or the installed major version differs from the affected one | 50% | never |
| ignored by you | you ran `emrac vulns ignore AVG-…` | 0% | never |

The standing always comes with a sentence saying why. An ignore lapses when the entry later records a fix (the fix the entry had when
you ignored it is kept; a different one ends the ignore).

## Commands

- `emrac vulns` — installed packages against the tracker, confirmed entries first; `--all` adds possibly stale and ignored ones,
  `--fixable` keeps entries whose fix the repositories already have, `--check` exits with an error while a confirmed High or Critical
  entry exists, `--json`.
- `emrac vulns ignore AVG-… [--reason …]`, `unignore`, `ignored`. Ids are `AVG-` and digits; the list is `~/.local/share/emrac/vuln-ignore.json`.
- `emrac doctor` has a "Known vulnerabilities" row (saved list, `--online` refreshes). It warns only for a confirmed High or Critical
  entry with a fix already in the repositories; otherwise it is a note.
- The pre-flight of `install`, `upgrade`, `reinstall`, `downgrade` checks the versions the plan would install, and says what the plan fixes.
  `safety.vulnerabilities`: `warn` (default; tells, does not stop), `block` (a confirmed High or Critical entry is a problem), `off`.

## In the score

The Security dimension counts standings (the cap stays 60). A **gate** is shown above the dimensions when a confirmed High or Critical
entry applies (or the PKGBUILD audit finds a critical problem); it is not part of the number and cannot be averaged away. Each
dimension names its **evidence**: authoritative (a database such as the tracker or a signature), corroborating, or heuristic (a
rule of thumb); the overall number says how much of its weight rests on known dimensions.

## Hardening

The list is fetched over https only (a `file://` address works for tests), entry ids must have the `AVG-` shape, a list that shrinks
by more than half against the saved one is refused, and a bad fetch falls back to the saved copy with a note saying how old it is.

## Not covered

Provenance, compatibility, reproducibility (reproducible.archlinux.org), packaging quality from PKGBUILD history, OSV and the
OpenSSF Scorecard, third-party repositories: later slices.

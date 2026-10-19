# Slice u24 (after r3) — `emrac bisect`

`additions.md` B1. Something worked last week and does not now. `bisect` finds the transaction that broke it by
putting the system back into states it really had, a few at a time, and asking whether it is still broken. A git
bisect for the system.

```
emrac bisect --good tx-41 --test 'pgrep -x foo && foo --selftest'
emrac bisect --good 2026-10-10                       # judged by you: good / bad / skip
emrac bisect --good 7d --bad tx-58 --dry-run         # what it would take; nothing is changed
```

## The rule that decides the design

Bisecting **packages** would build states that never existed (some changed packages new, some old): a partial
upgrade by construction. Bisecting **transactions** does not: the state after transaction *k* is a state the
machine was in, every package at the version it had then. So the search runs over the transactions between
*good* and *bad*, and every state it visits is a real, coherent one. The answer is the first transaction after
which the test fails, and the packages it changed.

## How the states are known

From the history, backwards from what is installed now: the newest committed transaction is undone, then the one
before, and so on (an upgrade goes back to its `previous_version`, an install is removed, a removal and a conflict
removal come back at the version they had). Each step is checked against the state it undoes; a package whose
version the history does not explain (changed by `pacman` directly, a failed transaction that half ran) is listed as
**unexplained** and kept at its current version in every state, and the report says so. Failed transactions are not
states.

## What a run does

1. `--good` is a transaction id (the system was fine after it), a date or `7d` (the state at the end of that
   moment), or `--bad` likewise (default: now).
2. The plan: transactions in the window, the steps it needs (⌈log₂ n⌉), the packages that differ between good and
   now, and — before anything changes — whether **every** package version any state in the window needs can be
   found (the cache, a repository, the Arch Linux Archive). Missing ones are listed together; nothing starts.
3. `--dry-run` stops there.
4. A snapshot named `bisect-<time>` of the current state is saved. Then for each step: the midpoint state is
   restored with the machinery of `restore --prune` (so it is recorded in the history, journaled, and runs through
   `pacman -U`), the test is run (`--test` command: exit 0 good, 125 cannot tell, anything else bad; without it you
   are asked), and the window is halved.
5. Whatever happens (a failure, `q`, Ctrl-C between steps), the last thing it does is put the system back to where
   it started and verify it; if that fails it says so and names the snapshot to restore.
6. The answer: the first bad transaction, the packages it changed with their versions, and what to do
   (`emrac downgrade <pkg>`, `emrac pin`). A window where the test cannot tell ends with the range it narrowed to.

## Not here

Bisecting inside one transaction (a single upgrade of 40 packages): the answer names the transaction and its
packages; telling which package inside needs a dependency-closed split and is a later slice. Packages built from the
AUR are restored only from files that exist (cache); a missing one is reported up front. Config files and data are
not part of a state: a package that migrates its data on upgrade cannot always be put back and tested.

## Design

`emrac-core/src/bisect.rs`: `Point`, `Timeline`, `reconstruct`, `locate`, `changed`, `Search` (pure, tested).
`Sources::{plan_restore_snapshot, bisect_availability}`; `commands/bisect.rs`. `restore` is reused, not copied.

## Verification

Unit tests: reconstruction through install, upgrade, downgrade, removal and conflict removal; unexplained packages;
the search over every window size with every possible culprit and with skips. Container check
(`dev/e2e/bisect.sh`): five real transactions with fixture packages, a test that fails from the third on, the right
transaction found, the system back at the start, the history containing the steps, a missing file refused up
front, and `--dry-run` changing nothing.

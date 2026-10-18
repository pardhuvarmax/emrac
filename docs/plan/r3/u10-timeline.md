# Slice u10 (after r3) — `emrac timeline`

`SPEC.md` Part IV #9. `history` is a flat log of transactions; `timeline` is the same story told by day
and with the other things emrac knows happened to the system.

```
emrac timeline                    # the last 7 days
emrac timeline --since 30d        # or a date: --since 2026-10-01 (UTC)
emrac timeline --pkg mesa         # only what touched one package
emrac timeline --json
```

```
Today
  18:42  Upgraded mesa 24.2.1 -> 24.2.3, linux 6.11.1 -> 6.11.2
  17:05  Built neovim-git 0.11 in 4 min
Yesterday
  21:10  Installed firefox 130.0
  20:58  Snapshot "before-big-upgrade" (1412 packages)
  09:12  Removed discord 0.0.60
2026-10-14
  Failed: install foo (conflicting files)
```

## Sources

Only records emrac already keeps, each event labelled with where it came from:

| Source | Events |
|---|---|
| the transaction history | installed, upgraded, downgraded, reinstalled, removed, purged; failed transactions with the first line of the error; rollbacks (a record with `rollback_of`) |
| `build-times.jsonl` | built (time taken) and failed builds |
| the snapshot store | snapshot created, with its name and package count |
| the `.pacnew` log | config file kept, taken, merged, edited or reverted |

Not here: `pacman.log`, file change times and enabled units (the package forensics `timeline` in
`docs/feats/pkgforensics.md` merges those; this one stays within emrac's own records).

## Rules

- Newest first, grouped by UTC day: `Today`, `Yesterday`, then `YYYY-MM-DD`. Times are UTC and the report says so.
- A transaction with more than 4 packages shows the first 4 and "and N more".
- `--pkg` keeps events that name the package; snapshots are kept only if they contain it, config events only for
  that package's files.
- `--since` takes `30d`/`12h`/`8w` or a `YYYY-MM-DD` date; the default is `7d`.
- Read only. A source that cannot be read is skipped with a note rather than failing the command.

## Design

`emrac-core/src/timeline.rs`: `Event`, `Kind`, `Filter`, `collect(history, builds, snapshots, pacnew, filter)`,
`group(events, now)`. Pure and tested. `Sources::timeline(filter)` reads the four sources;
`commands/timeline.rs` prints.

## Verification

Unit tests for each kind of event, the "and N more" cut, grouping across midnight, the `--pkg` and `--since`
filters, and ordering. A container check in `dev/e2e/timeline.sh` against real transactions.

# Slice u22 — upgrade previews that match reality

## The bug

`emrac upgrade` built its plan from the sync databases **as last refreshed**, but
the real step runs `pacman -Syu`, which refreshes first. On a machine whose
package list is days old the plan therefore:

- omitted every official update published since the last refresh (measured on
  this host, 11 days stale: **347 official packages** that `checkupdates`
  sees, **0** in emrac's plan — only 23 AUR packages were listed);
- said "nothing to upgrade — you're already up to date" and returned before any
  refresh when there was nothing in the stale view at all;
- and if even one official package was in the stale view, the confirmed
  upgrade would apply all the unseen ones: the preview didn't match what
  happens, which breaks the central promise of a preview-and-confirm tool.

`emrac doctor` had the same flaw ("Pending upgrades: up to date", computed from
the same stale list); its wording was fixed first (commit `b7620bb`).

## Design

- **A throwaway, freshly synced database**, the way pacman-contrib's
  `checkupdates` does it: a private 0700 directory under `$TMPDIR` whose
  `local` is a symlink to the real local database, and `fakeroot -- pacman -Sy
  --dbpath <dir> --logfile /dev/null` to fill its `sync/`. The real database is
  never touched and no root is needed. The directory persists, so later runs
  only download what changed.
- **The upgrade plan comes from that view**: names from `pacman -Su --print
  --dbpath <dir>`, versions and sizes from a libalpm handle opened on it. So the
  plan lists what `-Syu` will really do (modulo mirrors changing in the few
  minutes between preview and confirm).
- **When it can't be refreshed** (`--offline`, no network, no `fakeroot`) the
  plan falls back to the stale list and *says so*: how old the list is, why it
  couldn't be refreshed, and that the real upgrade refreshes first and may
  include more. An empty stale plan is **never** reported as "up to date".
- **`emrac doctor --online`** gets its pending-upgrade count from the same fresh
  view (it needs the network, so it is opt-in; the default stays offline and
  honest about the age of the list).
- Pins are honoured in the fresh resolution exactly as before.

## Not changed

`install <pkg>` and targeted `upgrade <pkg>` resolve against the current
local databases and execute against them (`pacman -S`, no refresh), so preview
and execution already agree. Whether those databases are stale is `doctor`'s
sync-database check.

## Verification

Pure tests for the freshness decision and wording, the private-directory safety
check and the `--dbpath` argument placement; then the real thing: on this host
the plan must list the ~347 official updates `checkupdates` finds, with the
real database's modification time unchanged afterwards; in the container, a
stale database whose upgrades are invisible without the fresh view; and the
fallback paths (`--offline`, no network).

## Files

`backend/fresh_db.rs` (new), `backend/pacman.rs` (`--dbpath`),
`backend/alpm.rs` (`init_with_dbpath`), `sources.rs`, `doctor.rs`, `error.rs`;
CLI `commands/upgrade.rs`, `commands/doctor.rs`, `cli.rs`, `output.rs`.

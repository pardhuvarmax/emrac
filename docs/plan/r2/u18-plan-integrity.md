# Slice u18 — what was previewed is what runs

From `docs/feats/additions.md` C2. A plan is shown, a person reads it, maybe reviews an AUR
PKGBUILD for a while, and answers; then `pacman` runs and resolves everything again against
whatever the databases say by then. If the repositories moved in between, or something else changed
the installed set, the transaction is not the one that was approved.

## Plan ids

`Plan::digest()` is a SHA-256 over what the plan *does*: its action, every package with version,
repository and previous version, the packages removed on the side, and the provider choices; not
the order, the sizes or the warnings. The first 12 digits are printed under every plan
(`Plan id: 3032684eaee6`) and the whole digest is `digest` in `--json`. Two plans with the same
digest do the same thing.

## `--expect-plan <id>` (install, upgrade, remove, purge)

The plan that is about to run must start with the id (at least 8 hex digits). Anything else is
refused, nothing is changed, and the message says the plan is not the expected one. This is for
scripts and for a second look: preview with `--json`, read it, run with the id.

```
id=$(emrac upgrade --dry-run --json | jq -r .digest)
emrac upgrade --yes --expect-plan "$id"
```

## The re-check before the commit

If the plan was on screen for 30 seconds or more (a person was deciding, or an AUR package was
being reviewed), the plan is made **again**, with a new `Sources` (the first has read the databases
once and would answer the same), right before the journal is begun. If its digest differs the run
stops with what changed (`changed: a 1-1 → 1-2`, `added:`, `dropped:`, `will now also remove:`),
nothing is changed, and the next run shows the new plan. A run with `--yes` goes straight
through. It covers `install`, `upgrade`, `remove` and `purge`: the commands whose plan depends
on the databases or on what is installed. A rollback, a downgrade, a reinstall and a restore are
made from the history and the package files you hold, and cannot drift that way.

## The record

Every history record now carries `plan_digest` (the digest of the plan that ran). It is in the hash
chain and, with the ledger, on the root-owned copy: what was approved is on record. Older records
have none.

## Not done

- It does not make pacman install *exactly* the previewed versions (that needs the resolved package
  files to be installed with `pacman -U`); it makes sure the plan has not changed by the time of the
  commit. pacman still syncs on its own, so a change in the seconds after the re-check is not seen.
- It is not the privileged helper of C8; that helper would take a plan id and a saved plan.

## Verification

Unit tests: the digest ignores order, sizes and warnings and sees everything else; the differences
between two plans are listed; `--expect-plan` matches by prefix and refuses non-ids; a plan shown
a moment ago is not made again and an old one is, and a drifted one is refused with the difference.
Container (`dev/e2e/run.sh preflight`, 6 more checks): ids and `digest`, `--expect-plan` runs the
right plan and refuses an old id after the repository changed, the record carries the digest, and
a plan whose repository changed while it waited 36 seconds for an answer is refused at the commit
with `changed: fixnew 2.0-1 → 3.0-1`. A first version of the re-check passed its unit tests and
failed this run: it made the plan again with the same `Sources`, which had already read the databases.

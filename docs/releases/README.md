# releases/

One document per release, named `r<N>.md`. Each is the full release notes — what is new, what changes for someone updating, requirements, how the release was checked, known limits — and is also the message of the annotated tag, so it travels with the tag:

```sh
git tag -a r2 -F docs/releases/r2.md --cleanup=verbatim
```

- [`r2.md`](./r2.md) — release r2 (2026-10-14): r1 plus the update slices u1 to u30.

The day-to-day record is the root [`CHANGELOG.md`](../../CHANGELOG.md); a release document summarizes a release from it.

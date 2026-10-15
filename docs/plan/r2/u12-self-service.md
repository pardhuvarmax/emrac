# Slices u12–u15 — emrac manages its own installation

Builds the rest of `emrac self` from `docs/feats/self-service-feat.md` (Emrac-SMS), after
`self check` (u11). Four slices, one commit group each; the plan is written once so the pieces
agree:

| Slice | Adds |
|---|---|
| u12 | the release index, `self sync`, `self news`, the `self.*` config keys |
| u13 | `self diagnose` on the same engine as `check`; the self-operation journal |
| u14 | `self upgrade` / `self up` |
| u15 | `self downgrade` / `self dn` |

(The dependency commands `tree`/`graph`/`impact`/`cycles` move to u16.)

## The trust problem, and the answer

emrac has no release channel, and a self-updating program is only as safe as what it believes
about the next release. The design:

- **One index, one place.** The maintainer publishes `releases.json` (schema 1) next to the
  project page, `https://pardhuvarmax.github.io/emrac/releases.json`. `self.source` can point
  elsewhere (another `https://` address, or a `file://` path for a mirror or a test).
- **What an entry carries:** tag, version (`x.y.z`), date, channel (`stable`/`prerelease`),
  `state_schema` (the version of the files emrac keeps; see u15), a summary, a page URL, the
  tarball URL and the tarball's **SHA-256**. The index also carries the news items.
- **Authenticity.** HTTPS and the host are the baseline. With `self.require_signature = true`
  and `self.signing_key = <fingerprint>` emrac also fetches `releases.json.asc` and checks it
  with `gpg --verify` against that fingerprint; no valid signature, no sync. Every plan says
  which of the two applied ("integrity from the address only" vs "signed by <fingerprint>"). The
  default is not to require a signature, because there is no key yet; `dev/release-index.sh`
  prints the `gpg` command to sign.
- **Validation before anything is kept:** schema known; every field present and well formed (tag
  characters, `x.y.z`, 64 hex digits, dates); the tarball URL is `https://` on `github.com`
  or on the source's own host; no duplicate tags; at most 2 MiB. Anything else rejects the whole
  index, and the active one is untouched.
- **Staged, then committed.** The new index is written beside the active one and renamed over it
  only after validation, keeping the previous as `releases.json.prev`.

## State (all under `~/.local/share/emrac/self/`)

`releases.json` (active index), `releases.json.asc` (its signature if one was checked),
`sync.json` (last attempt, last success, source, outcome, signature status),
`journal.json` (u13), `backup-<time>/` (u14, u15).

## u12 — `self sync`, `self news`

- `emrac self sync [--dry-run]`: reconciliation, never an upgrade. Steps as in the spec: load
  config, read the active state, fetch, validate, verify the signature if required, stage,
  commit, record, report what changed. `--dry-run` prints the plan (source, whether the
  network is needed, what would be refreshed) and fetches nothing; it says that the result of
  the fetch can't be known until it is made. `--offline` refuses with a reason.
- `emrac self news [--all] [--json]`: from the cache only. Newest first (10, `--all` for all),
  each with date, category, version if any, summary and reference; items newer than the
  installed version are marked. Labels the age of the cache ("synced 3 days ago", "stale: more
  than 7 days", "never synced") and says nothing is changed.
- Config: `self.source`, `self.channel` (`stable`|`prerelease`), `self.require_signature`,
  `self.signing_key`.
- `self check` gains the release-metadata row (never synced / fresh / stale / invalid) and a
  real "Release source" row.
- `dev/release-index.sh <tag> [channel]` adds an entry (downloads the tag's tarball, computes
  the SHA-256, reads the version, the date and the state schema) to `releases.json` in the
  repository; the initial index lists `r1`, `r2` and `r2-aur`.

## u13 — `self diagnose`

The deeper report on the same engine as `check` (`check` is the fast subset): adds the release
index's detail (schema, age, signature status, entries, latest eligible), the upgrade
prerequisites (`makepkg`, `cargo`, a C compiler, `sudo`, `pacman`, `gpg` when a signature is
required), free space for staging, the self journal and stale lock files, the permissions of the
`self/` folder, and `--online` to test that the source answers. Explains every inconclusive
result. Redacts the home directory in anything it prints for sharing (`--report` style).
Also the self-operation journal (a small lock-file journal, the same idea as u10, for
`upgrade` and `downgrade`), shown by `check` and `diagnose` when an operation was cut off.

## u14 — `self upgrade` / `self up`

Discover → Plan → Validate → Prepare → Stage → Activate → Verify → Commit, as in the spec:

- The installation method decides who does it. **Owned by a pacman package:** emrac builds a
  package from the verified tarball with `makepkg` (the same recipe as `install.sh` and the AUR
  package, embedded in the binary) in a temporary folder and installs it with `sudo pacman -U`.
  The package manager stays authoritative; the file is replaced by pacman. **Any other
  installation** (cargo, by hand, development, unknown): refused, with what to do instead. emrac
  never overwrites a file a package owns.
- Plan: installed and target version, method, source, the checks that will be made, the
  state-schema compatibility, the backup, disk space, tools, whether another self operation is
  running. Already latest: says so and does nothing. `--dry-run` fetches nothing beyond the
  cached index and states that.
- Prepare: the lock and the journal; a backup of `config.toml`, the history and the pins in
  `backup-<time>/`.
- Stage: download the tarball, check the SHA-256 against the index, unpack, check the top folder
  and that the version in `Cargo.toml` is the one the index promised.
- Activate: `makepkg`, then `sudo pacman -U` (asks first; `--yes` does not skip pacman's own
  prompts unless it was given).
- Verify: the new `/usr/bin/emrac --version` shows the target and `emrac self check` on it exits
  0 or 1 with no new problem; if not, says so and offers `self downgrade`.
- Commit: journal removed, history records nothing (it is not a package transaction of the
  user's system); a line goes to the self log.

## u15 — `self downgrade` / `self dn`

`--version x.y.z` (or `--tag`) names the target; without it, only the previous release in the
index if there is exactly one older eligible release; otherwise it lists the choices and stops.
Preflight as in the spec: the target is in the index and older, it is available, its SHA-256 is
checked, the installation method supports it, and **state compatibility**: the release's
`state_schema` must be at least the schema of the files on disk; a newer schema refuses, naming
the blocker and the backup to restore. The state is never migrated backwards or deleted; the
backup made at upgrade time is where it can be restored from by hand, and the plan says which
config keys the older version may not know (the ones in the file that the target's schema
predates). The activation is the same as upgrade.

## Not covered (said in the docs)

- No automatic or background updating; every action is a command the user runs.
- No mirror fallback for the index; the one `self.source`.
- Rollback of the executable is by `self downgrade` or `pacman -U` of the old package file;
  pacman remains the authority for the files.
- A signature is verified only when required by config; the key must be in the user's gpg
  keyring.

## Verification

Unit tests for the index parser and validator (good, each bad field, wrong schema, oversize,
duplicate tag, bad host), version ordering and the "latest eligible" choice for each channel,
the news formatting and staleness labels, the gpg status parser, the plan text for each
installation method, the state-schema rule. CLI tests with a `file://` source in a scratch
home (sync, news, offline, dry-run, a bad index leaves the active one). Container: a full
`self upgrade` and `self downgrade` against a local index whose tarball is this repository's
`git archive`, ending with the pacman database showing the new version.

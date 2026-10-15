# The release index

How emrac learns about its own releases (`emrac self sync`, `emrac self news`), for people who use
emrac and for the person who publishes it.

**What it is.** One JSON file, `releases.json`, at the root of the project page:
`https://pardhuvarmax.github.io/emrac/releases.json`. `self.source` (a setting) can point at another
`https://` address or at a `file://` path, for a mirror or a test.

```json
{
  "schema": 1,
  "generated": "2026-10-15",
  "releases": [
    { "tag": "r2", "version": "0.2.0", "date": "2026-10-14", "channel": "stable",
      "state_schema": 1, "summary": "…", "url": "https://github.com/…/releases/tag/r2",
      "tarball": "https://github.com/pardhuvarmax/emrac/archive/refs/tags/r2.tar.gz",
      "sha256": "…64 hex digits…" }
  ],
  "news": [
    { "date": "2026-10-14", "category": "release", "title": "Emrac r2",
      "summary": "…", "version": "0.2.0", "url": "https://…" }
  ]
}
```

**What emrac checks before it keeps a copy.** The schema is known; every field is present and
well formed (tag characters, `x.y.z` versions, dates, 64 hex digits for the SHA-256, a known
channel and news category); no tag is listed twice; tarballs and links are `https://` addresses on
github.com or on the index's own host; no text has control characters (which could write to your
terminal); the file is under 2 MiB. Any failure rejects the whole index, says why, and leaves the
copy you already have alone. A new copy is written next to the old one and renamed over it only when
everything has checked; the previous one is kept as `releases.json.prev`.

**Where it is kept.** `~/.local/share/emrac/self/`: `releases.json`, `sync.json` (when it was last
tried, when it last worked, why it failed), and `releases.json.asc` when a signature was checked.

**Authenticity.** By default the guarantee is the address (HTTPS to the project page). To go
further, sign the index and tell emrac whom to trust:

    gpg --armor --detach-sign --local-user <fingerprint> releases.json     # publisher
    emrac config set self.signing_key <fingerprint>                         # user
    emrac config set self.require_signature true

`self sync` then also fetches `releases.json.asc` and accepts the index only if `gpg` reports a good
signature by that key (the key itself or its signing subkey); a bad, missing, expired or foreign
signature rejects it. The key has to be in your gpg keyring.

**Channels.** `self.channel = stable` (the default) sees only stable releases; `prerelease` sees
both. Releases are ordered by version, then date, then tag; a release is "newer" only if its
version number is higher than the running one.

**Publishing.** After tagging and pushing: `dev/release-index.sh <tag> [channel] "summary"` (it
downloads the tarball and computes the checksum), optionally `--news …`, then commit
`releases.json` (and `releases.json.asc` if signing). The project page serves it.

**What it does not do.** It does not update anything: `self sync` and `self news` only keep and
show information. The cache is labelled stale after 7 days. `news` never contacts the network.

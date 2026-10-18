# Slice u20 (after r3) — more evidence for `emrac score`: reproducibility, provenance, compatibility, packaging quality, upstream data

**Status: built, all of it in this one slice (the split into u20 and u21 was dropped; see the end).**

Checked on 2026-10-19 before planning (so nothing below rests on a guess):

| Source | What exists | Verdict |
|---|---|---|
| reproducible.archlinux.org (rebuilderd) | `GET /api/v0/pkgs/list?name=<pkg>` returns JSON: `status` (`GOOD`, `BAD`, `UNKWN`), `version`, `suite` (repository), `built_at`, `has_attestation`, `build_id`. No key. | Usable. Authoritative for official packages. |
| OpenSSF Scorecard | `api.securityscorecards.dev/projects/github.com/<owner>/<repo>`; needs the upstream repository | Usable only when the upstream is on GitHub, found from the package's `url` or its sources. Heuristic. |
| OSV | `POST api.osv.dev/v1/query`; no Arch ecosystem. Works by upstream git tag (`ecosystem: GIT`, repo URL, tag) or commit | Usable only with a known upstream repo and tag. Corroborating, never authoritative for Arch. |
| Arch package pages / GitLab | package repository history (`gitlab.archlinux.org/archlinux/packaging/packages/<pkg>`) | Usable for packaging quality: PKGBUILD commit cadence, how long a new upstream took to appear, number of maintainers. |
| Third-party repositories | any `[repo]` in `pacman.conf` that is not core/extra/multilib: a signing key, who owns the key, sizes | Local facts only; no outside service. |

## What it would add (each is a dimension or a gate, with its evidence tier)

1. **Reproducibility** (authoritative): GOOD scores 100, BAD 20 and is shown, UNKWN is unknown (never zero). A package built from a different version than the installed one says so. AUR and third-party: not covered, and it says so.
2. **Provenance** (authoritative/corroborating): where the package comes from (core/extra, AUR, a named third-party repository), signed by an Arch packager or not, the signing key's age when known, an attestation present (`has_attestation`), and for the AUR the maintainer's history (already in the trust signals).
3. **Compatibility**: from local facts only: architecture, whether the installed set satisfies its dependencies at the repository's version, `provides`/`conflicts` with installed packages, a soname that other installed packages need and the new version drops (the check `emrac` already makes before upgrades).
4. **Packaging quality** (heuristic): from the packaging repository's history: days since the last PKGBUILD change, the gap between an upstream release and the Arch rebuild, number of open packaging issues, a split into more than one maintainer. Fetched only with `--online`, cached.
5. **Upstream health** (heuristic, `--online`): OpenSSF Scorecard (overall and a few named checks) and OSV entries for the packaged upstream version, only when the upstream repository can be named from the PKGBUILD `url`. It says "upstream not identified" otherwise.
6. **Third-party repositories**: `emrac score` of a package from such a repository shows it as third-party with its facts (key, how many packages, last database change) instead of pretending it is official; a gate when a repository has `SigLevel = Never`.

## Rules that stay

- An unknown stays unknown; a source that is down is "not checked", never a penalty.
- Heuristic sources can lower the number but never open a gate; only authoritative evidence (the tracker, a bad reproducibility result on the very installed version) can.
- Everything is cached, `--offline` uses the cache, https only, the answers are size-limited and parsed strictly like the tracker's.
- No new dependency unless unavoidable (the tracker code already fetches JSON).

## Decisions (made)

1. **One slice** for all of it.
2. A `BAD` reproducibility result **only lowers the dimension** (20); it opens no gate. The result counts only for the exact installed version (the rebuilders' answer for another version is shown as unknown, never as a pass or a failure).
3. **Weights**: Reproducibility, Provenance, Compatibility, Security, Build time, Performance and Source count 1 by default. The proxy dimensions, Maintenance, Packaging quality, Upstream health and Popularity, count **0** by default: they are collected and shown (and named in "left out") but only count when you give them a weight under `[score]` or with `--weights`. Mandatory checks (the tracker gate, a repository with `SigLevel = Never`) stay outside the weights.

## What was built

- **Reproducibility** (authoritative): `GET reproducible.archlinux.org/api/v0/pkgs/list?name=<pkg>`; GOOD 100, BAD 20, undecided or another version unknown. Official Arch repositories only.
- **Provenance**: an Arch repository 70, +15 for the signature in the database, +15 for a published attestation of this version, -20 with no signature; a third-party repository 30, +20 when pacman.conf requires its signature, -25 when it asks for none (and that opens a **gate**); the AUR 30, -15 orphaned, -20 per high trust signal (at most -40), -5 per warning (at most -15).
- **Compatibility** (authoritative, from libalpm and the databases): the package's architecture against this machine's, dependencies that nothing installed or in the databases satisfies (-40 each, at most -80), installed packages it conflicts with and does not replace (-30 each, at most -60); new dependencies are a fact with no points. An installed AUR build is checked the same way from the local database. Not covered: soname-level breakage (it would need file lists of every package).
- **Packaging quality** (heuristic): the PKGBUILD history of the package base on gitlab.archlinux.org: days since the last change (up to 90 days 100, a year 85, two years 65, older 45), -10 for no change in a year, -10 when every recent change is by one person. Stable packages can be old, which is why it is a proxy.
- **Upstream health** (heuristic): when the package's URL names a GitHub repository, its OpenSSF Scorecard (ten times the score, weak checks named) and the OSV entries for the upstream version (`ecosystem: GIT`, -5 each, at most -30; Arch may have patched them). Otherwise "upstream not identified".
- **Third-party repositories**: shown as such, with their facts, instead of as official; their Security summary no longer says "signed by an Arch packager".
- All sources are fetched unless `--offline`, over https only (an address can be replaced by `EMRAC_REPRO_URL`, `EMRAC_PACKAGING_URL`, `EMRAC_SCORECARD_URL`, `EMRAC_OSV_URL`, as `file://` for tests), limited to 4 MiB, parsed strictly, cached in `~/.local/share/emrac/evidence/` (6 hours for the rebuilders, 7 days for the rest), and used from the cache when offline or when the source is down, with a note. A source that cannot be reached leaves its dimension unknown with the reason; it is never a penalty.
- The overall line now prints "left out" wrapped, and the plain card aligns the longer titles.

## Verification

15 unit tests for the dimensions (`score_more.rs`), 12 for fetching and parsing (`score_sources.rs`), the existing score tests moved to the six original weights where they test arithmetic; 69 container checks in `dev/e2e/score.sh` with fixture files for each source, plus real runs against the live services (a real BAD result for a current `neovim` build was seen).

# Slice u20 (after r3) — more evidence for `emrac score`: reproducibility, provenance, compatibility, packaging quality, upstream data

**Status: plan, waiting for a go.**

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

## Order (two slices)

- **u20**: 1, 2, 3 and 6 (all from the rebuilderd API and local data), tests, container checks (a fixture server by `file://` like the tracker).
- **u21**: 4 and 5 (Scorecard, OSV, packaging history), because they need the upstream-identification step and cost network calls.

## Decisions I need

1. Split as above (u20 = 1, 2, 3, 6; u21 = 4, 5), or all in one slice?
2. Should a BAD reproducibility result for the installed version be a **gate** (shown apart from the number, like a confirmed High vulnerability), or only lower the Reproducibility dimension? I recommend the dimension only: unreproducible is common, and not by itself a danger.
3. Should the overall number include the new dimensions by default (weights 1) or start them at weight 0 until you opt in with `[score]`? I recommend weight 1 for reproducibility and provenance, 0 for the heuristic ones.

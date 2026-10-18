# Slice u12 (after r3) — `emrac export` and `emrac import`

`SPEC.md` Part V, export / import. A way to describe this machine in one small file and to bring another machine
(or a fresh install) towards it, with a report first and a decision for every part.

```
emrac export > workstation.toml            # packages, a safe subset of the config, pins, repository names
emrac export --packages -o pkgs.toml       # one part; -o refuses to overwrite a file
emrac import workstation.toml              # the report, then each part asks
emrac import workstation.toml --dry-run    # the report only
```

## The file

TOML, `format = 1`, written to be read: `[meta]` (emrac version, UTC date; no host name, no user name), `[[package]]`
(name, version, `explicit`, `foreign`), `[[repo]]` (repository names only, no server addresses), `[config]` (dotted keys with
string values) and `[[pin]]` (name, reason, until).

## What import does, part by part

| Part | What happens |
|---|---|
| **Packages** | Explicit packages that are not installed here are installed through the **normal install path** (`emrac install`: the plan, pre-flight, the AUR audit and the review of a PKGBUILD for foreign ones; it asks as usual). Dependencies come along on their own and are not listed. Versions in the file are information: a package installed at another version is reported, **never downgraded or upgraded**. |
| **Config** | Only a safe subset is exported and only that subset is ever applied: `search.*`, `snapshot.before_upgrade`, `build.*`, `cache.*`, `review.inspect_official`. Each change is shown as `key: now -> new` and asked about (default no). |
| **Pins** | A hold keeps a package from getting updates, security fixes included, so it asks (default no; **`--yes` does not answer it**). |
| **Repositories** | Never applied. Repositories are system configuration that decides which software is trusted; names missing here are only reported ("add them to `pacman.conf` yourself"). |

## Why the rest of the config is not importable

An exchange file may come from someone else. These keys are never exported and are **refused on import**, even if a file
contains them, and the report lists them as "not applied":

- `self.*`: the self-upgrade source, channel and signing key. Applying them would point emrac's own updates at someone
  else's server;
- `safety.*`, `review.skip_pkgbuild`, `review.skip_diff`, `flags.*`: they switch emrac's protections off or answer its
  questions for you.

Package names, versions, repository names, pin names and config keys are validated (a name that starts with `-`, or has
a character outside what pacman allows, rejects the file); a file over 5 MiB or with over 20 000 packages is refused.

## Not in this slice

`bundle` (an offline bundle of verified packages and databases for an air-gapped machine) is a separate, larger feature and is
not built here. Profiles, collections and the `repos` section of the spec beyond names do not exist yet.

## Design

`emrac-core/src/exchange.rs`: `Exchange`, `Scope`, `build`, `render`, `parse` (validation), `compare` ->
`ImportReport`, `apply_config`. Pure and tested. `Sources::export` and `Sources::import_report` gather the inputs
(libalpm for installed packages and repository names, the config, the pins). `commands/exchange.rs` prints and asks.

## Verification

Unit tests: the round trip (build, render, parse), every validation refusal, the safe-config list in both directions,
`self.*` and `safety.*` in a hostile file, the comparison (to install, already installed, version differs, dependencies not
listed), pins, repositories. A container check in `dev/e2e/exchange.sh`: export on one state, remove a package, import and
see it installed, a hostile file refused or ignored, `--yes` not answering the pin question.

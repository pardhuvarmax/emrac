# Slice u19 — sandboxed AUR builds (bubblewrap)

Implements §3.2–3.3 of `docs/design/source-build.md` with the owner's decisions:
host `makedepends` visible read-only is acceptable, `bwrap` is an optional
external dependency, a package that needs the network gets a **warn-and-ask**.

## What `makepkg` actually does (read from `/usr/bin/makepkg` 7.1, then proved by hand)

| Step | Needs | Flag |
|---|---|---|
| resolve + install dependencies | `sudo pacman`, network | `-s` |
| download + verify sources | network | `--verifysource` (with `-s` it also resolves deps, then exits) |
| extract, `prepare()`, `pkgver()` | nothing | `--nobuild` (`-C` removes a stale `src/`) |
| `build()`, `check()`, `package()`, archive | nothing | `--noextract` |

`--nodeps` skips the dependency step. So only the first two steps need privileges
or the network; the other two can run in a sandbox.

## Design

- **Three invocations** instead of one `makepkg -s`:
  1. host: `makepkg -s --verifysource --noconfirm --needed` — dependencies and
     verified sources, exactly the part that must reach the network and `sudo`;
  2. sandbox: `makepkg --nobuild --nodeps --noconfirm -C` — extract, `prepare()`;
  3. sandbox: `makepkg --noextract --nodeps --noconfirm` — build, check, package.
  Exit 13 ("already built") is still accepted; a package that is already built
  skips all three.
- **The sandbox** (`sandbox.rs`; arguments built by a pure, tested function):
  `bwrap --unshare-all --die-with-parent --new-session --ro-bind / /`, fresh
  `/dev` and `/proc`, `tmpfs` over `/tmp`, `/home`, `$HOME` and `/run/user`, and
  a writable bind of only the build directory plus any `SRCDEST`/`PKGDEST`/
  `BUILDDIR`/`LOGDEST` from the user's makepkg config. The environment is
  cleared and rebuilt from a whitelist (`PATH`, `HOME`, `USER`, `LANG`/`LC_*`,
  `TERM`, `TZ`, `MAKEFLAGS`, `SOURCE_DATE_EPOCH`, …) so tokens and agent
  sockets don't leak. Same uid, so `makepkg`'s refusal to run as root and
  `fakeroot` both work.
- **Network**: denied in steps 2 and 3. If the audit predicted network use in
  `prepare()`/`build()`/`check()`/`package()` (the existing `network-in-*`
  findings), emrac warns and asks, default **no**, not answered by `--yes`;
  `--allow-network` answers yes. If an unpredicted build fails with the network
  denied, the error says so and names `--allow-network`.
- **Availability**: `bwrap` is probed by actually running it (`--unshare-all
  --ro-bind / / true`), so a disabled user-namespace setup is caught too. If it
  isn't usable emrac refuses to build and says why and how to fix it;
  **`--no-sandbox`** (a flag, never a config key, so it can't be set silently)
  builds as before with a loud warning.
- **`emrac build <target>...`** (SPEC command): builds an AUR package (same
  review and audit as `install`) or a local PKGBUILD directory, prints the
  manifest and the package file paths, and installs nothing. It also makes the
  sandbox testable without touching the system's packages.
- `install`, `upgrade` and the AUR rebuild in `reinstall` use the sandbox by
  default; `--no-sandbox` and `--allow-network` are accepted by all of them.

## Not in this slice

Landlock as a second layer, cgroup limits, a throwaway root for build
dependencies, ccache. The host's installed packages remain visible read-only to
the build (owner decision, `source-build.md` §6).

## Files

`sandbox.rs` (new, core); `backend/aur_build.rs` (phases, `build_in`);
`sources.rs`; `error.rs`; CLI `build.rs`, `aur_install.rs`, `cli.rs`,
`output.rs`, `main.rs`.

# Design: the source-first build path

Status: **accepted** (owner answers recorded in §6); built slice by slice from u15. It settles the open
questions in `ideas.md` (sections 2–3 and 6) so the next slices (u15 onward) can
be small and unambiguous. `SPEC.md` stays the product contract; where this
document disagrees with `ideas.md`, this one wins once accepted.

## 1. Problem

Today an AUR install is `git clone` → user reads the PKGBUILD → `makepkg -si`.
That PKGBUILD is arbitrary bash. `makepkg` runs it as the user, with the
network, with full access to `$HOME` (SSH keys, browser profiles, tokens), and
`-i` then installs the result as root. The review step helps only as much as the
reader.

## 2. Threat model (what we do and don't claim)

| Stage | Runs as | Can a sandbox help? |
|---|---|---|
| Fetch sources, `prepare()` | user | Yes: limit filesystem, log what was fetched |
| `build()`, `check()`, `package()` | user | **Yes — the main target**: no network, no `$HOME`, no host writes |
| `.INSTALL` scriptlets and the installed files | **root, at `pacman -U`** | **No.** A sandbox around the build cannot constrain what the finished package does when installed |

So a sandboxed build removes the "steal my keys while compiling" class, and
nothing else does for the last row: that is covered by **review** (static
analysis, a manifest of what the package changes, §3.5), not isolation. The
documentation must say this plainly; "sandboxed" must never be read as "safe to
install".

## 3. Decisions

### 3.1 Static PKGBUILD analysis (no execution)

**Decision:** a small in-tree parser, conservative by construction. It
extracts the declarative fields (`pkgname`, `pkgver`, `source`, checksums,
`depends`, `makedepends`, `install`, `options`) and **refuses to guess** when
a value is dynamic (command substitution, `eval`): the report says "dynamic —
cannot be checked". Function bodies (`prepare/build/check/package`) are scanned
by rules, not interpreted: `curl|sh`, `base64 -d | sh`, `eval`, `sudo`, writes
outside `$srcdir`/`$pkgdir`, `chmod u+s`, reads of `~/.ssh`, `systemctl enable`,
`SKIP` checksums on non-VCS sources, non-HTTPS sources.

Exposed as `emrac audit <pkg|PKGBUILD>` (SPEC already lists `audit`) and shown
inside the existing AUR review. It produces **findings with severity and the
line**, never a pass/fail verdict. Rejected: a full bash AST (large, still
cannot answer dynamic questions), and executing the PKGBUILD "just to read
metadata" (the exact thing we are avoiding). `makepkg --printsrcinfo` also runs
the file, so it is not used for analysis of untrusted input.

### 3.2 Phases: fetch → build → package, with network only early

`makepkg` already separates source download/verify/extract from building.
Plan: (1) fetch and verify checksums/PGP with network; (2) `prepare()`; (3)
`build()`/`check()`/`package()` with **no network**.

Caveat found while designing: many real packages (Go, Rust, npm, pip) fetch
dependencies inside `prepare()` or even `build()`. A blanket "no network" breaks
them. Therefore: network is allowed in step 1–2 (filesystem still confined) and
denied in step 3, with a per-package, recorded override (`--allow-network`) that
the audit report flags as a finding. The exact `makepkg` flags for the split
(`--nobuild`, `--noextract`, `--noprepare`) are verified on the target version
in u16 before relying on them.

### 3.3 Sandbox mechanism: bubblewrap

Options considered:

| Option | Verdict |
|---|---|
| **bubblewrap (`bwrap`)** | **Chosen.** Small, audited, used by Flatpak; solves the namespace/mount/`/proc`/`/dev` plumbing that `ideas.md` section 6 worried about; works unprivileged where user namespaces are on (checked: this host) |
| Hand-rolled `unshare` + `mount` (+ OverlayFS) | Rejected for now: `unshare()` affects the calling process, mounts need care, and we would re-implement bwrap's bugs. Revisit only if bwrap cannot do something we need |
| Landlock alone | Good *additional* layer (path restrictions without namespaces), not a replacement: no network or PID isolation story on its own. Later slice |
| devtools clean chroot / systemd-nspawn | Strongest dependency isolation, but needs root and is heavy. Candidate for the maintainer mode in `ideas.md` §4, not the default |

Layout of the build step: `--unshare-all` (net on only for steps 1–2),
`--ro-bind / /`, a bind-mounted writable build directory, `--tmpfs` over `$HOME`
and `/tmp`, `--dev /dev`, `--proc /proc`, `--die-with-parent`, `--new-session`.
Only the build dir (and the cache dir, §3.6) are writable and visible.

**Build dependencies.** The sandbox does not install anything: `makedepends`
must already be on the host, as `makepkg -s` arranges today through the
existing `sudo pacman -S --asdeps` flow. This keeps the privileged part exactly
as it is, at the cost of the host's package set being visible read-only to the
build. An overlay-backed root (build deps installed into a throwaway layer) is
what would remove that cost; it is deferred, and listed as an open question.

**Availability.** `bwrap` is an optional dependency, detected at runtime.
If missing or user namespaces are disabled: say so and require explicit
consent (`--no-sandbox`) to build unsandboxed — never silently fall back.

### 3.4 Commit stays `pacman -U`

Keep SPEC's rule: the sandbox produces a normal `.pkg.tar.zst` in a build-owned
`PKGDEST`; emrac then installs it with `sudo pacman -U`. Rejected: extracting
the overlay into the real root through libalpm (pacman's database, hooks and
file-conflict logic stay authoritative; we do not duplicate them). This also
resolves `ideas.md` section 6 "commit step" and "snapshot timing" (a snapshot,
when used, is taken immediately before `-U`; emrac's package-set `snapshot`
already exists).

### 3.5 Install-time manifest (built in u17)

Before `pacman -U`, inspect the built package (not the PKGBUILD): list `.INSTALL`
scriptlet actions, setuid/setgid files, systemd units, `sysusers`/`tmpfiles`,
sudoers/cron files, pacman hooks, files outside the usual prefixes. Shown next to
the existing preview, severity-tagged, confirmation unchanged. This is the real
answer to the last row of §2.

### 3.6 Build cache

Key: hash of `PKGBUILD` + `.SRCINFO` + `makepkg.conf` build flags + the
checksums of sources + `pkgver`. Value: the built package file (and its log).
Lives under `~/.cache/emrac/builds/`; `reinstall`/`rollback` already look in
build dirs, so they benefit with no change. Dependency versions are *not* in the
key in the first version (stated limitation: a soname bump can need a rebuild).

### 3.7 Resource limits (later)

cgroup v2 limits (memory, CPU, PIDs, wall time) via `systemd-run --user
--scope` when available, applied to the sandboxed step. Not in the first
sandbox slice.

## 4. How `ideas.md` section 6 concerns resolve

| Concern | Resolution |
|---|---|
| `unshare()` changes the calling process | bwrap spawns the child; emrac never unshares itself |
| Shelling out to `mount` vs a crate | n/a: bwrap does the mounting |
| Overlay of `/` includes `/proc`,`/sys`,`/dev` | n/a for now (read-only bind + fresh `/dev`,`/proc`); revisit with the overlay question |
| tmpfs upper layer may not be RAM | n/a: build dir is a normal directory, size under the user's control |
| Snapshot nested in the snapshotted subvolume | out of scope here: emrac's snapshots are package-set records, not filesystem snapshots |
| Commit mechanism | §3.4 |
| Snapshot timing | §3.4: right before `-U` |

## 5. Slices

| Slice | Delivers | Risk |
|---|---|---|
| u15 | PKGBUILD parser + rules, `emrac audit`, findings in the AUR review | low (pure, unit-testable) |
| u16 | fetch/build split (`makepkg` phases), recorded network override | medium: verify `makepkg` flags and `prepare()` behaviour |
| u17 | bwrap sandbox for the build step, availability detection, `--no-sandbox` | medium: build-dir and cache binds, behaviour on real PKGBUILDs |
| u18 | install-time manifest before `pacman -U` | low–medium |
| u19 | build cache | low |
| later | Landlock layer, cgroup limits, reproducible-build check, overlay root, maintainer mode | — |

Testing: sandbox slices are tested on the host with tiny local PKGBUILDs built
into a temp dir (build only, no `-i`, so nothing system-wide changes), plus the
podman container for the install side. The container needs `--cap-add` or
rootless user namespaces to run bwrap; confirm in u17.

## 6. Decisions (owner, 2026-10-13)

1. **Host `makedepends` visible read-only is acceptable for the first sandbox.**
   The overlay root stays a later option.
2. **`bwrap` as an optional external dependency is acceptable.**
3. **Package whose `prepare()`/build needs the network: warn, then ask for
   permission** (never allow silently; `--yes` does not answer this prompt on
   its own — see the severity table).
4. **Audit findings are tiered; emrac does not decide for the user except at
   the top tier.**

   | Severity | Meaning | What emrac does |
   |---|---|---|
   | info | worth knowing (has an install scriptlet, dynamic field) | shown |
   | warning | suspicious or sloppy (http source, `SKIP` checksum, network in `build()`) | shown with the line; build proceeds after the normal review prompt |
   | high | dangerous but sometimes legitimate (pipe a download into a shell, `sudo`, setuid, `systemctl enable`, reading `~/.ssh`) | **warn and stop**: a separate explicit confirmation naming the findings; `--yes` does not bypass it, `--accept-risks` does |
   | critical | severe and able to spread or persist (writing shell startup files or `authorized_keys`, reading credentials *and* uploading, obfuscated decode-and-execute, reverse shells, destructive commands) | **blocked by emrac, no override flag**; the user can still inspect and build by hand outside emrac |

   Refinement (u16): shell startup files (`.bashrc`, `.profile`, …) are
   critical when a PKGBUILD writes them but **high** in an install scriptlet or
   shipped script, since some legitimate packages append a `source` line there;
   SSH key lists, `ld.so.preload`, sudoers and account files stay critical
   everywhere. Committed executables and added-by-patch findings are high and
   warning respectively (§ u16).

   The critical tier is deliberately narrow (almost no legitimate package does
   these) so that a block is rare and means something.

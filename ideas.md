# Emrac: Ideas

<img align="right" width="350" src="/assets/emrac.png" alt="emrac">

Running list of ideas beyond the core design in `emrac.md` (Layer 1 Shadow Tree, Layer 2 OverlayFS/namespace staging, Layer 3 Btrfs/ZFS snapshot, libalpm commit).

Status tags: `[idea]` not started, `[planned]` decided, `[building]` in progress, `[dropped]`.

---

## 1. Already queued from emrac.md

- `[idea]` **PKGBUILD AST parser**: verify/security-check a PKGBUILD before Phase 1 runs.
- `[idea]` **Config presets**: structure for automated configuration presets (optimization flags, build type, etc.). SPEC uses TOML for configuration and has build profiles and recipes, so presets should extend those rather than add a new format (see section 7).

---

## 2. Inspirations from other open-source projects

| Project | Language | What to borrow | Fit in Emrac |
|---|---|---|---|
| **Shelly** | Zig | Native PKGBUILD build pipeline + **Landlock** filesystem sandboxing | Alternative or extra hardening for Layer 2 (Landlock restricts what the build process can touch, even inside the overlay) |
| **Aura** | Haskell | Built-in source auditing, rollback tracking, explicit build cache management | Audit step before Phase 1, a rollback history view in the TUI, a managed build cache |
| **pacdef** | Rust | Declarative group files: list desired software in plain text, tool resolves the rest | Declarative "system profile" mode on top of the imperative TUI flow |
| **decman** | Python | Declarative state across pacman, AUR, Flatpak, and systemd units | Unified platform angle: one blueprint covering native, AUR, Flatpak, services |
| **ALPM project (Sovereign Tech Fund)** | Rust | Static, memory-safe PKGBUILD parsing without executing bash | Strong candidate for the AST parser: no need to run untrusted shell just to read metadata |
| **arch-repo-manager** | C++ | Local repo automation / build daemon | Optional local binary repo so verified builds can be reused instead of recompiled |
| **yay / paru** | Go / Rust | Keep libalpm as the source of truth, rewrite only the UX, parallel downloads | Confirms the Phase 4 approach: commit through libalpm so pacman's state stays consistent |

### Ideas that come out of this

- `[idea]` **Landlock + OverlayFS together** in the staging layer (defense in depth inside Layer 2).
- `[built u15]` **Static PKGBUILD analysis** (no bash execution) feeding a risk report before the user approves anything (`emrac audit`; TUI view still open).
- `[idea]` **Declarative profile file** (pacdef/decman style) that Emrac diffs against the installed system and turns into a Shadow Tree plan.
- `[idea]` **Rollback history** that ties each transaction ID to its snapshot and package set, browsable in the TUI.
- `[idea]` **Persistent build cache** keyed by PKGBUILD hash + flags, so identical builds are not repeated.
- `[idea]` **Optional local repo** of verified build outputs, reusable across machines on a network.
- `[idea]` **Multi-source backends** behind one interface: pacman repos, AUR, Flatpak, systemd unit management.

---

## 3. More ideas that strengthen Emrac

### Security and trust

- `[built u19]` **Split fetch and build phases**: sources are downloaded and checksum/PGP-verified first, then the build runs with **no network** inside the sandbox. Stops a malicious PKGBUILD from phoning home or pulling extra payloads mid-build.
- `[idea]` **Reproducible-build check**: build twice in separate sandboxes and compare hashes. A mismatch is flagged in the TUI as "non-deterministic build".
- `[idea]` **AUR trust signals**: surface maintainer changes, orphaned status, package age, votes, and recent PKGBUILD diffs in the preview, so risky updates stand out before Phase 1 completes.
- `[built u20]` **cgroups v2 limits** on builds (CPU, RAM, PIDs, build time) so a runaway or hostile build cannot take down the host.
- `[idea]` **Signed, tamper-evident transaction log**: each transaction records plan, hashes, and outcome in an append-only chained log for auditing.
- `[idea]` **SBOM / provenance output** per build (what source, which flags, which toolchain), exportable for compliance or personal records.

### Safety and recovery

- `[idea]` **Post-commit health check**: after commit, verify failed systemd units, broken shared-library links (`ldd`-style scan), and that the kernel/initramfs/bootloader entries are intact. Auto-rollback if checks fail.
- `[idea]` **Per-package rollback from cached packages**, in addition to full snapshot rollback. A full snapshot of `/` can revert unrelated changes made since, so a targeted rollback is often the better tool.
- `[idea]` **Bootable pre-transaction snapshots**: register the snapshot as a boot entry (grub-btrfs / systemd-boot style) so a broken system can boot back into the pre-tx state.
- `[idea]` **Partial-upgrade guard**: detect operations that would leave the system in an unsupported partial-upgrade state (soname bumps, split dependency sets) and block or warn in the Shadow Tree.
- `[idea]` **Snapshot backends behind one trait**: Btrfs, ZFS, and snapper/timeshift integration, plus a clear "no snapshot available" mode that warns instead of silently skipping Layer 3.

### Workflow and usability

- `[idea]` **Lockfile (`emrac.lock`)**: export a Shadow Tree plan with exact versions and hashes so the same transaction can be replayed on another machine.
- `[idea]` **Headless JSON API / machine-readable output** from the same engine the TUI uses, for scripting and CI.
- `[idea]` **`.pacnew` / config-merge handling in the TUI**: review and merge config changes as part of the commit step instead of leaving them for later.
- `[idea]` **Plan diff view**: show what changed between two plans (or between the current system and a profile) in the TUI.
- `[idea]` **Pacman hook compatibility**: run existing pacman hooks during commit so Emrac stays a good citizen on systems that depend on them.
- `[idea]` **Offline / air-gapped mode** using a local package and source cache.
- `[idea]` **Mirror ranking and parallel downloads** built into the engine.

### Platform and extensibility

- `[idea]` **Backend plugin interface** (pacman repos, AUR, Flatpak, others), possibly sandboxed via WASM, so new sources do not touch core transaction logic.
- `[idea]` **Preset sharing**: shareable optimization-flag presets (e.g. "low-latency desktop", "minimal server") with signatures, building on the config preset idea in section 1.

### Intelligence and automation

- `[idea]` **Smoke tests in the sandbox**: in Phase 3, also run the PKGBUILD's `check()` and a quick "does the binary start" test inside the overlay, so a package that compiles but is broken never reaches commit.
- `[idea]` **Binary vs source benchmarking**: build from source with the chosen flags, run a small benchmark against the binary version, and show in the TUI whether the source build was actually worth it.
- `[idea]` **Vulnerability-aware planning**: pull in Arch security advisory data so the Shadow Tree flags installed packages with known CVEs and highlights which pending upgrades fix them.
- `[idea]` **Safe unattended updates**: scheduled runs of Phases 1 to 3, commit only if the sandbox and health checks pass, roll back otherwise.
- `[idea]` **"Why" explanations**: a command or TUI view explaining why a package is being installed, upgraded, or kept (dependency chain, what requires it), including for a planned transaction before it runs.

### Arch-specific safety

- `[idea]` **Arch news gate**: before an upgrade, check the Arch Linux news feed and block or warn when an item requires manual intervention, surfaced inside the plan.
- `[idea]` **DKMS / kernel-module test**: if the transaction touches the kernel or DKMS packages, build the modules against the new kernel in the sandbox first, and check that the initramfs and bootloader entries generate correctly before commit.
- `[idea]` **Boot test for risky transactions**: for kernel, bootloader, or init changes, optionally boot the post-transaction result in a throwaway VM (QEMU or systemd-nspawn) to confirm it comes up. A stronger version of the post-commit health check.

### Install-time transparency

- `[built u17]` **Install-time capability manifest**: before Phase 4, show everything a package changes beyond files: `.install` scriptlets, new users and groups, setuid binaries, systemd units, sudoers or cron entries, and pacman hooks it adds. Risky items are flagged. Static analysis of scriptlets fits the AST parser work.
- `[idea]` **Config tracking tied to transactions**: git-backed history of `/etc` changes per transaction (etckeeper-style), so the effect of a given package change on config is visible and revertible on its own.

### Control and reproducibility

- `[idea]` **Time-travel installs**: resolve a plan "as of a date" using the Arch Linux Archive. Combined with `emrac.lock`, this reproduces or rolls back to a known-good package set without a filesystem snapshot.
- `[built u13]` **Holds with reasons and expiry**: pin a package with a note (e.g. "breaks my driver") and an expiry date, so holds don't silently outlive their purpose. The TUI reminds when one expires.
- `[idea]` **Cleanup simulation**: before removal, show what would be orphaned, how much disk is freed, and what depends on it, as a Shadow Tree plan. Also a bloat view of unused or oversized packages.

### Performance and supportability

- `[idea]` **Shared build acceleration**: integrate ccache or sccache, and optionally distcc or a LAN artifact cache, so source builds with optimization flags are not painfully slow. Pairs with the persistent build cache.
- `[idea]` **`emrac doctor`**: one command that checks for common problems (partial upgrades, broken links, stale snapshots, bad config permissions, expired holds) and produces a sanitized report suitable for a bug report.

---

## 4. Maintainer and developer workflow (publishing to AUR)

Emrac's Layer 2 sandbox already does most of what a maintainer needs (a clean-chroot build), so Emrac can also serve people who publish packages, not just people who install them.

### Pre-submit pipeline

- `[idea]` **`emrac maintain check`**: one command that runs the whole pre-push gauntlet:
  - static PKGBUILD lint (using the AST parser idea)
  - checksum update and automatic `.SRCINFO` regeneration, so the two never drift
  - clean-sandbox build plus `check()`
  - file-conflict check against official repos and other packages
  - install test and upgrade test (install the old version, then upgrade to the new one)
  - AUR guideline checks: naming conventions (`-git`, `-bin`), license field, duplicates of official packages
  - the push step runs only if everything passes

### Upstream tracking

- `[idea]` **Upstream watcher**: detect a new upstream release, bump `pkgver`, update checksums, test-build in the sandbox, and prepare a commit for the maintainer to approve. Never pushes on its own unless an explicit policy allows it.
- `[idea]` **Maintainer dashboard (TUI)**: out-of-date flags, failing builds, and packages needing a rebuild because a dependency's soname changed.

### Safe scripting and automation

- `[idea]` **Capability-based workflow files**: each pipeline step declares what it needs (network, paths, secrets) and Emrac enforces it with Landlock or the namespace. Build, check, and lint steps get no network and no secrets; only the push step sees credentials.
- `[idea]` **Restricted scripting layer** instead of raw bash for hooks: declarative TOML steps for common cases, with Lua or WASM when real logic is needed.
- `[idea]` **Dry-run for automation**: every workflow run is first shown as a Shadow Tree-style plan of what it will build, touch, and push.

### Credentials: each user brings their own SSH key

Emrac never ships, stores, or holds a key. Anyone publishing to AUR configures their own in the Emrac config file (TOML, per SPEC).

```toml
[aur]
user = "aur"

[aur.ssh]
identity_file = "~/.ssh/aur_ed25519"
use_agent = true
```

- `[idea]` **Reference, never embed**: config holds a path, not key material. Refuse to run the push step if the key file or the config is world-readable.
- `[idea]` **Prefer ssh-agent**: with `use_agent: true`, Emrac talks to the agent and never sees the passphrase or decrypted key. Emrac does not store or prompt for passphrases.
- `[idea]` **Only the push step touches the key**: the push step gets the key path or agent socket and only git and ssh; sandboxed build, check, and hook steps cannot reach `~/.ssh`.
- `[idea]` **Multiple identities**: profiles in the config (e.g. `profiles.personal`, `profiles.ci`), with each workflow choosing one.
- `[idea]` **Host key pinning**: require the AUR host key in `known_hosts` and refuse to push on a mismatch.
- `[idea]` **Confirm before push**: show the git diff, `.SRCINFO`, and target repo, and require explicit confirmation by default. Unattended pushes only if the config opts in.
- `[idea]` **Other secrets follow the same rule**: tokens (e.g. a GitHub API token for upstream watching) are referenced by env var or secret file path, readable only by steps that declare that capability.

### Two-sided trust and extras

- `[idea]` **Verified provenance indicator**: maintainers who publish signed or reproducible builds get a verified marker in the user-side TUI, linking the maintainer features to the user-trust signals in section 3.
- `[idea]` **CI template generation** (GitHub Actions or similar) from the same workflow files.
- `[idea]` **Local test repo**: install and try a package on your own machine before it goes to AUR.
- `[idea]` **Orphan adoption and flag/comment handling** integrated into the workflow.

### Open questions

- Should maintainer tooling be a mode of the same Emrac binary, or a separate companion tool on the same engine? One binary is simpler for users; a separate tool keeps the end-user package manager smaller and its attack surface lower.
- Push step: shell out to system `ssh` and `git` (respects the user's `~/.ssh/config` and agent setup) or use a library like libgit2 (tighter control over what the step can touch)?
- Should Emrac help generate the key (`emrac maintain setup` creating an ed25519 key and showing the public key to add to the AUR account), or leave key creation entirely to the user?
- Scope of "unified": Arch-only (pacman, AUR, Flatpak), or eventually other ecosystems such as cargo and pip?

---

## 5. Emrac's own tooling (CLI)

Command style: flat `emrac <verb>` commands. Maintainer commands stay grouped under `emrac maintain ...` (see section 4).

### Setup and configuration

- `[idea]` **`emrac setup`**: first-run wizard that detects what the machine supports and writes the config file (TOML) with safe permissions.
  - detects root filesystem (Btrfs, ZFS, or neither)
  - detects unprivileged user namespaces, OverlayFS, Landlock, and cgroups v2
  - reports which sandbox and snapshot layers will be active, with a clear warning when a layer is unavailable
  - optional maintainer setup (AUR user, SSH key path, agent); no key generation unless decided (see open questions in section 4)
  - `--non-interactive` for scripted installs
- `[idea]` **`emrac config`** (SPEC name; `conf` can be an alias): configuration management. SPEC already lists `show/edit/reset/validate/get/set/unset/export/import`; the items below are extras.
  - `get`, `set`, `edit`, `show --effective` (merged result), `diff` against defaults
  - `validate` / `lint` against a schema, including permission checks on the config and key paths
  - layered config: system (`/etc/emrac/`), user (`~/.config/emrac/`), per-project, with env var overrides
  - `migrate` for config version upgrades
  - `preset list/apply/export`, tying into the config presets idea

### Diagnostics and recovery

- `[idea]` **`emrac doctor`**: see section 3. Extend with `--fix` (generates a plan that goes through the normal Shadow Tree flow rather than patching directly) and `--report` (sanitized bug report).
- `[idea]` **`emrac self-test`**: run a tiny sandbox build and snapshot cycle to prove each layer works on this machine.
- `[idea]` **`emrac sandbox shell`**: drop into the overlay from a failed build to inspect it. Especially useful for maintainers.

### Everyday operations

- `[idea]` **`emrac plan` / `emrac apply`**: dry-run is first-class; a plan can be saved and applied later (connects to `emrac.lock`).
- `[idea]` **`emrac history` / `emrac rollback`**: browse the transaction log, see what each transaction changed, roll back a transaction or a single package.
- `[idea]` **`emrac snapshot`**: list and prune, with a retention policy (keep last N or last N days) so snapshots don't fill the disk.
- `[idea]` **`emrac cache`**: stats, gc, and clear for the package, source, and build caches.
- `[idea]` **Commands for earlier ideas**: SPEC already has `why`, `news`, `audit`, `pin`/`lock`, and `explain`. Holds with reasons and expiry extend `pin`/`lock` rather than add `hold`.
- `[idea]` **Declarative system profile** (apply or diff against the machine). Not `emrac profile`: SPEC uses `profile` for build profiles. Better fit is SPEC's collections and workspaces (see section 7).

### Housekeeping

- `[idea]` **Shell completions and man pages** generated from the command definitions.
- `[idea]` **`emrac self`**: version with detected features, update check, and clean uninstall that removes snapshots, caches, and config on request.
- `[idea]` **Global flags**: `--dry-run`, `--json`, `--yes`, `--profile`, `--verbose`, and stable, documented exit codes so scripts and CI can rely on them.
- `[idea]` **Structured logging**: human-readable by default, JSON on request, one log per transaction ID.

### Architecture note: privilege separation (undecided)

- `[idea]` Split Emrac into an **unprivileged front end** and a **small privileged helper**. Planning, sandbox builds, linting, and the TUI run as the user; only the commit step and snapshot creation need root. The helper (invoked via polkit or sudo) accepts only a verified, saved plan and executes it. This keeps the large, complex part of the program without root and shrinks the security-critical part, and fits the plan/apply model.
- Open question: privileged-helper split, or a single binary that elevates with sudo when needed?

---

## 6. Design concerns to revisit in emrac.md

Things I noticed while reading the current draft that are worth settling before building on it:

- **Namespace scope**: `unshare()` in `spawn_isolated_build` changes the calling process, not just the build. Safer to fork/spawn a child that unshares and does the mount and chroot itself.
- **Mounting**: shelling out to `mount` after `unshare` works, but the `nix` crate's `mount()` call avoids a dependency on an external binary and gives clearer errors.
- **Overlay lower dir `/`**: overlaying the whole root includes `/proc`, `/sys`, `/dev`, and other mounts, which need explicit handling inside the chroot.
- **tmpfs upper layer**: the code uses `/tmp/emrac/upper`, which is only RAM-backed if `/tmp` is tmpfs. Large builds may need a disk-backed option.
- **Snapshot location**: snapshotting `/` into `/var/lib/emrac/snapshots` nests the snapshot inside the subvolume being snapshotted; usually a separate snapshots subvolume is used. ZFS path is not implemented yet.
- **Commit step**: Phase 4 extracts sandbox output via libalpm (but SPEC Part X says mutating operations shell out to real `pacman`/`makepkg`, see section 7); the mechanism for turning overlay output into proper packages (so pacman's database stays accurate) needs defining.
- **Snapshot timing**: taking the snapshot before the sandbox test (Phase 2) means it may be stale if the system changes during a long build; consider taking it right before commit.

---

## 7. Alignment with SPEC.md

`SPEC.md` is the product spec (CLI/TUI, Rust, hybrid libalpm + pacman/makepkg, TOML config, first milestone = core CLI loop). This section records how the ideas above relate to it.

### Conflicts to resolve

| Topic | `emrac.md` / this doc | `SPEC.md` | Suggested resolution |
|---|---|---|---|
| Commit mechanism | Native libalpm transactions extract sandbox output into the real root | Mutations shell out to real `pacman` and `makepkg`; libalpm is read-only (FFI) | Keep SPEC's rule. Sandbox produces a normal `.pkg.tar.zst`, then `pacman -U` commits it, so pacman's logic stays authoritative |
| Config format | JSON/YAML mentioned | TOML | Use TOML everywhere (updated above) |
| `emrac profile` | Declarative system profile | Build profiles (compiler, LTO, flags) | Keep `profile` for build profiles; use collections/workspaces for declarative system state |
| `emrac conf` | New command | `emrac config` already exists | Use `config`; `conf` only as alias |
| Holds | `emrac hold` with reason and expiry | `pin`/`unpin`, `lock`/`unlock` | Add reason and expiry fields to `pin` |
| Sandbox | OverlayFS + user namespace (Layer 2) | "Build sandboxing" listed; Landlock not specified | Layer 2 design stays the implementation; spec only says sandboxed builds |
| Scope | Everything above | Part IV features land after the core CLI loop | Treat this doc as post-milestone unless marked otherwise |

### Ideas already covered by SPEC (reference the spec, don't duplicate)

- Install plan preview (Layer 1 Shadow Tree) = **Install Planner**, Part IV #1.
- "Why" explanations = `why`, `explain`, Part IV #2.
- Arch news gate = News & Advisory System (ABI transitions, manual intervention notices), Part III §9.
- Binary vs source benchmarking = `benchmark`, **Build Diff**, **Build Benchmark**, Part IV #6-7.
- Cleanup simulation = **Package Impact Preview**, Part IV; `impact`, `orphans`.
- Shared build acceleration (distcc) = **Compile Farm Support** and Build Artifact Library.
- Persistent build cache = Cache Management, **Build Cache Explorer**.
- `emrac doctor` = **Health Center**, Part IV #16 (spec lists partial upgrades, GPG keys, permissions, etc.).
- Snapshots and rollback = `snapshot`, `rollback`, `restore`, Part IV #10 (optional Btrfs/ZFS integration).
- `emrac sandbox`, `emrac dev` = Development family.
- Plan and apply = Transaction Lifecycle (Planned, Previewed, Confirmed, Executing, Committed/Failed/RolledBack).
- Lockfile, presets = build **Recipes** and **Package Collections** (partly); a version-pinned replayable lockfile is still new.
- Plugin/backend interface = Extensibility (future), `plugin` commands.
- Reproducible builds = listed under Extensibility (future).
- Smart unattended updates = **Smart Updates** (rule-based) plus `schedule`/`cron`; the sandbox-gated auto-commit and auto-rollback part is new.
- Mirror ranking and parallel downloads = Mirror Management and Mirror Intelligence.
- Shell completions, man pages, global flags, JSON output = Part V/VII (`completion`, `man`, `--json`).

### Genuinely new relative to SPEC (what this doc adds)

- Security: no-network build phase, fetch/build split, AUR trust signals, cgroup limits, signed transaction log, SBOM/provenance, install-time capability manifest, vulnerability-aware planning beyond `cves`.
- Safety: post-commit health check with auto-rollback, per-package rollback from cached packages, bootable pre-transaction snapshots, partial-upgrade guard in the plan, DKMS/kernel-module test, VM boot test, `/etc` change tracking per transaction.
- Reproducibility: time-travel installs from the Arch Linux Archive, replayable lockfile.
- Maintainer workflow: entire section 4 (pre-submit pipeline, upstream watcher, capability-based workflows, BYO SSH key, CI templates). SPEC has no AUR-publishing side.
- Tooling: `self-test`, `sandbox shell`, privilege separation (unprivileged front end plus privileged helper), per-transaction structured logs.

### Ideas from SPEC worth borrowing into the sandbox/transaction design

- **Resumable steps**: SPEC's transaction and build queue state machines support `queue resume`; the sandbox layers should record step state so a failed Phase 3 can resume.
- **Policy Engine** (e.g. "never compile packages larger than 500 MB"): good home for sandbox limits and the no-network rule.
- **First milestone discipline**: SPEC's testing note (exercise real installs in a disposable container or pacstrap chroot) matches the sandbox work and gives a safe way to develop Layers 2 and 3.

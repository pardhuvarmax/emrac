# Plugins: WebAssembly extensions with enforcement and security checks

Feature document for Emrac. Status: `[idea]`, in the spec as "Extensibility (future)" (`SPEC.md` III.21, Part V Plugins). Nothing here is built, and no plan exists yet. This page is a proposal to settle the open choices before one is written.

Decisions already taken (they bound everything below):

- **Plugins stay in.** They are the way to add things to emrac without changing its core.
- **Compiled WebAssembly modules, never scripts.** Emrac has no scripting of any kind (no engine, no interpreter, no scripting layer), so a plugin is a compiled `.wasm` file.
- **WASM enforcement and security checks are part of the feature**, not an afterthought: deny-by-default host calls, capabilities that are declared and granted, resource limits, a log of every host call, and verification before install.
- **Deterministic and local**, like the rest of emrac: no AI, no telemetry, no cloud lookups.

---

## 1. What plugins are for

What the documents say today is thin: `plugin search formatter` and `plugin install pkgbuild-lint` as examples (`SPEC.md`), "new package sources (Flatpak, others) without touching core transaction logic" (`ideas.md`), and an open question whether Wandrover should be an optional module (`cross-distro-feat.md`). This page turns that into extension points, smallest first.

| # | Extension point | A plugin supplies | Risk | Phase |
|---|---|---|---|---|
| E1 | **Audit rules** | Extra checks over a PKGBUILD, its `.install` scriptlet and patches; returns findings | Low: read-only, can only add findings | 1 |
| E2 | **Health checks** | Extra rows for `doctor`, from a read-only snapshot of facts | Low | 2 |
| E3 | **Plan notes** | Extra notes or warnings shown under a plan, from the plan's JSON | Low | 2 |
| E4 | **Formatters** | A new output format for an existing JSON report | Low | 2 |
| E5 | **Package sources** | A backend such as Flatpak: search, info, a plan, a way to fetch | High: touches transactions, needs external tools | later, separate design |
| E6 | **Event observers** | Receives structured events (`plan.created`, `preflight.completed`, `transaction.committed`, `build.failed`, `recovery.required`) and reports them somewhere: a desktop notification, a webhook, a log | Medium: outbound data, so a granted `net.post` host and minimal payloads | 2 |
| E7 | **Policy gates** | A rule that can **veto** a transaction under a policy you configured (a license list, a team rule) | Medium: availability, so opt-in per plugin and never on recovery | 2 |
| E8 | **Read-only commands** | A namespaced command (`emrac <plugin> inspect`) that prints a report from the data it was granted | Low | 3 |
| E9 | **Plan proposals** | A list of operations that emrac turns into a normal plan with the usual preview and confirmation (a fleet policy, a local repository) | Medium: it only proposes | later |

Not extension points, ever: anything that changes the system, runs as root, adds privileges, hooks into the root helpers (`policy-helper`, `ledger-append`), or lowers a built-in finding.

## 2. The security model

The one property everything else serves: **a plugin can add information but cannot remove or weaken any built-in check, and cannot act on the system.**

1. **Unprivileged only.** Plugins run as the user. No plugin is ever loaded by a root helper or a pacman hook, and there are no system-wide plugins.
2. **Out of process.** The runtime lives in a child process (`emrac plugin-host`) started inside the same bubblewrap sandbox emrac already uses for builds: no network, an empty home, a read-only root. A bug in the runtime is then contained by the sandbox, and a hung plugin is killed from outside.
3. **Deny by default.** A module gets no files, network, clock, randomness, environment or processes. It receives one input and returns one output. Anything else is a **host call** that exists only if the manifest declares it and the user granted it.
4. **Add-only.** Plugin findings are merged into the report and labelled with the plugin. A plugin cannot suppress or edit a built-in finding. Their severity is capped (default `high`; `critical`, which refuses a build, is reserved for built-in rules unless the user raises `plugins.max_severity`). The one extension that can stop something is a **gate** (E7), and a gate can only make a transaction *more* restricted: it can veto, but it can never approve what a built-in check stopped.
5. **Failure is not a verdict.** A plugin that traps, runs out of fuel, times out or breaks the output format is switched off for that run, with a note. It never blocks a transaction and never makes one pass. **One exception, by your configuration only:** a plugin you list under `plugins.gates` (E7) fails closed for the transaction kinds you named. A gate never applies to recovery (`rollback`, `history recover`, `self upgrade`, restoring the database backup), can be overridden for one run with a logged flag, and is never overridden by an unattended run.
6. **Everything is logged.** Every host call, limit hit and plugin finding is written to the transaction record.
7. **Deterministic.** The same input gives the same output: floating-point NaN bit patterns are canonicalised, threads and atomics are refused, the clock and randomness are not offered unless granted.

An honest limit: someone can compile an interpreter (Lua, say) into a WASM module. Emrac's "no scripting" is about emrac's own features; it does not stop a plugin author doing this, but such a module is held to the same capabilities and limits as any other.

## 3. Capabilities

| Capability | Default | What it permits |
|---|---|---|
| input of the extension point | always | The data passed in (a PKGBUILD, a facts snapshot, a plan). Inherent to the point, not a grant |
| `emit.findings` / `emit.rows` / `emit.notes` | by extension point | Returning output of the kind the point accepts |
| `fs.read:<path>` | refused | Read a specific file or directory (for example `/etc/makepkg.conf`), through a host call, size-limited |
| `net.get:<host>` | refused | An HTTPS GET to one named host, through a host call, response size-limited, every request logged |
| `events:<kinds>` | refused | Receive the named event kinds (E6). The payload is built by emrac and holds only what the kind needs, never package databases or file contents |
| `net.post:<host>` | refused | An HTTPS POST of such a payload to one named host (E6); every request is logged and the install preview names the host and the event kinds. The destination is always one you chose: there is no default endpoint and no telemetry |
| `time` | refused | A clock (breaks determinism, so it is marked in the output) |
| writing, executing, environment, other hosts | **do not exist** | There is no host call for them |

Grants are per plugin, recorded in `~/.config/emrac/plugins.toml`, and shown in plain words at install and in `emrac plugin info`.

## 4. Resource limits

Per call, enforced by the runtime and the host process, all overridable downward by the user:

| Limit | Default |
|---|---|
| Memory | 64 MiB |
| Instructions (fuel) | a fixed budget per call |
| Wall time | 5 s, enforced by killing the host process |
| Output size | 1 MiB; at most 500 findings |
| Host calls | counted; `net.get` at most a small number per call |

A limit hit is reported and the plugin is switched off for that run (principle 5).

## 5. The manifest

```toml
# plugin.toml
[plugin]
id = "pkgbuild-lint"
version = "0.3.1"
abi = 1                          # emrac's plugin ABI version
description = "Style and safety lint for PKGBUILDs"
license = "GPL-3.0-or-later"
homepage = "https://example.org/pkgbuild-lint"
wasm = "pkgbuild_lint.wasm"
sha256 = "..."                   # of the .wasm file

[extension]
points = ["audit"]               # E1

[capabilities]
emit = ["findings"]
fs_read = []
net_get = []

[limits]
memory_mib = 16                  # a request; the user's setting is the ceiling
```

The signature is a separate file next to it. Fields are validated strictly (unknown keys are an error), as `policy.toml` is.

## 6. The interface (a sketch)

JSON in, JSON out, versioned; chosen for simplicity and determinism, not speed. A module exports:

- `emrac_abi() -> i32`: the ABI version it was built for;
- `emrac_alloc(len) -> ptr`, so the host can place the input;
- `emrac_run(ptr, len) -> i64`: input JSON in, packed (pointer, length) of output JSON out.

It may import only functions of the module `emrac` (`emrac_log`, and `emrac_read_file` / `emrac_http_get` when granted). **Any other import, including WASI, makes the module unloadable.**

For E1 the input is the PKGBUILD text, its parsed variables, the `.install` text and each patch with its name; the output is findings in the shape `audit` already has:

```json
{"findings": [{"rule": "pkgbuild-lint/curl-in-prepare", "severity": "warning",
               "file": "PKGBUILD", "line": 14, "message": "...", "excerpt": "..."}]}
```

Implementation note: today `Finding.rule` is a `&'static str`; plugin findings need an owned rule id and a `source` tag (built-in or the plugin id), and the report and JSON must show the source.

## 7. Installing: the security checks

`emrac plugin install <file.wasm | name>`, as a plan with a preview, like a package:

1. **Integrity.** The hash is checked against the manifest, and, when installing by name, against a signed index; the signature is checked against a key you trust. A key is trusted explicitly the first time (its fingerprint is shown and the question defaults to no).
2. **Static inspection of the module**, before it is ever run: it must validate as WebAssembly; its **imports must be a subset of the capabilities declared**; no unknown imports, no WASI; no threads or atomics; declared memory within the limit; size within a cap; exports present and of the right type.
3. **Preview.** Id, version, author key fingerprint, extension points, capabilities in words, the imports found, the limits. A plugin that asks for anything beyond reading its own input gets a default-no question that `--yes` does not answer.
4. **Pinning.** The version and hash go into `plugins.lock`. An update whose hash changed is verified again; **an update that asks for more capabilities needs a new approval**; a lower version than installed is refused (rollback protection, as for repository databases).
5. **Recorded.** The install, update, removal, enable and disable are written to the history.
6. **After install.** `emrac plugin verify` re-hashes every installed module against the lock; `doctor` has a row (unverified, disabled, over-privileged, revoked). A signed revocation list can disable a plugin everywhere (the same feed question as the AUR defense blocklist).

## 8. Commands

```
emrac plugin list                      # installed, enabled, grants, last run
emrac plugin search <query>            # needs an index: phase 3
emrac plugin info <plugin>             # manifest, capabilities, imports, limits, key
emrac plugin install <file | name>     # the checks above
emrac plugin update [<plugin>]
emrac plugin remove <plugin>
emrac plugin enable | disable <plugin>
emrac plugin verify [<plugin>]
emrac plugin test <plugin> <fixture>   # run on a sample input in the host sandbox
```

All take `--dry-run` and `--json`. Storage: `~/.local/share/emrac/plugins/<id>/<version>/`, the enabled list and grants in `~/.config/emrac/plugins.toml`, the lock in `~/.local/share/emrac/plugins.lock`.

## 9. How it fits what exists

- **`audit` and the AUR defense.** Built-in rules stay built in. Plugins let people share extra rules (for example from a published advisory) without a release of emrac, which is the use the AUR defense page leaves to a signed feed.
- **The sandbox** (`sandbox.rs`) hosts the plugin process; the **limits** work the same way as build limits.
- **Trust and rollback protection** follow `repo_trust.rs`; **signed indexes** follow the release index (`docs/design/release-index.md`).
- **The hook parser and the pachooks idea** are unrelated: pacman hooks are declarative files, not plugins.

## 10. Phases

| Phase | Deliverable |
|---|---|
| 0 | Decide: the runtime, the signing scheme, and that the first release has **local installs only** (no registry), which avoids the governance problem |
| 1 | The host process in the sandbox; manifest and static inspection; `plugin install <file>`, `list`, `info`, `remove`, `enable`, `disable`, `verify`, `test`; E1 audit rules; hostile-plugin tests |
| 2 | E2 health checks, E3 plan notes, E4 formatters, E6 event observers, E7 policy gates; `net.get`, `net.post` and `fs.read` host calls; the doctor row |
| 3 | A signed plugin index, `search`, `update`, key trust, rollback protection, revocation; E8 read-only commands |
| 4 | E5 package sources and E9 plan proposals: their own designs (they need external tools and the transaction machinery) |

## 11. Testing

- **Hostile plugins, each of which must be contained:** an infinite loop, an allocation bomb, an undeclared import, a WASI import, an output flood, a trap, a plugin that emits `critical`, one that tries to read an ungranted path, one that tries a request to an ungranted host.
- **Install flow:** wrong hash, bad signature, unknown key, an update that adds a capability, a downgrade, a module whose imports exceed its manifest.
- **Determinism:** the same input twice gives byte-identical output, with the plugin sandboxed and with it not.
- **Fuzzing** the manifest parser and the module inspection.
- **Container check** (`dev/e2e/`): install, run, update with escalation refused, remove, against a real emrac.

## 12. Open questions

- **Runtime.** An interpreter (such as `wasmi`: small, pure Rust, deterministic, fuel metering) or a JIT (such as `wasmtime`: faster, a larger attack surface). My lean is an interpreter, because plugin work here is small and determinism and a small surface matter more than speed. This is untested; a prototype with a real rule set should decide it.
- **Signing.** Which scheme and which keys, and who is allowed to publish. Phase 1 avoids it by installing local files; phase 3 cannot.
- **A registry at all?** Who hosts and curates it, and how a wrongly published or malicious plugin is withdrawn. It is the same single point of trust as the AUR defense feed.
- **Authoring.** An SDK crate and a template, so writing a rule is not an exercise in the raw ABI.
- **Licensing.** Emrac is GPL-3.0-or-later; plugins are separate programs behind a defined ABI, and what licenses are acceptable for shared ones must be stated.
- **Is JSON the right ABI** once rule sets get large? It is easy to audit and to fuzz; a binary format can come with ABI 2.
- **Should E5 exist at all,** or should new sources stay a core responsibility?
- **Build workers, scanners and other tools.** A WebAssembly module cannot run `distcc`, a remote build worker or a malware scanner by itself. Whether emrac may run an external tool for a plugin is decided in section 15 (execution by proxy); it is not in phases 1 to 3. Until then a compile farm and distcc stay core (they are in `SPEC.md`).
- **Duplicating the core.** A plugin must add something, not re-implement a built-in feature (see section 14).

---

## 13. Example plugins

Illustrative only; none exists, and the names are placeholders. Grouped by extension point, with what each would need.

| Plugin | Point | Needs | What it does |
|---|---|---|---|
| `aur-advisory-rules` | E1 | its input only | Checks a PKGBUILD against indicators from a published advisory (a domain, a name pattern, a payload URL). The built-in defense stays in the core; this adds rules faster than a release |
| `pkgbuild-lint` | E1 | its input only | Style and safety lint (the spec's own example) |
| `license-policy` | E1 or E7 | `fs.read` of your policy file | Flags, or with a gate refuses, packages whose license is not on your list |
| `vuln-feed` | E1, E2 | `net.get:<feed host>` | Correlates installed packages with a vulnerability feed you choose and shows which pending upgrades fix them |
| `sbom-export` | E4 | its input only | Turns a package list into a Software Bill of Materials |
| `report-markdown` | E4 | its input only | Turns the `doctor --report` JSON into Markdown for a bug report |
| `driver-doctor` | E2 | a facts snapshot | GPU, kernel-module and driver compatibility findings |
| `kernel-module-notes` | E3 | the plan's JSON | Under a kernel upgrade, lists DKMS packages whose modules will be rebuilt |
| `team-rules` | E3 or E7 | `fs.read` of the rules file | Notes or vetoes packages on a shared "do not install" list |
| `notify` | E6 | `events`, no network | Desktop notification when a build ends or a transaction fails |
| `webhook` | E6 | `events`, `net.post:<your host>` | Sends build and transaction events to a webhook you configured |
| `upstream-watch` | E2, E8 | `net.get:<github host>` | Reports new upstream releases for the packages you built |
| `fleet-policy` | E9 | `fs.read` of a policy file | Proposes the operations that bring this machine in line with a shared policy; emrac shows them as a normal plan |
| `emrac-flatpak` | E5 | later | Flatpak apps beside native packages |

## 14. What should stay in the core, not become a plugin

A suggested list of plugin names gathered from other sources was reviewed against what emrac already has or has specified. Roughly a third to a half of it duplicates the core:

- **Built or specified in the core already:** `.pacnew` merge assistance (`pacnew`), rebuild advice (`rebuild`), orphan and removal analysis (`orphans`, `impact`), mirror ranking (Mirror Intelligence in the spec), upgrade explanation (update risk, news gate), disk forecasts (pre-flight, install planner), hook inspection (the hook preview and the package manifest), the shareable diagnostic report (`doctor --report`), package integrity watching (`verify`, package forensics), build duration tracking and benchmarks (Build Benchmark and statistics in the spec), build caches and a local repository (Cache Management and Repository Management), a compile farm and `distcc` (Compile Farm Support), fleet comparison (ideas A16 and A17), snapshots and backup, the rescue kit (C7), themes (TUI configuration), and the AUR defense itself.
- **The principle:** security and recovery that protect the transaction stay built in, so that removing a plugin can never remove a protection. Plugins add rules, data and presentation on top; they do not replace the core's checks.
- **Execution-heavy entries** (build farms, `distcc`, cross-build environments, alternate build engines, scanners) are not WebAssembly plugins; see the open question above.
- **Panels and themes** need the TUI to exist first. Themes are declarative data and need no plugin at all.

---

## 15. Industry-tool integrations (Git, containers, IaC, CI, observability)

Plugins may also connect emrac to developer and infrastructure tools. The rule is the same as everywhere else: **the plugin is a small adapter; the tool stays a separately installed program, never bundled and never a dependency of the core.** A `.wasm` module cannot run a program or touch the filesystem on its own, so every such link goes through a host call that you grant.

### Four kinds of integration

| Kind | How it works | Capability |
|---|---|---|
| Native tool | emrac runs an installed program for the plugin (execution by proxy) | `exec:<tool>` |
| Service API | the plugin asks for an HTTPS request to a named host | `net.get:<host>`, `net.post:<host>` |
| In-process | the whole job is done inside the module | none |
| Workflow | several of the above joined into one reviewed procedure | the union, shown in one preview |

### Execution by proxy (`exec:<tool>`), proposed rules

This is the riskiest capability, so it is narrow by design:

1. **Named tool, fixed path.** The manifest names the tool (`git`); emrac resolves it to a root-owned path in `/usr/bin` and records its sha256 in `plugins.lock`. A changed binary asks again.
2. **Argument templates, not strings.** The plugin may only pick from subcommands the manifest declares (`git rev-parse`, `git ls-remote`, `git log --format=...`). Free-form arguments, shell, pipes and environment changes are refused.
3. **Read-only by default.** Anything that writes (`git clone`, `docker build`, `terraform apply`) is a separate, louder capability (`exec.write:<tool>`), shown in the preview and never covered by `--yes`.
4. **The sandbox again.** The tool runs in the same bubblewrap sandbox as builds: no network unless `net.*` was granted for it, only the directories granted, and the same memory and time limits.
5. **Never privileged.** `exec` runs as the user, never through `sudo` and never from a pacman hook.
6. **Output is data.** What the tool prints is handed back as bytes, size-capped and treated as untrusted input like everything else a plugin reads.
7. **Logged.** Each run lands in history with tool, template and exit status.

### Families (all proposed, none exists)

| Family | Examples | Typical job |
|---|---|---|
| Source and forges | `emrac-git`, `emrac-github`, `emrac-gitlab`, `emrac-forgejo` | commit and tag provenance, signed-tag checks, build-from-commit, release metadata |
| Containers | `emrac-podman`, `emrac-docker`, `emrac-buildah` | clean build containers, image dependency inspection |
| Infrastructure as code | `emrac-terraform`, `emrac-opentofu`, `emrac-ansible` | describe build workers and mirrors, show a plan; applying stays outside |
| Orchestration | `emrac-kubernetes`, `emrac-helm`, `emrac-argocd` | submit build jobs, report their state |
| CI/CD | `emrac-github-actions`, `emrac-gitlab-ci`, `emrac-jenkins` | trigger builds, report results, release gates |
| Observability | `emrac-prometheus`, `emrac-opentelemetry`, `emrac-loki` | export build and transaction metrics, traces and logs |
| Supply chain and security | `emrac-sbom`, `emrac-cosign`, `emrac-slsa`, `emrac-trivy`, `emrac-sops`, `emrac-vault` | SBOMs, signing and provenance, scanning, secrets handling |
| Build systems | `emrac-cmake`, `emrac-meson`, `emrac-ninja`, `emrac-bazel` | build diagnostics |
| Artifacts and cloud | `emrac-oci-registry`, `emrac-artifactory`, `emrac-aws`, `emrac-gcp`, `emrac-azure` | publish and fetch artifacts, remote workers |
| Notifications | `emrac-matrix`, `emrac-slack`, `emrac-discord`, `emrac-webhook` | alerts (E6) |
| Editors and environments | `emrac-vscode`, `emrac-jetbrains`, `emrac-devcontainers` | editor and dev-environment setup |
| Fleet | `emrac-ssh`, `emrac-tailscale`, `emrac-fleet` | remote diagnostics, coordinated upgrades |

Secrets (`emrac-vault`, `emrac-sops`) need their own capability, since a plugin must never see a secret it was not given by name; this is an open question.

### The boundary that does not move

Plugins may inspect state, propose operations and talk to tools. **Installing, removing, privileged execution and committing a transaction stay with emrac's own authorization.** A plugin that proposes work gets it shown as a normal plan with the usual preview and confirmation. Anything that changes the system from outside emrac (an `apply`, a deploy) is out of scope for phases 1 to 3.

### Placement

Execution by proxy and the families above are phase 4 or later, each with its own design. Phase 1 to 3 plugins stay on `fs.read`, `net.*`, `events` and `time`.

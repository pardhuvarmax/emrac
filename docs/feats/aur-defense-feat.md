# AUR Defense: Malware Protection, Checks, and Dynamic Analysis

Feature document for Emrac. Status: `[idea]`, not yet in `SPEC.md`.

Emrac protects users from malicious AUR packages and from malware that arrives through any package, using layered static checks, sandboxed dynamic analysis, install-time inspection, post-install monitoring, and incident response. Everything is rule-based and local: no AI or LLMs, no telemetry, and no scripting engine.

---

## 1. Threat model

The AUR is user-submitted and unvetted: PKGBUILDs are shell scripts that run as the user at build time, and their install scriptlets run as root at install time. Recent incidents show the patterns Emrac needs to catch:

- **Remote script execution.** In July, three browser-related packages (`librewolf-fix-bin`, `firefox-patch-bin`, `zen-browser-patched-bin`) installed a script from a GitHub repository identified as a remote access trojan (CHAOS RAT). More packages were later linked to the same malware. Indicators advised by the community included a process named `systemd-initd` and a `rat-agent.service` unit.
- **Impersonation.** A campaign uploaded packages impersonating the `yay` and `paru` helper tools, with a malicious install script that ran a remote shell script as root.
- **Mass uploads.** A June attack reportedly distributed credential-stealing malware with rootkit capabilities through about 400 packages.
- **Short exposure windows.** The July packages were removed within about two days, which is still long enough to infect users who installed immediately.

Common traits: fetch and run something at build or install time, lookalike or "patch/fix/cracked" names, brand-new or mass-uploading maintainers, hidden persistence (services, cron, preload hooks), and credential theft.

Attacker goals the defenses target: run code, persist, steal credentials, and open a remote channel.

---

## 2. Principles

- **Deterministic and local.** Rules, signed indicator lists, and sandbox observation. No AI, no cloud lookups, no telemetry. The same inputs give the same findings.
- **Evidence for every finding.** Each finding states the rule, the severity, and the exact evidence (file and line, or the recorded event).
- **Fail closed for blocks.** A `BLOCK` finding stops the transaction. Overriding needs an explicit option, and every override is recorded in the transaction log.
- **Analysis never runs untrusted code outside the sandbox.** Static analysis only parses. Anything executed is executed in the sandbox.
- **Static and dynamic evidence combine.** Dynamic evidence outranks a clean static result: a package that looks fine but is observed doing something forbidden is blocked.
- **Be honest about limits** (section 10).

---

## 3. Defense layers

| # | Layer | When | Mode |
|---|---|---|---|
| 1 | Metadata and trust checks | Before downloading | Static |
| 2 | PKGBUILD and source analysis | Before building | Static |
| 3 | Diff review and soak time | Before building | Static |
| 4 | Isolated build with behavior tracing | During build | Dynamic |
| 5 | Built-package inspection | After build, before install | Static |
| 6 | Install detonation (scriptlets and hooks in a throwaway root) | Before the real install | Dynamic |
| 7 | Observe mode for the program itself | Optional, before or after install | Dynamic |
| 8 | Post-install audit and incident response | After install | Static and dynamic |

All layers feed one findings report shown in the transaction preview (see section 7).

---

## 4. Static layers

### 4.1 Layer 1: metadata and trust

Checks on the AUR record, with no download of the package contents:

- **Lookalike names.** Names within a small edit distance of popular packages, swapped words, and extra suffixes (`-bin`, `-patch`, `-fix`, `-git`) where the base name already exists. Compare against a curated list of well-known tools, including AUR helpers.
- **Maintainer signals.** New account, just-adopted orphan, maintainer change since the last version, and a burst of uploads in a short time.
- **Suspicious naming patterns** from unknown maintainers (`-cracked`, `-patched`, `-fix`) as a warning, never as a block on their own.
- **Shadowing.** A name that shadows or closely resembles an official repository package.
- **Signed blocklist.** Known-bad package names, maintainers, URLs, domains, and file hashes. The feed is signed, cached locally, and updated like Emrac news. A blocklist hit is a `BLOCK`.

### 4.2 Layer 2: PKGBUILD, `.install`, and source analysis

Parse with a real shell parser (the AST approach already planned). Never execute.

| Rule | Severity |
|---|---|
| Downloads fetched and executed (`curl ... \| sh`, `wget` then run) in any function or `.install` file | BLOCK |
| Network access in `prepare()`, `build()`, or `package()` outside declared sources | WARN (see 5.4 for legitimate ecosystem downloads) |
| Sources from paste sites, URL shorteners, raw-content hosts, or bare IP addresses | WARN |
| `SKIP` checksums on non-VCS sources | WARN |
| Obfuscation: `base64 -d`, `eval`, `xxd -r`, long encoded strings, dynamic command construction | WARN, BLOCK when combined with execution |
| Setuid or setgid bits, `chmod +s` | WARN |
| Writing or enabling systemd units, timers, cron entries, udev rules, `ld.so.preload`, sudoers, PAM config, shell startup files, `authorized_keys` | WARN to BLOCK depending on the target and whether the package's purpose explains it |
| `sudo`, `su`, or privilege commands inside the PKGBUILD | BLOCK |
| `.install` scriptlets doing anything beyond expected tasks (user creation, cache refresh) | WARN |
| `--no-check-certificate`, `curl -k`, disabled TLS verification | WARN |

Patches get the same treatment: scan added lines in any patch file for the patterns above, because the July incident delivered its payload through a patch.

### 4.3 Layer 3: diff review and soak time

- **Review ledger.** Record the PKGBUILD hash you last approved. On update, show only the diff since that approval and ask for re-approval. High-risk diff events are highlighted: new source host, new `.install` file, new dependencies, maintainer change.
- **Soak time.** A configurable delay before a new AUR version can be installed, unless it is marked as a security fix. Because malicious packages were typically removed within a couple of days, even a short delay avoids most infections. Applies mainly to AUR and VCS (`-git`) packages.
- **Trusted and blocked maintainers.** User allowlist and denylist.

---

## 5. Dynamic analysis

Dynamic analysis observes what code actually does, which catches obfuscated or downloaded payloads that static rules cannot see.

### 5.1 Goals and scope

- Detect behavior that static analysis misses: payloads fetched at runtime, obfuscated commands, hidden persistence, credential access.
- Produce an ordered, evidence-backed event log that a deterministic rule engine turns into findings.
- Run entirely inside the Layer 2 sandbox, with no effect on the real system.

### 5.2 Isolation and tracing technology

| Tier | Needs | Provides |
|---|---|---|
| Tier 1, unprivileged | User, mount, network, and PID namespaces; Landlock filesystem rules; cgroup v2 limits; seccomp filters; ptrace or seccomp user-notification tracing of the build's own processes | Containment plus tracing of syscalls from the build tree |
| Tier 2, via the privileged helper | eBPF tracepoints and fanotify | Deeper visibility into file and network activity, including events from detached processes |
| Tier 3, later | A lightweight VM (for example a QEMU microVM) for high-risk packages | Isolation that does not depend on the host kernel's namespace boundaries |

To keep builds fast, trace selectively: a seccomp filter that reports only the dangerous syscalls (`execve`, `connect`, writes to sensitive paths, `chmod` with setuid, `ptrace`, `mount`, module loading) instead of tracing everything.

Namespaces are not a boundary against kernel exploits. Tier 3 exists for that reason.

### 5.3 What is observed

- **Process tree.** Every `execve`, with arguments. Flag `curl`, `wget`, `nc`, `sh -c`, `python -c`, `base64 -d`, interpreters fed from pipes, and processes that fork, call `setsid`, and outlive the build step.
- **Network attempts.** Every `connect`, `sendto`, and DNS lookup, recorded even when denied.
- **File access.** Writes outside the build directory (blocked by the sandbox and logged), and reads of sensitive paths (see canary files).
- **Persistence attempts.** Creating or enabling units, timers, cron files, `ld.so.preload`, udev rules, sudoers entries, shell startup files, and SSH key files.
- **Privilege and evasion attempts.** `setuid` changes, `ptrace`, `LD_PRELOAD`, `mount`, kernel module loading, `keyctl`.

### 5.4 Network policy

Many legitimate builds download dependencies at build time (`cargo`, `go mod`, `npm`, `pip`). A blanket "no network" rule would break them, so the policy has tiers:

1. **Fetch phase.** Network is allowed, and only to fetch the declared `source=()` entries, with checksum verification.
2. **Ecosystem dependency step.** For recognized language ecosystems, Emrac runs the standard fetch command (for example `cargo fetch`, `go mod download`) in a restricted-network step with a domain allowlist (the ecosystem's package registry), then builds offline.
3. **Build phase.** No network. Any attempt is denied and recorded.
4. **Sinkhole (optional).** Instead of failing outright, a local sinkhole answers requests with empty or error responses and records the exact URL and host that the build tried to reach. This exposes where a payload would have come from, with no real traffic leaving the machine.

An unexplained network attempt during the build phase is a `BLOCK`.

### 5.5 Canary files

Place fake sensitive files in the sandbox's home directory, for example an SSH key, a browser profile database, cloud credentials, a keyring file, and a shell history file. They contain nothing real. Any read of a canary by a build or install process is high-severity evidence of credential-stealing behavior, even if static analysis found nothing.

### 5.6 Install detonation

Install scriptlets and hooks run as root, so they get their own dynamic check before the real install:

1. Extract the built package into a throwaway overlay of the real root.
2. Run the `.install` scriptlets and the pacman hooks that would fire, in the sandbox with no network.
3. Record every change they make.
4. Compare with the package's declared file list. Writes the package does not declare (services, users, cron, profile files) are reported as **undeclared changes**.

### 5.6.1 Limits of the real install

Emrac commits through pacman itself. It cannot easily wrap the real scriptlet execution in the same restrictions, so detonation runs before the install and monitoring (5.8) runs after. If a package behaves differently on the real system, the post-install audit is the safety net.

### 5.7 Observe mode for the program itself

`emrac try --observe <pkg>` (extends `try` from `additions.md` B2) runs the installed program in the sandbox with the network sinkhole and canary files, then reports what it touched. Useful for closed `-bin` packages and for checking a program before trusting it.

### 5.8 Post-install monitoring

- **Scheduled audit, not a daemon.** A systemd timer or manual run of `emrac audit --persistence` compares the system against the install records. It finds persistence points not owned by any package (units, cron, preload entries, shell startup edits, SSH keys), modified package files, and unexpected setuid files.
- **Optional watch window.** For packages installed from the AUR, an opt-in short observation window (through the privileged helper with eBPF) records network connections and new persistence from the package's binaries for a limited time.

### 5.9 Output and determinism

The sandbox produces an ordered event log. The same package built twice in the same environment gives the same relevant events, apart from timing. Each log is hashed and stored with the transaction record. A rule engine converts events to findings, using the same BLOCK, WARN, INFO model as the static layers.

---

## 6. Findings and policy

### 6.1 Severity model

| Level | Meaning | Behavior |
|---|---|---|
| BLOCK | Strong evidence of malicious or unsafe behavior | Transaction stops. Override needs an explicit option and is logged |
| WARN | Suspicious or unusual, may have a legitimate explanation | Shown in the preview. Needs confirmation |
| INFO | Notable but ordinary | Shown in the detailed report |

### 6.2 Example output (illustrative)

```
AUR: example-patch-bin  (new package, maintainer account 2 days old)
  [BLOCK] static   prepare() downloads and runs a script from raw.githubusercontent.com (PKGBUILD:14)
  [BLOCK] dynamic  build attempted connect to 203.0.113.9:443 (denied)
  [BLOCK] dynamic  build read a canary file (~/.ssh/id_ed25519)
  [WARN]  static   3 packages uploaded by this maintainer in 6 hours
  [WARN]  static   checksum SKIP on a non-VCS source
Verdict: refused by policy block-remote-exec
```

### 6.3 Policy configuration (illustrative)

```toml
[aur_defense]
soak_hours = 48              # delay before new AUR versions install
require_review = true        # PKGBUILD diff approval on update
dynamic_analysis = "aur"     # off | aur | all
network_policy = "ecosystem" # none | ecosystem | sinkhole
block_remote_exec = true

[aur_defense.trusted_maintainers]
names = ["example-maintainer"]
```

---

## 7. Incident response

`emrac incident <id>` takes a signed advisory file containing package names, service and process names, file paths, domains, and hashes. Because Emrac keeps a transaction history, it still works after a malicious package has been removed from the AUR.

It reports:

1. Whether any listed package was ever installed, with the transaction and date.
2. Which files, services, and persistence entries it left.
3. Whether any listed process, service, or file is present now.
4. Remediation steps: remove the package, list what to delete, and which credentials to rotate.

Indicator file format (illustrative):

```toml
[advisory]
id = "example-2026-001"
issued = "2026-07-18"

[[package]]
name = "example-patch-bin"

[[indicator]]
type = "service"
value = "example-agent.service"

[[indicator]]
type = "process"
value = "example-initd"
```

Advisories are signed and distributed through the same channel as Emrac news.

---

## 8. Commands (proposed)

| Command | Purpose |
|---|---|
| `emrac scan <pkg> [--static] [--dynamic]` | Run the defense layers on a package without installing |
| `emrac review <pkg>` | Show the PKGBUILD diff since the last approval and record approval |
| `emrac try --observe <pkg>` | Observe a program in the sandbox (section 5.7) |
| `emrac audit --persistence` | Find unowned persistence points and modified files |
| `emrac incident <id>` | Check history and system against an advisory |
| `emrac policy show` | Show active defense policies and blocklist version |

Command names are proposals. `audit` already exists in `SPEC.md` and would be extended.

---

## 9. Integration with other Emrac docs

- `additions.md`: A1 to A4 (typosquat, source change, new maintainer, key pinning) feed Layers 1 and 2. A9 (stray file scanner) feeds the post-install audit. B2 (`try`) is extended by observe mode. B3 (pre-flight) can include the defense checks. The binary inspector idea (ELF analysis) is used in Layer 5.
- `emrac.md` and `ideas.md`: Layer 2 (sandbox) hosts the dynamic analysis. The install-time capability manifest is where findings are shown. The no-network build phase and the fetch/build split come from `ideas.md`.
- Hook editor idea: the same lint rules apply to hooks that packages add.
- Critical features (`additions.md` Part C): the journal records overrides and findings, and plan integrity ensures that what was analyzed is what is installed.

---

## 10. Limits and honesty

- Static checks cannot catch everything: heavy obfuscation and malicious upstream source code can pass.
- Dynamic analysis can be evaded: payloads can wait, check for sandbox artifacts, or trigger only on later execution. A realistic fake environment (hostname, user, canary files) reduces but does not remove this.
- Namespaces are not a defense against kernel exploits. High-risk packages should use the VM tier when available.
- The signed blocklist only knows past incidents.
- The AUR remains unvetted. The strongest protections are isolation, least privilege, reviewing diffs, and soak time. Optional scanners such as ClamAV or YARA rules can be extra signals but never trust decisions.
- False positives are a real cost. Warnings should be few, specific, and explained, or users will ignore them.

---

## 11. Implementation order

| Phase | Deliverable |
|---|---|
| 1 | PKGBUILD and `.install` static rules with BLOCK, WARN, and INFO reporting |
| 2 | Metadata and trust checks, signed blocklist, soak time |
| 3 | Diff review ledger |
| 4 | Isolated build with selective syscall tracing, network policy, canary files |
| 5 | Built-package inspection and install detonation |
| 6 | Incident response and `audit --persistence` |
| 7 | Observe mode, sinkhole, eBPF watch window |
| 8 | VM isolation tier |

---

## 12. Testing

- **Synthetic samples.** Harmless test packages that emulate malicious behavior: a PKGBUILD that tries to download and run a script, one that reads a canary file, one that creates a persistence unit, one that obfuscates a command. Each must produce the expected findings.
- **False-positive corpus.** Build a large set of popular, legitimate AUR packages (including `cargo`, `go`, and `npm` builds) and measure how many raise warnings or blocks. Tune rules until warnings are rare.
- **Regression tests** for each rule, with the evidence format.
- **Reproducibility tests.** The same package and environment give the same findings.
- **Sandbox escape tests** for the isolation tiers, using known unprivileged escape attempts.

---

## 13. Open questions

- Who maintains the signed blocklist and advisory feed, and how are false entries corrected?
- How much performance overhead is acceptable for tracing, and which AUR packages should be traced by default?
- How should network policy handle ecosystems beyond `cargo`, `go`, `npm`, and `pip`?
- Is Tier 3 (VM isolation) in scope for the first release?
- What is the right default for `dynamic_analysis`: AUR only, or all source builds?
- How are `BLOCK` overrides handled in unattended updates (most likely: never allowed).

---

## Sources

- BleepingComputer, Arch Linux pulls AUR packages that installed CHAOS RAT malware: https://bleepingcomputer.com/news/security/arch-linux-pulls-aur-packages-that-installed-chaos-rat-malware
- Linuxiac, Malware Discovered in Arch Linux AUR Packages: https://linuxiac.com/malware-discovered-in-arch-linux-aur-packages/
- BleepingComputer, Fake Arch Linux packages push malware with root privileges: https://www.bleepingcomputer.com/news/security/fake-arch-linux-packages-push-malware-with-root-privileges/amp/
- Cyber Security News, malicious AUR packages: https://cybersecuritynews.com/malicious-aur-packages/amp/
- Community advisory summary with indicators: https://communities.stormux.org/post/15954/53386

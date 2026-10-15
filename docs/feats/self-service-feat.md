# Emrac Self Management Service (Emrac-SMS)

Feature specification · CLI architecture · Lifecycle management · Diagnostics · Recovery

Emrac-SMS is the subsystem responsible for managing Emrac's own installation, version lifecycle, local metadata, operational health, and release information. It provides a consistent interface for inspecting, synchronizing, upgrading, and downgrading Emrac independently of the package operations Emrac performs on the host system.

The central architectural principle is that Emrac must be able to manage its own lifecycle without confusing self-management with host package management. Upgrading Emrac is not the same operation as upgrading Arch Linux packages; synchronizing Emrac's metadata is not the same as synchronizing Pacman repositories.

Emrac-SMS is a logical subsystem exposed through the Emrac CLI. It does not require a continuously running background daemon. Its operations can execute on demand, with persistent state stored locally where necessary.

## 1. Goals and scope

### 1.1 Primary goals

1. Provide a canonical command interface for managing Emrac itself.
2. Synchronize Emrac-specific metadata and local state with trusted sources.
3. Inform users about releases, security advisories, announcements, and compatibility changes.
4. Verify the integrity and operational health of the Emrac installation.
5. Diagnose failures across the executable, configuration, state database, metadata cache, and update mechanisms.
6. Upgrade Emrac to a newer compatible release.
7. Downgrade Emrac to a previous or explicitly selected release.
8. Preview supported mutating operations using `--dry-run`.
9. Respect the installation method and ownership of Emrac's executable and files.
10. Preserve recoverability through preflight validation, transaction records, and post-operation verification.

### 1.2 Non-goals

Emrac-SMS is not, by itself:

- A replacement for Pacman, APT, DNF, or other host package managers.
- A general-purpose Linux distribution upgrade service.
- A mechanism for silently overwriting package-manager-owned files.
- A requirement to run a privileged system daemon.
- A guarantee that every release can be downgraded safely.
- A substitute for signed releases, trustworthy metadata, or secure update verification.

### 1.3 Terminology

| Term | Definition |
|---|---|
| Emrac-SMS | Emrac Self Management Service, the logical self-management subsystem |
| Self-management | Operations targeting Emrac's own installation, state, and lifecycle |
| Installation method | The mechanism that owns or maintains Emrac, such as Cargo, an OS package, or a manually installed binary |
| Release metadata | Information about versions, artifacts, checksums, compatibility, and release notes |
| Local state | Persistent Emrac data, including cached metadata, configuration references, and operation records |
| Dry run | A planning mode that does not apply the requested mutations |
| Upgrade | A transition from an installed release to a newer release |
| Downgrade | A transition from an installed release to an older release |
| Synchronization | Refreshing or reconciling Emrac-managed metadata and state without inherently changing the executable version |

## 2. Canonical command architecture

The canonical namespace is `emrac self`. All self-management operations must be routed through this namespace rather than implemented as unrelated top-level commands.

```
emrac
└── self
    ├── --help
    ├── sync
    │   └── --dry-run
    ├── news
    ├── check
    ├── diagnose
    ├── up
    │   └── --dry-run
    ├── upgrade
    ├── dn
    │   └── --dry-run
    └── downgrade
```

The command parser must distinguish three things:

- Command identity: the canonical operation being requested.
- Options: modifiers that change how the operation executes.
- Arguments: optional values such as a target release or a metadata source.

Short aliases must resolve to the same internal operation as their canonical command. They must not have separate implementations or separate behavioral rules.

### 2.1 Canonical command registry

| Canonical operation | Accepted command | Mutates state? | Network required? |
|---|---|---|---|
| Help | `emrac self --help` | No | No |
| Synchronize | `emrac self sync` | Yes, potentially | Usually |
| News | `emrac self news` | No, except optional cache writes | Not always |
| Check | `emrac self check` | No | No, by default |
| Diagnose | `emrac self diagnose` | No, except optional report output | No, by default |
| Upgrade | `emrac self upgrade` | Yes | Usually |
| Upgrade alias | `emrac self up` | Yes | Usually |
| Downgrade | `emrac self downgrade` | Yes | Usually |
| Downgrade alias | `emrac self dn` | Yes | Usually |

Network requirements depend on configuration and available caches. For example, `sync` may support offline validation, and `check` must still be useful when the network is unavailable.

The distinction between read-only and mutating operations is important. A command can create logs or refresh a cache without modifying Emrac's executable, but those side effects must be documented.

## 3. Command specifications

### 3.1 `emrac self --help`

Purpose: expose the complete self-management interface and its operational guarantees.

The help output should include:

- Available commands and their descriptions.
- Canonical names and aliases.
- Supported global and command-specific options.
- The meaning of `--dry-run`.
- The default release source and configuration locations, where appropriate.
- Links to project documentation and support resources.
- A note that installation ownership determines how upgrades and downgrades are performed.

Example:

```
Emrac Self Management Service (Emrac-SMS)

Manage the Emrac installation, release lifecycle, metadata,
and local operational state.

Usage:
  emrac self <COMMAND> [OPTIONS]

Commands:
  sync       Synchronize Emrac metadata and local state
  news       Show release news and announcements
  check      Check installation health and consistency
  diagnose   Perform detailed diagnostics
  up         Upgrade Emrac (short alias)
  upgrade    Upgrade Emrac
  dn         Downgrade Emrac (short alias)
  downgrade  Downgrade Emrac

Options:
  --help       Show help
  --dry-run    Preview changes without applying them

Examples:
  emrac self sync --dry-run
  emrac self sync
  emrac self news
  emrac self check
  emrac self diagnose
  emrac self up --dry-run
  emrac self upgrade
  emrac self dn --dry-run
  emrac self downgrade
```

The help command must be available without network access, elevated privileges, or a functioning metadata cache.

### 3.2 `emrac self sync`

Purpose: synchronize Emrac's own metadata and managed local state.

`sync` is a reconciliation operation, not an upgrade operation. It must not replace the Emrac executable merely because a newer release exists.

Possible synchronization targets include:

- Release manifests and version metadata.
- Release channels, such as stable and prerelease, if supported.
- Package and artifact indexes used by Emrac-SMS.
- Metadata schema versions and migration requirements.
- Cached release announcements.
- Locally maintained metadata indexes and validation records.
- Compatibility information for the running Emrac version.

The exact set of targets must be explicit in the implementation. A single `sync` command must not silently synchronize host package databases unless that behavior is separately designed and requested.

Normal execution:

1. Load configuration and determine the enabled metadata sources.
2. Inspect the current metadata state and identify stale or missing entries.
3. Retrieve updated metadata from configured, trusted sources.
4. Validate schema versions, signatures where available, checksums, and source identity.
5. Stage the new metadata separately from the active metadata.
6. Commit the staged metadata only after validation succeeds.
7. Record synchronization status, timestamps, and errors.
8. Report what changed and what remained unchanged.

Dry-run execution:

```
emrac self sync --dry-run
```

The command should report which metadata sources would be contacted, which records would be refreshed, whether local migrations would be required, and which files or indexes would be changed.

It must not commit metadata, migrate persistent state, or replace the active index. If the plan depends on fetching information that is not already available locally, the output must distinguish verified facts from estimates.

Example output:

```
Emrac-SMS: Synchronization Plan

Current metadata:  schema v3
Target metadata:   schema v3
Sources:           2 configured
Network access:    required
Planned actions:
  - Refresh release manifest
  - Validate release metadata
  - Update local metadata index

Changes applied:   none
Mode:              dry-run
```

This is illustrative output; the actual metadata schema and source count must come from runtime state.

### 3.3 `emrac self news`

Purpose: present Emrac-related release information without requiring the user to visit external websites manually.

News may include:

- New stable releases.
- Prerelease announcements, when enabled.
- Important bug fixes.
- Security advisories.
- Breaking configuration changes.
- Metadata schema changes.
- Deprecated commands and migration guidance.
- Known compatibility issues.
- Notices about unsupported upgrade or downgrade paths.

News should be sourced from authenticated or otherwise trusted project metadata. Each item should include its publication date, version or affected range when applicable, and a reference to the full announcement.

The command should support cached news when offline. It must clearly label stale information and must not imply that cached news represents the latest available announcements.

Example:

```
Emrac-SMS: Release News

[2026-10-12] Emrac 0.4.0
  Category: Release
  Summary: Example release announcement.
  Source:   Project release metadata

No installation changes were performed.
```

The example announcement is illustrative, not a statement that this release exists.

By default, `news` should be read-only with respect to the executable and configuration. Caching new news is acceptable only if that behavior is documented and its cache changes are atomic.

### 3.4 `emrac self check`

Purpose: perform a routine, non-destructive health check of the installed Emrac environment.

`check` is intended for fast answers to questions such as: Is Emrac installed correctly? Is the configuration valid? Can the local state be read? Is an update mechanism available?

Recommended checks:

| Check | Validation |
|---|---|
| Executable | Confirm the running binary can be identified and inspected |
| Version | Parse the installed version and validate its format |
| Installation ownership | Determine the installation method where possible |
| Configuration | Parse configuration and validate known settings |
| Local state | Check readability, schema compatibility, and basic consistency |
| Metadata cache | Validate structure, timestamps, and schema compatibility |
| Release source | Check configuration without necessarily contacting the network |
| Update capability | Determine whether the current installation method supports automated updates |
| Permissions | Identify relevant access or write-permission problems |
| Operation journal | Check whether an earlier operation is marked incomplete |

Example:

```
Emrac-SMS: Health Check

[PASS] Executable
[PASS] Version metadata
[PASS] Configuration
[PASS] Local state
[WARN] Release metadata is stale
[INFO] Installation method: detected

Result: Warnings found
```

The status of the installation method must be based on actual detection. If ownership cannot be determined reliably, the result should be `UNKNOWN`, not an inferred package-manager identity.

`check` should not automatically repair files, update Emrac, change configuration, or delete caches. It should report findings and suggest corrective actions.

### 3.5 `emrac self diagnose`

Purpose: perform a deeper, structured investigation when a routine health check is insufficient.

Diagnostics can include:

- Detailed configuration parsing and validation.
- Executable path and file-integrity inspection.
- Installation ownership detection.
- Metadata source reachability tests, when explicitly enabled.
- Cache consistency and schema compatibility analysis.
- Persistent-state corruption detection.
- Lock-file and incomplete-transaction inspection.
- Update artifact availability and validation checks.
- Permission and filesystem capability checks.
- Compatibility analysis for requested releases.
- Detailed explanations of failed or inconclusive checks.

Unlike `check`, `diagnose` may perform expensive tests. It should not automatically apply repairs.

Recommended behavior:

```
emrac self diagnose
```

Produces a detailed report in the terminal.

A future extension could provide:

```
emrac self diagnose --output report.json
```

This output option is a proposed extension, not part of the initial canonical command list.

Diagnostic reports must avoid exposing credentials, private tokens, sensitive environment values, or unrelated personal data. If users choose to export a report, they should be able to review redacted content before sharing it.

`diagnose` and `check` must share a common diagnostic engine so their results remain consistent. The difference is the depth and presentation of the checks, not two unrelated implementations.

### 3.6 `emrac self up` and `emrac self upgrade`

Purpose: transition the current Emrac installation to a newer release.

`upgrade` is the canonical operation name. `up` is its short alias.

These commands must resolve to the same operation, with the same defaults, validations, error codes, and safety rules.

```
emrac self up --dry-run
emrac self upgrade
```

The first command previews the upgrade. The second executes it.

#### Upgrade planning

Before applying an upgrade, Emrac-SMS must determine:

1. The currently installed version.
2. The installation method and the component responsible for updating it.
3. The requested target version or configured release channel.
4. The latest eligible release from trusted metadata.
5. Whether the target is newer than the installed version.
6. Whether the artifact matches the current operating system and architecture.
7. Whether the artifact's authenticity and integrity can be verified.
8. Whether the configuration and local-state schemas are compatible.
9. Whether migrations or backups are required.
10. Whether sufficient permissions and disk space are available.
11. Whether another self-management operation is already running.
12. Whether the operation can be rolled back if activation fails.

If the current version is already the latest eligible release, the command should report that no upgrade is necessary. It should not reinstall or rewrite the installation unnecessarily.

#### Upgrade execution

The recommended lifecycle is:

`Discover → Plan → Validate → Prepare → Stage → Activate → Verify → Commit`

- Discover: identify the installed release and update source.
- Plan: calculate the exact version transition and intended changes.
- Validate: check release trust, platform compatibility, permissions, and migration requirements.
- Prepare: acquire an operation lock and create necessary backups or recovery records.
- Stage: obtain and validate the new artifact without destroying the active installation.
- Activate: perform the installation-method-appropriate replacement.
- Verify: confirm that the new executable starts and passes essential health checks.
- Commit: mark the operation successful and finalize its journal.

The implementation must not claim full transactional rollback where the installation method cannot provide it. For package-manager-owned installations, the package manager remains authoritative.

#### Dry-run requirements

```
emrac self up --dry-run
```

The command should identify the installed and target versions, installation method, artifact source, verification steps, migration requirements, and planned file or package operations.

It must not download and activate the update as a hidden side effect. If a read-only metadata lookup or network request is needed to construct the plan, that request should be reported.

### 3.7 `emrac self dn` and `emrac self downgrade`

Purpose: transition Emrac to an older release.

`downgrade` is the canonical operation. `dn` is the short alias.

```
emrac self dn --dry-run
emrac self downgrade
```

A downgrade should generally require a target version when more than one older version is available. The bare command can use a documented default, such as the last known working version, but only if Emrac has trustworthy records for that version. It must not guess.

A future explicit syntax could be:

```
emrac self downgrade --version 0.3.2
emrac self downgrade --version 0.3.2 --dry-run
```

The `--version` option is a proposed extension.

#### Downgrade preflight

Before downgrading, the service must check:

- The target release exists in trusted metadata.
- The target artifact is available for the current platform.
- The target is older than the installed release.
- The target release's integrity can be verified.
- Current configuration can be read by the target release.
- Persistent state is compatible or can be safely migrated or restored.
- Any newer configuration fields that would be lost are identified.
- The downgrade is supported by the installation method.
- A recovery path exists for changes that cannot be reversed automatically.

#### Configuration and state compatibility

Executable rollback does not necessarily mean state rollback. A newer Emrac version might migrate its database or configuration into a format that an older version cannot read.

Therefore, the downgrade workflow must distinguish:

1. Replacing the executable.
2. Migrating or restoring persistent state.
3. Restoring configuration compatibility.
4. Reversing external side effects.

The service must not silently delete newer state or perform a destructive reverse migration. If a safe transition cannot be established, it should refuse the operation and explain the blocker.

#### Dry-run requirements

```
emrac self dn --dry-run
```

The plan should identify the proposed target, compatibility risks, state migrations, backup strategy, and any actions that cannot be automatically reversed.

If no valid target can be determined, the service should return an actionable error instead of inventing a version or selecting an arbitrary historical release.

## 4. Canonicalization and alias resolution

The CLI must normalize command names before dispatching to their implementation.

### 4.1 Alias mapping

| User input | Internal operation | Canonical display |
|---|---|---|
| `emrac self up` | `SelfUpgrade` | `upgrade` |
| `emrac self upgrade` | `SelfUpgrade` | `upgrade` |
| `emrac self dn` | `SelfDowngrade` | `downgrade` |
| `emrac self downgrade` | `SelfDowngrade` | `downgrade` |

Canonicalization is an internal parsing rule. It should not rewrite user input in shell history or change the literal command that the user entered.

The normalized operation identity should be used for:

- Permission and policy checks.
- Dry-run planning.
- Operation logging.
- Error reporting.
- Telemetry, if telemetry is implemented and explicitly enabled.
- Testing and documentation references.

For example, `emrac self up --dry-run` and `emrac self upgrade --dry-run` must produce equivalent plans when their configuration and environment are identical.

### 4.2 Parsing precedence

The parser should follow these rules:

1. Parse `self` as the self-management namespace.
2. Recognize `--help` and other supported options.
3. Resolve aliases to canonical operations.
4. Validate arguments and option combinations.
5. Construct an operation request.
6. Dispatch that request to the appropriate service implementation.

Unknown subcommands should produce an error with nearby valid command suggestions, if available.

Example:

```
error: unknown self-management command 'upgrde'

Did you mean:
  upgrade
```

A spelling suggestion must never cause the suggested operation to execute automatically.

## 5. The `--dry-run` contract

`--dry-run` is one of the most important safety guarantees in Emrac-SMS. It must have a precise and testable definition.

Contract: a dry run must not commit the requested operation's persistent mutations or change the installed Emrac version.

This does not mean the command is forbidden from performing all I/O. It may read files, inspect installed packages, and contact release metadata sources if required to construct an accurate plan. Those actions must be disclosed when relevant.

### 5.1 Required behavior

A supported dry-run operation must:

- Resolve the requested operation and all aliases.
- Read the current state.
- Validate the proposed operation as far as possible.
- Identify intended changes.
- Identify blockers and unresolved information.
- Report whether the plan is complete.
- Exit without applying the requested mutations.

It must not:

- Replace or overwrite the active executable.
- Commit new metadata or migrations.
- Change persistent configuration.
- Delete recovery records.
- Execute package-manager mutations.
- Mark an operation as successfully completed when it was only simulated.

Temporary files created for planning should be avoided where possible. If unavoidable, they must be isolated and cleaned up without modifying active state.

### 5.2 Plan completeness

A dry run can produce an incomplete plan when required information is unavailable.

For example, if Emrac is offline and the target release cannot be established from trusted cached metadata, it must not invent a target version.

The output should distinguish:

- `READY`: the intended changes and relevant prerequisites have been resolved.
- `BLOCKED`: a known condition prevents the operation.
- `INCOMPLETE`: the plan cannot be fully resolved with available information.

These are proposed plan states, not shell exit codes. A `READY` plan still does not guarantee that the actual operation will succeed, because the environment can change after planning.

### 5.3 Plan output format

All mutating commands should use a consistent plan representation, whether displayed as human-readable text or emitted through a future machine-readable interface.

```
Operation:          self.upgrade
Mode:               dry-run
Installed version:  detected-version
Target version:     candidate-version
Installation:       detected-method
Plan status:        READY

Planned actions:
  1. Validate target artifact
  2. Stage replacement
  3. Activate new version
  4. Verify executable and state compatibility

Persistent changes: none applied
```

The version and installation fields above are placeholders. Real output must use values obtained from the environment.

A plan should also explain whether the upgrade will require elevated privileges, a network connection, or a separate package-manager invocation.

## 6. Installation ownership and update delegation

Emrac-SMS must distinguish between managing Emrac's logical lifecycle and directly replacing its executable.

A Rust binary can be installed through Cargo, an Arch package, a Debian package, an RPM package, or a manually managed release archive. Each method may have a different owner.

The self-management service should detect the installation method when possible and report uncertainty explicitly.

| Installation method | Recommended upgrade strategy |
|---|---|
| Cargo | Use Cargo's installation mechanism or provide the appropriate upgrade instructions |
| Arch package | Delegate to Pacman and the package source that owns the installation |
| AUR package | Use the configured Arch package workflow, including rebuilds where required |
| Debian package | Delegate to APT/dpkg and the configured repository or package file |
| RPM package | Delegate to DNF/RPM and the configured repository or package file |
| Manual binary | Use a verified upstream release artifact and a controlled replacement procedure |
| Unknown or ambiguous | Inspect, report uncertainty, and avoid automatic replacement |

This table describes the intended delegation policy, not a claim that all integrations exist.

### 6.1 Ownership invariant

Emrac-SMS must never silently replace a file owned by a system package manager or another recognized installation mechanism.

If ownership is ambiguous, the service should:

1. Identify the executable path.
2. Inspect available ownership metadata.
3. Report the detection evidence.
4. Refuse an unsafe automatic replacement.
5. Provide a supported manual or package-manager-specific path.

### 6.2 Separation of concerns

Emrac-SMS should not duplicate the package manager's dependency resolution, package database, or installation transaction system.

Its responsibility is to determine the desired Emrac release, establish the appropriate upgrade plan, validate the operation, and delegate to the correct installation mechanism.

This prevents `emrac self upgrade` from accidentally becoming a system-wide upgrade command.

## 7. Persistent state and operation records

Emrac-SMS needs enough persistent state to detect interrupted operations, determine the installed release, support diagnostics, and recover where possible.

The precise paths should follow Emrac's existing filesystem policy. A possible logical layout is:

```
Emrac state directory/
├── config/
│   └── self-management configuration
├── metadata/
│   ├── release manifest cache
│   └── metadata validation records
├── state/
│   ├── current installation record
│   └── schema version
├── operations/
│   ├── operation journal
│   └── previous operation summaries
├── backups/
│   └── eligible recovery snapshots
└── logs/
    └── self-management logs
```

This is a conceptual layout, not a prescribed set of filesystem paths. Actual locations should use the appropriate XDG or system-wide conventions for the operating mode.

### 7.1 Operation journal

Each mutating operation should have a durable record containing, as appropriate:

- A unique operation identifier.
- Canonical operation name.
- Requested target version.
- Previous and intended versions.
- Installation method.
- Start and completion timestamps.
- Current operation phase.
- Relevant validation results.
- Backup or recovery references.
- Final result and error classification.

The journal must not contain secrets, authentication tokens, or unnecessary sensitive data.

### 7.2 Interrupted operations

If Emrac exits unexpectedly during an upgrade, a subsequent `check` or `diagnose` should detect the incomplete operation.

The service should not automatically repeat a potentially destructive action without determining the current state first.

It should inspect the executable, operation journal, installation ownership, and any staged artifact to determine whether the operation:

- Never began.
- Failed before activation.
- Activated the new version but did not finish verification.
- Completed successfully but failed to record completion.
- Reached an ambiguous state that requires user intervention.

Recovery behavior must be operation-specific. A generic "retry everything" mechanism is unsafe.

## 8. Concurrency and transaction safety

Only one incompatible Emrac-SMS mutation should operate on the same installation and state directory at a time.

For example, `sync` and `upgrade` should not simultaneously replace metadata or migrate shared state without coordination.

The service should implement an operation lock with the following properties:

- Lock acquisition is atomic.
- The lock identifies the owning operation.
- A second conflicting operation fails clearly or waits only when explicitly configured.
- A stale lock is not removed solely because its timestamp is old.
- Recovery inspects the associated operation record before releasing an abandoned lock.
- Lock release occurs reliably on normal completion.
- Crashes are detected through the operation journal rather than assumed to be clean failures.

The lock's scope should match the resources being protected. A per-user Emrac installation should not unnecessarily block an independent system-wide installation.

For filesystem mutations, prefer staging and atomic replacement where supported. If the target filesystem or installation method cannot provide atomic activation, the limitation must be documented and a recovery strategy established.

## 9. Security and trust model

Because Emrac-SMS can replace executable code, its security requirements are stricter than those of a read-only package browser.

### 9.1 Release authenticity

Before installing a release, the service should verify the artifact against a trusted release manifest and use cryptographic signature verification where the release infrastructure supports it.

Checksums detect corruption but do not independently establish authenticity if an attacker can replace both the artifact and its checksum.

The implementation should validate:

- The trusted identity of the release source.
- Manifest authenticity.
- Artifact integrity.
- Version and platform compatibility.
- Artifact type and expected executable layout.
- Any required metadata or state migrations.

An unverifiable artifact must not be installed automatically.

### 9.2 Privilege boundaries

Emrac-SMS should run with the least privilege necessary.

- `news`, `check`, and `diagnose` should ordinarily run without elevated privileges.
- `sync` should not require root merely to refresh per-user metadata.
- Upgrades and downgrades should request elevation only when required by the installation method.
- Privileged operations must not execute arbitrary scripts or commands derived from untrusted metadata.
- User-writable configuration must not be trusted as privileged executable instructions.

### 9.3 Source and release channels

If stable, beta, or nightly channels are implemented, their trust policy must be explicit. A channel change must not silently weaken signature validation or artifact verification.

The default channel should be conservative, with prereleases opt-in unless Emrac's release policy establishes another default.

### 9.4 Failure and security reporting

Errors should explain the failure without leaking secrets. When an artifact fails signature or integrity verification, the service should identify the failed check, refuse installation, and retain enough non-sensitive evidence for diagnosis.

## 10. Error model and exit codes

Emrac-SMS should provide predictable errors for scripting and automation. Human-readable messages should explain what failed and what the user can do next.

The following exit-code allocation is a proposed convention and should be reconciled with Emrac's global CLI error model before implementation.

| Exit code | Meaning |
|---|---|
| `0` | Operation succeeded, or a read-only check completed with no findings requiring a nonzero result |
| `1` | General operation failure |
| `2` | Invalid command, argument, or option combination |
| `3` | Configuration or persistent-state error |
| `4` | Network or release-source failure |
| `5` | Artifact authenticity or integrity failure |
| `6` | Unsupported operation or installation method |
| `7` | Permission failure |
| `8` | Compatibility or migration blocker |
| `9` | Conflicting operation or lock acquisition failure |
| `10` | Recovery required or operation outcome is ambiguous |

The final implementation should use a single source of truth for these codes. It should not assign different meanings to the same code in `sync`, `upgrade`, and `downgrade`.

For diagnostic commands, the CLI should also distinguish a successful diagnostic run that found problems from a diagnostic process that itself failed. If the project adopts separate exit codes for warnings or health findings, those codes must be documented and stable.

Example:

```
Error: downgrade cannot proceed safely.

Reason:
  The requested release is incompatible with the current state schema.

No executable replacement was performed.

Suggested next step:
  Run 'emrac self diagnose' and inspect the migration requirements.
```

## 11. Configuration and release policy

The service should have a documented configuration model for release sources, channel selection, metadata freshness, and installation policy.

Potential settings include:

| Setting | Purpose |
|---|---|
| Release channel | Select stable or an explicitly supported prerelease channel |
| Metadata source | Identify the trusted release metadata source |
| Metadata freshness | Define when cached metadata is considered stale |
| Installation strategy | Select or constrain supported self-update mechanisms |
| Downgrade policy | Define permitted targets and required safeguards |
| Backup policy | Define which state is backed up before a transition |
| Network policy | Control whether operations may contact remote sources |
| Diagnostic verbosity | Control the detail of reports without exposing secrets |

These are proposed configuration capabilities. The exact file format and defaults should be defined in Emrac's configuration specification.

Configuration validation should occur before a mutation begins. Invalid or conflicting settings should produce an actionable error rather than silently falling back to unsafe defaults.

## 12. Observability and reporting

All operations should produce consistent summaries, including read-only operations where useful.

For mutating operations, the summary should identify:

- The canonical operation.
- Whether the operation was a dry run.
- The initial and target states.
- The major completed phases.
- Whether persistent changes were applied.
- Whether verification passed.
- Whether recovery is required.
- Where to find the relevant operation record, if one exists.

Progress output must distinguish planned actions from completed actions. A dry run must never report a simulated activation as though the new executable had been installed.

A future structured-output mode could support machine-readable reports, but the initial interface can remain human-readable. If JSON output is later added, diagnostic messages and progress output must not contaminate the JSON stream.

## 13. Interaction with other Emrac subsystems

Emrac-SMS should be independent of host package resolution while remaining integrated with shared infrastructure.

| Subsystem | Relationship to Emrac-SMS |
|---|---|
| CLI parser | Resolves commands, aliases, options, and arguments |
| Configuration subsystem | Supplies trusted settings and validates configuration |
| Metadata subsystem | Provides release manifests and cached metadata |
| Storage subsystem | Manages local state, operation records, and backups |
| Logging subsystem | Records operation progress and failures |
| Package-manager adapters | Execute installation-method-specific updates |
| Build subsystem | May build a release from source where explicitly supported |
| Diagnostic engine | Performs checks shared by `check` and `diagnose` |
| Recovery subsystem | Supports recovery from interrupted or failed transitions |

The self-management subsystem should consume shared services instead of implementing separate versions of configuration parsing, logging, metadata validation, or package-manager detection.

This also keeps Emrac-SMS testable without requiring every test to modify the host operating system.

## 14. Testing and acceptance criteria

Emrac-SMS should be tested at the command-parser, service, filesystem, installation-adapter, and end-to-end levels.

### 14.1 Command parsing tests

The parser must verify that:

- `emrac self --help` works without network access.
- `up` and `upgrade` resolve to the same canonical operation.
- `dn` and `downgrade` resolve to the same canonical operation.
- Aliases accept the same supported options as their canonical commands.
- Unknown commands and invalid arguments return documented errors.
- `--dry-run` is rejected for operations where it is not supported, unless the CLI explicitly defines it as a harmless no-op.
- Command aliases never bypass authorization, validation, or safety checks.

### 14.2 Dry-run tests

Dry-run behavior must be tested by comparing filesystem and installation state before and after execution.

Required cases include:

- A valid upgrade plan.
- A valid downgrade plan.
- A synchronization plan with stale metadata.
- Missing network access.
- Invalid configuration.
- Untrusted release metadata.
- Insufficient permissions.
- An incomplete prior operation.
- A target release that is incompatible with local state.

The tests must verify that the operation does not apply its intended mutations. Any permitted side effects, such as temporary files or diagnostic logs, must be explicitly accounted for.

### 14.3 Lifecycle tests

Upgrade and downgrade tests should cover:

- Successful activation and post-operation verification.
- Download interruption.
- Invalid checksums or signatures.
- Insufficient disk space.
- Executable replacement failure.
- Migration failure.
- Process interruption at every critical phase.
- Concurrent mutation attempts.
- Recovery after an ambiguous result.
- Installation methods that do not support automated self-updates.

The test suite should use temporary installations, fixtures, mock package-manager adapters, and simulated release metadata rather than modifying the developer's active Emrac installation.

### 14.4 Acceptance criteria

The initial implementation is acceptable when all of the following hold:

1. All canonical commands have documented and tested behavior.
2. Aliases resolve to identical underlying operations.
3. `sync` does not implicitly upgrade the executable.
4. `check` and `diagnose` do not perform automatic repairs.
5. Dry runs do not apply the requested mutations.
6. Release artifacts are validated before installation.
7. Installation ownership is respected.
8. Unsafe downgrades are rejected before executable replacement.
9. Interrupted operations can be detected.
10. Errors and exit codes are consistent.
11. Network failures do not corrupt valid cached state.
12. The service can be tested without root privileges for operations that do not require them.

## 15. Recommended implementation phases

A staged implementation reduces the risk of building the upgrade mechanism before the metadata and state foundations are reliable.

**Phase 1: CLI foundation.** Implement command parsing, canonicalization, help output, shared error handling, and `check`. Exit condition: every command is parsed consistently, and routine health checks work offline.

**Phase 2: State and metadata.** Implement metadata validation, local state management, atomic synchronization, `sync`, and cached `news`. Exit condition: synchronization is repeatable, validated, and does not replace the executable.

**Phase 3: Diagnostics and safety.** Implement `diagnose`, operation journals, locking, artifact verification, and dry-run planning. Exit condition: the service can detect inconsistent state and reliably refuse unsafe plans.

**Phase 4: Upgrade.** Implement release selection, installation-method detection, staged activation, post-upgrade verification, and recovery. Exit condition: supported installations can upgrade through a validated, recoverable workflow.

**Phase 5: Downgrade.** Implement explicit target selection, state-compatibility checks, backup policies, and safe restoration. Exit condition: incompatible or unrecoverable downgrade plans are rejected before destructive changes.

The phases are a recommended implementation order, not a claim about the current state of Emrac's codebase.

## 16. Final canonical specification

**Subsystem:** Emrac self-management\
**Namespace:** `emrac self`\
**Primary responsibility:** Manage Emrac's own metadata, operational health, installation lifecycle, and recovery.

### Canonical commands

| Command | Contract |
|---|---|
| `emrac self --help` | Display self-management help |
| `emrac self sync` | Synchronize Emrac-managed metadata and state |
| `emrac self sync --dry-run` | Preview synchronization without committing changes |
| `emrac self news` | Display release announcements and project news |
| `emrac self check` | Perform routine, non-destructive health checks |
| `emrac self diagnose` | Perform detailed diagnostics |
| `emrac self up` | Alias for `emrac self upgrade` |
| `emrac self up --dry-run` | Preview an upgrade |
| `emrac self upgrade` | Upgrade Emrac through its supported installation mechanism |
| `emrac self dn` | Alias for `emrac self downgrade` |
| `emrac self dn --dry-run` | Preview a downgrade |
| `emrac self downgrade` | Downgrade Emrac through its supported installation mechanism |

### Mandatory invariants

1. `up` and `upgrade` share one implementation.
2. `dn` and `downgrade` share one implementation.
3. Synchronization does not implicitly upgrade Emrac.
4. Routine checks and diagnostics do not automatically repair the installation.
5. Dry-run mode does not commit the requested mutations.
6. Installation ownership is respected.
7. Untrusted artifacts are never installed automatically.
8. Incompatible downgrades are rejected before destructive changes.
9. Mutating operations are coordinated through locking and operation records.
10. Failures must preserve recoverable state wherever the installation mechanism permits.
11. The subsystem must operate without a background daemon.
12. All commands must produce consistent errors and documented exit codes.

### Architectural principle

Emrac-SMS manages Emrac itself. It does not replace the host distribution's package manager, and it does not implicitly perform system-wide package upgrades. It coordinates Emrac's own lifecycle through validated plans, explicit installation ownership, and recoverable operations.

The defining design decision is to make Emrac-SMS an orchestration and safety layer, not another package manager inside Emrac. It should determine what needs to happen, validate whether it is safe, delegate to the correct mechanism, and verify the result. That separation keeps the feature compatible with Emrac's broader package-management architecture.

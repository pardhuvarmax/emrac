# REA Research Plan for Emrac

Goal: use REA (Reverse Engineer Anything) to study how other package managers do things well, so Emrac can implement its own better versions. This is research, not integration: REA never becomes part of Emrac, and Emrac stays deterministic with no AI in its runtime.

## 1. Ground rules

- **Read first, reverse engineer second.** Most package managers are open source with docs, man pages, and tests. Read those and run the tool in a container first. Use REA only for behavior that is binary-only, undocumented, or unclear from the source.
- **Findings are hypotheses.** Every REA result must be confirmed by a test that passes or fails: run the same inputs through the real tool and through Emrac's Rust code and compare.
- **Implement from behavior, not from code.** Many of these tools are GPL (pacman, apt, dnf, zypper, Portage). Write the behavior down in your own words, then implement. Do not copy reconstructed code. Record where each idea came from.
- **Keep Emrac deterministic.** REA output informs a design note. It never decides anything at Emrac runtime.
- **Check licenses and local law** before analyzing any proprietary software. REA is an analysis aid, not permission.

## 2. Safe setup

1. Create a throwaway VM or container (one per tool family) with no SSH keys, tokens, or personal files. In particular, keep the AUR SSH key out of this environment.
2. Install REA only from official sources: `github.com/morluto/rea`, `rea.tools`, or the `rea-agents` npm package. A fake, trojanized copy has been reported, so verify the source.
3. Requirements: Node.js 22.19 or newer in the 22 line, 24.11 or newer, or 26 and up (23 and 25 are excluded). For native binaries, install a provider such as Ghidra, Hopper, or IDA.
4. Run `npx rea-agents@latest setup`. It registers the MCP server and the agent skill with your coding agent.
5. Build or install the target tool with debug symbols where possible. Output on stripped binaries is much harder to read.
6. Take a VM snapshot before starting so you can reset.

## 3. Research workflow per feature

1. **Question.** Write the one behavior you want to understand.
2. **Docs and source first.** Man pages, design docs, source, test suite.
3. **Observe.** Run the scenario in a container with `strace` (files, locks, ordering) and capture output.
4. **REA, only where needed.** Use the commands in section 4.
5. **Confirm.** Write a test that compares real tool output against your hypothesis. Keep it.
6. **Design note.** Describe the behavior in your own words, then the Emrac version: commands, failure cases, what is deliberately different.
7. **Decide.** Does it make the worst day safer or the everyday workflow faster? If neither, drop it.

## 4. Investigations

Command format (argument order as used in your REA setup; confirm each argument's meaning by running the prompt with no arguments):

```
/mcp__rea__investigate_feature [feature] [target_path] [document] [procedure] [provider_id]
```

Use `command -v <tool>` and `readlink -f` inside each container to get the real `target_path`. Paths below are examples and differ by distribution.

### R1. Version comparison (Wandrover)

```
/mcp__rea__investigate_feature "How vercmp compares versions: epoch, pkgver, pkgrel, letters vs numbers, ~ and ^" /usr/bin/vercmp research/vercmp.md "Find the comparison routine, trace each branch, output a truth table of examples, do not reproduce code" ghidra

/mcp__rea__investigate_feature "dpkg version comparison and status-file parsing, including epochs and ~ ordering" /usr/bin/dpkg research/dpkg-versions.md "Document the comparison algorithm and parser fields as behavior, with test cases" ghidra

/mcp__rea__investigate_feature "RPM version comparison (rpmvercmp) and database field layout" /usr/bin/rpm research/rpm-versions.md "Document the algorithm and edge cases as behavior, with test cases" ghidra
```

Best test: generate thousands of version pairs and compare each tool's answer to Emrac's Rust implementation.

### R2. Transaction commit order and locking (crash-safe journal, C1 and C6)

```
/mcp__rea__investigate_feature "Which files pacman locks, writes and syncs during a transaction, and in what order" /usr/bin/pacman research/pacman-commit-order.md "Trace the commit path from lock acquisition to database write, list every file operation in order, note failure paths" ghidra
```

Cross-check with `strace -f -e trace=file,fsync,flock pacman ...` in a container.

### R3. Hooks and scriptlets

```
/mcp__rea__investigate_feature "When libalpm runs hooks and scriptlets, with what environment, and what happens when one fails" /usr/lib/libalpm.so research/alpm-hooks.md "Locate hook discovery and execution, record trigger types and ordering, record failure handling" ghidra
```

### R4. Solver explanations and conflict resolution (conflict explainer, B4)

```
/mcp__rea__investigate_feature "How the dependency solver reports a conflict and proposes ranked solutions" /usr/bin/aptitude research/aptitude-resolver.md "Find the resolver search and scoring, record how solutions are ordered and presented, no code reproduction" ghidra

/mcp__rea__investigate_feature "How libsolv finds problems and builds solution lists (used by dnf and zypper)" /usr/lib64/libsolv.so research/libsolv-problems.md "Trace problem-rule discovery and solution generation, record the data it exposes" ghidra
```

### R5. Selective undo and generations (history, rollback)

```
/mcp__rea__investigate_feature "How dnf history records a transaction and how undo decides what it can reverse" /usr/bin/dnf5 research/dnf-history-undo.md "Document the stored record fields and the undo conflict checks as behavior" ghidra
```

### R6. Temporary build dependencies (removable makedepends)

```
/mcp__rea__investigate_feature "How apk --virtual groups packages in the world file and removes them in one step" /sbin/apk research/apk-virtual.md "Document how a virtual package is stored and how deletion resolves its members" ghidra
```

### R7. Closure size and generation storage

```
/mcp__rea__investigate_feature "How nix computes closure size (path-info -S) and stores profile generations" /path/to/nix research/nix-closure-generations.md "Document the traversal and the generation record layout as behavior" ghidra
```

### R8. Permission display model

```
/mcp__rea__investigate_feature "How flatpak merges app permissions and user overrides into the effective permission set" /usr/bin/flatpak research/flatpak-permissions.md "Document the merge order and the displayed permission categories" ghidra
```

Tools that are scripts (checkupdates, apt-listbugs, apt-listchanges, Spack) need no REA. Read the source.

## 5. Research log template

One file per feature, kept in `research/`.

```
# <feature name>
Tool and version:
Question:
Docs and source read:
Observations (strace, runs):
REA command used:
REA evidence ids and coverage notes:
Confirming test (inputs, expected, actual, file path):
Behavior in my own words:
Emrac design: commands, failure cases, deliberate differences:
Source of idea (for credit and license tracking):
Status: hypothesis / confirmed / rejected
```

## 6. Suggested order

1. R1 version comparison (easy to confirm with generated tests, used by Wandrover).
2. R2 and R3 commit order, locking, and hooks (feeds the critical features in `additions.md` Part C).
3. R4 solver explanations (feeds the conflict explainer).
4. R5 to R8 as time allows.

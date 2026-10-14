# Emrac Wandrover

**Cross-distribution package discovery and resolution**

Find, correlate, and resolve packages across Linux distributions using deterministic, explainable algorithms.

CLI identity: `emrac wandrover`

**No AI or LLMs.** Package discovery and resolution rely on deterministic metadata analysis, explicit mappings, dependency and capability relationships, and explainable ranking.

Feature document for Emrac. Status: `[idea]`, not yet in `SPEC.md`.

Wandrover is Emrac's cross-distribution package intelligence subsystem, designed to navigate the Linux package ecosystem and translate package intent into actionable Arch Linux solutions.

An Arch Linux user searches for a package from Ubuntu, Debian, Fedora, openSUSE, or another distribution. Emrac determines which Arch package provides the same software, a compatible replacement, or the closest functional alternative, and shows the evidence for each answer.

This is not a package-name translator. Package names, package boundaries, dependencies, and even the default implementation of common tools differ between distributions.

---

## 1. Principles

- **Deterministic.** Metadata, explicit mapping rules, package capabilities, and a rule-based ranking engine. No LLMs, embeddings, semantic inference, cloud AI APIs, or probabilistic recommendations. The same query against the same index always returns the same result.
- **Explainable.** Every result carries the exact reasons it was chosen. No opaque scores.
- **Conservative.** "No verified match" is a valid answer. False equivalence is worse than no result.
- **Offline.** Queries run against a local index. Network access is only needed to refresh metadata and mapping data.
- **Native metadata is the source of truth for availability.** Hand-reviewed equivalence rules live in a separate layer, so upstream metadata changes cannot silently corrupt them.
- **Never installs automatically.** Even a high-confidence alternative is shown, explained, and chosen by the user, then passed through Emrac's normal transaction planner.

---

## 2. Command line: `emrac wandrover`

| Subcommand | Purpose |
|---|---|
| `emrac wandrover search` | Search packages across distributions |
| `emrac wandrover resolve` | Identify the best corresponding Arch Linux package |
| `emrac wandrover alternatives` | Discover functionally equivalent packages and substitutes |
| `emrac wandrover map` | Inspect cross-distribution package mappings, relationships, and provenance |
| `emrac wandrover audit` | Validate mapping accuracy, source provenance, and package safety |

How the subcommands relate to the architecture in section 4:

| Subcommand | Mainly uses |
|---|---|
| `search` | Layers 1 and 2: universal input and the local cross-distro index |
| `resolve` | Layers 3 to 5: identity and capability matching, Arch candidate ranking, explainable result |
| `alternatives` | Functional-alternative discovery (matching rule 5, algorithm area E) with capability-overlap explanations |
| `map` | The relationship graph and the explicit mapping database, including evidence and provenance |
| `audit` | Mapping regression checks, provenance verification, and package-safety signals |

Illustrative invocations (flag names are not final):

```
emrac wandrover search fd-find --from debian
emrac wandrover resolve fd-find --from debian
emrac wandrover alternatives file-roller
emrac wandrover map debian:fd-find
emrac wandrover audit --mappings
```

Like every Emrac capability, Wandrover is available in both the CLI and the TUI, and `--json` returns the relationship type, evidence list, and confidence for scripting.

### What `audit` checks

- **Mapping accuracy.** Re-validate each mapping against current repository and upstream metadata, and flag mappings that have become invalid (renamed, split, merged, or removed packages).
- **Source provenance.** For each mapping and index record: where it came from, which source and version of the data, who reviewed it, when, and whether signed mapping data verifies.
- **Package safety.** For candidates Wandrover would recommend, surface safety signals before anything is installed, such as AUR trust signals (maintainer changes, orphaned status, package age) and static PKGBUILD analysis findings (see `ideas.md`). Audit never executes package code.

### What `map` shows

For a package, the typed relationships it participates in (same upstream, renamed, split, variant, alternative), the evidence behind each edge, the provenance of the mapping, and its confidence and review status.

---

## 3. Intent matters

A query can mean different things, and Emrac must not conflate them.

Example: a user searches for `apt`. They might want:

1. the actual Debian package manager,
2. the Arch tool that does the equivalent job, or
3. a graphical package installer.

These are three different intents. When a query is ambiguous, Emrac presents the intents separately instead of picking one.

---

## 4. Architecture

Five layers, so Emrac can explain why a package is recommended.

| Layer | Responsibility | Inputs / outputs |
|---|---|---|
| 1. Universal search input | Accept a package name, command, description, feature, or foreign distro package | Query, optional source distro |
| 2. Cross-distro metadata resolver | Names, descriptions, upstream project, versions, provides, dependencies, files | Local index |
| 3. Identity and capability matcher | Same software? Same functionality? Different implementation? | Typed relationships |
| 4. Arch candidate ranker | Official repository first, then AUR, with evidence-based confidence | Ordered candidates |
| 5. Explainable results | Exact match, Equivalent, Alternative, Variant, No reliable match | Result plus evidence |

Resolution pipeline:

```
Foreign package query
        |
        v
Normalize name and metadata
        |
        v
Exact and alias lookup ----> verified identity? --yes--> Arch candidates --+
        | no                                                              |
        v                                                                 |
Resolve Arch candidates                                                   |
        |                                                                 |
        v                                                                 |
Compare upstream identities                                               |
        |                                                                 |
        v                                                                 |
Match provides, files, capabilities                                       |
        |                                                                 |
        v                                                                 |
Analyze package splits and variants                                       |
        |                                                                 |
        v                                                                 |
Search curated alternatives                                               |
        |                                                                 |
        v                                                                 |
Rank by evidence and compatibility <--------------------------------------+
        |
        v
Validate availability in enabled repositories
        |
        v
Explainable results
```

Not every stage has equal authority. A verified identity mapping outranks inferred capability similarity. A package that merely shares a command name must not be mistaken for the same application.

---

## 5. Cross-distribution package index

Emrac does not scrape package websites per query. It maintains a searchable local index, refreshed periodically from upstream metadata.

### Sources

| Source | Provides |
|---|---|
| Arch repositories | Official package databases and metadata ([Arch package search](https://archlinux.org/packages/), supports JSON) |
| AUR | RPC metadata, PKGBUILDs, `.SRCINFO` where needed ([AUR RPC interface](https://wiki.archlinux.org/title/Aurweb_RPC_interface)): descriptions, dependencies, provides, conflicts, replaces |
| Debian / Ubuntu | `Packages` indexes and package relationship fields |
| Fedora | Repository metadata, including RPM provides and requirements |
| openSUSE | Repository metadata and RPM capabilities |
| Alpine, Gentoo, others | Native package metadata |
| [Repology](https://repology.org/) | Optional: cross-repository package identity and version comparison |

Repology is a good starting point for discovering candidate identities, but it is not an authoritative database of functional alternatives. Version tracking and software equivalence are different problems. Repology itself maintains rename, merge, and split rules to reconcile packages with different names, which shows why explicit mappings matter.

### Record schema

```
PackageIdentity
  distro
  repository
  package_name
  version
  description
  upstream_project
  homepage
  source_package
  provides[]
  dependencies[]
  executables[]
  shared_libraries[]
  capabilities[]
  conflicts[]
  metadata_timestamp
```

`upstream_project` is the key field. Two distributions can both ship a package called `foo` that is different software; different package names can refer to the same upstream project. Name equality alone proves nothing.

### Storage and updates

- SQLite for the local metadata and mapping database.
- Inverted indexes for package names, executables, capabilities, and upstream identifiers.
- Hash maps for exact aliases and canonical identifiers.
- A graph representation for relationships.
- Incremental updates instead of rebuilding every distribution's index.
- Versioned, signed mapping data for reproducible results.

---

## 6. Relationship model

A typed relationship graph, not a flat list of suggestions.

| Relationship | Meaning | Example |
|---|---|---|
| `same_upstream` | Same underlying software | Firefox on Debian and `firefox` on Arch |
| `renamed_package` | Different distro package names, same software | Debian `fd-find` and Arch `fd` |
| `split_package` | One distro package maps to several Arch packages | Runtime and development files packaged separately |
| `capability_match` | Provides the requested command or library | An Arch package providing a given executable |
| `functional_alternative` | Different software serving a similar purpose | One PDF reader and another |
| `variant` | Different edition, backend, or implementation | GTK versus Qt builds |
| `no_match` | No sufficiently reliable equivalent | Report it; do not invent a match |

User-facing result labels: **Exact match**, **Rename**, **Split**, **Equivalent**, **Alternative**, **Variant**, **No reliable match**.

Edges carry explicit type, evidence, provenance, and confidence. `same_upstream` is fundamentally different from `functional_alternative`, and traversing one must never silently imply the other. Evidence is versioned: a mapping can become invalid when a package is renamed, a feature disappears, or a distribution changes how it splits packages.

### Graph node types

- `Package`: a distribution-specific package record
- `UpstreamProject`: the underlying software project
- `Capability`: a command, library, protocol, or feature
- `Repository`: the source that provides the package
- `PackageGroup`: a curated collection of related packages

---

## 7. Matching rules

The resolver tries these in order and stops only when the relationship is sufficiently established.

| Priority | Method | Example |
|---|---|---|
| 1 | Explicit cross-distro mapping | Debian `fd-find` to Arch `fd` |
| 2 | Shared upstream project identifier | Same application, different package names |
| 3 | Exact executable or library provider | Find the package supplying a required command |
| 4 | Declared capabilities and virtual provides | Match by a capability the package provides |
| 5 | Curated alternatives database | Map one application to a reviewed substitute |
| 6 | Exact normalized name | Match names after conservative normalization |
| 7 | Description and category rules | Suggest candidates for manual inspection only |

A lower-priority match must never override a stronger, verified mapping. Description similarity never establishes equivalence on its own.

---

## 8. Explicit mapping database

Reliable alternatives come from version-controlled, reviewed data, not generated guesses.

```toml
[[mapping]]
source_distro = "debian"
source_package = "fd-find"
target_distro = "arch"
target_package = "fd"
relationship = "renamed_package"
confidence = "verified"
evidence = "same_upstream_project"

[[mapping]]
source_distro = "debian"
source_package = "example-reader"
target_distro = "arch"
target_package = "example-reader-gtk"
relationship = "variant"
confidence = "reviewed"
```

The second record is fictional and illustrative. Real mappings must be validated against authoritative package and upstream metadata.

The schema should also support one-to-many and many-to-one mappings, architecture restrictions, version ranges, and mapping provenance.

---

## 9. Algorithms

Package correlation is a multi-stage engine. Each algorithm handles one kind of relationship and contributes evidence.

| Area | What it does | Algorithms |
|---|---|---|
| A. Identity resolution | Normalize names, resolve aliases, compare upstream URLs, source package identifiers, rename histories | Canonicalization, hash indexes, union-find for verified identity groups |
| B. Relationship graph | Typed edges: same upstream, renamed, split, variant, provides, conflicts, alternative | Graph traversal, connected components, bounded path search, relationship propagation |
| C. Capability matching | Compare executables, libraries, protocols, file formats, declared features, dependency interfaces | Inverted indexes, set intersection, weighted Jaccard similarity, bipartite matching |
| D. Structure correlation | Decide whether a foreign package maps to one Arch package, several, or part of one | Bipartite matching, set cover, dependency closure analysis |
| E. Alternative discovery | When identity fails, compare capabilities, interfaces, categories, curated substitution rules | Feature-vector comparison, weighted similarity, rule engines, Pareto ranking |

### Mathematical models

**Identity matching.** Explicit identifiers and exact comparisons only. Fuzzy name matching may discover candidates but never establishes identity.

**Capability similarity.** Each package is a set of capabilities, for example:

- A: `extract-zip`, `extract-tar`, `gui`, `file-manager-integration`
- B: `extract-zip`, `extract-tar`, `gui`
- C: `extract-zip`, `extract-tar`, `create-archives`, `cli`

Jaccard similarity is the baseline:

```
J(A, B) = |A ∩ B| / |A ∪ B|
```

A weighted variant gives core capabilities more importance than optional integrations. A high score indicates similarity, not guaranteed substitutability.

**Package splitting.** Let F be the set of files or capabilities of a foreign package, and A1..An the sets for Arch candidates. The resolver looks for combinations whose union covers F, a weighted set-cover problem. It must distinguish runtime requirements from optional documentation, development headers, translations, and debug symbols.

---

## 10. Ranking

Constraints first, scores second.

1. Reject incompatible architectures and candidates unavailable in enabled repositories.
2. Prefer verified identity matches.
3. Prefer complete required-capability coverage.
4. Rank the remainder by compatibility, maintenance status, and repository provenance (official repositories before AUR).
5. When no candidate dominates, present the trade-offs.

A candidate that fails a required compatibility check must not win by accumulating points elsewhere, so hard eligibility rules and lexicographic ordering are preferred over a single combined score.

Every ranking exposes its reasons. Illustrative trace (numbers are placeholders):

```
Candidate: fd
  +100  Explicit verified package mapping
  +50   Same upstream project
  +30   Required executable is provided
  -100  Candidate unavailable in enabled repositories   (not triggered)

Result: Eligible direct equivalent
```

If an additive score is used at all, it is for explanation and tie-breaking, for example:

```
S = 0.40*U + 0.25*C + 0.15*M + 0.10*D + 0.10*P
  U upstream identity, C capability overlap, M curated mapping evidence,
  D description similarity, P packaging metadata agreement  (each 0..1)
```

These weights are a starting point, not validated coefficients. A verified upstream match must outweigh a high description-similarity score.

**Match confidence is separate from package quality.** A candidate can be a certain identity match with a stale AUR package; another can be a well-maintained alternative that is not the same application. Report both.

---

## 11. Capability taxonomy

Alternatives without AI depend on curated data. Emrac maintains a capability taxonomy, for example:

```
archive-manager
  |- create-archives
  |- extract-archives
  |- graphical-interface
  |- desktop-integration
  `- supported-formats
```

Applications declare the capabilities they provide, and a rule engine compares the declarations and returns alternatives with a capability-overlap explanation. Unknown or poorly documented packages return "no verified match" rather than a fabricated substitute.

---

## 12. Query modes

| Mode | Question it answers |
|---|---|
| Forward | What is the Arch equivalent of this Debian package? |
| Reverse | What packages in other distributions correspond to this Arch package? |
| Capability | Which Arch package provides this command, library, or feature? |
| Alternatives | What can replace this application? |
| Migration | Import a package list from another distribution and generate a reviewed Arch migration plan |

Mapping to subcommands: forward lookups use `resolve`, replacement discovery uses `alternatives`, and capability and reverse lookups start from `search`. Inspecting why a mapping exists is `map`.

Migration mode is especially useful for developers, distro hoppers, system administrators, and people moving complex development environments. It also needs dependency closure analysis so the generated plan is complete. It has no subcommand of its own yet (see open questions).

---

## 13. What the user sees

### Clear rename

Query: `fd-find` (source: Debian/Ubuntu)

```
fd    Arch official repositories
  Same software. Debian uses a different package name for the same upstream utility.
  Relationship: renamed package    Confidence: high (verify against current metadata)
  Actions: inspect package, install fd
```

Illustrative. Results must be verified against current repository metadata before installation.

### Ambiguous alternatives

A Debian package providing a graphical archive manager could show several Arch candidates, such as File Roller and Ark, with desktop integration and supported archive formats explained. They are labeled **Alternative**, never "same software".

### No match

```
No verified match found for "example-tool".
Closest candidates (for manual inspection only, not equivalents):
  ...
```

---

## 14. Integration with the rest of Emrac

- The chosen Arch package goes through the ordinary transaction planner (Planned, Previewed, Confirmed, Executing, Committed). The resolver never bypasses it.
- Evidence shown in the result view should be reachable from `emrac why` / `explain` style output.
- Like all Emrac features, the CLI and TUI expose the same functionality (SPEC design principle).
- `--json` output carries the full relationship type, evidence list, and confidence for scripting.
- Commands live under `emrac wandrover` (section 2). `SPEC.md` does not define them yet.
- The mapping repository should be updatable independently of Emrac releases, with signed data.

---

## 15. Implementation order

| Phase | Deliverable |
|---|---|
| 1 | Cross-distro metadata ingestion and normalized schema |
| 2 | Exact identity, alias, and rename resolution |
| 3 | Executable, library, and virtual-capability lookup |
| 4 | Relationship graph and split-package detection |
| 5 | Deterministic functional-alternative ranking |
| 6 | Migration planning, dependency closure, and explainable reports |

Build the identity resolver first, then capability matching, then functional alternatives. The first two rely mostly on structured metadata; the third needs curated relationships and more cautious ranking. Keeping them separate makes recommendations more trustworthy and easier to debug.

---

## 16. Testing and quality

- Build a corpus of known cross-distro mappings and ambiguous cases.
- Measure precision separately for exact equivalents, variants, and functional alternatives.
- Treat false equivalence as the most serious failure; prefer returning no result.
- Regression-test mapping records against current repository metadata, and flag mappings that become invalid (renamed or split packages).
- Verify reproducibility: same index plus same query gives the same output.

---

## 17. Open questions

- Who curates and reviews the mapping and capability-taxonomy repositories, and how are contributions validated?
- How large an initial mapping set is needed before the feature is useful (which distributions and package ranges first)?
- How are executables and shared-library lists obtained for foreign distributions without downloading full packages (file lists, `Contents` indexes, RPM file metadata)?
- Cache size and refresh cadence per distribution.
- Which license and trust model apply to Repology-derived data and the community mapping repository?
- Should Wandrover be part of the core binary or an optional module loaded through the plugin interface?
- Where does migration mode live: an option on `resolve` (for example reading a package list from a file), or its own subcommand?
- Exact scope of `audit` package-safety checks, and how they share code with Emrac's ordinary AUR trust and PKGBUILD analysis features.

---

## Sources

- Repology: https://repology.org/
- Arch Linux package search and API: https://archlinux.org/packages/
- AUR RPC interface: https://wiki.archlinux.org/title/Aurweb_RPC_interface

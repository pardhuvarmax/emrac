Feature Status
================

Every category in ``SPEC.md`` Parts III, IV and V, checked against
what is actually shipped as of **Slice i1 -> r1** (commit
``196141a``). The core CLI loop -- ``search`` / ``info`` / ``install``
/ ``remove`` / ``upgrade``, official repos + AUR -- is complete;
everything else is scoped for later by the spec itself.

MVP Core Loop
--------------

``SPEC.md`` Part X -- the i1 -> r1 bar. All five commands work
against both official repos (libalpm/pacman) and the AUR (RPC v5 +
git/makepkg). This is the milestone the whole slice was scoped
around, and it is finished.

.. list-table::
   :header-rows: 1
   :widths: 14 12 74

   * - Command
     - Status
     - Notes
   * - ``search``
     - Done
     - Official + AUR, ``--official``/``--aur`` scope, ``--offline``, ``--json``
   * - ``info``
     - Done
     - Official -> AUR fallback, ``--offline``, ``--json``
   * - ``install``
     - Done
     - Official + AUR (git clone/fetch + ``makepkg -si``); PKGBUILD/diff shown by default, ``--skip-pkgbuild``/``--skip-diff`` to opt out
   * - ``remove``
     - Done
     - ``--cascade``/``--recursive`` passed straight to pacman
   * - ``upgrade``
     - Done
     - Full (``pacman -Syu`` + foreign-package AUR scan) and targeted; same PKGBUILD/diff review as install

Milestone complete as of ``196141a``.

Complete Feature Catalog
--------------------------

``SPEC.md`` Part III -- 21 categories.

.. list-table::
   :header-rows: 1
   :widths: 4 26 12 58

   * - #
     - Category
     - Status
     - Notes
   * - 1
     - Unified Package Management
     - Partial
     - install/remove/upgrade + dry-run/preview/confirm done; downgrade, purge, reinstall, verification, rollback, transaction history, resume-interrupted -- not started
   * - 2
     - Source Build System
     - Partial
     - PKGBUILD build via ``makepkg -si`` with a persistent checkout cache; no queue, background/pause/cancel, log collection/replay, PKGBUILD editing/validation
   * - 3
     - Intelligent Recommendation Engine
     - Not started
     - --
   * - 4
     - ETA Prediction Engine
     - Not started
     - --
   * - 5
     - Package Discovery
     - Partial
     - Basic name/description search only; no fuzzy/regex, file-ownership, category browsing, popular/recent
   * - 6
     - Package Inspection
     - Partial
     - ``info`` covers metadata, version, license, homepage, deps, AUR stats; no changelog, installed-files, security advisories, reverse deps
   * - 7
     - Dependency Intelligence
     - Not started
     - No tree/graph/why/impact commands
   * - 8
     - Repository Management
     - Partial
     - Official + AUR only; no local/custom/git repos, mirror management, repo health
   * - 9
     - News & Advisory System
     - Not started
     - --
   * - 10
     - Build Profiles
     - Not started
     - --
   * - 11
     - Compiler & Build Optimization
     - Not started
     - ``makepkg`` defaults only, no flags exposed
   * - 12
     - Build Monitoring
     - Partial
     - Live inherited stdio from makepkg/pacman; no structured progress, stage tracking, summaries
   * - 13
     - Cache Management
     - Partial
     - ``~/.cache/emrac/build/<pkg>`` git cache exists; no explorer, stats, or cleanup commands
   * - 14
     - History & Recovery
     - Not started
     - --
   * - 15
     - Security & Integrity
     - Partial
     - Inherits pacman/makepkg's own signature/checksum verification; nothing emrac-level yet
   * - 16
     - Package Health
     - Not started
     - No ``doctor``/health checks
   * - 17
     - Analytics
     - Not started
     - --
   * - 18
     - Unified Interfaces
     - Partial
     - CLI complete for the 5 MVP commands, ``--json``, ``--dry-run``, ``-y``; no shell completion, no TUI at all
   * - 19
     - Configuration
     - Not started
     - No config file -- reads live ``pacman-conf`` only
   * - 20
     - Performance
     - Partial
     - Fast by construction (Rust + libalpm FFI); no deliberate indexing/caching layer yet
   * - 21
     - Extensibility (Future)
     - Not started
     - Explicitly future-scoped in the spec itself

Signature & Advanced Features
--------------------------------

``SPEC.md`` Part IV -- 20 items. The spec explicitly scopes all of
Part IV for **after** the core CLI loop, so the "not started" rows
here are not slippage, they are on schedule.

.. list-table::
   :header-rows: 1
   :widths: 4 30 12 54

   * - #
     - Feature
     - Status
     - Notes
   * - 1
     - Install Planner
     - Partial
     - Transaction preview (packages, download size, installed-size delta) shown pre-execution; no compile time, conflicts, services, disk-required fields
   * - 2
     - Explain Decisions
     - Not started
     - No ``explain`` command; no source/binary recommendation logic exists yet
   * - 3
     - Package Score
     - Not started
     - --
   * - 4
     - Build Profiles with Inheritance
     - Not started
     - --
   * - 5
     - Build Recipes
     - Not started
     - --
   * - 6
     - Build Diff
     - Partial
     - PKGBUILD diff-on-rebuild is shipped (``AurSyncOutcome::Changed``); spec's ``diff-build`` runtime comparison (binary vs. source perf) is not
   * - 7
     - Build Benchmark
     - Not started
     - --
   * - 8
     - Build Cache Explorer
     - Not started
     - --
   * - 9
     - Timeline
     - Not started
     - --
   * - 10
     - Rollback Snapshots
     - Not started
     - --
   * - 11
     - Interactive Conflict Resolver
     - Not started
     - Conflicts currently just surface as pacman's own error text
   * - 12
     - Dependency Graph Explorer
     - Not started
     - Needs the TUI
   * - 13
     - Package Inspector
     - Partial
     - ``info`` is a basic version of this, not the full aggregated view
   * - 14
     - Repository Analytics
     - Not started
     - --
   * - 15
     - Build Dashboard
     - Not started
     - Needs the TUI
   * - 16
     - Health Center (doctor)
     - Not started
     - --
   * - 17
     - Smart Updates
     - Not started
     - ``upgrade`` exists but has no rule-based skip logic
   * - 18
     - Build Statistics
     - Not started
     - --
   * - 19
     - Package Collections
     - Not started
     - --
   * - 20
     - Workspace Mode
     - Not started
     - --

Totals: 6 done / 15 partial / 25 not started, of 46 tracked.
Everything outside the MVP row is explicitly post-milestone per the
spec -- not slippage.

Additional Tracked Items
---------------------------

Two groups from ``SPEC.md`` that fall outside the numbered Part III
and Part IV tables above, but are still tracked items from the spec.
Neither is implemented yet.

Part IV -- Additional Distinguishing Ideas
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Listed in ``SPEC.md`` Part IV but left unnumbered in the spec, so
they were not folded into the 20-row signature-features table above.

.. list-table::
   :header-rows: 1
   :widths: 22 14 64

   * - Idea
     - Status
     - Notes
   * - Package Advisor
     - Not started
     - Warns on unusually large dependency trees or disk impact before install
   * - Mirror Intelligence
     - Not started
     - --
   * - Package Impact Preview
     - Not started
     - Removal-side impact preview (services, reverse deps, reclaimed space)
   * - Compile Farm Support
     - Not started
     - Remote/SSH build distribution
   * - Build Artifact Library
     - Not started
     - Cross-run reuse of compatible builds by profile/compiler/flags
   * - TUI Command Palette
     - Not started
     - Needs the TUI
   * - Policy Engine
     - Not started
     - Configurable rules, e.g. "always build Rust packages from source"

Part V -- Command Catalog: CLI Plumbing
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Part V is the CLI surface (command names and aliases) for features
already covered in Parts III/IV, not a new feature list. Most groups
map onto an existing row (``repo *`` -> Repository Management,
``cache *`` -> Cache Management, ``queue *`` -> Build Queue, etc.).
These groups are the exception: pure CLI infrastructure with no
corresponding Part III/IV row.

.. list-table::
   :header-rows: 1
   :widths: 40 14 46

   * - Group
     - Status
     - Notes
   * - Shell Integration (``completion``, ``alias``, ``integrate``)
     - Not started
     - No shell completion generation yet
   * - System (``version``, ``about``, ``env``, ``paths``, ``diagnostics``)
     - Not started
     - No ``--version``/introspection commands yet
   * - Development (``dev``, ``sandbox``, ``test``, ``trace``, ``debug``)
     - Not started
     - Internal dev/debug tooling
   * - Export / Import (``export``, ``import``, ``bundle``)
     - Not started
     - --
   * - Help (``help``, ``man``, ``docs``, ``license``, ``credits``, ``bug-report``)
     - Not started
     - Only clap's auto-generated ``--help`` exists today
   * - ``lock``/``unlock``, ``pin``/``unpin`` (under Package Management)
     - Not started
     - Version-pinning, not covered by any Part III/IV row

----

With these two groups included, the running total is **53 tracked
items** (46 from Parts III/IV + 7 Additional Distinguishing Ideas),
plus the 6 CLI-plumbing groups above tracked separately since they
are not product features. The completed count is unchanged: 6 done.

Source: ``SPEC.md`` Parts III, IV, V and X, checked against
``crates/`` and ``CHANGELOG.md`` as of commit ``03e0781``.

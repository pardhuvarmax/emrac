Feature Status
================

Every category in ``SPEC.md`` Parts III, IV and V, checked against
what is actually shipped as of **Slice r3** (release ``r3``, which
includes the updates ``u1`` to ``u20`` built on ``r2``; ``r2`` itself
included ``u1`` to ``u30`` of the cycle before, since the update numbers
start again after every release). The
core CLI loop -- ``search`` / ``info`` / ``install`` / ``remove`` /
``upgrade``, official repos + AUR -- has been complete since ``r1``. Since
then the work has gone into making a real upgrade safe and recoverable:
audited and sandboxed AUR builds, plans that match what pacman will do and
cannot change under you, checks before a transaction and a report after
it, history that can be verified and a ledger root alone can change, a
journal for a transaction that was cut off, trust signals for the AUR and
the Arch news, and emrac updating itself through pacman. ``CHANGELOG.md``
has the detail slice by slice; the TUI, build profiles, the build queue,
and the source-or-binary recommendation engine remain unstarted.

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

Commands Available Today
--------------------------

Everything a user can run, with the slice that introduced it. All of
these were exercised against a real pacman (in a container for anything
that changes the system) as well as unit-tested; ``CHANGELOG.md`` says
what was seen for each.

The ``Since`` column counts update slices per cycle: ``u1`` to ``u30`` on
the older rows (config, history, rollback, audit, sandbox, doctor, pacnew,
cache, ...) are the cycle that ended with ``r2``; the rows from ``history``'s
ledger, ``self``, ``rebuild``, ``news`` and ``report`` onward carry the numbers of the
cycle that ended with ``r3``.

.. list-table::
   :header-rows: 1
   :widths: 24 10 66

   * - Command
     - Since
     - What it does
   * - ``search`` / ``info``
     - i1
     - Official repositories (libalpm) and the AUR (RPC, with retries) behind one interface
   * - ``install`` / ``remove`` / ``upgrade``
     - i1, r1
     - Preview, confirm, run; pacman asks its own questions unless ``--yes`` (u23); plans come from freshly synced repositories (u22) and list conflict removals, replacements and provider choices (u24, u25)
   * - ``build <pkg|dir>``
     - u19
     - Builds an AUR package or a local PKGBUILD folder without installing; sandboxed, with limits (u20)
   * - ``audit <pkg|path|file>``
     - u15-u18
     - Static reading of a PKGBUILD, scriptlets and shipped files; inspects a built package file; runs automatically before AUR builds and again on the built package
   * - ``doctor [--online] [--report]``
     - u21
     - Read-only health checks with a fix for each finding; a sanitized bug report
   * - ``history``
     - u3
     - Append-only log of executed transactions, failures and conflict removals included
   * - ``rollback`` / ``undo`` / ``redo``
     - u4, u5
     - Reverse and re-apply recorded transactions from the pacman cache, the AUR build folder or the Arch Linux Archive
   * - ``downgrade`` / ``reinstall`` / ``purge``
     - u6-u8
     - One package back to an older version; the same version again; remove with its modified configs
   * - ``verify`` / ``check``
     - u9
     - Installed files against pacman's record
   * - ``snapshot`` / ``restore``
     - u10, u11
     - Named restore points of the installed package set; optional automatic one before ``upgrade``
   * - ``deps`` / ``rdeps`` / ``why`` / ``orphans``
     - u12
     - Read-only dependency inspection
   * - AUR trust signals
     - u20
     - Lookalike names, a claim on an official package, orphaned/new/few users, a changed maintainer, sources changed under the same version, a new source host, changed signing keys; shown in the review and in ``info``
   * - ``news`` / ``news ack`` / ``--ack-news``
     - u20
     - The Arch news; unread items under every upgrade plan; critical ones (breaking, manual intervention, kernel, bootloader, C library, systemd, pacman, drivers, filesystem) stop an upgrade until acknowledged
   * - pacman database protection
     - u20
     - A backup of the local database before each transaction, ``pacman -Dk`` before and after, the restore command, the lock in pre-flight
   * - repository trust
     - u20
     - An older version than the installed one is refused in pre-flight; a repository database older than one seen is a warning
   * - update risk / ``report``
     - u20
     - LOW/MEDIUM/HIGH under an upgrade plan; after it (and ``emrac report``): reboot, services to restart, new failures, broken AUR packages, new .pacnew files
   * - ``rebuild`` / ``rebuild --check`` / ``--stale``
     - u19
     - Build installed AUR packages again from source through the audited path; find the ones a library, Python or Qt update broke or may have broken (also a ``doctor`` row); source packages only, binary packages are named and left out
   * - plan ids / ``--expect-plan``
     - u18
     - A digest of what a plan does, shown under every plan (``digest`` in ``--json``); ``--expect-plan`` runs only that plan; a plan that was on screen for 30 seconds is made again before the commit and must be the same; history records carry the digest
   * - pre-flight checks
     - u17
     - Before a transaction changes anything: space on ``/``, the cache, ``/boot`` and the EFI partition, battery, mirrors, the kernel safety net (another kernel, fallback initramfs, boot entry, never removing the last kernel), and a gate against partial upgrades (``--allow-partial``); ``[safety]`` settings
   * - ``tree`` / ``graph`` / ``impact`` / ``cycles``
     - u16
     - The dependency tree and graph (text, Graphviz dot, JSON), what removing, installing or upgrading a package affects, and rings of packages that require each other; read-only
   * - ``pin`` / ``unpin``
     - u13
     - Hold a package back from ``upgrade`` with a reason and an expiry
   * - ``pacnew [diff|resolve|revert|status]``
     - u27, u30
     - Tells your edits from upstream's in ``.pacnew`` files by a three-way comparison, keeps/takes/merges/edits each after checking the result, records every change and can revert it
   * - ``cache [list|stats|clean|purge|trim]``
     - u28, u29
     - Source, build, binary and pacman caches; ``--lean`` keeps one rollback set; ``cc``/``cs`` shortcuts
   * - ``self check``
     - u11
     - Read-only check of emrac's own installation (binary, how it was installed, config, history, journal, ledger helper); needs no pacman, network or root
   * - ``self upgrade`` / ``self downgrade``
     - u14, u15, r3-u1
     - Download and check a release against the index, build the package, install it with ``sudo pacman -U``, check the result; a copy nobody owns (``cargo install``, by hand) in a folder you can write to is replaced in place instead, keeping the old file and putting it back if the new one fails; anything else is refused; refuses a downgrade whose file format is older than yours; journal, backup, ``--dry-run``
   * - ``self diagnose``
     - u13
     - The checks of ``self check`` plus what an upgrade needs (tools, space, permissions), the release index in detail, ``--online`` reachability; repairs nothing; hides the home directory
   * - ``self sync`` / ``self news``
     - u12
     - A validated, staged copy of the project's release index (``releases.json``; gpg signature checked if you require it) and the news from it; fetches nothing else, installs nothing
   * - ``config``
     - u1
     - Persisted defaults in ``~/.config/emrac/config.toml``
   * - ``completion``
     - u2
     - bash, zsh and fish completion scripts

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
     - ``install``/``remove``/``upgrade`` with ``--dry-run``, a preview with a plan id and, for an upgrade, a risk rating, a confirmation, pre-flight checks; every executed change is recorded (``history``) and can be reversed (``rollback``/``undo``/``redo``). The plan is built from freshly synced repositories (u22), lists what conflicts and replacements will remove and which provider pacman will pick (u24, u25), and warns when an AUR rebuild will hit a dependency conflict (u26). pacman asks its own questions unless ``--yes``; silence on stdin is never consent (u23). ``upgrade`` covers official packages and the AUR, holds back pins (``pin``/``unpin``), and can save a package-set snapshot first (``snapshot.before_upgrade``). Also ``downgrade``, ``reinstall``, ``rebuild``, ``purge`` and ``snapshot``/``restore``. No transaction queue, an interrupted transaction is recorded but not resumed, no ``lock``
   * - 2
     - Source Build System
     - Partial
     - AUR packages are fetched (a split package from its base's repo), reviewed (PKGBUILD or diff), audited, then built in two steps: dependencies and sources on the host (signatures verified with your keyring), the rest in a bubblewrap sandbox (read-only root, empty home, cleared environment, no network unless allowed, memory/process/size/time limits); the built package is inspected (manifest) before ``pacman -U``. ``emrac build <pkg|dir>`` builds without installing. A failure that looks like a blocked download offers a retry with the network on. No build queue, background/pause/cancel, log replay or PKGBUILD editing, no Landlock or throwaway root
   * - 3
     - Intelligent Recommendation Engine
     - Partial
     - ``recommend [source|binary] <pkg>`` (u2 after r3): a Source Suitability Score from stated rules (makepkg flags against the repository's, CPU level, build time measured here or estimated, update rate from your history, memory and disk), a verdict (binary, source, either, or source as the only option), a confidence and the reasons; builds are timed from now on. Advisory only: ``install`` and ``upgrade`` do not use it yet; no ``explain`` (u3), ``score`` (u4), ETA (u5) or ``benchmark`` (u6)
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
     - ``info`` covers metadata, version, license, homepage, dependencies and AUR stats; ``rdeps`` and ``why`` list who needs a package; ``audit <aur-pkg|path|file.pkg.tar.*>`` reads a PKGBUILD or inspects a built package; ``--inspect`` shows manifests of official packages before install; ``verify`` checks installed files against pacman's record. No changelog, no security advisories
   * - 7
     - Dependency Intelligence
     - Partial
     - ``deps [-r]``, ``rdeps``, ``why`` and ``orphans``, ``tree`` and ``graph`` (text, Graphviz dot, JSON), ``impact`` (what removing, installing or upgrading a package affects) and ``cycles`` (read-only, from libalpm). No interactive explorer (that needs the TUI)
   * - 8
     - Repository Management
     - Partial
     - Official + AUR only; no local/custom/git repos, mirror management, repo health
   * - 9
     - News & Advisory System
     - Partial
     - ``news`` reads the Arch Linux news, marks what you have not seen, and the upgrade shows it under the plan; an item that is a breaking change, asks for manual intervention or touches the kernel, bootloader, C library, systemd, pacman, drivers or the filesystem layout stops the upgrade until acknowledged (``news ack``, ``--ack-news``). ``self news`` covers emrac's own releases. No security advisories or CVE feed, no ABI-transition notices
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
     - Sandboxed build output is shown live and its tail is kept to recognise network failures (no terminal inside the sandbox: no colours or progress bars); resource limits report why a build was killed; no structured progress, stage tracking or summaries
   * - 13
     - Cache Management
     - Partial
     - ``emrac cache [list|stats|clean|purge|trim]`` (``cc``/``cs`` shortcuts) over four categories: downloaded sources, build trees, built/fetched package files and pacman's cache; keeps the installed and previous versions and anything a recent transaction needs (``--lean``/``cache.policy``: only what undoing the last ``cache.keep_days`` days needs plus AUR builds and held-back packages); dry-run, confirmation, ``--older-than``, size limit; no ``verify``/``export``/``import``/interactive manager/automatic cleanup
   * - 14
     - History & Recovery
     - Partial
     - ``history`` (append-only log with failures and conflict removals; each record is chained by SHA-256 to the one before, ``history verify`` finds an edit, ``history archive --older-than`` moves old records to archive files, ``history ack`` marks failures as seen; a copy cut off the end of the log is not detected locally; a root-owned append-only ledger at ``/var/lib/emrac/ledger.jsonl``, written through ``sudo emrac ledger-append``, is compared by ``history verify``, rebuilds the history with ``--repair`` and is caught up by ``history sync``; it does not stop root; ``ledger watch install`` adds kernel audit rules, ``ledger events`` and ``doctor`` report what else touched the ledger (``ledger events --ack`` marks events as seen, on record), ``history anchor`` / ``verify --against`` detect a rewrite or a cut-off tail; a transaction cut off by a crash is found next time from a journal and pacman's log and recorded, ``history recover``), ``rollback [tx]``/``undo``/``redo`` (post-state checked, files from the pacman cache, the AUR build folder or the Arch Linux Archive), ``downgrade``, ``reinstall``, ``snapshot``/``restore`` (package sets, no files), ``pin``/``unpin``, and ``pacnew`` config backups with ``pacnew revert``. No filesystem snapshots, an interrupted transaction is recorded but not resumed
   * - 15
     - Security & Integrity
     - Partial
     - emrac-level checks before anything is installed: ``audit`` (static PKGBUILD, scriptlet and shipped-file analysis with info/warning/high/critical tiers: high needs ``--accept-risks``, critical is refused), the package manifest (setuid files, hooks, sudoers, accounts, units), the build sandbox and limits, signatures verified on the host, and the untouched pacman/makepkg verification underneath. Before an AUR build: trust signals (lookalike names, a changed maintainer, sources changed under the same version, changed signing keys). Before any transaction: pre-flight checks (space, ``/boot``, power, mirrors, the kernel safety net), a partial-upgrade gate, an older-version gate, a pacman database backup and check, and plan ids so that what was approved is what runs. Compiled binaries inside a package are only counted. No Landlock layer, no advisory feed
   * - 16
     - Package Health
     - Partial
     - ``doctor [--online] [--report]``: unmet dependencies, lock and database age, pending upgrades against freshly synced repositories, kernel and modules, disk, caches, orphans, ``.pacnew`` files, failed units, pins, tools, config and sandbox, repository data older than one seen, the pacman database backup, an interrupted transaction, AUR packages the libraries have left behind, and the ledger and its audit watch, with a fix for each; ``--report`` writes a sanitized bug report. ``self check`` and ``self diagnose`` do the same for emrac's own installation, and ``report`` says what an upgrade left to do. ``verify`` and ``orphans`` complement it. No mirror health or duplicate-library checks
   * - 17
     - Analytics
     - Not started
     - --
   * - 18
     - Unified Interfaces
     - Partial
     - CLI complete for the 5 MVP commands, ``--json``, ``--dry-run``, ``-y``; ``emrac completion bash|zsh|fish`` (static, no package-name completion); no TUI at all
   * - 19
     - Configuration
     - Partial
     - ``~/.config/emrac/config.toml`` with ``emrac config show/get/set/unset/edit/validate``; persists defaults for the global, search and review flags, ``snapshot.before_upgrade``, the build limits (``build.timeout_minutes``, ``memory_percent``, ``max_tasks``, ``tmpfs_gib``) and the cache policy (``cache.policy``, ``cache.keep_days``). ``--no-sandbox`` is deliberately a command-line flag only. No theming, keybindings, aliases, profiles, system-wide or per-repo config
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
     - The preview shows packages, download size and installed-size delta, what the transaction will also remove (conflicts, replacements), provider choices and warnings, computed from freshly synced repositories, a plan id, an update risk (LOW/MEDIUM/HIGH with reasons) for an upgrade, and the pre-flight checks (disk space, ``/boot``, power, network, kernel safety net); ``impact`` adds the services and desktop entries a removal would take. No compile time
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
     - Partial
     - Per-package sizes in ``emrac cache list`` and per-path dry-runs; no browsable/selective-by-name deletion
   * - 9
     - Timeline
     - Not started
     - --
   * - 10
     - Rollback Snapshots
     - Partial
     - Package-set snapshots (``snapshot``, ``restore``, optional before ``upgrade``) and per-transaction rollback exist; no Btrfs/ZFS filesystem snapshots
   * - 11
     - Interactive Conflict Resolver
     - Partial
     - Conflict removals and provider choices are listed in plans (u24, u25); a dependency conflict that stops an AUR build is explained and a safe-looking package swap is offered, default no (u26). Other conflicts still surface as pacman's own text
   * - 12
     - Dependency Graph Explorer
     - Partial
     - ``deps -r`` prints the transitive closure as text, ``tree`` and ``graph`` show it as a tree or Graphviz source; no interactive view (that needs the TUI)
   * - 13
     - Package Inspector
     - Partial
     - ``info`` (with trust signals for AUR packages), ``rdeps``, ``impact``, ``audit``/manifest and ``verify`` together; not the single aggregated view the spec describes
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
     - Partial
     - ``emrac doctor`` and ``report`` (see Package Health above); not an interactive center
   * - 17
     - Smart Updates
     - Partial
     - ``pin``/``unpin`` hold packages back from ``upgrade`` with a reason and an expiry; no rule-based skip logic
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

Totals: 5 done / 24 partial / 17 not started, of 46 tracked (the five
MVP commands plus the 21 categories and 20 signature features above).
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
     - Partial
     - ``emrac completion bash|zsh|fish`` (static); no ``alias`` or ``integrate``
   * - System (``version``, ``about``, ``env``, ``paths``, ``diagnostics``)
     - Partial
     - ``--version`` and ``--help``; no ``env``, ``paths`` or ``diagnostics`` (``doctor --report`` covers the bug-report part)
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
     - Partial
     - ``pin``/``unpin`` are built (with ``--reason`` and ``--until``); ``lock``/``unlock`` are not

----

With these two groups included, the running total is **53 tracked
items** (46 from Parts III/IV + 7 Additional Distinguishing Ideas):
5 done / 23 partial / 25 not started. The 6 CLI-plumbing groups above
are tracked separately since they are not product features: 3 partial
(shell integration, system, pin/unpin), 3 not started.

Source: ``SPEC.md`` Parts III, IV, V and X, checked against
``crates/`` and ``CHANGELOG.md`` as of Slice r3.

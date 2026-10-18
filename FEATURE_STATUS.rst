Feature Status
================

Every feature emrac has, is specified to have, or has been proposed, in
three scopes, each row with its status and the document that tracks it.
Checked against what is actually shipped as of **Slice r3** (release
``r3``, version 0.3.0) plus the update slices ``u1`` to ``u8`` built after
it (``r2`` itself included ``u1`` to ``u30`` of the cycle before, since the
update numbers start again after every release). The same list, one row per
feature with a short description, is ``docs/feats/feats.md``; this file
keeps the detail for the product's core and the per-scope totals.

The core CLI loop -- ``search`` / ``info`` / ``install`` / ``remove`` /
``upgrade``, official repos + AUR -- has been complete since ``r1``. Since
then the work has gone into making a real upgrade safe and recoverable:
audited and sandboxed AUR builds, plans that match what pacman will do and
cannot change under you (and now show the hooks, config files and local
file policy they touch), checks before a transaction and a report after it,
history that can be verified and a ledger root alone can change, a journal
for a transaction that was cut off, trust signals for the AUR and the Arch
news, source-or-binary recommendations with their reasoning, ``override``
and ``divert`` for files pacman would reset, and emrac updating itself
through pacman. ``CHANGELOG.md`` has the detail slice by slice; the TUI,
build profiles, the build queue and the ETA engine remain unstarted.

Scopes
--------


.. list-table::
   :header-rows: 1
   :widths: 14 56 30

   * - Scope
     - What it holds
     - Tracked in
   * - **Core**
     - The product ``SPEC.md`` defines: the MVP command loop, the 21 feature categories, the 20 signature features, the additional distinguishing ideas and the CLI plumbing.
     - ``docs/feats/SPEC.md``; this file, sections "MVP Core Loop" and "Core scope"
   * - **Extended**
     - Features beyond the spec that deepen the product: the safety nets and trust checks (the ``additions.md`` ideas A, B and C), the ``ideas.md`` list, newer ideas (hook editor, git-style history, supply chain, offline bundles), package forensics, the self-management service.
     - ``docs/feats/additions.md``, ``ideas.md``, ``pkgforensics.md``, ``self-service-feat.md``, ``feats.md``
   * - **Future**
     - Larger bets for later cycles: the AUR defense (planned for r4), Wandrover (cross-distribution resolution), the extensibility group with the WASM plugin system, the research programme, and what is decided against.
     - ``docs/feats/aur-defense-feat.md``, ``cross-distro-feat.md``, ``rea-research-plan.md``, ``rea-portage.md``


Status words: **Done** (works, with a test or a container check),
**Partial** (the notes say what is missing), **Not started** (specified,
not begun), **Idea** (proposed, not decided), **Planned (r4)** (decided for
the r4 cycle), **Research** (a research task), **Not planned** (decided
against, or deliberately far future). Slice names: plain ``uN`` is update
N between ``r1`` and ``r2``; ``r2 uN`` is update N after ``r2`` (shipped in
``r3``); ``r3 uN`` is update N after ``r3``.


MVP Core Loop
--------------

``docs/feats/SPEC.md`` Part X -- the i1 -> r1 bar. All five commands work
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
   * - hook preview
     - u4 after r3
     - Under every install, remove and upgrade plan: the pacman hooks that will run, before and after, in pacman's order; which one would stop the transaction (``AbortOnFail``, missing ``Depends``); hooks a package brings; ``--json`` has a ``hooks`` object
   * - config file preview
     - u5 after r3
     - Under every install, remove and upgrade plan: the config files you changed that will get a ``.pacnew`` (certain when the new package is cached, otherwise "may") or a ``.pacsave``, by pacman's own three-way rule, ``NoUpgrade`` included; ``--json`` has a ``config`` list
   * - after a failed transaction
     - u6 after r3
     - A package pacman still lists with files missing (what a failed or cut-off install or removal leaves behind, seen with a full disk) is reported with the command that mends it (``emrac reinstall`` / ``remove``); also after ``history recover``
   * - build tools left by AUR builds
     - u7 after r3
     - After a run that built AUR packages: the packages the builds installed only to build, that nothing needs, are listed and removed or left as ``build.remove_make_deps`` (ask / always / never) says; the removal is recorded in the history and can be undone
   * - ``explain <pkg>``
     - u3 after r3
     - How ``recommend`` reached its answer, rule by rule (points, cap, source of each input), the score before and after the cap, and the alternatives that would change the score, verdict or confidence; ``--json`` has the same
   * - ``override`` / ``divert`` / ``policy``
     - u8 after r3
     - Keep the mode and owner of a packaged file (``override``) or your own version of it (``divert``) across upgrades and reinstalls, by a pacman hook; a diversion freezes the file, so it asks first, records what it diverted from, warns after every transaction that changes the file, and shows in plans, ``list``, ``status`` and ``doctor`` as STALE; ``policy status`` / ``apply``
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

Core scope
------------

The product as ``docs/feats/SPEC.md`` defines it, Parts III, IV and V.
The spec scopes everything beyond the MVP for after the core CLI loop, so
the "Not started" rows are on schedule, not slippage.

Complete Feature Catalog
~~~~~~~~~~~~~~~~~~~~~~~~~~

``SPEC.md`` Part III -- 21 categories.

.. list-table::
   :header-rows: 1
   :widths: 3 18 8 43 28

   * - No.
     - Category
     - Status
     - Notes
     - Tracked in
   * - 1
     - Unified Package Management
     - Partial
     - ``install``/``remove``/``upgrade`` with ``--dry-run``, a preview with a plan id and, for an upgrade, a risk rating, a confirmation, pre-flight checks; every executed change is recorded (``history``) and can be reversed (``rollback``/``undo``/``redo``). The plan is built from freshly synced repositories (u22), lists what conflicts and replacements will remove and which provider pacman will pick (u24, u25), and warns when an AUR rebuild will hit a dependency conflict (u26). pacman asks its own questions unless ``--yes``; silence on stdin is never consent (u23). ``upgrade`` covers official packages and the AUR, holds back pins (``pin``/``unpin``), and can save a package-set snapshot first (``snapshot.before_upgrade``). Also ``downgrade``, ``reinstall``, ``rebuild``, ``purge`` and ``snapshot``/``restore``. No transaction queue, an interrupted transaction is recorded but not resumed, no ``lock``
     - ``docs/feats/SPEC.md`` III.1; ``docs/plan/r1/`` u3-u8; ``docs/plan/r2/u17-preflight.md``; ``docs/plan/r2/u18-plan-integrity.md``
   * - 2
     - Source Build System
     - Partial
     - AUR packages are fetched (a split package from its base's repo), reviewed (PKGBUILD or diff), audited, then built in two steps: dependencies and sources on the host (signatures verified with your keyring), the rest in a bubblewrap sandbox (read-only root, empty home, cleared environment, no network unless allowed, memory/process/size/time limits); the built package is inspected (manifest) before ``pacman -U``. ``emrac build <pkg|dir>`` builds without installing. A failure that looks like a blocked download offers a retry with the network on. No build queue, background/pause/cancel, log replay or PKGBUILD editing, no Landlock or throwaway root
     - ``docs/feats/SPEC.md`` III.2; ``docs/design/source-build.md``; ``docs/plan/r1/u19-sandbox.md``; ``docs/plan/r1/u20-limits.md``
   * - 3
     - Intelligent Recommendation Engine
     - Partial
     - ``recommend [source|binary] <pkg>`` (u2 after r3): a Source Suitability Score from stated rules (makepkg flags against the repository's, CPU level, build time measured here or estimated, update rate from your history, memory and disk), a verdict (binary, source, either, or source as the only option), a confidence and the reasons; builds are timed from now on. Advisory only: ``install`` and ``upgrade`` do not use it yet; no ``explain`` (u3), ``score`` (u4), ETA (u5) or ``benchmark`` (u6)
     - ``docs/feats/SPEC.md`` III.3; ``docs/design/recommend.md``; ``docs/plan/r3/u2-recommend.md``
   * - 4
     - ETA Prediction Engine
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` III.4
   * - 5
     - Package Discovery
     - Partial
     - Basic name/description search only; no fuzzy/regex, file-ownership, category browsing, popular/recent
     - ``docs/feats/SPEC.md`` III.5
   * - 6
     - Package Inspection
     - Partial
     - ``info`` covers metadata, version, license, homepage, dependencies and AUR stats; ``rdeps`` and ``why`` list who needs a package; ``audit <aur-pkg|path|file.pkg.tar.*>`` reads a PKGBUILD or inspects a built package; ``--inspect`` shows manifests of official packages before install; ``verify`` checks installed files against pacman's record. No changelog, no security advisories
     - ``docs/feats/SPEC.md`` III.6; ``docs/plan/r1/u15-audit.md``; ``docs/plan/r1/u17-manifest.md``
   * - 7
     - Dependency Intelligence
     - Partial
     - ``deps [-r]``, ``rdeps``, ``why`` and ``orphans``, ``tree`` and ``graph`` (text, Graphviz dot, JSON), ``impact`` (what removing, installing or upgrading a package affects) and ``cycles`` (read-only, from libalpm). No interactive explorer (that needs the TUI)
     - ``docs/feats/SPEC.md`` III.7; ``docs/plan/r1/u12-deps.md``; ``docs/plan/r2/u16-dependency-views.md``
   * - 8
     - Repository Management
     - Partial
     - Official + AUR only; no local/custom/git repos, mirror management, repo health
     - ``docs/feats/SPEC.md`` III.8; ``docs/design/trust.md``
   * - 9
     - News & Advisory System
     - Partial
     - ``news`` reads the Arch Linux news, marks what you have not seen, and the upgrade shows it under the plan; an item that is a breaking change, asks for manual intervention or touches the kernel, bootloader, C library, systemd, pacman, drivers or the filesystem layout stops the upgrade until acknowledged (``news ack``, ``--ack-news``). ``self news`` covers emrac's own releases. No security advisories or CVE feed, no ABI-transition notices
     - ``docs/feats/SPEC.md`` III.9; ``docs/design/trust.md``; ``docs/plan/r2/u20-trust.md``
   * - 10
     - Build Profiles
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` III.10
   * - 11
     - Compiler & Build Optimization
     - Not started
     - ``makepkg`` defaults only, no flags exposed
     - ``docs/feats/SPEC.md`` III.11
   * - 12
     - Build Monitoring
     - Partial
     - Sandboxed build output is shown live and its tail is kept to recognise network failures (no terminal inside the sandbox: no colours or progress bars); resource limits report why a build was killed; no structured progress, stage tracking or summaries
     - ``docs/feats/SPEC.md`` III.12; ``docs/plan/r1/u20-limits.md``
   * - 13
     - Cache Management
     - Partial
     - ``emrac cache [list|stats|clean|purge|trim]`` (``cc``/``cs`` shortcuts) over four categories: downloaded sources, build trees, built/fetched package files and pacman's cache; keeps the installed and previous versions and anything a recent transaction needs (``--lean``/``cache.policy``: only what undoing the last ``cache.keep_days`` days needs plus AUR builds and held-back packages); dry-run, confirmation, ``--older-than``, size limit; no ``verify``/``export``/``import``/interactive manager/automatic cleanup
     - ``docs/feats/SPEC.md`` III.13; ``docs/plan/r1/u28-cache.md``; ``docs/plan/r1/u29-lean-cache.md``
   * - 14
     - History & Recovery
     - Partial
     - ``history`` (append-only log with failures and conflict removals; each record is chained by SHA-256 to the one before, ``history verify`` finds an edit, ``history archive --older-than`` moves old records to archive files, ``history ack`` marks failures as seen; a copy cut off the end of the log is not detected locally; a root-owned append-only ledger at ``/var/lib/emrac/ledger.jsonl``, written through ``sudo emrac ledger-append``, is compared by ``history verify``, rebuilds the history with ``--repair`` and is caught up by ``history sync``; it does not stop root; ``ledger watch install`` adds kernel audit rules, ``ledger events`` and ``doctor`` report what else touched the ledger (``ledger events --ack`` marks events as seen, on record), ``history anchor`` / ``verify --against`` detect a rewrite or a cut-off tail; a transaction cut off by a crash is found next time from a journal and pacman's log and recorded, ``history recover``), ``rollback [tx]``/``undo``/``redo`` (post-state checked, files from the pacman cache, the AUR build folder or the Arch Linux Archive), ``downgrade``, ``reinstall``, ``snapshot``/``restore`` (package sets, no files), ``pin``/``unpin``, and ``pacnew`` config backups with ``pacnew revert``. No filesystem snapshots, an interrupted transaction is recorded but not resumed
     - ``docs/feats/SPEC.md`` III.14; ``docs/design/journal.md``; ``docs/design/ledger.md``; ``docs/plan/r2/u7-history-integrity.md``
   * - 15
     - Security & Integrity
     - Partial
     - emrac-level checks before anything is installed: ``audit`` (static PKGBUILD, scriptlet and shipped-file analysis with info/warning/high/critical tiers: high needs ``--accept-risks``, critical is refused), the package manifest (setuid files, hooks, sudoers, accounts, units), the build sandbox and limits, signatures verified on the host, and the untouched pacman/makepkg verification underneath. Before an AUR build: trust signals (lookalike names, a changed maintainer, sources changed under the same version, changed signing keys). Before any transaction: pre-flight checks (space, ``/boot``, power, mirrors, the kernel safety net), a partial-upgrade gate, an older-version gate, a pacman database backup and check, and plan ids so that what was approved is what runs. Compiled binaries inside a package are only counted. No Landlock layer, no advisory feed
     - ``docs/feats/SPEC.md`` III.15; ``docs/design/trust.md``; ``docs/design/source-build.md``
   * - 16
     - Package Health
     - Partial
     - ``doctor [--online] [--report]``: unmet dependencies, lock and database age, pending upgrades against freshly synced repositories, kernel and modules, disk, caches, orphans, ``.pacnew`` files, failed units, pins, tools, config and sandbox, repository data older than one seen, the pacman database backup, an interrupted transaction, AUR packages the libraries have left behind, and the ledger and its audit watch, with a fix for each; ``--report`` writes a sanitized bug report. ``self check`` and ``self diagnose`` do the same for emrac's own installation, and ``report`` says what an upgrade left to do. ``verify`` and ``orphans`` complement it. No mirror health or duplicate-library checks
     - ``docs/feats/SPEC.md`` III.16; ``docs/plan/r1/u21-doctor.md``
   * - 17
     - Analytics
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` III.17
   * - 18
     - Unified Interfaces
     - Partial
     - CLI complete for the 5 MVP commands, ``--json``, ``--dry-run``, ``-y``; ``emrac completion bash|zsh|fish`` (static, no package-name completion); no TUI at all
     - ``docs/feats/SPEC.md`` III.18; ``docs/plan/r1/u2-completion.md``
   * - 19
     - Configuration
     - Partial
     - ``~/.config/emrac/config.toml`` with ``emrac config show/get/set/unset/edit/validate``; persists defaults for the global, search and review flags, ``snapshot.before_upgrade``, the build limits (``build.timeout_minutes``, ``memory_percent``, ``max_tasks``, ``tmpfs_gib``) and the cache policy (``cache.policy``, ``cache.keep_days``). ``--no-sandbox`` is deliberately a command-line flag only. No theming, keybindings, aliases, profiles, system-wide or per-repo config
     - ``docs/feats/SPEC.md`` III.19; ``docs/plan/r1/u1-config.md``
   * - 20
     - Performance
     - Partial
     - Fast by construction (Rust + libalpm FFI); no deliberate indexing/caching layer yet
     - ``docs/feats/SPEC.md`` III.20
   * - 21
     - Extensibility (Future)
     - Not started
     - Future-scoped in the spec itself. The plugin system (WebAssembly modules with enforcement and security checks, never scripts) and the other extensibility items are listed under Future scope, "Plugin system" and "Extensibility (SPEC III.21)".
     - ``docs/feats/SPEC.md`` III.21; Future scope

Signature & Advanced Features
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``SPEC.md`` Part IV -- 20 items.

.. list-table::
   :header-rows: 1
   :widths: 3 20 8 43 26

   * - No.
     - Feature
     - Status
     - Notes
     - Tracked in
   * - 1
     - Install Planner
     - Partial
     - The preview shows packages, download size and installed-size delta, what the transaction will also remove (conflicts, replacements), provider choices and warnings, computed from freshly synced repositories, a plan id, an update risk (LOW/MEDIUM/HIGH with reasons) for an upgrade, and the pre-flight checks (disk space, ``/boot``, power, network, kernel safety net); ``impact`` adds the services and desktop entries a removal would take. No compile time
     - ``docs/feats/SPEC.md`` IV.1; ``docs/design/hook-preview.md``; ``docs/design/config-preview.md``; ``docs/design/local-file-policy.md``
   * - 2
     - Explain Decisions
     - Partial
     - ``emrac explain <pkg>`` (u3 after r3): every rule of the source-or-binary recommendation with its points, its cap and where its input came from, the total, and what would change the answer (the engine run again with one input changed). Only the source-or-binary decision is explained; plans and other decisions are not
     - ``docs/feats/SPEC.md`` IV.2; ``docs/plan/r3/u3-explain.md``
   * - 3
     - Package Score
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.3
   * - 4
     - Build Profiles with Inheritance
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.4
   * - 5
     - Build Recipes
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.5
   * - 6
     - Build Diff
     - Partial
     - PKGBUILD diff-on-rebuild is shipped (``AurSyncOutcome::Changed``); spec's ``diff-build`` runtime comparison (binary vs. source perf) is not
     - ``docs/feats/SPEC.md`` IV.6
   * - 7
     - Build Benchmark
     - Partial
     - ``emrac benchmark <pkg>`` runs the package's programs with ``--version`` (after asking) and records the first run, the warm median, the executable size and a memory figure when it is above the launcher floor. CPU time, a cold-cache run and feeding the results to ``recommend`` are not built
     - ``docs/feats/SPEC.md`` IV.7; ``docs/plan/r3/u9-stats-benchmark.md``
   * - 8
     - Build Cache Explorer
     - Partial
     - Per-package sizes in ``emrac cache list`` and per-path dry-runs; no browsable/selective-by-name deletion
     - ``docs/feats/SPEC.md`` IV.8; ``docs/plan/r1/u28-cache.md``
   * - 9
     - Timeline
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.9
   * - 10
     - Rollback Snapshots
     - Partial
     - Package-set snapshots (``snapshot``, ``restore``, optional before ``upgrade``) and per-transaction rollback exist; no Btrfs/ZFS filesystem snapshots
     - ``docs/feats/SPEC.md`` IV.10; ``docs/plan/r1/u10-snapshot.md``; ``docs/plan/r1/u11-auto-snapshot.md``
   * - 11
     - Interactive Conflict Resolver
     - Partial
     - Conflict removals and provider choices are listed in plans (u24, u25); a dependency conflict that stops an AUR build is explained and a safe-looking package swap is offered, default no (u26). Other conflicts still surface as pacman's own text
     - ``docs/feats/SPEC.md`` IV.11; ``docs/plan/r1/u24-plan-removals.md``; ``docs/plan/r1/u26-dependency-conflicts.md``
   * - 12
     - Dependency Graph Explorer
     - Partial
     - ``deps -r`` prints the transitive closure as text, ``tree`` and ``graph`` show it as a tree or Graphviz source; no interactive view (that needs the TUI)
     - ``docs/feats/SPEC.md`` IV.12; ``docs/plan/r2/u16-dependency-views.md``
   * - 13
     - Package Inspector
     - Partial
     - ``info`` (with trust signals for AUR packages), ``rdeps``, ``impact``, ``audit``/manifest and ``verify`` together; not the single aggregated view the spec describes
     - ``docs/feats/SPEC.md`` IV.13
   * - 14
     - Repository Analytics
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.14
   * - 15
     - Build Dashboard
     - Not started
     - Needs the TUI
     - ``docs/feats/SPEC.md`` IV.15
   * - 16
     - Health Center (doctor)
     - Partial
     - ``emrac doctor`` and ``report`` (see Package Health above); not an interactive center
     - ``docs/feats/SPEC.md`` IV.16; ``docs/plan/r1/u21-doctor.md``
   * - 17
     - Smart Updates
     - Partial
     - ``pin``/``unpin`` hold packages back from ``upgrade`` with a reason and an expiry; no rule-based skip logic
     - ``docs/feats/SPEC.md`` IV.17; ``docs/plan/r1/u13-pin.md``
   * - 18
     - Build Statistics
     - Partial
     - ``emrac stats`` shows builds (count, failures, time, longest, most built) and transactions by kind with the AUR share. Hours and storage saved are not shown: emrac measures builds only, so they would be guesses
     - ``docs/feats/SPEC.md`` IV.18; ``docs/plan/r3/u9-stats-benchmark.md``
   * - 19
     - Package Collections
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.19
   * - 20
     - Workspace Mode
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV.20

Additional Distinguishing Ideas
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Listed in ``SPEC.md`` Part IV but left unnumbered in the spec.

.. list-table::
   :header-rows: 1
   :widths: 20 10 50 20

   * - Idea
     - Status
     - Notes
     - Tracked in
   * - Package Advisor
     - Not started
     - Warns on unusually large dependency trees or disk impact before install
     - ``docs/feats/SPEC.md`` IV+
   * - Mirror Intelligence
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` IV+
   * - Package Impact Preview
     - Partial
     - ``emrac impact`` shows what removing, installing or upgrading a package affects (reverse dependencies, orphans, space); affected services and desktop entries are not listed yet
     - ``docs/feats/SPEC.md`` IV+; ``docs/plan/r2/u16-dependency-views.md``
   * - Compile Farm Support
     - Not started
     - Remote/SSH build distribution
     - ``docs/feats/SPEC.md`` IV+
   * - Build Artifact Library
     - Not started
     - Cross-run reuse of compatible builds by profile/compiler/flags
     - ``docs/feats/SPEC.md`` IV+
   * - TUI Command Palette
     - Not started
     - Needs the TUI
     - ``docs/feats/SPEC.md`` IV+
   * - Policy Engine
     - Not started
     - Configurable rules, e.g. "always build Rust packages from source"
     - ``docs/feats/SPEC.md`` IV+

Part V -- Command Catalog: CLI Plumbing
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Pure CLI infrastructure with no corresponding Part III/IV row; the rest of
Part V is the command surface of features covered above.

.. list-table::
   :header-rows: 1
   :widths: 34 10 38 18

   * - Group
     - Status
     - Notes
     - Tracked in
   * - Shell Integration (``completion``, ``alias``, ``integrate``)
     - Partial
     - ``emrac completion bash|zsh|fish`` (static); no ``alias`` or ``integrate``
     - ``docs/feats/SPEC.md`` Part V
   * - System (``version``, ``about``, ``env``, ``paths``, ``diagnostics``)
     - Partial
     - ``--version`` and ``--help``; no ``env``, ``paths`` or ``diagnostics`` (``doctor --report`` covers the bug-report part)
     - ``docs/feats/SPEC.md`` Part V
   * - Development (``dev``, ``sandbox``, ``test``, ``trace``, ``debug``)
     - Not started
     - Internal dev/debug tooling
     - ``docs/feats/SPEC.md`` Part V
   * - Export / Import (``export``, ``import``, ``bundle``)
     - Not started
     - Nothing built yet.
     - ``docs/feats/SPEC.md`` Part V
   * - Help (``help``, ``man``, ``docs``, ``license``, ``credits``, ``bug-report``)
     - Not started
     - Only clap's auto-generated ``--help`` exists today
     - ``docs/feats/SPEC.md`` Part V
   * - ``lock``/``unlock``, ``pin``/``unpin`` (under Package Management)
     - Partial
     - ``pin``/``unpin`` are built (with ``--reason`` and ``--until``); ``lock``/``unlock`` are not
     - ``docs/feats/SPEC.md`` Part V

Core scope: 59 tracked: 5 done, 32 partial, 22 not started (the five MVP commands, 21 categories, 20 signature features, 7 additional ideas, 6 plumbing groups).

Extended scope
----------------

Beyond the spec. Most of the safety work of ``r2`` and ``r3`` lives here:
the journal, plan ids, pre-flight, trust signals, the history ledger, the
previews and ``override``/``divert``. Each section's rows are the ideas of
its document with the status the code gives them.


Additions (additions.md): A, B and C
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Ideas A1-A22, B1-B9 and the critical features C1-C8.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - A1 Typosquat and lookalike warnings
     - Done
     - AUR names near-copies of popular packages.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A2 Source-changed-without-bump alert
     - Done
     - Sources or checksums change under the same version, or move to another host.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A3 New-maintainer and first-publish warnings
     - Done
     - Orphaned/new/few users, changed maintainer, name shadows an official package.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A4 Signing-key pinning
     - Done
     - Warn when the PGP keys that sign a package's sources change.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A5 Update risk rating
     - Done
     - LOW/MEDIUM/HIGH by what the transaction touches and unread news.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A6 Staged upgrades
     - Idea
     - Split a big upgrade into ordered groups so a failure stops early.
     - ``docs/feats/additions.md``
   * - A7 Restart and reboot advisor
     - Done
     - After a transaction: reboot needed, services to restart.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A8 Rollback rehearsal
     - Idea
     - ``doctor --rollback-test``: prove a restore would work before you need it.
     - ``docs/feats/additions.md``
   * - A9 Stray file scanner
     - Idea
     - Files under ``/usr`` and ``/opt`` no package owns, with who put them there.
     - ``docs/feats/additions.md``
   * - A10 Config drift report
     - Partial
     - ``.pacnew`` handling and previews exist; a report of how ``/etc`` differs from package defaults does not.
     - ``docs/feats/additions.md``; ``docs/plan/r1/u27-pacnew.md``; ``docs/plan/r3/u5-pacnew-preview.md``
   * - A11 Orphan lifecycle
     - Idea
     - Grace period, recorded reason and restore instead of immediate delete (``orphans`` lists today).
     - ``docs/feats/additions.md``
   * - A12 Install reasons ledger
     - Idea
     - Record why each package is installed (explicit, dependency of X, project Y).
     - ``docs/feats/additions.md``
   * - A13 Build failure classifier
     - Partial
     - Network failures, resource limits and AUR dependency conflicts are recognised; a general pattern classifier is not.
     - ``docs/feats/additions.md``; ``docs/plan/r1/u19-sandbox.md``; ``docs/plan/r1/u20-limits.md``; ``docs/plan/r1/u26-dependency-conflicts.md``
   * - A14 Soname rebuild planner
     - Done
     - Which source-built packages a library bump broke; queue the rebuild.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u19-rebuild.md``
   * - A15 Clean-chroot build option
     - Idea
     - Build in a devtools-style clean chroot.
     - ``docs/feats/additions.md``
   * - A16 Machine diff
     - Idea
     - Compare two machines' package sets or one against a lockfile.
     - ``docs/feats/additions.md``
   * - A17 Remote apply
     - Idea
     - Review a plan locally, apply it on another machine over SSH.
     - ``docs/feats/additions.md``
   * - A18 Container and image export
     - Idea
     - A package set as a ``pacstrap`` list or Containerfile.
     - ``docs/feats/additions.md``
   * - A19 Post-upgrade "what changed" report
     - Done
     - New failures, broken AUR packages, new ``.pacnew``, reboot and services; per-package dependency/file/service changes are not yet listed.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - A20 Prompt and status-bar output
     - Idea
     - ``emrac status --short`` for updates and unread news.
     - ``docs/feats/additions.md``
   * - A21 Notifications
     - Idea
     - Desktop notifications or a webhook when a long build or scheduled update ends.
     - ``docs/feats/additions.md``
   * - A22 Foreign-package inventory
     - Idea
     - Flatpaks, pip/cargo/npm globals and AppImages beside system packages.
     - ``docs/feats/additions.md``
   * - B1 ``emrac bisect``
     - Idea
     - Find which update broke something using the transaction log and cached packages.
     - ``docs/feats/additions.md``
   * - B2 ``emrac try <pkg>``
     - Idea
     - Run a package in the sandbox overlay without installing it.
     - ``docs/feats/additions.md``
   * - B3 Pre-flight checks and kernel safety net
     - Done
     - Space, ``/boot``, EFI, battery, mirrors, a second kernel and fallback initramfs.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u17-preflight.md``
   * - B4 Conflict explainer
     - Partial
     - Conflict removals and providers in plans; AUR-build conflicts explained. The shortest chain and ranked ways out are not built.
     - ``docs/feats/additions.md``; ``docs/plan/r1/u24-plan-removals.md``; ``docs/plan/r1/u26-dependency-conflicts.md``
   * - B5 Driver and firmware doctor
     - Idea
     - Detected hardware against installed drivers, DKMS modules and firmware.
     - ``docs/feats/additions.md``
   * - B6 Machine manifest and bootstrap
     - Idea
     - One file describing a machine; ``bootstrap`` rebuilds it.
     - ``docs/feats/additions.md``
   * - B7 Downgrade assistant
     - Partial
     - ``downgrade`` finds versions in the cache and the Archive; working out which dependents must move too is not built.
     - ``docs/feats/additions.md``; ``docs/plan/r1/u6-downgrade.md``
   * - B8 Link-check for software built outside the manager
     - Partial
     - Installed AUR packages broken by a library update are found; unpackaged binaries under ``~/.local``, ``/opt``, ``/usr/local`` are not scanned.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u19-rebuild.md``
   * - B9 Metered and flaky-network mode
     - Idea
     - Resumable downloads, mirror fallback, bandwidth cap, deferred large packages.
     - ``docs/feats/additions.md``
   * - C1 Crash-safe transaction journal
     - Done
     - Write-ahead journal, stale lock check, recover or record.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u10-journal.md``
   * - C2 Plan integrity
     - Done
     - What was previewed is what runs: plan ids, ``--expect-plan``, re-plan on drift.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u18-plan-integrity.md``
   * - C3 Fail-closed verification chain
     - Partial
     - Older-database and older-version refusals exist; keyring-first upgrade and a clock-skew check do not.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - C4 Partial-upgrade hard gate
     - Done
     - A planner rule, with the exact packages named.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u17-preflight.md``
   * - C5 Mirror and repository trust
     - Partial
     - Rollback protection and the downgrade refusal exist; comparing database hashes across two mirrors does not.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - C6 Protecting pacman's own database
     - Done
     - Backup before every transaction, ``-Dk`` before and after, lock respected.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u20-trust.md``
   * - C7 Rescue binary (``emrac-rescue``)
     - Idea
     - A small static binary that reads the journal, restores the database backup and rolls back when the main binary cannot start.
     - ``docs/feats/additions.md``
   * - C8 Privilege separation
     - Partial
     - Root-only hidden helpers exist for the ledger and the local file policy (validated arguments, trusted executable); the planner, builds and the rest still run as the user, but the commit goes through ``sudo pacman``, not a helper that accepts a saved plan.
     - ``docs/feats/additions.md``; ``docs/plan/r2/u8-ledger.md``; ``docs/plan/r3/u8-override-divert.md``

Ideas (ideas.md)
~~~~~~~~~~~~~~~~

Ideas from other projects, safety and recovery, workflow, the maintainer workflow, emrac's own tooling.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - PKGBUILD AST parser
     - Partial
     - Static analysis without running bash exists (``audit``); a full shell AST is the stated direction.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u15-audit.md``
   * - Config presets
     - Idea
     - Shareable, signed presets extending build profiles and recipes.
     - ``docs/feats/ideas.md``
   * - Landlock + OverlayFS together in the staging layer
     - Idea
     - Defense in depth inside the sandbox.
     - ``docs/feats/ideas.md``
   * - Declarative profile file
     - Idea
     - pacdef/decman style: diff a plain-text desired set against the machine.
     - ``docs/feats/ideas.md``
   * - Rollback history in the TUI
     - Idea
     - CLI history and rollback are built; the TUI view is not.
     - ``docs/feats/ideas.md``
   * - Persistent build cache keyed by PKGBUILD hash + flags
     - Idea
     - Identical builds are not repeated.
     - ``docs/feats/ideas.md``
   * - Optional local repository of verified builds
     - Idea
     - Reuse across machines.
     - ``docs/feats/ideas.md``
   * - Multi-source backends
     - Idea
     - pacman repos, AUR, Flatpak, systemd units behind one interface.
     - ``docs/feats/ideas.md``
   * - Split fetch and build phases
     - Done
     - Sources fetched and verified first; the build runs with no network.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u17-manifest.md``; ``docs/plan/r1/u19-sandbox.md``
   * - Reproducible-build check
     - Idea
     - Build twice in separate sandboxes and compare hashes.
     - ``docs/feats/ideas.md``
   * - Signed, tamper-evident transaction log
     - Done
     - SHA-256 chain, root-owned ledger, audit watch (chained and witnessed, not cryptographically signed).
     - ``docs/feats/ideas.md``; ``docs/plan/r2/u7-history-integrity.md``; ``docs/plan/r2/u9-ledger-watch.md``
   * - SBOM / provenance output
     - Idea
     - What source, flags and toolchain went into each build.
     - ``docs/feats/ideas.md``
   * - Post-commit health check with auto-rollback
     - Partial
     - The post-upgrade report and the after-failure check exist; automatic rollback on failed checks does not.
     - ``docs/feats/ideas.md``; ``docs/plan/r2/u20-trust.md``; ``docs/plan/r3/u6-reconcile.md``
   * - Per-package rollback from cached packages
     - Done
     - ``rollback``, ``undo``, ``redo``, ``downgrade``.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u4-rollback.md``; ``docs/plan/r1/u6-downgrade.md``
   * - Bootable pre-transaction snapshots
     - Idea
     - Register the snapshot as a boot entry.
     - ``docs/feats/ideas.md``
   * - Snapshot backends behind one trait
     - Idea
     - Btrfs, ZFS, snapper/timeshift; package-set snapshots are built.
     - ``docs/feats/ideas.md``
   * - Lockfile (``emrac.lock``)
     - Idea
     - Replay a transaction with exact versions and hashes on another machine.
     - ``docs/feats/ideas.md``
   * - Headless JSON API
     - Partial
     - ``--json`` on most commands; no long-running API.
     - ``docs/feats/ideas.md``
   * - ``.pacnew`` / config-merge handling
     - Done
     - CLI merge and resolve with validation and revert (TUI integration is not).
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u27-pacnew.md``; ``docs/plan/r1/u30-pacnew-revert.md``
   * - Plan diff view
     - Partial
     - A drifted plan shows what changed; there is no command to diff two saved plans.
     - ``docs/feats/ideas.md``; ``docs/plan/r2/u18-plan-integrity.md``
   * - Pacman hook compatibility
     - Done
     - pacman runs its hooks as always, and plans now preview them.
     - ``docs/feats/ideas.md``; ``docs/plan/r3/u4-hook-preview.md``
   * - Offline / air-gapped mode
     - Partial
     - ``--offline`` for lookups and plans from local data; no verified bundle (see section 11).
     - ``docs/feats/ideas.md``
   * - Mirror ranking and parallel downloads
     - Idea
     - Built into the engine.
     - ``docs/feats/ideas.md``
   * - Preset sharing
     - Idea
     - Signed optimization-flag presets.
     - ``docs/feats/ideas.md``
   * - Smoke tests in the sandbox
     - Idea
     - Run ``check()`` and "does the binary start" before commit.
     - ``docs/feats/ideas.md``
   * - Binary vs source benchmarking
     - Idea
     - Measure whether a source build was worth it (``recommend`` predicts, ``benchmark`` would measure).
     - ``docs/feats/ideas.md``
   * - Vulnerability-aware planning
     - Idea
     - Arch security advisory data in the plan.
     - ``docs/feats/ideas.md``
   * - Safe unattended updates
     - Idea
     - Scheduled runs that commit only if checks pass.
     - ``docs/feats/ideas.md``
   * - "Why" explanations
     - Done
     - ``why`` for installed packages and ``explain`` for recommendations.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u12-deps.md``; ``docs/plan/r3/u3-explain.md``
   * - Arch news gate
     - Done
     - See section 1.
     - ``docs/feats/ideas.md``; ``docs/plan/r2/u20-trust.md``
   * - DKMS / kernel-module test
     - Idea
     - Build modules against the new kernel in the sandbox first.
     - ``docs/feats/ideas.md``
   * - Boot test for risky transactions
     - Idea
     - Boot the result in a throwaway VM.
     - ``docs/feats/ideas.md``
   * - Install-time capability manifest
     - Done
     - Everything a package changes beyond files, before ``pacman -U``.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u17-manifest.md``
   * - Config tracking tied to transactions
     - Idea
     - etckeeper-style git history of ``/etc`` per transaction.
     - ``docs/feats/ideas.md``
   * - Time-travel installs
     - Idea
     - Resolve a plan "as of a date" from the Arch Linux Archive (the Archive is used today only to fetch files for rollback).
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u5-archive-fetch.md``
   * - Holds with reasons and expiry
     - Done
     - ``pin --reason --until``.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u13-pin.md``
   * - Cleanup simulation
     - Partial
     - ``impact`` and ``orphans``; no bloat view.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u12-deps.md``; ``docs/plan/r1/u16-audit-coverage.md``
   * - Shared build acceleration (ccache, sccache, distcc)
     - Idea
     - Pairs with the persistent build cache.
     - ``docs/feats/ideas.md``
   * - ``doctor``
     - Done
     - See section 1.
     - ``docs/feats/ideas.md``; ``docs/plan/r1/u21-doctor.md``
   * - Migration mode
     - Idea
     - Import a package list from another distribution into a reviewed Arch plan.
     - ``docs/feats/ideas.md``
   * - ``maintain check`` (pre-submit pipeline)
     - Idea
     - Lint, regenerate ``.SRCINFO``, clean-sandbox build and ``check()``, conflict, install and upgrade tests, AUR guideline checks, then the push.
     - ``docs/feats/ideas.md``
   * - Upstream watcher
     - Idea
     - Detect a new release, bump ``pkgver``, test-build, prepare a commit for approval.
     - ``docs/feats/ideas.md``
   * - Maintainer dashboard (TUI)
     - Idea
     - Out-of-date flags, failing builds, soname rebuilds.
     - ``docs/feats/ideas.md``
   * - Capability-based workflow files
     - Idea
     - Each pipeline step declares needs (network, paths, secrets); enforced by Landlock or namespaces.
     - ``docs/feats/ideas.md``
   * - Dry-run for automation
     - Idea
     - Every workflow run shown as a plan first.
     - ``docs/feats/ideas.md``
   * - Bring-your-own SSH key for publishing
     - Idea
     - Reference the key by path or agent, never embed; only the push step sees it; host key pinning; confirm before push; multiple identities; other secrets by reference.
     - ``docs/feats/ideas.md``
   * - Verified provenance indicator, CI template generation, local test repo, orphan adoption
     - Idea
     - Maintainer extras.
     - ``docs/feats/ideas.md``
   * - ``emrac setup``
     - Idea
     - First-run wizard that detects what the machine supports and writes the config.
     - ``docs/feats/ideas.md``
   * - Config extras (layered config, ``migrate``, ``preset``, ``diff``)
     - Idea
     - Beyond show/get/set/unset/edit/validate.
     - ``docs/feats/ideas.md``
   * - ``emrac self-test``
     - Idea
     - Prove each sandbox and snapshot layer works here.
     - ``docs/feats/ideas.md``
   * - ``emrac sandbox shell``
     - Idea
     - Drop into the sandbox of a failed build.
     - ``docs/feats/ideas.md``
   * - ``plan`` / ``apply``
     - Partial
     - Dry-run, plan ids and ``--expect-plan`` exist; saving and applying a plan later does not.
     - ``docs/feats/ideas.md``; ``docs/plan/r2/u18-plan-integrity.md``
   * - Snapshot retention policy
     - Idea
     - Keep last N or N days.
     - ``docs/feats/ideas.md``
   * - Man pages
     - Idea
     - Generated from the command definitions.
     - ``docs/feats/ideas.md``
   * - Structured logging
     - Idea
     - One log per transaction ID, JSON on request.
     - ``docs/feats/ideas.md``

Newer ideas
~~~~~~~~~~~

Collected after the documents above; tracked in ``docs/feats/feats.md`` section 11. Includes ``emrac pachooks`` (the pacman hook editor; ``emrac hooks`` is already the spec's event-hook group), git-style history, supply-chain and offline-bundle ideas.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - Pacman hook editor: ``emrac pachooks list | show | new --template | edit | test | enable | disable``
     - Idea
     - A form-assisted, validating editor for pacman's ``*.hook`` files (declarative files, not scripts): live validation (missing sections, unknown keys, an ``Exec`` that does not exist, ``AbortOnFail`` where it is not allowed), a "when would this fire?" preview against a plan (the matching engine of the hook preview already exists), a sandboxed test run of ``Exec`` with a fake target list, templates (snapshot before upgrade, notify after, clear a cache, rebuild after a kernel update), safe overrides (copy a package-owned hook to ``/etc/pacman.d/hooks/`` under the same name or mask it with a ``/dev/null`` symlink, never edit in place), owner display (package or user), history with diffs and one-command enable/disable/undo, saving through the privileged helper with a diff and a confirmation, lint for risky patterns, and package-added hooks shown in the install preview. The group is named ``pachooks`` because ``SPEC.md`` already uses ``emrac hooks`` for emrac's own event hooks (the schedule/cron/hooks/events group). ``edit`` uses the same editor as ``emrac edit``. CLI in the style of ``visudo`` (validates on save, refuses to install an invalid file) and TUI (hook list, form, preview).
     - ``docs/feats/feats.md``; ``docs/plan/r1/u4-rollback.md``; ``docs/plan/r1/u8-purge.md``; builds on ``docs/design/hook-preview.md``; ``docs/design/local-file-policy.md``
   * - Update cooldown (soak time)
     - Idea
     - Do not install a new AUR version until it is N hours old unless it is flagged as a security fix; also in the AUR defense document.
     - ``docs/feats/feats.md``; ``docs/feats/aur-defense-feat.md``
   * - PKGBUILD review ledger
     - Idea
     - Record the hash you last reviewed; show only the diff since and ask to re-approve; later import others' signed review records.
     - ``docs/feats/feats.md``; ``docs/feats/aur-defense-feat.md``
   * - Lockstep groups
     - Idea
     - User-defined sets (GPU driver, utilities, kernel module) that upgrade together or not at all, with the reason; extends the partial-upgrade gate.
     - ``docs/feats/feats.md``
   * - Provider preferences
     - Idea
     - Remember your answer to "there are N providers available" as a named preference, show it in every plan, change it with one command.
     - ``docs/feats/feats.md``
   * - File history: ``emrac blame <path>``
     - Idea
     - Which package installed a file, in which transaction, and what changed it since; with ``emrac diff <date|tx> <date|tx>`` for packages, files and services between two points.
     - ``docs/feats/feats.md``; relates to ``docs/design/journal.md``
   * - Transaction notes and known-good markers
     - Idea
     - ``-m "before the demo"`` on a transaction; mark a state known good; bisect, rollback and diff can target it.
     - ``docs/feats/feats.md``
   * - Build failure bundle
     - Idea
     - A sanitized bundle (PKGBUILD, toolchain versions, environment summary, relevant log lines) to attach to an AUR comment or bug; pairs with the failure classifier (A13).
     - ``docs/feats/feats.md``
   * - Offline bundles: ``bundle create | apply``
     - Idea
     - Export verified packages, databases and a reviewed plan to a drive; import and verify them on an air-gapped machine through the normal preview.
     - ``docs/feats/feats.md``
   * - Git-style ``log`` / ``show``
     - Idea
     - Every transaction gets a short ID, a parent and a message; ``log --oneline --graph``, filters by package or date; ``show <tx>`` lists what changed.
     - ``docs/feats/feats.md``; relates to ``docs/design/journal.md``
   * - ``diff <tx> <tx>`` and ``revert <tx>``
     - Idea
     - Changes between two points, including tags; ``revert`` undoes one past transaction by creating a new one, after checking later transactions for conflicts (unlike ``rollback``, which resets).
     - ``docs/feats/feats.md``; relates to ``docs/design/journal.md``
   * - Tags
     - Idea
     - ``emrac tag known-good``; bisect, diff and restore can target it.
     - ``docs/feats/feats.md``
   * - ``emrac status`` (a git status for the system)
     - Idea
     - Packages behind, held packages, orphans, stray files, unmerged ``.pacnew``, drift from a manifest or lockfile.
     - ``docs/feats/feats.md``; relates to ``docs/design/journal.md``
   * - Ignore file
     - Idea
     - A ``.gitignore``-style list of paths the stray-file scanner and drift reports skip.
     - ``docs/feats/feats.md``
   * - Experiment branches
     - Idea
     - ``branch new`` snapshots and tracks changes; ``discard`` returns to the branch point, ``keep`` accepts them.
     - ``docs/feats/feats.md``
   * - Content-addressed plans and manifests
     - Partial
     - Plan ids are already SHA-256 digests of what a plan does; manifest ids are not.
     - ``docs/feats/feats.md``; ``docs/plan/r2/u18-plan-integrity.md``
   * - Config repo in git
     - Idea
     - Keep config, manifest, lockfile, profiles, recipes, patches and hooks in a git repo; commit each change with the transaction ID; ``bootstrap <git-url>`` rebuilds a machine.
     - ``docs/feats/feats.md``
   * - ``/etc`` history (etckeeper-style)
     - Idea
     - Commit ``/etc`` after every transaction to see which package changed which config and revert one file.
     - ``docs/feats/feats.md``
   * - AUR history as evidence
     - Idea
     - Commit log, author and email changes and PKGBUILD diffs since your last review; a force-pushed history is a trust warning.
     - ``docs/feats/feats.md``
   * - VCS package pinning
     - Idea
     - For ``-git`` packages record the built commit, show "3 commits behind" and the commits between your build and the update in the preview; build at a chosen commit; verify signed tags.
     - ``docs/feats/feats.md``
   * - Signed git data feeds
     - Idea
     - The Wandrover mapping repository, the blocklist and advisories as git repositories fetched with signature verification and fast-forward only.
     - ``docs/feats/feats.md``
   * - Patch workflow: ``emrac patch new <pkg>``
     - Idea
     - Open the unpacked source as a git worktree, generate the patch into your patch directory, rebase it when upstream changes and report conflicts.
     - ``docs/feats/feats.md``
   * - Maintainer pre-push checks
     - Idea
     - Clean worktree, regenerated and committed ``.SRCINFO``, and the ``master`` branch the AUR requires.
     - ``docs/feats/feats.md``
   * - ``emrac vercmp``
     - Idea
     - Compare two versions with pacman's rules (the semantics are documented in ``research/vercmp.md``).
     - ``docs/feats/feats.md``
   * - ``emrac query --format``
     - Idea
     - Filter and format installed packages by size, install date, reason, repository (rpm ``-q --qf``, ``dpkg-query -W -f``).
     - ``docs/feats/feats.md``
   * - Optional-dependency suggestions
     - Idea
     - After a transaction, which optional dependencies are unmet and what each adds (rpm weak dependencies).
     - ``docs/feats/feats.md``
   * - ``.pacnew`` resolution offered right after an upgrade
     - Idea
     - Offer ``emrac pacnew resolve`` for the files the upgrade just created, using the u5 prediction.
     - ``docs/feats/feats.md``

Package forensics (pkgforensics.md)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Proposed commands under ``emrac forensics``.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - ``forensics timeline --since <date> [--pkg <name>]``
     - Idea
     - One ordered timeline merging transactions, ``pacman.log``, file change times and enabled units, each event showing its source.
     - ``docs/feats/pkgforensics.md``
   * - ``forensics provenance <pkg or file>``
     - Idea
     - Where it came from: repository or AUR, PKGBUILD hash and git commit, sources and checksums, build environment, signature status, the installing transaction and its review record.
     - ``docs/feats/pkgforensics.md``
   * - ``forensics verify [--deep]``
     - Partial
     - ``verify``/``check`` compares installed files with pacman's record (hashes, sizes, modes). ``--deep`` (re-extract the cached package and compare, which catches a tampered metadata record), ownership and extra-file checks are not built.
     - ``docs/feats/pkgforensics.md``; ``docs/plan/r1/u9-verify.md``
   * - ``forensics persistence``
     - Idea
     - Every persistence point (units, timers, cron, udev, ``ld.so.preload``, shell startup files, SSH keys, PAM, sudoers, autostart, kernel modules, initramfs hooks) and which package owns each; unowned ones flagged. Same ground as the AUR defense ``audit --persistence`` and the stray file scanner (A9).
     - ``docs/feats/pkgforensics.md``
   * - ``forensics delta <pkg> <v1> <v2>``
     - Idea
     - Files added or removed, hash and permission changes, scriptlet and hook changes, dependency changes, ELF imports and embedded URLs.
     - ``docs/feats/pkgforensics.md``
   * - ``forensics baseline create | diff``
     - Idea
     - Save a hash baseline of package files and the persistence set at a known-good time and show what differs now.
     - ``docs/feats/pkgforensics.md``
   * - ``forensics capture``
     - Idea
     - An evidence bundle (package list, transaction log, hashes, persistence inventory, timeline) with a hash manifest, an optional signature and a redaction option.
     - ``docs/feats/pkgforensics.md``
   * - Offline analysis (``--root /mnt``)
     - Idea
     - Analyze a mounted disk from a live USB or the rescue binary (C7) without trusting the running kernel.
     - ``docs/feats/pkgforensics.md``
   * - Tamper-evident history with an off-machine anchor
     - Partial
     - The SHA-256 chain, the root-owned ledger and the audit watch are built; storing the latest hash somewhere else (a git remote or a printed code) is not.
     - ``docs/feats/pkgforensics.md``; ``docs/plan/r2/u7-history-integrity.md``; ``docs/plan/r2/u9-ledger-watch.md``

Self-management service (self-service-feat.md)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``emrac self``, all five phases built.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - ``self check``
     - Done
     - Phase 1.
     - ``docs/feats/self-service-feat.md``; ``docs/plan/r2/u11-self-check.md``; ``docs/design/release-index.md``; ``docs/design/self-upgrade.md``
   * - ``self sync``, ``self news``
     - Done
     - Phase 2: validated state and metadata.
     - ``docs/feats/self-service-feat.md``; ``docs/plan/r2/u12-self-service.md``; ``docs/design/release-index.md``; ``docs/design/self-upgrade.md``
   * - ``self diagnose``
     - Done
     - Phase 3: diagnostics (journals and locking arrived with the journal).
     - ``docs/feats/self-service-feat.md``; ``docs/plan/r2/u12-self-service.md``; ``docs/design/release-index.md``; ``docs/design/self-upgrade.md``
   * - ``self upgrade`` (``up``)
     - Done
     - Phase 4: selection, installation-method detection, verification.
     - ``docs/feats/self-service-feat.md``; ``docs/plan/r2/u12-self-service.md``; ``docs/plan/r3/u1-in-place-update.md``; ``docs/design/release-index.md``; ``docs/design/self-upgrade.md``
   * - ``self downgrade`` (``dn``)
     - Done
     - Phase 5: explicit target, compatibility check.
     - ``docs/feats/self-service-feat.md``; ``docs/plan/r2/u12-self-service.md``; ``docs/design/release-index.md``; ``docs/design/self-upgrade.md``

Extended scope: 135 tracked: 28 done, 18 partial, 89 idea.

Future scope
--------------

Larger bets, planned for later cycles or not yet decided.


AUR defense (planned for r4)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``docs/feats/aur-defense-feat.md``: layered malware protection, to be planned in the r4 cycle, not before. Several layers overlap what is built.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - Layer 1 Metadata and trust
     - Partial
     - Lookalikes, maintainer signals, shadowing exist; the signed blocklist and upload-burst detection do not.
     - ``docs/feats/aur-defense-feat.md``; ``docs/plan/r2/u20-trust.md``
   * - Layer 2 PKGBUILD, ``.install`` and source analysis
     - Partial
     - Most rules exist (``audit``); patch scanning and a few rules are the spec's additions.
     - ``docs/feats/aur-defense-feat.md``; ``docs/plan/r1/u15-audit.md``; ``docs/plan/r1/u16-audit-coverage.md``; ``docs/plan/r1/u18-close-gaps.md``
   * - Layer 3 Diff review and soak time
     - Partial
     - The diff is shown on every update; soak time, an approval hash ledger and trusted/blocked maintainer lists are not built.
     - ``docs/feats/aur-defense-feat.md``
   * - Layer 4 Isolated build with behavior tracing
     - Partial
     - Sandbox, no-network build and limits exist; syscall tracing, canary files, the sinkhole and the ecosystem network tier do not.
     - ``docs/feats/aur-defense-feat.md``; ``docs/plan/r1/u19-sandbox.md``; ``docs/plan/r1/u20-limits.md``
   * - Layer 5 Built-package inspection
     - Done
     - The manifest.
     - ``docs/feats/aur-defense-feat.md``; ``docs/plan/r1/u17-manifest.md``
   * - Layer 6 Install detonation
     - Planned (r4)
     - Run scriptlets and hooks in a throwaway root and report undeclared changes.
     - ``docs/feats/aur-defense-feat.md``
   * - Layer 7 Observe mode (``try --observe``)
     - Planned (r4)
     - Run the program in the sandbox with a sinkhole and canaries.
     - ``docs/feats/aur-defense-feat.md``
   * - Layer 8 Post-install audit and incident response
     - Planned (r4)
     - ``audit --persistence``, ``incident <id>`` against a signed advisory, using the history.
     - ``docs/feats/aur-defense-feat.md``
   * - Signed blocklist and advisory feed
     - Planned (r4)
     - Open question: who maintains and signs it.
     - ``docs/feats/aur-defense-feat.md``
   * - VM isolation tier
     - Planned (r4)
     - A microVM for high-risk packages.
     - ``docs/feats/aur-defense-feat.md``

Wandrover: cross-distribution package resolution
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``docs/feats/cross-distro-feat.md``. Nothing is built.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - ``wandrover search``
     - Idea
     - Find, by name or description, a package of another distribution (Debian, Ubuntu, Fedora, openSUSE, ...).
     - ``docs/feats/cross-distro-feat.md``
   * - ``wandrover resolve``
     - Idea
     - The Arch package that provides the same software, with the evidence.
     - ``docs/feats/cross-distro-feat.md``
   * - ``wandrover alternatives``
     - Idea
     - Compatible replacements and the closest alternatives, ranked.
     - ``docs/feats/cross-distro-feat.md``
   * - ``wandrover map``
     - Idea
     - Show how a foreign package maps (clear rename, ambiguous, no match).
     - ``docs/feats/cross-distro-feat.md``
   * - ``wandrover audit``
     - Idea
     - Check the explicit mapping database.
     - ``docs/feats/cross-distro-feat.md``
   * - Cross-distribution package index
     - Idea
     - Records with a schema, stored and updated locally.
     - ``docs/feats/cross-distro-feat.md``
   * - Explicit mapping database
     - Idea
     - Curated mappings that override the matching rules.
     - ``docs/feats/cross-distro-feat.md``
   * - Capability taxonomy and ranking
     - Idea
     - Intent-aware matching and a deterministic ranking model.
     - ``docs/feats/cross-distro-feat.md``
   * - Migration mode
     - Idea
     - Import a foreign package list into a reviewed Arch plan.
     - ``docs/feats/cross-distro-feat.md``

Plugin system (plugin-feat.md)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The plugin system stays, as WebAssembly modules (never scripts) with enforcement and security checks. Nothing is built; ``docs/feats/plugin-feat.md`` is the design proposal, with the open choices listed.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - ``emrac plugin list | search | install | remove | enable | disable | update``
     - Not started
     - The command surface in ``SPEC.md`` Part V. The design adds ``info``, ``verify`` and ``test``.
     - ``docs/feats/plugin-feat.md``
   * - Plugin runtime and enforcement
     - Idea
     - The WASM runtime runs in a child process inside the same bubblewrap sandbox as builds (no network, empty home); host calls are deny-by-default and logged; plugins run unprivileged and are never loaded by a root helper or a pacman hook.
     - ``docs/feats/plugin-feat.md``
   * - Capability model
     - Idea
     - Capabilities declared in the manifest and granted by you: ``fs.read:<path>``, ``net.get:<host>``, ``net.post:<host>``, ``events:<kinds>``, ``time``, and output kinds. There is no host call to write, read the environment or reach other hosts; running an external tool is a separate, later capability (section 15).
     - ``docs/feats/plugin-feat.md``
   * - Resource limits
     - Idea
     - Per call: memory, instruction budget (fuel), wall time (the host process is killed), output size and number of findings, host-call counts. A limit hit switches the plugin off for that run and never blocks or passes a transaction.
     - ``docs/feats/plugin-feat.md``
   * - Add-only plugin findings
     - Idea
     - Plugin output is merged and labelled with the plugin; it cannot suppress or edit a built-in finding, and its severity is capped below ``critical`` by default.
     - ``docs/feats/plugin-feat.md``
   * - Plugin manifest and module inspection
     - Idea
     - ``plugin.toml`` (id, version, ABI, extension points, capabilities, limits, hash) validated strictly; before install the module must validate as WebAssembly and its imports must be a subset of the declared capabilities (no WASI, no threads).
     - ``docs/feats/plugin-feat.md``
   * - Plugin install security checks
     - Idea
     - Hash and signature verified; key trust explicit on first use; an install preview in words with a default-no question ``--yes`` does not answer; pinning in ``plugins.lock``; an update that asks for more capabilities needs a new approval; downgrades refused; history records; ``plugin verify`` and a ``doctor`` row.
     - ``docs/feats/plugin-feat.md``
   * - E1 Audit-rule plugins
     - Idea
     - Extra checks over a PKGBUILD, ``.install`` and patches, returning findings in the shape ``audit`` already has. The first extension point to build.
     - ``docs/feats/plugin-feat.md``
   * - E2-E4 Health-check, plan-note and formatter plugins
     - Idea
     - Extra ``doctor`` rows, extra notes under a plan, and new output formats for existing JSON reports.
     - ``docs/feats/plugin-feat.md``
   * - E5 Package-source plugins (Flatpak and others)
     - Idea
     - New sources without touching core transaction logic; needs its own design because it involves external tools and the transaction machinery.
     - ``docs/feats/plugin-feat.md``
   * - E6-E7 Event-observer and policy-gate plugins
     - Idea
     - Observers receive structured events and report them (notification, webhook, log). Gates can veto a transaction under a policy you configured; they only ever make it stricter, never approve what a built-in check stopped, and never apply to recovery.
     - ``docs/feats/plugin-feat.md``
   * - E8-E9 Read-only command and plan-proposal plugins
     - Idea
     - A namespaced read-only command from the data a plugin was granted, and plugins that propose operations that emrac turns into a normal previewed plan.
     - ``docs/feats/plugin-feat.md``
   * - Execution by proxy (``exec:<tool>``)
     - Idea
     - emrac runs a named, hash-pinned tool in the build sandbox on a plugin's behalf, from declared argument templates only; read-only by default, writes need a separate capability that ``--yes`` does not answer; never privileged, output treated as untrusted, every run logged. Phase 4 or later.
     - ``docs/feats/plugin-feat.md``
   * - Industry-tool plugin families
     - Idea
     - Adapters for Git and forges, containers, infrastructure as code, Kubernetes, CI/CD, observability, supply chain and security, build systems, artifact registries, notifications, editors and fleet tools. The tool stays a separately installed program; installs, removals and commits stay with emrac.
     - ``docs/feats/plugin-feat.md``
   * - Core-versus-plugin boundary
     - Idea
     - Protections for the transaction and for recovery stay built in so removing a plugin never removes one; a suggested plugin list was reviewed and roughly a third to a half duplicates the core.
     - ``docs/feats/plugin-feat.md``
   * - Signed plugin index, ``search``, revocation
     - Idea
     - A signed index with rollback protection and a revocation list, in phase 3 only. Shares the governance question of the AUR defense feed.
     - ``docs/feats/plugin-feat.md``
   * - Plugin SDK and template
     - Idea
     - A crate and template so writing a rule is not an exercise in the raw ABI.
     - ``docs/feats/plugin-feat.md``

Extensibility (SPEC III.21)
~~~~~~~~~~~~~~~~~~~~~~~~~~~

The other extensibility items in the spec, all future-scoped.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - Public API
     - Not started
     - A stable programmatic interface to the same engine the CLI uses (the machine-readable ``--json`` output is the first step).
     - ``docs/feats/SPEC.md`` III.21
   * - Remote build workers
     - Not started
     - Builds run on other machines while package management stays central.
     - ``docs/feats/SPEC.md`` III.21; compile farm in IV+
   * - Distributed compilation
     - Not started
     - Spread one build across machines (distcc style).
     - ``docs/feats/SPEC.md`` III.21; ``docs/feats/ideas.md`` (shared build acceleration)
   * - Reproducible build verification
     - Not started
     - Build twice, compare hashes, flag non-deterministic builds.
     - ``docs/feats/SPEC.md`` III.21; ``docs/feats/ideas.md``
   * - Binary cache servers
     - Not started
     - Serve verified local builds to other machines.
     - ``docs/feats/SPEC.md`` III.21; ``docs/feats/ideas.md`` (optional local repository)
   * - Team build profiles
     - Not started
     - Build profiles shared by a team.
     - ``docs/feats/SPEC.md`` III.21
   * - Package benchmarking database
     - Not started
     - Collected benchmark results feeding the recommendation engine.
     - ``docs/feats/SPEC.md`` III.21; IV.7

Research programme
~~~~~~~~~~~~~~~~~~

Reading and running other package managers to inform design; nothing here runs inside emrac. Findings so far are in ``research/``.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - R1 Version comparison (pacman, dpkg, rpm)
     - Research
     - Done: ``research/vercmp.md``, ``dpkg-versions.md``, ``rpm-versions.md``, a truth table, and the pacman rows pinned as a test.
     - ``docs/feats/rea-research-plan.md``
   * - R2 Transaction commit order and locking
     - Research
     - Done (by ``strace`` in a container): ``research/pacman-commit-order.md``, with upgrade, backup files, scriptlets, hooks and failure injection.
     - ``docs/feats/rea-research-plan.md``
   * - R3 Hooks and scriptlets
     - Research
     - Done: ``research/alpm-hooks.md``.
     - ``docs/feats/rea-research-plan.md``
   * - R4 Solver explanations and conflict resolution (aptitude, libsolv)
     - Research
     - Planned.
     - ``docs/feats/rea-research-plan.md``
   * - R5 Selective undo and generations (dnf history)
     - Research
     - Planned.
     - ``docs/feats/rea-research-plan.md``
   * - R6 Temporary build dependencies (apk ``--virtual``)
     - Research
     - Idea taken; built as u7 without a research pass.
     - ``docs/feats/rea-research-plan.md``
   * - R7 Closure size and generation storage (nix)
     - Research
     - Planned.
     - ``docs/feats/rea-research-plan.md``
   * - R8 Permission display model (flatpak)
     - Research
     - Planned.
     - ``docs/feats/rea-research-plan.md``
   * - Bin-package audit (network endpoints, self-updaters, bundled libraries)
     - Research
     - Needs a real target package.
     - ``docs/feats/rea-research-plan.md``
   * - P1 Subslot-style rebuilds
     - Idea
     - Record the library ABI each source build linked against; rebuild plans on change (Portage ``:=``).
     - ``docs/feats/rea-portage.md``
   * - P2 Preserved libraries
     - Idea
     - Keep an old ``.so`` that programs still need until dependents are rebuilt.
     - ``docs/feats/rea-portage.md``
   * - P3 Rebuild on changed inputs
     - Idea
     - Rebuild when flags or dependencies changed since the build.
     - ``docs/feats/rea-portage.md``
   * - P4 Saved build environment
     - Idea
     - Store each package's exact build inputs.
     - ``docs/feats/rea-portage.md``
   * - P5 ``--keep-going``
     - Idea
     - Continue a build queue past a failure and report what was skipped.
     - ``docs/feats/rea-portage.md``
   * - P6 Resume an interrupted run
     - Idea
     - A saved list lets a run continue.
     - ``docs/feats/rea-portage.md``
   * - P7 Separate phases
     - Idea
     - Fetch, build and install each run alone.
     - ``docs/feats/rea-portage.md``
   * - P8 Jobs and load-average limits
     - Idea
     - Limit parallel builds by load.
     - ``docs/feats/rea-portage.md``
   * - P9 ``--oneshot``
     - Idea
     - Install without marking it wanted.
     - ``docs/feats/rea-portage.md``
   * - P10 ``--depclean``
     - Idea
     - Remove unneeded packages after checking reverse dependencies.
     - ``docs/feats/rea-portage.md``
   * - P11 Per-package build environment overrides
     - Idea
     - Different flags or toolchain for one package.
     - ``docs/feats/rea-portage.md``
   * - P12 User patches applied on every build
     - Idea
     - A patch directory per package.
     - ``docs/feats/rea-portage.md``
   * - P13 License acceptance groups
     - Idea
     - Filter by license class.
     - ``docs/feats/rea-portage.md``
   * - P14 Autounmask-style suggestions
     - Idea
     - When resolution fails, propose the exact change and write it after confirmation.
     - ``docs/feats/rea-portage.md``
   * - P15 ``REQUIRED_USE``-style option rules
     - Idea
     - Valid option combinations.
     - ``docs/feats/rea-portage.md``
   * - P16 Install message log (``elog``)
     - Idea
     - Keep the messages a package prints and show them afterwards.
     - ``docs/feats/rea-portage.md``
   * - P17 ``emerge --info``-style environment dump
     - Idea
     - One command for bug reports (``doctor --report`` covers part).
     - ``docs/feats/rea-portage.md``
   * - P18 Package integrity check
     - Partial
     - Built as ``verify`` (hashes, modes, sizes from the mtree); modification times are not compared.
     - ``docs/feats/rea-portage.md``; ``docs/plan/r1/u9-verify.md``
   * - P19 Build sandbox and network sandbox
     - Done
     - The bubblewrap sandbox with the network off.
     - ``docs/feats/rea-portage.md``; ``docs/plan/r1/u19-sandbox.md``
   * - P20 Signed sync and signed binary packages
     - Idea
     - Verify signed commits and package signatures.
     - ``docs/feats/rea-portage.md``
   * - P21 Binary packages with matching rules
     - Idea
     - Reuse a prebuilt package only if its inputs match.
     - ``docs/feats/rea-portage.md``
   * - P22 USE-flag-like per-package toggles
     - Idea
     - Per-package feature toggles that change build options and dependencies.
     - ``docs/feats/rea-portage.md``

Decided against, or far future
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Recorded so nobody re-proposes them by accident.

.. list-table::
   :header-rows: 1
   :widths: 24 9 45 22

   * - Feature
     - Status
     - Notes
     - Tracked in
   * - AI or LLM features, telemetry, cloud lookups
     - Not planned
     - Decided against: emrac is deterministic and local. (``SPEC.md`` III.21 lists AI-assisted items; they are excluded.)
     - 
   * - Scripting of any kind
     - Not planned
     - Decided: no scripting engine, no embedded interpreter, no user-written scripts run by emrac, and no scripting layer for hooks or workflows. Everything is declarative files and fixed rules. Plugins are compiled WASM modules under enforcement, not scripts. (The restricted scripting layer idea was removed from ``ideas.md``.)
     - 
   * - Committing transactions through libalpm instead of the ``pacman`` binary
     - Not planned
     - A far-future idea of yours, deliberately not planned or scheduled. The research documents hold what dpkg and rpm do for atomic file replacement and journaling, which is where it would start.
     - 

Future scope: 77 tracked: 2 done, 5 partial, 8 not started, 45 idea, 5 planned (r4), 9 research, 3 not planned.

Totals
--------

.. list-table::
   :header-rows: 1
   :widths: 16 8 8 10 30 8

   * - Scope
     - Done
     - Partial
     - Not started
     - Idea / planned / research / not planned
     - Total
   * - Core
     - 5
     - 32
     - 22
     - 0
     - 59
   * - Extended
     - 28
     - 18
     - 0
     - 89
     - 135
   * - Future
     - 2
     - 5
     - 8
     - 62
     - 77
   * - **All**
     - **35**
     - **55**
     - **30**
     - **151**
     - **271**

Some features appear in more than one scope's list (soak time, the offline
bundle, migration mode); each table counts its own rows. The core catalogue
rows are categories, so one row there can cover many of the extended rows.

Source: ``docs/feats/SPEC.md`` Parts III, IV, V and X, ``docs/feats/``
(additions, ideas, Wandrover, self-management, AUR defense, forensics,
research), checked against ``crates/`` and ``CHANGELOG.md`` as of Slice r3
plus ``u1``-``u8`` after it. ``docs/feats/feats.md`` carries the same rows
in one flat list.

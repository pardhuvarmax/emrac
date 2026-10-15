# emrac-cli

The `emrac` program. (The crate is named `emrac-cli`; its binary is `emrac`.) A thin layer over [`emrac-core`](../emrac-core): it parses arguments ([`src/cli.rs`](./src/cli.rs), with `clap`), dispatches to [`src/commands/`](./src/commands), formats output ([`src/output.rs`](./src/output.rs)) as text or `--json`, and asks for confirmation ([`src/prompt.rs`](./src/prompt.rs)) before anything changes. No business logic lives here.

Commands: `search`, `info`, `install`, `remove`, `upgrade`, `build`, `audit`, `doctor`, `history`, `rollback` / `undo` / `redo`, `downgrade`, `reinstall`, `rebuild`, `purge`, `verify`, `snapshot`, `restore`, `deps` / `rdeps` / `why` / `orphans` / `tree` / `graph` / `impact` / `cycles`, `pin` / `unpin`, `pacnew`, `cache` (with the shortcuts `cc` and `cs`), `news`, `report`, `ledger`, `self`, `config`, `completion`. The root [`README.md`](../../README.md) has the table; `emrac --help` the details. Global flags: `--dry-run` (`-n`), `--yes` (`-y`), `--offline`, `--json`, `--quiet`, `--verbose`.

`emrac --version` and `--help` name the builder (`pardhuvarmax`) and the project URL; both come from the workspace package metadata.

Tests: 11 unit tests here and the 28 integration tests in [`tests/`](./tests), all listed in [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst). Commands that change the system are tried in a container: see [`dev/`](../../dev).

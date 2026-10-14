# tests/

Integration tests for the `emrac` binary: [`cli.rs`](./cli.rs) runs the compiled program as a user would, with a temporary `HOME` so nothing touches the real config, history or caches, and checks its output and exit status. 18 tests, listed in [`docs/tests/emrac-tests.rst`](../../../docs/tests/emrac-tests.rst).

What they cover:

- `--version` (names the builder) and `--help`; usage errors exit with status `2`;
- shell completion for bash, zsh and fish;
- the config file: set, get, unset, errors;
- history, snapshots and pins that only touch emrac's own files;
- a closed pipe (`emrac … | head`) does not panic;
- `audit`: PKGBUILDs graded by severity and exit code, `--json`, a whole package folder, a built package file, scripts shipped in a package;
- the build sandbox hides the network, secrets and the home directory, and resource limits stop a runaway build and name the reason;
- `doctor`: checks as JSON with the problem count as the exit status, and a markdown report without the user's identity.

Tests that need bubblewrap or a systemd user session check for them first and pass without asserting when the machine has none, so they are safe to run anywhere. Commands that change the system are tested in the container instead: see [`dev/`](../../../dev).

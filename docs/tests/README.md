# tests/

[`emrac-tests.rst`](./emrac-tests.rst) lists every automated test that passes (each suite heading gives its count), grouped by suite and module, and the end-to-end checks.

It is generated, not written by hand:

```sh
dev/gen-tests-doc.sh           # runs the tests and rewrites the file
dev/gen-tests-doc.sh --check   # runs the tests and fails if the file is stale
```

CI runs `--check`, so a test that is added, renamed or removed always appears in the same change. A failing test stops the script, so the file only ever holds tests that passed; the script also stops if the number of listed tests differs from the number the run reports as passed, or if any test is ignored. Commit the regenerated file with the change that touched the tests.

The end-to-end sections are built from the titles in [`dev/e2e/final-check.sh`](../../dev/e2e/final-check.sh), [`dev/e2e/preflight.sh`](../../dev/e2e/preflight.sh) and [`dev/e2e/self-upgrade.sh`](../../dev/e2e/self-upgrade.sh) and their "last full run" comments; those checks need podman, so they are run by hand with `dev/e2e/run.sh`, `dev/e2e/run.sh preflight` and `dev/e2e/run.sh self`.

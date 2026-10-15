# e2e/

End-to-end checks: the release binary against a real pacman, in the disposable container from [`../container/`](../container).

- [`final-check.sh`](./final-check.sh) — the checks, one `ck "title" "command"` line each (26 at r2): version and exit codes, search and info, install and remove, history, verify, snapshots, pins, undo and redo, audit, an AUR package built in the sandbox and installed, `cache`, `pacnew`, `doctor`, config, completion, and a closed stdin declining. A comment at the top records the last full run; update it when you run them all.
- [`run.sh`](./run.sh) — builds the release binary with [`../release-build.sh`](../release-build.sh), builds the container image and runs the checks in it.

```sh
dev/e2e/run.sh
```

`dev/e2e/run.sh install` runs [`install-script.sh`](./install-script.sh) instead: the root `install.sh` in the container — its options, its refusals (not Arch, no terminal, wrong checksum), a real install piped through `sh` the way `curl | sh` runs it, a second run and the removal.

`dev/e2e/run.sh preflight` runs [`preflight.sh`](./preflight.sh) as root: it builds a small local repository so that an upgrade is really waiting, then checks the partial-upgrade gate, `--allow-partial`, a pin, the `warn` setting and the refusal to remove the only (fake) kernel, and the plan ids (`--expect-plan`, and a plan whose repository changed during a 36 second wait).

`dev/e2e/run.sh self` runs [`self-upgrade.sh`](./self-upgrade.sh): it builds a 0.8.0 and a 0.9.0 release from the committed source, installs 0.8.0 as a real package, then checks `self sync`, a tampered tarball refused, an upgrade killed half way, a real `self upgrade` to 0.9.0 and a real `self downgrade` back (it compiles emrac three times, so it takes several minutes).

Needs podman and network access, so CI does not run these. The check titles are listed in [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst), which `dev/gen-tests-doc.sh` regenerates.

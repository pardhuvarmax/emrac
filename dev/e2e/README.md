# e2e/

End-to-end checks: the release binary against a real pacman, in the disposable container from [`../container/`](../container).

- [`final-check.sh`](./final-check.sh) — the checks, one `ck "title" "command"` line each (26 at r2): version and exit codes, search and info, install and remove, history, verify, snapshots, pins, undo and redo, audit, an AUR package built in the sandbox and installed, `cache`, `pacnew`, `doctor`, config, completion, and a closed stdin declining. A comment at the top records the last full run; update it when you run them all.
- [`run.sh`](./run.sh) — builds the release binary with [`../release-build.sh`](../release-build.sh), builds the container image and runs the checks in it.

```sh
dev/e2e/run.sh
```

`dev/e2e/run.sh install` runs [`install-script.sh`](./install-script.sh) instead: the root `install.sh` in the container — its options, its refusals (not Arch, no terminal, wrong checksum), a real install piped through `sh` the way `curl | sh` runs it, a second run and the removal.

`dev/e2e/run.sh preflight` runs [`preflight.sh`](./preflight.sh) as root: it builds a small local repository so that an upgrade is really waiting, then checks the partial-upgrade gate, `--allow-partial`, a pin, the `warn` setting and the refusal to remove the only (fake) kernel, the plan ids (`--expect-plan`, and a plan whose repository changed during a 36 second wait), the Arch news gate (critical news stops an upgrade), the pacman database backup and lock, an older version than the installed one, a rolled-back repository, and the update risk and report.

`dev/e2e/run.sh self` runs [`self-upgrade.sh`](./self-upgrade.sh): it builds a 0.8.0 and a 0.9.0 release from the committed source, installs 0.8.0 as a real package, then checks `self sync`, a tampered tarball refused, an upgrade killed half way, a real `self upgrade` to 0.9.0 and a real `self downgrade` back, then the same on a copy placed by hand in `~/.local/bin` (updated in place, the old file kept; it compiles emrac three times, so it takes several minutes).

`dev/e2e/run.sh preview` runs [`preview.sh`](./preview.sh) as root: what emrac says a plan will do beyond its packages is compared with what pacman then does. Probe hooks are placed next to the system ones and the hooks emrac lists are compared, pass by pass and in order, with the ones pacman runs for an install, a removal and a full upgrade; a small local repository with real `backup=` files shows which config files become `.pacnew` or `.pacsave`.

`dev/e2e/run.sh recovery` runs [`recovery.sh`](./recovery.sh) as root: pacman is made to fail the way a full disk does in the middle of an install (strace fault injection), then emrac must notice the package pacman still lists with a file missing, name the command that mends it, and that command must mend it.

`dev/e2e/run.sh buildtools` runs [`buildtools.sh`](./buildtools.sh) as the ordinary user: a local checkout posing as an AUR package has a build-only dependency and a runtime one; after the build emrac must offer to remove exactly what the build added and nothing needs, in each of the `build.remove_make_deps` modes.

`dev/e2e/run.sh timeline` runs [`timeline.sh`](./timeline.sh) as the ordinary user: real installs, a removal, an undo and a snapshot plus a build-times fixture, told by `emrac timeline` by day and newest first, with the `--pkg` and `--since` filters, the JSON form and the refusal of a bad period.

`dev/e2e/run.sh stats` runs [`stats.sh`](./stats.sh) as the ordinary user: `emrac stats` over a build-times file with known builds (all time, `--since`, failed builds left out of the averages, the refusal of a bad period) and `emrac benchmark` of an installed package (nothing runs without an answer, the programs are listed first, a result is kept and shown by `--history`, packages with no programs and unknown executables are refused).

`dev/e2e/run.sh policy` runs [`policy.sh`](./policy.sh) as root: a small package is upgraded, reinstalled and removed by real pacman while an override and a diversion are set; it shows what pacman does without the policy, what survives with it, the refusals, the warnings about a frozen file, the untrusted-executable refusal, and the plan preview.

Needs podman and network access, so CI does not run these. The check titles are listed in [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst), which `dev/gen-tests-doc.sh` regenerates.

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

`dev/e2e/run.sh clui` runs [`clui.sh`](./clui.sh) as the ordinary user, with emrac under a real pseudo-terminal (`script`) and piped: the `[clui]` settings and their refusals; marks, boxes and tables on a terminal and the old text piped; colour by terminal, `NO_COLOR`, `--color never|always`, `CLICOLOR_FORCE`, `TERM=dumb`, `COLORTERM=truecolor` and the themes; ASCII for a non-UTF-8 locale, `clui.unicode never` and the Linux console; `COLUMNS` narrowing the layout; the `emrac ui` gallery; and the animations: none by default, with `clui.animations active` the cursor hidden, a spinner, one result line, the cursor shown again, nothing piped, in CI, on a dumb terminal, with `--json` or `--quiet`, and the cursor shown again after Ctrl-C.

`dev/e2e/run.sh mirrors` runs [`mirrors.sh`](./mirrors.sh) as root (so `--apply` can use sudo without a password), with real mirrors over the network and one address certain to fail: the list with nothing measured, `rank` refused before a measurement, bad files refused, `test` recording and showing a failure, `--limit`, groups and numbers in the list, the unreachable mirror last, `rank` changing nothing, `--write` keeping the header and comments and the same active mirrors and no others (and not overwriting), `--keep`, JSON, and `--apply` (no answer: unchanged; `--yes`: replaced, the old one saved unchanged, mode 644, a second apply finding nothing to change).

`dev/e2e/run.sh advisor` runs [`advisor.sh`](./advisor.sh) as the ordinary user: no advice with the default limits on a small plan, then with the limits lowered on `vim` the large-download, disk-growth and many-packages advice (naming the biggest package and giving the `emrac tree` command), `advice` in the JSON plan with the plan id unchanged, `advisor.enabled false` and a limit of 0 turning it off, bad values refused, an install still going ahead, and a removal getting none.

`dev/e2e/run.sh bundle` runs [`bundle.sh`](./bundle.sh) as the ordinary user, downloading real packages: `bundle create` of a package and its dependencies (every package has a signature file, a folder that exists or a package that does not exist is refused and leaves nothing), `verify` on a good bundle and on a changed byte, a missing signature and an unlisted file, `apply` (dry run, a damaged bundle refused, the real install through the pre-flight and into the history, a second apply finding nothing to do), `--for` leaving out what the target has, an upgrade bundle for an older target, a bundle whose signature is bad and whose manifest was edited to match (verify cannot tell; pacman refuses it), and a bundle older than what is installed (never a downgrade).

`dev/e2e/run.sh helppages` runs [`helppages.sh`](./helppages.sh) as the ordinary user: `man` for a command, a subcommand, `emrac` itself and every command as roff, `docs`, `license` (and the whole text offline), `credits`, `support`, `feedback`, and `bug-report` (the doctor report, the failed-transactions section, scrubbed of the user name and home path, and the line that it was only printed).

`dev/e2e/run.sh exchange` runs [`exchange.sh`](./exchange.sh) as the ordinary user: `emrac export` of a real machine (the file, no overwrite, no protection or self-update setting in it, no server addresses), the round trip (remove two packages, `import` installs them through the install path), another version reported and left alone, and a hostile file: `self.*` and `safety.*` reported as not applied and really unchanged, an unknown repository only reported, `--yes` not answering the hold question, and bad files refused.

`dev/e2e/run.sh graph` runs [`graph.sh`](./graph.sh) as the ordinary user: `emrac graph --format boxes` for real packages (the root in a double box, every direct dependency drawn, a shared package once, the size refusal, JSON and dot unchanged), then `--format easy` refused cleanly without `graph-easy`, installed from the AUR by emrac itself, and drawn with it.

`dev/e2e/run.sh timeline` runs [`timeline.sh`](./timeline.sh) as the ordinary user: real installs, a removal, an undo and a snapshot plus a build-times fixture, told by `emrac timeline` by day and newest first, with the `--pkg` and `--since` filters, the JSON form and the refusal of a bad period.

`dev/e2e/run.sh stats` runs [`stats.sh`](./stats.sh) as the ordinary user: `emrac stats` over a build-times file with known builds (all time, `--since`, failed builds left out of the averages, the refusal of a bad period) and `emrac benchmark` of an installed package (nothing runs without an answer, the programs are listed first, a result is kept and shown by `--history`, packages with no programs and unknown executables are refused).

`dev/e2e/run.sh policy` runs [`policy.sh`](./policy.sh) as root: a small package is upgraded, reinstalled and removed by real pacman while an override and a diversion are set; it shows what pacman does without the policy, what survives with it, the refusals, the warnings about a frozen file, the untrusted-executable refusal, and the plan preview.

Needs podman and network access, so CI does not run these. The check titles are listed in [`docs/tests/emrac-tests.rst`](../../docs/tests/emrac-tests.rst), which `dev/gen-tests-doc.sh` regenerates.

# libalpm hooks: discovery, execution, failure

Subject: `libalpm.so.16.0.1` (pacman 7.1.0.r9.g54d9411-3). Method: Ghidra via REA on the
installed library, cross-read with `alpm-hooks(5)`. Prose only; no code reproduced.
Labels: **[bin]** read from the binary, **[man]** from the manual page only, **[?]** not confirmed.

## Discovery

- **[bin]** Each configured hook directory is read in turn. Files whose name does not end in
  `.hook` are skipped (logged at debug level). Subdirectories are skipped.
- **[bin]** A file whose basename matches one already loaded from an earlier directory is
  skipped as "overridden". So earlier directories win on name collision, which matches the
  manual's override rule. **[man]** The manual says the system dir is
  `/usr/share/libalpm/hooks` and `/etc/pacman.d/hooks` is the configured default, with
  later directories taking priority. Which side is "earlier" depends on how the front end
  orders the list it hands to libalpm; not traced **[?]**.
- **[man]** A symlink to `/dev/null` disables a hook. **[bin]** Files are `stat`ed, and the
  parse step runs on whatever is opened; the /dev/null case is consistent with an empty
  parse that is rejected or ignored, but the exact branch was not traced **[?]**.
- **[man]** Hooks run in alphabetical order of filename with the suffix ignored.

## Parse-time validation **[bin]**

Each of these aborts loading that hook with an error naming the file:
missing trigger targets, missing trigger operation, missing trigger type, missing `Exec`,
missing `When`, and `AbortOnFail` set on a `PostTransaction` hook. A failed parse is logged
and the hook is dropped; other hooks still load.

## Trigger matching

Types `Path`/`Package`, operations `Install`/`Upgrade`/`Remove`, glob targets with `!`
negation, later targets overriding earlier: **[man]** only. Matching code was not traced.
Man-page caveats worth encoding in tests: an install over an existing file counts as
`Upgrade` even if ownership changes; `.pacnew` extractions trigger on the original name;
removal triggers on every file the package owned whether or not present on disk.

## Execution **[bin]**

For each matched hook, in order:

1. Log `running '<description>'...` to the pacman log and fire the front-end callbacks
   (start-of-hook-run, per-hook start, per-hook end, end-of-run).
2. **Depends check:** every `Depends` entry must have a satisfier among installed packages.
   If one has none, the hook is not run and the error
   `unable to run hook <name>: could not satisfy dependencies` is reported. This counts as a
   hook **failure** for the abort rule below.
3. If `NeedsTargets` is set: the matched target names are sorted bytewise and
   **de-duplicated**, then written to the hook's stdin one per line. Otherwise stdin is an
   empty stream.
4. The command runs through the shared "run in root" helper (also used by scriptlets;
   three call sites in total, the other two presumably package scriptlets **[?]**):
   - the current directory is remembered, then changed to the configured root
   - output: stdout and stderr go to one socket pair read by the parent; stdin is the
     other socket pair
   - in the child: root `chroot` is applied unless root is exactly `/`; then `chdir("/")`
   - environment: `SHLVL` set to `1` only if unset, `BASH_ENV` removed, umask `022`
   - two more setup calls run before `execv` that were not identified **[?]** (likely fd or
     signal cleanup)
   - the command is `execv`ed directly, **no shell**. `Exec` is split on whitespace with
     quote support by the parser, so shell syntax in `Exec` does nothing.
   - exec failure prints `call to execv failed` and exits 1 in the child.
5. The parent polls both sockets. Output is split on newlines; each line is written to the
   log as an `ALPM-SCRIPTLET` entry and passed to the front-end callback. Lines longer than
   about 2 KB are flushed in chunks. A trailing partial line is flushed with a newline added.
6. After EOF the parent `waitpid`s and decides: exit status 0 means success; a non-zero
   exit prints `command failed to execute correctly`; death by signal prints
   `command terminated by signal N: <name>`. (A signal in the low 7 bits that equals 0x7f,
   i.e. stopped state, is treated as success by the status test. Edge case.)
7. The parent restores its original working directory.

## Failure handling **[bin]**

- Non-zero exit, signal death, fork/pipe errors and unsatisfied `Depends` all mean "hook
  failed".
- A failed hook with `AbortOnFail` set makes the runner stop immediately and return
  failure, but only in the PreTransaction pass. Remaining hooks do not run.
- A failed hook without `AbortOnFail` is only reported. The runner continues and returns
  success unless an earlier abort-flagged hook failed.
- PostTransaction hooks cannot abort (parser rejects the flag). **[man]** They do not run at
  all if the transaction failed.
- What the caller does with the failure (rolling back before the commit starts) was not
  traced; `alpm-hooks(5)` says the transaction is aborted **[man]**.

## Rust-port consequences

- Parse and validate hooks eagerly; drop bad ones with a message instead of failing the run.
- Implement `Depends` as a check against installed provides, with the same failure class.
- Do not shell-wrap `Exec`. Split on whitespace honoring quotes, `execv` semantics.
- `NeedsTargets`: sort, then de-duplicate before writing.
- Keep stdout/stderr merged and line-buffered into the log with an `ALPM-SCRIPTLET` tag.
- Run in the target root with `chroot` (skipped for `/`), then `chdir("/")`, umask 022,
  `SHLVL=1` if unset, `BASH_ENV` unset.

## Status

Discovery loop, parse validation, runner loop and exec helper traced statically. Behavior
then confirmed by running real hooks in a container (details in `pacman-commit-order.md`):
- `AbortOnFail` + failing PreTransaction hook: transaction aborted before any write, exit 1.
- Failing hook without `AbortOnFail`: reported, next hook still runs, transaction proceeds.
- `NeedsTargets`: matched package name arrives on stdin.
- Missing `Depends` + `AbortOnFail`: `unable to run hook <file>: could not satisfy
  dependencies`, aborted.
Not traced: trigger matching, hook ordering/sort, the two unidentified pre-exec calls, the
caller's abort handling beyond the observed result. Not run: `Type = Path` triggers and
missing `Depends` without `AbortOnFail`.

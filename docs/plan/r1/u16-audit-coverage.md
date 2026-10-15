# Slice u16 — audit coverage: extra repo files and more obfuscation

Follow-up to u15's stated limits. (The third limit, the manifest of what a
*built package* installs, needs the build and install steps split and is its
own slice.)

## Extra files in the AUR repo

An AUR repo holds more than the PKGBUILD: helper scripts, patches, committed
binaries. A malicious patch or script is as good as a malicious `build()`.
Every file at the audited revision is now classified:

| File | Treatment |
|---|---|
| `PKGBUILD`, the declared scriptlet | as before |
| shell/Python/Perl/Ruby/JS/Lua/PHP scripts (by shebang or extension), other `.install`/`.hook` | scanned in **script mode**: the dangerous-pattern rules at full strength (persistence, destructive, reverse shell, exfiltration, decode-and-run, download-and-run); the PKGBUILD-only rules (sudo, network-in-build, writes outside the package) don't apply — a shipped wrapper may legitimately do those |
| `*.patch`, `*.diff` | only the **added** lines, script rules, severity **capped at warning** and labelled "in a patch" (a patch adding `sudo` to an upstream script is common; one adding `curl \| sh` is still worth a warning) |
| ELF binary | **high**: an executable committed to the repo can't be read |
| other binary (not images/fonts/keys) | warning: opaque |
| docs, licenses, keys/signatures, images, fonts, `.SRCINFO` | skipped as data |
| over 1 MiB | info: too large to scan |

Reading is by `git ls-tree`/`git show` at the same revision as the PKGBUILD
(`FETCH_HEAD` for a pending update, `HEAD` otherwise), so the files audited are
the ones that would be built. For a path target the directory is walked
(not `.git`, `src`, `pkg`, built packages; depth 3, at most 200 files).

## More obfuscation coverage

- **download-then-run** (high): a file fetched by `curl`/`wget`/`aria2c`
  (`-o`, `-O`, redirection) and executed, sourced or `chmod +x`'d later in the
  same file.
- **extract-pipe-exec** (high): `gpg -d`, `zcat`, `unxz`, `tar -O`, `unzip -p`,
  `openssl` … piped straight into an interpreter.
- **obfuscated-command** (high): a command spelled with `$'\x..'` escapes.
- **dynamic-command** (warning): the command word is a bare variable or
  substitution (`$a$b`, `${cmd}`), except the usual tool variables
  (`$CC`, `$MAKE`, …).
- **encoded-blob** (warning): a 200+ character base64-looking token in code.
- Text-level fallbacks so non-shell scripts are covered: `curl … | sh` inside
  a quoted `os.system(...)`, `exec(`/`eval(` of a decoded payload, and the
  Python socket + `dup2`/`pty.spawn` reverse-shell idiom.

Findings are still per file: a credential read in one file and an upload in
another is not correlated (stated limit).

## Verification

Unit tests for every new rule both ways; the 67-PKGBUILD survey re-run **with
every file in each repo** (the false-positive gate: still no high/critical on
the official and popular AUR packages); planted malicious helper scripts and
patches through the real review flow.

## Found while testing (all fixed, all pinned by tests)

- `install=$pkgname.install` (the usual form) was never resolved, so most
  scriptlets were not being read at all. Names are now expanded from the
  PKGBUILD's own plain assignments.
- `x=$(curl … | sh)` at top level runs when the PKGBUILD is merely read, but
  assignments never reached the command rules. Assignments with a command
  substitution are now scanned.
- A reverse shell written as `s = socket.socket()` on one line and
  `os.dup2(...)` on another was missed; it is now decided per file from the
  raw text.
- Persistence is judged by where a command **writes** (redirect target, `tee`,
  `cp`/`mv`/`ln`/`install` destination, `dd of=`, `sed -i`), not by a path
  appearing anywhere on the line: `grep x /etc/passwd > /dev/null` is not a
  write. Two tiers: SSH keys, `ld.so.preload`, sudoers, account files and
  `/etc/crontab` are critical everywhere; **shell startup files** are critical
  in a PKGBUILD but **high** in an install scriptlet or shipped script, because
  some legitimate packages (e.g. `nvm-git`) append a `source` line there. This
  refines the severity table in the design doc.
- The command-word reader now understands `case`/`for` headers, array
  assignments and `x=$(cmd …)` (where the command that runs is `cmd`), which
  removed most `dynamic-command` noise (39 → 6 over the survey) and sharpened
  the other rules.
- A single-file target audited its whole parent directory, recursively; a file
  target now looks only at the files beside it.

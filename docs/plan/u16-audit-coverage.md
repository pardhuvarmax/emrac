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

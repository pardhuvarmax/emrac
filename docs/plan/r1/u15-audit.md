# Slice u15 — `emrac audit` (static PKGBUILD analysis)

Implements §3.1 and the severity policy of §6 in `docs/design/source-build.md`.

## Design

- **`audit.rs` (core, pure)**: a conservative PKGBUILD reader and a rule set.
  - Reader: declarative assignments (scalar and multi-line arrays, quotes,
    comments) and function bodies (`name() { … }`, brace-matched, quote- and
    heredoc-aware: heredoc bodies are data and skipped). Nothing is executed.
    A value with command substitution (`$(…)`, backticks, `<(…)`) is marked
    *dynamic* and reported as such rather than guessed.
  - Commands inside functions are split on `;`, `&&`, `||` and `|` (outside
    quotes); the command word of each segment (path and leading `VAR=`
    stripped) drives the rules, so `curl … | sh` is seen as a pipeline.
  - Rules emit a **finding**: rule id, severity, file (`PKGBUILD` or the
    `.install` scriptlet), line, message, excerpt. Rule list and severities
    are in `audit.rs` and its tests; the tiers are those of the design doc.
  - Scriptlets (`install=`) run as root, so they get the same function rules
    (except path-writing rules, which don't apply to them) plus an info finding.
- **Policy**: `Verdict::{Allow, Confirm, Block}` from the highest severity:
  critical → Block, high → Confirm, otherwise Allow.
- **`emrac audit <target>...`**: a target is an AUR package name (its upstream
  PKGBUILD and scriptlet are read from emrac's git checkout, cloned or fetched
  but not advanced — same as the review step) or a path to a PKGBUILD / its
  directory. Prints findings (or "no findings"), `--json`. Exit `0` with only
  info/warning, `1` if any target has high or critical findings (usable in
  scripts). Read-only apart from the build cache checkout.
- **In the install/upgrade review**: the audit runs on exactly the PKGBUILD and
  scriptlet about to be built, *even with `--skip-pkgbuild`/`--skip-diff` and
  even when the checkout is up to date*. Findings print before the review
  prompt. Critical → the operation is refused (`found`-voiced error naming the
  findings). High → an extra `emrac asks:` confirmation that `--yes` does not
  answer; `--accept-risks` (install/upgrade) does. Warnings don't add a prompt.
- Not logged in the history.

## Known limits (stated in the output and docs)

Pattern rules can miss obfuscation they don't know and can flag legitimate
code; the audit is a reading aid, not a guarantee. Extra files in the AUR repo
(patches, helper scripts) are not scanned in this slice. The install-time
manifest of the built package is u18.

## Files

- `audit.rs` (new, core); `backend/aur_build.rs` — `read_file` at a revision;
  `sources.rs` — `audit_aur`, `audit_path`; `error.rs` — `AuditBlocked`,
  `RisksNotAccepted`.
- CLI: `commands/audit.rs`, `aur_review.rs`, `cli.rs` (`audit`, `--accept-risks`),
  `output.rs`.

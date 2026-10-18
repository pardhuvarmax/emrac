# dpkg version comparison

Subject: `dpkg` 1.23.11-1 (Arch build, stripped, libdpkg linked statically), analysed in a
throwaway container copy. Ground truth comes from `dpkg --compare-versions` run in the same
container. Behavior is described in prose; no decompiled code is reproduced.

Scope: the comparison algorithm and the version parser. The status-file parser (field layout)
was **not** traced yet; see Status.

## Parsing a version string

1. Leading and trailing whitespace is ignored. Embedded whitespace is an error.
2. Empty string is an error.
3. Epoch: if a `:` is present, the text before the first `:` must be a plain non-negative
   decimal number below 2^31. Errors: empty, negative, too big, not a number, nothing after
   the colon. No `:` means epoch 0.
4. Revision: split at the **last** `-`. No `-` means revision is the empty string, and an
   empty revision after a `-` is an error.
5. Upstream version (what is left) must be non-empty and **must start with a digit**.
6. Allowed characters: upstream = alphanumerics and `. - + ~ :`; revision = alphanumerics
   and `. + ~`. Anything else is "invalid character" (a warning-class error in the library;
   this build reports it as a parse failure for `--compare-versions`).

## Comparison

Compare epoch as integers, then upstream, then revision. First difference wins.
A missing revision counts as the empty string, so `1.0` < `1.0-1`.

Upstream and revision use the same routine. Repeat until both strings are exhausted:

1. **Non-digit pass.** Walk both strings while either current char is a non-digit. Compare
   chars by weight, first difference wins:
   - `~` weighs lowest of all, even below end-of-string
   - end-of-string and digits weigh 0
   - letters weigh their own byte value
   - every other char weighs its byte value + 256 (so punctuation sorts above all letters)
2. **Digit pass.** Skip leading zeros on both. Then compare the digit runs: the first
   digit difference is remembered, but a longer run wins outright, so numeric order holds
   for any length. Empty run counts as 0.
3. Loop.

Consequences: `1.0~rc1` < `1.0`; `1.0~~` < `1.0~` < `1.0`; `1.0a` > `1.0`;
`1.0+1` < `1.0.1` (`+` is byte 43, `.` is byte 46, both plus 256).

## Observed (dpkg column of the table below, -1 left older, 1 left newer)

See `research/version-truth-table.txt`. Highlights that differ from pacman:
`1.0~rc1 < 1.0`, `1.0a > 1.0`, `1.0.` > `1.0`, `a` > `1` (but `a` is not a valid
upstream version; the tool accepts it in comparison but real parsing requires a digit start).

## Test cases for the Rust implementation

Use every row of `version-truth-table.txt`, dpkg column. Add parse-failure cases for each
error above (`:1`, `-1:1`, `x:1`, `1:`, `1.0-`, ` `, `a1`, `1.0 1`).

## Status file (`/var/lib/dpkg/status`) and how dpkg writes it

Observed by installing a real `.deb` in a container and by feeding crafted status files to
`dpkg-query -W` (dpkg 1.23.11). Behavior only.

### Layout

- Text file of stanzas separated by one or more blank lines. Each stanza is one package
  (or one architecture instance of it). A package in an installed state with all
  its fields looks like: `Package`, `Status`, `Priority`, `Section`, `Installed-Size`,
  `Maintainer`, `Architecture`, `Version`, then `Depends` and similar, `Conffiles`, and
  `Description` last. Field order in the file is dpkg's own, not the order in the
  `.deb`'s control file.
- `Status` is three words: `want flag state`, e.g. `install ok installed`; after a plain
  remove it becomes `deinstall ok config-files`, and a purge deletes the stanza entirely
  (status file was empty again).
- After a remove, `Config-Version: <ver>` appears, and `Conffiles:` keeps the checksums.
- `Conffiles:` is a multi-line field: one ` <path> <md5>` per continuation line.
- `Description:` has a one-line summary then continuation lines starting with one space; a
  line containing only ` .` stands for a blank line.
- Per-package companion files in `/var/lib/dpkg/info/`: `<pkg>.list` (every path, one per
  line, starting with `/.`, parents before children), `<pkg>.md5sums` (`md5  relpath`,
  two spaces, no leading slash), `<pkg>.conffiles`, and one file per maintainer script
  (`.postinst` etc.). `info/format` holds the info-dir format version. For multi-arch
  packages the files are named `<pkg>:<arch>.*`.

### Parser behavior (all confirmed by execution)

| input | result |
|---|---|
| field names in lower case (`package:`) | accepted, case-insensitive |
| unknown fields, `X-...` fields | accepted and kept |
| space before colon (`Package : a`) | accepted |
| no space after colon (`Package:a`) | accepted |
| empty value (`Depends:`) | accepted |
| continuation lines starting with space or tab | accepted |
| several blank lines between stanzas | accepted |
| CRLF line endings | accepted with a warning about a blank line in a value |
| duplicate field in one stanza | **error**, exit 2: `duplicate value for 'Package' field` |
| `#` comment line | **error**: `field name '#' must be followed by colon` (no comments) |
| missing final newline | **error**: `end of file during value of field ... (missing final newline)` |
| missing `Package` | **error** |
| missing `Version` on an installed package | **error** `missing 'Version' field` |
| `Status` with 2 words | **error**: `third (status) word ... is missing a value` |
| `Status` with unknown state | **error**: `has invalid value 'bogus'` |
| two stanzas for the same name and arch | **error**: `multiple non-coinstallable package instances` |
| same name, different arch with `Multi-Arch: same` | accepted, two entries (both printed as `a` by `${Package}`) |
| missing `Description` / `Maintainer` | warning only |

Any error makes the whole query fail (no partial result, exit 2).

### How dpkg writes the status file (install of one package)

1. Take a write lock on `/var/lib/dpkg/lock` (`fcntl F_SETLK`), then a second lock file in
   `triggers/` (`F_SETLKW`).
2. Read `status`. Write a journal record to `updates/tmp.i`, `fsync` it, then
   `rename` it to `updates/0000`, `fsync` again (an interrupted run is replayed from
   `updates/` next time).
3. Extract the control data to `/var/lib/dpkg/tmp.ci/` via `dpkg-deb --control`.
4. Rewrite the whole status file: write `status-new`, `fsync` it, delete `status-old`,
   **hard-link** `status` to `status-old`, then `rename status-new` to `status`, then
   `fsync` the directory `/var/lib/dpkg` and `updates/`.
5. Payload files: written as `<path>.dpkg-new` (created `O_EXCL`, mode 000), then `rename`d over `<path>`.
   dpkg also probes for `<path>.dpkg-tmp` and `<path>.dpkg-new` leftovers first and removes
   them; on a fresh install those probes returned ENOENT. What it does with `.dpkg-tmp` on an
   upgrade was not exercised. Directories use the same trick (`mkdir <dir>.dpkg-new`, rename).

Compare with pacman, which writes in place and never syncs (see `pacman-commit-order.md`).

### Rust-port notes

- Parse eagerly and fail the whole read on the error cases above.
- Be case-insensitive on field names, strict on `Status` words, tolerant of CRLF.
- Group by (`Package`, `Architecture`); only coinstallable (`Multi-Arch: same`) duplicates are legal.

## Status

Comparison: traced and confirmed. Parser: confirmed by execution. Status-file layout and
write protocol: confirmed by execution. Not covered: `available` file, `diversions` and
`statoverride` formats, triggers files, the `Status` want/flag combinations beyond
`install ok installed` and `deinstall ok config-files`, `Provides`/`Depends` grammar.

# Slice u18 — close the gaps left open by u15–u17

Each item below was named as "not covered" in an earlier slice.

## 1. Scripts shipped inside a built package

The manifest read the file list and a few known config files, but not the
scripts a package puts in `usr/bin` and friends — a wrapper script is as good a
place for a payload as `build()`. Now: every small (≤128 KiB) executable or
script-looking file, up to 300, is read and run through the script rules.

Reading each member with its own `bsdtar -xO` would decompress the archive once
per file. Instead one `bsdtar -x` extracts exactly the wanted members (the
config files already inspected plus those scripts) into a private 0700 temp
directory, which is removed afterwards; nothing is executed and setuid bits are
not restored. This also speeds up the existing hook/unit/sudoers reads.

Prebuilt ELF binaries and nested archives (`.zip`, `.jar`, `.tar.*`, …) can't be
read by static rules: they are **counted in the summary** (a `-bin` package is
expected to ship binaries; a source-built one is not), not judged.

## 2. Credential read in one file, upload in another

`secret-access` and the upload rule were correlated only within a file. The
audit now collects both facts across the PKGBUILD, scriptlet and every extra
file, and rates an upload **critical** when any *other* file reads credentials.

## 3. Opt-in inspection of official packages before install

Official packages are signature-verified by pacman, so by default they are not
inspected. `--inspect` (or `review.inspect_official = true`) on `install` and
`upgrade` downloads first (`pacman -Sw` / `-Syuw`), prints each package's
manifest from the cache, and then installs. Policy is deliberately lighter than
for AUR builds: official packages legitimately ship setuid helpers and hooks, so
findings are **shown** and only **critical** ones stop the install.

## 4–5. Verification gaps

Run the `reinstall` AUR-rebuild branch for real (checkout present, no built
file), and repeat the earlier container scenario several times to look for the
unexplained blank run.

## Still not coverable (stated, not hidden)

What a compiled binary does; a sufficiently obfuscated payload; behaviour that
only appears at runtime. The sandbox slices are the mitigation for those.

## Files

`audit.rs` (facts across files), `manifest.rs` (scripts, counts, wanted
members), `backend/package_file.rs` (extract), `backend/pacman.rs` (download),
`sources.rs`, `config.rs`, CLI `install`/`upgrade`/`output`.

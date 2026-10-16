# Releasing, and receiving a small update

How a change reaches people who already run emrac, however small it is. The pieces already exist: the release index ([`release-index.md`](./release-index.md)) and `emrac self upgrade` ([`self-upgrade.md`](./self-upgrade.md)). This page ties them together and describes `dev/release.sh`, which does the repetitive part.

## What the person running emrac does

```sh
emrac self sync          # refresh the index (self upgrade also does it when it is over a day old)
emrac self news          # what changed, security and breaking items first
emrac self upgrade       # builds the release with makepkg and installs it with pacman -U
```

`emrac self upgrade --dry-run` shows the plan without doing anything. `emrac self downgrade` goes back.

## What makes a release an update

- **The version number is what counts.** A release is newer only when its `x.y.z` is higher than the installed one. A new tag with the same version (`r2` and `r2-aur`) or a changed `pkgrel` is not an update. So every release, however small, needs a version bump.
- The tarball of the tag must unpack to a folder named `emrac-<tag>`, and its `Cargo.toml` version must equal the version in the index; `self upgrade` checks both.
- `state_schema` (`STATE_SCHEMA` in `self_meta.rs`) must not go below what is on disk, or `self downgrade` refuses the older release. Raise it only when the saved files change shape.
- Tag names: letters, digits, `.`, `_`, `-`; at most 40 characters; not starting with `-` or `.`. Suggested for small updates: `r3-u1`, `r3-u2` (the update slice after `r3`), or `r3.1`.
- Channels: `stable` (everyone) or `prerelease` (only those who set `self.channel = "prerelease"`). Publish a risky change as `prerelease` first, then add the same tag as `stable` later, or release a higher version.

## What the maintainer does: the release pipeline

One command starts it; GitHub does the rest (`.github/workflows/release.yml`).

```sh
dev/release.sh status                                              # version, last tag, what is next
dev/release.sh prepare r3-u1 0.3.1 "one-line summary" --push       # check, bump, notes, commit, tag, push
```

`prepare` refuses unless the tree is clean, you are on `main`, the version is well formed and higher than the current one, and the tag is well formed and new. Unless `--skip-checks` is given it first runs `cargo fmt --check`, clippy with warnings denied, the tests and `dev/gen-tests-doc.sh --check`. It bumps the version in `Cargo.toml` and in the two `emrac` entries of `Cargo.lock`, writes `docs/releases/<tag>.md` (from the commits since the last tag, or from `--notes FILE`; it contains the line `Channel: stable` or `prerelease`, set with `--channel`), adds a changelog entry and a line in `docs/releases/README.md`, commits, and creates the annotated tag from the notes. Without `--push` it stops there and nothing leaves your machine; `--dry-run` says what it would do. `--push` pushes the commit and the tag, which starts the pipeline.

The pipeline, on a pushed tag `r*`:

1. **check** (Arch container): the tag is annotated and reachable from `main`; its notes exist; its version is higher than every version in `releases.json`; then format, clippy, tests and the test list. A failure stops everything; nothing is published.
2. **publish**: `dev/release.sh publish <tag> --gh`: the tag's tarball is downloaded and its checksum taken, `releases.json` gets the release and a news item, `install.sh` is re-pinned (stable only), the GitHub release is created from the notes (marked prerelease on that channel), and the result is committed to `main` by `github-actions[bot]`.

Not done by the pipeline: the AUR recipe. `publish` prints the `pkgver`, `pkgrel`, `_tag` and `sha256sums` (see the job log) for you to put into the AUR package. If the pipeline cannot run, push yourself (`git push && git push origin <tag>`) and run `dev/release.sh publish <tag>` by hand; it does the same steps, minus the commit by the bot.

If `check` fails: fix, delete the tag (`git push origin :<tag>` and `git tag -d <tag>`), and `prepare` again with a new commit; the version bump from the failed attempt is then already in `Cargo.toml`, so use that same version.

## Checking it from outside

After the second push, from a clean home:

```sh
HOME=$(mktemp -d) emrac self sync && HOME=$HOME emrac self upgrade --dry-run
```

It should name the new tag, its version and the checksum it will check.

## Limits

- The pipeline needs the repository's Actions to be allowed to write to `main` (no branch protection that blocks the bot), and the AUR recipe is still updated by hand.
- The index is not signed yet: `releases.json.asc` is verified only when `self.require_signature` and `self.signing_key` are set, and no key is published.
- A copy not installed by the `emrac` pacman package is not upgraded by emrac.
- A build needs `makepkg`, a Rust toolchain, a C compiler, `fakeroot`, `tar` and about 2 GiB free; there is no prebuilt binary.

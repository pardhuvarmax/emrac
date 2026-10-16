#!/usr/bin/env bash
# Prints what a person should look at before a release goes out, as markdown:
# versions, channel, commits and files since the previous tag, and the changes
# that deserve a second look (the saved-state schema, the install recipe,
# dependencies, the workflows, the packaging recipe).
#
#   dev/release-review.sh <tag>
#
# The release pipeline shows it in the run's summary before the approval step;
# run it by hand to see the same thing before pushing a tag.
set -euo pipefail

tag=${1:?usage: dev/release-review.sh <tag>}
cd "$(dirname "$0")/.."
git rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null || { echo "no such tag: $tag" >&2; exit 1; }

prev=$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)
range=${prev:+$prev..}$tag

version_at() { git show "$1:Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' | head -n 1; }
new=$(version_at "$tag")
old=${prev:+$(version_at "$prev")}
channel=$(git show "$tag:docs/releases/$tag.md" 2>/dev/null | sed -n 's/^Channel: *//p' | head -n 1)

echo "# Review of $tag"
echo
echo "- version: ${old:-none} → **$new**"
echo "- channel: **${channel:-stable}**"
echo "- previous tag: ${prev:-none}"
echo "- commits: $(git rev-list --count "$range"), files changed: $(git diff --name-only "$range" | wc -l)"
echo

echo "## Look twice at"
flags=0
flag() { echo "- $1"; flags=1; }
schema() { git show "$1:crates/emrac-core/src/self_meta.rs" 2>/dev/null | sed -n 's/.*STATE_SCHEMA: [a-z0-9]* = \([0-9]*\);.*/\1/p' | head -n 1; }
if [ -n "$prev" ]; then
    [ "$(schema "$prev")" = "$(schema "$tag")" ] || flag "the saved-state schema changed (${s_old:=$(schema "$prev")}${s_old:-none} → $(schema "$tag")): older releases will refuse to be downgraded to over newer files"
    git diff --quiet "$range" -- install.sh || flag "\`install.sh\` changed"
    git diff --quiet "$range" -- crates/emrac-core/data/PKGBUILD.in || flag "the embedded build recipe changed (what \`self upgrade\` builds with)"
    git diff --quiet "$range" -- .github || flag "the workflows changed"
    git diff --quiet "$range" -- LICENSE || flag "the license changed"
    added=$(git diff "$range" -- Cargo.lock | sed -n 's/^+name = "\(.*\)"/\1/p' | tr '\n' ' ')
    [ -z "${added// /}" ] || flag "new crates in Cargo.lock: $added"
    removed=$(git diff "$range" -- Cargo.lock | sed -n 's/^-name = "\(.*\)"/\1/p' | tr '\n' ' ')
    [ -z "${removed// /}" ] || flag "crates removed from Cargo.lock: $removed"
    unsafe_added=$(git diff "$range" -- 'crates/**/*.rs' | grep -c '^+.*\bunsafe\b' || true)
    [ "$unsafe_added" = 0 ] || flag "$unsafe_added added line(s) with \`unsafe\`"
fi
git cat-file -e "$tag:docs/releases/$tag.md" 2>/dev/null || flag "no release notes in \`docs/releases/$tag.md\`"
[ "$flags" = 1 ] || echo "- nothing out of the ordinary"
echo

echo "## Commits"
git log --no-merges --format='- `%h` %s' "$range"
echo
echo "## Files"
echo '```'
git diff --stat=100 "$range" | tail -n 40
echo '```'
echo
echo "## Release notes"
echo
git show "$tag:docs/releases/$tag.md" 2>/dev/null || true

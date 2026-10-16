#!/usr/bin/env bash
# Releases emrac in two steps, so that a small update is not five manual jobs.
#
#   dev/release.sh prepare <tag> <version> ["one-line summary"] [options]
#       Checks the tree, bumps the version (Cargo.toml and Cargo.lock), writes
#       docs/releases/<tag>.md, adds a changelog entry, commits, and makes the
#       annotated tag locally. Pushes nothing.
#       options: --notes FILE   use this file as the release notes
#                --title TEXT   the notes' title (default: "emrac <tag>")
#                --channel C    stable (default) or prerelease; written into the notes
#                --push         then push the commit and the tag, which starts the
#                               release pipeline (.github/workflows/release.yml)
#                --yes          with --push: do not ask before pushing
#                --skip-checks  do not run fmt, clippy and the tests first
#                --dry-run      say what would happen and change nothing
#
#   dev/release.sh publish <tag> [stable|prerelease] [options]
#       The release pipeline runs this when a tag is pushed; run it by hand only
#       if the pipeline did not. The channel is read from the notes (a line
#       `Channel: prerelease`; stable otherwise). Checks that the tag is
#       public, pins install.sh to it (stable only), adds it to releases.json
#       with a news item, commits that, and prints the values for the AUR
#       PKGBUILD. Pushes nothing.
#       options: --summary TEXT  (default: the first paragraph of the notes)
#                --gh            also run `gh release create` (needs gh login)
#                --dry-run
#
#   dev/release.sh review <tag>
#       What a person should look at before the tag goes out (the same text the
#       pipeline shows before its approval step).
#
#   dev/release.sh status
#       The version, the last tags, and what the next step is.
#
# `emrac self upgrade` only moves to a release whose version number is higher,
# so every release, however small, needs a new version. See
# docs/design/releasing.md.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

repo_url=https://github.com/pardhuvarmax/emrac

die() { echo "release.sh: $*" >&2; exit 1; }
say() { echo "release.sh: $*"; }
usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; }

current_version() {
    sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml | head -n 1
}

# 3.14.15 -> 000030001400015, so that versions compare as strings.
version_key() {
    local a b c
    IFS=. read -r a b c <<<"$1"
    printf '%05d%05d%05d' "$a" "$b" "$c"
}

valid_tag() { [[ $1 =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,39}$ ]]; }
valid_version() { [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; }

clean_tree() { [ -z "$(git status --porcelain)" ]; }

previous_tag() { git describe --tags --abbrev=0 2>/dev/null || true; }

# ---------------------------------------------------------------- prepare

prepare() {
    local tag=${1:-} version=${2:-} summary="" notes="" title="" skip=0 dry=0 channel=stable push=0 yes=0
    [ -n "$tag" ] && [ -n "$version" ] || { usage; exit 2; }
    shift 2
    if [ $# -gt 0 ] && [[ $1 != --* ]]; then summary=$1; shift; fi
    while [ $# -gt 0 ]; do
        case $1 in
        --notes) notes=${2:?--notes needs a file}; shift 2 ;;
        --title) title=${2:?--title needs text}; shift 2 ;;
        --channel) channel=${2:?--channel needs stable or prerelease}; shift 2 ;;
        --push) push=1; shift ;;
        --yes) yes=1; shift ;;
        --skip-checks) skip=1; shift ;;
        --dry-run) dry=1; shift ;;
        *) die "unknown option: $1" ;;
        esac
    done
    case $channel in stable | prerelease) ;; *) die "the channel is stable or prerelease, not '$channel'" ;; esac

    valid_tag "$tag" || die "'$tag' is not a usable tag (letters, digits, . _ -, at most 40 characters)"
    valid_version "$version" || die "'$version' is not a version like 1.2.3"
    ! git rev-parse -q --verify "refs/tags/$tag" >/dev/null || die "the tag $tag already exists"
    [ "$(git rev-parse --abbrev-ref HEAD)" = main ] || die "not on main"
    clean_tree || die "the working tree is not clean: commit or stash first"
    [ -z "$notes" ] || [ -f "$notes" ] || die "no such notes file: $notes"

    local cur prev date
    cur=$(current_version)
    [ -n "$cur" ] || die "could not read the version from Cargo.toml"
    [[ $(version_key "$version") > $(version_key "$cur") ]] ||
        die "the version must be higher than the current $cur (emrac self upgrade only moves to a higher version)"
    prev=$(previous_tag)
    date=$(date +%F)
    summary=${summary:-"Update $version"}
    title=${title:-"emrac $tag"}

    if [ "$dry" = 1 ]; then
        say "dry run: would bump $cur -> $version, write docs/releases/$tag.md ($(git rev-list --count "${prev:+$prev..}HEAD") commits since ${prev:-the start}), add a changelog entry, commit and tag $tag"
        [ "$skip" = 1 ] || say "dry run: would first run fmt, clippy and the tests"
        return 0
    fi

    if [ "$skip" = 0 ]; then
        say "checking: fmt, clippy, tests, the test list"
        cargo fmt --check
        cargo clippy --all-targets -- -D warnings
        cargo test
        dev/gen-tests-doc.sh --check
    fi

    say "bumping $cur -> $version"
    python3 - "$cur" "$version" <<'PY'
import re, sys
cur, new = sys.argv[1:3]
toml = open("Cargo.toml").read()
toml, n = re.subn(r'(\[workspace\.package\][^\[]*?\nversion = ")' + re.escape(cur) + '"', r'\g<1>' + new + '"', toml, count=1)
if n != 1:
    sys.exit("could not find the workspace version in Cargo.toml")
open("Cargo.toml", "w").write(toml)
lock = open("Cargo.lock").read()
for name in ("emrac-cli", "emrac-core"):
    lock, n = re.subn(r'(name = "%s"\nversion = ")%s"' % (name, re.escape(cur)), r'\g<1>' + new + '"', lock, count=1)
    if n != 1:
        sys.exit("could not find %s in Cargo.lock" % name)
open("Cargo.lock", "w").write(lock)
PY
    cargo metadata --locked --offline --format-version 1 >/dev/null ||
        die "Cargo.lock does not match Cargo.toml after the bump"

    local file=docs/releases/$tag.md
    if [ -n "$notes" ]; then
        cp "$notes" "$file"
    else
        {
            echo "# $title"
            echo
            echo "Tag \`$tag\`, $date. Version \`$version\`. Built by pardhuvarmax — $repo_url"
            echo
            echo "$summary"
            echo
            echo "Channel: $channel"
            echo
            echo "## What changed${prev:+ since \`$prev\`}"
            echo
            git log --no-merges --format='- %s' "${prev:+$prev..}HEAD"
            echo
            echo "## Updating"
            echo
            echo "\`emrac self sync\` and then \`emrac self upgrade\` (or your AUR helper, or pacman with the new package)."
            echo "Requirements and known limits are those of ${prev:-the previous release}."
        } >"$file"
    fi

    python3 - "$tag" "$version" "$date" "$summary" <<'PY'
import sys
tag, version, date, summary = sys.argv[1:5]
text = open("CHANGELOG.md").read()
i = text.index("\n## ") + 1
entry = (f"## Release {tag} — {date} — {summary}\n\n"
         f"Version `{version}`; tag `{tag}`. The notes are `docs/releases/{tag}.md` (also the text of the annotated tag). "
         f"Published with `dev/release.sh`; `emrac self upgrade` finds it once `releases.json` lists it.\n\n")
open("CHANGELOG.md", "w").write(text[:i] + entry + text[i:])
readme = open("docs/releases/README.md").read()
bullet = f"- [`{tag}.md`](./{tag}.md) — {summary} (tag `{tag}`, {date}, version {version}).\n"
j = readme.index("\n- [`") + 1
open("docs/releases/README.md", "w").write(readme[:j] + bullet + readme[j:])
PY

    git add -A
    git commit -q -m "emrac $tag: $summary (version $version)"
    git tag -a "$tag" -F "$file" --cleanup=verbatim
    say "committed $(git rev-parse --short HEAD) and tagged $tag (nothing is pushed)"
    echo
    if [ "$push" = 1 ]; then
        dev/release-review.sh "$tag"
        if [ "$yes" = 0 ]; then
            read -r -p "Push $tag now (this starts the release pipeline)? [y/N] " ans
            case $ans in y | Y | yes) ;; *) say "not pushed; push when ready: git push && git push origin $tag"; return 0 ;; esac
        fi
        git push
        git push origin "$tag"
        say "pushed; the release pipeline now publishes $tag (https://github.com/pardhuvarmax/emrac/actions)"
    else
        echo "Next: git push && git push origin $tag"
        echo "      the release pipeline does the rest (or, by hand: dev/release.sh publish $tag)"
    fi
}

# ---------------------------------------------------------------- publish

publish() {
    local tag=${1:-} channel="" summary="" gh=0 dry=0
    [ -n "$tag" ] || { usage; exit 2; }
    shift
    if [ $# -gt 0 ] && [[ $1 != --* ]]; then channel=$1; shift; fi
    if [ -z "$channel" ]; then
        channel=$(git show "$tag:docs/releases/$tag.md" 2>/dev/null | sed -n 's/^Channel: *//p' | head -n 1)
        channel=${channel:-stable}
    fi
    while [ $# -gt 0 ]; do
        case $1 in
        --summary) summary=${2:?--summary needs text}; shift 2 ;;
        --gh) gh=1; shift ;;
        --dry-run) dry=1; shift ;;
        *) die "unknown option: $1" ;;
        esac
    done
    case $channel in stable | prerelease) ;; *) die "the channel is stable or prerelease, not '$channel'" ;; esac
    git rev-parse -q --verify "refs/tags/$tag" >/dev/null || die "there is no local tag $tag: run prepare first"
    clean_tree || die "the working tree is not clean: commit or stash first"

    local code
    code=$(curl -sIL -o /dev/null -w '%{http_code}' --proto '=https' --proto-redir '=https' \
        "$repo_url/archive/refs/tags/$tag.tar.gz" || true)
    [ "$code" = 200 ] || die "the tag $tag is not public yet (HTTP $code): git push && git push origin $tag"

    local version date notes=docs/releases/$tag.md
    version=$(git show "$tag:Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' | head -n 1)
    date=$(git log -1 --format=%cs "$tag")
    if [ -z "$summary" ]; then
        summary=$(git tag -l --format='%(contents)' "$tag" | sed -n '4,$p' | sed -n '/./{p;q}')
        summary=${summary:-"Update $version"}
    fi
    local title
    title=$(git tag -l --format='%(contents)' "$tag" | sed -n '1s/^# //p')
    title=${title:-"emrac $tag"}

    if [ "$dry" = 1 ]; then
        say "dry run: would add $tag ($version, $date, $channel) to releases.json with the news item '$title'"
        [ "$channel" != stable ] || say "dry run: would pin install.sh to $tag"
        return 0
    fi

    if [ "$channel" = stable ]; then
        say "pinning install.sh to $tag"
        dev/pin-install-script.sh "$tag"
    else
        say "a prerelease: install.sh stays where it is"
    fi
    say "adding $tag to releases.json"
    dev/release-index.sh "$tag" "$channel" "$summary"
    dev/release-index.sh --news "$date" release "$title" "$summary" "$version"

    if clean_tree; then
        say "install.sh and releases.json already say this; nothing to commit"
    else
        git add -A
        git commit -q -m "$tag is public: install.sh and releases.json${channel:+ ($channel)}"
        say "committed $(git rev-parse --short HEAD) (push it: git push)"
    fi

    local sha
    sha=$(python3 -c "
import json
d = json.load(open('releases.json'))
print(next(r['sha256'] for r in d['releases'] if r['tag'] == '$tag'))")
    echo
    echo "For the AUR PKGBUILD:"
    echo "  pkgver=$version"
    echo "  pkgrel=1"
    echo "  _tag=$tag"
    echo "  sha256sums=('$sha')"

    if [ "$gh" = 1 ]; then
        say "creating the GitHub release"
        local flags=()
        [ "$channel" = prerelease ] && flags+=(--prerelease)
        gh release create "$tag" --verify-tag --title "$title" --notes-file "$notes" "${flags[@]}"
    else
        echo
        echo "GitHub release (when you want it):"
        echo "  gh release create $tag --verify-tag --title \"$title\" --notes-file $notes"
    fi
}

# ----------------------------------------------------------------- status

status() {
    local cur prev
    cur=$(current_version)
    prev=$(previous_tag)
    echo "version in Cargo.toml : $cur"
    echo "latest tag            : ${prev:-none}"
    echo "commits since it      : $(git rev-list --count "${prev:+$prev..}HEAD")"
    echo "tree                  : $(clean_tree && echo clean || echo 'not clean')"
    echo "local, not pushed     : $(git rev-list --count '@{u}..HEAD' 2>/dev/null || echo '?') commit(s)"
    if [ -n "$prev" ]; then
        if [ "$(git rev-list --count "$prev..HEAD")" = 0 ]; then
            echo "next step             : nothing to release"
        else
            echo "next step             : dev/release.sh prepare <tag> <version above $cur> \"summary\""
        fi
    fi
}

case ${1:-} in
prepare) shift; prepare "$@" ;;
publish) shift; publish "$@" ;;
review) shift; dev/release-review.sh "$@" ;;
status) status ;;
-h | --help | help | "") usage ;;
*) die "unknown command '$1' (prepare, publish, review or status)" ;;
esac

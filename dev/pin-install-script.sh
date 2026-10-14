#!/bin/sh
# Points install.sh at a release tag: downloads the tag's tarball from GitHub,
# computes its SHA-256, reads the version from its Cargo.toml and rewrites the
# three pinned values (TAG, VERSION, SHA256) at the top of install.sh.
#
#   dev/pin-install-script.sh r2-aur
#
# Run it after pushing a new tag, check the diff, and commit install.sh.
set -eu

tag=${1:?usage: dev/pin-install-script.sh <tag>}
root=$(cd "$(dirname "$0")/.." && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM HUP

curl -fsSL --proto '=https' --proto-redir '=https' -o "$work/src.tar.gz" \
    "https://github.com/pardhuvarmax/emrac/archive/refs/tags/$tag.tar.gz"

top=$(tar tzf "$work/src.tar.gz" | head -n 1 | cut -d/ -f1)
[ "$top" = "emrac-$tag" ] || {
    echo "the tarball unpacks to '$top', but install.sh's recipe expects 'emrac-$tag'" >&2
    exit 1
}
sum=$(sha256sum "$work/src.tar.gz" | cut -d ' ' -f 1)
version=$(tar xzf "$work/src.tar.gz" --to-stdout "$top/Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -n 1)
[ -n "$version" ] || {
    echo "could not read the version from the tarball's Cargo.toml" >&2
    exit 1
}

sed -i \
    -e "s|^TAG=.*|TAG=\"$tag\"|" \
    -e "s|^VERSION=.*|VERSION=\"$version\"|" \
    -e "s|^SHA256=.*|SHA256=\"$sum\"|" \
    "$root/install.sh"
echo "install.sh now installs $version from $tag"
echo "  sha256 $sum"

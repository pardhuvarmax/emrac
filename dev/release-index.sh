#!/bin/sh
# Adds (or refreshes) one release in releases.json, the index `emrac self sync`
# reads: downloads the tag's tarball from GitHub, computes its SHA-256, reads
# the version and the state schema from inside it, and takes the date from the
# tag's commit.
#
#   dev/release-index.sh r3 [stable|prerelease] ["one-line summary"]
#   dev/release-index.sh --news 2026-11-01 release "Emrac r3" "What is new" [version]
#
# Run it after pushing the tag; check the diff; commit releases.json (the
# project page serves it as https://pardhuvarmax.github.io/emrac/releases.json).
# To sign the index (emrac can require this: `emrac config set
# self.require_signature true`):
#   gpg --armor --detach-sign --local-user <fingerprint> releases.json
# which writes releases.json.asc next to it.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
index=$root/releases.json

if [ "${1:-}" = "--news" ]; then
    date=${2:?usage: --news <date> <category> <title> <summary> [version]}
    category=${3:?category}
    title=${4:?title}
    summary=${5:?summary}
    version=${6:-}
    python3 - "$index" "$date" "$category" "$title" "$summary" "$version" <<'PY'
import json, sys
path, date, category, title, summary, version = sys.argv[1:7]
try:
    index = json.load(open(path))
except FileNotFoundError:
    index = {"schema": 1, "releases": [], "news": []}
item = {"date": date, "category": category, "title": title, "summary": summary}
if version:
    item["version"] = version
index.setdefault("news", [])
index["news"] = [n for n in index["news"] if (n["date"], n["title"]) != (date, title)] + [item]
index["news"].sort(key=lambda n: (n["date"], n["title"]))
json.dump(index, open(path, "w"), indent=2, ensure_ascii=False)
open(path, "a").write("\n")
PY
    exit 0
fi

tag=${1:?usage: dev/release-index.sh <tag> [channel] [summary]}
channel=${2:-stable}
summary=${3:-}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM HUP

url=https://github.com/pardhuvarmax/emrac/archive/refs/tags/$tag.tar.gz
curl -fsSL --proto '=https' --proto-redir '=https' -o "$work/src.tar.gz" "$url"
sum=$(sha256sum "$work/src.tar.gz" | cut -d ' ' -f 1)
top=$(tar tzf "$work/src.tar.gz" | head -n 1 | cut -d/ -f1)
version=$(tar xzf "$work/src.tar.gz" --to-stdout "$top/Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -n 1)
schema=$(tar xzf "$work/src.tar.gz" --to-stdout "$top/crates/emrac-core/src/self_meta.rs" 2>/dev/null | sed -n 's/^pub const STATE_SCHEMA: u32 = \([0-9]*\);/\1/p' | head -n 1 || true)
date=$(git -C "$root" log -1 --format=%cs "$tag" 2>/dev/null || date +%F)
[ -n "$version" ] || { echo "could not read the version from the tarball" >&2; exit 1; }

python3 - "$index" "$tag" "$version" "$date" "$channel" "${schema:-1}" "$summary" "$url" "$sum" <<'PY'
import json, sys
path, tag, version, date, channel, schema, summary, url, sha = sys.argv[1:10]
try:
    index = json.load(open(path))
except FileNotFoundError:
    index = {"schema": 1, "releases": [], "news": []}
entry = {
    "tag": tag, "version": version, "date": date, "channel": channel,
    "state_schema": int(schema), "summary": summary,
    "url": f"https://github.com/pardhuvarmax/emrac/releases/tag/{tag}",
    "tarball": url, "sha256": sha,
}
index["releases"] = [r for r in index["releases"] if r["tag"] != tag] + [entry]
index["releases"].sort(key=lambda r: (r["date"], r["tag"]))
index["generated"] = date
json.dump(index, open(path, "w"), indent=2, ensure_ascii=False)
open(path, "a").write("\n")
PY
echo "releases.json: $tag ($version, $date, $channel) sha256 $sum"

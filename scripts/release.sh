#!/bin/bash
# Prepare and publish a release from main.
#
# Sets the CalVer version (YY.M.D) in Cargo.toml and Cargo.lock, prepends the
# commits since the last tag to CHANGELOG.md, commits, tags the commit as
# vYY.M.D-HASH, and pushes both. The tag push starts .github/workflows/release.yml,
# which builds the archives and creates the GitHub release.
#
# Usage: scripts/release.sh [--dry-run]
#   --dry-run  print the changelog section and change nothing

set -euo pipefail
cd "$(dirname "$0")/.."

dry_run=false
if [[ ${1:-} == --dry-run ]]; then
    dry_run=true
elif [[ $# -gt 0 ]]; then
    echo "usage: $0 [--dry-run]" >&2
    exit 2
fi

version="$(date +%y).$(date +%-m).$(date +%-d)"
previous=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null || true)
range=${previous:+$previous..}HEAD

# One changelog group per gitmoji family. Commits with other gitmojis (docs,
# merges, tests, CI, tooling) stay out of the changelog.
features="" fixes="" changes="" dependencies=""
while IFS=' ' read -r hash emoji subject; do
    [[ -n $hash ]] || continue
    line="- $(printf %s "${subject:0:1}" | tr '[:lower:]' '[:upper:]')${subject:1} ($hash)"$'\n'
    case $emoji in
        ✨* | 🎉*) features+=$line ;;
        🐛* | 🚑* | 🩹* | 🔒*) fixes+=$line ;;
        💄* | ⚡* | ♻* | 🎨* | ⏪* | 🚸* | 💬* | 🌐* | ♿* | 🔥* | 🗑*) changes+=$line ;;
        ➕* | ➖* | ⬆* | ⬇* | 📌*) dependencies+=$line ;;
    esac
done < <(git log --no-merges --format='%h %s' "$range")

section="## $version, $(LC_ALL=C date '+%-d %B %Y')"$'\n'
for group in "Features:$features" "Fixes:$fixes" "Changes:$changes" "Dependencies:$dependencies"; do
    [[ -n ${group#*:} ]] && section+=$'\n'"### ${group%%:*}"$'\n\n'"${group#*:}"
done

if [[ $section != *"###"* ]]; then
    echo "Error: no changelog-worthy commits since ${previous:-the first commit}." >&2
    exit 1
fi

if $dry_run; then
    printf '%s' "$section"
    exit 0
fi

if [[ $(git rev-parse --abbrev-ref HEAD) != main ]]; then
    echo "Error: not on main. Check out main before releasing." >&2
    exit 1
fi
if [[ -n $(git status --porcelain) ]]; then
    echo "Error: the working tree has changes. Commit or stash them first." >&2
    exit 1
fi

git fetch origin main
local_commit=$(git rev-parse HEAD)
remote_commit=$(git rev-parse origin/main)
base_commit=$(git merge-base HEAD origin/main)
if [[ $local_commit != "$remote_commit" ]]; then
    if [[ $local_commit == "$base_commit" ]]; then
        echo "Error: local main is behind origin/main. Pull first." >&2
        exit 1
    elif [[ $remote_commit != "$base_commit" ]]; then
        echo "Warning: local main has diverged from origin/main."
        read -p "Continue anyway? (y/n) " -n 1 -r
        echo
        [[ $REPLY =~ ^[Yy]$ ]] || exit 1
    else
        echo "Local main is ahead of origin/main."
    fi
fi

sed -i '' "1,/^version = /s/^version = \"[^\"]*\"/version = \"$version\"/" Cargo.toml
cargo update --workspace --offline --quiet

title="# Changelog"
body=$(sed '1{/^# Changelog$/d;}' CHANGELOG.md 2>/dev/null || true)
printf '%s\n\n%s%s\n' "$title" "$section" "${body:+$'\n'$body}" > CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "🔖 release $version"

tag="v$version-$(git rev-parse --short=7 HEAD)"
git tag -a "$tag" -m "Druck $tag"
git push origin main
git push origin "$tag"
echo "Released $tag. The release workflow builds the archives and publishes the GitHub release."

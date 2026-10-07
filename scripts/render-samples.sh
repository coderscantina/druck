#!/bin/sh
# Render every Markdown file in samples/ to OUTDIR with the given kyber binary.
# A sample that has a theme of the same name in samples/themes/ is rendered
# a second time with it, as NAME-theme.pdf.
#
# The binary runs from an empty temporary directory, so nothing may depend on
# the working directory. Renders use only the bundled fonts and hyphenation
# data. Picks up whatever samples exist, so new samples need no change here.
#
# Usage: scripts/render-samples.sh KYBER OUTDIR
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 KYBER OUTDIR" >&2
  exit 2
fi

repo=$(cd "$(dirname "$0")/.." && pwd)
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$2"
out=$(cd "$2" && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

for md in "$repo"/samples/*.md; do
  name=$(basename "$md" .md)
  echo "render $name"
  "$bin" render "$md" -o "$out/$name.pdf"
  theme="$repo/samples/themes/$name.json"
  if [ -f "$theme" ]; then
    echo "render $name with its theme"
    "$bin" render "$md" --theme "$theme" -o "$out/$name-theme.pdf"
  fi
done

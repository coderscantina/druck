#!/bin/sh
# Check rendered sample PDFs from several platforms.
#
# DIR holds one subdirectory per platform, each with the PDFs written by
# scripts/render-samples.sh. The check fails when
#   - a PDF differs byte for byte from the first platform's PDF, or exists on
#     only some platforms, or
#   - the first platform's PDFs contain a font that is not embedded.
#
# For a byte difference it prints the first differing text lines
# (pdftotext -layout) and the page counts to show whether the layout changed.
# Needs pdffonts and pdftotext (poppler). If byte comparison ever proves too
# strict, compare the pdftotext -layout output and page count instead.
#
# Usage: scripts/check-pdfs.sh DIR
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 DIR" >&2
  exit 2
fi

dir=$1
status=0
ref=""

for platform in "$dir"/*/; do
  platform=${platform%/}
  if [ -z "$ref" ]; then
    ref=$platform
    continue
  fi
  for pdf in "$ref"/*.pdf "$platform"/*.pdf; do
    name=$(basename "$pdf")
    if [ ! -f "$ref/$name" ] || [ ! -f "$platform/$name" ]; then
      echo "MISSING $name on $(basename "$ref") or $(basename "$platform")"
      status=1
    elif ! cmp -s "$ref/$name" "$platform/$name"; then
      echo "DIFFERENT $name: $(basename "$ref") vs $(basename "$platform")"
      pdfinfo "$ref/$name" | grep '^Pages'
      pdfinfo "$platform/$name" | grep '^Pages'
      pdftotext -layout "$ref/$name" - >"$ref/$name.txt"
      pdftotext -layout "$platform/$name" - >"$platform/$name.txt"
      diff "$ref/$name.txt" "$platform/$name.txt" | head -20 || true
      status=1
    fi
  done
done

for pdf in "$ref"/*.pdf; do
  # pdffonts columns: name type encoding emb sub uni object generation.
  # The type can contain spaces, so count the embedded flag from the end.
  if ! pdffonts "$pdf" | tail -n +3 | awk '$(NF-4) != "yes" { print "NOT EMBEDDED: " $0; bad = 1 } END { exit bad }'; then
    echo "in $pdf"
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "all PDFs match across platforms and embed their fonts"
fi
exit "$status"

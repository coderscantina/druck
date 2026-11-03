#!/bin/sh
# Check that rendering needs no network, no installed fonts, and no user config.
#
# Renders every Markdown sample twice from an empty directory with an empty home:
# once normally and once with network access denied (sandbox-exec on macOS,
# unshare -rn on Linux). The PDFs must be identical. Druck has no network code,
# so this guards against a future dependency adding some.
# Windows has no equivalent here: check by hand with the network adapter off.
#
# Usage: scripts/check-offline.sh DRUCK
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 DRUCK" >&2
  exit 2
fi

repo=$(cd "$(dirname "$0")/.." && pwd)
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/cwd" "$work/home" "$work/online" "$work/offline"
cd "$work/cwd"

case $(uname -s) in
  Darwin)
    echo '(version 1)(allow default)(deny network*)' >"$work/deny.sb"
    denied() { sandbox-exec -f "$work/deny.sb" "$@"; }
    ;;
  Linux)
    denied() { unshare -rn "$@"; }
    ;;
  *)
    echo "no network sandbox for $(uname -s)" >&2
    exit 2
    ;;
esac

status=0
for md in "$repo"/samples/*.md; do
  name=$(basename "$md" .md)
  HOME="$work/home" "$bin" render "$md" -o "$work/online/$name.pdf" >/dev/null 2>&1
  if ! HOME="$work/home" denied "$bin" render "$md" -o "$work/offline/$name.pdf" >/dev/null 2>"$work/stderr"; then
    echo "FAILED offline: $name"
    cat "$work/stderr"
    status=1
  elif ! cmp -s "$work/online/$name.pdf" "$work/offline/$name.pdf"; then
    echo "DIFFERENT offline: $name"
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "all samples render the same with the network denied"
fi
exit "$status"

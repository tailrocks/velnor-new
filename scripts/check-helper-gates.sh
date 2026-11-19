#!/usr/bin/env bash
# Usage: check-helper-gates.sh <base-ref>            diff <base-ref>...HEAD
#        check-helper-gates.sh --diff FILE --current 0.1.N
# Fails when an added or changed helper-version gate in the generator crates is not
# strictly greater than the velnor-ci version already released on the base.
set -euo pipefail

diff_file=""
current=""
base=""
while [ $# -gt 0 ]; do
  case "$1" in
    --diff) diff_file=$2; shift 2 ;;
    --current) current=$2; shift 2 ;;
    *) base=$1; shift ;;
  esac
done

if [ -z "$current" ]; then
  [ -n "$base" ] || { echo "usage: $0 <base-ref> | --diff FILE --current 0.1.N" >&2; exit 2; }
  current=$(git show "$base:crates/velnor-ci/Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)
fi
if [ -z "$diff_file" ]; then
  diff_file=$(mktemp)
  trap 'rm -f "$diff_file"' EXIT
  git diff "$base...HEAD" -- crates/velnor-gen crates/velnor-render crates/velnor-rust >"$diff_file"
fi

patch=${current##*.}
case "$patch" in '' | *[!0-9]*) echo "cannot read the velnor-ci version from '$current'" >&2; exit 2 ;; esac
series=${current%.*}
next="$series.$((patch + 1))"

awk -v series="$series" -v patch="$patch" -v next_version="$next" '
  function versions(line, out,    rest, n, v) {
    n = 0
    rest = line
    while (match(rest, /\(0, ?1, ?[0-9]+\)|0\.1\.[0-9]+/)) {
      v = substr(rest, RSTART, RLENGTH)
      gsub(/[() ]/, "", v)
      gsub(/,/, ".", v)
      out[++n] = v
      rest = substr(rest, RSTART + RLENGTH)
    }
    return n
  }
  /^diff --git / { file = $4; sub(/^b\//, "", file); counted = (file ~ /^crates\/velnor-(gen|render|rust)\/src\//); next }
  /^\+\+\+ |^--- / { next }
  counted && /^-/ { removed[file SUBSEP substr($0, 2)] = 1; next }
  counted && /^\+/ {
    text = substr($0, 2)
    if (tolower(text) !~ /helper|gate|minimum|at_least|pin/) next
    added[++count] = file SUBSEP text
  }
  END {
    bad = 0
    for (i = 1; i <= count; i++) {
      split(added[i], parts, SUBSEP)
      if (added[i] in removed) continue
      n = versions(parts[2], found)
      for (k = 1; k <= n; k++) {
        if ((added[i] SUBSEP found[k]) in seen) continue
        seen[added[i] SUBSEP found[k]] = 1
        split(found[k], v, ".")
        if (v[1] "." v[2] != series || v[3] + 0 > patch) continue
        printf "%s: gate %s, but tools-v%s is already released; use %s\n  %s\n", parts[1], found[k], found[k], next_version, parts[2]
        bad = 1
      }
    }
    exit bad
  }
' "$diff_file" || {
  echo "helper gates must be strictly greater than velnor-ci $current on the base (tools-v$current is already tagged)" >&2
  exit 1
}
echo "helper gates: OK (every added gate is above velnor-ci $current)"

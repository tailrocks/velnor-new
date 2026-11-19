#!/usr/bin/env bash
# Usage: check-helper-gates.sh <base-ref>            diff <base-ref>...HEAD
#        check-helper-gates.sh --diff FILE --current 0.1.N
# Fails when an added or changed helper-version gate in the generator crates is not
# strictly greater than the velnor-ci version already released on the base.
# A correction may carry `// helper-gate: first in tools-v0.1.N via <commit-ish>` on the
# gate line or the line directly above it; it passes only when the gate is N, the commit is
# in tools-v0.1.N, and the commit is not in tools-v0.1.(N-1).
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

verify_correction() {
  local file=$1 gate=$2 n=$3 commit=$4 tag="tools-v0.1.$3" prev
  prev="tools-v0.1.$(($3 - 1))"
  if [ "$gate" != "0.1.$n" ]; then
    echo "$file: marker names $tag but the gate is $gate; the marker must name the gate's release" >&2
    return 1
  fi
  if ! git rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null; then
    echo "$file: tag $tag is not in this checkout (fetch tags: git fetch --tags)" >&2
    return 1
  fi
  if ! git rev-parse -q --verify "$commit^{commit}" >/dev/null; then
    echo "$file: marker commit '$commit' is not a commit in this checkout" >&2
    return 1
  fi
  if ! git merge-base --is-ancestor "$commit" "refs/tags/$tag"; then
    echo "$file: $commit is not in $tag, so gate $gate is not shipped there" >&2
    return 1
  fi
  if ! git rev-parse -q --verify "refs/tags/$prev^{commit}" >/dev/null; then
    echo "$file: tag $prev is not in this checkout, cannot confirm $commit is new (fetch tags)" >&2
    return 1
  fi
  if git merge-base --is-ancestor "$commit" "refs/tags/$prev"; then
    echo "$file: $commit is already in $prev, so gate $gate is not its first release" >&2
    return 1
  fi
  echo "helper gate correction: OK ($gate first in $tag via $commit)"
}

report=$(awk -v series="$series" -v patch="$patch" -v next_version="$next" '
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
  function marker(line, out,    m) {
    if (!match(line, /helper-gate: first in tools-v0\.1\.[0-9]+ via [^ ]+/)) return 0
    m = substr(line, RSTART, RLENGTH)
    match(m, /tools-v0\.1\.[0-9]+/)
    out["n"] = substr(m, RSTART + 11, RLENGTH - 11)
    out["commit"] = substr(m, index(m, " via ") + 5)
    return 1
  }
  /^diff --git / { file = $4; sub(/^b\//, "", file); counted = (file ~ /^crates\/velnor-(gen|render|rust)\/src\//); prevnew = ""; next }
  /^@@/ { prevnew = ""; next }
  /^\+\+\+ |^--- / { next }
  /^-/ { if (counted) removed[file SUBSEP substr($0, 2)] = 1; next }
  /^[ +]/ {
    text = substr($0, 2)
    above = prevnew
    prevnew = text
    if (!counted || $0 !~ /^\+/) next
    stripped = text
    sub(/helper-gate: first in tools-v0\.1\.[0-9]+ via [^ ]+/, "", stripped)
    has = marker(text, mk) || marker(above, mk)
    markN_now = has ? mk["n"] : ""
    markC_now = has ? mk["commit"] : ""
    delete mk
    if (has && versions(stripped, probe) == 0) next
    if (tolower(stripped) !~ /helper|gate|minimum|at_least|pin/) next
    added[++count] = file SUBSEP stripped
    markN[count] = markN_now
    markC[count] = markC_now
  }
  END {
    for (i = 1; i <= count; i++) {
      split(added[i], parts, SUBSEP)
      if (added[i] in removed) continue
      n = versions(parts[2], found)
      for (k = 1; k <= n; k++) {
        if ((added[i] SUBSEP found[k]) in seen) continue
        seen[added[i] SUBSEP found[k]] = 1
        if (markN[i] != "") {
          printf "VERIFY\t%s\t%s\t%s\t%s\n", parts[1], found[k], markN[i], markC[i]
          continue
        }
        split(found[k], v, ".")
        if (v[1] "." v[2] != series || v[3] + 0 > patch) continue
        printf "BAD\t%s\t%s\t%s\n", parts[1], found[k], parts[2]
      }
    }
  }
' "$diff_file")

bad=0
while IFS=$'\t' read -r kind f1 f2 f3 f4; do
  case "$kind" in
    VERIFY)
      verify_correction "$f1" "$f2" "$f3" "$f4" || bad=1 ;;
    BAD)
      printf '%s: gate %s, but tools-v%s is already released; use %s\n  %s\n' "$f1" "$f2" "$f2" "$next" "$f3" >&2
      bad=1 ;;
  esac
done <<<"$report"

if [ "$bad" != 0 ]; then
  echo "helper gates must be strictly greater than velnor-ci $current on the base (tools-v$current is already tagged)" >&2
  exit 1
fi
echo "helper gates: OK (every added gate is above velnor-ci $current or carries a verified correction marker)"

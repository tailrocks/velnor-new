#!/usr/bin/env bash
set -euo pipefail

diff_file=""
tree_file=""
head_version=""
base_version=""
tag_version=""
base=""
while [ $# -gt 0 ]; do
  case "$1" in
    --diff) diff_file=$2; shift 2 ;;
    --tree) tree_file=$2; shift 2 ;;
    --current) head_version=$2; shift 2 ;;
    --base-version) base_version=$2; shift 2 ;;
    --tag-version) tag_version=$2; shift 2 ;;
    *) base=$1; shift ;;
  esac
done

manifest_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' | head -n1
}

vmax() {
  printf '%s\n' "$@" | { grep -v '^$' || true; } | sort -V | tail -n1
}

gate_dirs="crates/velnor-gen crates/velnor-render crates/velnor-rust"
if [ -z "$head_version" ]; then
  [ -n "$base" ] || { echo "usage: $0 <base-ref> | --diff FILE --current 0.1.N [--base-version V] [--tag-version V] [--tree FILE]" >&2; exit 2; }
  head_version=$(git show "HEAD:crates/velnor-ci/Cargo.toml" | manifest_version)
  base_version=$(git show "$base:crates/velnor-ci/Cargo.toml" | manifest_version)
  tag_version=$(git tag -l 'tools-v*' | sed 's/^tools-v//' | sort -V | tail -n1)
fi
floor=$(vmax "$base_version" "$tag_version")
current=$(vmax "$floor" "$head_version")
scan_tree=0
if [ -n "$base_version" ] && [ "$(vmax "$base_version" "$head_version")" = "$head_version" ] && [ "$base_version" != "$head_version" ]; then
  scan_tree=1
fi
if [ -z "$diff_file" ]; then
  diff_file=$(mktemp)
  trap 'rm -f "$diff_file" "$tree_file"' EXIT
  moved_from=()
  while IFS= read -r -d '' status; do
    IFS= read -r -d '' old
    case "$status" in
      R*)
        IFS= read -r -d '' dest
        for d in $gate_dirs; do
          case "$dest" in "$d"/*) moved_from+=("$old"); break ;; esac
        done ;;
    esac
  done < <(git diff -M -z --name-status "$base...HEAD")
  git diff -M "$base...HEAD" -- $gate_dirs ${moved_from[@]+"${moved_from[@]}"} >"$diff_file"
  tree_file=$(mktemp)
  if [ "$scan_tree" = 1 ]; then
    git ls-files -- $(printf '%s/src ' $gate_dirs) | while IFS= read -r f; do
      printf 'diff --git a/%s b/%s\n@@ -0,0 +1 @@\n' "$f" "$f"
      sed 's/^/+/' "$f"
    done >"$tree_file"
  fi
fi

patch=${current##*.}
case "$patch" in '' | *[!0-9]*) echo "cannot read the velnor-ci version from '$current'" >&2; exit 2 ;; esac
series=${current%.*}
next="$series.$((patch + 1))"

is_released() {
  local v=$1
  [ "${v%.*}" = "$series" ] && [ "${v##*.}" -le "$patch" ]
}

release_ref() {
  local n=$1
  if git rev-parse -q --verify "refs/tags/tools-v0.1.$n^{commit}" >/dev/null; then
    echo "refs/tags/tools-v0.1.$n"
  elif [ "$n" != "$patch" ] || git rev-parse -q --verify "refs/tags/rust-ci-v0.1.$n^{commit}" >/dev/null; then
    echo "refs/tags/rust-ci-v0.1.$n"
  else
    echo "refs/tags/tools-v0.1.$n"
  fi
}

verify_correction() {
  local file=$1 gate=$2 n=$3 commit=$4 tag prev ref prev_ref
  ref=$(release_ref "$n")
  prev_ref=$(release_ref "$((n - 1))")
  tag=${ref#refs/tags/}
  prev=${prev_ref#refs/tags/}
  if [ "$gate" != "0.1.$n" ]; then
    echo "$file: marker names $tag but the gate is $gate; the marker must name the gate's release" >&2
    return 1
  fi
  if ! git rev-parse -q --verify "$commit^{commit}" >/dev/null; then
    echo "$file: marker commit '$commit' is not a commit in this checkout" >&2
    return 1
  fi
  if git rev-parse -q --verify "$ref^{commit}" >/dev/null; then
    if ! git merge-base --is-ancestor "$commit" "$ref"; then
      echo "$file: $commit is not in $tag, so gate $gate is not shipped there" >&2
      return 1
    fi
  elif [ "$n" = "$patch" ]; then
    if ! git merge-base --is-ancestor "$commit" HEAD; then
      echo "$file: $commit is not in this tree, so gate $gate cannot ship with it" >&2
      return 1
    fi
  else
    echo "$file: tag $tag is not in this checkout (fetch tags: git fetch --tags)" >&2
    return 1
  fi
  if ! git rev-parse -q --verify "$prev_ref^{commit}" >/dev/null; then
    echo "$file: tag $prev is not in this checkout, cannot confirm $commit is new (fetch tags)" >&2
    return 1
  fi
  if git merge-base --is-ancestor "$commit" "$prev_ref"; then
    echo "$file: $commit is already in $prev, so gate $gate is not its first release" >&2
    return 1
  fi
  echo "helper gate correction: OK ($gate first in $tag via $commit)"
}

scan() {
  local tree=$1 file=$2
  awk -v series="$series" -v patch="$patch" -v tree="$tree" -v lo="${base_version##*.}" '
  function versions(line, out,    rest, n, v, off, before, after) {
    n = 0
    gsub(/\047([^\047\\]|\\.)\047/, "\047\047", line)
    gsub(/"([^"\\]|\\.)*"/, "\"\"", line)
    rest = line
    off = 0
    while (match(rest, /\([ \t]*[0-9][0-9_]*(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)?[ \t]*,[ \t]*[0-9][0-9_]*(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)?[ \t]*,[ \t]*[0-9][0-9_]*(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)?[ \t]*,?[ \t]*\)|0\.1\.[0-9]+/)) {
      v = substr(rest, RSTART, RLENGTH)
      before = substr(line, 1, off + RSTART - 1)
      sub(/[ \t]+$/, "", before)
      before = substr(before, length(before), 1)
      after = substr(rest, RSTART + RLENGTH)
      off += RSTART + RLENGTH - 1
      rest = after
      if (substr(v, 1, 1) == "(" && (before == "!" || after ~ /^[ \t]*(=>|=([^=]|$)|if[ \t]|\|)/ || after ~ /^[^()]*\)[ \t]*(=>|=([^=]|$)|if[ \t]|\|)/)) continue
      gsub(/[ \t_]/, "", v)
      gsub(/(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)/, "", v)
      sub(/,\)$/, ")", v)
      gsub(/[()]/, "", v)
      gsub(/,/, ".", v)
      if (v !~ /^0\.1\.[0-9]+$/) continue
      out[++n] = v
    }
    return n
  }
  function const_item(s) {
    if (s !~ /^[ \t]*(pub([ \t]*\([^)]*\))?[ \t]+)?(const|static)[ \t]+(mut[ \t]+)?[A-Za-z_]/) return 0
    if (s ~ /^[ \t]*(pub([ \t]*\([^)]*\))?[ \t]+)?const[ \t]+(fn|unsafe|async|extern)[ \t(]/) return 0
    if (s !~ /\([ \t]*u64[ \t]*,[ \t]*u64[ \t]*,[ \t]*u64[ \t]*\)/) return 0
    return versions(s, cprobe) > 0
  }
  function marker(line, out,    m) {
    if (!match(line, /(^[ \t]*(\/\/|\/\*|\*)|[;,{}][ \t]*\/\/).*helper-gate: first in (tools|rust-ci)-v0\.1\.[0-9]+ via [^ ]+/)) return 0
    m = substr(line, RSTART, RLENGTH)
    match(m, /helper-gate: first in (tools|rust-ci)-v0\.1\.[0-9]+ via [^ ]+/)
    m = substr(m, RSTART, RLENGTH)
    match(m, /0\.1\.[0-9]+/)
    out["n"] = substr(m, RSTART + 4, RLENGTH - 4)
    out["commit"] = substr(m, index(m, " via ") + 5)
    return 1
  }
  function strip(line,    s) {
    s = line
    sub(/helper-gate: first in (tools|rust-ci)-v0\.1\.[0-9]+ via [^ ]+/, "", s)
    return s
  }
  /^diff --git / { hdr = $0; cut = 0; while ((at = index(substr(hdr, cut + 1), " b/")) > 0) cut += at; file = substr(hdr, cut + 3); if (match(hdr, / "b\/[^"]*"$/)) file = substr(hdr, RSTART + 4, RLENGTH - 5); counted = (file ~ /^crates\/velnor-(gen|render|rust)\/src\//); prevnew = ""; prevadded = 0; prevmk = ""; next }
  /^@@/ { prevnew = ""; prevadded = 0; prevmk = ""; next }
  /^\+\+\+ |^--- / { next }
  /^-/ { if (counted) removed[file SUBSEP substr($0, 2)] = 1; next }
  /^[ +]/ {
    text = substr($0, 2)
    above = prevnew
    aboveadded = prevadded
    if (above ~ /^[ \t]*#!?\[/ && prevmk != "") above = prevmk
    prevmk = (text ~ /^[ \t]*#!?\[/ && marker(above, carry)) ? above : ""
    delete carry
    prevnew = text
    prevadded = ($0 ~ /^\+/)
    if (!prevadded) {
      if (!counted || !aboveadded || !marker(above, am) || versions(strip(above), av) > 0) next
      added[++count] = file SUBSEP strip(text)
      markN[count] = am["n"]
      markC[count] = am["commit"]
      ctx[count] = 1
      delete am
      next
    }
    if (!tree && tolower(text) ~ /^[ \t]*(\/\/|\/\*|\*).*(first released in|not tagged yet)/) { printf "PROSE\t%s\t%s\n", file, text; next }
    if (!tree && text ~ /^[ \t]*(\/\/|\/\*|\*).*helper-gate:|[;,{}][ \t]*\/\/.*helper-gate:/ && !marker(text, probe_mk)) { printf "MARKER\t%s\t%s\n", file, text; next }
    if (!counted) next
    stripped = strip(text)
    has = marker(text, mk) || marker(above, mk)
    markN_now = has ? mk["n"] : ""
    markC_now = has ? mk["commit"] : ""
    delete mk
    if (has && versions(stripped, probe) == 0) next
    if (tolower(stripped) !~ /helper|gate|minimum|at_least|pin/ && !const_item(stripped)) next
    added[++count] = file SUBSEP stripped
    markN[count] = markN_now
    markC[count] = markC_now
  }
  END {
    for (i = 1; i <= count; i++) {
      split(added[i], parts, SUBSEP)
      if (!ctx[i] && added[i] in removed) continue
      n = versions(parts[2], found)
      for (k = 1; k <= n; k++) {
        if ((added[i] SUBSEP found[k]) in seen) continue
        seen[added[i] SUBSEP found[k]] = 1
        if (tree) {
          split(found[k], tv, ".")
          if (tv[1] "." tv[2] != series || tv[3] + 0 <= lo + 0 || tv[3] + 0 > patch) continue
        }
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
' "$file"
}

report=$(scan 0 "$diff_file")
if [ -n "$tree_file" ] && [ "$scan_tree" = 1 ]; then
  report=$(printf '%s\n%s' "$report" "$(scan 1 "$tree_file")")
fi

bad=0
while IFS=$'\t' read -r kind f1 f2 f3 f4; do
  case "$kind" in
    VERIFY)
      if is_released "$f2"; then
        verify_correction "$f1" "$f2" "$f3" "$f4" || bad=1
      else
        printf '%s: gate %s is unreleased (tools-v%s is the latest tag) and must carry no marker\n' "$f1" "$f2" "$current" >&2
        bad=1
      fi ;;
    PROSE)
      printf '%s: prose gate marker %s\n  use `// helper-gate: first in tools-v0.1.N via <commit>` on a released gate; leave unreleased gates unmarked\n' "$f1" "$f2" >&2
      bad=1 ;;
    MARKER)
      printf '%s: helper-gate marker is not the released form: %s\n  use `// helper-gate: first in tools-v0.1.N via <commit>` on a released gate; leave unreleased gates unmarked\n' "$f1" "$f2" >&2
      bad=1 ;;
    BAD)
      printf '%s: gate %s, but tools-v%s is already released; use %s\n  %s\n' "$f1" "$f2" "$f2" "$next" "$f3" >&2
      bad=1 ;;
  esac
done <<<"$report"

if [ "$bad" != 0 ]; then
  echo "helper gates must be above velnor-ci $current (this tree), or at or below it with the released marker" >&2
  exit 1
fi
echo "helper gates: OK (every added gate is above velnor-ci $current or carries a verified correction marker)"

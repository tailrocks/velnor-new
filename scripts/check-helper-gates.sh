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
  git diff -U100000000 "$base...HEAD" -- $gate_dirs >"$diff_file"
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
  BEGIN {
    lx_mode = "code"; sq = sprintf("%c", 39); lx_depth = 0; lit_s = 0
    ws = "[[:space:]]*"
    num = "[0-9][0-9A-Za-z_]*"
    tup = "\\(" ws num ws "," ws num ws "," ws num ws ",?" ws "\\)"
    ver = "(" tup ")|0\\.1\\.[0-9]+"
    digits = "0123456789abcdef"
  }
  function intval(s,    base, i, d, v, n) {
    sub(/_*(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)$/, "", s)
    gsub(/_/, "", s)
    base = 10
    if (s ~ /^0x/) { base = 16; s = substr(s, 3) }
    else if (s ~ /^0o/) { base = 8; s = substr(s, 3) }
    else if (s ~ /^0b/) { base = 2; s = substr(s, 3) }
    n = length(s)
    if (n == 0) return -1
    v = 0
    for (i = 1; i <= n; i++) {
      d = index(digits, tolower(substr(s, i, 1))) - 1
      if (d < 0 || d >= base) return -1
      v = v * base + d
      if (v > 1000000000) v = 1000000000
    }
    return v
  }
  function tupver(t,    e, n, a, b, c) {
    gsub(/[()[:space:]]/, "", t)
    sub(/,$/, "", t)
    n = split(t, e, ",")
    if (n != 3) return ""
    a = intval(e[1]); b = intval(e[2]); c = intval(e[3])
    if (a != 0 || b != 1 || c < 0) return ""
    return "0.1." c
  }
  function versions(line, out,    rest, n, v) {
    n = 0
    rest = line
    while (match(rest, ver)) {
      v = substr(rest, RSTART, RLENGTH)
      rest = substr(rest, RSTART + RLENGTH)
      if (substr(v, 1, 1) == "(") {
        v = tupver(v)
        if (v == "") continue
      }
      out[++n] = v
    }
    return n
  }
  function isword(ch) { return ch ~ /^[A-Za-z0-9_]$/ }
  function lex_addr(a, b) { nr++; rs[nr] = a; re[nr] = b }
  function inrange(p,    k) {
    for (k = 1; k <= nr; k++) if (rs[k] <= p && p < re[k]) return 1
    return 0
  }
  function lit_addr(a, b) { nq++; qs[nq] = a; qe[nq] = b }
  function inlit(p,    k) {
    for (k = 1; k <= nq; k++) if (qs[k] <= p && p < qe[k]) return 1
    return 0
  }
  function tuple_code(line,    s, off, st) {
    off = 0; s = line
    while (match(s, tup)) {
      st = off + RSTART
      if (!inrange(st) && !inlit(st) && tupver(substr(s, RSTART, RLENGTH)) != "") return 1
      off = st + RLENGTH - 1
      s = substr(line, off + 1)
    }
    return 0
  }
  function mask_code(line,    out, i, n) {
    out = ""; n = length(line)
    for (i = 1; i <= n; i++) out = out ((inrange(i) || inlit(i)) ? " " : substr(line, i, 1))
    return out
  }
  function lex_line(s,    i, n, c, cs, pc, ppc, j, h, hs, k) {
    split("", rs)
    split("", re)
    split("", qs)
    split("", qe)
    nr = 0
    nq = 0
    n = length(s)
    cs = (lx_mode == "block") ? 1 : 0
    lit_s = (lx_mode == "str" || lx_mode == "raw") ? 1 : 0
    for (i = 1; i <= n; i++) {
      c = substr(s, i, 1)
      if (lx_mode == "block") {
        if (c == "/" && substr(s, i + 1, 1) == "*") { i++; lx_depth++; continue }
        if (c == "*" && substr(s, i + 1, 1) == "/") {
          i++
          if (--lx_depth == 0) { lx_mode = "code"; lex_addr(cs, i + 1); cs = 0 }
        }
        continue
      }
      if (lx_mode == "str") {
        if (c == "\\") i++
        else if (c == "\"") { lx_mode = "code"; lit_addr(lit_s, i + 1) }
        continue
      }
      if (lx_mode == "raw") {
        if (c == "\"" && substr(s, i + 1, lx_h) == lx_hs) { i += lx_h; lx_mode = "code"; lit_addr(lit_s, i + 1) }
        continue
      }
      if (c == "/" && substr(s, i + 1, 1) == "/") { lex_addr(i, n + 1); break }
      if (c == "/" && substr(s, i + 1, 1) == "*") { cs = i; lx_mode = "block"; lx_depth = 1; i++; continue }
      if (c == "\"") { lx_mode = "str"; lit_s = i; continue }
      if (c == sq) {
        if (substr(s, i + 1, 1) == "\\") { k = index(substr(s, i + 3), sq); if (k > 0) i += 2 + k }
        else if (substr(s, i + 2, 1) == sq) i += 2
        continue
      }
      if (c == "r") {
        pc = (i > 1) ? substr(s, i - 1, 1) : ""
        ppc = (i > 2) ? substr(s, i - 2, 1) : ""
        if (!isword(pc) || ((pc == "b" || pc == "c") && !isword(ppc))) {
          j = i + 1
          h = 0
          while (substr(s, j, 1) == "#") { h++; j++ }
          if (substr(s, j, 1) == "\"") {
            hs = ""
            for (k = 0; k < h; k++) hs = hs "#"
            lx_mode = "raw"; lx_h = h; lx_hs = hs; lit_s = i
            i = j
          }
        }
      }
    }
    if (lx_mode == "block" && cs) lex_addr(cs, n + 1)
    if ((lx_mode == "str" || lx_mode == "raw") && lit_s) lit_addr(lit_s, n + 1)
  }
  function analyze(line,    s, off, p, m, st) {
    c_mv = 0; c_mn = ""; c_mc = ""; c_mention = 0
    lex_line(line)
    c_tuple = tuple_code(line)
    off = 0; s = line
    while (!c_mv && match(s, /helper-gate: first in (tools|rust-ci)-v0\.1\.[0-9]+ via [^ ]+/)) {
      st = off + RSTART
      m = substr(s, RSTART, RLENGTH)
      off = st + RLENGTH - 1
      s = substr(line, off + 1)
      if (inrange(st)) {
        match(m, /0\.1\.[0-9]+/)
        c_mn = substr(m, RSTART + 4, RLENGTH - 4)
        c_mc = substr(m, index(m, " via ") + 5)
        c_mv = 1
      }
    }
    off = 0; s = line
    while ((p = index(s, "helper-gate:")) > 0) {
      if (inrange(off + p)) c_mention = 1
      off += p + 11
      s = substr(line, off + 1)
    }
  }
  function strip(line,    s) {
    s = line
    sub(/helper-gate: first in (tools|rust-ci)-v0\.1\.[0-9]+ via [^ ]+/, "", s)
    return s
  }
  /^diff --git / { file = $4; sub(/^b\//, "", file); counted = (file ~ /^crates\/velnor-(gen|render|rust)\/src\//); prevnew = ""; prevadded = 0; lx_mode = "code"; p_mv = 0; c_mv = 0; inhdr = 1; next }
  /^@@/ { inhdr = 0; prevnew = ""; prevadded = 0; p_mv = 0; c_mv = 0; next }
  inhdr && /^(\+\+\+|---) / { next }
  /^-/ { if (counted) removed[file SUBSEP substr($0, 2)] = 1; next }
  /^[ +]/ {
    text = substr($0, 2)
    above = prevnew
    aboveadded = prevadded
    p_mv = c_mv; p_mn = c_mn; p_mc = c_mc
    analyze(text)
    scanned_now = strip(mask_code(text))
    prevnew = text
    prevadded = ($0 ~ /^\+/)
    if (!prevadded) {
      if (!counted || !aboveadded || !p_mv || versions(strip(above), av) > 0) next
      added[++count] = file SUBSEP strip(text)
      scan_of[count] = scanned_now
      markN[count] = p_mn
      markC[count] = p_mc
      ctx[count] = 1
      next
    }
    if (!tree && tolower(text) ~ /^[ \t]*(\/\/|\/\*|\*).*(first released in|not tagged yet)/) { printf "PROSE\t%s\t%s\n", file, text; next }
    if (!tree && c_mention && !c_mv) { printf "MARKER\t%s\t%s\n", file, text; next }
    if (!counted) next
    stripped = strip(text)
    has = c_mv || p_mv
    markN_now = c_mv ? c_mn : (p_mv ? p_mn : "")
    markC_now = c_mv ? c_mc : (p_mv ? p_mc : "")
    if (has && versions(scanned_now, probe) == 0) next
    if (tolower(stripped) !~ /helper|gate|minimum|at_least|pin/ && !c_tuple) next
    added[++count] = file SUBSEP stripped
    scan_of[count] = scanned_now
    markN[count] = markN_now
    markC[count] = markC_now
  }
  END {
    for (i = 1; i <= count; i++) {
      split(added[i], parts, SUBSEP)
      if (!ctx[i] && added[i] in removed) continue
      n = versions(scan_of[i], found)
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

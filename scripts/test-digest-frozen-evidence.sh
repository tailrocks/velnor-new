#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
CHECKER="${1:-$ROOT/scripts/check-digest-frozen-evidence.py}"
LIST=scripts/digest-frozen-paths.txt
FIRST=docs/implemented/evidence/freshness-2026-10-20/first
work="$(mktemp -d "${TMPDIR:-/tmp}/digest-frozen.XXXXXX")" || exit 1
trap 'rm -rf "$work"' EXIT

cd "$ROOT" || exit 1

fixture() {
  local dir="$work/$1"
  mkdir -p "$dir"
  {
    cat "$LIST"
    printf '%s\n' .alint.yml "$LIST" \
      docs/implemented/evidence/freshness-2026-10-20/capture-inventory.json \
      "$FIRST/freeze.json" docs/implemented/evidence/freshness-2026-10-24/SHA256SUMS
  } | sort -u | tar -cf - -T - | tar -xf - -C "$dir"
  git -C "$dir" init -q
  git -C "$dir" add -A
  printf '%s\n' "$dir"
}

fails=0
expect() {
  local name="$1" want="$2" dir="$3" out rc
  out="$(python3 -I "$CHECKER" "$dir" "$dir/.alint.yml" 2>&1)"
  rc=$?
  if [ "$want" = ok ]; then
    if [ "$rc" -eq 0 ] && grep -q 'digest-frozen evidence check: OK' <<<"$out"; then echo "ok   $name"; return; fi
  elif [ "$rc" -ne 0 ] && grep -qF "$want" <<<"$out"; then
    echo "ok   $name (rejected)"; return
  fi
  echo "FAIL $name: wanted '$want', exit $rc"
  sed 's/^/     /' <<<"$out" | head -5
  fails=$((fails + 1))
}

victim="$FIRST/agreement.json"

d="$(fixture control)"
expect untouched-tree ok "$d"

d="$(fixture drop-entry)"
grep -vF "\"$victim\"" "$d/.alint.yml" > "$d/.alint.new" && mv "$d/.alint.new" "$d/.alint.yml"
printf ' ' >> "$d/$victim"
expect dropped-entry-and-rewritten "$victim: pinned in $LIST but no longer exempt" "$d"

d="$(fixture unlisted-exempt)"
grep -vxF "$victim" "$d/$LIST" > "$d/list.new" && mv "$d/list.new" "$d/$LIST"
expect exempt-not-listed "$victim: exempt but not pinned in $LIST" "$d"

d="$(fixture missing-manifest)"
rm "$d/$FIRST/freeze.json"
expect deleted-pinned-manifest "$FIRST/freeze.json: pinned manifest is missing" "$d"

d="$(fixture deleted-exempt)"
rm "$d/$victim"
expect deleted-exempt-file "$victim: exempt path does not exist" "$d"

d="$(fixture symlink)"
mv "$d/$victim" "$d/payload"
ln -s "$d/payload" "$d/$victim"
expect symlinked-exempt-file "$victim: not a regular file" "$d"

d="$(fixture chmod-x)"
chmod +x "$d/$victim"
expect executable-exempt-file "$victim: mode" "$d"

d="$(fixture index-mode)"
chmod +x "$d/$victim"
git -C "$d" add "$victim"
chmod -x "$d/$victim"
expect index-mode-100755 "$victim: git mode 100755" "$d"

if [ "$fails" -ne 0 ]; then
  echo "digest-frozen evidence fixtures: $fails failing"
  exit 1
fi
echo "digest-frozen evidence fixtures: OK"

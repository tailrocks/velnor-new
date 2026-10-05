#!/bin/bash
set -euo pipefail
out=/root/velnor-stop-preservation-20261005
: > "$out/clones.tsv"
while IFS= read -r g; do
  [ -d "$g" ] || continue
  p=${g%/.git}
  case "$p" in
    *velnor-mise-*|*velnor-cli-*|*velnor-goldens-work*|*/target/*|*/maintenance-assets/*|*/uv/*|*fixture*|*velnor-git-status-*|*velnor-git-writes-*|*/.tmp*) continue;;
    /tmp/velnor-*|/root/.cache/velnor-*|/root/.velnor-*|/root/velnor-consumer-scratch-20261005/*) ;;
    *) continue;;
  esac
  remote=$(git -C "$p" remote get-url origin 2>/dev/null || true)
  head=$(git -C "$p" rev-parse --verify HEAD 2>/dev/null || true)
  count=$(git -C "$p" status --porcelain 2>/dev/null | wc -l)
  printf '%s\t%s\t%s\t%s\n' "$p" "$head" "$count" "$remote" >> "$out/clones.tsv"
done < "$out/git-locations.before"
cat "$out/clones.tsv"

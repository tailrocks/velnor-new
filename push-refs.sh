#!/bin/bash
set -euo pipefail
cd /root/Projects/tailrocks/velnor-project/velnor-new
out=/root/velnor-stop-preservation-20261005
: > "$out/preserved-refs.tsv"
while read -r ref sha; do
  case "$ref" in refs/heads/*) remote="preservation/stop-20261005/heads/${ref#refs/heads/}";; *) continue;; esac
  printf '%s\t%s\t%s\n' "$sha" "$remote" "$ref" >> "$out/preserved-refs.tsv"
done < <(git for-each-ref --format='%(refname) %(objectname)' refs/heads)
while IFS= read -r p; do
  ref=$(git -C "$p" symbolic-ref -q HEAD || true)
  [ -z "$ref" ] || continue
  id=$(printf '%s' "$p" | sha256sum | cut -c1-12)
  printf '%s\t%s\t%s\n' "$(git -C "$p" rev-parse HEAD)" "preservation/stop-20261005/detached/$id" "$p" >> "$out/preserved-refs.tsv"
done < "$out/worktree-paths"
git stash list --format='%H' | awk '{printf "%s\tpreservation/stop-20261005/stash/%02d\tstash\n",$0,NR-1}' >> "$out/preserved-refs.tsv"
mapfile -t spec < <(awk -F '\t' '{print $1 ":refs/heads/" $2}' "$out/preserved-refs.tsv")
git push --atomic origin "${spec[@]}" > "$out/push-refs.log" 2>&1
git ls-remote --heads origin 'refs/heads/preservation/stop-20261005/*' > "$out/remote.verified"
while IFS=$'\t' read -r sha branch source; do
  grep -Fqx "$sha"$'\t'"refs/heads/$branch" "$out/remote.verified" || { printf 'FAILED %s\n' "$branch"; exit 1; }
done < "$out/preserved-refs.tsv"
printf 'Verified %s preservation refs\n' "$(wc -l < "$out/preserved-refs.tsv")"

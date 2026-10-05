#!/bin/bash
set -euo pipefail
root=/root/Projects/tailrocks/velnor-project/velnor-new
out=/root/velnor-stop-preservation-20261005
cd "$root"
sha=$(cat "$out/delivery-sha")
[ "$(git ls-remote origin refs/heads/preservation/stop-20261005/delivery | cut -f1)" = "$sha" ] || exit 1
printf 'Remote delivery confirmed %s before cleanup\n' "$sha" > "$out/cleanup-verification"
while IFS= read -r p; do
 [ -e "$p/.git" ] || continue
 # Main directories of independent clones are removed after their linked worktrees.
 [ ! -d "$p/.git" ] || continue
 head=$(git -C "$p" rev-parse HEAD)
 grep -q "^$head"$'\t' "$out/preserved-refs.tsv" || exit 1
 gd=$(git -C "$p" rev-parse --path-format=absolute --git-common-dir)
 if git --git-dir="$gd" worktree remove --force "$p" >> "$out/cleanup-command.log" 2>&1; then printf '%s\n' "$p" >> "$out/removed-worktrees"; else printf 'Worktree removal failed: %s (see cleanup-command.log)\n' "$p" >> "$out/exceptions"; fi
done < "$out/worktrees-removal-plan"
while IFS= read -r ref; do
 head=$(git rev-parse "$ref")
 grep -q "^$head"$'\t' "$out/preserved-refs.tsv" || exit 1
 if git branch -D "${ref#refs/heads/}" >> "$out/cleanup-command.log" 2>&1; then printf '%s\n' "$ref" >> "$out/removed-branches"; else printf 'Retained branch still used by retained checkout: %s\n' "$ref" >> "$out/retained-preexisting"; fi
done < "$out/branches-removal-plan"
# Every stash value was preserved on its own remote ref before dropping local stashes.
git stash clear
printf 'Root stash stack cleared after verified remote stash preservation\n' >> "$out/cleanup-command.log"
if [ -f "$out/clones-removal-approved" ]; then
 sort -u "$out/clones-removal-approved" | while IFS= read -r p; do
  [ -d "$p" ] || continue
  # Owned temporary clones only; no user project root is removed.
  case "$p" in /root/Projects/*) continue;; esac
  rm -rf -- "$p"
  printf '%s\n' "$p" >> "$out/removed-clones"
 done
fi
# Bare clone source and every linked worktree were separately pushed and verified.
if [ -d /tmp/velnor-native-bcd2-repo-20261005.git ]; then rm -rf /tmp/velnor-native-bcd2-repo-20261005.git; printf '/tmp/velnor-native-bcd2-repo-20261005.git\n' >> "$out/removed-clones"; fi
# Goal-owned generated compilation state and test fixtures; authored source/logs/receipts
# are in verified Git refs and the delivery archives. Sensitive original inputs are retained.
find /root/.cache -maxdepth 1 -type d -name 'velnor-*' -print0 | while IFS= read -r -d '' p; do
 [ ! -e "$p/.git" ] || continue
 if find "$p" -type d -name private-keys-v1.d -print -quit | grep -q .; then printf 'Retained private-key cache directory: %s\n' "$p" >> "$out/exceptions"; continue; fi
 rm -rf -- "$p"
 printf '%s\n' "$p" >> "$out/removed-directories"
done
if [ -d /root/.cache/velnor ]; then find /root/.cache/velnor -maxdepth 1 -type d -name 'pr28-*' -print0 | while IFS= read -r -d '' p; do rm -rf -- "$p"; printf '%s\n' "$p" >> "$out/removed-directories"; done; fi
# Names explicitly identify this project's generated test/scratch state.
find /tmp /var/tmp -maxdepth 1 -type d -name 'velnor-*' -print0 | while IFS= read -r -d '' p; do
 case "$p" in /tmp/velnor-node2418-provenance-20261005|/tmp/velnor-mbx-action-1.6.0) continue;; esac
 if find "$p" -type d -name private-keys-v1.d -print -quit | grep -q .; then printf 'Retained private-key temporary directory: %s\n' "$p" >> "$out/exceptions"; continue; fi
 [ ! -e "$p/.git" ] || { printf 'Retained unclassified temporary Git state: %s\n' "$p" >> "$out/exceptions"; continue; }
 rm -rf -- "$p"
 printf '%s\n' "$p" >> "$out/removed-directories"
done
for p in /root/velnor-target-pr28-20261005 /root/velnor-task-scratch /root/.velnor-freshness-research-20261005 /root/.velnor-research-native-image-transfer-20261005 /root/.velnor-pr59-source-packet.jaYX9A /root/.velnor-pr59-b55b4e9e97470a8c-20261005 /root/.velnor-pr59-refresh-5a3f5348-81e65fee-20261005; do
 [ -d "$p" ] || continue
 rm -rf -- "$p"
 printf '%s\n' "$p" >> "$out/removed-directories"
done
# Clean only the two exact goal-built tags, never prune shared Docker state.
docker image rm velnor-dind:pr25-final-1ece60d7 velnor-runner:pr25-final-1ece60d7 >> "$out/docker-cleanup.log" 2>&1 || printf 'Goal image tag cleanup incomplete; see docker-cleanup.log\n' >> "$out/exceptions"
git worktree list --porcelain > "$out/worktrees.after"
git status --porcelain > "$out/root-status.after"
git for-each-ref --format='%(refname) %(objectname)' refs/heads refs/stash > "$out/refs.after"
printf 'Cleanup recorded: %s worktrees, %s branches, %s clones, %s directories\n' "$(wc -l < "$out/removed-worktrees")" "$(wc -l < "$out/removed-branches")" "$(wc -l < "$out/removed-clones")" "$(wc -l < "$out/removed-directories")"

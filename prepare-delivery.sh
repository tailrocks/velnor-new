#!/bin/bash
set -euo pipefail
root=/root/Projects/tailrocks/velnor-project/velnor-new
out=/root/velnor-stop-preservation-20261005
cd "$root"
# Retain only one manifest record for each exact remote branch and SHA.
sort -u "$out/preserved-refs.tsv" -o "$out/preserved-refs.tsv"
git ls-remote --heads origin 'refs/heads/preservation/stop-20261005/*' > "$out/remote.final"
while IFS=$'\t' read -r sha branch source; do grep -Fqx "$sha"$'\t'"refs/heads/$branch" "$out/remote.final" || { printf 'Remote verification failed %s\n' "$branch"; exit 1; }; done < "$out/preserved-refs.tsv"
find "$out/archives" -type f -print0 | sort -z | xargs -0 sha256sum > "$out/archive-hashes.sha256"
: > "$out/worktrees-removal-plan"
while IFS= read -r p; do
 case "$p" in "$root"|/root/Projects/*) continue;; esac
 # Every HEAD must have an exact, independently read remote preservation ref.
 sha=$(git -C "$p" rev-parse HEAD)
 grep -q "^$sha"$'\t' "$out/preserved-refs.tsv" || exit 1
 printf '%s\n' "$p" >> "$out/worktrees-removal-plan"
done < "$out/worktree-paths"
cat "$out/additional-worktree-approved" | sort -u | while IFS= read -r p; do case "$p" in /root/Projects/*) continue;; esac; printf '%s\n' "$p"; done >> "$out/worktrees-removal-plan"
sort -u "$out/worktrees-removal-plan" -o "$out/worktrees-removal-plan"
: > "$out/branches-removal-plan"
while read -r ref sha; do
 [ "$ref" != refs/heads/main ] || continue
 first=$(git reflog show --format='%ct' "$ref" | tail -1)
 if [ -n "$first" ] && [ "$first" -ge 1791158703 ]; then printf '%s\n' "$ref" >> "$out/branches-removal-plan"; else printf 'Retained pre-existing/undated branch %s\n' "$ref" >> "$out/retained-preexisting"; fi
done < <(git for-each-ref --format='%(refname) %(objectname)' refs/heads)
printf 'Sensitive-pattern source inputs were excluded from publication. Original consumer scratch directory is retained; exact candidate paths are recorded in extra-source.secrets0. No secret values are recorded in manifests.\n' >> "$out/exceptions"
printf 'Retained /tmp/velnor-node2418-provenance-20261005/gpg-release-key-only: contains private-key material, cannot publish to Git. Agent process stopped; directory secured.\n' >> "$out/exceptions"
printf 'Retained /tmp/velnor-mbx-action-1.6.0: external upstream commit API returned HTTP 404; remote availability not confirmed, so not deleted.\n' >> "$out/exceptions"
printf 'User project checkouts under /root/Projects retained; ownership of their creation was not established. Their goal-related WIP and refs were preserved.\n' >> "$out/retained-preexisting"
printf '\nAdditional repo worktrees contributed 7 checkpoints (27 total at preparation), including old PR12 source/probes/retirement, native candidate and /var/tmp integration. Additional local refs/reflogs were pushed under additional/. Public/downloaded dependency caches and compiled outputs are generated state; authored source and existing receipts are preserved in split archives.\n' >> "$out/STOP.md"
[ ! -d /tmp/velnor-node2418-provenance-20261005/gpg-release-key-only ] || chmod 700 /tmp/velnor-node2418-provenance-20261005/gpg-release-key-only
if ps -p 3106067 -o args= | grep -q '^gpg-agent --homedir /tmp/velnor-node2418-provenance-20261005/gpg-release-key-only '; then kill -TERM 3106067; fi
printf 'Verified %s manifest records; cleanup plan %s worktrees, %s branches\n' "$(wc -l < "$out/preserved-refs.tsv")" "$(wc -l < "$out/worktrees-removal-plan")" "$(wc -l < "$out/branches-removal-plan")"
# Separate delivery tree protects the user's main checkout from evidence additions.
export GIT_INDEX_FILE="$root/.git/stop-delivery-index"
git --git-dir="$root/.git" --work-tree="$out" read-tree --empty
git --git-dir="$root/.git" --work-tree="$out" add -A
tree=$(git --git-dir="$root/.git" --work-tree="$out" write-tree)
sha=$(git -c user.name='Alexey Zhokhov' -c user.email='alexey@zhokhov.com' -c commit.gpgsign=false commit-tree "$tree" -m 'WIP: deliver immediate-stop notes, snapshots and preservation manifest [skip ci]' -m 'Existing partial outputs preserved as-is. This is a delivery checkpoint, not a completed implementation or new validation.' -m 'Co-authored-by: Codex <codex@openai.com>' -m 'Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>')
git push origin "$sha:refs/heads/preservation/stop-20261005/delivery" > "$out/push-delivery.log" 2>&1
actual=$(git ls-remote origin refs/heads/preservation/stop-20261005/delivery | cut -f1)
[ "$actual" = "$sha" ] || exit 1
printf '%s\n' "$sha" > "$out/delivery-sha"
printf 'Delivery verified %s\n' "$sha"

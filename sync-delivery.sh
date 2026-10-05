#!/bin/bash
set -euo pipefail
root=/root/Projects/tailrocks/velnor-project/velnor-new
out=/root/velnor-stop-preservation-20261005
cd "$root"
sort -u "$out/preserved-refs.tsv" -o "$out/preserved-refs.tsv"
find "$out/archives" -type f -print0 | sort -z | xargs -0 sha256sum > "$out/archive-hashes.sha256"
export GIT_INDEX_FILE="$root/.git/stop-delivery-index"
git --git-dir="$root/.git" --work-tree="$out" add -A
tree=$(git --git-dir="$root/.git" --work-tree="$out" write-tree)
parent=$(cat "$out/delivery-sha")
sha=$(git -c user.name='Alexey Zhokhov' -c user.email='alexey@zhokhov.com' -c commit.gpgsign=false commit-tree "$tree" -p "$parent" -m 'WIP: preserve remaining temporary snapshots and delivery records [skip ci]' -m 'Co-authored-by: Codex <codex@openai.com>' -m 'Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>')
git push origin "$sha:refs/heads/preservation/stop-20261005/delivery" >> "$out/push-delivery.log" 2>&1
actual=$(git ls-remote origin refs/heads/preservation/stop-20261005/delivery | cut -f1)
[ "$actual" = "$sha" ] || exit 1
printf '%s\n' "$sha" > "$out/delivery-sha"
printf '%s\n' "$sha"

#!/bin/bash
set -euo pipefail
out=/root/velnor-stop-preservation-20261005
root=/root/Projects/tailrocks/velnor-project/velnor-new
helper='!/root/.local/share/mise/installs/github-cli-cli/2.102.0/bin/gh auth git-credential'
for gd in /root/Projects/tailrocks/velnor-project/velnor-new-pr-12/.git /tmp/velnor-native-bcd2-repo-20261005.git /tmp/velnor-git-steward-retirement-20261005/.git /root/.velnor-generator-preview-20261005/pr65-source/.git; do
 id=$(printf '%s' "$gd" | sha256sum | cut -c1-12)
 git --git-dir="$gd" worktree list --porcelain > "$out/additional-worktrees-$id"
 while IFS= read -r p; do
  [ "$p" != "$gd" ] || continue
  wid=$(printf '%s' "$p" | sha256sum | cut -c1-12)
  git -C "$p" status --porcelain > "$out/status-$wid"
  if [ -s "$out/status-$wid" ]; then
   mkdir -p "$out/metadata/$wid"
   printf '%s\n' "$p" > "$out/metadata/$wid/path"
   wgd=$(git -C "$p" rev-parse --absolute-git-dir)
   for f in index MERGE_HEAD MERGE_MSG MERGE_MODE ORIG_HEAD; do [ ! -f "$wgd/$f" ] || cp "$wgd/$f" "$out/metadata/$wid/$f"; done
   git -C "$p" diff --binary > "$out/metadata/$wid/unstaged.patch"
   git -C "$p" diff --cached --binary > "$out/metadata/$wid/staged.patch"
   git -C "$p" ls-files -m -o --exclude-standard -z | while IFS= read -r -d '' f; do [ ! -f "$p/$f" ] || rg -Il -- '(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----)' "$p/$f" || true; done > "$out/secret-candidates-$wid"
   [ ! -s "$out/secret-candidates-$wid" ] || { printf 'Held source secret candidate %s\n' "$p" >> "$out/exceptions"; continue; }
   git -C "$p" add -A -- . ':!.velnor/archive-guard' ':!scripts/__pycache__'
   if ! git -C "$p" diff --cached --quiet; then
    git -C "$p" -c user.name='Alexey Zhokhov' -c user.email='alexey@zhokhov.com' -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -m 'WIP: checkpoint existing work at immediate stop [skip ci]' -m 'Unfinished work preserved unchanged; no new validation or implementation.' -m 'Co-authored-by: Codex <codex@openai.com>' -m 'Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>' > "$out/commit-$wid.log"
    printf '%s\t%s\n' "$p" "$(git -C "$p" rev-parse HEAD)" >> "$out/checkpoints.tsv"
   fi
  fi
  sha=$(git -C "$p" rev-parse HEAD)
  branch="preservation/stop-20261005/additional/$id/worktrees/$wid"
  git --git-dir="$gd" -c credential.helper= -c credential.helper="$helper" push https://github.com/tailrocks/velnor-new.git "$sha:refs/heads/$branch" >> "$out/push-additional.log" 2>&1
  actual=$(git -C "$root" ls-remote origin "refs/heads/$branch" | cut -f1)
  [ "$actual" = "$sha" ] || exit 1
  printf '%s\t%s\t%s\n' "$sha" "$branch" "$p" >> "$out/preserved-refs.tsv"
  printf '%s\n' "$p" >> "$out/additional-worktree-approved"
 done < <(sed -n 's/^worktree //p' "$out/additional-worktrees-$id")
 git --git-dir="$gd" for-each-ref --format='%(objectname) %(refname)' refs/heads refs/stash > "$out/additional-refs-$id"
 git --git-dir="$gd" reflog --all --format='%H' | sort -u | while read -r sha; do printf '%s reflog/%s\n' "$sha" "$sha"; done >> "$out/additional-refs-$id"
 (git --git-dir="$gd" reflog show refs/stash --format='%H' 2>/dev/null || true) | awk '{printf "%s stash/%02d\n",$0,NR}' >> "$out/additional-refs-$id"
 # Preserve the original graph without rewriting any historical commit.
 mapfile -t specs < <(awk -v prefix="preservation/stop-20261005/additional/$id/" '{sub(/^refs\//,"",$2); print $1 ":refs/heads/" prefix $2}' "$out/additional-refs-$id")
 if [ "${#specs[@]}" -gt 0 ]; then git --git-dir="$gd" -c credential.helper= -c credential.helper="$helper" push --atomic https://github.com/tailrocks/velnor-new.git "${specs[@]}" >> "$out/push-additional.log" 2>&1; fi
 git -C "$root" ls-remote origin "refs/heads/preservation/stop-20261005/additional/$id/*" > "$out/additional-remote-$id"
 while read -r sha ref; do branch="preservation/stop-20261005/additional/$id/${ref#refs/}"; grep -Fqx "$sha"$'\t'"refs/heads/$branch" "$out/additional-remote-$id" || exit 1; printf '%s\t%s\t%s\n' "$sha" "$branch" "$gd" >> "$out/preserved-refs.tsv"; done < "$out/additional-refs-$id"
done

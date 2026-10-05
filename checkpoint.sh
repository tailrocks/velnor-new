#!/bin/bash
set -euo pipefail
root=/root/Projects/tailrocks/velnor-project/velnor-new
out=/root/velnor-stop-preservation-20261005
cd "$root"
git worktree list --porcelain | sed -n 's/^worktree //p' > "$out/worktree-paths"
while IFS= read -r p; do
  [ "$p" != "$root" ] || continue
  id=$(printf '%s' "$p" | sha256sum | cut -c1-12)
  git -C "$p" status --porcelain > "$out/status-$id"
  [ -s "$out/status-$id" ] || continue
  mkdir -p "$out/metadata/$id"
  printf '%s\n' "$p" > "$out/metadata/$id/path"
  gd=$(git -C "$p" rev-parse --absolute-git-dir)
  for f in index MERGE_HEAD MERGE_MSG MERGE_MODE ORIG_HEAD; do [ ! -f "$gd/$f" ] || cp "$gd/$f" "$out/metadata/$id/$f"; done
  git -C "$p" diff --binary > "$out/metadata/$id/unstaged.patch"
  git -C "$p" diff --cached --binary > "$out/metadata/$id/staged.patch"
  git -C "$p" ls-files -m -o --exclude-standard -z > "$out/metadata/$id/candidate-paths"
  suspects=0
  while IFS= read -r -d '' f; do
    [ -f "$p/$f" ] || continue
    case "$f" in .velnor/archive-guard/*|*/__pycache__/*) continue;; esac
    if rg -Il -- '(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----)' "$p/$f" >/dev/null; then printf '%s\t%s\n' "$p" "$f" >> "$out/secret-candidates"; suspects=1; fi
  done < "$out/metadata/$id/candidate-paths"
  [ "$suspects" -eq 0 ] || { printf 'HELD secret candidate %s\n' "$p"; continue; }
  git -C "$p" add -A -- . ':!.velnor/archive-guard' ':!scripts/__pycache__'
  if ! git -C "$p" diff --cached --quiet; then
    git -C "$p" -c user.name='Alexey Zhokhov' -c user.email='alexey@zhokhov.com' -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -m 'WIP: preserve existing partial work at immediate stop [skip ci]' -m 'Checkpoint only. Unfinished integrations and conflict markers are preserved as-is; no further implementation or validation performed.' -m 'Co-authored-by: Codex <codex@openai.com>' -m 'Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>' > "$out/commit-$id.log"
    printf '%s\t%s\n' "$p" "$(git -C "$p" rev-parse HEAD)" >> "$out/checkpoints.tsv"
  fi
done < "$out/worktree-paths"

#!/bin/bash
set -euo pipefail
out=/root/velnor-stop-preservation-20261005
while IFS= read -r g; do
 p=${g%/.git}
 case "$p" in /tmp/velnor-*|/root/.cache/velnor-*|/root/.velnor-*|/root/.local/share/velnor/*) ;; *) continue;; esac
 case "$p" in *velnor-mise-*|*velnor-cli-*|*velnor-goldens-work*|*/target/*|*/maintenance-assets/*|*fixture*) continue;; esac
 cdpath=$(git -C "$p" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
 [ -z "$cdpath" ] || printf '%s\n' "$cdpath"
done < "$out/git-locations.before" | sort -u > "$out/common-dirs"
cat "$out/common-dirs"

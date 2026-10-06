#!/bin/bash
# Stay quiet while run 37252271536 uses the drained image or the cargo-symlink image.
# Wake on a third image, a new Scale Set failure, a symlink reject, or a terminal run.
set -u
export MISE_DISABLE=1
OLD="sha256:e1f52fda1be303f878eeac94b4407405ef332e8f6f8b72ce7ee95e9280b6f9e3"
NEW="sha256:07e2a620a246c1566f093ea5ed80cdd6fbee35700cf0fb72c5d1d26e76816d03"
RUN=37252271536
KNOWN_FAIL="111583386219"
while true; do
  for id in $(docker ps -q --filter label=velnor.role=runner); do
    img="$(docker inspect --format '{{.Image}}' "$id" 2>/dev/null || true)"
    if [ "$img" != "$OLD" ] && [ "$img" != "$NEW" ]; then
      echo "ACTION_REQUIRED: runner $id image $img"
      exit 0
    fi
    if [ "$img" = "$NEW" ]; then
      logs="$(docker logs "$id" 2>&1 || true)"
      if printf '%s\n' "$logs" | grep -q 'symlink escapes'; then
        echo "ACTION_REQUIRED: new image still rejects a symlink on $id"
        exit 0
      fi
      if printf '%s\n' "$logs" | grep -q 'Cache hit for: velnor-v1-sources'; then
        echo "ACTION_REQUIRED: cargo source hit on new image $id"
        exit 0
      fi
    fi
  done
  st="$(gh run view "$RUN" --repo ChainArgos/java-monorepo --json status,conclusion --jq '.status + " " + (.conclusion // "")' 2>/dev/null || true)"
  case "$st" in
    completed*)
      echo "ACTION_REQUIRED: run $RUN $st"
      exit 0
      ;;
  esac
  fail="$(gh api --paginate "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100" --jq '.jobs[] | select(.conclusion=="failure" and (.name | contains("Velnor Scale Set")) and .id != 111583386219 and .id != 111583386308) | .id' 2>/dev/null || true)"
  if [ -n "$fail" ]; then
    echo "ACTION_REQUIRED: scale-set failure $fail"
    exit 0
  fi
  sleep 30
done

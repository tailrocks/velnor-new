#!/bin/bash
# Wake when run 37252271536 has a Scale Set worker, a Scale Set failure, or a terminal run.
set -u
export MISE_DISABLE=1
WANT="sha256:e1f52fda1be303f878eeac94b4407405ef332e8f6f8b72ce7ee95e9280b6f9e3"
RUN=37252271536
while true; do
  st="$(gh run view "$RUN" --repo ChainArgos/java-monorepo --json status,conclusion --jq '.status + " " + (.conclusion // "")' 2>/dev/null || true)"
  case "$st" in
    completed*)
      echo "ACTION_REQUIRED: run $RUN $st"
      exit 0
      ;;
  esac
  jobs="$(gh api --paginate "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100" --jq '.jobs[] | select((.name | contains("Velnor Scale Set")) and (.status=="in_progress" or .conclusion=="failure")) | [.id, .name, (.runner_name // ""), (.conclusion // ""), .status] | @tsv' 2>/dev/null || true)"
  if [ -n "$jobs" ]; then
    while IFS="$(printf '\t')" read -r id name runner conclusion status; do
      [ -n "$id" ] || continue
      if [ "$conclusion" = "failure" ]; then
        echo "ACTION_REQUIRED: scale-set failure job $id $name"
        exit 0
      fi
      [ -n "$runner" ] || continue
      cid="$(docker ps -q --filter "label=velnor.volume=${runner}" | head -1)"
      [ -n "$cid" ] || continue
      img="$(docker inspect --format '{{.Image}}' "$cid" 2>/dev/null || true)"
      if [ "$img" != "$WANT" ]; then
        echo "ACTION_REQUIRED: job $id runner $runner image $img"
        exit 0
      fi
      echo "ACTION_REQUIRED: fresh job $id runner $runner image $img container $cid"
      exit 0
    done <<<"$jobs"
  fi
  sleep 20
done

#!/bin/bash
# Wake when a new runner has the drain perl, when scale-set failures grow,
# or when run 37250304506 reaches a terminal state.
set -u
export MISE_DISABLE=1
LOG="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-new-runner.trace"
WANT="c181b4f6111d84fe2f41dc1a961206959238f450675f5148fa6dc2f29fd6cc1c"
base_fail=3
while true; do
  for id in $(docker ps -q --filter label=velnor.role=runner); do
    img="$(docker inspect --format '{{.Image}}' "$id" 2>/dev/null || true)"
    case "$img" in
      sha256:c7b3b8bdd470*) continue ;;
    esac
    perl="$(docker exec "$id" sha256sum /usr/local/bin/tar-member.pl 2>/dev/null | awk '{print $1}')"
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) id=$id img=$img perl=$perl" >>"$LOG"
    if [ "$perl" = "$WANT" ]; then
      echo "ACTION_REQUIRED: drain perl is live on $id image $img"
      exit 0
    fi
  done
  st="$(gh run view 37250304506 --repo ChainArgos/java-monorepo --json status,conclusion --jq '.status + " " + (.conclusion // "")' 2>/dev/null || true)"
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) run=$st" >>"$LOG"
  case "$st" in
    completed*)
      echo "ACTION_REQUIRED: run 37250304506 $st"
      exit 0
      ;;
  esac
  fails="$(gh api --paginate 'repos/ChainArgos/java-monorepo/actions/runs/37250304506/jobs?per_page=100' --jq '[.jobs[] | select(.conclusion=="failure")] | length' 2>/dev/null || echo "$base_fail")"
  if [ "${fails:-0}" -gt "$base_fail" ]; then
    echo "ACTION_REQUIRED: scale-set failure count $fails was $base_fail"
    exit 0
  fi
  sleep 30
done

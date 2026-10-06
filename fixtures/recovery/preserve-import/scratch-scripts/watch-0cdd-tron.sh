#!/bin/bash
# Wake only on a decisive cargo-extract event for tron-migration.
# Job 111583386491 container gracious_tu image sha256:0cdd9fc3.
set -u
CID=gracious_tu
JOB=111583386491
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$SCRATCH/watch-0cdd-tron.log"
SEEN="$SCRATCH/watch-0cdd-tron.state"
: >"$LOG"
echo "start $(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
seen_plan=0
seen_dl=0
seen_link=0
seen_restore=0
seen_compile=0
dl_epoch=0

mark() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >>"$LOG"
}

while true; do
  running="$(docker inspect -f '{{.State.Running}}' "$CID" 2>>"$LOG" || echo false)"
  if [ "$running" != true ]; then
    echo "ACTION_REQUIRED: $CID stopped restore=$seen_restore compile=$seen_compile link=$seen_link"
    exit 0
  fi

  if [ "$seen_plan" = 0 ]; then
    snapshot="$(docker exec "$CID" sh -c 'f=$(ls /tmp/velnor-tar-plan.* 2>/dev/null | head -1); if [ -n "$f" ]; then lines=$(wc -l <"$f"); maxnf=$(awk "{print NF}" "$f" | sort -n | tail -1); echo "$lines $maxnf"; fi' 2>>"$LOG" || true)"
    if [ -n "$snapshot" ]; then
      mark "plan $snapshot"
      echo "ACTION_REQUIRED: plan lines=${snapshot%% *} max_fields=${snapshot##* }"
      seen_plan=1
      lines="${snapshot%% *}"
      if [ "$lines" -gt 100 ]; then
        echo "FAILED: per-member plan still active lines=$lines"
        exit 1
      fi
    fi
  fi

  if [ "$seen_link" = 0 ]; then
    link="$(docker exec "$CID" sh -c 'readlink /home/runner/work/_temp/velnor/cargo/bin/rls 2>/dev/null || true' 2>>"$LOG" || true)"
    if [ "$link" = rustup ]; then
      seen_link=1
      mark "rls -> rustup"
      echo "ACTION_REQUIRED: rls -> rustup"
    fi
  fi

  one() {
    docker exec "$CID" sh -c "grep -F -m 1 -h -- '$1' /home/runner/_diag/pages/*.log 2>/dev/null || true" 2>>"$LOG" || true
  }
  if [ "$seen_dl" = 0 ]; then
    if one 'Received 147446399 of 147446399' | grep -F -q 'Received 147446399 of 147446399'; then
      seen_dl=1
      dl_epoch="$(date -u +%s)"
      mark "download complete"
      echo "ACTION_REQUIRED: cargo download complete"
    fi
  fi
  if [ "$seen_restore" = 0 ]; then
    if one 'symlink escapes' | grep -F -q 'symlink escapes'; then
      echo "FAILED: symlink escapes on 0cdd9fc3"
      exit 1
    fi
    if one 'member list failed' | grep -F -q 'member list failed'; then
      echo "FAILED: member list failed on 0cdd9fc3"
      exit 1
    fi
    if one 'Cache restored successfully' | grep -F -q 'Cache restored successfully'; then
      seen_restore=1
      mark "cache restored"
      echo "ACTION_REQUIRED: Cache restored successfully"
    fi
  fi
  if [ "$seen_compile" = 0 ] && [ "$seen_restore" = 1 ]; then
    if one 'Compiling ' | grep -F -q 'Compiling '; then
      seen_compile=1
      mark "compile started"
      echo "ACTION_REQUIRED: compile started"
    fi
  fi

  if [ "$seen_dl" = 1 ] && [ "$seen_restore" = 0 ]; then
    now="$(date -u +%s)"
    if [ $((now - dl_epoch)) -gt 480 ]; then
      echo "FAILED: no cache restore within 8 minutes after download link=$seen_link plan=$seen_plan"
      exit 1
    fi
  fi

  conclusion="$(gh api "repos/ChainArgos/java-monorepo/actions/jobs/${JOB}" --jq '.conclusion // "in_progress"' 2>>"$LOG" || echo api_err)"
  case "$conclusion" in
    success|failure|cancelled)
      mark "job $conclusion"
      echo "ACTION_REQUIRED: job ${JOB} ${conclusion} restore=${seen_restore} compile=${seen_compile} link=${seen_link}"
      exit 0
      ;;
  esac
  sleep 30
done

#!/bin/bash
# Phase watcher for magical_cori, image sha256:07ba2bf2.
set -u
CID=magical_cori
LOG="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-magical.log"
: >"$LOG"
mark() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >>"$LOG"; }
seen_plan=0
seen_restore=0
seen_compile=0
seen_reg=0
mark "start"
while true; do
  running="$(docker inspect -f '{{.State.Running}}' "$CID" 2>>"$LOG" || echo false)"
  if [ "$running" != true ]; then
    echo "ACTION_REQUIRED: $CID stopped restore=$seen_restore compile=$seen_compile reg=$seen_reg"
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
        echo "FAILED: per-member plan lines=$lines"
        exit 1
      fi
    fi
  fi
  if [ "$seen_reg" = 0 ]; then
    if docker exec "$CID" sh -c 'test -d /home/runner/work/_temp/velnor/cargo/registry/cache' 2>>"$LOG"; then
      seen_reg=1
      mark "registry cache dir exists"
      echo "ACTION_REQUIRED: registry cache directory exists"
    fi
  fi
  one() {
    docker exec "$CID" sh -c "grep -F -m 1 -h -- '$1' /home/runner/_diag/pages/*.log 2>/dev/null || true" 2>>"$LOG" || true
  }
  if [ "$seen_restore" = 0 ]; then
    if one 'member missing after extract' | grep -F -q 'member missing after extract'; then
      echo "FAILED: member missing after extract"
      exit 1
    fi
    if one 'member extract failed' | grep -F -q 'member extract failed'; then
      echo "FAILED: member extract failed"
      exit 1
    fi
    if one 'symlink escapes' | grep -F -q 'symlink escapes'; then
      echo "FAILED: symlink escapes"
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
  sleep 20
done

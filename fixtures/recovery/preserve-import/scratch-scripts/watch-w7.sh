#!/usr/bin/env bash
# Print only when the w7 worker pair is gone. Do not write velnor-seed.
set -u
LOG="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-w7.log"
NAME="w7a5eed3e3cad06ad769fedbb8c29496c"
while true; do
  up=$(docker ps --filter "name=${NAME}" --format '{{.Names}}' | wc -l | tr -d ' ')
  printf '%s up=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$up" >>"$LOG"
  if [ "$up" = "0" ]; then
    echo "ACTION_REQUIRED: w7 workers exited up=0"
    exit 0
  fi
  sleep 30
done

#!/bin/bash
PID=52163
ERR="$HOME/Library/Logs/Velnor/host.err.log"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"
start=$(wc -c < "$ERR" | tr -d ' ')
stall=0
while true; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "ACTION_REQUIRED: daemon $PID exited"
    exit 0
  fi
  runners=$(docker ps --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)
  if [ "${runners:-0}" -ge 1 ]; then
    echo "ACTION_REQUIRED: runners=$runners"
    exit 0
  fi
  now=$(wc -c < "$ERR" | tr -d ' ')
  if [ "$now" -gt "$start" ]; then
    chunk=$(tail -c +"$((start + 1))" "$ERR" || true)
    start=$now
    if printf '%s\n' "$chunk" | grep -E -q 'batch id=|JobAvailable|started=true'; then
      echo "ACTION_REQUIRED: broker sent a job batch"
      exit 0
    fi
  fi
  stall=$((stall + 1))
  if [ "$stall" -ge 6 ]; then
    echo "ACTION_REQUIRED: no job batch and no runner for 90s while daemon $PID is up"
    exit 0
  fi
  sleep 15
done

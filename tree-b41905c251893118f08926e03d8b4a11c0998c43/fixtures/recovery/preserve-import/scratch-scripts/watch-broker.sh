#!/bin/bash
PID=52163
SA=52167
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$SCRATCH/broker-watch.log"
while true; do
  now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "$now daemon-exited" >>"$LOG"
    echo "ACTION_REQUIRED: daemon $PID exited"
    exit 0
  fi
  if lsof -nP -p "$PID" 2>/dev/null | grep -E -q 'TCP|IPv4|IPv6'; then
    echo "$now socket" >>"$LOG"
    echo "ACTION_REQUIRED: daemon $PID opened a socket"
    exit 0
  fi
  runners="$(docker ps --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)"
  if [ "${runners:-0}" -ge 1 ]; then
    echo "$now runners=$runners" >>"$LOG"
    echo "ACTION_REQUIRED: runner containers=$runners"
    exit 0
  fi
  if ! kill -0 "$SA" 2>/dev/null; then
    echo "$now securityagent-exited" >>"$LOG"
    echo "ACTION_REQUIRED: SecurityAgent $SA exited and daemon $PID still has no socket"
    exit 0
  fi
  echo "$now still-blocked" >>"$LOG"
  sleep 30
done

#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

root=$(dirname "$0")
. "$root/fake_orb.env"
seen=$FAKE_ORB_SEEN
printf '%s\n' "$*" >> "$FAKE_ORB_LOG"

gone() {
  [ ! -d "$root" ] && return 0
  [ -n "$FAKE_TEST_PID" ] && ! kill -0 "$FAKE_TEST_PID" 2>/dev/null
}

lock() {
  tries=0
  while ! mkdir "$seen/.lock" 2>/dev/null; do
    owner=$(cat "$seen/.lock/owner" 2>/dev/null)
    if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
      rm -rf "$seen/.lock"
      continue
    fi
    gone && exit 75
    tries=$((tries + 1))
    [ "$tries" -gt 3000 ] && exit 75
    sleep 0.01
  done
  echo $$ > "$seen/.lock/owner"
}

unlock() {
  rm -rf "$seen/.lock"
}

mkdir -p "$seen/calls"
lock
call=$(cat "$seen/calls.count" 2>/dev/null || echo 0)
call=$((call + 1))
echo "$call" > "$seen/calls.count"
unlock
if [ -n "$FAKE_REQUESTS" ]; then
  lock
  seq=$(cat "$seen/seq" 2>/dev/null || echo 0)
  seq=$((seq + 1))
  echo "$seq" > "$seen/seq"
  printf '%s - ORB %s %s\n' "$seq" "$call" "$*" >> "$FAKE_REQUESTS"
  unlock
fi
env > "$seen/calls/$call.env"
trap 'echo $? > "$seen/calls/$call.status"' EXIT

in_table() {
  [ -f "$FAKE_ORB_PS" ] && grep -q "^[[:space:]]*$1 " "$FAKE_ORB_PS"
}

drop_from_table() {
  lock
  if [ -f "$FAKE_ORB_PS" ]; then
    grep -v "^[[:space:]]*$1 " "$FAKE_ORB_PS" > "$FAKE_ORB_PS.tmp"
    mv "$FAKE_ORB_PS.tmp" "$FAKE_ORB_PS"
  fi
  unlock
}

add_to_table() {
  lock
  printf '%s\n' "$1" >> "$FAKE_ORB_PS"
  unlock
}

hang() {
  [ "$FAKE_ORB_HANG" = "$1" ] || return 0
  echo $$ > "$seen/hang.$call.pid"
  while [ ! -e "$seen/unhang" ] && ! gone; do
    sleep 0.05
  done
  exit 70
}

immortal() {
  [ -e "$seen/release" ] && return 1
  for pid in $(echo "$FAKE_ORB_PS_IMMORTAL" | tr ',' ' '); do
    [ "$pid" = "$1" ] && return 0
  done
  return 1
}

case "$1" in
list)
  hang list
  cat "$FAKE_ORB_LIST"
  exit 0
  ;;
create)
  printf '[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]\n' > "$FAKE_ORB_LIST"
  exit 0
  ;;
-m) ;;
*) exit 64 ;;
esac

shift 2
if [ "$1" = "-u" ]; then
  shift 2
fi

if [ "$1" = unshare ]; then
  while [ "$1" != /var/lib/velnor/bin/velnor-runner ]; do
    shift
  done
fi

case "$1" in
ln)
  exit 0
  ;;
test)
  [ -x "$FAKE_ORB_STORE${3#/var/lib/velnor}" ]
  exit $?
  ;;
ps)
  if [ -n "$FAKE_ORB_PS_FAIL" ] && in_table 7001; then
    failed=$(cat "$seen/ps.failed" 2>/dev/null || echo 0)
    if [ "$failed" -lt "$FAKE_ORB_PS_FAIL" ]; then
      echo $((failed + 1)) > "$seen/ps.failed"
      exit 1
    fi
  fi
  {
    [ -z "$FAKE_ORB_PS_NO_INIT" ] && printf '1 /sbin/init\n'
    [ -f "$FAKE_ORB_PS" ] && cat "$FAKE_ORB_PS"
  } | tee "$seen/calls/$call.out"
  exit 0
  ;;
kill)
  shift 2
  for pid in "$@"; do
    echo "$pid" >> "$seen/kills"
    immortal "$pid" || drop_from_table "$pid"
  done
  exit 0
  ;;
/var/lib/velnor/bin/velnor-runner)
  jit="$FAKE_ORB_STORE${4#@/var/lib/velnor}"
  work="$FAKE_ORB_STORE${6#/var/lib/velnor}"
  name=$(basename "$jit")
  cp "$jit" "$seen/$name"
  env > "$seen/$name.env"
  if [ "$(uname)" = Darwin ]; then
    stat -f %Lp "$jit" > "$seen/$name.mode"
  else
    stat -c %a "$jit" > "$seen/$name.mode"
  fi
  mkdir -p "$work"
  entry=${FAKE_RUNNER_PS_PID:-$$}
  add_to_table "$entry $*"
  if [ -z "$FAKE_RUNNER_PS_PID" ]; then
    trap 'drop_from_table $$; echo $? > "$seen/calls/$call.status"' EXIT
  fi
  if [ -n "$FAKE_RUNNER_LOCK" ]; then
    mkdir -p "$work/locked"
    : > "$work/locked/keep"
    chmod 0500 "$work/locked"
  fi
  if [ -n "$FAKE_RUNNER_HOLD" ]; then
    sleep 60 &
    echo $! > "$seen/$name.child"
  fi
  echo $$ > "$seen/$name.pid"
  if [ -n "$FAKE_RUNNER_HOLD" ]; then
    while [ ! -e "$seen/release-$name" ] && ! gone; do
      sleep 0.05
    done
    kill $! 2>/dev/null
  fi
  exit "${FAKE_RUNNER_EXIT:-0}"
  ;;
*) exit 64 ;;
esac

#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

. "$(dirname "$0")/fake_orb.env"
printf '%s\n' "$*" >> "$FAKE_ORB_LOG"

case "$1" in
list)
  cat "$FAKE_ORB_LIST"
  exit 0
  ;;
create)
  printf '[{"name":"velnor"}]\n' > "$FAKE_ORB_LIST"
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
/var/lib/velnor/bin/velnor-runner)
  jit="$FAKE_ORB_STORE${4#@/var/lib/velnor}"
  name=$(basename "$jit")
  cp "$jit" "$FAKE_ORB_SEEN/$name"
  env > "$FAKE_ORB_SEEN/$name.env"
  if [ "$(uname)" = Darwin ]; then
    stat -f %Lp "$jit" > "$FAKE_ORB_SEEN/$name.mode"
  else
    stat -c %a "$jit" > "$FAKE_ORB_SEEN/$name.mode"
  fi
  if [ -n "$FAKE_RUNNER_HOLD" ]; then
    sleep 60 &
    echo $! > "$FAKE_ORB_SEEN/$name.child"
    wait
  fi
  exit "${FAKE_RUNNER_EXIT:-0}"
  ;;
*) exit 64 ;;
esac

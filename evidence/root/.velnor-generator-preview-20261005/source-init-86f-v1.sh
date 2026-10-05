#!/bin/sh
set -eu
umask 077

TASK=/root/.velnor-generator-preview-20261005
SOURCE="$TASK/main-source-86f"
COMMIT=86f864aabc2192a9f8ccd3f01f2e25b7858c3f20
ORIGIN=https://github.com/tailrocks/velnor-new.git

[ "$#" -eq 0 ] || exit 2
[ ! -e "$SOURCE" ] && [ ! -L "$SOURCE" ] || exit 2
[ -d "$TASK" ] && [ ! -L "$TASK" ] || exit 2

exec 3>&1
umask 077
/usr/bin/date -u +%Y-%m-%dT%H:%M:%SZ >&3

/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  LC_ALL=C \
  /usr/bin/git clone --no-checkout --no-tags --template=/dev/null "$ORIGIN" "$SOURCE" >&3

/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" config --local core.hooksPath /dev/null

/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" update-ref refs/heads/main "$COMMIT"

REMOTE_TIP=$(/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" rev-parse refs/remotes/origin/main)
[ "${#REMOTE_TIP}" -eq 40 ] || exit 2
case "$REMOTE_TIP" in *[!0-9a-f]*) exit 2 ;; esac

HEAD_COMMIT=$(/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" rev-parse HEAD)
[ "$HEAD_COMMIT" = "$COMMIT" ] || exit 2

TREE=$(/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" rev-parse 'HEAD^{tree}')
[ "$TREE" = 1e16a9270f3ea99b04e68521cfc876fa19db67ac ] || exit 2

ORIGIN_CHECK=$(/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" remote get-url origin)
[ "$ORIGIN_CHECK" = "$ORIGIN" ] || exit 2

ORIGIN_HEAD=$(/usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME=/nonexistent \
  GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 \
  GIT_NO_LAZY_FETCH=1 \
  GIT_NO_REPLACE_OBJECTS=1 \
  GIT_PROTOCOL_FROM_USER=0 \
  GIT_OPTIONAL_LOCKS=0 \
  LC_ALL=C \
  /usr/bin/git -C "$SOURCE" rev-parse --abbrev-ref origin/HEAD)
[ "$ORIGIN_HEAD" = origin/main ] || exit 2

printf 'source_commit=%s\nsource_tree=%s\nobserved_origin_main=%s\norigin=%s\n' \
  "$HEAD_COMMIT" "$TREE" "$REMOTE_TIP" "$ORIGIN_CHECK" >&3

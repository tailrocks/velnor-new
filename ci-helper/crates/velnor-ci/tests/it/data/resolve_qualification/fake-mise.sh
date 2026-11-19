#!/bin/sh
# Stands in for mise when the legacy op runs `mise ... exec gh@<version> -- gh api <route>`.
# Every route is appended to $VELNOR_FAKE_API/requests.log; the answer is the file named after the
# route in $VELNOR_FAKE_API (every character outside A-Za-z0-9._- replaced by _), or a failure.
# `mise ls --json <tool>` answers that the pinned rust is installed, which is all the legacy
# preparation asks before it reaches the resolver.
printf '%s\n' "$*" >> "$VELNOR_FAKE_API/mise.log"
case " $* " in
  *" ls --json "*)
    for last; do :; done
    version=
    [ "$last" = rust ] && version=1.99.1
    printf '[{"version":"%s","installed":true}]\n' "$version"
    exit 0 ;;
esac
route=
while [ "$#" -gt 0 ]; do
  if [ "$1" = api ]; then route=$2; break; fi
  shift
done
[ -n "$route" ] || exit 0
printf '%s\n' "$route" >> "$VELNOR_FAKE_API/requests.log"
name=$(printf '%s' "$route" | tr -c 'A-Za-z0-9._-' '_')
[ -f "$VELNOR_FAKE_API/$name" ] || exit 1
cat "$VELNOR_FAKE_API/$name"

#!/bin/sh
set -eu

if [ -n "${VELNOR_TOPOLOGY_MARKER:-}" ]; then
  : "${GITHUB_WORKSPACE:?missing Docker-action workspace}"
  marker_path="$GITHUB_WORKSPACE/$VELNOR_TOPOLOGY_MARKER"
  if [ ! -f "$marker_path" ] || [ "$(cat "$marker_path")" != docker-action-work-volume-ok ]; then
    printf '%s\n' 'Docker-action workspace marker is missing or invalid' >&2
    exit 1
  fi
  printf '%s\n' workspace-mount-ok
fi
printf '%s\n' docker-action-ok

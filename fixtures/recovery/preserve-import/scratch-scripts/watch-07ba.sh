#!/bin/bash
# Wake when a worker is created from the directory-safe tar image.
set -u
while true; do
  line="$(docker ps --filter label=velnor.role=runner --format '{{.Names}} {{.ID}} {{.Image}}' | grep '07ba2bf2' || true)"
  if [ -n "$line" ]; then
    echo "ACTION_REQUIRED: directory-safe worker $line"
    exit 0
  fi
  sleep 15
done

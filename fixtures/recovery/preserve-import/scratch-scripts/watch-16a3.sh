#!/bin/bash
# Wake when a worker is created from the dirname-fix image.
set -u
WANT=sha256:16a3ec9c82f4abccc1c06462959f2857320d97a6027bec845fbe5bcc2e61b79d
while true; do
  line="$(docker ps --filter label=velnor.role=runner --format '{{.Names}} {{.ID}} {{.Image}}' | grep '16a3ec9c' || true)"
  if [ -n "$line" ]; then
    echo "ACTION_REQUIRED: dirname-fix worker $line"
    exit 0
  fi
  sleep 20
done

#!/bin/bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
mounts="${VELNOR_PROC_MOUNTS:-/proc/mounts}"
if [[ -r "$mounts" ]] && awk '$2 == "/" && $4 ~ /(^|,)ro(,|$)/ { ro = 1 } END { exit !ro }' "$mounts"; then
  exit 0
fi
sudo -n rm -f -- /.dockerenv

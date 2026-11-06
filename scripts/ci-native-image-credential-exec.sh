#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
unset_args=()
while IFS= read -r -d '' entry; do
  name=${entry%%=*}
  case "$name" in
    *TOKEN*|*PASSWORD*|*SECRET*|GH_HOST|GH_CONFIG_DIR|BASH_ENV|ENV|BASH_FUNC_*) unset_args+=(-u "$name") ;;
  esac
done < <(/usr/bin/env -0)
exec /usr/bin/env "${unset_args[@]}" "$@"

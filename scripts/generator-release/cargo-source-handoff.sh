#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

mode="${1:-}"
source_home="${2:-}"
destination="${3:-}"
[[ "$mode" == check && "$#" == 2 || "$mode" == copy && "$#" == 3 ]] || exit 2
[[ "$source_home" == /* && -d "$source_home" && ! -L "$source_home" ]] || exit 1
[[ "$(cd "$source_home" && pwd -P)" == "$source_home" ]] || exit 1
[[ -d "$source_home/registry" && ! -L "$source_home/registry" ]] || exit 1
scratch="$(mktemp -d "${TMPDIR:-/tmp}/velnor-source-handoff.XXXXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
entries=0
bytes=0
if [[ "$(uname -s)" == Darwin ]]; then stat_format=(-f '%l %z'); else stat_format=(-c '%h %s'); fi
scan() {
  local base="$1" file links size
  if ! find -P "$base" -print0 | head -c 67108865 >"$scratch/paths"; then return 1; fi
  (( $(wc -c < "$scratch/paths") <= 67108864 )) || return 1
  while IFS= read -r -d '' file; do
    entries=$((entries + 1))
    (( entries <= 10000 )) || { printf 'source handoff exceeds 10000 entries\n' >&2; return 1; }
    [[ ! -L "$file" ]] || return 1
    [[ -f "$file" || -d "$file" ]] || return 1
  done <"$scratch/paths"
  find -P "$base" -type f -exec stat "${stat_format[@]}" {} + >"$scratch/sizes"
  while read -r links size; do
    [[ "$links" == 1 && "$size" =~ ^[0-9]+$ ]] || return 1
    bytes=$((bytes + size))
    (( bytes <= 536870912 )) || { printf 'source handoff exceeds 512 MiB logical bytes\n' >&2; return 1; }
  done <"$scratch/sizes"
}

for family in index cache; do
  path="$source_home/registry/$family"
  [[ -d "$path" && ! -L "$path" ]] || exit 1
  scan "$path"
done
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
if [[ "$mode" == check ]]; then
  /usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 "$script_dir/cargo-git-handoff.py" inspect "$source_home" "$scratch" "$entries" "$bytes"
fi
if [[ "$mode" == copy ]]; then
  [[ "$destination" == /* && -d "$destination" && ! -L "$destination" ]] || exit 1
  [[ ! -e "$destination/registry" && ! -L "$destination/registry" ]] || exit 1
  mkdir -m 700 -- "$destination/registry"
  for family in index cache; do
    mkdir -m 700 -- "$destination/registry/$family"
    cp -RP -- "$source_home/registry/$family/." "$destination/registry/$family/"
  done
  entries=0
  bytes=0
  scan "$destination/registry/index"
  scan "$destination/registry/cache"
  /usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 "$script_dir/cargo-git-handoff.py" "$source_home" "$destination" "$entries" "$bytes"
fi

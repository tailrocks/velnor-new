#!/bin/bash
set -euo pipefail
out=/root/velnor-stop-preservation-20261005
sort -zu "$out/evidence.paths0" > "$out/evidence.unique0"
while IFS= read -r -d '' p; do
  case "$p" in "$out"/*) continue;; esac
  [ -f "$p" ] || continue
  if rg -Il -- '(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----|[?&](sig|signature|access_token|token)=[A-Za-z0-9%+/]{20,})' "$p" >/dev/null; then printf 'Sensitive evidence retained locally: %s\n' "$p" >> "$out/exceptions"; continue; fi
  sz=$(stat -c '%s' "$p")
  if [ "$sz" -gt 90000000 ]; then printf 'Oversize evidence retained locally (%s bytes): %s\n' "$sz" "$p" >> "$out/exceptions"; continue; fi
  dest="$out/evidence/${p#/}"
  mkdir -p "${dest%/*}"
  cp "$p" "$dest"
  printf '%s\t%s\n' "$(sha256sum "$p" | cut -d' ' -f1)" "$p" >> "$out/evidence-hashes.tsv"
done < "$out/evidence.unique0"

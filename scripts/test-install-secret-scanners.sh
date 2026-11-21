#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
installer="$root/scripts/install-secret-scanners.sh"
workflow="$root/.github/workflows/ci.yml"
fail() { echo "FAIL: $*" >&2; exit 1; }

job=$(awk '/^  secret-scan:/ {on=1; print; next} on && /^  [A-Za-z0-9_-]+:/ {on=0} on {print}' "$workflow")
[ -n "$job" ] || fail "no secret-scan job in $workflow"
if printf '%s\n' "$job" | grep -Ev '^[[:space:]]*#' | grep -Eiq 'mise|tool-versions'; then
  fail "the secret-scan job must not use mise or read .tool-versions"
fi
if grep -Ev '^[[:space:]]*#' "$installer" | grep -Eiq 'mise|tool-versions'; then
  fail "the installer must not use mise or read .tool-versions"
fi
grep -q 'sha256sum --check' "$installer" || fail "the installer does not verify sha256"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin" "$tmp/repo"
cat >"$tmp/repo/mise.toml" <<'TOML'
[tools]
gitleaks = "0.0.1"
betterleaks = "0.0.1"
kingfisher = "0.0.1"
TOML
printf 'gitleaks 0.0.1\n' >"$tmp/repo/.tool-versions"
cat >"$tmp/bin/mise" <<SH
#!/bin/sh
echo "\$*" >>"$tmp/mise-called"
exit 1
SH
cat >"$tmp/bin/curl" <<'SH'
#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
  [ "$1" = --output ] && out=$2
  shift
done
printf 'not the pinned release\n' >"$out"
SH
chmod +x "$tmp/bin/mise" "$tmp/bin/curl"

if (cd "$tmp/repo" && PATH="$tmp/bin:$PATH" MISE_TRUSTED_CONFIG_PATHS="$tmp/repo" bash "$installer" "$tmp/out") 2>"$tmp/err"; then
  fail "the installer accepted a download that does not match the pinned sha256"
fi
grep -q 'sha256 mismatch' "$tmp/err" || { cat "$tmp/err" >&2; fail "the installer failed for another reason than the sha256 pin"; }
[ ! -e "$tmp/mise-called" ] || fail "the installer ran mise"
[ ! -e "$tmp/out/bin/gitleaks" ] || fail "the installer installed a binary whose sha256 did not match"
echo "secret scanner pins: OK"

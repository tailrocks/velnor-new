#!/usr/bin/env bash
# Tests scripts/secret-scan-worktree.sh on temp trees. A fake GitHub token planted in a tree must
# make the scan exit with kingfisher's findings code (200), and a clean tree must exit 0.
#
# The scanner is the pinned kingfisher when it is on PATH. Otherwise a stub named kingfisher is put
# first on PATH: it exits 200 when a ghp_ token is in the tree and 0 when it is not, the same exit
# codes kingfisher 2.19.1 uses. CI runs this before the installer, so CI uses the stub.
# The fake token is built at runtime, so this file holds no token-shaped literal.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scan="$root/scripts/secret-scan-worktree.sh"
fail() { echo "FAIL: $*" >&2; exit 1; }
[ -f "$scan" ] || fail "no $scan"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/stub" "$tmp/tree"

pin=$(sed -n 's/^KINGFISHER_VERSION=//p' "$root/scripts/install-secret-scanners.sh")
[ -n "$pin" ] || fail "no KINGFISHER_VERSION in the installer"
if command -v kingfisher >/dev/null 2>&1 && [ "$(kingfisher --version 2>/dev/null)" = "kingfisher $pin" ]; then
  scanner=pinned
else
  scanner=stub
  cat >"$tmp/stub/kingfisher" <<'SH'
#!/bin/sh
# Stub for the scan arguments, which it ignores: 200 on a ghp_ token, 0 otherwise.
if grep -rqE 'ghp_[A-Za-z0-9]{36}' --exclude-dir=.git .; then exit 200; fi
exit 0
SH
  chmod +x "$tmp/stub/kingfisher"
  PATH="$tmp/stub:$PATH"
fi
echo "secret-scan-worktree scanner: $scanner (kingfisher $pin)"

# 36 hex characters from /dev/urandom: a GitHub-token-shaped value that differs on every run.
token="ghp_$(od -An -N18 -tx1 /dev/urandom | tr -d ' \n')"
printf 'GITHUB_TOKEN=%s\n' "$token" >"$tmp/tree/app.env"
set +e
(cd "$tmp/tree" && bash "$scan") >"$tmp/planted.log" 2>&1
rc=$?
set -e
[ "$rc" -eq 200 ] || { cat "$tmp/planted.log" >&2; fail "planted token: exit $rc, want 200 (findings)"; }
if grep -q "$token" "$tmp/planted.log"; then fail "the scan printed the planted token; --redact is missing"; fi

rm "$tmp/tree/app.env"
printf 'no secrets here\n' >"$tmp/tree/README.txt"
(cd "$tmp/tree" && bash "$scan") >"$tmp/clean.log" 2>&1 || {
  cat "$tmp/clean.log" >&2
  fail "clean tree: exit $?, want 0"
}
echo "secret-scan-worktree: OK"

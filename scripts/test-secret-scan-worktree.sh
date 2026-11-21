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
# Stub: exit 2 when --baseline-file names a file that is not there, relative to the cwd (the real
# kingfisher ignores a missing baseline silently). Otherwise 200 on a ghp_ token, 0 if none.
prev=
for a in "$@"; do
  if [ "$prev" = --baseline-file ] && [ ! -f "$a" ]; then echo "stub: no baseline $a" >&2; exit 2; fi
  prev=$a
done
if grep -rqE 'ghp_[A-Za-z0-9]{36}' --exclude-dir=.git .; then exit 200; fi
exit 0
SH
  chmod +x "$tmp/stub/kingfisher"
  PATH="$tmp/stub:$PATH"
fi
echo "secret-scan-worktree scanner: $scanner (kingfisher $pin)"

# A GitHub-token-shaped value (ghp_ + 36 characters) from fixed pieces, so the scan result is the same on every run.
token="ghp_$(printf '%s' 'Ab3Cd5Ef7Gh9' 'Jk2Lm4Np6Qr8' 'St0Uv1Wx3Yz5')"
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
# The wrapper reads its baseline from the repo root, so the clean tree above (scanned from a cwd
# that has no baseline) must pass. A wrapper copy in a root without the baseline must fail clearly.
mkdir -p "$tmp/noroot/scripts"
cp "$scan" "$tmp/noroot/scripts/"
set +e
bash "$tmp/noroot/scripts/secret-scan-worktree.sh" >"$tmp/noroot.log" 2>&1
rc=$?
set -e
[ "$rc" -ne 0 ] || fail "wrapper without a baseline file exited 0"
grep -q 'baseline' "$tmp/noroot.log" || { cat "$tmp/noroot.log" >&2; fail "missing-baseline error does not name the baseline"; }
echo "secret-scan-worktree: OK"

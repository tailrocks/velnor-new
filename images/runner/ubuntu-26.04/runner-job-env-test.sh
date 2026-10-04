#!/bin/bash
# Half of 18 is 9. One CPU stays 1. Other .env lines stay.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
dir="$(mktemp -d)"
bin="$(mktemp -d)"
trap 'rm -rf "$dir" "$bin"' EXIT
printf '%s\n' '#!/bin/bash' 'echo 18' >"$bin/nproc"
chmod 0755 "$bin/nproc"
export PATH="$bin:$PATH"
export RUNNER_ROOT="$dir"
bash "$root/runner-job-env.sh"
grep -qx 'CARGO_BUILD_JOBS=9' "$dir/.env"
printf '%s\n' 'KEEP=1' >>"$dir/.env"
bash "$root/runner-job-env.sh"
grep -qx 'KEEP=1' "$dir/.env"
grep -qx 'CARGO_BUILD_JOBS=9' "$dir/.env"
test "$(grep -c '^CARGO_BUILD_JOBS=' "$dir/.env")" -eq 1
printf '%s\n' '#!/bin/bash' 'echo 1' >"$bin/nproc"
bash "$root/runner-job-env.sh"
grep -qx 'CARGO_BUILD_JOBS=1' "$dir/.env"
grep -qx 'KEEP=1' "$dir/.env"
echo "runner-job-env ok"

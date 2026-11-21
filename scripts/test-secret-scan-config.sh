#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fail() { echo "FAIL: $*" >&2; exit 1; }

for scanner in gitleaks betterleaks; do
  command -v "$scanner" >/dev/null 2>&1 || fail "$scanner is not on PATH"
done
for cfg in .gitleaks.toml .betterleaks.toml; do
  [ -f "$root/$cfg" ] || fail "missing $root/$cfg"
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$root/.gitleaks.toml" "$root/.betterleaks.toml" "$tmp/"
mkdir "$tmp/tree"

token="ghp_$(printf '%s' 'Ab3Cd5Ef7Gh9' 'Jk2Lm4Np6Qr8' 'St0Uv1Wx3Yz5')"
fixtures=(
  crates/velnor-actions-contract/src/config/dbt_tests.rs
  crates/velnor-actions-contract/src/workflow/qualification_cache_lineage_tests.rs
  crates/velnor-actions-mise/tests/impl_mise_env_policy.rs
  crates/velnor-ci/tests/it/audit/selection.rs
  crates/velnor-ci/tests/it/cache_credential_scrub.rs
  crates/velnor-ci/tests/it/cache_private_repo.rs
  crates/velnor-ci/tests/it/public_probe.rs
  crates/velnor-runner/crates/velnor-runner-attestation/tests/fixtures/tough/snakeoil.pem
  crates/velnor-runner/crates/velnor-runner-host/src/launch/gate/removal_tests.rs
  generator/crates/velnor-gen/tests/it/detect_private.rs
)
for f in "${fixtures[@]}"; do
  mkdir -p "$tmp/tree/$(dirname "$f")"
  printf 'const TOKEN: &str = "%s";\n' "$token" >"$tmp/tree/$f"
done
mkdir "$tmp/tree/src"
printf 'let password = "%s";\n' secret >"$tmp/tree/src/unrelated.rs"

cd "$tmp"

files_of() {
  python3 -I -c '
import json, os, sys
path = sys.argv[1]
findings = json.load(open(path)) if os.path.exists(path) else []
print("\n".join(sorted({f["File"] for f in findings or []})))
' "$1"
}

(
  cd "$tmp/tree"
  gitleaks dir . --no-banner --redact --config ../.gitleaks.toml \
    --report-format json --report-path ../gitleaks.json >../gitleaks.log 2>&1 || true
  betterleaks dir . --no-banner --redact -c ../.betterleaks.toml \
    --report-format json --report-path ../betterleaks.json >../betterleaks.log 2>&1 || true
)
files_of gitleaks.json >gitleaks.files
files_of betterleaks.json >betterleaks.files

for f in "${fixtures[@]}"; do
  grep -qxF "$f" gitleaks.files || fail "gitleaks did not report a planted token in $f"
  grep -qxF "$f" betterleaks.files || fail "betterleaks did not report a planted token in $f"
done
grep -qxF "src/unrelated.rs" betterleaks.files || fail "betterleaks did not report a password in an unrelated file"

if grep -q "$token" gitleaks.log betterleaks.log; then
  fail "a scanner printed the planted token; --redact is missing"
fi
echo "secret-scan-config: OK"

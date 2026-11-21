#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
baseline="$root/.kingfisher-worktree-baseline.yml"
[ -f "$baseline" ] || { echo "secret-scan-worktree: baseline $baseline is missing" >&2; exit 2; }

exec kingfisher scan . --git-history none --confidence low --no-validate --redact --no-update-check \
  --skip-regex '^m-[0-9a-f]{16}$' \
  --baseline-file "$baseline" \
  --exclude '**/.git/**' \
  --exclude '.kingfisher-baseline.yml' \
  --exclude '.kingfisher-worktree-baseline.yml' \
  --exclude '**/crates/velnor-ci/tests/it/cache_credential_scrub.rs' \
  --exclude '**/crates/velnor-ci/tests/it/cache_private_repo.rs' \
  --exclude '**/crates/velnor-ci/tests/it/public_probe.rs' \
  --exclude '**/crates/velnor-ci/tests/it/audit/selection.rs' \
  --exclude '**/velnor-gen/tests/it/detect_private.rs'

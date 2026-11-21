#!/usr/bin/env bash
# Kingfisher over the working tree. It runs in the pre-push hook and in CI; full-history
# kingfisher stays a manual security-audit step because its findings vary between runs.
#
# Test files with fake credentials are skipped by path. The live evidence and lock files that
# hold published digests are scanned: their reviewed findings are listed one by one in
# .kingfisher-worktree-baseline.yml, so any new finding in those files still fails the scan.
# Run with bash: the hook calls `bash scripts/secret-scan-worktree.sh`, as CI does.
set -euo pipefail

# The baseline is repo-relative: resolve it from the repo root, so the scan works from any cwd.
# The target is still the cwd (scan .).
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

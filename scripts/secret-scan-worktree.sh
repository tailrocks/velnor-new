#!/usr/bin/env bash
# Kingfisher over the working tree. It runs in the pre-push hook and in CI; full-history
# kingfisher stays a manual security-audit step because its findings vary between runs.
set -euo pipefail

exec kingfisher scan . --git-history none --confidence low --no-validate --redact --no-update-check \
  --skip-regex '^m-[0-9a-f]{16}$' \
  --exclude '**/.git/**' \
  --exclude '.kingfisher-baseline.yml' \
  --exclude '**/crates/velnor-ci/tests/it/cache_credential_scrub*.rs' \
  --exclude '**/crates/velnor-ci/tests/it/cache_private_repo.rs' \
  --exclude '**/crates/velnor-ci/tests/it/public_probe.rs' \
  --exclude '**/crates/velnor-ci/tests/it/audit/selection.rs' \
  --exclude '**/crates/velnor-ci/tests/it/data/app_token/TEST-ONLY-*.pem' \
  --exclude '**/crates/velnor-runner-attestation/tests/fixtures/tough/snakeoil.pem' \
  --exclude '**/velnor-gen/tests/it/detect_private.rs' \
  --exclude 'dist/**/index.js' --exclude 'dist/index.js' \
  --exclude 'e2e/**' \
  --exclude '.velnor/runner.lock' \
  --exclude 'docs/implemented/evidence/freshness-2026-10-20/qualification/mise-crypto-admission.json' \
  --exclude 'docs/implemented/freshness-evidence-2026-10-08.md' \
  --exclude 'docs/reviews/ci-performance-owned-source-publication.json' \
  --exclude 'docs/reviews/uv-0.12.23-qualification-evidence.json'

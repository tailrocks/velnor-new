# T56 security gate: cache GitHub-only (GC1) — PASS

Verdict: PASS. No security finding blocks GC1 integration.

Scope: cache readers and writers only. Reviewed on-disk code: T52 bindings
(`crates/velnor-actions-orchestrator/tests/gc1_t52.rs`), T53-T55 fixtures and
path-include pins (`crates/velnor-actions-orchestrator/tests/gc1_runtime_cases.rs`,
`crates/velnor-actions-workflow-renderer/tests/gc1_workflow_cases.rs`), T41 delta
(`crates/velnor-actions-orchestrator/tests/gc1_t41.rs`,
`crates/velnor-actions-workflow-renderer/tests/gc1_t41.rs`), and the modules they
exercise (`cache_source_projection.rs`, `cache_recovery.rs`,
`cache_publication.rs`, `cache_cleanup_policy.rs`, `cache_keys.rs`,
`cache_steps*.rs`, `cache_prune.rs`, `cache_cleanup.rs`).

## Findings

Source projection trust: PASS. `cache_source_projection.rs` rejects absolute
paths and `ParentDir`/`CurDir` escapes with `projection_path_escape`. Symlink
targets outside the root fail with `projection_link_escape`. Mtime values are
range-checked before use.

Token least privilege: PASS. Generated workflows carry `permissions: {}` by
default (`cache_cleanup.rs`). The prune workflow adds only `actions: write` on
its own job, gated to protected refs, with a 5-minute timeout (`cache_prune.rs`).
No broad `contents: write` grant appears in cache steps.

Provenance checks: PASS. `cache_recovery.rs` verifies repository identity, key
match, and restored-bytes digest match. Mismatches yield named evidence
(`key-mismatch`, `ref-mismatch`, `version-mismatch`, `stored-bytes-mismatch`,
`restored-digest-mismatch`, `restored-size-mismatch`). `decide_save` halts saves
on interruption or service failure instead of publishing partial state.

Budget ceiling intact: PASS. Key bound `MAX_CACHE_KEY_BYTES = 512`
(`velnor-actions-contract/src/cachekey.rs`) is enforced at emission
(`cache_keys.rs`). Cleanup policy caps candidates at 8 and text at 512 bytes
(`cache_cleanup_policy.rs`). No change raises these limits.

No external stores, protocols, or deps: PASS. Cache traffic uses only pinned
`actions/cache/restore` and `actions/cache/save` SHAs. The single `https://`
match is an allowlist check on tool pins, not a fetch
(`cache_steps_tools_restore.rs`). No S3, GCS, Azure, or self-hosted endpoint
appears in cache readers or writers. No new dependency enters the cache path.

## Verification evidence (independently observed)

- `cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(gc1_runtime_)' --no-tests fail` → 2 passed, 51 skipped. Both `gc1_runtime_preserves_contract` and `gc1_runtime_rejects_invalid_state` PASS.
- `cargo nextest run --locked -p velnor-actions-workflow-renderer -E 'test(gc1_workflow_)' --no-tests fail` → 2 passed, 28 skipped. Both `gc1_workflow_preserves_contract` and `gc1_workflow_rejects_invalid_state` PASS.
- `git diff --check` → exit 0. No whitespace errors.

## Not covered

This gate did not review non-cache GC1 modules. It did not run a live cache
round-trip against GitHub. It did not audit the `actions/cache` action code
itself, only its pinned use. It did not review performance or cost effects.
Fixture data uses synthetic digests, not production cache contents.

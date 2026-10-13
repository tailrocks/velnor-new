# PR #12 added script disposition — 2026-10-04

This passive file records every added script path at PR #12 head
`2a2dab0e04a7da18975ca634e505eb9d72dc2e58` against base
`47815c83b9eeadbaf84b741918fffa7ea550da89`. The paginated GitHub Files API
snapshot at `2026-10-04T07:27:54Z` returned 280 unique changed paths; exactly
54 added `scripts/**` paths remain after excluding modified
`scripts/check-freshness.sh`. The source candidate is
`df0c4df8a12e9bc66c7356f414de4f6f1f751148`. It includes reviewed source
candidate `6ea09f2cb2beb595995bb2987b622791b0b7729d` plus the later nonce,
release-qualification, and same-repository PR policy slices.
The paginated Files API returned 100, 100, and 80 paths on pages 1–3. Page
ETags were `ca4e008788f87776e91d2af15ff03d6b0b28a108ef6f63f575184299d3270e7a`,
`4d7651cb44acca585e29942992f40e0b4823536d7876f28031a11876bea1ac21`, and
`afe12f654b9a208dd41d3510036abb123f90a031b4754af1bca48338061e5057`.
The newline-delimited, sorted 54-path inventory has SHA-256
`3ff3d8af864a0a3dafc2ec53ffb89f093871920fc0996331a7b5f0935b576e95`.
An exact tree search at candidate `df0c4df8a12e9bc66c7356f414de4f6f1f751148`
matched zero listed paths. The immutable 0ef-to-2a2 compare response records
only the latest delta, not this full path set.

## Disposition meanings and evidence

- `NOT-CARRIED`: exclude the proposed executable/source feature from Velnor.
  Its current-tree requirement/caller is absent, or the selected stock/API
  route replaces the proposed private mechanism. No semantic equivalence is
  claimed for an excluded private protocol.
- `ALREADY-ABSENT`: the path is test-only or fixture-only and was not carried
  into the candidate; no deletion from the candidate was performed.
- The current Rust mise isolation tests retain the required real-binary
  no-config contract. Official fixed-distribution adoption remains `PARTIAL`.
- The Rust Git caller retains `GIT_OPTIONAL_LOCKS=0` for discovery/status and
  explicit `git add` uses lock value `1`. This is not claimed equivalent to
  every test in the absent Python test file.
- The current MBX route uses stock MBX `1.22.0` and normal directory
  import/export. The private comparison-state/export-v2 files are not an
  equivalent transport. Hosted cache, corrupt-import, cancellation, parallel,
  disk/inode, and affected-consumer qualification remain separate partial
  obligations in `docs/implemented/cache-measurements.md`.
- Performance-analysis scripts and their pinned requirements are not adopted
  runtime or workflow behavior. Preserve passive historical evidence and the
  independent freshness, release, OCI, Git/process, runner, and workload
  support recorded in the main disposition.

## Exact 54-path inventory

| Added path | Disposition |
|---|---|
| `scripts/analyze-ci-performance.py` | `NOT-CARRIED` |
| `scripts/bootstrap-owned-tool-builder.py` | `NOT-CARRIED` |
| `scripts/build-owned-tool.py` | `NOT-CARRIED` |
| `scripts/ci-performance-analysis-requirements.txt` | `NOT-CARRIED` |
| `scripts/collect-ci-performance.py` | `NOT-CARRIED` |
| `scripts/download-owned-tool-candidate.py` | `NOT-CARRIED` |
| `scripts/immutable_publication.py` | `NOT-CARRIED` |
| `scripts/owned_mise_qualification.py` | `NOT-CARRIED` |
| `scripts/owned_tool_behavior.py` | `NOT-CARRIED` |
| `scripts/owned_tool_execution_test_fixtures.py` | `ALREADY-ABSENT` |
| `scripts/owned_tool_publication_test_fixtures.py` | `ALREADY-ABSENT` |
| `scripts/owned_tool_qualification_evidence.py` | `NOT-CARRIED` |
| `scripts/owned_tool_source.py` | `NOT-CARRIED` |
| `scripts/publish-owned-tool-artifacts.py` | `NOT-CARRIED` |
| `scripts/publish_owned_source.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/artifact_closure_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/bind_inputs.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/cache_transport_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/manifest.json` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/negative_inputs_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/registry-fixture/Cargo.lock` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/registry-fixture/Cargo.toml` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/registry-fixture/src/lib.rs` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/run.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_negative_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_same_root.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/test_negative_pipeline_v2.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_negative_v2.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_observer_artifact_closure.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_run_same_root.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_run_v2.py` | `ALREADY-ABSENT` |
| `scripts/qualify-owned-tool.py` | `NOT-CARRIED` |
| `scripts/source_proof_capsule.py` | `NOT-CARRIED` |
| `scripts/source_publication.py` | `NOT-CARRIED` |
| `scripts/source_publication_records.py` | `NOT-CARRIED` |
| `scripts/source_qualification_execution.py` | `NOT-CARRIED` |
| `scripts/source_release_policy.py` | `NOT-CARRIED` |
| `scripts/source_semver_capsule.py` | `NOT-CARRIED` |
| `scripts/stage-owned-tool-source.py` | `NOT-CARRIED` |
| `scripts/test_ci_performance_analysis.py` | `ALREADY-ABSENT` |
| `scripts/test_download_owned_tool_candidate.py` | `ALREADY-ABSENT` |
| `scripts/test_git_optional_locks.py` | `ALREADY-ABSENT` |
| `scripts/test_owned_tool_build.py` | `ALREADY-ABSENT` |
| `scripts/test_owned_tool_execution_evidence.py` | `ALREADY-ABSENT` |
| `scripts/test_owned_tool_qualification_evidence.py` | `ALREADY-ABSENT` |
| `scripts/test_publish_owned_tool_artifacts.py` | `ALREADY-ABSENT` |
| `scripts/test_qualify_owned_tool.py` | `ALREADY-ABSENT` |
| `scripts/test_source_proof_capsule.py` | `ALREADY-ABSENT` |
| `scripts/test_source_publication.py` | `ALREADY-ABSENT` |
| `scripts/test_source_publication_records.py` | `ALREADY-ABSENT` |
| `scripts/test_source_qualification_execution.py` | `ALREADY-ABSENT` |
| `scripts/test_source_release_transaction.py` | `ALREADY-ABSENT` |
| `scripts/test_source_semver_capsule.py` | `ALREADY-ABSENT` |

Counts: 29 `NOT-CARRIED`; 25 `ALREADY-ABSENT`; 54 total. There is no
path-level hold in this 54-path set. External adoption and hosted qualification
remain partial in their owning evidence ledgers.

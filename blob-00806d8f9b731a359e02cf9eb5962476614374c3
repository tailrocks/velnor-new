# PR #12 Python paths and 7c52 delta — 2026-10-04

## Snapshot

- PR #12: open, head `7c52bbda8ae9433228180c2ddc63af8513552a0b`, base `47815c83b9eeadbaf84b741918fffa7ea550da89`.
- GitHub Files API capture: 2026-10-04 09:55:29 UTC; 291 paths (167 added, 124 modified), 79 added `.py` paths. Pages contained 100, 100, and 91 paths.
- Raw paginated response SHA-256: `33c01f32631da1fe3e7bc1aaef3db4a446e0ced9c4a736a56a014d3c97255a04`.
- Durable sorted `status<TAB>path` inventory: [`pr-12-paths-7c52-2026-10-04.tsv`](pr-12-paths-7c52-2026-10-04.tsv), 291 lines, SHA-256 `31f6ed9f7ac5332d6222a785568da34f1315a2cc21b7427dbe7eb0659d2de468`.
- At the same cutoff, PR issue comments, reviews, and inline review comments were each empty. The PR remained open. CI run `37192429975` at the exact head failed `Alint`, `Plan`, and aggregate `Required`; Plan reports an unclassified `mbx-synchronous-registry-fixture`, and Alint reports its nested `Cargo.lock` and Rust fixture outside `crates/`. Treat these as PR fixture failures, not integration-candidate failures.

This record adds the current 79-path Python partition and the exact 2a2-to-7c52 delta. The earlier 54-script inventory remains historical at its recorded cutoff; it does not describe the 7c52 head.

## Current Python path partition

The 79 added Python paths have mutually exclusive path dispositions: 53
`NOT-CARRIED`, 23 `ALREADY-ABSENT`, 1 `CARRY REQUIRED`, 1 `CARRY TEST INTENT`,
and 1 `PARTIAL`. Six of the 53 `NOT-CARRIED` paths have behavior retained in
the registered shell gate; that is a separate behavior fact, not another path
disposition. The path counts sum to 79.

### Freshness — behavior carry required; implementation pending at the PR snapshot

The existing active owner is `scripts/check-freshness.sh` and its current ten-tool inventory. Preserve bounded upstream reads there: reject unsupported encodings, malformed/truncated gzip, and both encoded and decoded bodies above the configured cap before parsing. Do not add UV, PyPI, REUSE, or a new Python product tool to the current catalog. The modified shell file is outside the 79 added-Python list.

| Added path | Path disposition | Behavior disposition |
|---|---|---|
| `scripts/check_freshness.py` | `NOT-CARRIED` | Retained inline: `check-freshness.sh` is the registered entrypoint and invokes its validation body. |
| `scripts/freshness_context.py` | `NOT-CARRIED` | Retained inline: shell gate captures run time and parses evidence timestamps. |
| `scripts/freshness_dependencies.py` | `NOT-CARRIED` | No product behavior retained; do not add Python/UV/REUSE product dependencies. |
| `scripts/freshness_evidence.py` | `NOT-CARRIED` | Retained inline: shell gate emits structured pass/fail/info rows. |
| `scripts/freshness_inventory.py` | `NOT-CARRIED` | Retained inline: shell gate reads the current inventory and validates its structure. |
| `scripts/freshness_probe.py` | `CARRY REQUIRED` | Bounded HTTP/content-coding behavior must move into the existing gate; separate module is not adopted. |
| `scripts/freshness_probe_check.py` | `NOT-CARRIED` | Retained inline only for current catalog source formats; no PyPI parser or source is adopted. |
| `scripts/freshness_validation.py` | `NOT-CARRIED` | Retained inline: shell gate validates pins, policy, and evidence. |
| `scripts/test_freshness_probe.py` | `CARRY TEST INTENT` | Equivalent local HTTP regressions must be registered against the existing gate. |
| `scripts/test_freshness_probe_check.py` | `PARTIAL` | Retain cases for current GitHub, crates.io, and Rust sources; exclude unused PyPI/download-page behavior. |

The six inline behavior mappings are the rows marked `Retained inline` above. They match the registered implementation at `scripts/check-freshness.sh`: `check_freshness.py`'s entrypoint/arguments and embedded body (`22-104`); `freshness_context.py`'s clock, timestamp parser, and evidence age (`106`, `162-173`, `699-705`); `freshness_evidence.py`'s row emitters (`109-129`); `freshness_inventory.py`'s inventory load, schema, and tool rows (`176-211`, `298-368`); `freshness_probe_check.py`'s GitHub and crates.io current-catalog response parser (`967-1003`); and `freshness_validation.py`'s policy/pin/lock sections (`213-690`), upstream evidence checks (`691-770`), and exception validation (`771-859`). Bounded HTTP/content-coding behavior is tracked separately below.

### Freshness carry status after the PR snapshot

At pre-carry integration checkpoint `25d1eb3ea8fd4e1a90ec1723a7dc480439e97ca4`, `scripts/check-freshness.sh` had blob `b3f4ae20aa95bcc3af55d7db6da88ac080ecbffe` (unchanged from `571a649f9c2ad5d4f412bceb7163e2ef705e15de`) and the old `fetch_text` that read at most 512 KiB plus one byte, then parsed the prefix. Carry commits `69853fe92cb59479d97cd4316290578a47e525d7` and `148f73df05ddb7f43967d1f2c9d7f00b17cac0f8` integrated the bounded implementation and its edge tests. At integration head `148f73df05ddb7f43967d1f2c9d7f00b17cac0f8`, the script blob is `6784efb3c326fbb2921d51d538c658dd77af397a`; `response_encoding`, `read_bounded`, `decode_gzip`, and `fetch_text` (`910-964`) allow identity or one gzip encoding, reject malformed/truncated gzip and oversized encoded or decoded bodies, and read in bounded chunks. Eight registered HTTP tests passed, `scripts/check-freshness.sh` passed against the recorded 2026-10-04 inventory, and `cargo fmt --all -- --check` passed. The initial source review and exact Sol/medium review of the combined carry returned GO. These integration results do not change the PR-head path counts above; broader workspace gates and final exact-head CI remain open.

### OCI proposal — `NOT-CARRIED`; preserve unrelated image owners

The proposed OCI path introduces an unregistered generated-source execution and Docker Hub publishing route. It has no production caller or task registration. Keep the existing image-release workflow, runner and DinD images, tar shim, and nested runner. The OCI proposal is not a replacement for those obligations.

| Added path | Disposition |
|---|---|
| `crates/velnor-actions-native/src/oci/oci_archive.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_delivery.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_digest.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_digest_parts.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_index_receipt.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_platform_publish.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/src/oci/oci_registry.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_archive_test.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_index_receipt_test.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_platform_publish_test.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_registry_test.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_support_test.py` | `NOT-CARRIED` |
| `crates/velnor-actions-native/tests/oci/oci_transport_test.py` | `NOT-CARRIED` |

### Proposed release/source-proof capsules — `NOT-CARRIED`; no equivalence claim

No production registration or caller exists. `docs/proposed/release-contract.md` says “Proposed. No release behavior is implemented”; its proposed consumer-release scope is distinct from the implemented Velnor generator release. The active release workflow retains its own exact-source, attestation, and artifact qualification gates. The asset manifest is not claimed to reproduce Git-tree authentication, ZIP snapshot comparison, Cargo read containment, or admission semantics from these Python modules.

| Added path | Disposition |
|---|---|
| `crates/velnor-actions-orchestrator/src/release_admission.py` | `NOT-CARRIED`; its only product caller is the unregistered OCI proposal. |
| `crates/velnor-actions-orchestrator/src/release_source_snapshot.py` | `NOT-CARRIED`; proposed capsule only. |
| `crates/velnor-actions-orchestrator/src/release_source_tree.py` | `NOT-CARRIED`; proposed capsule only. |
| `crates/velnor-actions-orchestrator/tests/release_source_snapshot_test.py` | `ALREADY-ABSENT`; no registered test target. |
| `crates/velnor-actions-orchestrator/tests/release_source_tree_test.py` | `ALREADY-ABSENT`; no registered test target. |
| `crates/velnor-actions-rust/src/release_source_intent_guard.py` | `NOT-CARRIED`; proposed capsule only. |
| `crates/velnor-actions-rust/tests/release_source_intent_guard_test.py` | `ALREADY-ABSENT`; no registered test target. |

### Mise isolation — `PARTIAL`; official adoption separate

| Added path | Disposition |
|---|---|
| `scripts/owned_mise_qualification.py` | `NOT-CARRIED`; selected real-binary flag/environment and command behavior has Rust regression coverage in `impl_miserc_isolation.rs`. Private wrapper/HTTP cases are not claimed equivalent. Official fixed distribution adoption remains `PARTIAL`. |

### Git optional locks — scoped behavior transferred

| Added path | Disposition |
|---|---|
| `scripts/test_git_optional_locks.py` | `NOT-CARRIED`; retained Rust tests cover `GIT_OPTIONAL_LOCKS=0` for status and explicit task writes. This does not claim equivalence to every Python staging/clone case. |

### Performance analysis — `NOT-CARRIED` optional research tooling

| Added path | Disposition |
|---|---|
| `scripts/analyze-ci-performance.py` | `NOT-CARRIED`; no runtime registration. |
| `scripts/collect-ci-performance.py` | `NOT-CARRIED`; no workflow registration. |
| `scripts/test_ci_performance_analysis.py` | `ALREADY-ABSENT`; preserve historical results, but no registered test target. |

### Private build and publication protocol — `NOT-CARRIED`

These paths implement the retired private owned-source builder, qualification, transaction, and publication route. They do not register the Velnor generator workflow. Historical archive proof remains passive evidence; this disposition does not authorize snapshot-ref deletion.

| Added path | Disposition |
|---|---|
| `scripts/bootstrap-owned-tool-builder.py` | `NOT-CARRIED` |
| `scripts/build-owned-tool.py` | `NOT-CARRIED` |
| `scripts/download-owned-tool-candidate.py` | `NOT-CARRIED` |
| `scripts/immutable_publication.py` | `NOT-CARRIED` |
| `scripts/owned_tool_behavior.py` | `NOT-CARRIED` |
| `scripts/owned_tool_execution_test_fixtures.py` | `ALREADY-ABSENT` |
| `scripts/owned_tool_publication_test_fixtures.py` | `ALREADY-ABSENT` |
| `scripts/owned_tool_qualification_evidence.py` | `NOT-CARRIED` |
| `scripts/owned_tool_source.py` | `NOT-CARRIED` |
| `scripts/publish-owned-tool-artifacts.py` | `NOT-CARRIED` |
| `scripts/publish_owned_source.py` | `NOT-CARRIED` |
| `scripts/qualify-owned-tool.py` | `NOT-CARRIED` |
| `scripts/source_proof_capsule.py` | `NOT-CARRIED` |
| `scripts/source_publication.py` | `NOT-CARRIED` |
| `scripts/source_publication_records.py` | `NOT-CARRIED` |
| `scripts/source_qualification_execution.py` | `NOT-CARRIED` |
| `scripts/source_release_policy.py` | `NOT-CARRIED` |
| `scripts/source_semver_capsule.py` | `NOT-CARRIED` |
| `scripts/stage-owned-tool-source.py` | `NOT-CARRIED` |
| `scripts/test_download_owned_tool_candidate.py` | `ALREADY-ABSENT` |
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

### MBX comparison protocol — `NOT-CARRIED`; ordinary cache qualification remains separate

These paths implement the private synchronous comparison harness and artifact transport. They do not replace the stock cache route. Hosted reader/writer, corruption fallback, cancellation, parallel writers, disk/inode pressure, and affected workload results remain tracked separately in `docs/implemented/cache-measurements.md`.

| Added path | Disposition |
|---|---|
| `scripts/qualification/mbx-synchronous/artifact_closure_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/bind_inputs.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/cache_transport_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/negative_inputs_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_negative_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_same_root.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/run_v2.py` | `NOT-CARRIED` |
| `scripts/qualification/mbx-synchronous/test_negative_pipeline_v2.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_negative_v2.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_observer_artifact_closure.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_run_same_root.py` | `ALREADY-ABSENT` |
| `scripts/qualification/mbx-synchronous/test_run_v2.py` | `ALREADY-ABSENT` |

### Exact 2a2-to-7c52 delta (17 paths)

| Path | Disposition |
|---|---|
| `.velnor/freshness-inventory.json` | `NOT-CARRIED` for added UV tool; current ten-tool inventory stays authoritative. |
| `.velnor/version-policy.toml` | `NOT-CARRIED` for UV pin; preserve current catalog scope. |
| `crates/velnor-actions-mise/src/catalog.rs` | `NOT-CARRIED` for unused UV constant. |
| `crates/velnor-actions-mise/tests/impl_mise_catalog.rs` | `NOT-CARRIED` for UV catalog row. |
| `docs/reviews/ci-performance-completion-ledger.md` | `PARTIAL`; passive review evidence only, no runtime adoption. |
| `docs/reviews/uv-0.12.23-qualification-evidence.json` | `NOT-CARRIED`; no current UV tool or gate. |
| `scripts/check-freshness.sh` | `CARRY REQUIRED`; existing fetch path needs gzip handling and independent encoded/decoded size bounds. |
| The ten `scripts/*.py` paths listed in the Freshness section | Use each path's individual disposition above; the three carry/test rows remain open. No separate Python freshness product is adopted. |

The latest delta is recorded by immutable GitHub compare response `2a2dab0e...7c52bbda`; the complete PR path set above is the paginated Files API capture, not a subtraction-only guess. Do not count this document as proof the pending freshness behavior has landed.

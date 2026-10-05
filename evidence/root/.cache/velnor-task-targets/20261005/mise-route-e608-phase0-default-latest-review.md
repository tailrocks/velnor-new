# Phase0 and default-latest Mise source slice

This is a frozen source-only review packet. It is an isolated derived tree, not the shared checkout, a Git worktree, or generated-CI evidence.

## Source identity

- Immutable source base: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-20261005`, commit `e608f82039375a4c2a839d5229e1d2dd3035617b`.
- Base regular-file manifest: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-base-source-files.sha256`, SHA-256 `4604b2da09e37e0b9e2775551efa97ec0d1c754d4bcec16f07baf2f486ca8f12`.
- Joined candidate: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-joined-103`.
- Joined regular-file manifest: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-joined-source-files.sha256`, SHA-256 `7f30cd7d13fc11771d71d2dff2b5d17c13d0610c84be8edd770d433f1a47443a` (1,531 entries). Manifest verification log: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-joined-103-manifest-verify.log`; `sha256sum -c` succeeded for every entry.
- No Git/index operations were performed and no generated `.github` outputs were edited or copied into a shared tree.

## Main source postimages

| Path | SHA-256 |
|---|---|
| `.mise-version` | `2d9dadb93c01d677d76053d03b3a147c150c727dc839ab27bfd8efe76795a113` |
| `mise.toml` | `446f3e6556206e5567b9e3b467cc100ff97920a6223824252e7dd6257c326067` |
| `.mise/tasks/mise-phase0` | `2582296892daf802eeae6785f1cd775de259e42a4de40b79aa00f3c7ed6082a7` |
| `crates/velnor-actions-mise/src/catalog.rs` | `dd922305bf19b0e89e1feda3ed62f8e6859419e02d5e0acc2228460092053e3f` |
| `crates/velnor-actions-actionlint/src/actions.rs` | `bc20c7f19e5e9f7baec157c9cadceaab51b523a02696e856efce59d0973a4fbb` |
| `crates/velnor-actions-mise/src/generator_release_workflow.rs` | `c36519c21693c4f54124e3c051885852b2aef97f8f0d259271e219bf4bc3aac4` |
| `crates/velnor-actions-mise/tests/impl_mise_catalog.rs` | `a5ebec7b8d3f9d982fcd2e021b2f383b10eb51fa32157c2c8a9ff560b36deb91` |
| `crates/velnor-actions-mise/tests/impl_mise_policy.rs` | `55cc676861abb2375581fb27799d533231ae4fabc0546e73dec2ff64c9d91a20` |
| `crates/velnor-actions-mise/tests/impl_mise_generator_release_workflow.rs` | `1ae93b30c1b7b729664bb68aa390366039c6f546f9f6edb52a35fc72415ab51a` |
| `crates/velnor-actions-mise/tests/impl_miserc_isolation.rs` | `fc51a3b161afa35faa12fa6630879e8fc37e741da713c650dfa50b7e74ce67d2` |
| `crates/velnor-actions-orchestrator/src/pins.rs` | `52bae4cece02dc6532ea9067849f3537914258c38ed386e31a20e78cda662746` |
| `crates/velnor-actions-orchestrator/src/pins_tests.rs` | `240f4a2a84492940ac6340835a7753c4c2151101a94bdcbb61c578788d8c33dc` |
| `crates/velnor-actions-orchestrator/src/generate.rs` | `f71989a96d12782f4c91b11a14a6aa8d4b90eab165bceb2aa300cc67c081001e` |
| `crates/velnor-actions-orchestrator/src/workflow_jobs_mise_phase0.rs` | `4b7b5c04f04eecc7d959ac1c9df8087c102d31a0dffbcb50c2c8ac815fa1dd6b` |
| `crates/velnor-actions-orchestrator/src/workflow_jobs_mise_phase0_tests.rs` | `0a0cf4d5567bf7ca3ba88550b3eae234fa7e5db655add70888c34c2918be9c05` |
| `crates/velnor-actions-orchestrator/src/workflow_mise_phase0_tests.rs` | `0c9cb76eed359acae2f2193a949b6a3b45b27878adf05524cf945aca0c7ce361` |
| `.velnor/version-policy.toml` | `d371b134f03b2b5c3c080de7c61a6b8ec3fb9e3b14aaf5ef2cfc9c13471c5a85` |
| `.velnor/freshness-inventory.json` | `28c03834b003aec319182814a86f886fb673e002d6ce215e4e6fb3acb5a00590` |

Related renderer target/API files are bound by the joined source manifest; their individual hashes are in the candidate manifest. The candidate contains 10.3 typed setup selection for the three Phase0 jobs and ordinary/default 10.3 setup resolution. The 1.98.1 compatibility check is a distinct Linux Phase0 step, not the default compiler and not the Cargo.toml MSRV declaration.

## Required emitter test evidence

Command:

```sh
CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/root/.cache/velnor-task-targets/20261005/mise-route-e608-joined-target mise --no-config --no-env --no-hooks exec rust@1.99.0 -- cargo test --locked --offline --jobs 1 -p velnor-actions-orchestrator --lib mise_phase0
```

Raw log: `/root/.cache/velnor-task-targets/20261005/mise-route-e608-joined-103-phase0-test-final.log`, SHA-256 `bc514165de6a50fd7b07fb2c423c06e1f265bea03c2e0fa9738dc0a265e29c7d`. Result: 5 passed, 0 failed, 0 ignored, 487 filtered. It covers the three-platform job emitter, transactional duplicate rejection, Linux-only separate Rust 1.98.1 locked/offline workspace check, default selected toolchain, and Required fan-in.

Independent fixed-binary evidence: `/root/.cache/velnor-task-targets/20261005/no_python_policy-103-evidence/real-linux-rust199-mise103.log`, SHA-256 `0123b545d4719cc3f7a3befb8df5f754ccb7af6531d00ca061315651b14f348b` (four official Mise isolation tests pass on Linux under Rust 1.99.0). The separate local Rust 1.98.1 package check log is `/root/.cache/velnor-task-targets/20261005/no_python_policy-103-evidence/rust-1.98.1-mise-msrv-check.log`, SHA-256 `d85d2c5e298046a8b1322d4b44c9a93146da96be75734c2d8c235db64e9e2d34`; it is not proof of hosted execution of the newly emitted full-workspace compatibility step.

## Verified identity and limits

The official Mise v2026.10.3 release metadata capture is `/root/.cache/velnor-task-targets/20261005/no_python_policy-103-evidence/official-release-assets.json`, SHA-256 `8b87c84a9e7041a9ed9cc33670c5e78f857c1c89e2ac005b4665f442813f4336`. Raw executable SHA-256 values are Linux x64 `8d48bc510b7d844fad0bc7156c0855c2a1e7230c51e498a7f6b63b021f8b57c5`, macOS ARM64 `0999bd4943523ccdbd149b5ad4080281536801b3f6406ac4495699bd0310969c`, and macOS x64 `38cdd1d904b25e66ec2fff3b5c88930853ab929b63e72c70e77198f82d6ac144`. Only the Linux executable has been run in the isolation regression; both macOS platforms remain unexecuted.

MBX v1.22.0 Linux asset identity is release metadata/archive SHA-256 `eb3e8c74277dcc8314227ba2cd32ef00a9a362ec8a90cd06ef6580eb48a6b375`, with the separately verified Linux executable SHA-256 `124a1c7d856c2355e1a53b1e9bd04342d952eb6845b7edf7ac81d0eb1766f296`. The ARM archive digest is official release metadata `e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2`; no Intel MBX asset is claimed. MBX action v1.7.1 is pinned to commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` with tracked dist raw SHA-256 `0785ff9249529187d58abc12bb603e3e80965c122d031f37d46ce547d4aa58d1`. Mise action v5.1.1 is pinned to commit `2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca`.

No macOS hosted run, deployed release adoption, actual full-workspace Rust 1.98.1 CI run, or generated-workflow capture is claimed by this packet. Current `.velnor/version-policy.toml` and inventory hashes above are the candidate preimage for the separate policy/inventory owner’s ongoing jactionlint/readiness derivative; that derivative must supersede those two entries before any final joined manifest.

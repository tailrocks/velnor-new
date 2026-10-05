# B lane: MBX GC policy caller handoff

Status: renderer policy and focused coverage are implemented and pass. The resulting selector puts job-level `MBX_GC_AUTO=0` on every typed Hosted or Scale Set job containing the MBX action. The same selected job set continues to receive `MBX_SHARE_OUT_DIR=0`.

## Candidate identity and ownership

The joined private B candidate is `/root/.cache/velnor-task-targets/20261005/mbx-cli-portability-20261005T1530Z`. Its 15:30 source/copy file inventory manifests are identical: `source.files` and `copy.files` SHA-256 `7c1e4acd80411dd02d8f4f942d82b4b341903aa0686065969231688ae9e948dc`; the corresponding directory manifests both hash to `25a626c2282e936d85e3e1753474a18081ba0361ed392f7f9b60a135ce8a440b`, and symlink manifests both hash to `d590b8086b50e1bcb2ee0363d2c43a798b63a4b24996b86ac4e4c012e6d79c02`. These inventory files are under `/root/.cache/velnor-task-targets/20261005/mbx-cli-portability-evidence-20261005T1530Z/`. The candidate is a private source copy without Git metadata; these inventory hashes bind the source/copy inputs, and no commit SHA is claimed for the copy.

Only the GC policy selector, its caller, and its renderer tests changed in this lane. The stock bundle implementation owns `cache_steps_mbx_preflight.rs`; its current SHA-256 is `32a9f3ffca650c60d79c48756ff9d071863aab12730bbd6ac69bd0f3e63e9492`, and it includes the `gc-auto-disabled-v1` cache-generation segment. I did not edit that file or the separate A compaction clone.

## Behavior and tests

`mbx_gc_policy.rs` selects typed Hosted and Scale Set jobs with MBX action steps. `render.rs` passes that same set to the existing generic document renderer for both GC and shared-`OUT_DIR` policy. `document.rs` is unchanged: its existing job-level env insertion already writes both flags when the selection booleans are true.

The selector unit test covers Ubuntu Linux, both `macos-15` and `macos-15-intel`, and a Scale Set token; it verifies a Cargo-only macOS job remains excluded. The rendered workflow integration test verifies both Hosted Ubuntu and Scale Set MBX jobs emit `MBX_GC_AUTO: "0"`, retain `MBX_SHARE_OUT_DIR: "0"`, and keep local export behavior. It also checks that a Cargo-only job receives neither policy.

Rust formatting check passed. The focused test command exited 0:

```sh
cargo test --locked -p velnor-actions-workflow-renderer mbx_gc_policy
```

It ran one policy unit test and two renderer integration tests, all passing. The build used Rust 1.98.1, one job, and private target directory `/root/.cache/velnor-task-targets/20261005/mbx-cli-portability-gc-policy-target`. Raw log `policy-test-pass.log` SHA-256: `bae195e2c1e46884be9a294923aea8cff79c8b51938a28890d6cef75f275c9da`.

Two early test attempts are retained as harness diagnostics: `focused-test.log` SHA-256 `9094a18b0cfb5f24b8e7778c46ca78f1d839c806d4d25fc6a21be3c59935c58a` and `policy-test.log` SHA-256 `c7965d509396c7afea67a823f28281d4f27637e53dc87d25cb7ab5a0293a52d6`. Their temporary integration fixture used synthetic macOS jobs under a single-label renderer context, then an incorrect Ubuntu label expectation. Both were replaced by the passing selector-level macOS coverage and corrected Linux/Scale Set renderer assertions above.

## Exact file images and diff sizes

Frozen preimages, postimages, and raw `diff -u` patches are under this evidence directory. Hashes are SHA-256.

| Path | Preimage | Postimage | Diff hunks; added/removed lines |
|---|---|---|---:|
| `crates/velnor-actions-workflow-renderer/src/mbx_gc_policy.rs` | `096d27e4fc068a0e0b112af8afaf05a755722e85ea9969f503066e454d181632` | `7d99dcdc7b900edd7dbcd45cdd79c7091a56680f785ba44c460776730f6a7c92` | 3; 59/29 |
| `crates/velnor-actions-workflow-renderer/src/render.rs` | `62e1df9d7089ddbbfda3adc85b2c11f5e763ffc4762aa67ff5496364f6e04812` | `1f5c82d330da59c136a867c5e4d32661823ff657458e91ac11593c6471e17083` | 1; 2/4 |
| `crates/velnor-actions-workflow-renderer/tests/impl_renderer_mbx_gc_policy.rs` | `f16674f916744619758835302ad54089882e5590eec8d39f2703e11084af39e0` | `93b280db335657156c1e2b30169869c8a4bd632bae3cfc0137b8b982a91ef244` | 3; 4/4 |
| `crates/velnor-actions-workflow-renderer/src/document.rs` | `3ecbd0ae95615647de91acf3f827dc5a03e32d05c219bcb26857a7bce0ddbb1f` | `3ecbd0ae95615647de91acf3f827dc5a03e32d05c219bcb26857a7bce0ddbb1f` | 0; 0/0 |

The stock owner separately owns the `impl_schema2_mbx_qualification.rs` assertion that currently expects `MBX_GC_AUTO: "1"`; they were asked to change it to `"0"` alongside their active production action-constant migration to avoid overlapping edits. This report does not claim that external test hunk is complete. Catalog/action pin integration and combined key tests remain with their owners; the passing renderer test is not final MBX 1.22.0 or 1.7.1 qualification evidence.

No Git command or Python command was launched by this lane.

## Current follow-up snapshot (2026-10-05 17:39 UTC)

Stock has now changed the separate orchestrator qualification test so both its MBX writer and reader assert `MBX_GC_AUTO: "0"`. Current observed SHA-256 for `crates/velnor-actions-orchestrator/tests/impl_schema2_mbx_qualification.rs` is `f621d45b8ecf67f1e4b6ed4108c31f4afcf48d54a90b57f8bc67e7c353e01086`; stock owns that hunk and is still completing its final test/manifest, so this is an observed intermediate hash rather than its final attestation.

The renderer source/test files pass `rustfmt --edition 2024 --check` (`rustfmt-check.log`, SHA-256 `e092350a1a5f56eeb9333fd657fe2dd03c6b46d61c0aee9e23e8add626d684af`). Narrow `cargo clippy --locked -p velnor-actions-workflow-renderer --lib --test velnor_workflow_renderer -- -D warnings` passed (`clippy-product-and-integration.log`, SHA-256 `a13905646e2aed93ec6ea9ed6ac91490a4e01f91af0215c3965279a910612149`). The full package `--all-targets -- -D warnings` attempt did not pass: it stopped on the stock-owned existing test helper `crates/velnor-actions-workflow-renderer/src/stock_mbx_bundle_cli_failure_tests.rs:5`, whose `generated_import_rejects_noncanonical_private_paths` function is 87 lines against the 80-line limit (`clippy.log`, SHA-256 `cc2714874cf89118887714cdf4ca48a2666fe239b0d87b43c17d0d034ccd7985`). The stock owner was notified to split that function before a clean all-targets Clippy run.

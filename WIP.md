# WIP

## Native build-task artifacts forward-port

- Owner: Integration agent `/root/common_packaging_implementation`, session `cb840dd5-b4b4-42a9-a45a-b9384c14b186`.
- Coordinator: `/root`, thread `01a16321-86f7-7512-84a2-53f83ae01732`.
- Branch: `codex/native-build-task-artifacts-20261028-cb840dd5`, based on `main` at `4afd0a059ce19d03a3e88f5cdc03e7e533c20218` (v0.1.63).
- Source and document scope is exactly these seven paths; `WIP.md` is a separate coordination-only record:
  - `crates/velnor-actions-contract/src/config/build_task.rs`
  - `crates/velnor-actions-contract/src/config/rust_suites_validate.rs`
  - `crates/velnor-actions-workflow-renderer/src/build_task_jobs.rs`
  - `crates/velnor-actions-workflow-renderer/src/steps.rs`
  - `crates/velnor-actions-workflow-renderer/src/steps_artifact.rs`
  - `crates/velnor-actions-workflow-renderer/src/native_authority/inputs/graph.rs`
  - `docs/proposed/tooling-input-contract.md`
- The five-file artifact patch is `0cc871199bcf5d8c5a9fefb8ff18ee6a38d487b1599b235a491a8f223cf46805`; the graph patch is `929c6e484ba1e4e2dfc87c85ec157efd3f795be61e00f46e144f1c2433f298bd`; the contract-document patch is `d2ea87bb8ab4c331bae1e216e77b3f7825f067ad75a9481560e6b8f30abbb814`. The seven postimage hashes are recorded in the Integration handoff and must match before publication.
- The build-task artifact contract accepts literal names, one exact path, retention from 1 through 90 days, and `failure`, `success`, or `always` conditions. Generated uploads run after the declared task, fail when the file is missing, and remain in the native task's `Required` job. RustSuite's existing warn-on-missing behavior is unchanged. Generated YAML is never edited by hand.
- The runner-authority change admits the registered workflow-task source before applying runner validation, then matches exactly one task or native-image owner and runner to the observed workflow-task job. Named checks and unrelated runner guards stay unchanged; the candidate does not weaken unknown-source or Required checks.
- Independent Sol-high review accepted the source and the native-only public generation fixture on the earlier proof base `422ce5aab350ef8a12e7051f91f75d87be7e72a4`. That fixture is not combined native-plus-RustSuite proof. Current-main compilation and generation have not been run on this branch, and no current-main hosted upload, artifact-retention run, or Required-gate acceptance is claimed.
- The earlier CLI binary `575a111ecda585f0f662a58c8385ba32494d2b21633a0c3206ba2cb9593feee6` is absent from its recorded MBX target and local target directory; it is not evidence for this branch. Any current-main build must use the guarded pinned toolchain after a fresh disk, provider, and compiler-capacity check.
- Keep this branch isolated from the dirty Termrock worktree. Do not rewrite history or force-push. After reviewed integration into `main`, independently verify the integrated result and full recovery checks, record the source-to-integration commit map and recovery copy, and only then delete this temporary branch. Do not merge or delete the branches for PR #26 or #38, and do not change PR #391. Send source, build, and generated-output evidence for independent review before publication.

# G0 merge-state preservation snapshot (WIP)

This records an unresolved generator-release merge solely for recovery. It is not a resolved merge, release candidate, test result, or product qualification.

- Worktree HEAD: `8c31864bdc80439d03dfcbf1f98bc6f2816b09fa` (tree `b9020a2af31d21e4d82e99e3c7c2a563acf2dd87`)
- In-progress `MERGE_HEAD`: `71494f442b4e8a2419a8e9b5e29ba86fbd77afad`; its source commit is preserved by the published branch `origin/codex/g0-generator-publication`.
- Snapshot status: 61 changed/unmerged status records and 83 conflict index-stage records.
- The original index bytes, `MERGE_HEAD` file, NUL status, NUL path list, and unmerged-stage listing were copied to `/private/tmp/g0-conflict-state-20261006/` before snapshot creation.
- SHA-256: original index `9870d8fb1ab1a340c5ccecb033c768c7e0b16708dff10ca425a1672bce56f076`; merge-head file `66729c8216f202376cefd7a8eaca60678ec2f570786b5f7b4f645a350262782b`; NUL status `e55e2b5b8eb31c38ff3bb763ac65a2a290101e7bf0b8c630960e278ce960b16e`; NUL stage listing `50379ec72eb5961db03d160885f0114c13b9ea4ff29fda93bb2315c743224eb0`.
- No command changed the live merge index or aborted the merge. This preservation commit has one parent (`HEAD`) and intentionally does not represent a merge resolution.
- All 83 index-stage blobs are reachable in the preserved `HEAD`/`MERGE_HEAD` source histories.

## Unmerged index stages

`100644 935e9e5e0e6bc4c8202fb0dae023210985a98e32 stage 1` — `.github/actions/generator-release-attest-macos-intel/action.yml`
`100644 aa135b7d6fc6f12a3a0c967c7f0dcfde4fffe772 stage 2` — `.github/actions/generator-release-attest-macos-intel/action.yml`
`100644 e7de663db23648a2a1aa9571ec17b9e4f4c2b0b9 stage 1` — `.github/actions/generator-release-attest-manifest/action.yml`
`100644 81f5fed5646e7f1a5019a483322a5e47ecdcac3a stage 2` — `.github/actions/generator-release-attest-manifest/action.yml`
`100644 1bc1fdd42cc25310ed290e6d87badaf5af5019c2 stage 3` — `.github/actions/generator-release-attest-manifest/action.yml`
`100644 b753f9e872c14c85b31f18c68b8cf6b768751384 stage 1` — `.github/actions/generator-release-publish/action.yml`
`100644 fc8c6025128773d7614bfafab2ff32341faf1431 stage 2` — `.github/actions/generator-release-publish/action.yml`
`100644 ada95b30f2c9b10f664de36d9087c4d714bc25a3 stage 3` — `.github/actions/generator-release-publish/action.yml`
`100644 0ccba040522b463afa704652cdb388e0af1afaaf stage 1` — `.github/actions/generator-release-qualify-macos-intel/action.yml`
`100644 1efb46c157eb1ef14adbfecd6524c60c046fabd5 stage 2` — `.github/actions/generator-release-qualify-macos-intel/action.yml`
`100644 53d434e59ab6007a53a92a05f6a7b1658506757f stage 1` — `.github/workflows/generator-release.yml`
`100644 c30c686fa10814b02db5ceffa93603ddf06bf8eb stage 2` — `.github/workflows/generator-release.yml`
`100644 3335e19925bd1611f08560aa6647c31fb0f28bb9 stage 3` — `.github/workflows/generator-release.yml`
`100644 fe2433ac2f4f231b8c4043efa30845050fe43347 stage 1` — `crates/velnor-actions-contract/src/targets.rs`
`100644 6c3bb72c614a665bdc27fa708ce8cc85514d6e5c stage 2` — `crates/velnor-actions-contract/src/targets.rs`
`100644 90870c92db50fbcf85e9c5c07e1634e30cd9b266 stage 3` — `crates/velnor-actions-contract/src/targets.rs`
`100644 a8a400fc758a3998dbf5250aee31b7b58820e374 stage 1` — `crates/velnor-actions-contract/tests/impl_contract_targets.rs`
`100644 e9770228eeebb64e230917bcee26f2d0b0ed025b stage 2` — `crates/velnor-actions-contract/tests/impl_contract_targets.rs`
`100644 65d70800d0a6f1b80f0c22a331af85de47bb683b stage 3` — `crates/velnor-actions-contract/tests/impl_contract_targets.rs`
`100644 f0f0a2913030aa210054ca002752b8d0cd031241 stage 1` — `crates/velnor-actions-orchestrator/src/pins.rs`
`100644 0224633422ce953ec5e76b576a9b562f7da92178 stage 2` — `crates/velnor-actions-orchestrator/src/pins.rs`
`100644 de71f3b5074b941ea902263a37138a1fa8a36582 stage 3` — `crates/velnor-actions-orchestrator/src/pins.rs`
`100644 1905eb8955c04d7f786d8bbbcc514cd59beedda4 stage 1` — `crates/velnor-actions-orchestrator/tests/impl_consumer_manifest_file.rs`
`100644 d728b8d6c338aa98559c411bb4ff39b77d690f31 stage 2` — `crates/velnor-actions-orchestrator/tests/impl_consumer_manifest_file.rs`
`100644 f23c77e4dc2653c0a57b050cb1f2f1675bfd1ab0 stage 3` — `crates/velnor-actions-orchestrator/tests/impl_consumer_manifest_file.rs`
`100644 4b468333498ea19e0e83ce0c904fff74d30287c3 stage 1` — `crates/velnor-actions-orchestrator/tests/schema2_generator_release_publication_snapshots.rs`
`100644 7e4bbc2941d9f2fd9fa3b0ca607cc90b3cfe9546 stage 2` — `crates/velnor-actions-orchestrator/tests/schema2_generator_release_publication_snapshots.rs`
`100644 2affc05b0df49bb888ab19cab12b9f8e661e6c27 stage 3` — `crates/velnor-actions-orchestrator/tests/schema2_generator_release_publication_snapshots.rs`
`100644 8fe2249fb0598abb6c2b9790ac3dbf0d7c71f568 stage 1` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-attest-macos-intel.yml`
`100644 b5b886880768160476c39a4ba94c334163c59161 stage 2` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-attest-macos-intel.yml`
`100644 c3128c71b4e2e4d5d3f3525dde7bf6b63954057c stage 1` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-attest-manifest.yml`
`100644 cce326e20acbadc11edfbe6a3b2f88e0910eda97 stage 2` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-attest-manifest.yml`
`100644 24034e9c356330b64f4ffe0928ba8508a151e721 stage 3` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-attest-manifest.yml`
`100644 cba83b6a4647d0e3ed85f47bc6887884cc3a1691 stage 1` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-publish.yml`
`100644 e1dd64aaedc9297a02aa15ccd0b2337c6c2ba85e stage 2` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-publish.yml`
`100644 6013aa869bcb1cda6f0476fa3a620e165a6baa7d stage 3` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-publish.yml`
`100644 fb030fae2c81403917838e4d7b27ef36c6a7eb0f stage 1` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-qualify-macos-intel.yml`
`100644 60857ff497fca439f64def395ea9630d292df390 stage 2` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release-qualify-macos-intel.yml`
`100644 94a102070e9cbac0c6047b0a0f3c9d9d4eaa750f stage 1` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release.yml`
`100644 17db05db70676dc692b1a93dd635ddaa2c41e559 stage 2` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release.yml`
`100644 d2fdbb4bc1fe75d6e6d6dd812b39ec9e1ce04a08 stage 3` — `crates/velnor-actions-orchestrator/tests/snapshots/generator-release.yml`
`100644 b85452aafbf5379b2b8e3f6528be952ea52e4715 stage 1` — `crates/velnor-actions-workflow-renderer/src/lane_share_tests.rs`
`100644 0cea6d11d7f42b6603cf4fb2398e47c320273d1a stage 2` — `crates/velnor-actions-workflow-renderer/src/lane_share_tests.rs`
`100644 ce5207bebbfd0bb67085e5fab21747a0386781d6 stage 3` — `crates/velnor-actions-workflow-renderer/src/lane_share_tests.rs`
`100644 2295be60daab441ebc80d2cb29837889a68fc078 stage 1` — `crates/velnor-actions-workflow-renderer/src/schema2.rs`
`100644 eef3f8dfdc913de72e9a149f22d354e30f551d45 stage 2` — `crates/velnor-actions-workflow-renderer/src/schema2.rs`
`100644 71e6a5c56151efed4088e735fb17e9da3d7affd2 stage 3` — `crates/velnor-actions-workflow-renderer/src/schema2.rs`
`100644 3f748b3265ecdd7a2e14ae4ab050fcd56e2abd3b stage 1` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release.rs`
`100644 2fee313ac817f70a0b17d3cfd6d0540e0ea4da40 stage 2` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release.rs`
`100644 2789a9576ed6037053f3d80186aa25a61d289be7 stage 3` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release.rs`
`100644 7b17983d35e9b96532e9600b8726db6d53e0e2cb stage 1` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_candidate_manifest.rs`
`100644 3a0241654dd339cdbe969d172da02bdbb6babbf9 stage 2` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_candidate_manifest.rs`
`100644 8394225109c71f281655bffe9ac402c889fc6186 stage 3` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_candidate_manifest.rs`
`100644 0c7d086ea4133c5a00fc889943d41bf479eb5382 stage 1` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_source_tests.rs`
`100644 e6126efeccfb95d4bf9964bdae01c56bb9dc8995 stage 2` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_source_tests.rs`
`100644 30cf629f157ad8d2ee72c4d80fda5e0f54f368d8 stage 3` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_source_tests.rs`
`100644 3deb65d5fb8c72d98f1b63e5aa9dfd9db47591e4 stage 1` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_test_pins.rs`
`100644 6495550ad513057e606aae2d04abaf3210bfb6c8 stage 2` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_test_pins.rs`
`100644 38b5c92e5de89c839618849482510eb615ff39e2 stage 3` — `crates/velnor-actions-workflow-renderer/src/schema2_generator_release_test_pins.rs`
`100644 b88d9301fe8bdebb06483e64f0b2c5bd1ed3ce19 stage 1` — `docs/proposed/opentofu-goldens/MANIFEST.sha256`
`100644 f1c0bfb4e82fa1c298e74bdb25976c9eafe22189 stage 2` — `docs/proposed/opentofu-goldens/MANIFEST.sha256`
`100644 e5e3de8ad359f882f05dd358e48d1d72d9520637 stage 3` — `docs/proposed/opentofu-goldens/MANIFEST.sha256`
`100644 935e9e5e0e6bc4c8202fb0dae023210985a98e32 stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-attest-macos-intel/action.yml`
`100644 aa135b7d6fc6f12a3a0c967c7f0dcfde4fffe772 stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-attest-macos-intel/action.yml`
`100644 e7de663db23648a2a1aa9571ec17b9e4f4c2b0b9 stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-attest-manifest/action.yml`
`100644 81f5fed5646e7f1a5019a483322a5e47ecdcac3a stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-attest-manifest/action.yml`
`100644 b753f9e872c14c85b31f18c68b8cf6b768751384 stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-publish/action.yml`
`100644 fc8c6025128773d7614bfafab2ff32341faf1431 stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-publish/action.yml`
`100644 d0b1c869823bbe359f05ca46d16737fc5906b118 stage 3` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-publish/action.yml`
`100644 0ccba040522b463afa704652cdb388e0af1afaaf stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-qualify-macos-intel/action.yml`
`100644 1efb46c157eb1ef14adbfecd6524c60c046fabd5 stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/actions/generator-release-qualify-macos-intel/action.yml`
`100644 53d434e59ab6007a53a92a05f6a7b1658506757f stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/workflows/generator-release.yml`
`100644 c30c686fa10814b02db5ceffa93603ddf06bf8eb stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/workflows/generator-release.yml`
`100644 a156d5a457661a083e91b176def9d77eca008e77 stage 3` — `docs/proposed/opentofu-goldens/cases/dogfood/preview/.github/workflows/generator-release.yml`
`100644 685e4e2165e9ddf3be2ee51e5c83442555d768dd stage 1` — `docs/proposed/opentofu-goldens/cases/dogfood/tree.sha256`
`100644 7da5b969244d57f4a156fa6bcceb0dc3d64dee97 stage 2` — `docs/proposed/opentofu-goldens/cases/dogfood/tree.sha256`
`100644 df149d12bfd48f51f339b002022665c0adef995a stage 3` — `docs/proposed/opentofu-goldens/cases/dogfood/tree.sha256`
`100644 5d0fca8d920737245e8d9e04653bd62c9fb7fd7c stage 1` — `scripts/generator-release/create-release-manifest.sh`
`100644 d3230a33225a4a9a5fae8a44067024a0fc18f355 stage 2` — `scripts/generator-release/create-release-manifest.sh`
`100644 08196d10c2d52453b71bbc44f9541d67552641e1 stage 3` — `scripts/generator-release/create-release-manifest.sh`
`100755 a342f1b1ef7ea1e215a6e38b7de544bd5bcdfe11 stage 1` — `scripts/test-capture-opentofu-goldens-bin.sh`
`100755 798a7da1105d49ea49b201237c8a3aa03b77b67c stage 2` — `scripts/test-capture-opentofu-goldens-bin.sh`
`100755 ea9aa14928a8d5c26a214c9933e5767c5b90fd5c stage 3` — `scripts/test-capture-opentofu-goldens-bin.sh`

# CI performance preservation checkpoint

Recorded 2026-10-06. This sanitized ledger records preserved refs and registered temporary worktrees. It is not a release acceptance report. Verify remote content and process handles before removing any candidate.

## Preserved status

- Merged: PR #74 at bdb1e14509225d18b4d8ee180731825d0826c16f; PR #88 at 787157ed; PR #89 at 540e12a4a225683e78378e63bfdbba2ddba29d86. Current main is 4f6def90e7b1008626db18675d1cac129b8f2ad7; run 37351459281 has 19 jobs (18 success, 1 in progress, 0 failed). Across 22 checks, 20 succeeded, one separate image-publish check was cancelled, one remains in progress, and Required is pending.
- Qualification dispatch 37336033812 completed two readiness, SQL, and cleanup attempts (29.835 seconds). Physical host, daemon binary, and outer image identity remain unknown; this does not prove deployment.
- R1 branch codex/ci-performance-r1-checkpoint-20261006 has WIP commits 7d4c145ace13760d97e1942a56c1bcbc16b0c8e4 and dd93ee82335013eabc66fef7d7b1376bb8f528ae (tree 3ca41fc604c3a11d239973b8baf7e76b35a3e0c6). Independent byte checks confirmed four preserved follow-up files: worker/resources.rs, nested Cargo.lock, launch/steps.rs, and launch/steps/assigned_resume_tests.rs. The R1 integration worktree was removed after matching the remote tree and finding no open handles. Integrated runner tests/build remain incomplete; baseline was 243 pass and 13 fail out of 256 selected.
- Guest sampler passed 52/52 on isolated tree 9ff328ee98840d032d1b536d44cf4871653d8710; this is component-only evidence.
- Publication source was pushed for preservation on codex/chainargos-ci-performance: docs commit 638ac17d556969f4f484be7efd978bcd7873030e and WIP commit a860822d0a4e6a48ff1d9a45346ed9cbaa67819e. Focused tag-publication tests passed 3/3 on the preserved unit; the product has not been published.
- No deployed daemon/image identity, complete consumer run, two independent warm readers, or final matched performance sample is established. Keep the goal open.

## Cleanup inventory

The table lists registered <LOCAL_PATH_REDACTED> worktrees at capture time. The 10 permanent worktrees and primary checkout are excluded. All entries marked candidate still require Release to verify remote coverage, dirty files, and process handles. Four paths are held: archive-fixture-reproduction is unrelated; local-release-manifest-verify contains separate dirty release WIP; subagent-model-policy contains separate model-policy and runner-stage history; runner-null-stats-current-2217 has unresolved ownership. The independent cleanup audit verified 13 targeted worktrees absent and unregistered, including the R1 integration worktree.

G0 note blob 6bef5d9e67d6e197ae7acf20f415f79a486b761e at docs/wip/g0-merge-state-2026-10-06.md was absent from WIP commit 9ca089fe591d0dbae7beb651bfc83c2f516f8f80. It is preserved and independently verified at archival commit f1a2cbd419e31cea66290c0d554fcb520e920a9d (tree 7716cdd01038804042b4d55df89a578646e59450), with the original file SHA-256 e1c251b6c7bbedc6dff98dafd23e5a41901f1f700e720ca9347a9b1de018214b. Raw logs, container captures, runtime databases, credentials, and generated artifacts are excluded from this ledger and from Git staging.

### Registered temporary worktrees (87 at capture)

| Registered temporary worktree | HEAD | Branch | Disposition |
|---|---|---|---|
| <LOCAL_PATH_REDACTED> | 9ece72f0860bfba8a835d3062f0f555aa5954933 | codex/archive-fixture-reproduction | hold: unrelated or separate WIP |
| <LOCAL_PATH_REDACTED> | 79e8a12625045749ab94cfa2c1e309a70c02bce0 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 703d1b84d9f9e0246a52e3989bf3b78a13bbbf05 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 226a1bff9f220abe4c3a34385b5db25373714ac8 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | ac3ab6a3d5ba3701c1300bbdd8114390093c5c29 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 703d1b84d9f9e0246a52e3989bf3b78a13bbbf05 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 5122b0e6ac7e8b366d4fb604f3f65240881fa667 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | aa7d05e43383997231b02bdad781be1115d26b1a | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 226a1bff9f220abe4c3a34385b5db25373714ac8 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | f0d5d3858a7972c36103f3956c2f4fde8ef08967 | codex/cache-admission-caab | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | d435ac5b7e686ad9c9c594dde4b024b702435e47 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | a917608caa7b1b2ce368fd11d842a60ae383748b | codex/d-test-binary-prep | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 579dbb1d9c0eda6b19eddf825357820922c4dc28 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 703d1b84d9f9e0246a52e3989bf3b78a13bbbf05 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 9788a5e1c1303da05e9d26aa96043785b2f970a1 | codex/freshness-evidence-20261005 | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | bdb1e14509225d18b4d8ee180731825d0826c16f | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | bdb1e14509225d18b4d8ee180731825d0826c16f | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 076dc3f322fc0e02888def52d29cc9c3ff90854d | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | bc79d1415b767fa0ac3cdace1fa9852985d59921 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 8c31864bdc80439d03dfcbf1f98bc6f2816b09fa | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | f3a96d9829f44945afa6d51d357f64d4be92ad71 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | c8a42ba234556ef2be1742d8ed608d394b778e53 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | bdb1e14509225d18b4d8ee180731825d0826c16f | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 2d9bca8a37b0440e29a3520aa03e77752079f400 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | d435ac5b7e686ad9c9c594dde4b024b702435e47 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 508533c8a5b698125e2d7ac057b60df56c968bff | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 1caa21848df513f21718bde6f92c828dd4deb666 | codex/local-release-manifest-verify | hold: unrelated or separate WIP |
| <LOCAL_PATH_REDACTED> | a3d14cbb2690d5f48daecf9e1d6db53a451ea0da | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | c6b1e9283bcffadcd12d6343f476b97a3102eb6e | codex/nextest-bench-output | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 764fe32dbedfc6b46c8cba0277406fe3453ae5bc | fix/tofu-exact-cache-admission | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 817d04abc7903be03b9d141dbfce85cd136850bb | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | b3857ec26515d5309cae329082b164fa842dc871 | codex/freshness-evidence-captures-20261005 | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 10a6b12666d92278fc2bbd1ad723fc6fa2ee46e9 | codex/freshness-evidence-followup-20261005 | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | aea2cf5bb281ee307b0c00a7faee415486da2bcd | codex/v0111-draft-recovery-aea2 | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | ccd4642ec382c7f67ec0ea7247f42de840e4cf2e | codex/v0111-product-release-closure | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 766fbe11988868ec4eb339327ef6ac09c2b26678 | codex/g0-quote-main-fea935 | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0efa83e92d4eca0001795298dcbdf49635bfd905 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 7e6cd8c2cde7c41cfd11a0833935099c8f4c8aa1 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 006aa7b4a7ba764e6d77fb058bba1ae89b1729aa | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 9badf6491f5637f884d484fb4e2f7a3b6d4e6831 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 56ccc7f3a6a65ef8361853d8015151d95c79b9c9 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 8e6d29417217bdd3da2665e73b99194d8c1590e6 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 2a0e4ab2d9dccf675ab644186556d47b52504fc1 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 40c34e26894f832469510ed14e98af52b6890fb6 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | da79cf135eb37b0e3578573ae8516d90e585b5a6 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | f48ef76f71be19d375b0a222e8c39ad603ed8e79 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0b58b76dbf2fae6fb5746c65ce6e0752f0ab4966 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 5138149c9bb3ee7af97dcf93aae2ba1175717f97 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 2b0f5d83dd798a8745f6c8cfa18f4b859eb67744 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0f67e848ad227c16e29cd8a00493d4d6f9a172ed | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 630ec33ba08cc1737d384691a135cbbfa3d4df12 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | ffeb6b01cb75bd28e7ef6cbccc3a3cc347fb4e0b | codex/runner-null-stats-progress-2217 | hold: ownership unresolved |
| <LOCAL_PATH_REDACTED> | 91b37bccfec1146199524d9017846eb9d0575c42 | codex/runner-readiness-pr | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 9595984327980bfcc123dfc089ddc7ae39a5c86b | codex/runner-shell-semantics | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 11a7a516c698f880ecfd67bb1ece613f5840d991 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 7581150e73df43aeaa8a7b8ab80157c87afa02d7 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 2f0317f336b6f803376db85ac5404014768c06f4 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 31842c679d2a0e9434db82bac1a993dd45fb89af | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 8a1d4e92f8a01c011de4e99e650ac78fa295a588 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 81f3119e5bfcc03f55828cf789769bd0da978979 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | b018d078b82672c4e94647c044b2e61c11fa74b2 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 12962b2ab11b959a7ac00179d38070c7237b317e | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0efce75fb442698efbea20ff90010e3f9d98bce3 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | dac41338802dcf0ca55cd01d66dd099e08f06058 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | dac41338802dcf0ca55cd01d66dd099e08f06058 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 45db07ebdcfea6e147ce268db3cf1041baf25c95 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 7581150e73df43aeaa8a7b8ab80157c87afa02d7 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 7581150e73df43aeaa8a7b8ab80157c87afa02d7 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | ed6a8096a3e0b8cb37aaf20b59a73c9c9a907663 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | eaebeb96a8ff53ec9c412870cacd93d8a6461452 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 81f3119e5bfcc03f55828cf789769bd0da978979 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 81f3119e5bfcc03f55828cf789769bd0da978979 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0b5e8eb3353256a75281ef62a8f453b8b6ecde6b | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0b5e8eb3353256a75281ef62a8f453b8b6ecde6b | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 0b5e8eb3353256a75281ef62a8f453b8b6ecde6b | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | eaebeb96a8ff53ec9c412870cacd93d8a6461452 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | eaebeb96a8ff53ec9c412870cacd93d8a6461452 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | e7f24f4439a38e1f101cec6ad083e0c587742c64 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | a0d97950d880d37c764d739bbfba09d0d061e1c0 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | a0d97950d880d37c764d739bbfba09d0d061e1c0 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 2419d0415c18b12de3012e93916e0122af2022fb | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 5a5057da1ea2f7bcc0d41010220697c89885405b | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 6a1390429544c64975c5f55d85e00943a88664fc | codex/runner-stage-id-failclosed | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | ab323d77be8bcf3814c5d8f343124d14c5429f4f | codex/subagent-model-policy | hold: unrelated or separate WIP |
| <LOCAL_PATH_REDACTED> | bdb1e14509225d18b4d8ee180731825d0826c16f | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 7581150e73df43aeaa8a7b8ab80157c87afa02d7 | detached | candidate: audit before removal |
| <LOCAL_PATH_REDACTED> | 67e8d9da9a98b11d86a09ade6ead3fc8681b7ec0 | detached | candidate: audit before removal |

# Source-progress recovery, 2026-10-04

Status: recovery and bounded source-progress record only. All 47 performance statuses remain **INCOMPLETE**. This supplement grants no full integration, runtime, cache, hosted, or performance qualification.

## Recovery boundary

The read-only `~/.codex-chainargos2/recovery-evidence/velnor-pr12-recovery-20261004.md`, SHA256 `92af2c5282e2105baafcf95931cb2993a2884c9c799d4964e0c0cf08050ffba8`, records the checkout and evidence observed on 2026-10-04. At that inspection, several older `/private/tmp` receipts cited by prior summaries were absent. The observation establishes absence at inspection only; exact disappearance time, cause, and actor are unknown. No outcome is inferred from a missing receipt, and prior local-pass summaries are not treated as current qualification.

## Durable evidence recovered

- **Audit47:** [preservation report](ci-performance-audit47-evidence-preservation.md) records ten collections, 37,469 files, 3,095,542,884 bytes, and all 47 rows still incomplete. Private `audit-47-20261004/copy-manifest.json` SHA256 is `b7719c740229f03d8b49b070444a9d091ca9192197fdd10c4b9c0f58043498e2`; independent copy verification at `audit-47-20261004/verification/independent-copy-verification.json` SHA256 is `f4b4d6b8a045243d34b8c189bf32978e5228969dc2a453b84bc8da9815fe9822`. This preserves audit/source/run evidence; it does not qualify performance.
- **Recovered merge source:** private `d94fe9c/private-merge-5a/postbinding/MERGED_SOURCE-manifest.json` SHA256 `b92c2f0e78aa405393aeefe27fde2008028006018872f7a224af0cd981a19fd3` binds the source snapshot to HEAD `d94fe9cefa6a0b97f00d235191c2d3142d1af6ff`, upstream main `5a946c33cf005777feab2bc91fa4aa8e01dd58f4`, and Git tree `abd906474433cc1703360c234ea217863f0832ab` (1,381 tracked entries, seven symlinks; tree recomputed from recorded modes and object IDs). `source-conservation.json` SHA256 `2748267b42ffe72e03afa0ba56cea864461c36ac28d5daebd435eba3f3f1d056` and `acquisition-manifest.json` SHA256 `fbafa60d4fd3602fe328c034380dddc669c687b82c3e575cf962064af54a0d9c` preserve the bounded source and acquisition records. Source conservation explicitly leaves the merge unresolved and grants no compiler, test, runtime, or performance status. The older f3c/b9 merge snapshot is historical; it is not this recovered source identity.
- **Policy update:** commit `263e102826ae33046ba8043c75e831d283559232` records the current model policy. Parent execution receipt `recovery-20261004/parent/policy-commit-execution.json` SHA256 `107441f64d1609771c32801724382fb441ad5a2d248c76b6292efc7b1392e954` and independent review `recovery-20261004/parent/policy-commit-independent-review.json` SHA256 `9ff6cf489c723e0c1e26be3de8ffdd76f8826a370ce01e01ebca7859c1278fe1` both record PASS for that two-document policy transaction. It adds no source qualification.

## Bounded local build observations

A Rust 1.98.0 CLI build attempt failed the `hcl-rs 0.19.8` minimum-version guard; stderr `recovery-20261004/raw/cli-build.stderr` SHA256 `7aedf9b6636db13783bf55444ea049359bebefe640938f56b8f4e54735eaaf75`. A later Rust 1.98.1 CLI build log ends with Cargo `Finished` and a binary exists at `cargo-target/debug/velnor-actions`, SHA256 `d30abd1804ad67b0e14191194c615d01c02026acf09e6628346af7d828212f3b`; retained stderr `raw/cli-build-1.98.1.stderr` SHA256 `4d320ffb6705c4973e873ac4b6af1b3a5571dc7e6edd6cdf640fbe9b22643b3a`. The orchestrator library `--no-run` log also ends with Cargo `Finished` and an emitted test binary at `cargo-target/debug/deps/velnor_actions_orchestrator-1bf79c3d771508d8`, SHA256 `2b4f88900da5de03e970718c4867b42c30479959c224547cdbccf06354bc7d65`; retained stderr `raw/orchestrator-lib-no-run.stderr` SHA256 `3c81fb751cdc4326f76a9b2208f60cc6a80e2da5ab900fdf65fef9b586791abd`.

These logs name the `recovery-20261004/merge-qualification-v2` source path. No authoritative execution receipt containing full argv, cwd, environment, source identity, and exit status together is retained. The artifacts therefore record bounded local build output, not proof bound to the `MERGED_SOURCE` manifest. A later source review cannot reconstruct missing historical invocation provenance. The `--no-run` invocation executed no tests; neither output establishes full integration or qualification.

The recovery checkpoint separately records PR #12 at remote head `263e102826ae33046ba8043c75e831d283559232` with DCO as its only check; no CI or Required run was attached. It was draft and dirty against recorded base `c57c700459bbe1549fe7eedcb7d8689585c38986`. Refresh remote facts before any merge decision. The recovered d94+5a local source candidate and its build outputs are not the captured remote PR head.

## Remaining qualification work

Still pending before source/runtime qualification:

1. Finalize and independently review the exact integrated source, and retain an authoritative execution receipt for any required source-bound compile/test/lint/freshness gates.
2. Canonical builder execution and source-bound result.
3. MBX execution with qualified coverage.
4. Selection qualification, including full-obligation parity.

Do not infer these outcomes from source review, metadata-only commands, historical CI, or receipts that are now absent. Keep all 47 performance statuses **INCOMPLETE** until their separate gates pass.

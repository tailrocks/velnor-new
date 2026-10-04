# CI source audit v2: current main `47815c83`

Scope: source-bounded review of the CI and generator-release workflow bytes at exact commit `47815c83b9eeadbaf84b741918fffa7ea550da89` (tree `fd82cdbf7f96e63bab5b024c68fe55b0475ad147`, parent `9a249e9ab99a019ab7ba0e6f835dee406827db20`), with corresponding hosted-run observations. This records source behavior and run facts. It does not qualify performance or runtime adoption.

## Source identity

The raw GitHub Contents API response selected `.github/workflows/ci.yml` and `.github/workflows/generator-release.yml` at the exact target commit. Run metadata supplies workflow ID/path and `head_sha`, but no separate `workflow_sha` field; source identity here comes from retrieving workflow bytes at the commit, not inferring them from a run object.

| Workflow | Blob at target commit | Raw byte SHA-256 | Evidence file |
| --- | --- | --- | --- |
| `.github/workflows/ci.yml` | `aee06e8685ea16bcaf840b4240ebf6e3d1f4f2d6` | `cc68fa9248dd3d40007b693ec7e73657387e5e77a1e85b986e8e8c863bd3f06a` | `ci-47815.yml` |
| `.github/workflows/generator-release.yml` | `87abf573dd3da43695aaf0b7ccd7da41325c2aa0` | `d13645f1aa9667a1ce9fa0886ca5f4930c3a9a0e33c3dfe7df6b8942cad9962a` | `generator-release-47815.yml` |

These files are in the private evidence packet below. Both workflows pin their referenced actions to full commit SHAs. Their pinned dependency trees and actual run records are also captured there. Toolkit versions differ by action: workflow `actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9` pins `@actions/cache` 6.1.0; pinned `jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6` and `jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5` resolve `@actions/cache` 6.2.0. The saved package-lock/aube-lock evidence and both registry tarballs are in the packet.

## CI workflow and observed cache paths

The CI workflow triggers on pull requests, pushes to `main`, and merge queues. It configures read-only top-level repository permissions. Its Plan job uses pinned checkout, Mise, `actions/cache`, and MBX actions. The source sets Rust 1.98.1 and Mise 2026.9.18; task jobs execute the selected Rust obligations and upload task reports.

Source defines separate cache payloads even where visible keys match:

- Cargo source restore uses an ordered six-path vector beneath `$RUNNER_TEMP/velnor/cargo`; the key is based on `Cargo.lock` and `crates/velnor-runner/Cargo.lock`. It does not cache Cargo build outputs.
- The Mise action resolves its cache under `/home/runner/.local/share/mise`. The generated explicit `actions/cache/save` step names `~/.local/share/mise`. The pinned toolkit's version formula includes ordered paths, compression method, and salt, so these strings produce different version hashes. The saved API index has version `e61abf9f359d5350c7c9daa8fafea5bf582b24379c29361dec0dc476075a131a` for the explicit path. The action path computes `7a8f38effbb692ecc590f129f1a759a935e12173263c55e06b5c8ec7ae304837`; that version is absent under the logged Plan Mise key. This is consistent with the observed Setup Mise miss and fresh download/install.
- MBX custom single-bundle cache stores `$RUNNER_TEMP/mbx-single-bundle`; native object-mode MBX stores `/home/runner/.cache/mbx/actions/github-actions-cache-v1`. The visible cache key is not a unique payload identity. The API reports current-key entries ID `8467880101`, version `b84c808e4fb7778bbabd36f3f259656cbf76e2f9281f995126f20ba741c25832`, size 69,947,379 bytes; and ID `8467894819`, version `0157fd6dc00a4028c01319445731e038de11127c91c5df4087964826ce683348`, size 29,062,963 bytes. The pinned `@actions/cache` formula and actual path vectors reproduce these values.

On main CI run `37163556069`, Plan restored the parent custom bundle by prefix and imported 106 actions / 667 objects (216.4 MiB). Plan later saved the new custom bundle; matrix jobs restored it. The native MBX cache was a separate path and save. The Plan and matrix task reports show cache `not_attempted`, while MBX logs separately report hits, misses, non-lookups, and bypasses.

There is an unresolved source/runtime mismatch in native MBX save policy. The captured Rust / `velnor-runner-core` restore step environment says `ACTIONS_CACHE_MODE: read`. Pinned MBX source passes that value to its save-policy check and denies writes for read mode. Yet the same run logs save eligibility, exports 23 actions / 135 objects, and records cache ID `8467894819`. The separate custom bundle save uses a different `actions/cache/save` step and does not explain the native object write. The packet does not capture the actual Node process environment or saved `GITHUB_STATE`; do not infer a cause or claim read-only mode was enforced at runtime.

## Generator-release source guard gap

At this commit, generator release triggers only on `workflow_dispatch`. The workflow has no `if:` or step that checks `github.ref` against protected `main`, and no step that checks a same-SHA `Required` result. `publish-generator` grants `contents: write`, depends only on `attest-linux` and `attest-macos`, and has no GitHub `environment:` declaration. Its final step creates tag `generator-${GITHUB_SHA}`, targets `${GITHUB_SHA}`, and publishes assets from those two attested build jobs. The captured release job API reports no environment. The source therefore permits a dispatch-selected ref to build, attest, and publish without waiting for that source SHA's CI Required gate.

Observed runs demonstrate the ordering on this exact source, not an attack or external protection state:

- Main CI `37163556069`, event `push`, exact target SHA: 20/20 jobs succeeded; Required job `111323907861` finished at 00:13:48Z.
- Generator release `37163597428`, event `workflow_dispatch`, same exact SHA: 5/5 jobs succeeded and release publication finished at 00:03:35Z, 10 minutes 13 seconds before Required finished.
- The release output is source-bound; this proves neither downstream adoption nor a qualified runtime. Repository rulesets, allowed dispatch refs, and other out-of-workflow protection settings were not captured, so no claim is made about external enforcement.

A source repair should restrict publication to the protected source ref and require successful Required evidence for the same SHA before publishing. A protected release environment may add an approval control if that is intended. This document records the gap; it does not alter workflow source.

## Run boundary and qualification limit

The commit-message Required ID `37162836495` is a different PR run, head `37416ae648c9bdb7319ddfc8a21776dcec93eba9`. The exact-main CI run is `37163556069`; release dispatch is `37163597428`. Keep these identities separate.

The exact-main final CI artifact reports 71 selected/executed, zero failed/reused/covered/not-run, and successful required results. All 71 task reports mark task cache `not_attempted` and have zero in queue, preparation, download, compiler, link, test, lock-wait, MBX, cache, and runner timing fields. The 59 matrix MBX summaries record 1,307 hits, 7 misses, 2,585 not-looked-up, and 59 bypassed. These show work and report delivery; they do not provide compiler/link wall time, measured savings, a controlled cold→warm→third comparison, or runtime qualification. Orchestrator integration tests dominate the visible job wall time, but APIs do not split runner queue/provisioning or task compiler/link/download/test stages. No performance claim follows from the green checks, MBX estimates, or cache archive sizes.

## Evidence custody

Frozen raw packet: `~/.codex-chainargos2/evidence/velnor-pr12/recovery-20261004/current-main-47815-ci-audit/`; `SHA256SUMS` has 252 verified entries and SHA-256 `31a91f2c76889cad4a8ce8588bee5e6c183ba3ced31196bf25cc2a6a51e1b416`. Raw cache API response is `cache-index-final.json`, SHA-256 `d91ec1ea8f4400204602183cc69fa981417cafcffe68675123ced47e324cca4d`, from `GET /repos/tailrocks/velnor-new/actions/caches?per_page=100`.

Independent cross-check receipt: `~/.codex-chainargos2/evidence/velnor-pr12/recovery-20261004/current-main-47815-ci-audit-independent-review.md`, SHA-256 `f5a6406a1fbb5d1602b23e7a48bae8d33532021910c9f01d3314da61b09a5f3a`. It records the raw/enriched Plan-log distinction and corrected Mise hash. The versioned erratum `ci-performance-actual-runs-20261004-erratum-v2.md` preserves those corrections separately from the packet.

No repository status or 47-row qualification ledger was changed. This source audit does not mark any performance or runtime gate complete.

# Consumer caller audit: bounded delta freeze

**Frozen:** 2026-10-05 06:12:27 UTC. Velnor refs and PR metadata were refreshed at 06:03:34 UTC; unauthenticated Holla-callee probes completed at 06:06:49 UTC. Consumer-head observations below retain their own earlier probe windows.

This is a bounded delta and locator for the reconstructed 47-row union. It does not replace the immutable Jackin and Tailrocks packets or turn message-only ChainArgos observations into a complete raw packet. Missing evidence remains `PARTIAL` or `UNKNOWN`.

## Scope and evidence locators

The declared row count is 47: 8 Jackin + 34 Tailrocks + 5 ChainArgos. `tailrocks/repo-scan` is a distinct row, the 47th relative to the historical 46, and is included in the 34 Tailrocks rows. Its independent current-head probe is recorded below. The source list remains reconstructed, not an authoritative provider inventory; therefore the union’s completeness and any additions or omissions remain `UNKNOWN`.

The current reconstructed source files are `scope.json` SHA-256 `3f82007f8f57e2fc31ce642666a877f4b4034f7d38aa67f73ee3ecce74d50868`, `repositories.txt` `03ca56ac023196f681551676cef5860a209ae2b388439ac6e144b95025a21883`, and `repository-evidence.csv` `84a4bbf37713fdc7b5283da61eedef5b655d57655933c234a78e36456d3d708a`; the scope audit is `docs/reviews/ci-performance-scope-audit.md` SHA-256 `fd780c00c6e18ce4b3ebb03c417799eec7c8321b46cfd553693ff814f09aab5e`.

Frozen cohort packets:

- [`consumer-caller-audit-jackin-2026-10-05.md`](consumer-caller-audit-jackin-2026-10-05.md), SHA-256 `09470e6f00ce744b0d12fa0653ca5ad20619ab7d443f8827edb1e029c22fc3a4`.
- [`consumer-caller-audit-tailrocks-2026-10-05.md`](consumer-caller-audit-tailrocks-2026-10-05.md), SHA-256 `403b4349f46fbef1bf38de12aca7c3a9d9847be988983d35d026be899f643c62`.
- No durable ChainArgos packet path or hash was available in this checkout. Its 05:40–05:53 UTC refresh below survives as message-only evidence, so PR-path details beyond those explicitly listed remain partial.

The Tailrocks packet’s source links used the wrong repository owner path. The verified PR #12 source commit `8625f5d191bffc807f7b80a607ad1fc51cbc171b` belongs to `tailrocks/velnor-new`; the corrected links are [owned-source inventories](https://github.com/tailrocks/velnor-new/blob/8625f5d191bffc807f7b80a607ad1fc51cbc171b/docs/reviews/owned-source-review-2026-10-04/owned-source-inventories.md), [implementation goal](https://github.com/tailrocks/velnor-new/blob/8625f5d191bffc807f7b80a607ad1fc51cbc171b/docs/reviews/owned-source-review-2026-10-04/velnor-owned-source-implementation-goal.md), and [goal continuation](https://github.com/tailrocks/velnor-new/blob/8625f5d191bffc807f7b80a607ad1fc51cbc171b/docs/reviews/owned-source-review-2026-10-04/velnor-owned-source-implementation-goal-continuation.md). The frozen packet itself is unchanged so its recorded SHA remains valid.

## Current Velnor refs and scoped deltas

At the 06:03:34 UTC refresh, unauthenticated `git ls-remote` and GitHub API agreed that `tailrocks/velnor-new` `main` was `bcd2fd3ef612dc7e22a7e21d6eed3504bebc1642`; GitHub reports commit time `2026-10-05T05:51:31Z`, parent `5a3f5348c86d733d8705dadd7c2b36420a2fc0bd`. PR #12 remained open and draft at head `a949662c58e4b411387218fb48ccfdc7b94ebc40`, base `7fb8367d7daa67f13ccaa7c76caae47d55d6262b`, updated `2026-10-05T05:03:09Z`, `mergeable=false`, `mergeable_state=dirty` (389 changed files, 119 commits). `refs/pull/12/head` matched the API head. The local checkout was at stale commit `d707cd232a5516627835476e81e596e2ed6902a2` with a dirty worktree; all current remote identities here come from the remote ref/API reads.

The latest `main` commit `bcd2fd3` is qualification log instrumentation. Its five changed paths and resulting blob IDs are:

| Path | Blob |
| --- | --- |
| `.github/workflows/qualification.yml` | `5699a2f1760c8ff03d84d0b4a99b6efe373365fe` |
| `crates/velnor-actions-orchestrator/src/schema2_mbx_qualification.rs` | `9445fa06323e034f7b3511d52e14359c277bcced` |
| `crates/velnor-actions-orchestrator/tests/impl_schema2_mbx_qualification.rs` | `74f8593e1bbda63d0bffb3b875c83a32b205cd41` |
| `crates/velnor-actions-orchestrator/tests/schema2_class_snapshot_continuation.txt` | `5f772bea0063f662b0557803628602188671b6c3` |
| `docs/implemented/cache-measurements.md` | `ada885f06079ff05a29838ef1d6ab012588ba7a6` |

That commit samples runner disk and inode use and tees MBX stats JSON in the existing protected-main qualification writer/reader. It preserves the `jdx/mr-boxington-action` pin and the writer guard. The required qualification run `37268537569` was reported successful at `05:50:02Z`. These changes do not add a consumer route to retired owned-source snapshots.

PR #12’s current head commit `a949662c58e4b411387218fb48ccfdc7b94ebc40` was committed `2026-10-05T05:02:45Z`, parent `0977a7ec88d8f61900ca0d5d800d4d5c991a365e`; its only changed file is `docs/reviews/owned-source-retirement-ledger-2026-10-05.md`, blob `14779ce1fa1de02b40b0b49264ba11c67b657794`. No later PR head was visible at the API/ref refresh.

The four requested helper/test paths are present at current PR #12 head `a949662` with these blobs:

| Path | Blob | Observed caller |
| --- | --- | --- |
| `crates/velnor-actions-orchestrator/src/release_source_snapshot.py` | `5fee1253ec1678c6d02561fb70fab4d8fa4711ce` | Loaded by its test below |
| `crates/velnor-actions-orchestrator/src/release_source_tree.py` | `b2bd17c0b7211f923fe503dfc0ab569ecb24f524` | Loaded by its test below |
| `crates/velnor-actions-orchestrator/tests/release_source_snapshot_test.py` | `61861804d87f08082d2b9a1f25ac513a3db2df50` | Resolves the snapshot helper path and runs `exec(compile(...))` in its test fixture |
| `crates/velnor-actions-orchestrator/tests/release_source_tree_test.py` | `82d58a1b926d4b4dbbee7df344c5a0a29aadaec4` | Resolves the tree helper path and runs `exec(compile(...))` in its test fixture |

The exact-name check of current `.github/workflows/ci.yml`, `Cargo.toml`, `crates/velnor-actions-orchestrator/src/workflow.rs`, and `workflow_tools_tests.rs` found no helper invocation; `workflow.rs` only includes the separate `workflow_tools_tests` module. This establishes the two test-loader edges in these inspected paths, not a global absence of other invocations. No Python code was executed.

## Row 47 and ChainArgos refresh

At `2026-10-05T05:54:11Z`, `tailrocks/repo-scan` was separately confirmed at `main`/HEAD `13e534b5d2ad688a4d05901d36d5afc95bc454ce`; the open-PR endpoint returned `[]`. Its actual content blob IDs were `.github/workflows/ci.yml` `de5a2ac432c9ee3ad4cc28f01de154bb98e1ca18`, `.velnor/config.toml` `647d4117b64880a5a725be4f7d998554348006f7`, and `.velnor/release-manifest.json` `51d743cd74b6a5da97bb50e7039de537faea3969`. The bounded direct marker scan found no owned-source/custom-cache marker in those files. This row is distinct from the historical 46 and is not an alias.

The ChainArgos observations were captured in the `05:40–05:53Z` window. These are the exact heads and selected direct content blobs retained in the thread; they are not a complete durable PR-head packet:

| Repository | Default head | Workflow blob | Config blob | Evidence status |
| --- | --- | --- | --- | --- |
| `ChainArgos/blockchain-nodes` | `0881638712a837bdcb90b3cd811a8a9be3aa8d83` | `.github/workflows/ci.yml` `42eb4239ddc400cbecc8bb8855966dc1d2005443` | `.velnor/config.toml` `d952e4a60b9683463b7079eaf56d082dafdfe61d` | Selected current blobs were read; no retired-snapshot marker recorded. |
| `ChainArgos/java-monorepo` | `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0` | `.github/workflows/ci.yml` `715bd247f39edbaa2218132bbea23677fc6309ea`; qualification `8b95c3323a05334255e36ec7f641390f1e39500b` | `.velnor/config.toml` `b1049b149c033e0c538a819d6e3eba13e8666d34` | Current main and PR #2085/#2086 were inspected in the message checkpoint. #2085 is a normal MBX GitHub object-cache qualification on protected `main`, using `jdx/mr-boxington-action@d0825fbaf3cc36ca2609aa38e71046265a1f1e37`; it is not an owned-source snapshot reference. |
| `ChainArgos/cloudflare-tofu` | `cc0e2b687dc6c1a9943b0ae9f5911f1e782c1a20` | `.github/workflows/ci.yml` `c326f5e069333504d7e0b3eec91b3b469f567663` | `.velnor/config.toml` `f9836e6e78f8002631babfb76c8c3f27fffce66e` | Selected current blobs were read; no retired-snapshot marker recorded. |
| `ChainArgos/github-terraform` | `c8d47ea87a611251f965555dafbd8015f29f303b` | `.github/workflows/ci.yml` `bc463561817ed8325dddb95949c6f3c15033a28f` | `.velnor/config.toml` `bc0edd56aaddb5a97120cf4b582e12c5ff60ee4a` | Selected current blobs were read; no retired-snapshot marker recorded. |
| Brown row | `UNKNOWN` | `UNKNOWN` | `UNKNOWN` | Public access was not established; see below. |

The Java PR message checkpoint recorded #2085 head `8c08c76454fdbbe301bcc8bb459cd8a4893373fd`, base `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0`, and #2086 head `f6fce617d9da8aeca637dfce104849dbf2e04b7e`. Those PR-head identities have not been refreshed in this delta; do not treat them as current at freeze time. The ChainArgos packet’s missing raw locator also leaves its full open-PR/current-file coverage partial.

## Unresolved access and indirect edges

Brown remains `UNKNOWN`. A prior default `git ls-remote` run used the configured credential helper and returned candidate main `6b2ac227…` and PR #241 `092fe1db…`; that was not an unauthenticated result and is not treated as public proof. With credentials disabled, `git ls-remote` could not read a username; unauthenticated REST returned HTTP 403/rate-limit, and codeload for the candidate OID returned HTTP 404. No Brown current workflow/config/PR-head content is established here. These results do not prove the repository or callers are absent.

Holla PR #202 is a positive indirect edge. Its inspected `.github/workflows/ci.yml` at head `560b51105c76c7f90673989b06843b091a51a90a` (workflow blob `0d8e4ae6cba94d5a4987e55e9eb51d0a2ed22603`, unchanged from base `fca7d0cc41e139014900e1876a4ab30f126cddda`) calls three reusable workflow refs:

- `jackin-project/velnor-actions/.github/workflows/ci-code.yml@851ef541d67f9cabebf2ddb2a2a02f51f6c54130`
- `tailrocks/velnor-actions/.github/workflows/ci-code.yml@3d108dd476d10e4327e5f34295b7324aa6207130`
- `ChainArgos/velnor-actions/.github/workflows/ci-code.yml@76a459854130953c67fd2c8d33cce8b843732be7`

At `2026-10-05T06:06:49Z`, unauthenticated GitHub REST repo-metadata and exact-commit endpoints returned HTTP 404 for all three, and strict unauthenticated `git ls-remote` could not read a username for any. The pinned callee trees, current default heads, and PR inventories therefore remain unavailable; 404 does not establish deletion or zero callers. The local bounded Git-object lookup previously found no matching objects. Holla #202 changes its Renovate workflow; its unchanged CI workflow remains the positive edge. This unresolved route prevents a blanket “no consumer” conclusion.

The bounded marker checks in the frozen cohort packets cover owned-source/Foundation/snapshot references, retired custom MBX protocol names, and relevant workflows/configs/local actions/invoked scripts at the captured immutable heads. Tailrocks’ captured default scan included 34 non-truncated trees and 104 selected blobs; Jackin’s packet covers its eight captured main heads and the listed PR heads. Searches found normal Velnor plan/action use, which is not itself a retired snapshot caller. Inaccessible Brown/Holla callees and the missing ChainArgos raw packet remain explicit gaps; no index-only or missing-content result is treated as a no-hit.

## Per-consumer audit method and disposition

For each declared row, retain the provider/list provenance separately from consumer evidence; pin the actual default SHA and fetch its tree; record exact workflow, task, config, reusable/composite action, and invoked local-script paths with blob IDs; scan the bounded retired-source, snapshot-ref, and custom-MBX marker set; follow each positive `uses:` edge at its pinned immutable SHA; and inspect relevant open-PR heads by actual changed paths and head files. Record a `NO-CHANGE` result only for the named inspected files at the captured SHA. Record inaccessible refs, truncated path coverage, rate limits, missing raw packets, and uninspected indirect callees as `PARTIAL`, with the next probe identified. Current overall closure remains `PARTIAL`; the 47-row declared union is not authoritative scope proof.

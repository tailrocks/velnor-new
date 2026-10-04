# Consumer scope audit — refreshed readiness evidence, 2026-10-04

## 07:42–07:53 UTC refresh — readiness only

Current integration candidate: PR #28 at head
`52297bc15ce19e39c84deffe10be76d0ba026d15`. The candidate generator version
is `0.1.1`. The 07:42 UTC GraphQL snapshot rechecked all 86 refs (46 default
heads and 40 open PR heads) for the same ordered set of 46 consumer
repositories; every ref was accessible and its API SHA matched the scanned
SHA. Only two open PR heads moved since the earlier packet: Jackin #1111 is
now `3a28c199f17da335ecd9abd8dd67ebf1aecc0421` (updated
`2026-10-04T04:14:24Z`) and ChainArgos/java-monorepo #2085 is now
`7cbe1db11ccabb463053a93a8a08d33bda418b29` (updated
`2026-10-04T05:01:14Z`). Their refs are refreshed in the companion CSV;
all other refs, rows, order, and owner checkpoints are preserved.

At 07:53 UTC, scans of those two pinned PR heads found zero matches for the
29 retired-reference needles: nine owned-source ref names, 18 reviewed or
excluded SHAs, `owned-source/`, and `tailrocks/velnor-new@`. The full 31-needle
R1 set also includes two generic Velnor URL literals, which match expected
documentation, workflow, and manifest files. The union CSV's generic-path
counts/classifications for the moved PR rows retain their prior-snapshot
provenance; they are not current-head counts. Jackin inventory: 2,519 text and 116 binary files. Java monorepo
inventory: 7,007 text and 5,939 binary files. Binary counts are inventory
evidence; they do not imply semantic text coverage of binary contents. See
[the retirement-union refresh](consumer-retirement-union-2026-10-04.md) for
the exact-pattern set, row-level heads, and scan limits. The refresh did not
repeat generic path or workflow-use classification for those two new heads;
those fields retain their prior-snapshot evidence.

This is readiness evidence, not adoption or migration approval. All 46
consumer repositories await the exact immutable `0.1.1` packet from source
`52297bc15ce19e39c84deffe10be76d0ba026d15`: the three official target assets
(`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
`x86_64-apple-darwin`) with SHA-256 digests; a schema-1
`velnor-actions-release-manifest.json` binding version, repository, full
source commit, target URLs of the form
`https://github.com/tailrocks/velnor-new/releases/download/v0.1.1/<asset-filename>`,
and digests; source-bound asset attestations; and a byte-identical published
manifest committed by each consumer at
`.velnor/release-manifest.json`, followed by exact-artifact regeneration and
deterministic parity. Three owner holds remain: Velnor-actions-fixture
disposition/retired hosted triggers; ChainArgos/java-monorepo #2085
qualification; and ChainArgos/jackin-agent-brown #241 rollout and
runner/cache qualification. Jackin #1108 remains a separate validator
checkpoint. No consumer repository or remote GitHub API/data was mutated.

## Snapshot and authority

Read-only consumer audit snapshot completed 2026-10-04 02:27 UTC. Facts are time-bounded; they are not a migration approval or a live status check.

Scope source is open draft PR #12, head c694d8029eb880db639fa89b0589090dd2b15364, base c57c700459bbe1549fe7eedcb7d8689585c38986. PR updated 2026-10-04T01:02:17Z. Candidate scope.json blob is 33928dd5d3a617bc0b1341b8df81955a54f7607d; repository-evidence.csv blob is b1bbcfa8fb8c8f992d6be62e427860aaf2c8c1fa.

Scope has 47 rows including generator and 46 consumers. Its ordered consumer names match repository-evidence.csv and historical Appendix C exactly; no duplicate or set/order delta. Scope provenance says reconstructed from CSV and cross-checked against the historical performance-goal list. Treat it as the active PR candidate input, not independent upstream authority.

GitHub metadata resolved all 46 repositories. Every default branch is main; all current configs, manifests, and CI workflow files were accessible. Four private ChainArgos repos were readable. The only canonicalization is tailrocks/tui-snap to tailrocks/tuiscotti (repository id 1358764452). No repository was inaccessible.

The companion scope-audit CSV preserves all 46 numbered rows in Appendix-C order: exact default SHA/time, config/manifest/CI blob IDs, effective policy, generator source/platform digests, workflow files, caller inventory, stock-MBX classification, all 40 open PR head refs/SHAs, and owner checkpoints. The 07:42 snapshot refreshed the open-PR head values for Jackin #1111 and ChainArgos/java-monorepo #2085 in both CSVs. All other scope-audit fields preserve the original bounded audit evidence.

## Current consumer state

Forty-five manifests pin Velnor Actions 0.1.0 from source c57c700459bbe1549fe7eedcb7d8689585c38986. ChainArgos/java-monorepo alone pins d40868152f7fe0106e3ede858a411f502f00810f with distinct platform digests. No current manifest points to the separate generator-47815c83b9eeadbaf84b741918fffa7ea550da89 release URL; that artifact is old-source evidence, not the consumer target.

All 46 current ci.yml files stage Velnor acquisition, generation/plan-v1, and velnor-plan artifact publication. This is staged executable evidence; runtime observations are stated only where checked. No velnor-compare or retired owned-source custom behavior was found in the bounded audit.

Fifteen current CI workflows stage the stock MBX action: jackin-project/jackin, tailrocks/termpane, tailrocks/velnor, tailrocks/parallax, tailrocks/parallax-telemetry-playground, tailrocks/velnor-actions-fixture, tailrocks/holla, tailrocks/tracing-request-level, tailrocks/pg-bigdecimal, tailrocks/ruxel, tailrocks/schemalane, tailrocks/tablerock, tailrocks/cloudflare-tofu, ChainArgos/java-monorepo, ChainArgos/cloudflare-tofu. These generic action paths must be preserved or explicitly migrated; no owned-snapshot cache behavior was found.

CI has no relative local action callers. ChainArgos/java-monorepo invokes its project scripts/Cargo.toml package chainargos-scripts for MBX build/clippy/nextest/doc; this is not a retired Velnor Phase D/Appendix B script. Its extra manual qualification.yml uses local qualification test composites. Termpane also has a Velnor-generated release.yml with workflow_dispatch/release-plz.

## Probe method

- Fetched PR #12 metadata and candidate scope/CSV blobs by exact head c694d8029eb880db639fa89b0589090dd2b15364; compared their ordered consumer names with Appendix C.
- One GitHub GraphQL snapshot resolved canonical full names and collected each default branch SHA/time, .velnor config/manifest blobs, CI blob/workflow names, privacy, and every current open PR head. Repository contents were accessible for all 46.
- Cohort reports inspected exact default trees and relevant open-PR heads/diffs. Searches used bounded git grep/rg over fetched files, not GitHub code search alone. Supplemental A-cohort clones matched all ten API heads and were searched with fixed strings.
- Positive caller classification distinguishes docs/product/test text from staged executable workflows and from runtime invocation. Current plan artifacts are staged in all CI files; log-level runtime evidence is claimed only for the runs recorded in the cohort findings.
- Direct lane execution was verified from session JSONL /Users/donbeave/.codex/sessions/2026/10/04/rollout-2026-10-04T07-15-47-01a10444-4d87-7111-b1a2-37e5b2fce6b8.jsonl as gpt-6-luna/max. Jackin findings came from the separate gpt-6.1-sol/medium lane.

## Bounded reference search

Compliant cohort/deep audits searched current default trees and relevant open-PR heads/diffs for six full in-scope ref/tip pairs, all 15 Appendix A commit SHAs below, Phase 0/Phase D/Appendix B script names/import stems, custom behavior, and Foundation/SDK/quarantine identifiers. No in-scope owned-source ref, snapshot SHA, or custom runtime dependency was found in that bounded consumer set. Broad positives were classified as product/tests/docs, generic host settings, or stock MBX.

Six in-scope owned-source refs and tips searched:

- owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96
- owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81
- owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075
- owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696
- owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd
- owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf

All 15 Appendix A SHAs searched:

- MISE: dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96.
- MBX: 1ca12eb48391061a75e97d32e5b06fedf8a6253d; 2f324e89af509a1c61d802cf23741a04d7510b57; ee250ac37654a4cfbb55b6cd470f2a257204bbe9; e07c07cfec773897e5c439043901e661ad3f7b45; 29865b5a18e3414084ff5db27d1da455772b4c34; ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81.
- MBX action: f053f215866af0ddb6d2f32ecc19d45a5d25edc2; 198f0d1a538d34a91d7692f8f302259643c0f737; 06f353d41002af758d27490164f53c82e2165637; c3cbe8e56ccb4727624df45022357f49d2953075; 62ec0713473dffeab46884b7c03906042794e696.
- Semver: d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a; 583dddce84706786fc54c41a2c768c28a09c65fd.
- Cache: 8758d976a1b25eb387f48aa04ea86f57739b84cf.

A separate archive inventory snapshot at 2026-10-04 01:19 UTC listed two additional MBX refs outside the six-ref retirement set: owned-source/mbx/1cde36b9f519f1fea8bf4ba8d82721906410448c and owned-source/mbx/7fd10972216ce478478b704781b4a4f2715f46d4. The 02:27 consumer scan did not include these needles. Do not extend the no-hit result to them; H8 remains open.

Post-cutoff discovery: at 2026-10-04 02:30:31 UTC, a third excluded ref was recorded: owned-source/mbx/3e462cf9248dd3d17baf9131af5f7a4aec76b80e. Its tip is a test-only commit parented to 7fd10972216ce478478b704781b4a4f2715f46d4. This was discovered after the 02:27 cutoff and was not searched in that consumer union. The bounded no-hit result applies only to the six in-scope refs/tips and 15 listed SHAs; it does not establish zero consumers for any of the three excluded refs.

Limits: snapshot covers current default heads and open PR heads in the CSV plus relevant PR diffs from cohort reports. It does not cover every closed PR, every nondefault branch, or binary payload. NO-CHANGE is limited to this bounded evidence.

## Ordered consumer status

| # | Scope name → canonical | Current main SHA | Status and positive evidence | Owner/checkpoint |
|---:|---|---|---|---|
| 1 | jackin-project/jackin | 0aa821a088e1bacf3d4d85a4c9faaa67faa85132 | NO-CHANGE (source); HOLD (#1108 validator); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; #1108 task validator binds legacy generated workflow/state paths. | Jackin owner; migrate #1108 validator before adoption. |
| 2 | jackin-project/jackin-agent-smith | 2e7119b9c668ca7a9c55218b20049885299d198f | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 3 | jackin-project/homebrew-tap | cd05a0ea2cf68fd6c2753ee938247b2dcd4c7551 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 4 | jackin-project/jackin-role-action | 59e538704b8c119f3b6668cea0154909a6158ff6 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 5 | jackin-project/jackin-sentinel | 587a0d1a8eef96108c9d9d530bdaa13df91edd2b | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 6 | jackin-project/jackin-dev | a01b342162bc56cdf1e8bbaab793e73d31c1d621 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 7 | jackin-project/jackin-github-terraform | b43a2314c6b58906d52828115d3b3973026f6a2a | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 8 | jackin-project/jackin-the-architect | 2cf461e2fed1b95d9fd1e7ba74c10d4d8b1c685d | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 9 | tailrocks/github-terraform | 667eae4d8ecedde5153eb486fefe07a9d00cc3ad | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no; unrelated Tofu lock/install mismatch 1.12.5 vs 1.13.1. | Repo owner; resolve unrelated Tofu pin drift separately. |
| 10 | tailrocks/termpane | 7cf2f9981ef9a3f7c8fc295a7501a261fba955a8 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; separate generated release.yml dispatches release-plz. | Termpane owner; preserve release and stock MBX paths. |
| 11 | tailrocks/tui-snap → tailrocks/tuiscotti | a47c9aaefb34e4c00026f99d8a8dd7ee5916b274 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no; old scope alias resolves to tailrocks/tuiscotti. | Repo owner; exact-artifact regeneration and deterministic parity. |
| 12 | tailrocks/velnor | 72f0fb14ec8f477fce8a505a624b92837acab85d | NO-CHANGE (source); HOLD (workflow API state); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; three API-active workflows absent from main/#1133. | Velnor owner; reconcile stale API active workflow records. |
| 13 | tailrocks/termrock | e2515bac765f440b11843a25d36ed8f720ae6435 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no; Foundation/SDK hits are passive docs/theme/local checks. | Repo owner; exact-artifact regeneration and deterministic parity. |
| 14 | tailrocks/parallax | 90d901c9d12477e93a56a9e021077ced7c78f9df | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; run exported 1,597 objects (~777.7 MiB); cache reservation warning. | Parallax/cache owner; qualify stock MBX lifecycle. |
| 15 | tailrocks/terminal-components-claude | 84482d066c5f0bc531f875f7f9d7716929c1b9c6 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 16 | tailrocks/tailrocks-repository-skills | 036063edc58d88cd88f57e4e3a9721ebadf3ad62 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 17 | tailrocks/tailrocks-skills | 1e9a23a63e0a44abf6a5ef17b69711011316cd9f | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 18 | tailrocks/tailrocks-pull-request-skills | 1b260bab1e356b1123fbbbdcdb17bb7bbf8ae83a | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 19 | tailrocks/homebrew-velnor | a0db8c185b76e1bfab3508124504d0f6361e83b6 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 20 | tailrocks/velnor-apt | 115b5c42d7ad5659c8496600fabf6cd8061b64b1 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 21 | tailrocks/parallax-telemetry-playground | 763518791d60d4197d36119a47311e008ae3f5bf | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 22 | tailrocks/velnor-actions-fixture | 1c076c5b5828fb6ba04887885c567da26fe01a69 | HOLD (workflow-contract disposition); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; dual-lane docs conflict with current one-lane workflow; #172 removed hosted-only dispatch/schedules. | Fixture + Velnor owners; disposition docs and retired hosted triggers. |
| 23 | tailrocks/holla | c756189538c776eaa563ff83a5488e1ac96600c9 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 24 | tailrocks/tracing-request-level | 2675c867fa2f8af78c2bf2543482add847eec713 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 25 | tailrocks/pg-bigdecimal | 0a06226df6ec5f4e1a70a853d14368ebf2e69a80 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 26 | tailrocks/ruxel | 304da9f21271be81964459f5c7e6f51f41e5fdcf | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 27 | tailrocks/schemalane | 94a58dad0fe6714312373fd5dac69f2ced75d194 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 28 | tailrocks/holla-apt | 405239a2ffec34bca5eb14d95915fb0cdaae0453 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 29 | tailrocks/homebrew-parallax | 70af3b38e051abfa5a14c08ed5cd5388219ffc40 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 30 | tailrocks/homebrew-ruxel | 1db9876988e9912bafb6ea9333eea116853dc31f | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 31 | tailrocks/homebrew-tablerock | 20428a6d85086aa8454e3dd693e575541e714df2 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 32 | tailrocks/homebrew-holla | 5b7c0f27b570de6ef07f17f593cf96737b0de3ce | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 33 | tailrocks/tablerock | a9771cab32271b1b3fca40f536d9112970989623 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; Foundation/SDK hits are native product/tests/docs. | Tablerock/cache owner; preserve stock MBX path. |
| 34 | tailrocks/cloudflare-tofu | 03fb253cc0f352c3f57ff7dff9d35e4ea95eec4e | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending | Repo owner; exact-artifact regeneration and deterministic parity. |
| 35 | tailrocks/tailrocks-typescript-skills | 0652a50fce67c4a11f01ebb960a50fd5e283e0a9 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no; quarantine hit is generic file-transaction cleanup. | Repo owner; exact-artifact regeneration and deterministic parity. |
| 36 | tailrocks/tailrocks-skill-authoring-skills | 93fa4d609f348c04869a94d0dc7a10751e31bf1d | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 37 | tailrocks/tailrocks-rust-skills | 0317f100714dc66285c01c24f7967134375b5ac5 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 38 | tailrocks/tailrocks-roadmap-skills | 98d23280cd562c9298ce13ae40bd0eb73a3e376a | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 39 | tailrocks/tailrocks-open-source-skills | 3e51bc5c91949f361ed926d8f760bcb16edef111 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 40 | tailrocks/tailrocks-macos-skills | eb0be5522fe0c1c9c74d41ee354446a011b10c74 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 41 | tailrocks/tailrocks-code-quality-skills | e63a82f28b688a7fada418e615aa9ec0eeec4c04 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |
| 42 | ChainArgos/blockchain-nodes | 0881638712a837bdcb90b3cd811a8a9be3aa8d83 | NO-CHANGE (source); HOLD (fleet usage); staged Velnor plan-v1; stock MBX no; fleet MBX host-env deployment unknown. | Fleet owner; prove host-env deployment. |
| 43 | ChainArgos/java-monorepo | 0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0 | HOLD (MBX qualification); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; #2085 cache qualification; CI uses project scripts/Cargo.toml; manual qualification has local test composites. | ChainArgos cache owner; complete #2085 qualification. |
| 44 | ChainArgos/jackin-agent-brown | 6b2ac2277103a22b9ae155bc92415ca5ddf05f95 | HOLD (staged rollout); staged Velnor plan-v1; stock MBX no; #241 staged three-provider rollout; plan run did not complete full workflow. | Brown owner; complete #241 rollout and runner/cache qualification. |
| 45 | ChainArgos/cloudflare-tofu | cc0e2b687dc6c1a9943b0ae9f5911f1e782c1a20 | NO-CHANGE (source); staged Velnor plan-v1; stock MBX yes; MBX qualification pending; current stock MBX runtime. | ChainArgos Cloudflare owner; preserve stock MBX path. |
| 46 | ChainArgos/github-terraform | c8d47ea87a611251f965555dafbd8015f29f303b | NO-CHANGE (source); staged Velnor plan-v1; stock MBX no | Repo owner; exact-artifact regeneration and deterministic parity. |

## Open holds and blocked probes

Checkpoint for each row: 2026-10-11 UTC. Owner supplies a result tied to exact repo/PR/run/SHA or keeps the item visibly blocked with reason. Missing evidence does not become NO-CHANGE.

| ID | Owner | Blocked probe required by checkpoint |
|---|---|---|
| H1 Jackin #1108 | Jackin maintainers; separate Jackin 6.1/medium lane | At #1108 head 2990df17e25f30afca84804d9c402abc1ce00231, trace ci-evidence and ci-push-head-ledger through Mise/cargo xtask. Reconcile validator paths .github-gen/velnor-workflow.toml and .github/ci/.github-actions-generator-state plus generator revision 69 against selected ConsumerV1 output. Record disposition of five failed checks from audit snapshot. |
| H2 Velnor workflow API state | Velnor workflow owner | Reconcile three API workflow records marked active against main 72f0fb14ec8f477fce8a505a624b92837acab85d and #1133 head ef5f10a2b21036de95703c7279e848974ed2dc81. Identify missing paths, stale records, and any dispatch/schedule history. |
| H3 Fixture contract | Fixture and Velnor owners | At main 1c076c5b5828fb6ba04887885c567da26fe01a69 and merged #172 head e296656c5bc804ea3e1a2dc768243c3ce74179d9, decide whether README/GOAL dual-lane contract remains required. Compare it to deleted hosted-only manual/scheduled workflows and current CI; record owner disposition. |
| H4 Java MBX qualification | ChainArgos Java/cache owner | Complete #2085 head baa78037a99c89cde1b9c2201be68e80c9755f67 on hosted and scale-set lanes. Verify writer/reader and push-only export with proposed public MBX 1.21.1 action, including cache/disk/inode evidence. Run 37164041817 was incomplete at snapshot. |
| H5 Brown staged rollout | ChainArgos Brown owner | Complete end-to-end #241 head 092fe1db796367479029d4e94379530f7f5965c0 across three-provider plan/execution. Manual run 35592492823 uploaded a plan but was cancelled overall; verify MBX execution and inspect external Velnor action revision 1048337062ea625fada1b4f7c07f2feed75f60c7. |
| H6 Fleet host settings | ChainArgos fleet owner | Prove whether config/fleet/velnor-host.env values MBX_GC_MAX_TOTAL_SIZE=50GiB and VELNOR_MBX_GENERATION_BOUND=6 are deployed on fleet hosts and identify active contract. Repository text does not prove deployment. |
| H7 Stock MBX qualification | Velnor cache owner + affected consumer owners; ChainArgos runtime/cache owner for peak evidence | Fifteen current CI heads use stock MBX. On the exact final immutable Velnor artifact and supported runner family, capture hosted writer/reader reuse and affected ChainArgos peak bytes/inodes. Keep qualification PARTIAL until supported lifecycle/resource evidence passes. |
| H8 Additional archive refs | Velnor owned-source/archive owner | Fresh union addendum [consumer-retirement-union-2026-10-04.md](consumer-retirement-union-2026-10-04.md) and its per-head CSV completed 2026-10-04 03:11 UTC. It scanned 46 current default heads and 40 open-PR heads for all nine owned-source refs, 18 reviewed/tip/ancestry SHAs (including 3e462… → 7fd109… → 1cde36… → in-scope ac6ceed…), and generic path forms; no exact owned-source ref/SHA hit. This is time-bounded and excludes closed PRs, other branches, and binary payload. Recheck recorded heads before deletion and route for owner review by 2026-10-11 UTC. The 02:27 result remains limited to its original six refs and 15 SHAs. |
| H9 Final artifact and rollout | Velnor release owner + each consumer owner | Bind final source SHA, supported targets, action/runtime pin, manifest and downloaded asset digests. Then regenerate each consumer twice, record changed-file scope, applicable checks, PR head, and post-merge main run/attempt. Current manifests remain on 0.1.0 sources c57/d408; 478 is not the migration target. |

## Companion CSV and boundary

consumer-scope-audit-2026-10-04.csv retains the original 02:27 snapshot’s 46 rows and Appendix-C order. Its 40 open-PR head values were refreshed from the 07:42 snapshot for the two moved PRs; the other scope-audit fields preserve the earlier evidence. The 86-row retirement-union CSV records all 46 default and 40 PR-head snapshots plus the refreshed per-ref scan results.

This is a passive evidence commit only. No consumer, generator behavior, release, PR, or branch changed. No tests/builds run. No consumer-repository worktrees created.

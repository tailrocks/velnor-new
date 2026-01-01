# CI performance source-progress and history record

Status: append-only context for the completion ledger. This document preserves
earlier blocker snapshots and publication records, then adds the current Exact47
accession. It does not replace the 47-row table or grant qualification. All47
remain INCOMPLETE; controlled experiments remain NOT_PERFORMED.

## Retained prior blocker snapshot

The following blocker record is preserved from the prior ledger snapshot. Its run
and source identities remain historical; newer Exact47 observations below are
separately bound and do not retroactively alter those results.

## Current observed blockers

Historical [source-bound progress supplement](ci-performance-source-progress-b0df.md)
records failed PR run `37099764713` and two sealed immutable source publications.
Runtime, hosted reuse, consumer rollout and all47 performance remain unqualified.

Later pushed units are `ec89e630cde905754669e269a85fdccb2072fb11`
(isolated Git clone fixture), `eefb3187360ab50b57c35fe2607ad8965fa7ccfe`
(SHA test-build optimization), and `1dcd0791e90abb0e5b758556f0746d0f061c994e`
(closed source publisher with replayable proofs). All have DCO signoffs and Codex
coauthor trailers, no cryptographic commit signatures; all are ancestors of the
recorded remote qualification branch. Source-publisher implementation supplies
no hosted tool/generator runtime grant or consumer performance qualification.

Bounded hosted attempt-1 observations supersede the older CI blocker snapshot:

| PR head / integration checkout | Run / orchestrator job | Orchestrator suite | CLI / Required / baseline |
|---|---|---|---|
| `ec89e63…` / `adab4bb576cdd7936961d203e0a523d6d2005341` | [37094038593](https://github.com/tailrocks/velnor-new/actions/runs/37094038593) / [111120453098](https://github.com/tailrocks/velnor-new/actions/runs/37094038593/job/111120453098) | 1061 PASS, zero skipped; 551.061 s | 17 stale freshness entries; CLI FAIL, Required FAIL, baseline skipped |
| `1dcd079…` / `22d9aa6a60b00febe7c8af30b5354c4fc56f723d` | [37096868863](https://github.com/tailrocks/velnor-new/actions/runs/37096868863) / [111128721442](https://github.com/tailrocks/velnor-new/actions/runs/37096868863/job/111128721442) | 1061 PASS, zero skipped; 57.898 s | 17 stale freshness entries; CLI FAIL, Required FAIL, baseline skipped |

Both use base `c57c700…` and image `20260927.149.1`, but distinct runner IDs,
changed source/context and 11 additional publisher Python files. All 1061 passed
test identities match; the 15 unsafe-manifest negative cases remain. These are
unpaired observations, with CPU model/features/count and actual hosted compiler
optimization flags unavailable; no paired Linux speedup, percentile, qualified
runtime or consumer performance claim follows. Declared SHA test optimization is
3 in the later source; local compiler-artifact proof remains separate.
Private `hosted-1dcd/comparison.json` under `/tmp/velnor-sha2-profile-ec89/` has
SHA256 `849fc7456c9af6ea253a1a9cc1f80eeaa37db71d27e92bec6e8d44a20150907e`;
actual independent `review.json` (not the earlier navigation name
`independentreview.json`) has SHA256
`f676dde3075763d1afeaebc4706e4e56237cc0516f673ea84555d683b72cf5cd`.
Reviewer `orchestrator_hosted_hotspot/profile_review` cleared this bounded proof.
PR receipts `/tmp/velnor-pr12-ci-evidence-{ec89e63,1dcd079}.json` bind raw APIs,
workflow bytes and archives; independent rehash found zero mismatches in 21 files.

Local MBX native receipt `/tmp/velnor-mbx-native-exportv2-final/execution-receipt.json`
has SHA256 `6872edc5d57276885af7834d482bf228cd1cb142405680f2b3f58ed18ab921f5`:
interfaces passed, store 127 PASS and Cargo library 32 PASS plus a nested one-test
run. Four CLI/lineage/useful/session `--bin` filters each executed **zero tests**;
command success does not prove those suites. Status is
`commands_passed_coverage_incomplete`, `qualification_passed=false`; corrective
`--lib` recipe remains unexecuted at this receipt boundary. No runtime grant.

Root-index audit `/tmp/velnor-index-reconcile-1dcd-1791002992307301000.json`
(SHA256 `5d4b12ead16f15aed086fd6f4ce7df40284c1288e1b4f5198b8920013bfc556f`)
retains raw drift: 11 publisher entries changed stat fields, while all 1188
semantic entries match the exact `1dcd079…` tree. Actor attribution is unproven;
the recorded semantic equivalence does not erase or attribute the raw change.

The earlier recorded commit `6209c06…` fixed Git branch grammar and pinned-gate
issues; it is not the complete remediation source. Its [PR CI run 37075875245](https://github.com/tailrocks/velnor-new/actions/runs/37075875245)
completed **failure**, with API head SHA
`6209c06d87c2f7e41ff162b77cc51e0c99989eec`. The metadata below was read on
2026-10-03; it is not a full raw-log performance audit of that run.

| Job | Actual result / failing step | Exact job |
|---|---|---|
| Plan | success | [111065538672](https://github.com/tailrocks/velnor-new/actions/runs/37075875245/job/111065538672) |
| Rust / orchestrator | failure / Clippy | [111065889616](https://github.com/tailrocks/velnor-new/actions/runs/37075875245/job/111065889616) |
| Rust / CLI | failure / Unit and integration tests | [111065889728](https://github.com/tailrocks/velnor-new/actions/runs/37075875245/job/111065889728) |
| Required | failure / Merge reports | [111066274947](https://github.com/tailrocks/velnor-new/actions/runs/37075875245/job/111066274947) |
| Publish baseline | skipped | [111066349950](https://github.com/tailrocks/velnor-new/actions/runs/37075875245/job/111066349950) |

Current implementation still requires an integrated reviewed source freeze and
all repository gates; generator default-branch runtime qualification is absent.
Native hosted artifact qualification, build attestation and source-bound generator
runtime promotion remain uncompleted dependencies. No qualified runtime has been
distributed to consumers. Cold/warm/third and changed/negative hosted experiments,
compiler/download/cache/queue telemetry and final obligation equivalence remain
pending for all47.

The [owned publication record](ci-performance-owned-tool-publication.md) and
[actual source-only publication receipt](ci-performance-owned-source-publication.json)
now establish two published immutable source releases, with all ten assets
independently downloaded and hash-checked:

| Source | Exact source/tree | Source-only release |
|---|---|---|
| Mise | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` / `d5eefb0470da013d4f524555df763f16a0faf22d` | [402229309](https://github.com/tailrocks/velnor-new/releases/tag/owned-source-mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96) |
| MBX action | `c3cbe8e56ccb4727624df45022357f49d2953075` / `57a9336f26b9ce4a31f5f914c17594ecaa9c1248` | [402232689](https://github.com/tailrocks/velnor-new/releases/tag/owned-source-mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075) |

Both owned source refs are reachable; source-ref pushes triggered zero observed
workflows. Release tags target reviewed generator `c57c700…`, excluding foreign
source triggers. Historical action `06f353…` remains withdrawn for protected-save
ordering. The Mise draft lookup failure was recovered by exact witnessed IDs;
no replacement upload/tag occurred. Later latest-release metadata correction also
made legacy release `401790118` immutable through a server policy side effect;
the receipt retains that observation and verifies unchanged asset identities.

These source commits have DCO signoffs, no cryptographic commit signatures.
Behavioral qualification and signed build provenance remain `null`; source-only
assets do not qualify optimized tool binaries, action behavior, production owned
distribution, generator runtime or performance.

[Protection execution](ci-performance-protection-execution.json) independently
records protected main/tags, strict `Required`, no standing bypasses and enabled
immutable releases. Authoritative IaC [PR39](https://github.com/tailrocks/github-terraform/pull/39)
merged as `667eae4d8ecedde5153eb486fefe07a9d00cc3ad`; its resulting default-branch
[CI37079136334](https://github.com/tailrocks/github-terraform/actions/runs/37079136334)
passed OpenTofu and Required. This is the policy repository's CI, not generator
runtime qualification. Existing generator W0 Required failed; new exact reviewed
generator source must pass its own checks. Protection is not retroactive proof
for old caches, baselines, assets or source provenance.

Generator native source-population review binds private
[corrected native source map](ci-performance-generator-native-source-map.md):
`generator-source-map/source-map-24d2d53886ed5fa9497d16992915f26015f95b1ae74aae42a4b676e83c1d54d1.json`,
SHA256 `24d2d53886ed5fa9497d16992915f26015f95b1ae74aae42a4b676e83c1d54d1`.
The prior `967dccf2…` map is retained at its immutable SHA-named path; its floating
PR metadata/diff mismatch is superseded by the reviewed correction.
Independent reviewer `closure_inputs/generator_map_review` verified 124 immutable
source references, five full tree inventories and exact workflow events/job/step
dependencies. Each W0/current attempt maps 47 Rust phases plus 11 control phases,
with 15 executed raw job logs and one unexecuted baseline job; unavailable logs
are zero for these bounded attempts. The current attempt actually checked out
integration `9927a4907c1b3aeb4cc2043df823c62cdcd80935`, whose tree equals the
recorded PR head tree. The later finite W0 audit below supersedes the prior
source-reading limits; failed jobs, skipped baseline and all qualification
gates remain distinct.

The [generator runtime audit](ci-performance-runtime-audit.md) now closes its
**finite W0 evidence audit with explicit unavailable observations**. Immutable
private closure `whole-w0-closure-0947e2e94a4ac1e54595c32d5a7f2a2308d1a0c5ee43ba29088b8ed12a1dd64f.json`
binds evidence manifest `whole-w0-evidence-manifest-01dc3760ac94e32ebe469db8d5439e9bd7b4589775e3fc4f1711e2e770cce361.json`
(716 files; 166,890,887 bytes). Final independent review
`whole-w0-review/independent-review-385c3087cf7446c64886c008172ccb50db8b34a3bc2e3709cdde000dd85b2c91.json`
subsequently checked both records.
All are SHA-named under the private `generator-source-map/` root. The audit
retains 11 complete relevant PR diffs, 101 complete semantic files, 33 complete
workflow files, seven pinned action sources, 30 executed logs and 22 archives
with 230 extracted payloads. Retrieval gaps are zero; original frozen archives
remain unchanged. Identity-only inventories are not claimed full semantic reads.

Skipped baseline, no recorded freshness runs, empty task-output telemetry,
unemitted executable/compiler/queue measurements and attestation HTTP 404s retain
explicit dispositions. Both Required attempts failed. G14/G17 semantic input
proof, supported Mise task delivery/replay, ToFu/cache behavior, later ninth
Rust member/eight CI lanes, final feedback and controlled T01–T26 qualification
remain OPEN. This audit closure grants no runtime, rollout or performance proof.

## 2026-10-04 addendum: source-set capture and hosted observation

This addendum supplements the recorded source and run evidence. It does not replace the immutable W0 source SHAs in the Exact47 table. Rechecking `scope.json` and `repository-evidence.csv` at source `0efb5565dd33a70b67fc2198d032153f6c5bee9f` confirms 47 ordered rows: generator G1, Wave A8, Wave B33, Wave C5. The existing performance table remains 47 `INCOMPLETE`, zero `PERF_VERIFIED`, zero `STATIC_ONLY`, zero waived, and zero inaccessible. This addendum grants no waiver or qualification.

### Consumer W0 `.github` source sets

The retained migration-PR source-set packets are `consumer-obligations/w0-source-sets-wave-a.json` (SHA-256 `09292c7671f900378a004c303adb390fe675994ed5b99ca6bce05d27fbb6a31a`) and `consumer-obligations/w0-source-sets-wave-b-c.json` (SHA-256 `0828d2f60b71f0c2f5e301a4bc3bb58a1326a5ea09695bf37250a543ebb41c8b`) under the recovery evidence directory. Together they carry repo-keyed source sets for all 46 consumers and record a migration PR for each. Independent source-completeness review and local rehashing verified 384 `.github` files, 224 parsed workflow files, 1,240 historical job rows, all file byte lengths, SHA-256 and Git blob SHA-1 values, and untruncated Wave B/C trees. The only PR-number difference from `repository-evidence.csv` is `tailrocks/github-terraform`: the source-set packet records migration PR [#37](https://github.com/tailrocks/github-terraform/pull/37), while the inventory records later pin-update PR [#38](https://github.com/tailrocks/github-terraform/pull/38). Their titles and distinct purposes reconcile the numbers; both diffs and associated runs remain in Appendix A review scope. Ten Wave B/C base snapshots contain a complete empty `.github` set. For `ChainArgos/jackin-agent-brown`, PR #243 is recorded with head `9b85ac956fc917df97198dd6df67f844d41d0c84`, merge commit `6b2ac2277103a22b9ae155bc92415ca5ddf05f95`, and captured source revision `3e128bd21ab9294841abb1c25f7ebe2d03a5b89f`. This resolves the prior PR-discovery gap only; no complete diff audit is claimed. These are frozen PR-base snapshots, not assertions about current repository heads.

This closes the raw `.github` source-capture gap at those 46 frozen boundaries. It does not complete Appendix A's current-head workflow/config/required-check and release/CD review, full relevant PR-diff review, representative run audit, or controlled benchmark. The V1 completeness gate still requires the implementation to preserve or explicitly disposition all 160 captured nonworkflow `.github` files. Implementation must use a trusted closed source-set registry and map every historical job exactly once to a typed workload, control, or reviewed retirement; a caller-supplied `complete=true` or self-computed digest is not completeness proof. Reviewer's implementation review is pending.

### Retained hosted measurement for source `47815c83b9eeadbaf84b741918fffa7ea550da89`

Use corrected collector v2. Its freeze manifest `hosted-phase-measurement-v2/freeze-manifest.json` is SHA-256 `dff15d573a5f5d8c3c119dce26ca45ae69dac648a1bc7acf069a3cc960e40878`; all eight bound artifacts rehash correctly. Receipt `receipts/main-37163556069-a1-v2.json` is SHA-256 `3c5eea096298abf9b9f4156aa735b440a0a17a857e9b18b2575c57e6133b9640`; independent review `hosted-phase-measurement-v2-independent-review.json` is SHA-256 `b3965d6f2edbc661ee91935d3b0a1f7aa2a8095594d23008787ce4e25fb7dae1`. The review verifies the source-bound collection, phase map, raw-log joins, cache-counter accounting, and DAG path. Compile and 15 unit tests passed. The actual push run `37163556069`, attempt 1, source `47815c83b9eeadbaf84b741918fffa7ea550da89`, had 20 successful jobs and 420 API step rows. Its final report passed for 71 selected/executed tasks. This is ordinary green CI and one observational measurement, not a controlled cache or performance experiment, nor evidence for source `0efb`.

V2 reports a computed DAG path of 808,000 ms; the job-span envelope is also 808,000 ms. The path uses 78 API step rows: 76 active rows joined to raw logs and two skipped rows. Log-reported restore counters are Cargo source 873,285,907 bytes, MBX bundle 909,622,593 bytes, and MBX object 29,062,963 bytes. Log-reported save counters are MBX bundle 69,947,379 bytes and MBX object post-upload 29,062,963 bytes, for 99,010,342 bytes total uploads. API step-span sums over workers are 130,000 ms for 13 MBX object-restore steps and 113,000 ms for 13 MBX object post-upload steps; these sums can overlap and are not isolated transfer durations. The 45 MBX object-summary observations report zero remote download/upload counters, but do not measure compiler freshness or all cache transfer. Runner queue, isolated cache transfer durations, tool-download bytes/duration, Cargo-download bytes, link duration, fresh compiler units, lock wait, and hosted native CPU remain unknown/null. The 71 legacy task-report phase fields remain zero/null sentinels, not zero-duration measurements; per-crate parent artifact ZIP digests are unavailable because those ZIP bytes were not retained.

The v1 receipt and erratum remain preserved. Erratum `hosted-phase-measurement-v1-erratum.md` (SHA-256 `a0b4bcd4cb80b6c9ce2a0e6ae28770457242e71ab5ddbecff7979177764a4800`) records that v1 omitted the 29,062,963-byte MBX object upload, assigned the 113,000 ms post-upload phase sum to restore, and counted two skipped rows in its joined-step total. V2 corrects these classifications; use v2 for the metrics above.

### Published source progress and SourceOnly reference

Source commit `0efb5565dd33a70b67fc2198d032153f6c5bee9f` was pushed to `perf/cache-selection-qualification`; it is DCO-signed source progress. The independent source review binds the final source tree but records 2,781 passing Nextest tests, one `uv` freshness failure, and one skipped test; full gates remain red. Neither the source push nor this run establishes runtime publication, activation, or consumer adoption.

MBX parser commit `c0996a090ae992de83a5131889e28342a04f4d9b` is published at `owned-source/mbx/c0996a090ae992de83a5131889e28342a04f4d9` as SourceOnly. Its independent audit confirms the remote ref resolves to the exact commit and the prior refs remain unchanged; the publisher wrapper separately reports a local postcondition caused by Git's expected remote-tracking-ref update. The packet records focused parser tests and a locked offline binary build, not a full workspace run, Velnor runtime integration, or performance qualification.

### Gate disposition

The W0 capture strengthens C07's source inventory and the static Appendix A evidence boundary, but no C gate closes: trusted registry consumption, complete obligation mapping, execution before expensive setup, and the required hosted proof remain open. C01-C06 and C08 gain no acceptance proof from one successful generic CI run or cache counters. C09 gains a frozen collector, independent review, and one source-bound observation; candidate-source qualification remains open. None of T01-T26 is a controlled experiment represented by this push run. Appendix A's per-consumer current workflows, migration diffs, required checks, CD/release obligations, representative runs, and controlled warm comparisons remain incomplete. Unknown observations stay unknown; no access limit or waiver is inferred.


## 2026-10-04 addendum: observer artifact-closure source publication

Commit 2a2dab0e04a7da18975ca634e505eb9d72dc2e58 (tree b7df358f0dce318f5cc5e7936897792122722aa4) is a direct child of source progress 0efb5565dd33a70b67fc2198d032153f6c5bee9f. Its bound diff contains exactly 18 additions, zero modifications, and zero deletions. Publication used the v3 exact-parent publisher, with git commit -s and hooks enabled; commit receipt 43c801a9b0b88467a6c3bd3456b44d75d17abbfd5fe3614f4c743b0d92273503 records the DCO and Co-authored-by: Codex <codex@openai.com> trailers. Push receipt 974274d0f9573f717a00f3ca53e0c33bb3b8431cfe8578b6474e4059ba94e459 records exit 0 and the explicit non-force --no-follow-tags push of that exact SHA to git@github.com:tailrocks/velnor-new.git, refs/heads/perf/cache-selection-qualification. Its captured remote refs place that branch and PR head at 2a2dab0e04a7da18975ca634e505eb9d72dc2e58; GitHub also recomputed the PR merge ref. The v3 manifest is SHA-256 2c53660b64b5740edb04cce5bd9b6a60eaaa90d14a2b15388588c67ee2a4a46c. Independent actual-publication audit was pending at this addendum's evidence cutoff.

The 18-file source packet adds the observer artifact-closure implementation, its fixture and focused test files, and a short review note. Independent source review 92323acd8d1434af8e5fc03d42f25ad989570aba45f9ff43d30e0a64731d55a found no source issue in 17 code/fixture files; it did not rerun tests or execute Rust. The separate local fixture receipt a15196c06cddddc81a4701cfc29dc7a35973a3cee2dd77b6cfde77b1db56005a and independent review f27ca59ee5e81a7371773da30bc741fa521475ca73bba3e947fd1f7a488d787a are limited to the local fixture. That packet records 17 focused Python tests passing and demonstrates fixture search-root recovery plus rejection of an injected type error; it is not MBX runtime, hosted CI, T06, or performance qualification. Publisher verification recorded 41 commands, 59 assertions, and zero failures; these validate the publication transaction, not the Velnor workspace or runtime.

This source-only publication closes no C01-C09 or T01-T26 gate and changes no exact47 performance state. The ledger remains 47 INCOMPLETE, with no PERF_VERIFIED, waiver, or inaccessible disposition. It establishes no runtime activation, consumer adoption, controlled cache experiment, or current-source hosted qualification. Metadata whose raw bytes were later fetched and hash-verified remains source evidence only; it was not executed qualification. Later .firecrawl raw research captures were written in MAIN; this addendum makes no claim of whole-worktree physical conservation.


## 2026-10-04 addendum: independent publication audit and PR check

Independent read-only actual-publication audit 789a9e9e-2381-4616-b204-66d6fd9afa56 passed at 2026-10-04 06:51:28 UTC. Audit JSON SHA-256: b549741826d666b0ada0bbb3c552b803b8d120b9e34bc769b05f37cfa304f260. Its 128 assertions passed; it re-read the raw commit/push commands and streams, validated the 18 added blobs against SHA-256, size, and mode, confirmed the exact parent/tree and DCO/Codex trailers, and independently read the live target branch at 2a2dab0e04a7da18975ca634e505eb9d72dc2e58. The requested push changed only the target branch; GitHub also advanced refs/pull/12/head and recomputed refs/pull/12/merge. Publisher worktree/source/index maps remained equal across push. Reviewer made no MAIN/index or publisher-repository mutation and ran no Cargo/Rust command.

The source commit did not produce green PR CI. Run 37183335691, workflow CI, event pull_request, attempt 1, is completed/failure on exact head 2a2dab0e04a7da18975ca634e505eb9d72dc2e58; PR 12 base is 47815c83b9eeadbaf84b741918fffa7ea550da89. Run API and jobs records, plus the live PR 12 record, establish the PR association. Their filenames begin pr-28 due an inventory-label error; those names do not identify the run's PR. The raw log ZIP is 132,024 bytes, SHA-256 33ba50edda622bd802460b42e39a3f2f8ac43b9ce8cb976574160c11a5e8fee6; all 77 entries pass unzip integrity.

The Plan job failed generated-file validation with unclassified_repository_suite:mbx-synchronous-registry-fixture and uploaded no plan artifact. Alint failed on “First-party Rust sources MUST live under crates/” and “Stray Cargo.lock. Only the root lock and crates/velnor-runner/Cargo.lock are resolved.” Required then failed while merging reports; 13 Rust jobs were skipped; Publish baseline was also skipped. This is a current-source integration failure, not a successful run or performance measurement. Resolving the fixture's repository discovery and Alint placement findings is required before this source head can produce green required CI.

The audit pass validates the commit/push transaction only. The CI failure does not change the exact47 performance table: all 47 rows remain INCOMPLETE, with zero PERF_VERIFIED, waived, or inaccessible states. No C01-C09 or T01-T26 gate closes, and no runtime, T06, hosted-current-source qualification, or consumer adoption is established.


## 2026-10-04 addendum: Exact47 current audit index and source boundary

The frozen Exact47 index at /Users/donbeave/.codex-chainargos2/evidence/velnor-pr12/recovery-20261004/appendix-current-audits/exact47-current-audit-index-20261004T100005Z-v1/ is the current source-audit accession. Its scope source is commit 0efb5565dd33a70b67fc2198d032153f6c5bee9f. Index hashes: rows.json SHA-256 d18fd25c8b467a0f63df1dde7b48ec82e203f2d0d998aa6de84ef7b8f3c9eed8; report.md 4e8848fb53c8e0077ad16c84674831463dd87c5a76cd2e7b005ce12cd7cdb022; manifest.json 572e73a19cfceeca9fd103d79bd24d987c07cccee72f419f29851fcd3d9b6a40. Recorded input hashes bind goal 6c9d2429508ee07b2a35d271419e55718f048d0d47a59392ea144f2d94b58309, spec a3a50d0ffa638c3a0fd8a8587454342b5cb14e4ae4a1bf1d27cbd1936449b4fb, scope 3f82007f8f57e2fc31ce642666a877f4b4034f7d38aa67f73ee3ecce74d50868, repositories 03ca56ac023196f681551676cef5860a209ae2b388439ac6e144b95025a21883, and CSV 84a4bbf37713fdc7b5283da61eedef5b655d57655933c234a78e36456d3d708a.

The index has the goal's exact generator-plus-46-consumer order and unique repository selectors. All 47 rows are INCOMPLETE; all 47 controlled experiments are NOT_PERFORMED; there are zero PERF_VERIFIED, STATIC_ONLY, waived, or INACCESSIBLE final states. Unknown performance fields remain unknown. Independent review parent/exact47-current-audit-index-independent-review-20261004T103734Z-v2.md, SHA-256 86db58370b6e8d5d2b6ba5b50a87b73fe57a11784bb1d6be0fafba30588eb47a, found no index integrity error: seven packet manifests cover 24,621 files / 1,082,086,843 bytes with all size and SHA-256 checks passing, and every row selector resolves uniquely. This review qualifies the index only as a hash-consistent status projection, not success, static qualification, or performance proof.

The W0 source inventory contains 384 .github files, 224 workflow YAMLs, 1,240 historical workflow jobs, and 160 non-workflow .github files. There are 225 paths under .github/workflows/; the extra path is tailrocks/velnor/.github/workflows/AGENTS.md, correctly classified as non-workflow. The current worktree CSV has CRLF on all 48 lines (17,797 bytes); the source commit blob is LF (17,749 bytes) and matches the index's recorded SHA. Complete inventory is not duty parity: all historical jobs still need exact-once workload, control, or reviewed-retirement mapping; all non-workflow files need preservation or explicit disposition.

### Current lost duties and custody limits

- G1 is generator tailrocks/velnor-new. Exact PR #12 head cabf115bc74f48321c8bfe29288e8574c4020043, base 47815c83b9eeadbaf84b741918fffa7ea550da89, run 37191548281, attempt 1, failed. Its 77-entry ZIP passed CRC; SHA-256 475ab2730a85308e2442a0c1eaa460f91885514f01bcd28caee3ddcc2c6f8cc7. Plan failed the unclassified mbx-synchronous-registry-fixture; Alint rejected first-party Rust outside crates/ and a stray Cargo.lock; Required could not download the absent Plan artifact. Thirteen Rust jobs and the separate Publish baseline were skipped. This is an integration failure, not a performance result.
- A1 Jackin's current main success does not restore 15 removed W0 validation units, desktop cadence, signing/notarizing/attestation, updater/Renovate, nightly, or maintenance duties. Do not reconstruct the earlier reported 637-entry freeze: its bytes were lost before custody capture. Preserve the hash-verified 677-entry successor packet and custody erratum.
- A2–A8 scope provenance says scope.json was reconstructed from CSV and cross-checked against the goal. Their current trees and individual missing policy, nightly, maintenance, and release duties stay as recorded per row; no common ci.yml is a duty-parity claim.
- B9–B15 retain missing policy/nightly/maintenance controls, required-check gaps, and unreplaced historical allocation/byte gates and optional strict timing gate. Twenty-seven expected skipped jobs have no job-log members; executed-job logs are present. API/access gaps remain unresolved.
- B16–B22 have no controlled comparison. Row 17's historical run 33386437956 logs returned HTTP 404; compiler, transfer, and critical-path metrics stay unknown. Migration PRs lacked formal reviews; the recorded bot comments describe review limits.
- B23–B29 lack the recorded nightly, maintenance, and pull_request_target duties. Row 26 includes Docker fixture and Rust dependency policy; row 28 includes APT release scheduling, signing/source verification, staged publish, rollback, and feed duties; row 29 includes Homebrew formula/local-tap tests. Rows with successful no-task runs do not cover W0 duties. The initial 432-GET pass stopped; partial row 25 was excluded and bounded replacement evidence is selected.
- B30–B35 have no controlled cold/warm/third-run series. Single-run walls, cache counters, or green rows do not qualify speed. The collector did not expand jobs arrays; branch-protection requests returned 404; the selected final row ledger is v2. Queue, CPU, and tool-download bytes are unknown.
- B36–B41 lack a project task catalog; empty plans still install Rust tooling; selected attempts missed Mise cache keys. Rows 37 and 40 retain unreplied post-merge docs-parity findings. No prior workflow existed at the migration base, so this audit finds no prior job removal. Final r1 supersedes a18f.
- C42–C46 retain repository-specific loss: row 42's DCO requirement has no DCO workflow and 36 Docker validation units / 147 image-release jobs are absent; row 43's current main run failed five MBX cache exports with ENOSPC and its 55 non-Rust/self-hosted task family is outside generic Rust lanes; row 44's required ci-required context mismatches generated Required; rows 45–46 still lack source/tool or required-context/dispatch proof for ToFu. Twelve initial raw stdout responses were overwritten by duplicate-path reads and are unrecoverable. The corrected ledger identifies them; later responses, stderr, extracted logs, and run ZIPs remain hash-bound.

Other limits remain row-bound in rows.json: B9–B15 API/access uncertainty; B16–B22 missing historical log; B23–B29 stopped collection; B30–B35 collector and status gaps; B36–B41 superseded packet; C42–C46 overwritten output. No unavailable evidence becomes success, no waiver becomes a pass, and no access gap becomes INACCESSIBLE without its defined final disposition. Queue, CPU, bytes, compiler, transfer, critical-path, and controlled-cache metrics stay null until measured.

### Latest local gate attempt: private base plus frozen source overlay

Attempt 4 ran against a private candidate based on head
`7c52bbda8ae9433228180c2ddc63af8513552a0b` / tree
`9b0ea7e28c40192739c797edfa7067ae975a3df0`, with a frozen 41-path working-tree
overlay and 57-path source union. The source map
`source-map-7c52-typed3-v1.json`, SHA-256
`6dbbdba8111cd2b1d7b9a5a62279bf3a9851a9b8784f0b30d6c9a7972442d5f4`, records
the overlay, unchanged HEAD/index/tree, and complete union. The execution receipt separately records unchanged refs across gates. Its 41-path allowlist
tar has SHA-256
`a7f451b0d169eeea08ec6f12b419354d5f58032ca57a4aff099d5b637b5f4944`; the
57-path source-union tar has SHA-256
`f39cebddbbfe7eabb4dd0ef6c075deffc8b4535b0f6c5cf4d9fce9c6ff70957f`. The
source correction patch has SHA-256
`139f419ca7092be7e0f1f87caa319d8cb296ec776780bdc67e5d8ebdbd8eb761`. Thus the
receipt's candidate head/tree identify its Git base; those IDs alone do not
identify the full test source. The execution receipt is
`assert-empty-v7c52-typed-slice-fullvalidation-20261004T1027Z/rust-gates-attempt4/execution-receipt.json`,
SHA-256 `54ab1f338fb9fc4837f3bc7beee781d077520099e471dce0f05326065001570b`.
The raw Nextest stderr is `03-nextest-no-fail-fast.stderr.log`, SHA-256
`e9b73cfd7fccb6f98b92ca9007665da305c2310b0b8d715dab9b9d93864f7a49`; corrected
failure analysis v2 records 16 failures among 2,782 run tests, 2,766 passed, and
1 skipped.

`fmt` and Clippy passed; Nextest failed. The frozen `failure-analysis-v2.json` reports 8 source/fixture-contract failures, 1 source-size failure, 1 stale-freshness failure, 3 unavailable Mise Rust 1.98.1 failures, and 3 missing-`sha256sum`-on-PATH failures. This paragraph records an erratum without changing that receipt: review of the four `p11_metadata` diagnostics shows each failed because offline Cargo could not fetch uncached `anstyle-wincon v3.0.11`; these are environment/cache/setup failures, not source defects. Reclassifying those four leaves 6 source findings and 10 environment/cache/setup failures. The six are three shell/source assertions (`rq211_lock_staleness_probe`, `script_covers_all_forms_scopes_and_namespaces`, and `action_const_wiring_is_mapped`), the 446-line completion ledger size violation, 14 stale freshness records, and the standalone fixture manifest outside `crates/`. The ten environment/cache/setup failures are four uncached offline metadata checks, three missing Mise Rust 1.98.1 failures, and three missing `sha256sum` PATH failures. This ledger split removes the recorded size violation; this status-only review did not rerun Nextest. The attempt remains failed and grants no source qualification, hosted runtime qualification, performance, or adoption.

### Source publication ancestry and transaction receipts

Publication records are source-only. CABF commit cabf115bc74f48321c8bfe29288e8574c4020043 (parent 6b1632d4b6bdd39e348ca33ac701241cd987b921, tree 0f34efee5f76b1885c5932de6133e2cfce18e1d1) has commit receipt parent/freshness16-publication/commit-3877dfa5-dc84-4d4c-a931-6cea715ff668/receipt.json (SHA-256 b6ca5c01ed7660cbc1e9f4be9bf90edd56ccb56b7b85579bbb21abf90a759dcd) and push receipt parent/freshness16-publication/push-923afc52-8785-4199-92a5-33933023f773/receipt.json (0301ad2c17e71a2eff2c622816ccaafe387651f2946e05c73a8e77b2c76443b6). Its actual-publication audit parent/freshness16-actual-publication-audit-20261004T091836Z-5661ca5f/audit.json (5962aecc4cd52da139d3aa3a1a2dcf30080bd2af3be7f658a6965367a11a7b67) records transaction PASS.

Child 7c52bbda8ae9433228180c2ddc63af8513552a0b (tree 9b0ea7e28c40192739c797edfa7067ae975a3df0) has receipt parent/freshness-four-tests-publication/a44fc683-efff-4def-8b0b-2d1502f1adda/receipt.json (SHA-256 9e09144f615cc6d850dd94ba00cb79505b3df9f1cd0a7acaa56054d5a9081cb3) and publication audit parent/freshness-four-tests-actual-audit-20261004T093407Z-8c14988f/audit.json (2904bdd8a39dd5f6b7700ea377ad1047b47655e17bf2ab79ed4e6cbfe318ac3d).

Historical policy commit 01c4c7593abf088e75aae2c6966e1f9d6980a7f9 is a direct
child of 7c52bbd. Its source-publication receipt
parent/goal-policy-publication/3e5b9dc4-3934-4219-85ea-19e53ab113fa/receipt.json
has SHA-256 7d2cd5e08f9a4e06053d0fa933ca0abb21361d236bfde0ae058cea35c5d9081c. Its
independent publication audit
parent/goal-policy-publication/independent-publication-audit-01c4c759-20261004-v1.json
has status PASS, SHA-256
f46dc031caf7802e39e70ea463e40ce2926835e5dbf94b8f7917196d1a0e555a; it verifies
that one-path transaction. The 01c4 all-Luna goal wording is historical and
superseded by the role policy below.

The current role policy is published in commit
4817c15f47f2052f762a2f6eff1b0501444ee9e6, a direct child of 01c4. It requires
implementation work to use exactly gpt-6-luna at max, every review to use exactly
gpt-6.1-sol at medium, and the coordinator's existing goal-assigned configuration
to remain unchanged. The source-publication receipt
parent/role-policy-publication/649feb64-2374-4ee9-b31c-b075c5aadc9a/receipt.json
has SHA-256 c41188c1dd9aaeace826638958c87e524a4f42b1660010a92df072fbe217e241.
Its frozen independent actual-publication audit
/tmp/velnor-role-policy-publication-sol-review-4817c15.json has SHA-256
6940322c3585cf4310e3560ff1dee68872a1dcac0d8ffc6a477ff32569f9520b and result
PASS_SOURCE_PUBLICATION. It verifies the exact parent and ref, the source-only
change, and preservation of 1,426 other tracked entries. The receipt and audit
set runtime_qualification=false; they establish policy publication only, not
runtime qualification, consumer adoption, gate completion, or performance.

The index does not close C01–C09 or T01–T26. Remaining gates: repair Plan/Alint/required-artifact failures; reconcile and preserve or explicitly retire every W0 duty/file for rows 2–46; run controlled workload, cache, trust, and release experiments; qualify a source- and binary-bound generator runtime; then stage consumer rollout in jackin-project → tailrocks → ChainArgos order and revalidate after generator changes. No runtime activation, adoption, T06, current-source hosted qualification, or performance result is claimed here.

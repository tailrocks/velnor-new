# CI performance qualification

Status: qualification protocol prepared; hosted experiments remain pending.
This document records evidence and prerequisites, never substitutes for T01–T26.
Scope is exactly `scope.json`: generator plus 46 consumers. Private raw evidence
stays outside repository checkouts; publish only reviewed, redacted measurements.

## Reproduced baseline

Run [37012391691](https://github.com/tailrocks/velnor-new/actions/runs/37012391691)
attempt 1 tested source `c57c700459bbe1549fe7eedcb7d8689585c38986` on a push.
Full orchestrator log, job
[110855475750](https://github.com/tailrocks/velnor-new/actions/runs/37012391691/job/110855475750),
shows rustfmt rejecting `internal_request.rs:305` at 13:22:53Z: the single-line
`Ok(matches!(verdict.status, FinalStatus::Passed | FinalStatus::NoWork))` requires
multiline formatting. Required failed; baseline publication was skipped. Missing
coverage from this run is expected. Do not infer a selector defect from it.

Full contract log, job
[110855475688](https://github.com/tailrocks/velnor-new/actions/runs/37012391691/job/110855475688),
reports Clippy's 1 hit, 0 misses, 51 not-looked-up and 3 `cc-missing-output`
bypasses. Its MBX summary explicitly excludes Cargo reuse and CI archive
transfers. Source archive was 18,244,511 compressed bytes and MBX archive
86,449,650 compressed bytes. These observations establish neither zero real
compiler work nor a qualified baseline. Later MBX save was suppressed after
an exact hit; fresh-run persistence still requires T03/T04/T17.

## Collection commands

`scripts/collect-ci-performance.py` reads completed run-attempt metadata, every
jobs page, full job logs, and all run artifact pages through supported `gh api`
endpoints. It writes mode-0600 files in a mode-0700 external directory. It performs
no dispatch, publication, cache deletion, or merge. `--reuse` reuses immutable
responses; mismatched repository/run/attempt identities fail collection.
Incomplete or inconsistent pagination and cross-run/source artifact listings fail
collection. Reads/writes reject symlinks and multiply linked files; output cannot
be inside any Git checkout. The summary records its creation time and whether
existing evidence was reused; that timestamp is not a fetched log's origin time.

```sh
rtk proxy /usr/bin/python3 scripts/collect-ci-performance.py \
  tailrocks/velnor-new 37012391691 --attempt 1 \
  --output /absolute/private/evidence/tailrocks-velnor-new/r37012391691-a1
```

Keep one output directory per exact run/attempt. Repeated collection without
`--reuse` refreshes API evidence. Logs can expire; missing logs are explicit
unavailable evidence. Raw API/log bodies are never printed by the collector.
GitHub masking does not make raw private logs suitable for public publication.

Job/step wall times are measured from their timestamps. The initial start delay
includes scheduling, so it is not pure queue time. The earliest-job to latest-job
envelope is not the dependency critical path; recompute that from the validated
plan and exact workflow graph. A sum with a missing job duration remains unknown.
Artifact listing is run-wide: inspect artifact names/manifests and attempts before
using an artifact as attempt-bound evidence. MBX summary sessions remain separate;
do not sum repeated counters without checking their owning process/session.

## Hosted cold/warm/persistence procedure

1. Qualify generator fixes locally and independently. Commit generated dogfood
   workflows from the fixed generator. Use a new typed cache generation; preserve
   old immutable cache entries. Record source/config/catalog/workflow digests,
   payload versions, selected obligations, runner class and cache policy.
2. Obtain an authorized trusted default-branch **validation** push using that
   isolated namespace. Inspect the exact generated workflow before execution:
   the benchmark must not run publishing/deployment or mutate infrastructure.
   PR read-only caches cannot populate novel trusted snapshots.
3. T01 is the first complete successful validation run in the isolated namespace.
   Save every raw job log, report, helper/plan identity and cache snapshot. Wait
   for all late producers and post-job exports to finish.
4. Rerun **all** jobs from the same immutable run/SHA for T02, then T03. GitHub
   hosted allocations must be fresh; record runner/image identity from setup
   logs. Both attempts must execute the same required compilation/test domains.
   If complete baseline coverage omits them, the attempt proves coverage only;
   it cannot qualify compiler reuse. Use an explicitly supported conservative
   qualification plan, never a handwritten workflow or invented config flag.
5. Collect each completed attempt with the script. A relevant controlled command
   is `rtk gh run rerun RUN_ID --repo OWNER/REPO`; after completion, read the
   actual attempt number from `rtk gh api repos/OWNER/REPO/actions/runs/RUN_ID`.
   Then collect with that exact `--attempt`. Poll with bounded frequency.
6. Match validation snapshots and compiler/prediction work from T01/T02 to T03.
   Independently compare raw counters, tool payloads, archive transfers, real
   compiler work and output correctness. Only then mark the applicable cases
   passed. Three runs do not support percentile claims.

Current CLI/config inspection found no documented force-full benchmark switch.
Qualification must establish a supported conservative execution path before
dispatch; do not silently turn a coverage-only no-op into warm compiler proof.

## Experiment ledger

Every row needs exact source/base/candidate, run/attempt/job, raw evidence digests,
expected obligations, observed dispositions, independent verdict and limitations.
All rows below are pending hosted qualification unless genuinely N/A with evidence.

| Test | Controlled input | Required independent proof |
|---|---|---|
| T01 | New isolated namespace, unchanged qualified source | Cold success; actual downloads/compiler work; trusted save |
| T02 | Fresh hosted runner, same source after T01 | Complete tool restore; zero redundant payloads; eligible reuse |
| T03 | Third fresh runner, same source after late writes | Late validation state persists; unchanged exports suppressed |
| T04 | Helper release then Clippy/test | Distinct domains; supported workspace/prediction reuse |
| T05 | Reverse sibling completion order and retry | No collision/lost work/concurrent mutable target |
| T06 | Real leaf semantic edit | Leaf/reverse closure; unchanged eligible dependency reuse |
| T07 | Shared/proc-macro/build-script semantic edit | Complete justified reverse closure beyond `src/` |
| T08 | Actual dependency resolution/version change | Missing source fetched; changed work builds; no stale result |
| T09 | Feature/target/profile/flags/linker/compiler change | Correct compatibility invalidation and task identity |
| T10 | Add/delete/rename package or edge | Both graphs used; every required obligation retained |
| T11 | Verified irrelevant documentation change | Compiler jobs omitted before setup; minimal correct gate |
| T12 | Included Markdown/schema/fixture/native edit | Affected work executes; no blanket docs exclusion |
| T13 | Corrupt/missing/unavailable owned cache | Visible safe cold fallback; honest producer outcome |
| T14 | Failed/canceled producer or missing report | Required fails; no trusted state or baseline promotion |
| T15 | Fork/PR/merge-group | Server/event isolation; read-only trusted defaults |
| T16 | Advancing base/candidate or PR rerun | Exact candidate/proof validation; stale proof rejected |
| T17 | Existing exact snapshot plus useful new state | New immutable useful snapshot; no discarded delta |
| T18 | Missing Rustup proxy/target/symlink destination | Owned closure detects/repairs damage before execution |
| T19 | Generator/catalog/action/schema update | Required regeneration; source archives preserve compatibility |
| T20 | Source hit missing selected target dependency | Offline failure triggers supported bounded source fetch |
| T21 | Different checkout/build directory | Supported relocation/fingerprint validation; no mtime spoof |
| T22 | Qualified deterministic task replay | Complete input/output proof; report says reused |
| T23 | Nextest archive where beneficial | Exact source/runtime/fixtures; injected test failure fails |
| T24 | Conservative full vs selected candidate | All obligations accounted; zero selection false negatives |
| T25 | Non-Rust/tools-only consumer | Real checks; no irrelevant Rust/MBX/Cargo setup |
| T26 | Release safe dry-run/provenance | Protected source-bound artifacts; no PR executable reuse |

Run negative cases in isolated fixtures/namespaces and non-production workflows.
Never delete organization caches or damage a consumer's real source/tool state.
Retain real failed experiment outcomes; a retry does not erase failure evidence.

## Required telemetry and interpretation

| Measurement | Available baseline source | Qualification limit |
|---|---|---|
| Job/setup/install/cache/test step walls | Attempt jobs API and timestamped logs | One-second API resolution; command overhead included |
| Archive transfer bytes | Exact `Cache Size` log observation | Import/object sizes are not additional network bytes |
| MBX hits/misses/not-looked-up/bypass | Supported pinned MBX session summaries | Zero misses does not prove zero compilation |
| Actual compiler/link/build-script/rustdoc work | Requires supported tool diagnostics/report extension | Collector retains null; `Compiling` text is insufficient |
| Tool/source payload byte totals | Requires payload/transport evidence | Successful install or missing text is not zero downloads |
| Test-only execution | Actual Nextest/test summary and reports | Step duration also includes test-binary preparation |
| Useful delta and hidden cache version | Pinned transport inputs/output and owning tool | Visible key alone cannot prove payload equivalence |
| Queue/provision/critical path | Event/job/log/plan timeline together | Run `updated_at-created_at` is not execution duration |
| Runner image/resources/load | Setup logs plus supported runner telemetry | Label alone does not prove identical environment |

Extend the existing report contract for missing measurements. Do not build a new
telemetry engine, parse MBX internals, or assume undocumented CLI flags. Retain
unknowns until supported raw evidence establishes a value. Separate compiler
objects, qualified task-result reuse and baseline coverage. Report failures,
excluded/non-looked-up work and legitimate unsupported bypasses explicitly.
Use paired comparable observations, initially at least 20 for percentile claims;
disclose sample size, queue effects, outliers and failures. Investigate warm
ordinary PR critical paths over 120 seconds without shrinking obligations.

## Runtime identity audit

Live read-only audit on 2026-10-03 downloaded the published `v0.1.0` assets and
independently hashed them. Manifest and actual tag both name source
`c57c700459bbe1549fe7eedcb7d8689585c38986`. `target_commitish` metadata instead
names `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5`; existing-tag commit resolution
is authoritative for tag identity, not proof of a binary build's provenance.

| Asset | Measured SHA-256 (matches API/manifest) |
|---|---|
| `release-manifest.json` | `38285a07c4d931980590380e9ff7c5aac9e60245be81bd107d201231b7743414` |
| Linux x86_64 binary | `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5` |
| macOS arm64 binary | `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f` |

Release metadata reports `immutable=false`. Supported attestation REST lookup
and actual `gh attestation verify --repo tailrocks/velnor-new --source-digest
c57c700459bbe1549fe7eedcb7d8689585c38986 --format json` return HTTP 404 for both
binaries under present credentials. No provenance attachment was listed.
Source-bound provenance remains **unverified**, even though byte integrity
matches. Compiled code reports package version and executable digest; a reviewed
manifest's asserted commit is distinct from signed build provenance.

Tagged/current catalog supports `x86_64-apple-darwin` too; published assets and
manifest omit it. Contract requires every supported target. Published manifest
filename also differs from canonical `velnor-actions-release-manifest.json`.
Do not repair these gaps by replacing `v0.1.0` assets or moving its tag.

## Publication and integration gates

Follow [bootstrap-and-release-contract.md](../proposed/bootstrap-and-release-contract.md)
and inspect [release-gates.md](../implemented/release-gates.md) against current
live infrastructure. Goal authorization covers legitimate publication; it does
not fabricate two seed approvals, independent reproducibility, required review,
attestations, or protected release infrastructure.

1. Freeze the qualified default-branch source/catalog/version. Build exact
   candidate binaries through isolated pinned Mise and MBX for every supported
   target. Retain digest, source, toolchain, target and current-run identity.
2. Independent qualification consumes those exact artifacts without rebuilding.
   Finish all required checks, independent correctness/security/performance
   review and feedback dispositions before promotion.
3. Publish through the protected official release process, using a new version
   tag exactly once. Verify uploaded bytes, complete per-target manifest and
   source/workflow-bound provenance. Existing versions remain immutable.
4. Commit byte-identical published manifest updates with independent review;
   generator lock updates are separate reviewed changes. Verify SHA/tag/artifact
   binding before regeneration and execution, not just manifest shape.
5. Regenerate first `jackin-project/jackin`, then the remaining Wave A, Wave B,
   Wave C. Preserve all non-workflow `.github` content and CI/CD obligations.
   Requalify earlier consumers after any relevant generator correction.
6. Use one integration branch per repository; serialize index/commit/generation.
   Read all reviews/comments/replies/threads at final head. Accepted findings
   get verified fix commits and replies before resolution; rejected findings
   get evidence/rationale. No force-push, protection weakening or silent waiver.
7. Verify actual reviewed integration and resulting default branch separately.
   Private merge waivers apply only where current metadata and authority prove
   them; waived CI remains `CI_WAIVED_PERF_UNVERIFIED`. Never force production
   execution to manufacture measurements.

Final closure retains exactly 47 unique rows and distinguishes `PERF_VERIFIED`,
`STATIC_ONLY`, `CI_WAIVED_PERF_UNVERIFIED`, `INACCESSIBLE`, and `INCOMPLETE`.
Neither the collector nor this protocol assigns `PERF_VERIFIED` automatically.

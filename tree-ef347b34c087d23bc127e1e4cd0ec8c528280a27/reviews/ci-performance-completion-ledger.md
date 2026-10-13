# CI performance completion ledger

Status: **OPEN**. Snapshot: 2026-10-03; recorded PR/source HEAD
`b0df793576d020ff5778bfce7efe9093469b7144` plus concurrent uncommitted work.
This pushed commit has DCO signoff, no `gpgsig` header.
No final source qualification, fresh hosted sequence, runtime promotion or consumer
rollout is established by this ledger. Refresh identities after freezing the head.

Authority: full [goal](../../velnor-actions-ci-performance-goal.md),
[specification](../../velnor-actions-ci-performance-spec.md), [scope](../../scope.json),
[ordered repositories](../../repositories.txt), and historical
[CSV](../../repository-evidence.csv). The latest user role policy requires
implementation work to use exactly gpt-6-luna at max and every review to use
exactly gpt-6.1-sol at medium. The coordinator keeps its existing goal-assigned
configuration. Verify actual model and effort from authoritative runtime metadata
before relying on output; requests and self-description are not proof. Missing,
incomplete, or different metadata makes that execution unverified and
nonqualifying. Continue authorized source work. Earlier assignments remain
historical.

The [2026-10-04 source-progress recovery supplement](ci-performance-source-progress-recovery-20261004.md) records the observed recovery boundary, durable Audit47 and merge-source evidence, and pending integrated qualification. It grants no qualification.

The [2026-10-04 actual hosted-run audit](ci-performance-actual-runs-20261004.md) records source-bound Jackin and generator jobs, raw-log hashes, a source-backed inference that differing restore/save paths explain the generator cache miss, and unavailable performance metrics. It leaves all47 statuses INCOMPLETE and grants no qualification.

## Evidence levels

`I` = working implementation exists; `S` = source inspected independently;
`L` = recorded local execution; `H` = exact fresh hosted-runner qualification.
These levels are independent. A fixture or source review cannot grant `H`.
Uncommitted code is not the final reviewed artifact. Every requirement below
remains OPEN until its applicable behavior and distribution gates pass.
Unknown metrics remain `null`/unknown; historical green CI and merge waivers are
separate evidence. No repository currently receives `PERF_VERIFIED` here.

## Current Exact47 audit status

The frozen 2026-10-04 Exact47 index is a hash-consistent status projection of the
goal's generator plus 46 consumers. rows.json SHA-256 is
d18fd25c8b467a0f63df1dde7b48ec82e203f2d0d998aa6de84ef7b8f3c9eed8;
its independent review is
parent/exact47-current-audit-index-independent-review-20261004T103734Z-v2.md,
SHA-256 86db58370b6e8d5d2b6ba5b50a87b73fe57a11784bb1d6be0fafba30588eb47a.
All 47 rows remain INCOMPLETE; all 47 controlled experiments remain
NOT_PERFORMED. Zero rows are PERF_VERIFIED, STATIC_ONLY, waived, or
INACCESSIBLE. Full 47-row scope and each qualification gate remain below.

W0 inventory: 384 .github files, 224 workflow YAMLs, 1,240 historical workflow
jobs, and 160 non-workflow files. Exact-once workload/control/retirement mapping
and preservation/disposition remain open. The current generator observation is
PR #12 head cabf115bc74f48321c8bfe29288e8574c4020043, run 37191548281 attempt 1:
Plan, Alint, and Required failed; 13 Rust lanes and Publish baseline were skipped.
This grants no source, runtime, adoption, or performance qualification.

The latest local gate attempt used a private candidate based on
`7c52bbda8ae9433228180c2ddc63af8513552a0b` / tree
`9b0ea7e28c40192739c797edfa7067ae975a3df0` plus a frozen 41-path source overlay
(57-path union). Formatting and Clippy passed; Nextest failed (2,766 passed, 16
failed, 1 skipped). Evidence review separates 6 source findings from 10
environment/cache/setup failures. This is not source qualification or
performance evidence; exact overlay bindings and the failure-analysis erratum
are in the linked history supplement.

The complete prior blocker record, source-progress history, Exact47 lost-duty and
custody details, and publication receipts are preserved in
[source-progress/history](ci-performance-source-progress-history-20261004.md).
Its evidence does not change ledger status or close C01-C09/T01-T26.


## C01–C09 traceability

Source abbreviations: `R` = `crates/velnor-actions-workflow-renderer/src/`;
`O` = `crates/velnor-actions-orchestrator/src/`; `U` =
`crates/velnor-actions-rust/src/`. Paths describe current implementation areas,
not immutable qualification identities.

| Gate | Current evidence | Owner | Required next action |
|---|---|---|---|
| C01 canonical transport | I/S/L: `R/cache_snapshot.rs`, `cache_p08.rs`, shared roots; actual bundled toolkit execution in [hidden-version proof](ci-performance-cache-version.md) | tools_cache, repair_snapshots | Regenerate exact head; verify identical resolved ordered payload/compression/version on hosted restore/save; new namespace, isolated Mise hooks/config |
| C02 complete Rust tools | I/S/L: `O/rust_tools_prepare.rs`, bootstrap/proxy guards; [local closure](ci-performance-rust-tool-closure.md) | tools_cache, rust_profile, rust_health | Complete damaged toolchain/component/target detection; restore before execution; fresh Linux/macOS closure and repaired snapshot persistence; minimum actual components/targets |
| C03 MBX immutable domains | I/S: `O/mbx_domain.rs`, `attach.rs`; local owner-API development remains separate from published integration | mbx, upstream_delta, upstream_action | Source-qualified supported MBX/action distribution; bounded useful immutable union snapshots, cohorts, reversed writers/retry and late validation persistence |
| C04 single verified MBX | I/S: isolated action executable-path/digest interface development; earlier staged action candidate withdrawn, reviewed replacement source-only release published; current official pin alone does not provide that interface | mbx, upstream_action | Qualify/consume reviewed supported exact integration; prove one Mise installation, no fallback download; action and task compiler/homes agree |
| C05 workspace state | I/S: upstream MBX comparison/retention development; owner-reported local experiments need an independently inspected immutable record | mbx, transport_source, build_dir_scope | Immutable resolved target/intermediate-build roots; supported relocation/configuration history; visible bypass accounting, no raw target cache or spoofed mtimes |
| C06 useful source exports | I/S/L: `O/source_prep.rs`, behavior fixtures; [source closure](ci-performance-source-closure.md), [observer](ci-performance-snapshot-observer.md) | tools_cache, source_closure, repair_snapshots | Selected package/feature/target closure remains conservative complete locked workspace; qualify narrowing or verified contract decision; actual fill/export and source unpack-versus-archive costs |
| C07 early selection | I/S: `O/analysis_*`, baseline/lineage, job predicates; `U/manifest_graph.rs`, `semantic_inputs.rs`; [correctness review](ci-performance-correctness-review.md) | selection, closure_inputs, early_plan, gate | Authenticate persisted inventory; Plan and freshness consume it before Cargo setup; both exact graphs; carried proof/rerun lineage; hosted covered path without Rust/MBX setup |
| C08 scheduling | I/S/L: real Nextest binary preparation [evidence](ci-performance-nextest-tools.md); feature policies | scheduling, rust_obligations, obligation_restore_design | Measure equivalent cohorts/direct-run/archive/shards and costly fixtures; retain separate outcomes, feature semantics, doctests, injected failures and native obligations |
| C09 helper/report path | I/S: current-source dogfood and digest-bound consumer helper; attempt-bound collector [protocol](ci-performance-qualification.md) | early_plan, qualification, release_restore | Measure current-source build/profile/link and artifact/API/checkout/report costs; supported bounded transfers; exact run/attempt/source; no stale helper or missing-report success |

C06's pinned `cargo fetch` lacks package/features selection. The current containing
workspace fallback fixes completeness but does **not** establish optimal selected
closure. Disabled unqualified native caches preserve safety but do **not** fulfill
native performance acceptance. Neither condition is a completed gate.

Owner-reported upstream MBX local sequences in the table below are navigation
leads, not L-level qualification here: record exact fork commit/binary/action
digests, raw experiment paths/digests and independent replay before promoting them.

## T01–T26 experiment traceability

All rows: **hosted pending**. Local evidence supplements, never replaces, fresh
hosted observations. Applicability/N/A needs the audited workload and independent
decision; no global N/A is assigned. Execute through generated supported fixtures,
isolated namespaces and validation flows; never benchmark production publication.

| Test | Current evidence / regression target | Owner | Open observation |
|---|---|---|---|
| T01 cold namespace | [protocol](ci-performance-qualification.md); collector; failing baseline raw archive | qualification | Full successful cold obligations; downloads, real compiler work and saves |
| T02 unchanged warm | Local Rust install sequence; no hosted archive restore | qualification, tools_cache, mbx | Fresh runner; no redundant payloads or avoidable eligible third-party compilation |
| T03 third unchanged | Local observer and MBX owner experiments only | qualification, mbx, repair_snapshots | Late-produced state retained, no unchanged compression/upload; same immutable inputs |
| T04 helper then validation | `O/mbx_domain_tests.rs`; owner retention development | mbx | Distinct useful release/validation snapshots and predictions on fresh runner |
| T05 reversed siblings | Domain isolation test source; no hosted writer-order proof | mbx, qualification | Both orders plus same-SHA retry; no lost state/shared mutable-target race |
| T06 leaf source | `U/semantic_inputs_tests.rs`, reverse-graph fixtures | selection, closure_inputs | Conservative-versus-selected obligations and unchanged dependency reuse |
| T07 shared/proc macro/build script | Both-graph implementation; unknown reads broaden | selection, closure_inputs | Actual reverse closure including generated/outside-src inputs |
| T08 dependency lock change | Source optional/platform defect reproduced locally | source_closure, mbx | Real new version/source fetched/built; unchanged compatible dependencies reused |
| T09 compatibility change | `O/mbx_domain_tests.rs`, task semantic identity/feature fixtures | mbx, rust_obligations | Actual feature/target/profile/flags/linker/SDK/compiler invalidation |
| T10 topology change | `U/manifest_graph.rs`, base/candidate Git snapshots | selection | Add/delete/rename/edge fixtures and hosted complete obligation retention |
| T11 irrelevant docs | Early inventory implementation in flight | early_plan, selection | Valid coverage; no Plan/freshness Rust setup or allocated compiler jobs |
| T12 included input | `U/semantic_inputs_tests.rs`; unknown include/macro reads broaden | closure_inputs | Included Markdown/schema/fixture/native edit executes required work |
| T13 corrupt/unavailable cache | Local observer/sanitizer negatives; owner API failure classification | security_review, tools_cache, mbx | Safe real fallback; optional transport failure never falsifies task result |
| T14 failed/canceled/missing | `tests/impl_required_skip.rs`, action evidence admission, publisher guards | gate, action_obligations, security_review | Missing shard/report or selected failure fails gate; no trusted state publication |
| T15 fork/PR/merge group | [security review](ci-performance-security-review.md), default read-only policy | security_review | Server/event scopes, credential/private-data exclusion, no promotion; optional PR-only policy remains unqualified |
| T16 advancing base/rerun | `provenance_lineage_tests.rs`, baseline retry/carry tests; candidate parent binding | selection, gate, security_review | Fresh exact candidate and original proof run/attempt; stale identity rejected |
| T17 exact plus useful delta | Observer same-byte repair; isolated supported MBX useful-delta experiments | mbx, repair_snapshots | Published integration writes new immutable useful state, not starvation |
| T18 Rust damage | Local proxy removal/repair evidence; Rust integrity work in progress | rust_profile, rust_health | Missing target/component/proxy/destination detected before use and durable repaired snapshot |
| T19 generator/tool/schema | Catalog/delivery pins and source configuration identities | tool_pins, tools_cache, release_restore | Correct invalidation/regeneration without needless source-archive churn |
| T20 source hit incomplete | `O/source_prep_behavior_tests.rs`; actual Cargo metadata false-success reproduced | source_closure | Selected target/optional source missing: supported fill, no hidden offline failure |
| T21 relocation/build-dir | Owner MBX local relocation research | build_dir_scope, mbx | Resolved immutable roots; changed absolute path/configured build-dir supported or explicit evidenced limitation |
| T22 task replay | No qualified deterministic replay established | selection, qualification | Complete declared inputs/outputs/proof and fresh reused report; existing Mise owner only |
| T23 Nextest artifacts | Preparation plus injected failing test local proof; no archive comparison | scheduling, qualification | Demonstrate beneficial archive comparison, exact runtime/fixtures and real injected failure; N/A only with evidence |
| T24 full comparison | Typed Full/Affected dispatch contract, tests in integration queue | selection, nightly_contract, correctness_review | Full obligation parity, real failure injection, zero false negatives |
| T25 tools-only/native | Plan Rust role tests; native restoration/source privacy audit | scheduling, obligation_restore_design, native_caches | Actual substantive native checks with no irrelevant Rust/MBX/Cargo archives |
| T26 release | Native APT/admission local fixtures; [distribution ledger](ci-performance-distribution-obligations.md) | release_restore, security_review | Exact approved source/artifact signing/provenance; source-only trust; safe qualification without accidental publish/deploy |

## Cross-cutting completion gates

| Gate | Evidence / current limitation | Owner / next action |
|---|---|---|
| W0 bounded audit closure | [Scope audit](ci-performance-scope-audit.md), wave audits below, [durable archive](ci-performance-evidence-archive.md); all47 finite W0 evidence rows independently closed: generator with explicit unavailable observations, Wave A eight, Wave B 33, Wave C five; full relevant diffs/fallbacks, representative executable raw logs, bounded attempts/jobs and exact historical/nonexecution limits retained; all47 performance statuses remain INCOMPLETE | Audit owners / all47 bounded W0 evidence rows CLOSED at their recorded source boundaries; explicit unavailable observations retained; refresh actual rollout source identities and qualify C01–C09/T01–T26; private wave closure records bind frozen collection SHAs, not later live heads |
| Cache ownership | Tools vs sources distinct; MBX owns supported compiler/workspace state; native stores separate; same-run artifacts not passed tests; task results disabled absent qualification | tools_cache, mbx, native_caches / verify no overlapping roots, credentials, state or private content |
| Descriptor compatibility §4.1 | Current typed fields/source inspection, not final runtime compatibility proof | tools_cache, mbx / preserve runner image or reviewed schema; actual platform/ABI/compiler/components/config/path-layout/trust; constrained prefixes and useful-export budget/retention |
| Native §4.3 | [Native cache audit](ci-performance-native-cache-audit.md), [obligation restoration](ci-performance-obligation-restoration.md); Node/Bun/Gradle public-source/output proof incomplete; Tofu/Docker separate owners | native_caches, obligation_restore_design / complete public-origin proof and native relocation/cache-mount measurements |
| Complete prior obligation universe | Wave before/after audits identify lost substantive CI/CD/security/policy/nightly/maintenance; typed primitives alone are not replacements | obligation_restore_design, security_obligations, release_restore / map every prior obligation to qualified replacement or evidenced explicit retirement; preserve nonworkflow `.github` and verify emitted/actual execution |
| Baseline §6 | Complete tasks and exact candidate/base; unknown closure broadens; original proof carry and strict publisher identity in flight | selection, gate, early_plan / final source, malformed/cycle/expiry/rerun/empty/missing tests plus hosted coverage |
| Security §5 | [Independent review](ci-performance-security-review.md); cache-mode server support vs pinned validator rejection is documented, not permission to bypass | security_review / final code/generated outputs + T13–16/T18/T26; preserve default PR read-only and release protection |
| Telemetry §8 | Actual compiler CPU/fresh/link/build-script/rustdoc, payload bytes, queue/provision and full critical path remain unknown where unavailable | qualification / extend existing reports with supported diagnostics and exact layer/session identities; redact private data |
| Statistics/budgets §7.3 | Three hosted runs not yet done; no p95; local timings only local | qualification / paired comparable sample initially20 for percentile claims, disclose outliers/queue/failures; investigate warm >120s; transport costs vs avoided work |
| Deterministic repo gates | Focused local checks exist; historical recorded PR head `b0df793576d020ff5778bfce7efe9093469b7144` CI failed (CLI stale17, Required); final integrated fmt/clippy/nextest/alint/deny/freshness not yet established | parent / serialize complete checks at frozen head, record commands/results and failures honestly |
| Official runtime | [Runtime audit](ci-performance-runtime-audit.md): old asset integrity matches, provenance incomplete/missing supported target | release_restore, qualification / qualified actual default branch, complete supported-target assets, exact source/digests/attestation/approvals; immutable new version |
| Consumer waves | All47 below remain INCOMPLETE; no qualified generated rollout completed | parent, wave owners / first Jackin canary; A then B then C; requalify earlier waves after relevant generator changes |
| Final feedback/closure | Earlier bounded audits do not approve final changes | parent, independent reviewers / re-fetch reviews/comments/replies/threads at final head; verified fixing commit links or evidenced rejection; verify integration and default branch separately |

Descriptors must expose layer/schema/repository/workspace/domain, platform/ABI,
actual compiler/components/targets, archive format and canonical ordered paths,
execution compatibility including current runner-image identity, producer/trust
and snapshot/restore policy. Prefixes may relax snapshot identity only. No shared
mutable Cargo writer roots. Save only after successful actual producer and valid
reports; canceled/failed runs cannot export trusted state or publish baseline.
Retain nonworkflow `.github` content and all observed CI/CD obligations, platforms
and features. Current runtime publication also needs canonical manifest naming,
every supported target including x86_64-apple-darwin, independent reproducibility
and required approvals; matching old asset digests do not supply provenance.

## Reproduction and qualification commands

Evidence documents record actual executed commands and local fixture limitations.
The following are **pending final-head checks**, not claimed successful runs:

```sh
rtk cargo fmt --all -- --check
rtk cargo clippy --locked --workspace --all-targets -- -D warnings
rtk cargo nextest run --locked --workspace
rtk proxy alint validate-config
rtk proxy alint check --fail-on-warning
rtk cargo deny check --locked
rtk proxy bash scripts/check-freshness.sh
```

Use `cargo test --locked --workspace` only as documented nextest fallback. Focused
regression modules include `source_prep_behavior_tests`, `rust_tools_prepare_tests`,
`analysis_authority_tests`, `mbx_domain_tests`, `provenance_lineage_tests`,
`baseline_publish_carry_tests`, `baseline_publish_retry_tests`, and
`semantic_inputs_tests`; execute through the serialized Cargo queue. Presence of
a test function is I-level evidence until the exact committed run is recorded.

Hosted collection command and complete procedure are in
[qualification](ci-performance-qualification.md). Freeze source/config/catalog,
workflow, cache-generation, runner and producer identities before T01; collect
each exact run attempt after all exports. Keep all selected domains equivalent
across T01–T03; baseline-only omission cannot prove compiler reuse.

## Exact47 repository closure inventory

The next table separates **bounded W0 audit closure** from performance status.
All47 finite W0 evidence audits are CLOSED at their recorded source boundaries.
Generator closure explicitly retains unavailable observations and does not grant
semantic execution, runtime publication, rollout or performance qualification.
The table repeats immutable **W0 collection** SHAs from the scope audit;
they are not asserted live heads. Each evidence link supplies PR/run/job findings
and limits. Every row: qualification run/attempt/job, final runtime digest,
cache-domain/download/compiler metrics, selected/covered counts and critical path
for the new generator are **unknown/pending**; rollout PR and resulting default
SHA are **pending**. Historical observations remain in their audit, not zeroed.
All repositories were accessible at collection, including four private Wave C
repositories. No current access limit or newly granted waiver is inferred.

Allowed final statuses: PERF_VERIFIED, STATIC_ONLY,
CI_WAIVED_PERF_UNVERIFIED, INACCESSIBLE, INCOMPLETE. Changes to a row require exact
source-bound evidence and an independent verdict. Waiver cannot grant performance.

| # | Wave | Repository | W0 source SHA | Evidence / owner | W0 audit | Performance status |
|---:|---|---|---|---|---|---|
| 0 | G | `tailrocks/velnor-new` | `c57c700459bbe1549fe7eedcb7d8689585c38986` | [Audit](ci-performance-runtime-audit.md) / runtime_logs / parent | CLOSED: explicit unavailable observations | INCOMPLETE |
| 1 | A | `jackin-project/jackin` | `6c389d38eadab93d6d6a4005e01dbdd8c4160221` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 2 | A | `jackin-project/jackin-agent-smith` | `2e7119b9c668ca7a9c55218b20049885299d198f` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 3 | A | `jackin-project/homebrew-tap` | `cd05a0ea2cf68fd6c2753ee938247b2dcd4c7551` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 4 | A | `jackin-project/jackin-role-action` | `59e538704b8c119f3b6668cea0154909a6158ff6` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 5 | A | `jackin-project/jackin-sentinel` | `587a0d1a8eef96108c9d9d530bdaa13df91edd2b` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 6 | A | `jackin-project/jackin-dev` | `a01b342162bc56cdf1e8bbaab793e73d31c1d621` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 7 | A | `jackin-project/jackin-github-terraform` | `b43a2314c6b58906d52828115d3b3973026f6a2a` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 8 | A | `jackin-project/jackin-the-architect` | `2cf461e2fed1b95d9fd1e7ba74c10d4d8b1c685d` | [Audit](ci-performance-wave-a-audit.md) / wave_a_audit | CLOSED | INCOMPLETE |
| 9 | B | `tailrocks/github-terraform` | `288dc40dec53500dfeb8bed148613b99cfea92c2` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 10 | B | `tailrocks/termpane` | `7cf2f9981ef9a3f7c8fc295a7501a261fba955a8` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 11 | B | `tailrocks/tui-snap` | `a47c9aaefb34e4c00026f99d8a8dd7ee5916b274` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 12 | B | `tailrocks/velnor` | `3f6633252963efef0d71244aadae36516a11601e` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 13 | B | `tailrocks/termrock` | `e2515bac765f440b11843a25d36ed8f720ae6435` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 14 | B | `tailrocks/parallax` | `90d901c9d12477e93a56a9e021077ced7c78f9df` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 15 | B | `tailrocks/terminal-components-claude` | `84482d066c5f0bc531f875f7f9d7716929c1b9c6` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 16 | B | `tailrocks/tailrocks-repository-skills` | `036063edc58d88cd88f57e4e3a9721ebadf3ad62` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 17 | B | `tailrocks/tailrocks-skills` | `1e9a23a63e0a44abf6a5ef17b69711011316cd9f` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 18 | B | `tailrocks/tailrocks-pull-request-skills` | `1b260bab1e356b1123fbbbdcdb17bb7bbf8ae83a` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 19 | B | `tailrocks/homebrew-velnor` | `a0db8c185b76e1bfab3508124504d0f6361e83b6` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 20 | B | `tailrocks/velnor-apt` | `115b5c42d7ad5659c8496600fabf6cd8061b64b1` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 21 | B | `tailrocks/parallax-telemetry-playground` | `763518791d60d4197d36119a47311e008ae3f5bf` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 22 | B | `tailrocks/velnor-actions-fixture` | `1c076c5b5828fb6ba04887885c567da26fe01a69` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 23 | B | `tailrocks/holla` | `c756189538c776eaa563ff83a5488e1ac96600c9` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 24 | B | `tailrocks/tracing-request-level` | `2675c867fa2f8af78c2bf2543482add847eec713` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 25 | B | `tailrocks/pg-bigdecimal` | `0a06226df6ec5f4e1a70a853d14368ebf2e69a80` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 26 | B | `tailrocks/ruxel` | `304da9f21271be81964459f5c7e6f51f41e5fdcf` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 27 | B | `tailrocks/schemalane` | `94a58dad0fe6714312373fd5dac69f2ced75d194` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 28 | B | `tailrocks/holla-apt` | `405239a2ffec34bca5eb14d95915fb0cdaae0453` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 29 | B | `tailrocks/homebrew-parallax` | `70af3b38e051abfa5a14c08ed5cd5388219ffc40` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 30 | B | `tailrocks/homebrew-ruxel` | `1db9876988e9912bafb6ea9333eea116853dc31f` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 31 | B | `tailrocks/homebrew-tablerock` | `20428a6d85086aa8454e3dd693e575541e714df2` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 32 | B | `tailrocks/homebrew-holla` | `5b7c0f27b570de6ef07f17f593cf96737b0de3ce` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 33 | B | `tailrocks/tablerock` | `a9771cab32271b1b3fca40f536d9112970989623` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 34 | B | `tailrocks/cloudflare-tofu` | `03fb253cc0f352c3f57ff7dff9d35e4ea95eec4e` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 35 | B | `tailrocks/tailrocks-typescript-skills` | `0652a50fce67c4a11f01ebb960a50fd5e283e0a9` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 36 | B | `tailrocks/tailrocks-skill-authoring-skills` | `93fa4d609f348c04869a94d0dc7a10751e31bf1d` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 37 | B | `tailrocks/tailrocks-rust-skills` | `0317f100714dc66285c01c24f7967134375b5ac5` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 38 | B | `tailrocks/tailrocks-roadmap-skills` | `98d23280cd562c9298ce13ae40bd0eb73a3e376a` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 39 | B | `tailrocks/tailrocks-open-source-skills` | `3e51bc5c91949f361ed926d8f760bcb16edef111` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 40 | B | `tailrocks/tailrocks-macos-skills` | `eb0be5522fe0c1c9c74d41ee354446a011b10c74` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 41 | B | `tailrocks/tailrocks-code-quality-skills` | `e63a82f28b688a7fada418e615aa9ec0eeec4c04` | [Audit](ci-performance-wave-b-audit.md) / wave_b_audit | CLOSED | INCOMPLETE |
| 42 | C | `ChainArgos/blockchain-nodes` | `0881638712a837bdcb90b3cd811a8a9be3aa8d83` | [Audit](ci-performance-wave-c-audit.md) / wave_c_audit | CLOSED | INCOMPLETE |
| 43 | C | `ChainArgos/java-monorepo` | `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` | [Audit](ci-performance-wave-c-audit.md) / wave_c_audit | CLOSED | INCOMPLETE |
| 44 | C | `ChainArgos/jackin-agent-brown` | `6b2ac2277103a22b9ae155bc92415ca5ddf05f95` | [Audit](ci-performance-wave-c-audit.md) / wave_c_audit | CLOSED | INCOMPLETE |
| 45 | C | `ChainArgos/cloudflare-tofu` | `cc0e2b687dc6c1a9943b0ae9f5911f1e782c1a20` | [Audit](ci-performance-wave-c-audit.md) / wave_c_audit | CLOSED | INCOMPLETE |
| 46 | C | `ChainArgos/github-terraform` | `c8d47ea87a611251f965555dafbd8015f29f303b` | [Audit](ci-performance-wave-c-audit.md) / wave_c_audit | CLOSED | INCOMPLETE |

## Update rule

For every completed unit record fixing commit, exact generated/runtime identity,
executed command or run/attempt/job/artifact digest, independent reviewer and
remaining limits. Re-check related gates after integration; never infer hosted
qualification from local state. Final closure requires all applicable gates or
exact irreducible authorization/access limits after authorized work is exhausted.

For the earlier ledger snapshot, independent second reader
`gate_traceability/ledger_verify` inspected the full
goal/specification, computed ordered47-row/SHA equality against scope/audit,
checked all9 C gates/all26 T cases, resolved links and source/test references,
and reviewed evidence grades. Two missing cross-cutting rows and an unsupported
local MBX evidence grade were corrected; final rereview found no remaining ledger
coverage/factual issue. This is ledger verification, not implementation approval.

First refresh reviewer `closure_inputs/ledger_refresh_review` independently verified
ordered47 number/wave/repository equality against all three scope inputs, frozen
SHA equality against the scope audit, all C01–C09/T01–T26 rows, bounded consumer
W0 closure, retained CI metadata and raw local source receipt/commit distinctions.
No unsupported ledger claim remained. This review used existing read-only records;
it grants no hosted behavior, runtime publication or performance qualification.

The same independent reviewer verified the later source-only publication records,
all ten asset identities/digests, retained legacy-release metadata side effect,
DCO/signature distinction at `577694a…`, policy-repository merge/default CI and
corrected immutable `24d2d538…` native map. No unsupported claim remained in
that bounded postimage. Whole generator W0 closure and performance are separate.

Final finite generator W0 closure is bound to immutable `0947e2e9…` evidence,
`01dc3760…` manifest and independent `385c3087…` review. This supersedes only
the earlier generator audit-reading gap; qualification limits remain recorded.

Final ledger reviewer `closure_inputs/ledger_refresh_review` rehashed the closure,
manifest, final review and all 716 evidence files: zero mismatches. Ordered47
rows/frozen SHAs, all9 C/all26 T gates and 47 INCOMPLETE performance statuses
remain unchanged. Aggregation grants only finite bounded W0 evidence closure.

## Source-progress and custody history

Earlier blocker snapshots, publication records, and Exact47 row-level findings
remain in the [source-progress/history supplement](ci-performance-source-progress-history-20261004.md).
The supplement preserves the prior evidence and its limits; the ledger table above
remains authoritative for current status.

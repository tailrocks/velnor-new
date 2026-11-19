# Rust-only redesign by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor CI on a native Rust runner

## 1. Summary

Make GitHub responsible for the head, queue candidate and merge. Make Velnor responsible for proving what ran on each input.
One Rust executor accepts the current Actions protocol. A sealed Linux worker produces immutable outputs and execution receipts. GitHub’s official runner runs only in the differential lab.
A labelled PR asks for verification of its latest head. A merge group independently asks for verification of its actual tree. Both join the same content-addressed work.
A protected main job verifies promotion evidence and owns the exact optimised binaries, packages and provenance that release will publish.
**Committed targets**, P50/P90, are **90/240 s to PR verdict** and **180/480 s from final push to merge**. **Stretch targets** are **30/100 s** and **90/240 s**, respectively.
These are engineering budgets for a labelled, review-complete PR at normal admitted load. Count every merged PR separately on the unconditional clock, including time waiting for the owner’s label, reviews and queue eligibility. Those waits have no finite upper bound.
The measured baselines are 26.7/92.3 min per scale-set-era PR run and 85/457 min from the first run on the final head to merge. Neither measures the new label-to-verdict clock.
Eliminate hosted gates and repeated builds first. Keep Cargo, private XFS reflinks and the full test inventory. Budget separately for checking that the reuse machinery itself still works.
The release cutover requires protocol parity, proof of input completeness and a successful exact-SHA publication rehearsal. The targets below are estimates until a paired deployment trace meets them.

## 2. The latency budget

**Clocks.** M denotes a measurement, S a fact from source, E an engineering estimate or target, and U an unmeasured interval. All design columns are E in seconds. Pairs are a typical and a slow admitted scenario. Their sums budget individual traces. Population percentiles must come from paired end-to-end timestamps.
Start at final push acceptance, record label time, last required review, actionable failure, check visibility, enqueue, candidate SHA, merge and main verification. Also time label-to-verdict for a label added later. Include superseded, open, ejected and abandoned heads alongside the merged cohort.

| Stage | Today | Design, seconds | Bottleneck and change |
|---|---|---:|---|
| Event, webhook, job offer | M 2/226 s run creation to first job created, 60/626 s for superseders. U webhook delivery alone. | 3/8 to authenticated ingress. Offer 10/25 in parallel. | App reads current head and starts the requested graph immediately. One dispatch creates the native Actions coordinator. Measure webhook and offer separately. |
| Acquire | M cluster job queue 12/18 s includes assignment. U isolated acquire. | 0.45/1.4 | Warm TLS and Broker session. Budget roughly 3 control exchanges at 0.15/0.45 s. Confirm ownership before executing a job. |
| Sandbox start | U inside ARC queue and worker startup | 0.05/0.20, overlapped with acquire | Lease a clean namespace from a pool. Rebind fresh writable mounts. Misses pay measured cold-start time. |
| Checkout | M 6/8 s on maintenance helpers, 12 s for Plan on example below | 1.5/4 | Local Git objects, verify tree and materialise fixed path. Fetch missing objects once. |
| Toolchain and tools | M mise 2/5 s, pinned tools 10/18 s, Rust prepare 1/2 s | 0.4/1 | Mount a verified immutable tool set. Its manifest digest is in the work key. |
| Dependency fetch | M 3.5–6.1 s for 59 MB direct, 2.7–3.3 s through the node mirror. Scale-set sources missed 48/48. | 0.5/3 | Mount verified Cargo inputs offline. One fetch fills each missing checksum or Git object. |
| Compile | M helper test executables 14/23 s, runner-host 81/116 s, local-mode 88/126 s | 24/68 | Example central allowance is 16 s delta compile + 8 s link. Slow allowance is one measured 67.4 s lockfile-sized rebuild, rounded. |
| Tests and sibling checks | M helper tests 10/62 s, runner-host 192/417 s. Maintenance executes its named check in 486/570 s. | 29/99 | Budget 25/90 s longest remaining required lane + 4/9 s inventory, distribution and joining. Lint, docs and named checks overlap about as far as cores allow. |
| Result reporting | M example 3 s upload, 3 s merge, 1 s publish. U final service acknowledgement. | 1.2/4 | Stream masked events during execution. Flush final receipts, Results metadata and the terminal state. |
| Required check | M hosted verdict queues 1.6/24.3 min and wrapper queues 1.9/21.4 min across events | 2/6 | One protected App writer binds proof to the current SHA. GitHub Actions still supplies the release witness on main. |
| Queue entry and run | M 19 s median enqueue to group run created. M group wall 19.9/84.1 min. | 4/15 to enqueue + 19/55 to group + 8/105 for group | Verify actual ordered candidate. Typical 3 s plan + 3 s proof validation + 2 s check. A changed overlapping dependency spends the slow budget. |
| Merge | M 26 s median from group finish to merge. U corresponding P90 here. | 26/55 | GitHub controls the final ref update. Reserve 55 s for the slow scenario and measure it. |

The work can start before the coordinator receives the offer as sealed computation without job tokens. The job independently validates and joins it. API-dependent or event-sensitive steps wait for acquisition. The join includes the slower of the coordinator and compute paths.
For a conservative additive budget including acquire, PR is about 62/195 s. Add 4+19+8+26 or 15+55+105+55 for about 119/425 s to merge.
The 90/240 and 180/480 s commitments leave roughly 28/45 and 61/55 s for contention, scheduling and service tails. They are hypotheses about the eligible population, not sums of marginal quantiles.
For stretch, budget 2+1+8+12+4 = 27 s on a warm small edit and 5+4+35+45+8 = 97 s on a heavier edit. Overlap coordinator admission. Allow another 60/140 s for the queue and merge. The target rounds to 30/100 and 90/240 s.
Measure the hit mix before accepting stretch. A wholly cold toolchain, real-time integration test or service incident can exceed both budgets.

**One historical trace.** PR #1234’s final-head run started at 23:27:28 UTC on November 17. Plan started 281 s later and ran 56 s. Maintenance ran for 534 s. The report job finished at 23:45:52 and the two-second wrapper started 65 s later. PR CI took 19.53 min.
From the first run on that head to merge was 56.13 min. Enqueue to merge occupied 35.70 min, including 13.58 min of group CI. The timeline leaves about 0.90 min between the PR run finishing and enqueue. Exact push acknowledgement and the sub-second RPC spans were unrecorded. [PR run](https://github.com/tailrocks/velnor-new/actions/runs/44199526022) · [Group run](https://github.com/tailrocks/velnor-new/actions/runs/44205503589)
Sources for the distributions are `evidence.md:75–91,164–213,245–321,335–353,576–583`, `v2/velnor-runner-architecture.md:24–47,709–722` and the cluster experiment rows P1–L1. On November 18 alone, the median of the three hosted gate queues was 57 min on a 71.8 min successful PR path.

## 2b. Duplicate-work audit

Inventory snapshot S is velnor-new `d6be276a6c`. The timing pack used `95de9b704b`. There are 16 workflow files in the inspected checkout. The live workflow comes from the older velnor-actions generator. The separate `generator/` workspace renders a different CI and release product.
P = labelled PR, G = merge group, M = main, R = release, A = audit/schedule, W = warmer, O = oracle, L = sealed agent pre-push. “Same” means equal effective inputs. Cost is occupied runner-minutes or explicitly identified seconds and decimal GB. U means the operation has no isolated measurement. Rows within a job overlap. Add the disjoint totals below.

| Operation | Stages | Same input? | Today’s cost | Decision | Mechanism |
|---|---|---|---|---|---|
| Accept drafts, ready, edited, pushes, reruns | P | Yes on unchanged head | M 1,156/2,558 PR runs cancelled, 469 runs on unchanged SHA | Suppress or coalesce | Unlabelled pushes, ready and edit events do zero heavy work. New labelled heads supersede immediately. Duplicate deliveries join the current request. |
| Fetch commit, checkout, submodules, Git history | L/P/G/M/R/W/O | Yes for same checkout contract | M 4/6 s checkout across 6,313 successful steps | Reuse objects | Reuse verified objects. Materialise required ref/depth/modes per caller. Oracle receives an independent checkout of the same inputs. |
| Resolve tools, action pins, helper, manifests, registry sources | All | Yes by blob and platform | M 10/18 s tools per scale-set job, 435–522 crates repeatedly downloaded | Reuse downloads | Fetch once globally, verify checksums, replicate blobs. Preserve version-specific installations and credentials. |
| Compute plan, keys, dependency graph and test inventory | L/P/G/M/R | Often. Event and required set can differ | M Plan 0.9 min execution plus 5.5 min queue median | Reuse graph inputs | Protected planner recalculates cheap bindings per request. Share resolved Cargo graph, inventory and content hashes. |
| Ledger CLI, helper bootstrap, upload and redownload | P/G/M/W | Yes per helper version/target | M ledger job 0.8 min, build 21 s, download 3 s | Eliminate shuttle | Reuse verified released binary or build keyed helper once. One join writes the run-specific report. |
| Format, cargo-deny, machete, jactionlint, alint | P/G/M/A | Yes with same input set, advisory snapshot and rules | M job medians 0.4/0.2/0.3/0.3 min for the last four in listed order | Reuse checks | One receipt per check. Advisory refresh is a separate live observation with a freshness deadline. |
| Resolve/fetch five Cargo roots | P/G/M/W/source qualification | Yes per registry object/Git revision | M 59 MB benchmark, 10/10 Plan restores near 0.086 GB, 48/48 Rust restores missed | Reuse sources | Single checksum-addressed source pool. Offline builds, registry identity included. |
| Compile dependencies, proc macros, build scripts, dev/test units | L/P/G/M/A/W | Yes only for the same Cargo unit contract | M 14–88 s median test build for cited jobs, 10.64 GiB logical runner target | Reuse units | One fenced build per key across requests. Own immutable outputs before tests can write. |
| Build fixture binaries, doctests, clippy metadata, docs, standalone check | P/G/M/A | Different command modes, common dependencies | M runner-host clippy 70 s, docs 60 s. CLI image fixture 89 s. | Reuse common builds | Preserve each obligation, invoke Cargo with its original target/features, link mode and lint settings. Share proven units and fixtures. |
| Root, runner, local-mode, generator and helper tests | L/P/G/M/A | Yes per executable/runtime closure and case | M CLI tests 238 s, runner-host 192 s, local-mode 246 s medians | Reuse proved cases | Separate feature variants. Run each expected case on validated bytes. Keep fresh tests for live or stochastic properties. |
| Named `generator-samples`, `maintenance-helpers` | P/G/M/A | Yes on same fixtures, tools and helpers | M maintenance 8.41 runner-min/PR run, 104 crates | Reuse fixtures | Reuse setup and verified goldens by input. Keep the real generation, installation, drift and negative assertions. |
| Assemble group ledgers, reports and verdict | P/G/M | Contents may match, public identity differs | M upload 3 s, merge reports 4 s, final publish 1 s | Rebind verdict | Share result objects. Emit fresh check identity, attempt, links and required inventory for each subject. |
| Push N → N+1, rebase-only, user rerun | P/P | Partly, often wholly | M three runs per head branch median, admission stall 60/626 s on supersession | Reuse unchanged work | Register new interest before detaching old. Rerun changed packages and reverse dependents on misses. Equal sealed tree/policy gives immediate proof lookup. |
| Shared/identical changes, stacked branches and pre-push | L/P/P | Yes for shared subgraphs | U previously repeated parts within 52.9 min median PR run | Join by content | Global key and single in-flight lease. Each head obtains a distinct GitHub verdict after a request. Local unsigned work contributes zero proof. |
| Combine PR with actual queue tip, then land | G/M | M 7/45 observed PR/group trees equal, 120/159 green groups had identical-SHA push | M 64.6 + 62.0 runner-min per representative group/main pair | Reuse matching proofs | Verify a complete group plan by key. Main independently checks exact source, build identities and signatures before advancing trusted aliases. |
| Source qualification plan/selection/ordinary/keep/generation/final | P/M preview | Conditional | M #1234’s two PR runs 34.28 and 40.45 min | Fold duplicate suites | Fold required Linux properties into labelled requests, compute each unique harness once. Move live release checks to their actual product stage. |
| Build, attest, publish Linux job-image candidate | Branch and queue pushes/M/R | Build yes by image DAG. Public effects differ | M #1234 had five image runs across heads, groups and main, 21.70 min total | Eliminate push builds | Eliminate unconditional push builds. Verify Dockerfile, base/layers and product binary once. Publish only under the protected publication intent. |
| Sources/tools/state producer, seal, cache prune | M/W | Often overlaps CI | M one #1234 main seal 8.85 min, two prunes 0.78 min. Separate seal example 30.50 min | Replace warm builds | Promote immutable verified outputs once. Warmers copy or hydrate. Reference-count live roots and sweep expired unreferenced blobs. |
| Product eligibility and caller authority | M/R | No for live main tip, run attempt or tag | M 2.1 min eligibility in release example | Check live authority | Recheck online. Reuse signed main manifest. Preserve freshness, caller identity and exact push-run constraints. |
| Current generator binary, Linux qualification, manifest and aliases | M preview/R | Yes only with same profile, version, target and build identity | M Linux build 3.9 min, qualify 3.4 min in release example | Promote verified bytes | Build/qualify once for main, bind GNU asset, checksum, provenance JSON and manifest. Preserve versioned and compatibility names by digest. |
| Runner image, DinD image and resource probe | M/R | Yes when all layer inputs and metadata match | M 8.4 min build, 1.1 min attest, 2.7 min publish | Promote digests | Promote OCI manifests, archives, licenses and probe manifest. Replace production image payload with native Rust executor. |
| Current host and attestation helper binaries | R | Target differs from test profile | M macOS build 8.9 min | Pause Darwin release | Pause Darwin product. Future Linux products declare their own targets. Keep loose-file asset contracts where consumers depend on them. |
| Rust CI tools: `velnor-ci` and `velnor-gen`, COMMIT, sums | M/tag R | Yes by target, toolchain and version | S release-rust-ci-tools.yml:101–234. M 0.0293 GB of published files for rust-ci-v0.1.83 | Promote Linux products | Build both Linux musl targets once on main. Read the verified manifest at tag time. Remove macOS row and update expected asset lists together. |
| Weekly freshness and daily tag-ruleset drift | A/R | No when upstream state changes | S Monday 06:00 UTC and daily 04:23 UTC. U disjoint minutes | Keep fresh observation | Audit upstream once per observation/cadence. Release checks the allowed age and current authority. Keep the live ruleset probe. |
| Source preview build, runtime, publication and mac proof | M/preview tags/R | Linux product often shared. Publication/principal differs | M main preview for #1234 28.65 min | Reuse Linux preview | Reuse Linux candidate and SDK checks. Preserve its independently authorised publication. Suspend Darwin proof, Homebrew adoption and Intel jobs. |
| Qualification service/container/compose, failure, OIDC, masks, cache, post, pressure and cancellation modes | Manual/O/A | Backends/capabilities intentionally differ | S qualification.yml has paired jobs. U population cost | Keep independent controls | Run supported probes on native Linux. Move official comparisons and JS refusal controls to the lab. Never use a lab output as a product receipt. |
| Release-plz release PR, changelog, tag and registry decision | M/R | No across changes of registry, refs, PR or version | U for this repo. S renderer release.rs:178–274 | Reconcile intent | Retain per-root release planning and `release-pr`. Lock release intent to verified SHA/version. Query authoritative tag/registry state before retry. |
| Optimised product compile and exact-binary smoke/integration | M or final candidate with complete inputs, then R | No vs debug, yes in later stages | M Linux release build job 3.9 min including setup. U isolated optimised rustc | Build once on main path | Choose separate product profile once. Section 6 prices the alternatives. Release has zero compiler invocations. |
| Package generated `<bin>-<target>.tar.gz` and optional registry `.crate` | M/R | Yes if names, bytes and archive metadata match | U isolated packaging. S release_binaries.rs:42–55,97–127 | Package once | Canonical order, mode, UID/GID and timestamps, pinned compressors. Store final archive bytes, unpack and test them. |
| Produce `SHA256SUMS`, product manifest and SBOM where required | M/R | Yes by ordered names/digests, format, tool and SBOM inputs | U isolated. S release_binaries.rs:164–193 | Generate once | Make the exact set on main once. Verify hashes during transport. Never repackage to reproduce a checksum. |
| Sign/attest binary, archives, checksums and manifest | M/R | Same only with identical subject, predicate and authorised identity | M release sample attestation jobs 0.4–1.1 min each | Reuse matching statement | Protected main signs build provenance once. Reuse original bundle. A required tag-specific release statement is a distinct predicate, signed once under its real identity. |
| Publish asset, draft release, tag, crates.io or OCI digest | R | Yes per destination, version, name, digest, authorised intent | M generator publish 1.1 min, image publish 2.7 min | Publish once per destination | Durable intent, compare remote digest/ownership, then create missing effects. A different existing digest stops publication. |
| Nightly cold audit, conformance on new runner, cache warmers | A/O/W | Different verifier/input epoch or independent control | U historical oracle. M state restores skipped 3,636/3,636 | Share each independent control | Coalesce audit requests for the same independent observation. Warmers subscribe to verified outputs. Oracle pairs run once per version/corpus/environment. |

The generator’s sample `generator/samples/release-bin/view/release-plz.toml` uses `git_only = true`, draft releases and package-version tags. There is no root release-plz config in this checkout.
Its rendered names include `app-linux-x64.tar.gz` and `app-linux-arm64.tar.gz`. The suffix maps to the full musl triple. The current product release instead publishes loose binaries, Linux image tars and versioned GNU generator assets with `.sha256`, `.provenance.json` and `.intoto.jsonl` companions.
For a registry-publishing configuration, stage and verify the exact `.crate` on main. Use a publisher that transmits those bytes and preserves release-plz’s ordering, metadata, tag/commit checks and registry visibility wait. A mode that insists on packaging/building again blocks that configuration’s promotion cutover.
Track consumers of `COMMIT`, `LICENSE`, `NOTICE`, the alias, archive paths, expected signer and subject names. Rehearse fresh install from release assets. [Published generator files](https://api.github.com/repos/tailrocks/velnor-new/releases/tags/v0.1.80) · [Rust CI tools](https://api.github.com/repos/tailrocks/velnor-new/releases/tags/rust-ci-v0.1.83)

**Totals and denominators**

| Per unit | Today | Native design, E |
|---|---|---|
| Merged PR, core CI | M #1234 had 2 PR runs, 2 group attempts and 1 push: 52.52+65.98+64.42+77.02+83.08 = 343.02 runner-min. E one of each at cohort medians = 179.5, three PR attempts = 285.3. | 12 initial + 2 superseded work + 4 group misses + 1 main witness = 19 runner-min for a representative merge. |
| Merged PR, all attributed work | M #1234 used **477.73 runner-min** across its two head SHAs, two group candidates and final main SHA, including source qualification, image candidates, previews, seal and prune. | E 19 + 4 amortised optimised products/smokes + 2 source/image obligations + 1 warm/speculation + 4 independent audit/oracle + 0.5 control = **30.5 runner-min**. Commitment ≤35. |
| Merged PR, cache bytes | M **at least 8.600 GB** logged cache sizes for #1234: five CI runs × 0.76195, two source-qualification × 0.34374, preview 0.08594, state seal 4.01690. A cancelled image run had no retained log. | E ordinary merge 0.50 GB internal cache transfer (0.20 ingest + 0.20 replication + 0.10 fills). At 10.5% lock changes each moving another 6.8 GiB, plus 0.10 amortised batch, **1.37 GB/merge**. Hot receipt-only merge moves KB. Target zero external CI cache GB. |
| Product release, dispatch to published | M run 43864723181 **103.97 runner-min**, including 77.48 on macOS-labelled jobs, 26.48 on other jobs. **0.00 GB in explicit cache-size records** across its 27 retained job logs. | E 1 min authority + 1 manifest/signature checks + 1–3 publication/remote readback = **3–5 runner-min**, zero build-cache GB when products are staged. A cold publisher reads up to about 1.17 GB of product blobs in the measured Linux image/generator size class. |
| Product release inclusive of prebuilt work | M 103.97 dispatch minutes plus the main verification that authorised release. The three associated v0.1.80 product asset sets total about 1.32 GB of published files. | E a changed release train incurs **15–20** main build/qualification minutes once plus **3–5** promotion minutes = **18–25**. At one such change in five merges that accounts for 3–4 of the 4 amortised minutes above. |

The per-PR measured total counts occupied job seconds once and includes the failed main CI. It attributes shared queue candidates fully to the example PR. Fleet reporting apportions them across members. Scheduled work unrelated to its SHAs is outside the historical trace. The design charges the consuming repository for its share.
Cache sizes come from the archive-size field in retained job logs. GitHub’s 3.495 GB active-cache snapshot measures storage at one point in time.
Five populated nodes would hold 5 × 10.64 = 53.2 GiB of logical runner-workspace targets before histories, tools, other workspaces and release blobs. XFS extent accounting supplies the physical figure.
Report retained physical GB, GB-days, bytes ingested, reads per hop, release uploads, CPU-seconds and occupied runner-minutes separately.
Reconstruction used the Jobs API plus one top-level log per job. PR CI run IDs 44192411238/44199526022, groups 44202521034/44205503589, main 44207443352, seal 44207443310 and preview 44207443671. [Example PR](https://github.com/tailrocks/velnor-new/pull/1234) · [Product release run](https://github.com/tailrocks/velnor-new/actions/runs/43864723181)

## 3. The native Rust runner

Put the executor in the existing nested `crates/velnor-runner` workspace. The system has two callers. A GitHub job needs faithful step execution. A verification request needs authoritative evidence for a tree. Generated jobs call the latter through the same interface as agent preflight.

| Owner | Responsibility and boundary |
|---|---|
| `velnor-runner-github`, extended | App auth, scale-set session, JIT, Broker, run-service and Results. Typed credentials per audience. Decode wire structs privately. |
| `velnor-runner-core`, extended | Job identity, fencing epoch, ownership state and recoverable side effects. One journal owns a job until remote completion and cleanup reconcile. |
| `velnor-runner-exec`, new | Admitted job, expression values, template evaluation, step scopes, post stack and result semantics. |
| `velnor-runner-actions`, new | Pinned native action manifests, versioned input/output contracts and capability-limited implementations. |
| `velnor-runner-host`, adapted | Warm sandboxes, private mounts, process trees, cgroups, service lifetimes, independent observation and teardown. |
| `velnor-verify`, new | Released planner, operation DAG, immutable store, leases and independently signed receipts. Named consumers request a proof for their own policy and subject. |
| App controller and lab | The controller owns event reconciliation, authoritative checks, queue tracking and promotion. The isolated lab owns reference runs, compatibility decisions and canaries. |

The public operations are `ensure(subject, policy) -> Proof | Blocked`, `execute(admitted_job, lease) -> JobResult` and `promote(main_subject, proof, outputs) -> SignedManifest`.
A job return value cannot create a trusted main manifest. A proof cannot fabricate a GitHub job completion. The single-writer boundaries make both errors visible in review.

**Wire path**

1. The App exchanges its installation credential for the scale-set administrative credentials the current service requests. Discover the runtime scale set and runner group. Open one fenced listener session per set, refresh its queue token and long-poll with `lastMessageId` and `X-ScaleSetMaxCapacity`.
2. A 202 poll has no message. On a JobAvailable batch, persist IDs and advertised capacity, call `acquirejobs` for the eligible IDs, record which GitHub accepted, then acknowledge the scale-set message. Redelivery recovers from a lost response. See `arc/cmd/ghalistener/scaler/scaler.go:149` and `evidence.md:1160–1183`.
3. Maintain a small stock of distinct, idle JIT identities with bounded age. Generate each using the scale-set ID, runner name and work folder. Parse the double-encoded settings/credentials/key in memory. Require Broker v2 and a negotiated service URL. One identity serves one job. See `arc/controllers/actions.github.com/ephemeralrunner_controller.go:853` and `actions-runner/src/Runner.Listener/Runner.cs:234–266`.
4. Use runner OAuth client credentials and the provisioned RSA key to create the Broker session. Long-poll for messages with identity/version/OS/arch. Acknowledge a reference if the negotiated message requests it. Acquire via its supplied run-service URL with job message ID, runner OS and billing owner. See `src/Runner.Listener/Configuration/OAuthCredential.cs:27–49`, `src/Sdk/WebApi/WebApi/BrokerHttpClient.cs:60,143,225` and `src/Runner.Listener/Runner.cs:708–756` in actions-runner.
5. Scale-set capacity acceptance does not reserve a particular GitHub job for a chosen JIT identity. Separate sets classify capabilities and reserved capacity. The identity receiving the actual reference owns its lease. Place its sealed operations on suitable nodes after admission.
6. Persist acquire intent before sending. Interpret 404/409/422 by typed response, refresh expired credentials once through their authority, back off on rate limits. A lost response is uncertain ownership. Reconcile or wait for expiry. A duplicate offer never launches the job twice. See `src/Sdk/RSWebApi/RunServiceHttpClient.cs:70–112`.
7. Obtain first successful renewal before executing credentials-bearing steps. Renew in an independent task at the negotiated deadline, normally every 60 s with margin for RTT, skew and retries. Convert returned `LockedUntil` to a conservative monotonic deadline. Revoke write authority on lease loss. See `src/Runner.Listener/JobDispatcher.cs:380–410,750–810`.
8. Restrict the wire client to current runtime scale-set, Broker, run-service, Results, hosted cache v2, artifacts v4-generation and OIDC. Even the upstream scale-set SDK has a distributed-task agent-deletion helper. Use supported runner administration or ephemeral expiry, confirm removal and quarantine ambiguity. An obsolete endpoint requirement fails the compatibility gate.

**Job admission and semantics**

The acquired job carries plan, timeline, job ID, attempt and request, ordered steps and references, template tokens, context data, resources/endpoints, variable secrecy and mask hints, job/container/service configuration, defaults, outputs and environment metadata. Preserve null, absent and typed values separately at decode time. See `src/Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs:134–236,288–315`.
GitHub expands workflow-level scheduling, matrices, reusable-workflow calls, permissions and `needs`. Velnor executes the concrete job, reads its contexts and reports the outputs that GitHub needs to schedule successors. It never elevates permissions inferred from a copy of YAML.
Before executing user code, resolve the protected workflow and every reachable pinned remote/local action, even in conditional branches, including nested composites. Read local manifests directly from the immutable repo objects. Validate shell, service, platform and action inputs. Reject unknown revisions, dynamic references, cycles, unsupported semantics or limits exceeded with source location.
Only an admitted released verification definition may launch pre-offer computation. If the acquired job differs, it cannot inherit authority from that computation. Unknown wire metadata is tolerable only when classified as explicitly non-semantic.

Use one Rust parser/evaluator for the current Actions expression contract. Port and test the archive’s lexer/value fixtures, reconcile them against the latest release. Support short-circuiting, loose equality and number coercion, case-insensitive string comparisons, null/missing properties, object filters, indexing, JSON functions, formatting, status functions and workspace-scoped `hashFiles`.
Implement glob ordering, exclusions, empty matches, symlinks, path boundaries and aggregate digest in Rust. Exercise interpolation as a template separately from evaluating a typed value. Match upstream errors and limits. The names `Expressions2` and `DTPipelines` in reference source describe observed semantics. Production contains no legacy evaluator or distributed-task transport. See `src/Runner.Worker/StepsRunner.cs:92–124`, `Expressions/HashFilesFunction.cs:74–120` and `ActionManifestManagerWrapper.cs:73–86` under Runner.Worker.

Execute steps serially in their job scope. Construct contexts at the official evaluation point: `github`, `runner`, `job`, `steps`, `inputs`, `env`, `vars`, `needs`, `matrix`, `strategy` and permitted secrets. Linux environment names are case-sensitive. Honour job/step env precedence, shell defaults, working directory and command exit behavior.
Apply the implicit success condition where applicable. Record both outcome and conclusion before considering `continue-on-error`. Preserve failure, skip, timeout and cancellation distinctions. Evaluate final job outputs after steps and posts, retaining upstream handling of outputs that may expose secrets. See `src/Runner.Worker/StepsRunner.cs:168–221,296–353` and `ExecutionContext.cs:1424–1446`.
A composite establishes distinct inputs, step scope, action path, action status and output mappings. Register an action’s post before its main body where upstream does. Execute registered posts in reverse order with their own conditions and scoped state, including cleanup after failed actions. See `ActionRunner.cs:111–136`, `ExecutionContext.cs:335–369` and `Handlers/CompositeActionHandler.cs:138–151`.
Use fresh per-step command files for `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STATE` and `GITHUB_STEP_SUMMARY`. Apply writes at step end, retain multiline delimiter/error behavior, scope saved state to that action, and process annotations, groups, stop-commands and masks. See `FileCommandManager.cs:173,215,264,413–424` and `ActionCommandManager.cs:118`.
Keep App, scale-set admin, Broker and job secrets out of snapshots. Supply only context-appropriate credentials. Script steps get an OIDC request URL/token only when authorised. Mask at ingestion before disk, live feed or Results. Test chunk-boundary matches, encoded forms and newly added masks. Never use the text of a secret as a reusable key.

**Start, cancel and finish**

Keep loaded policy, action registry, Git mirror and immutable rootfs on each worker. Restore from a credential-free baseline or lease a clean pre-created sandbox. Overlap private mount creation with Broker/acquire and first renewal. Start a simple step within E 0.5/1.5 s from reference arrival on a warm identity, with E 20–50 ms of local plumbing. Admit misses separately.
A cancellation first fences the request, then signals its processes, re-evaluates applicable conditions, runs allowed posts against a bounded deadline and kills remaining descendants. A step timeout records failure unless the job was cancelled. Orphaned containers, background services and ports belong to the job lease.
For Results-only jobs, discover the results endpoint, job access token and live feed from the message. Stream numbered redacted lines, send workflow step updates, request signed blob URLs, upload job/step logs and summaries, then create metadata. Use ordered, idempotently reconciled outbox records. See `src/Runner.Common/JobServerQueue.cs:121–138,696–705` and `src/Sdk/WebApi/WebApi/ResultsHttpClient.cs:57–82,134–163`.
Drain required evidence before `completejob` with actual conclusion, outputs, step results, annotations, environment URL and applicable telemetry/billing fields. Persist the exact completion body/hash. A lost completion ACK never reruns a release or produces a positive check by inference. See `src/Runner.Worker/JobRunner.cs:64–85,293–357` and `RunServiceHttpClient.cs:123–152,184–197`.
Journal states are reserved → acquiring/uncertain → leased → executing → flushing → completing/uncertain → cleanup → released or quarantined. Persist the owned sandbox UID, generation and external effect IDs. On restart establish ownership, reconcile remote state and purge temporary data only after durable flush. Only the current fence can complete, publish or sign.
Keep HTTPS peers and credential audiences bound to the job and advertised service. Fail closed on deprovisioning, unsupported protocol or a refresh that requires an unqualified build. Roll back to a qualified Rust version or drain the set.

**Code provenance.** Keep velnor-new’s existing GitHub, core, qualification, image manifest, attestation and journal code where its contracts survive. In particular use `crates/velnor-runner/crates/velnor-runner-core/src/lifecycle.rs:9–52,159`. Replace its official-runner launch backend.
Select Apache-2.0 archive pieces by file and revision: scale-set session/refresh, expression syntax fixtures, step-outcome model, current wire DTOs and individual native adapters. Carry notices, verify licences of transcribed third-party pieces, import tests and record the source SHA in velnor-new. See `velnor-legacy/crates/velnor-runner/src/scaleset/session.rs:52,100,125,165`, `expression/mod.rs:1–14` and `execution/step_outcome.rs:21–71`.
Write fresh typed wire boundaries, current protocol negotiation, job admission, sealed operation graph, Results outbox and differential driver. The archived protocol pins 2.337.0, and archived native performance has no measured baseline. No runtime, package, submodule or update process depends on the archive.

## 4. Native actions

The registry key is `(owner, repository, subpath, full commit, implementation digest)`. Generate the allowed revisions and their fixtures from one manifest. Preflight traverses reachable composites, reusable-workflow manifests and platform variants before granting a sandbox.
The action major and service generation are separate. Checkout is v7.0.1, cache adapters v6.1.0, upload v7.0.2 and download v8.0.2 here. They speak the requested current services. The repository pins and versions are in `.velnor/version-policy.toml:42–120`.
Times below are seconds. M is step time including service effects where indicated. E assumes a warm node. Private CI removes redundant remote operations from its generated YAML. Where a public Actions effect is requested, Velnor still performs it.

| Pinned action or family | Native behavior and locality | Today → expected |
|---|---|---|
| `actions/checkout` | Use local verified Git packfiles, fetch absent objects, set exact SHA/ref and outputs, implement depth, tags, sparse cone/non-cone, path, submodule URLs/auth, LFS when declared, safe.directory, clean/reset and credential post-cleanup. Unknown inputs fail admission. | M 4/6 across 6,313 steps → E 0.3–2 warm, 2–8 on a small fetch. |
| `actions/cache/restore` | Cache v2 GetCacheEntryDownloadURL with key, restore keys in order, version derived from paths/compression/platform and allowed ref scope. Respect lookup-only, fail-on-miss and exact-hit output. Local immutable bytes may satisfy the resolved entry. | M Rust sources 1/1 (usually misses), hosted tools 3/10. E 0.02–0.1 local CI lookup, 0.2–1 for a real service lookup plus missing bytes/bandwidth. |
| `actions/cache/save` and combined `actions/cache` | Reserve, upload and finalise under the job’s real cache-write authority. Preserve primary key/matched key state, save conditions and conflict behavior on post. Prefix restores are acceleration hints. Only verified exact inputs yield receipts. | M state restore attempts skipped 3,636/3,636. U isolated save. E 0.02–0.1 local hit, ≥bytes/link speed for a real save. |
| `jdx/mise-action` v5.2.6 | Expose pinned verified mise, honour install/cache flags, tool-version selection, PATH and post behavior. Mount the preinstalled inventory. Create private writable paths where workflow probes require them. | M 2/5 mise plus 10/18 preparing tools → E 0.05–0.4 on warm pins. |
| `jdx/mr-boxington-action` v1.7.3 | Install/configure the pinned mbx binary, wrapper, backend, env and post summary for jobs that request it. Keep its target private. Generate plain Cargo for the measured fast path. | M 1/2 setup, post 0/1 → E 0.02–0.2. |
| `actions/upload-artifact` | Validate name, path selection, hidden files, symlinks, compression/retention and no-files behavior. Implement create, signed upload, finalise, artifact ID/URL/digest. CAS can supply the exact ZIP or raw bytes for supported modes. | M reports 3/4, Linux release files 2 in example → E 0.3–2 small, plus bytes/bandwidth for large uploads. |
| `actions/download-artifact` | List/select by ID, name, pattern, attempt/run/repo as authorised. Preserve merge-multiple and file layout, validate returned data and archive paths. Hydrate by digest after authorisation. | M ledger CLI 3/4, generator files 2, image assets 49. E 0.05–0.3 for cached bytes plus lookup, 0.5–2 for small remote results. |
| `actions/attest` and `actions/attest-build-provenance` v4.2.2 | Hash actual subjects, generate the specified in-toto predicate, use job OIDC to obtain signing identity, sign and submit bundle/attestation to required service or registry. Validate subject, issuer, repo, ref, workflow SHA, identity, claim set and transparency evidence. Preserve attestation outputs. | M 2 Linux generator, 9 image signing → E 2–10 for a fresh statement, 0.02–0.2 to validate an already authorised immutable bundle locally plus freshness checks. |
| `asamarts/alint` v0.21.0 | Run the pinned tool with the same inputs, exit codes, annotations and config against the declared workflow files. Keep binary/tool manifest locally. | M 1/2 Run Alint → E 0.5–2 on same input size, receipt lookup on repeats. |
| `tailrocks/velnor-new/actions/setup-velnor` | Interpret pinned composite. Verify release version/platform/expected SHA before installing the private helper. Retain wrapper integrity checks and PATH. | U isolated action, M ledger CLI download 3/4 as a transfer proxy → E 0.05–0.4 with helper present. |
| `actions/create-github-app-token` v3.3.0 | Appears in generated release jobs. Mint a correctly scoped installation token for the specified owner/repos/permissions. Mask outputs, support expected installation ID/slug and post revocation. No cross-job token cache. | U today → E 0.3–2 per mint/revoke exchange, based on 1–3 authenticated requests. |
| Local composites and pinned reusable workflows | Expand `declared-task-0`, tools prelude/cache scrub/seed, Rust and jactionlint verifiers, u26 and Linux generator build/qualification. Read manifests from the selected trees. GitHub owns reusable-workflow scheduling and caller authority. | U wrapper alone → E 1–10 ms dispatch per child plus the real child cost. |
| `actions/github-script` v9.0.0 | Replace its actual ruleset-drift script with a typed Rust API command. Replace the two JS qualification fixtures with supported equivalent Rust probes and an explicit unsupported-action test. Reject arbitrary script input. | U isolated → E 0.3–2 for drift API read, zero JS startups. |

For a public cache hit, verify the server still selects that record at this ref. For an artifact hit, verify the ID is still visible to this run and has not expired.
For a public upload, returning a local digest cannot stand in for the real API ID. For a private intermediate, the generator can omit the public action entirely.
The generic Docker action, node24 post-fail fixture and any unrecognised `uses` are refused before step one. Keep a supported post-failure fixture that exercises a native action’s registered post. Linux jobs may still run pinned service containers and image builds through the sandbox runtime.
A policy-list entry such as `aws-actions/configure-aws-credentials` is not evidence that a workflow uses it. A future action, new SHA or materially new input needs a qualified manifest entry.
Action timings use `evidence.md:335–353,666–688`, successful step records in `ci-raw/jobs_compact.json` and the release Jobs API. Service behavior is pinned against upstream clients in the lab. [Cache v2 client](https://github.com/actions/toolkit/blob/main/packages/cache/src/cache.ts) · [Artifact upload protocol](https://github.com/actions/toolkit/blob/main/packages/artifact/src/internal/upload/upload-artifact.ts) · [Attestation action](https://github.com/actions/attest)

## 5. The conformance oracle

As checked for this brief, GitHub’s latest released runner is **v2.338.2**, published October 26, 2026, tag commit `d1eb12d4b115c301201de27d667284e1a80f2ab5`. The supplied source checkout is `7afce9edc8`. It is a different revision. Pin the release artifact by digest and resolve its tag before using a source line to specify that release. [Latest release API](https://api.github.com/repos/actions/runner/releases/latest) · [Release record](https://github.com/actions/runner/releases/tag/v2.338.2)

**Two independent observations**

1. A lab repository dispatches paired jobs from one fixture commit to isolated Linux pools, one running the released official binary and upstream action code, one running Velnor.
2. Match source, generated YAML, actual event kind, dependency snapshot, shell, toolchain, permissions, container digests, resources and initial files. Obtain separate real jobs, tokens, caches and publication destinations.
3. Pair by fixture ID, event, job, matrix, revision, environment and attempt. Seed two equivalent cache namespaces and lab registries. Neither executor can read the other’s outputs as inputs.
4. Collect externally visible GitHub steps, job/check conclusion, outputs consumed by downstream jobs, annotations, logs, summaries, cache visibility, artifact digests, OIDC claims and release/registry effects.
5. Capture filesystem diffs, shell exit values and received signals inside instrumented fixtures. A transcript replay with virtual time covers transport fault boundaries. Two real GitHub jobs cover the service contract that replay cannot establish.

**Corpus**

- Replay all supported branches of all 16 workflow files, generated release-plz samples and exact action pins. Include red and cancelled runs, empty matrices, a missing obligation and the main release witness.
- Exercise nested composites, shell/defaults, absent/null/empty values, JSON numbers, coercion, masks, interpolation, `hashFiles`, `needs`, outputs through multiple jobs and mutable step environments.
- Exercise pre/main/post ordering, post registration before failure, failure in post, `always()`, `cancelled()`, continue-on-error, command files, group nesting, annotation locations, step versus job timeouts and cancellation at each boundary.
- Exercise checkout depth/submodules, cache exact/prefix/miss/save conflicts/read-only scopes, artefact overwrite/expiry/empty inputs, pin changes, private helper and authenticated readback.
- Run live OIDC/attestation in the lab’s identity. Verify bundle signatures, predicate contents, subjects and observable scopes. Exercise missing permission, late revocation and request expiry.
- Exercise zero/one/multiple services, isolated rootless container fixtures, cleanup, response loss after acquire/upload/finalise/complete, restart on each journal boundary, and cancellation with a slow post.
- The deliberate refusal suite includes unknown actions/SHAs, nested JS, legacy protocol negotiation and unsupported platforms. The expected native observation is an admission failure with zero user commands, secrets or external effects.
- Pair each supported failure with a positive control. Use fault-injected semantic regressions, including a skipped test binary and leaked mask marker, to establish that the comparator detects meaningful differences.

**Comparison**

Map only generated identities by a bijection. Translate workspace prefixes as paths, keep path suffixes/escaping. Translate timestamps to causal ordering plus explicit timeout tolerances. A missing event, wrong parent or changed ordering of dependent effects fails.
Compare user output and error semantics, typed values, step outcome/conclusion, job state, file bytes/modes, ordered PATH, inventory, outputs, permissions and policy decisions. Preserve stream ordering per producer and compare only genuinely concurrent events as unordered.
Validate tokens through audience, subject, issuer, scope, duration and signature. Detect cleartext markers across chunks, summaries and durable logs. Never store raw tokens. Verify attestations independently. Random signature and job-specific subject fields use schema-aware equivalence.
Keep official setup banners in the diagnostic trace under an explicit comparator rule. Never normalise away a workflow’s own bytes, warnings, ANSI-dependent assertion, missing line, timeout or release identity.

**Cutover and drift**

Every supported case must have two completed observations and agree, every negative case must show the specified refusal, and every injected defect must turn its control red. Required threshold **100%**, zero unexplained differences, zero secret leaks and zero false greens.
Exercise the full corpus on each executor/action/planner revision and official release. Require two consecutive clean full passes on the pinned release and seven days of clean hourly service canaries before authorising that version in production.
Poll the release API every 15 minutes with an ETag. On a change verify the binary and start the critical suite immediately, targeting first results within 60 minutes and the entire corpus within four hours.
Run a small real paired acquire→log→artifact/cache→OIDC→complete canary every 30 minutes even without a release. Two consecutive failures page the owner within 60–90 minutes. Unknown semantics, a false success or a credential exposure trips the breaker on first observation.
Freeze promotion for affected capabilities. Roll back to the last qualified Rust build only while live canaries confirm compatibility. Otherwise stop assigning that job class, keep checks blocking and replay the missing cases after repair.
Track the GitHub repository’s actual rollout and version deprecation dates. The documented 63-day update window can be shortened for critical updates. Advertising an untested newer version would conceal the very drift the oracle is intended to find. [Self-hosted runner update policy](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)

## 6. Compile, cache and test

**Choose the storage path by the work it removes**

| Experiment, measured | Decision |
|---|---|
| Cluster P1/P2: cold runner workspace 92.0 s, 10.64 GiB. XFS clone 0.25 s/+2.5 MB, warm Cargo 0.31 s. | Fixed path `/w/velnor-new` in every namespace, independent writable clones. E warm setup + no-op 0.56 s. |
| P3: hot full copy to ext4 8.6 s and 10.75 GiB. | Put targets on the reflink-capable data disk. Test FICLONE on the actual mount at enrolment. |
| M2/M2b: mbx 0.32 s with the same managed target, 7.2 s after that target was removed despite 496 hits. | The first number measures reusing mutable state. Keep mutable targets private. An immutable snapshot wins on repeat work here. |
| M1/M3: first writer 93.2 s and 8.7 GiB up. Empty second node 39.9 s and 8.7 GiB down vs 92.0 s cold. | An optional site mbx-cache fills authorised missing objects. Measured saving 52.1 s at this transfer rate. Foreground uses it only when measured total cost beats other available routes. |
| L1p/L1m/L1mb: one lock change rebuilt 11 units. Plain 67.4 s, mbx 88.4 s with 6.8 GiB up. Second mbx node 43.4 s. | Keep old validated dependency blobs, invalidate affected units. Place on the warm node, or reuse the second-node transfer when it wins. Avoid fleet-wide rebuilds on any lock edit. |
| E1d: each fully read clone had 14.29 GB resident even though disk blocks were shared. | Mount immutable executable/fixture files by the same inode for read-only test consumers. Budget separate page cache for private build clones. |
| Mac different-path APFS worktrees: plain +457 MiB vs mbx +473 MiB, both recompiled six workspace crates. HFS+ full copy +1,411 MiB vs mbx +835 MiB. | Path identity and filesystem determine the result. Mac is supporting evidence. Linux measurements set production policy. |
| Blaze supported subset 215.5 s cold vs Cargo incremental 17.9 s. Edits 21.3/22.9 s vs Cargo 1.5/2.8 s. Five whole workspaces did not qualify. | Keep pinned rustc, Cargo and Clippy authoritative. The missed denied `unwrap_used` is sufficient to exclude this blaze build. Re-evaluate a future revision against the same faults and inputs. |

Experiment sources are `v2/experiments/mbx-cache-cluster.md:27–47,52–81`, `mbx-cache-mac.md:25–29,95–108` and `blaze.md:30–80`. These benchmarks used particular workspaces and load. Cross-site transfer performance remains unmeasured.

**Identity and admission**

- Hash a canonical, length-delimited, versioned manifest with SHA-256. Domain-separate sources, operations, outputs, proofs and release manifests.
- A request identifies a repository, head/group/main SHA, tree, requested scope, event projection, policy epoch and authority. Each operation has its own input root. A whole-graph check binds every required operation and inventory to the request.
- Compile keys include compiler/rustdoc/Clippy binaries and sysroot, linker/native tools, target and host triples, CPU features, profile/panic/overflow/assertions, flags, Cargo config, feature unification, build dependencies, proc macros, build scripts, source tree and previous outputs read.
- Hash observable filesystem entries including names, modes, symlinks, directory membership, absence and contents. Normalise timestamps, UIDs, locale, paths and permitted environment only where the workload’s contract allows it.
- Test keys include test executable bytes, all spawned executables including `CARGO_BIN_EXE_*`, loaders, shared libraries, tools, fixtures, test name/filter/args, nextest profile, case inventory, isolation class and the complete runtime input root. There are 171 binary-env uses in 102 files in the review inventory.
- The sandbox accounts for reads, execs, mmap, child processes and directory/negative lookups. A forbidden read is a verifier failure even if the program handles ENOENT. A read trace from one passing run is evidence for proposing a declaration, never proof of what another execution might read.
- Start with the full source tree and clean target. Use narrower declarations only for tools whose contract establishes them, with enforced closure, full-checkout controls and audited equivalence. An unexplained path selects the whole root or workspace and runs cold.
- Real time including vDSO, entropy, network responses, scheduling, local ports, CPU capabilities and credentials can change a test. Use a fully controlled clock/seed/fixture only for properties defined on those inputs. Enrolment requires enforcement across the process tree. Real-clock, live-state or race properties execute fresh in their proper class.
- Expose immutable upstream advisory/index snapshots for deterministic analysis. A separate live fetch enforces the existing freshness window and dated exceptions. If the window expires or upstream cannot be observed when required, keep the check blocking.

**Why every reuse is sound**

| Reuse or skip | Completeness condition | Failure if wrong | Detect → automatic response |
|---|---|---|---|
| Tools, Git and dependencies | Origin identity plus verified digest, complete object/dependency closure and platform | Wrong source/compiler silently changes behavior | Verify on ingest and read, exercise cold mirrors → quarantine blob, refetch from authority and invalidate descendants. |
| Package/reverse-dependency selection | Full candidate plan accounts for every required package, feature and non-code input | A hidden dependent or new test vanishes | Compare clean full inventory, mutate undeclared inputs → disable narrow selection, full plan/build for affected epochs. |
| Compile object, mbx hit or reflink target | Per-unit manifest and provenance valid for all consumed intermediates, including build-script outputs and absent inputs | Stale `OUT_DIR`, proc-macro or Cargo timestamp turns a bad edit green | Reconstruct random units from empty targets at other paths/nodes, compare expected bytes/results → revoke lineage and rebuild cold. |
| Test verdict or deterministic fixture | Actual runtime closure, hermetic inputs, independent inventory, correct case outcomes | A test uses a changed sibling binary/fixture or silently runs zero cases | Mutation and cold replay, compare list vs observed process exits → disable case class, invalidate dependent proofs and re-execute. |
| Same-tree rebase, rerun, stacked/concurrent PR or pre-push | Same complete operation roots. Fresh SHA binding, policy and authorisation. Pre-push ran in Velnor. | A PR inherits another context’s green | Replan at consuming SHA, verify signatures/fences, sample cold → remove receipt, rerun exact heads. |
| Queue speculation, group and main join | GitHub’s actual group/tree, current members and complete obligations match, or individual operations match by key | A changed predecessor hides a regression | Compare actual candidate on delivery and main before promotion → discard prediction, run every miss. |
| Cache prefix/local artifact shortcut | Service authorises and resolves exact scoped entry and bytes. Partial/prefix state supplies no evidence. | Wrong ref, archive version or expired artifact appears available | Cross-ref negative controls and remote readback → disable local shortcut and use qualified service. |
| Packaging, signing, publication | Immutable payload, archive/manifest semantics, attestation identity, destination and version all match | Wrong bits, wrong signer, overwritten tag or double upload | Verify actual downloaded bytes and signature, reconcile remote effects → stop publication, retain evidence for the exact missing or conflicting intent. |
| On-demand skip | Skipping work creates no passing verdict. Current head and group are independently required. | Green leaks to an unverified or stale head | Native GitHub admission probes and state reconciliation → block check, dequeue affected candidate, halt writer on systemic failure. |

Accept a reusable build only if every pre-populated target output that Cargo can consume has a valid key and producer. Reflinking yesterday’s tree and trusting Cargo’s mtime heuristics would bypass this check.
Mount proven predecessor outputs read-only or recreate an explicitly sanitised private target, purge unproven fingerprints/intermediates and stale outputs, seal outputs before tests. If the bridge to Cargo cannot establish per-unit ownership, discard the entire affected target and compile clean.
A PR can upload a candidate blob. The trusted verifier separately checks released command graph, mounted input hashes, process status, expected inventory and output digests, then signs with a key inaccessible to the sandbox. The main controller alone verifies that receipt against its own plan and advances main aliases. Job-produced JSON, a content hash alone or a passing PR check cannot promote anything.
Use a durable unique constraint on operation key and verifier epoch. Requests attach independently. Publish pass only on complete success. Incomplete work supplies at most finished, independently sealed dependencies. Eviction removes availability, which causes a miss.
Before enabling a class, run all candidate hits in shadow with cold controls. Target 3,000 clean comparisons per class after structural proof. At maturity sample 2% of hits, cover every class regularly, and run independent cold landed-tree audits nightly and on every policy/toolchain change.
On the first reused-pass/cold-fail, revoke the class and transitively block affected open heads, groups and releases. Run a clean comparison immediately. Statistics discover defects. They never license a suspected false green.
One full cold audit samples one independently defined control input and serves all audit consumers. The 4 min/merge audit/oracle allowance in §2b assumes, for example, 60 batch runner-min/day at 30 merges/day plus 2 min/merge sampled controls. Publish actuals if volume falls or controls cost more.

**Choose a profile once per product input**

The CI contract includes debug assertions, overflow behavior and the local-mode variant. `cargo test --release` also links test harnesses whose bytes differ from the shipped executable. The generator and helper release profiles use size optimisation, thin LTO and panic=abort. See `ci-helper/Cargo.toml:13–17`, `generator/Cargo.toml:35–39` and `crates/velnor-runner/Cargo.toml:74`.
In the measured product release, the Linux generator build composite occupied 224 s and qualification 3.4 min. With two changing labelled heads, adding the same product-class lane to both is E 2 × (224+204) = 856 s of occupied work. Doing it once on the final changed product input is E 428 s, before savings from shared units.
These numbers price a concrete Linux product lane. They are not a measurement of the isolated release compiler or a promise for every target. Phase 0 benchmarks debug tests, optimised tests, final binary linking and package smoke separately with the same cgroup.
Keep debug verification for each changed input. Build the optimised product exactly once when its final identity is available, on the verified merge candidate if all inputs are fixed, otherwise on main. On main exercise the packaged bytes with installation, CLI/API and target-appropriate smoke tests, then seal them before release is eligible.
The runner host embeds source SHA and helper digest and reads a Git object in `crates/velnor-runner/crates/velnor-runner-host/build.rs:30–43,55–79`. Normal PR/debug builds omit the release identity. A squash or rebase that changes the product’s embedded SHA requires the exact main build. Restricting the optimised lane to that identity avoids silently rebuilding a speculative binary.
For PRs, move any bookkeeping-only commit assertions into a cheap signed head binding and keep invariant code tests in a stable fixture. Until an actual SHA-sensitive test is migrated, re-execute its changed input. A tree-equivalent rebase then costs a proof lookup plus a fresh check write for all qualified PR obligations.

**Test scheduling and the long case**

1. Enumerate tests with the released tool and exact binaries before assigning shards. Keep ignored/expected-empty cases explicit. Reconcile the union of completed cases against that inventory.
2. Schedule expensive cases first using recent timing by binary, case and resource class. Preserve nextest groups, setup scripts, retries=0 for required checks, platform variants, ordering dependencies, service isolation and exclusive resources.
3. Mount shared immutable executables and individual fixture digests near workers. A 1.48 GB archive sent to four workers within 5 s requires about 9.5 Gbit/s aggregate. Move only missing binaries or keep the shard local.
4. Splitting two loops is necessary but insufficient. After excluding them, measured median longest cases are 132.3 s actions-cli, 124.8 s runner-host, 137.6 s local-mode. The corresponding P90s are 141.5/281.5/484.1 s. See `gpt-6.2-astra/design-feedback.md:78–91`.
5. Split the 40 publication cases and six initializer cases by scenario. Build the expensive valid prefix once with production code, close the database, take a consistent snapshot, clone privately, then perform every original mutation, reopen, readback and preservation assertion.
6. Test `Journal::open` with a pre-migrated template, and separately run empty schema, historical upgrades, corrupt schema, migration failure, concurrency and actual process/deadline cases. The 924.8 s receipt-integrity case rebuilt the same valid prefix four times. Preserve all four independent checks.
7. Time setup/body/teardown, CPU-seconds and elapsed time under quiet and contended load. Gate on template open ≤1 s, then longest legitimate case ≤60 s, stretch ≤15 s. Run the old/new inventory on known bad fixtures before retiring the old shape.
8. Improve Cargo incremental isolation and split runner-host by independently changing modules where measurements warrant it. Retain both feature configurations. Benchmark dependency optimisation with assertions and overflow checks fixed. The reported 142.5→47.9 CPU-s trial disabled assertions and paid 764 CPU-s rebuilding dependencies.
9. Deterministic identical work runs once. If repeated historical runs provided independent opportunities to find a timing failure, preserve that coverage with explicit required schedule/seed/load samples or fresh real-system executions. Any failure blocks. Three independent trials detect a p=0.2 fault with probability 0.488, versus 0.2 for one. Measure independence before changing the sample contract.

## 7. Scheduling and the cluster

The five workers supply 480 logical threads and about 627.5 GiB total RAM. The 20-thread control-plane node stays out of the build pool. The inventory places two workers in Germany and three in Finland, with direct documentation for one node at each site. Validate topology labels before scheduling.
On November 18 some workers reached 92–95% CPU and one pod used 83.7 cores despite requesting two. Worker memory limits sum to 281–306% of capacity. The tailrocks, ChainArgos and jackin scale sets have ceilings of 60, 60 and 40 runners. A runner count cannot express spare compiler capacity. See `evidence.md:437–449,502–548`.

**Capacity plan, initial E settings**

| Allocation | Budget | Enforcement |
|---|---:|---|
| Kernel, kubelet, network and node agent | 4 threads and 8 GiB per worker | Reserve cgroups/cpusets. Keep another 4 GiB per worker as eviction headroom. |
| Schedulable total | 5 × 92 = 460 threads, about 567.5 GiB | Count live memory, page-cache pressure, I/O and ephemeral storage as well as requests. |
| Tenant floors under contention | Tailrocks 160 threads, ChainArgos 100, jackin 80, 120 elastic | Starting allocations only. Agree floors with tenant owners. Lend idle shares using work-conserving weighted scheduling. |
| Tailrocks queue/gate reserve | Per site one 16-thread builder plus 2 control threads, 36 of its 160 | Keep capacity immediately reclaimable from batch. Ordinary PRs cannot occupy the last required slot. |
| Build/test classes | Start with 16 threads/24 GiB build, 4/6 GiB test, 1/1 GiB control | Admit on actual available memory. Tune by job profile. Use one Cargo jobserver per allocation, cap test processes separately. |

For example, 49 GiB other requests + 2 × 24 + 8 + 4 leaves 16.5 GiB on a 125.5 GiB worker. A third such build would overcommit it. Size warm pools inside the same budget. Count reachable clone pages and service containers.
Use all available workers for independent shards and package builds. The other tenants and housekeeping can consume the rest of the 480 threads. Opposing tenant demand and a serial dependency both bound what a particular PR can use.
Expose a per-tenant queue ordered by deadline and age. Reserve merge and check publication, schedule labelled PRs fairly, then run main-product preparation, warmers, speculation and audits against their deadlines. Preempt interruptible batch work. Keep release side effects fenced and non-preemptible at commit.
Maintain ready sandboxes and idle single-use identities independently. E targets at both sites are median local backlog 0, P90 ≤2 s eligible-to-start for required jobs, P99 ≤10 s. Track the distribution including empty-pool misses and saturation.
Stock each site with one ready gate and one builder at minimum while operating. Size additional inventory from observed arrivals over the P99 sandbox/JIT refill time. Pool entries carry no credentials from earlier jobs. Refill after a lease without waiting for cleanup of the previous namespace.
Choose the node minimising estimated finish time: queue + missing bytes/measured bandwidth + compile/test critical path + any cross-site control cost. Keep the largest executable, linker inputs and `target/` near their consumers. Steal individual shards when transfer is cheaper than waiting.
Measured FI↔DE RTT is about 35 ms. At 1 Gbit/s, 8.7 GiB requires at least 74.7 s before overhead. Whole-target intersite transfer can consume the entire PR budget. Measure actual bandwidth. Replicate immutable deltas outside foreground latency where practical.
One globally fenced operation index owns successful computation. Each site serves local reads and can stage outputs. Use a durably replicated journal with a single write leader and a fenced failover. A two-site partition cannot make both sides authoritative. Hold new successes/publications on any side that cannot prove ownership.

**Execution classes**

- Ordinary builds use user/mount/PID/network namespaces, read-only tools and sources, a private writable workspace and target, no host Docker socket, bounded PIDs, memory, storage and I/O, and no Kubernetes token.
- Container/service jobs get isolated networks, port allocation, service health checks, cgroups and cleanup. Use rootless OCI/BuildKit where the corpus qualifies it. Allocate a job-private disposable VM for workflows genuinely requiring daemon privileges.
- Product publishers use separate identity and egress, read verified objects and the release manifest, and cannot mutate the compile store. Main promotion keys never reach PR, test or pre-push processes.
- Internal authors can still run buggy build scripts, leak credentials accidentally, fill disks or leave processes around. Scope caches and signing authority by tenant. Destroy all descendants and private writes before recycling capacity.
- Produce Linux amd64 and arm64 as declared. The current workers are x86. Use pinned cross-musl toolchains and an isolated, qualified arm64 Linux guest for target smoke tests. If a property depends on native ARM hardware, require an owner-cluster ARM worker for that property before promotion. Keep macOS a dormant target descriptor.
- Other organisations may retain their existing runners in their namespaces. Velnor’s runner identity, store, budget and credentials are independent. Enforce reservations at node admission as well as in Velnor, since another scheduler otherwise can consume the reserve.

**Host work**

1. Mount fixed-path XFS reflink data volumes with project quotas. Track physical extents and inode exhaustion. Keep the OS disk out of target clones.
2. Pin latency-critical linker/test work to a NUMA-local CPU/memory set where measurements support it. Compare SMT sibling contention, governor, CPU frequency and compiler jobs using the same tree and load.
3. Watch CPU throttling, pressure stall information, major faults, memory reclaim, IO wait and NVMe latency. Rate-limit remote cache fills and GC. Avoid evicting hot executable pages to warm a cold site.
4. Keep manifests and three recent warm generations per root under a measured disk budget. Retain all live release subjects independently from cache eviction. Start optional mbx-cache at a capped size, e.g. E 100 GiB/site, then size it from bytes and time saved.
5. Expose eligible-to-start and placement regret under peak arrivals, one worker lost and one site lost. Stop speculation when it increases P90 latency or infringes a tenant floor. A documented BBR/buffer experiment gave no gain. No generic network sysctl change earns a latency credit.

## 8. The merge queue and GitHub integration

**Trigger contract**

Use a GitHub App subscribed to `pull_request` and `merge_group`. Verify webhook signatures and deduplicate delivery IDs. A durable per-PR state machine stores head, draft status, accepted label-request generation, current proof, cancellation epoch and queue membership.
The required check measures a verified commit. The label authorises starting verification. A missing, cancelled or unrequested result stays blocking even when the label exists.
For the edge case of someone deliberately labelling a draft, apply the more specific “while the label is present, every new push” rule. The normal unlabelled draft stays idle. Draft status independently blocks merge.

| GitHub transition | Action | Required state |
|---|---|---|
| Unlabelled draft opened or pushed | No pipeline, job dispatch or heavy verification. A new head detaches the obsolete run. | Absent/expected, or pending if an older binding must be withdrawn. |
| Marked ready for review | Start nothing and add no label. A never-requested head stays pending. | Preserve only a valid explicit request for this head. |
| `labeled` with the exact verify label | Read the current PR from GitHub, capture its latest head and request generation, and prove its complete graph. An explicit label addition is an independent request even if the PR is still a draft. | Pending on that head until passed. GitHub itself also blocks merging a draft. |
| Other label, edited title/body, comment, reopen, automatic merge setting | No new verification. A change of protected target/policy can withdraw eligibility and require a new explicit request. | Existing valid binding or blocking. |
| `synchronize` while the verify label is present | Atomically subscribe the new head, fence old publisher, detach old work and send cancellation. No wait for cleanup or old workflow completion. | New SHA pending. Old green cannot satisfy it. |
| `synchronize` without an active request | Record head for reconciliation. Start nothing. | Blocking. |
| `unlabeled` for the verify label, or PR closed | Fence before releasing the old worker, cancel outstanding PR interest, withdraw queue eligibility and dequeue if necessary. A future request needs a new label edge. | Revoke an existing green for the active PR, otherwise remain blocking. |
| Converted to draft | No new pipeline. GitHub blocks merge. An existing explicit label subscription continues to apply to future pushes. | Preserve the verified binding only while it remains current. |
| Duplicate delivery or unchanged manually rerun request | Join current request or read its complete proof. No duplicate computation. | Write only if the current binding requires it. |
| `merge_group.checks_requested` | Always verify the exact `merge_group.head_sha`, candidate tree, predecessor base and member heads. It has a separate request. | Pending until the whole group passes. |
| Push to protected main | Keep one real native Actions witness. Verify promotion against the landed SHA/tree. Direct or unexpected main updates execute all missing obligations and cannot release early. | Genuine Actions `Required` witness on the push. |

Read base, draft, current head and labels on every transition that could grant authority. A delivery from an old generation cannot resurrect a removed request.
Reconstruct label and head transitions from the PR timeline. In particular, recovery must distinguish a label addition or eligible labelled push from a ready-for-review event that starts nothing.
Use the trusted workflow as an execution coordinator when GitHub offers it. The App may dispatch a protected workflow with an explicit subject/epoch when no eligible workflow delivery exists. A workflow_dispatch job’s Actions check cannot itself satisfy a PR’s required check under GitHub’s documented event rules. The external App’s status can. [Required check evaluation](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

**How label events arrive**

GitHub documents that a workflow using the repository’s `GITHUB_TOKEN` to apply a PR label does not create a `pull_request.labeled` workflow run. The special handling of opened, synchronize and reopened events is different. Do not build the label path around Actions recursion. [Workflow triggering](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
GitHub documents App subscriptions to the `pull_request` webhook with read access to pull requests. That describes the subscription. It does not establish delivery to this installation for every possible labelling principal. [Webhook events](https://docs.github.com/en/webhooks/webhook-events-and-payloads#pull_request)
Before cutover, in the lab apply/remove the label via UI, user token, a separate App installation and a workflow’s `GITHUB_TOKEN`. For each, compare the visible label timeline, App delivery GUID/action/head, Actions run list and actual verifier request count. Repeat with an intervening push and a missed/reordered delivery.
Fallback is an ETag-aware reconciliation poll on active/open PRs, e.g. every 30 s within API budget, plus App delivery recovery. Diff the actual label timeline and head against the durable cursor. Start only an observed eligible request. Agents applying the owner’s requested label can use a separately scoped installation token. [App webhook deliveries](https://docs.github.com/en/rest/apps/webhooks)

**Required at both boundaries**

1. For `refs/heads/main`, enforce a PR, the GitHub merge queue, the repository’s existing review obligations and a required status context `Velnor / Verified` pinned to the Velnor App’s integration ID. Disable normal bypasses and direct pushes. The same named context must be satisfied on the PR head and on `merge_group.head_sha`. Do not require branches to rebase onto current main just to enter the queue.
2. The App serialises writes by repository, SHA and context. Using the commit-status API, publish `pending`, `failure/error` or `success`. The latest state for that context controls a withdrawal. No same-named job/check run is emitted on a PR test-merge ref. This prevents the synthetic merge ref from masking the required head check. [Commit statuses](https://docs.github.com/en/rest/commits/statuses) · [Ruleset checks and expected source](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)
3. Never implement “not requested” as a conditional job that concludes skipped or neutral. GitHub accepts both as successful required checks. Validate in the merge box and queue API that missing, pending, failed, wrong-App, old-head and removed-request cases block. [Required status behavior](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
4. Enable `ALLGREEN` / require all queue entries to pass. Initially use build concurrency 2, min/max entries to merge 1/1, squash, no batching delay and 20 min response timeout once the P99 fits. Today protection uses HEADGREEN, build depth 2, max merge 10, minimum 1 and timeout 90 min. Queue concurrency and merge limits govern different operations. [Managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
5. SHA-scoped statuses have a real limitation: two open PRs can point to the very same commit. Receipt sharing across them is safe. A shared green alone cannot establish that each PR requested verification. Block the shared context while a known peer at that SHA lacks a valid request. Agents can use distinct head commits with the same tree for instant reuse.
6. To cover a peer opened *after* green, use a PR-scoped admission witness as well. Where supported, require one all-path approval from a dedicated verifier team, issued by its service account only against that PR’s current passing proof. Dismiss stale approvals on updates and withdraw on unlabel. Preserve any independent human review requirements. Lab-test coverage for new, deleted, dotfiles and same-tree commits. This adds no human approval or verification work. [GitHub required reviewer rule](https://github.blog/changelog/2026-02-17-required-reviewer-rule-is-now-generally-available/)
7. A raw GitHub UI label edit and an already executing GitHub queue merge have no atomic shared transaction. On observed withdrawal, post pending, dismiss the PR witness, cancel auto-merge/dequeue, and re-read membership before a group success. For strict pre-revocation of an already-green PR, the project label controller performs those steps *before* removing the GitHub label. Direct concurrent UI edits retain a webhook propagation window. Do not certify instantaneous revocation for that case or enable an unattended route that relies on it.
8. Run barriers in the lab for unlink during check write, two PRs with the same commit, a new SHA while queued and a group replaced during publication. If protection cannot enforce the PR-specific guard on the installed plan, keep ambiguous admissions blocked and require distinct PR head identities before enabling automatic entry. A green on a never-verified commit has no acceptable exception.

**The release gate is an additional consumer**

The existing ruleset requires `Required verdict` and `Required` without a bound integration in the captured snapshot. The current product eligibility body demands the latest same-SHA main push run of `.github/workflows/ci.yml`, correct attempt/authority and exactly one successful Actions job `Required`. See `.github/workflows/product-release.yml:81–86`.
The released helper enumerates effective rules, checks commit membership, push runs and job names, and rejects a foreign App ID for a normal required check. Its PR-only exemption also rejects that check if it exists on the landed SHA, which can equal the queue SHA. See `ci-helper/crates/velnor-ci/src/gate.rs:15,83–99,127–159,241–243,277–289`.
Ship the new helper, generator pins, rules and publisher as one compatibility migration. Explicitly recognise the pinned Velnor App proof, policy epoch, repository/subject and signed main manifest. Still require the genuine Actions 15368 push witness and all unrelated effective rules. Never turn a foreign-App check into an unchecked exemption.
The protected main job independently verifies candidate receipts/output digests, obtains exact-SHA release evidence, signs the promotion record and returns its real conclusion through run-service. For a changed shipping identity it waits for the real optimised product proof, off the PR critical path.
Shadow the new App context and witness on lab refs, exercise wrong App, wrong workflow path or run attempt, same-SHA group→push, duplicate `Required`, missing test and stale tag. Switch protections while admissions are held, read them back, then retire old PR writers.

**Queue execution**

- Only after a passing latest head and required approvals may an agent enqueue. Use GitHub’s `enqueuePullRequest(expectedHeadOid)` or enable auto-merge with the expected head once eligible. These fields were verified by read-only GraphQL schema inspection. Enabling auto-merge never applies the verify label.
- Speculate only for a requested head or an active queue entry, after counters show a gain. Build the ordered prefix from the actual tip, not just main plus this PR. A candidate pays for unique misses only. Reuse a whole PR verdict only for identical trees, complete plans and semantic inputs.
- Historical replays estimated 78–83% exact-tree matches at 2.5–5 min windows, versus 7/45 in the point-in-time sample. These are workload hypotheses. A candidate pre-run saving 40 s on a 75% ready-by-deadline hit mix yields E 30 s saved per opportunity before costs.
- Activate speculation only after paired replay/live shadow shows positive end-to-end gain, wasted runner-minutes <5% and no interactive P90 regression. Disable at pressure or on repeated invalidations. Keep one owner for identical work even when two consumers arrive simultaneously.
- On queue reorder, ejection or main advance, the obsolete request loses authority instantly. Keep sealed outputs for later exact-key matches. Re-plan the whole actual group. A diff to its predecessor cannot establish completeness.
- Increase build depth to 4, then at most 8, only when measured queue residence improves after ejections are charged and the eligible-to-start SLO holds. At a 2% independent false-failure probability, an eight-entry window has a 14.9% chance of at least one such failure.
- GitHub workflow concurrency is no authority lock. Remove the branch-wide cancellation group and the `always()` tail from the admission dependency. A new epoch gets a reserved control slot immediately. Shared unit work keeps running only while needed.

## 9. What would make CI not a bottleneck at all

The absolute minimum **verification work after push is zero** for a tree whose complete proof already exists. The absolute minimum **time to merge** is the causal path through GitHub’s two decisions and final ref update. GitHub supplies no fixed lower-bound duration for that path.

For an eligible, preverified change, the sequence is:

1. GitHub accepts the head. The App observes the active request and confirms the exact head, policy and proof.
2. The head’s required status becomes visible. GitHub admits the PR to its queue under the expected head.
3. GitHub creates an authoritative ordered candidate. Velnor checks membership, tree and every obligation against that candidate.
4. GitHub observes group success and updates main. The main witness and product promotion can finish afterwards.

For an all-receipt plan the App can validate its own proof directly. No coordinator job is necessary to republish a pure observation. If a job-scoped input, output or live operation is required, wait for the native job described in §2.
An illustrative fast path is E 2 s ingress + 1 s proof + 1 s status + 2 s enqueue + M 19 s typical group creation + E 2 s proof + 1 s status + M 26 s typical finalisation = **54 s**.
The 19 and 26 s are observed medians from separate spans. Measure both on the same request before reporting a population percentile.
On a genuinely new input, the lower bound also contains the longest uncompleted causal chain, including serial link, longest unsplittable test, and required live observations. Extra machines can reduce independent work only.
In symbols, finish each graph vertex at its service time plus the maximum finish time of its parents. Subtract eligible work completed before push. Add GitHub’s head and group barriers in their actual order.
Reach that boundary by sealing agent preflight in the same Linux environment, keeping popular roots warm, and computing ordered queue-prefix misses once while other work proceeds. Publish the share of merges whose critical path contains **zero post-push compile or test seconds**.
Report hit-only, small delta, cold build and externally blocked distributions. A draft that waits a day for its label really took a day longer to merge. No infrastructure investment can remove that chosen wait.

## 10. Critique of v2

| v2 section / decision | Change and consequence |
|---|---|
| §1.1, I117–I118, clocks | Retain end-to-end and occupied-slot accounting. Add separate label-to-verdict, human eligibility and final-push clocks. The first CI run on a head only approximates push arrival. |
| §§1.2–1.3, C1/C4/C6 | Close the HEADGREEN gap. Require each PR’s latest head and each actual queue entry to pass. Preserve stochastic confidence with distinct required samples. Drop unconditional triple execution of deterministic inputs. |
| §2, diagnosis | Hosted queues and cancelled-run stalls are real. The 39-job triple understates costs because source qualification, image candidates, previews, state seal and release rebuilds sit outside it. |
| §§3.1–3.4, N57 | Apply Rust-only at the first production admission. Remove interim official ARC execution, fallback lanes, fork conditions and production macOS. Separate scale-set demand, JIT assignment and run-service acquisition. |
| §§3.5–3.6, N58 | Add generated `create-github-app-token` and distinguish action version from protocol generation. Use the latest released binary and a continuously exercised live service. Blanket masking of paths, log lines or ANSI codes can hide differences. |
| §§3.7–3.10 | Keep journalled ownership, warm pools and Rust rollback. Account for real memory/page cache, competing schedulers and a fenced control-plane partition. A scale-set acquire cannot pin a particular request to a JIT identity. |
| §§4.1–4.2, N02/I032 | Put actual runtime and spawned binary digests in the first reusable test key. Seven-day expiry cannot make a clock read deterministic. Share independently verified PR/pre-push output candidates while reserving main aliases for promotion. |
| §§4.3–4.4, trust | A sparse checkout or observed read set can change the tested program’s behavior. Require an enforced, equivalent input contract. Revoke on first discrepancy. Audits detect implementation faults. |
| §5.1, I036/N54 | Keep private XFS clones. Overturn “build once per site”. Build once globally, then replicate bytes. Distinguish mbx’s mutable managed-target no-op from its true object-store restore. |
| §§5.2–5.5, cache budget | Price the remote miss in GB, page-cache duplication and cold-control costs. Retire state-seal and cache-prune together after the immutable replacement proves its coverage. Preserve public semantics when an action requests cache v2. |
| §§6.1–6.2, I013/I068 | Replace push-start and ready/edited-start with the exact label state machine. Keep one App writer for head/group plus the genuine Actions main witness. Migrate the release-gate helper at the same time. |
| §§6.3–6.4, I063 | Keep both feature variants, fix the 125–138 s floor and schedule by case. Add normalised product profiles, exact-byte package tests, intent reconciliation and release-plz’s separate product contract. |
| §§6.5–6.6, I075 | Adopt ALLGREEN immediately. Count actual admissions, group mismatches and all ejections. Enable auto-merge against a verified head. Credit speculation only after measuring saved wall time and wasted capacity. |
| §§7–8, N38 | Reserve physical service at both sites and enforce tenant floors. Reuse warming and housekeeping share the 480-thread budget. Keep tuning experiments reversible and measured. |
| §§9–11, N07 | Put sealed pre-push reuse and cross-PR single flight in the core design now. Remove the proposed Velnor-controlled merge train. GitHub remains the owner of the queue and final merge. |
| §§12–14, risks/open questions | Set monitored failure behavior for App event gaps, publication identity and same-SHA PRs. Pause Darwin in phase 0 and keep Linux ARM validation explicit. Buy hardware only for a measured irreducible resource limit. |

The original decision log already rejected an official fallback in I008 and adopted a release oracle in N58. The conflict is its migration narrative retaining the official runner until cutover (`v2/decisions.md:319–364`). Preserve the binding decision and change the migration.

## 11. Phases

All numbers are exit targets E. No production job may use the C# runner during transition. If native coverage is incomplete, leave that route unavailable and its protection blocking. The lab can run the official reference from day one.

**First five work items, in order**

1. **Phase 0: disable macOS release work.** Remove Darwin/Intel/Homebrew builds, source-preview mac proof and corresponding expected assets from the emitted graph and consumers. Cover product binary, generator, rust-ci-tools and qualification modes. Preserve target descriptors for a future return.
2. **Inventory and instrument the whole surface.** Walk all 16 workflows, composites, release-plz goldens and both generators. Stop branch-wide image/preview/qualification heavy pushes. Reject any emitted hosted runner label or unknown action. Capture timestamps, per-job resources, cache bytes and release filenames.
3. **Prove the minimum native vertical slice in the lab.** Scale-set → JIT → Broker → acquire/renew → one shell step → Results → complete. Run the paired release oracle, restart at every boundary and verify one genuine GitHub Actions main witness.
4. **Prove the label and merge contract on GitHub.** Test all principals, suppressed events, pending/green withdrawal, latest head, duplicate SHA, two queue entries and one main push. Rehearse migration of both required names, effective rules, release eligibility and publisher pins.
5. **Benchmark and qualify the reusable unit.** On the real worker configuration compare clean Cargo, XFS warm clone, remote fill, dev/optimised products and a representative long case. Shadow one hermetic generation/fixture operation and a deliberately omitted input. Demonstrate revocation and a clean rebuild.

| Phase | Delivered boundary | PR verdict P50/P90 | Push-to-merge P50/P90 | Runner-min / merge | Cross-job compile hit | Conformance |
|---|---|---:|---:|---:|---:|---|
| 0, lab and protected configuration | No Darwin/hosted production routes. Events, timings, runner canary and exact-SHA release probe. | ≤12/25 min lab replay | ≤25/60 min lab replay | ≤250 model | 0% credited | 100% of vertical-slice cases. Full matrix coverage reported, production closed |
| 1, native correctness | All supported Linux jobs/actions, complete oracle, all mandatory checks, label flow, main witness, warm tool/source mounts. | ≤8/16 min | ≤16/40 min | ≤180 | ≥25% qualified units | 100% full supported and refusal corpora |
| 2, build/release once | Input graph, verified private snapshots, candidate/main promotion, release assets, package closures, merged test fixtures. | ≤3/7 min | ≤7/18 min | ≤65 | ≥65% | 100%, first export/install and failed-publication recovery pass |
| 3, committed | Cross-PR/pre-push joins, package and completed-case reuse, reserved scheduling and longest case ≤60 s. | **≤90/240 s** | **≤180/480 s** | **≤35** | **≥85%** | **100%**, zero uncategorised differences |
| 4, stretch | More ≤15 s cases, measured queue prediction, most small deltas preflighted, product promotion on hot data. | **≤30/100 s** | **≤90/240 s** | **≤15** | **≥92%** | **100%** |

Measure compile hits as previously verified usable Cargo unit outputs divided by all units required by the released plan, including cold builds and changed units. Report lookup hit rate, CPU-weighted savings, all-receipt groups and critical-path seconds saved separately.
Conformance denominator is all required cases for the advertised release and capability set. A pair missing one side is incomplete and cannot improve the percentage.
Measure all-work runner-minutes including candidate generation, cancelled work, pre-push, distributed shards, release preparation, audits and speculation. Attribute shared cost once in fleet totals. Publish idle pool thread-hours separately.
Require at least 200 matched eligible heads and enough class-specific samples for tail comparisons across busy and quiet periods. Evaluate 300 merges or four weeks, reporting count and confidence intervals. A small class stays provisional.
Keep false-failure targets ≤3% in phase 1 and ≤1% by phase 3. Compare paired green/red under representative load. M 10/124 identical-SHA pairs disagreed in one direction. This is an 8.1% discordance signal, not ten proven flakes.
With zero disagreements in 299 representative independent pairs, the one-sided 95% upper bound is about 1%. Failed cases remain red. A rerun that passes records a second observation, never an automatic pardon.
A failing gate keeps the prior qualified Rust capability or stops it. Never count a reduced suite, a pending main artifact or a disabled check as a successful phase.

## 12. Risks

| Risk | Detection | Automatic containment and recovery |
|---|---|---|
| GitHub protocol drift or runner rejection | Half-hourly live probes, release polling, typed unknown-message/403/version metrics | Stop the capability. Hold checks and publications. Validate the latest Rust client against the released oracle before resuming. |
| Missing App event, stale label or out-of-order completion | Compare delivery GUIDs and PR timeline with authoritative head, request generation and posted status | Reconcile from the cursor. Fence old writers. Withhold success and dequeue on uncertainty. |
| Commit-scoped green seen by a different PR | Test two PRs at one SHA, a late peer, force-push back to an old SHA and manual enqueue | PR-specific admission witness, shared-SHA blocking and fresh member validation. Keep ambiguous routes closed until enforcement is demonstrated. |
| Incomplete key gives a false green | Input-policy violations, inventory mismatch, negative controls, reused-pass/fresh-fail | Revoke key class and all descendants immediately, block releases, recheck affected current heads/landed artifacts cold. No threshold of tolerated false greens. |
| Stale Cargo target, mutable mbx path or nondeterministic build | Cross-node cold digest/inventory comparisons, changed lock/profile controls, unexpected path/mtime reuse | Discard contaminated lineage. Use private clean targets, declare release identity explicitly and re-enrol only after root cause is fixed. |
| Gate helper trusts a self-reported pass | Omit a required case, modify a report, or execute the wrong helper in the lab | Require controller-observed event, execution and expected inventory. Fail the current request and disable that verifier revision. |
| Partial acquire, lock expiry or duplicate publication | Expiring lease, journal/outbox divergence, remote digest differs from intent | Fence sandbox and writer, reconcile by IDs and digest. Hold ambiguous effects, clean only owned resources and never retry by rebuilding. |
| Wrong release SHA, signer, target or tar bytes | Re-download the published subject, verify bundle and compare main manifest, tag and witnessed push run | Stop before making the release public. Preserve immutable bytes for repair. A known wrong digest cannot be overwritten by retry. |
| Retention or CAS garbage collection removes release evidence | Walk main/release manifests, replica integrity and restore drills | Pin release payloads, proofs and attestations independently from cache TTL. Repair lost copies by exact digest. Release waits while a verified copy is unavailable. |
| False red, real race mistaken for flake or missing schedule exposure | Paired results under production-like load, observed CPU vs wall, failed property by case | Keep failures blocking. Preserve independent real-time/schedule obligations. Fix fixture/harness or resource starvation under controls. |
| Disk, page-cache or tenant contention defeats zero queue | P90 admission, PSI, throttling, per-tenant service, OOMs, extents and per-site cold rates | Shed warmers/speculation, reserve gate/merge slots, reduce concurrent builds and route to the site with the earliest measured finish. |
| Loss of node, site, control plane or signing authority | Heartbeat/fencing failures, unsigned outbox, store durability and failover drills | Continue safe reads and leased work only within existing authority. Stop new trusted commits on a partition. Recover fenced leadership and replay. |
| Unknown action, bad cleanup or secret exposure | Transitive admission scan, fixture canaries, masked-log probes and leftover namespace/port inventory | Refuse pre-execution, destroy or quarantine the sandbox, rotate affected credentials and retire the leaking Rust revision. |
| Incorrect ARM or container equivalence | Target-native assertions, ABI checks, exact-output and service/privilege differential corpus | Hold the affected Linux product/capability. Supply an equivalent owner-cluster execution class before marking it green. |
| Better median hides deferred work | First-push and final-push clocks, time to failure, abandoned requests, nightly debt, post-merge regressions and publish latency | Charge the work to its origin, stop speculation on regression, and withhold the next phase until coverage and both tails recover. |

**Acceptance evidence to retain**

- For each visible green, the current subject, policy, full obligation set and verified receipt IDs.
- For each release, the exact main SHA, Actions push run/attempt, product manifest, payload digests, signatures, destination and independently verified readback.
- For each claimed speedup, a paired trace including queue wait, all charged computation, transferred bytes, audit work and failed attempts.
- For each drift incident, the failing oracle fixture, affected protocol/action revision, revoked proofs and the cold rerun that restored confidence.

This is a design and a measured baseline. The native executor, GitHub label-delivery probes, ARM qualification, release rehearsal and latency targets have not been exercised in production.

# Is this the fastest runner? Critique by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Velnor v3 speed review · tailrocks/velnor-new · 19 November 2026

## Verdict

**The claim is unjustified.** Velnor could substantially beat this repository’s historical standard GitHub-hosted and mixed ARC runs. Removing hosted gate queues and reusing verified work explain the expected gain. The repository has no matched end-to-end Velnor result yet. [Historical timings](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/evidence.md:75)

- A tuned warm **ARC** pool and **Velnor with GitHub Official** are unranked against Native. Standard **GitHub-hosted** is a baseline. **GitHub larger runners**, **Namespace**, and **Depot** are credible Actions controls with different compute, startup or cache characteristics. [GitHub larger runners](https://docs.github.com/en/actions/reference/runners/larger-runners) · [ARC warm pools](https://docs.github.com/en/actions/how-tos/manage-runners/use-actions-runner-controller/deploy-runner-scale-sets) · [Namespace cache volumes](https://namespace.so/blog/zero-latency-caching) · [Depot runners](https://depot.dev/docs/github-actions/overview)
- A persistent **Buildkite Elastic CI Stack** agent can keep local state and integrate with GitHub’s merge queue. **Bazel with BuildBuddy RBE** is a credible build/test contender where the rules preserve the required checks. Neither has a comparable velnor-new result. [Buildkite agents](https://buildkite.com/docs/agent/reference-architectures) · [Buildkite merge queue](https://buildkite.com/docs/pipelines/tutorials/github-merge-queue) · [Bazel remote execution](https://bazel.build/remote/rbe) · [BuildBuddy RBE](https://www.buildbuddy.io/docs/rbe-setup/)

Use **“designed to cut velnor-new’s push-to-merge time”** today. A benchmark win can support “fastest measured for velnor-new under these conditions.” The zero-false-green contract still has open proof gaps.

## Where the time goes, per stage

**M** is previously measured, **P** is a measured proxy, **E** is an estimate or replay, and **T** is a target.
I reviewed v3 and its research read-only and ran no CI trials. Vendor sources establish capabilities.
I used the final architecture and the owner’s decisions where the research retains earlier rulings.
Decision 20 makes Official selectable in production with separate receipt keys. Decision 7 keeps GitHub’s merge queue. [Owner decisions](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md:22)

### Historical clocks and v3 forecasts

| Clock | Historical evidence | V3 forecast |
|---|---|---|
| PR verification | **15.8 / 22.0 min** P50/P90, all-hosted, 28 runs. **26.7 / 92.3 min**, mixed ARC + hosted, 77 runs. Both P, run-creation to completion, success and failure included. | Whole-tree + cold witness **6 / 18 min** E. Goal **90 s / 4 min** T. Stretch **30 / 100 s** T. |
| Push to merge | **85 / 457 min** P50/P90 P, 98 already-merged PRs, clock starts at first observable CI creation at final SHA. | Whole-tree **9 / 30 min** E. Goal **3 / 8 min** T. Stretch **100 s / 4 min** T. |
| Occupied capacity | **52.9 / 64.6 / 62.0 runner-min** M, PR / group / push medians for mixed era. **285–347 runner-min per merge** E for CI. | **110 → 37 runner-min** E for modeled generator/helper traffic. Goal **≤50**, stretch **≤20** T. |
| Avoidable arrivals | **1,156/2,558** PR runs cancelled, **469/2,558** unchanged-head edits, **120/159** successful groups repeated as a same-SHA push run, all M. | Label on demand, share qualified records, keep the per-landed-commit cold audit. Report label wait and wasted slot time independently. |

The mixed-era slowdown is evidence for removing the hosted gate bottleneck. On 18 November, 13 successful
PR runs had a 71.8 min median wall time and 57.0 min median for the combined three hosted gate queues.
On 17 November those medians were 16.0 and 5.8 min. The two eras have different arrivals, job graphs and failures.
The modeled 35 s verdict and 85 s landing are for the worked **47% generator/helper-only** class.
The 94 → 34 min phase-zero estimate covers a two-verdict agent cycle with review-fix work and has a different clock.
[Evidence T1–T7](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/evidence.md:42) · [Push-to-merge method](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/proposals/opus-5.5.md:36) · [V3 targets and worked path](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:117) · [Phase-zero arithmetic](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/delivery-plan.md:472)

### Stage budget

| Stage | Present cost and evidence | V3 and remaining work |
|---|---|---|
| Event and plan | V3 cites push → run **1–2 s** and run → first job **2 / 226 s** P50/P90 M. V5 separately finds **3 / 4 s** from main ref update → push run, **1–2 s** group run → first job in 24/24 and rare **122–424 s** push-run creation delays. Hosted Plan median queue **5.5 min** M. | Webhook **1–2 s** E, 10 s reconciliation cadence T. Exact tree resolution, released planning and ledger lookup still run. The poll cadence gives no end-to-end upper bound. |
| Acquire and sandbox | Cluster job creation → job start **13 / 27 s** M, n=1,124. Idle starts can take ≤3 s. ARC pod prep **~6 s** M in the design’s sample, plus Docker probing where used. | Actions acquisition leaves the PR/group path. Sandbox start **≤0.3 s** T, informed by a **0.25 s target clone** M. Queueing for both eligible nodes is a separate span. |
| Checkout | **6 s** per routine job, **12 s** Plan, M. | **≤1 s** T with mirror and sealed bases. Whole-tree verification **0.1–0.3 s** E for 110 MiB. Fetch exact missing objects and verify after misses. |
| Tools, action downloads and dependencies | Tools **10 / 18 s**, mise **2 s**, action downloads **4.4 s**, crate fetch **3.5–6.1 s** M from separate observations. Sources cache missed **48/48** sampled Rust jobs. | Near zero warm, **~3 s** E on a lockfile change. Charge upstream misses and mirror fill work. Native versus Official is a separate A/B. |
| Compile and check | **196–497** crate-build lines per sampled heavy job, **81 s** runner-host test build, **14 s** helper build in the design, M. The **8.5 min** figure describes critical job or fan-out work that includes other steps. | **≤25 s** T (**15 s** E) with eligible warm state. **0.31 s, zero crates** M was same path and unchanged tree. One pre-fetched cold build took **92 s** M with 32 build jobs and idle workers, outside setup and tests. |
| Tests | Helper **10 / 62 s**, runner-host **192 / 417 s** M. Legacy looped tests **258–274 s** and residual **125–138 s** M. Surviving measured work includes **679 s** Git differential suite, **924.8 s** receipt-integrity test and **132 s** actions-cli test. | **≤60 s** T (**10 s** E for the worked class). The slower witness controls each new obligation. The selected unsplit 924.8 s test takes **15.4 min** under its measured conditions. |
| Results and required status | **1.48 GB** artifact upload **36–43 s**, required verdict **49 s** M. Hosted verdict and final Required waits **1.6 / 24.3** and **1.9 / 21.4 min** M. | **≤1 s** each T for results and status after the last receipt. Floor includes receipt validation, durable publication and externally visible GitHub status. |
| Enqueue and group | **7.5 min** green → enqueue median M, **19 s** enqueue → group creation M, historical group run **19.9 / 84.1 min** P. | **≤30 s** T (**~25 s** E) on the worked path. Exact-tree hit forecast **60–75%** E, observed head-tree match **7/45** M. Recompute every group vector, execute fresh obligations and missing witnesses. |
| Landing | **26 s** group run finished → merge median M on old Actions clock. | **26 s** carried into budget. App status acceptance and ALLGREEN queue behavior need direct instrumentation. |
| Post-merge and release | Full push run **21.7 / 59.2 min** P in mixed era. Tool releases **190–434 s**, product releases **4,105–6,238 s** M. | A1 cold audit **≤2 h at P90** T, product bytes built per tip until stamping qualifies. **~15 s** E for promotion with prerequisites ready. Dispatch-to-published also includes Actions acquisition, proof waits and publication. |

[Architecture stage table](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:144) · [V5 offer clocks](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md:35) · [T9–T10 setup and check timings](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/evidence.md:241)

### What sets the floor

For each exact subject, take the longest dependency path from admission to an accepted status.
That path can be a ledger lookup, a fresh C5 check, or the slower witness’s wait + build + test + receipt.
Add GitHub admission, group verification and landing spans in timestamp order, taking overlap where it exists.
The historical **19 + 26 ≈45 s** queue subtotal is a reference for the GitHub-controlled spans. The actual minimum is unknown.
Applying those two medians to the worked **35 s** PR verdict gives **~80 s** before residual group work and green-to-enqueue delay. That leaves **~5 s** in the **85 s** worked example if those assumptions hold on the same trace.
A fully matched group with zero fresh work can finish with proof lookup. A novel group can add another two-witness critical path.
After F01 the old Turso journal is gone. The floor then depends on surviving tests, toolchain/link spans, the uncached obligation vector and loaded capacity.
There is no measured general floor for v3. Summing stage medians or P90s would conceal which delays co-occur.
[Request execution](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:527) · [Queue and surviving tests](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:861)

The cache experiments bound individual choices. The XFS target clone is **0.25 s / 2.5 MB**, versus **8.6 s / 10.75 GiB** for an ext4 copy.
The cross-node mbx hit is **39.9 s** versus **92 s** cold, with **8.7 GiB** transferred. The lockfile edit takes **67.4 s** plain and **88.4 s** local mbx with upload, or **43.4 s** on the second node once the server is warm.
This was one commit on nearly idle workers, with GC disabled and in-memory metadata. The Mac experiment changed checkout paths and both methods rebuilt six workspace crates.
The RustFS loopback test measured **~1.2 s** GET plus **~17 s** extraction for a 190 MiB registry. Extraction dominated restore.
Blaze failed full compatibility across five workspaces and missed a planted Clippy denial. Its busy-Mac subset timing cannot rank future releases, and the observed verdict mismatch excludes the tested build as a replacement gate.
[Cluster cache experiment](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-cluster.md:33) · [Mac cache experiment](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-mac.md:27) · [RustFS experiment](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/rustfs.md:162) · [Blaze experiment](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/blaze.md:62)

## The strongest objections

1. **Warm compilation can retain stale output through an omitted consumer. Likelihood high as a gap in the written rule.**
   Step 3 of §7.3 falls back only when a changed path lies outside *every* unit’s declared inputs.
   A path declared by one unit may have another, undeclared build-script or proc-macro consumer. An unchanged unit’s dep-info check cannot detect a dependency absent from it.
   Two warm generations can share that omission. A1 and 2% cold sampling detect some errors after a green is possible.
   Require complete per-unit dependencies or conservative invalidation. Compare warm/cold outcomes and binary digests on planted overlapping-consumer changes, additions, removals and old mtimes. Every cold failure must block warm publication.
   [Compile invalidation](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:698)

2. **Narrow keys could miss inputs, and wide keys could miss the speed goal. Likelihood high for widespread wide keys.**
   Q5 reproduced a false green from trimming the tree and a verdict change from different Cargo package selection. 79/86 source files using `CARGO_MANIFEST_DIR` climb out of the package and 70 test files list directories.
   A cache hit needs a sound contract before it returns. Define complete observation and fail-closed behavior on trace loss, unresolved paths and unqualified ambient inputs, including clock and randomness.
   Replay full-tree cold controls against enrolled contracts with changed directory membership, absent files, cross-root inputs and observation loss. Demand proven equality or fresh work, then report the fraction of critical-path seconds saved.
   [Keys and audit](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:475) · [Q5 reproduction](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md:125)

3. **Two concurrent witnesses may share the failures that spaced execution detects. Likelihood material, magnitude unknown.**
   Distinct nodes and orders help. At per-run failure probability 0.2, independent twins detect the latent defect with **36%** probability versus **20%** when their outcomes are perfectly correlated.
   The historical **10/124** discordance is a one-direction observation after passing groups. The twins’ joint miss rate remains unknown.
   Pair same-time twins with time-separated witnesses on identical inputs, including shared load, clock-sensitive tests and Docker or service conditions. If equivalence is unresolved, time the conservative group re-execution even for equal trees.
   [Q12](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md:195) · [S1](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/decisions.md:21)

4. **The recovery gate and remote status publication need the same proof as the normal path. Likelihood high for the written recovery exception, unknown for the status race.**
   D15 describes a one-node break-glass pass. C1 and §6.2 require two distinct nodes for a new key. Retain both required witness slots or keep the recovery result blocking.
   Epoch revocation updates a remote status asynchronously. Test the boundary where a delayed successful write or a previously accepted status meets a revoked epoch and a ready-to-land GitHub group.
   Run Lab A against observed GitHub admission and landing, including same-SHA PRs, latest-head races and ALLGREEN prefixes. Pass only if every landed SHA has a valid proof under the declared epoch semantics and recovery meets the same witness rule.
   [Recovery](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:431) · [Publication and revocation](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:566)

5. **The queue tail depends on group misses, timing and ejections. Likelihood high.**
   The 60–75% match estimate comes from replaying **172 entries over three days** with queue-tip prediction and prompt auto-enqueue. The observed **7/45** head-tree comparisons may use PR heads fetched later. Zero of 40 recent PRs used auto-merge.
   At a 25–40% tree-miss rate, the cost of those groups can dominate the tail even with partial key reuse. Record tree equality, qualified obligation hits and the time and work of each ejected or rebuilt prefix.
   Replay actual order and bursts. Verify against live ALLGREEN outcomes at depth 2. Compare depth 4 after group P90 ≤5 min and paired false failures ≤3%.
   [Entry replay](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md:178) · [Final queue settings](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:861)

6. **One reserved merge slot per site cannot guarantee two simultaneously eligible witnesses. Likelihood high at burst load.**
   V3 keeps a request in one site and reserves one 16-CPU merge slot and a separate audit slot there. Distinct compatible nodes, correct generations, RAM and isolation class all matter. Isolated XFS clones used **14.29 GB** of page cache each when fully read.
   A1 qualification can age the newest usable generation. Historical cluster samples reached **92–95% CPU** on several workers.
   Measure second-witness wait, age of trusted generations, NVMe reads, refaults, memory pressure and cold-build rate with other orgs and the audit active. Charge both twins and occupied capacity during cleanup.
   [Placement](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:929) · [Cluster observations](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/evidence.md:502)

7. **The remaining test workload can overwhelm the runner gain. Likelihood high until test work lands.**
   Runner-host deletion removes its journal floor. The **924.8 s** test, **679 s** differential suite and **132 s** test still require real work.
   Measure post-F01 selection and cold/warm paths for every PR class, including toolchain/lockfile, Docker, policy and cross-root edits. Preserve exact assertions and test inventories through splits and verify the gate catches a fault in every transformed coverage class.
   Publish the reweighted repository distribution and the longest surviving test under load.
   [Surviving suites](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:910) · [Delivery T02–T05](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/delivery-plan.md:390)

8. **The executor itself may account for only seconds of the gain. Likelihood high on verification-heavy paths.**
   The **4–5 s** Worker setup and **87–147 ms** Node launch per JavaScript action are component observations. Official shares Velnor’s pools, stores, scheduler and ledger. The App removes Actions admission from the PR/group path for both executor choices.
   Run paired Native/Official trials with the same job types, commands, hardware and storage. Use separate qualified cache namespaces with equal warmth and include first execution after switching identity. Add warm ARC and hosted/Namespace/Depot drop-in controls before attributing the overall difference to Rust.
   [Executor comparison](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:251) · [Decision log A19](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/decisions.md:887)

## Missed speed

These are conditional budgets. Several extend experiments in the plan.

- **Guarantee a pair of distinct warm slots for a ready group.** Admit a pair as a unit and lend idle reservations to work that can yield safely.
  Compare one versus two **16-CPU** merge slots per site on separate eligible nodes, plus the A1 slot.
  Budget **30 s** saved if the second witness currently waits 30 s on the critical path. With a 25–40% group-miss rate that is **7.5–12 s per landing** on average if every miss incurs that wait. Measure idle cost and lost PR capacity.
- **Minimize bytes needed before useful work.** Compare a verified file delta or lazy read-only lower layer with full reflink or mbx seeding.
  At the reported **~215 MB/s**, each **1 GiB** avoided cuts about **5 s** of raw transfer. A **2 GiB** required subset takes **~10 s** of raw transfer versus **~40 s** observed for the full remote hit, before other costs.
  Preserve digests and give shards immutable inputs. Under full reads, sharing one lower layer across four sandboxes could avoid **~43 GB** of duplicate page cache. [Cluster transfer and page cache](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-cluster.md:54)
- **Batch the real Git oracle.** Reuse bare fixture repositories and run compatible glob cases in isolated parallel shards while checking every existing case against Git.
  For **45,000** process invocations, saving **1 ms per case** buys **45 s of aggregate work**. At 16-way perfect balance a fully parallel **679 s** single-lane workload would reach **~42 s** plus startup and serial work.
  Measure case runtime and output equivalence before claiming either gain. [Differential contract](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:912)
- **Stream independent test binaries to their verified lanes as they link.** Preserve exact package sets, feature unification, binary dependencies and receipt binding.
  A **25 s** long link/build branch and **20 s** of independent early-ready tests can overlap by up to **20 s**. Compare with a full link barrier and with the proposed integration-binary consolidation, which changes the available overlap.
  [Check lanes and test layout](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:784)
- **Try per-action remote execution for a cold build lane.** Keep rustc, Clippy, package/feature selections and full runtime fixtures.
  Bazel rules_rust with BuildBuddy RBE can distribute declared compile actions and leave unused outputs remote. If it removes **30 s** of work from the **92 s** cold build’s critical path, charge CAS, scheduling and requalification time against that **30 s** budget.
  Keep both witnesses and the cold evidence the receipt policy requires. [Bazel RBE](https://bazel.build/remote/rbe) · [rules_rust](https://bazelbuild.github.io/rules_rust/) · [BuildBuddy remote output guidance](https://www.buildbuddy.io/docs/remote-runner-introduction/)
- **Move qualified work earlier.** Extend pre-push verification with explicit requests for the exact candidate tree and predicted queue base, including both witnesses.
  Starting a **35 s** verdict path **25 s** before the push saves up to **25 s** when keys remain identical and no fresh check dominates.
  Count pre-push CPU and superseded requests. Measure total verification effort and first actionable red alongside post-push time. [Pre-push receipt contract](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:177)
- **Reduce ordinary-tip release build contention.** Qualify deterministic stamp-after-link with exact-byte smoke tests and attestation, and place release preparation away from reserved witness capacity.
  If **1 in 4** tips changes the release closure, build cost falls from **~8 to ~2 runner-min per merge** before stamping and smoke overhead.
  A release requested immediately after landing must publish only the exact qualified bytes. [Release path and stamping item](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:876)

## What to measure first to earn the "fastest" claim

1. **Qualify the green path before timing it.** Use the released full cold inventory as oracle with identical expected IDs, Cargo configuration, profiles, inputs and release checks.
   Plant failures at plan, compile, test, queue and publication boundaries. Pass with **zero** unexplained result gaps, false greens or missing receipts and fail-closed behavior on ambiguous inputs.
   Keep A1 for every landed commit. Run **about 3,000** representative comparisons per applicable reuse class before graduation. Zero disagreements bounds an independent mismatch rate at roughly **0.1%** with 95% confidence. Reuse also requires a structural proof.
   [Parity inventory](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:784) · [Qualification matrix](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/delivery-plan.md:584)
2. **Establish the real work floor.** Alternate at least **five** runs per setup for each pinned workload slice, then sample loaded workers. Record wall and CPU, linker time, bytes read, peak memory, test IDs and binary digests.
   Cover no-op, leaf edit, wide edit, lockfile, Docker and release. Pass with **no surviving single test over 60 s** for goal and **15 s** for stretch, and both witnesses completed within the changed-tree budget.
   Q7’s **67% CPU** reduction was one legacy test with assertions on and an increased cold build cost. Measure the profile trade-off on surviving suites. [Q7 experiment](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md:149)
3. **Benchmark both the executor and the pipeline.** On equal hardware compare Native, Official and tuned warm ARC with equivalent commands, tool and cache locality and isolation.
   Then compare tuned pipelines on standard and larger hosted, Namespace or Depot, persistent Buildkite and a qualified RBE lane. Match the obligation inventory, ALLGREEN rule and GitHub approval constraints. Publish equal-budget and fastest-available outcomes with CPU-hours and cost.
   Pass a scoped winner claim only when paired intervals for its advantage exclude zero at both P50 and P90 against the fastest eligible comparison. A comparator that cannot run the full contract remains unranked.
4. **Replay arrivals, then measure real landings.** Collect at least **200 matched heads** for each class with a P90 claim and **300 merges** for queue validation. Include peak, quiet, same-tree, miss, cancel, failure, shared org load and ejection cases.
   Trace accepted final labelled push → visible status → accepted enqueue → group ready → group status → landed SHA. Report queue eligibility and cleanup. Pass **≤90 s / 4 min** verdict, **≤3 / 8 min** landing and **≤50 runner-min per merge** before claiming the goal.
   Report unsuccessful and superseded attempts and first actionable red alongside success latency. [Metric and targets](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:23) · [Sample decision N40](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/decisions.md:261)
5. **Count correlated failures and release waits.** Compare twins against spaced witnesses with a preregistered joint-miss test and shared-failure cases. Keep spaced group execution if parity remains unresolved.
   Sample both directions of false failures under load. **299 independent representative clean pairs** give a one-sided 95% upper bound of about **1%**.
   Report release dispatch-to-published for fresh landings and stale caches, plus the ready-to-publish subspan. Pass release parity with zero tag compiles, exact verified bytes and an A1 result on the landed SHA.
   [Flake budget](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:101) · [Release gate](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md:876)

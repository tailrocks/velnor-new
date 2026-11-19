# Proposal by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date / run | 2026-11-18 17:13–17:27 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

- CI is slow for three fixable reasons, in this order: 8 tiny gate/lint jobs still queue on GitHub-hosted runners (median 57 min of a 71.8 min wall on 2026-11-18), every merge verifies the same 39 jobs three times, and cluster jobs run cold because caches almost always miss; compilation itself is only fourth.
- Move every job onto the cluster, verify each tree once, and make node-local caches hit: a typical 1–3-crate PR then reaches a verdict in 4–6 min (estimated, arithmetic in Q5), a full-width PR in 10–14 min, at the same confidence as today.
- Keep GitHub compatibility at the wire (scale-set, broker, run service, Results) and in workflow YAML; redesign everything behind it.
- Build one Rust controller (from velnor-new's journal/ownership code) with two execution lanes: Lane O runs the unmodified official runner for arbitrary workflows, Lane N runs a native Rust executor for velnor-gen's restricted profile only; Lane O ships first, Lane N is gated on measured win.
- Replace ARC's cold pod-per-job + emptyDir with warm slot pools, node-local content-addressed caches, and warm-node placement; keep Kubernetes, drop ARC.
- Keep mbx as the compile-cache engine but wire its remote to a cluster service; reject blaze for merge verdicts because it silently substitutes its own nightly toolchain for the pinned 1.99.1.
- The "native Rust beats JavaScript actions" hypothesis is the smallest lever (seconds per job); queue elimination, deduplication and cache locality are worth tens of minutes — fund them first.
- Every phase below ends at a numeric gate on velnor-new itself; no phase is "done" on a benchmark alone.

## 1. Where the time goes today

Method note. Numbers marked **[E]** come from the evidence pack (`evidence.md` sections A–F, `gh api` at 2026-11-18T16:54Z, 3,009 CI runs 11-04..11-18, 200-run job sample). Numbers marked **[M]** I re-measured read-only in this session (2026-11-18 ~17:00–17:15Z: `gh api`, `kubectl get/top/describe`, code reads; repo heads match the pack: velnor-new `95de9b704b`, legacy `1b9fc038`, runner v2.338.0). Numbers marked **[Est]** show their arithmetic. Where I disagree with the pack or the brief, I say so.

### 1.1 The headline: queueing on hosted runners dominates, and it is volatile

| Quantity | Value | Source |
|---|---|---|
| PR wall, 14-day median / p90 | 17.5 / 60.0 min | [E] T1 |
| PR wall, scale-set era (11-15..18) median / p90 | 26.7 / 92.3 min | [E] T2 |
| PR wall by day (population medians) | 11-16: 19.2, 11-17: 16.2, 11-18: 74.7 min | [E] T1 |
| Gate queue (Plan + Required verdict + Required), 11-18 PR | 57.0 of 71.8 min wall (79%) | [E] T5 |
| Same gate queue, 11-16 / 11-17 | 12.6 / 5.8 min | [E] T5 |
| Cluster job queue, median / p90 | 0.2 / 0.3 min (max 4.5) | [E] T4 |
| Hosted job queue in scale-set era, median / p90 / max | 4.1 / 23.6 / 49.2 min | [E] T4 |
| Queued at 2026-11-18T16:54Z | 86 jobs, all hosted, 0 on cluster | [E] T11 |
| Queued when I sampled (~17:05Z) | 1 queued run org-wide for the repo | [M] `gh api runs?status=queued` |

Three corrections to the brief's framing follow. First, "~92 min median" is the scale-set-era **p90**, reached as a median only during the 11-18 ~01:00–13:00Z hosted-pool collapse [E, Discrepancies]. The honest statement is worse in a different way: the median tripled day-over-day with no change in our code, because the critical path includes a shared external queue we do not control. Second, the 86 queued jobs included 25 non-CI jobs (Source qualification, Job image candidate) [E] T11 — hosted-pool pressure is partly self-inflicted by our own auxiliary workflows, not only GitHub's other tenants. Third, my live sample found the queue already drained an hour later [M]: any design justified by "the queue is always 57 min" overfits one regime. The design requirement is to **remove hosted queue variance from the critical path entirely**, which pays 5–13 min on good days and ~57 min on bad days.

The gate structure is the amplifier. Every run is Plan (hosted) → fan-out (cluster, fast) → Required verdict (hosted) → Required (hosted) (`.github/workflows/ci.yml:367` plan; required chain per [E] C.1). The fan-out span itself is only 8–13 min even on 11-18 [E] T5. Three sub-minute jobs (Plan 0.9 min, verdict 0.7, Required 0.1 [E] T3/T4) serialize the whole run behind hosted scheduling — twice, because verdict and gate run sequentially. Their hosted placement is deliberate ("keep the merge verdict, signatures and credentials off shared self-hosted nodes", `velnor-runners/state/RESULT.md` multi-org section), so the fix is not "just move them" but "give them a trusted, pre-warmed lane on our hardware" (Q3, Q6).

### 1.2 Execution: a few heavy jobs, mostly compiling and testing cold

Per PR run (scale-set era): 39 jobs, 52.9 runner-min median (merge_group 64.6, push 62.0) [E] T2. The brief's "~73 runner-min" is between the merge_group median and p90, not a PR figure [E]. Ranked cost [E] T3:

- Check / maintenance-helpers: 8.4 runner-min/run, 486 s of its 508 s in one opaque "Execute named check" step, 104 crates compiled, **zero cache steps** [E] T9a/T10. This is the single largest job and it is uncached by construction.
- Rust / velnor-actions-cli: 6.6 runner-min; tests 238 s, image-fixture prep 89 s (23%), clippy 36 s [E] T10.
- Rust / velnor-runner-host + (local-mode): 11.7 runner-min combined (the brief's ~18 holds only on 11-18, when contention stretched both) [E]; test steps 192 s / 246 s median with p90 417/497 s — a long, variable tail (journal-schema tests; evidence [E] D.5 saw single tests >540 s locally).
- Everything else is ≤3.4 runner-min; the long tail of 1–2 min crate jobs still pays full per-job setup (checkout, mise, tool prep ~10 s median × 3,636 sampled jobs = 706 runner-min of "Prepare pinned tools" alone [E] T9b).

Setup vs work split within a typical Rust job [E] T9b/T10: checkout ~6 s, mise + tool prep ~12 s, cache-restore attempts ~2 s (they miss, see below), then build-test-executables + parallel clippy/tests/doc = 80–95% of the job. Downloads are small but constant: 435–522 crates downloaded per top job [E] T9a because the sources cache misses (86 MB, hits only on hosted Plan). So inside cluster jobs, the order is: **cold compilation+tests ≫ downloads > setup > cache transfer (≈0, because nothing transfers)**.

Contention is already visible. Top-job durations rose monotonically 11-15→11-18 (runner-host 3.5→8.8 min median [E] T6). Live, I observed 4 of 5 workers at 87–95% CPU while velnor-105 sat at 9%, and single pods bursting to 88, 60, 41 cores [M] `kubectl top nodes/pods`. Pods request 2 CPU with **no CPU limit** [M] pod spec; the scheduler therefore packs by a fiction and cargo's `num-cpus` (nextest `test-threads = num-cpus`, `.config/nextest.toml:9` [M]) explodes to 96 threads per pod. The brief's "cluster at 18–29% CPU" and the pack's "92–95%" are both true samples of a bursty, unbalanced system — neither is a capacity statement. Capacity arithmetic: 5×96 = 480 threads; at 52.9 runner-min/PR and ~170 PR runs/day plus merge/push triplication [E] T1/T7, offered load is roughly 52.9×(170+50+60)/1440 ≈ 10 runner-equivalents average [Est] — trivially small against 480 threads **if** runner-minutes meant CPU-minutes, which they do not under unlimited bursting. The cluster is not too small; it is unmanaged.

### 1.3 Caches: effectively cold on every cluster job

- Cargo sources cache: not found in 48/48 Rust job logs; Plan on hosted hits the same key family 10/10 [E] T9a. Root cause is scoping/placement, not size (86 MB).
- Rust toolchain: "no restored Rust toolchain" in 58/58 logs [E] — cluster jobs install/verify per job; the runner image seeds tools but explicitly not Rust (`RESULT.md` 2026-11-15.4: "Not baked in: the Rust toolchain… Reusing a prebuilt toolchain needs a generator change" [M]).
- mbx: `MBX_REMOTE_URL` empty in 68/68 logs, 0 bytes transferred; local object-cache hits 40–74% **within** a job's passes [E] T9a. Cross-job reuse is zero: every pod's `emptyDir` dies with the job [M] pod volumes. The within-job hit rate proves the keys are stable; only persistence is missing.
- State snapshots: restore skipped in 3,636/3,636 jobs [E] T9b. State seal succeeded only for generator (6/18) and ci-helper (4), never root/runner/local [E] T9d. The brief blames the merge-queue tip race; the pack shows only ~half the producer failures are the tip check, the rest fail at Clippy/wait steps [E] — the seal pipeline is both racy **and** fragile, and its 60-min producer chain (`state-seal.yml`, [E] C.3) guarantees staleness even when it works.
- GitHub cache total: ~3.5–3.7 GB in 3–4 entries [E] T9c, [M] `actions/cache/usage` — well under the 10 GB cap, yet useless to cluster jobs. The constraint that shaped this design ("inside GitHub's free limits", `docs/goals.md:45` [M]) does not bind a self-hosted runner with terabytes of local NVMe.

Cache transfer time today is therefore ~0 s and cache value is ~0: the worst combination. Jobs pay restore-step overhead (1–2 s each, 3,636 times [E]) to discover nothing.

### 1.4 Duplicate work: three full verifications per merge, plus cancelled churn

All 39 jobs run on pull_request, merge_group **and** push [E] T7. 120/159 successful merge_group runs have a push run on the **identical SHA** — a bit-for-bit rerun. The PR→merge_group rerun is not purely duplicate: only 7/45 sampled PR heads had a tree equal to the merge_group tree (34/45 were behind base) [E] T7, so the queue genuinely tests a different tree — but it retests all 39 jobs instead of the delta's affected set, which the PR path already knows how to compute ([E] C.4). On top: 45% of PR runs are cancelled (1,156/2,558) [E] T7, and my live sample of the 20 most recent completed CI runs found 20/20 cancelled [M] — agents push faster than CI drains, and each cancelled run still consumed hosted queue slots and cluster CPU before dying. Cancelled-run runner-minutes are excluded from all pack medians [E, Method], so true cost per merge is higher than 3×52.9.

Also on the hosted pool: Source qualification (267 runs on 11-18) and Job image candidate (306) rival CI (348) in run count [E] T11, all competing for the same `ubuntu-26.04` queue that gates merges. Nobody designed that contention; it accreted.

### 1.5 The budget in one table (PR run, scale-set era)

| Component | Good day (11-17) | Bad day (11-18) | Nature |
|---|---|---|---|
| Hosted gate queue (3 jobs) | 5.8 min | 57.0 min | External, volatile [E] |
| Other hosted lint queue (parallel, mostly hidden) | ~2.6 min | ~21.8 min | External [E] |
| Cluster queue | ~0.2 min | ~0.2 min | Ours, healthy [E] |
| Fan-out execution span (longest job chain) | 9.3 min | 10.7 min | Cold compile+test [E] |
| **Wall (success PR)** | **16.0 min** | **71.8 min** | [E] T5 |
| Runner-minutes consumed | ~53 | ~55 | [E] |
| × events per merge | ×~3 (+ cancellations) | ×~3 | [E] |

Conclusion: fix queue ownership first (worth 6–57 min), deduplication second (worth ~2/3 of all runner-minutes and most merge-queue latency), cache locality third (worth most of the 9–11 min span), and only then per-job execution speed. Any architecture that starts by rewriting the step executor optimises the smallest term.

## 2. Hypotheses: verdicts

### H1. "Rust everywhere, native actions instead of JavaScript, speeds CI dramatically because native beats interpreted" — **Fails as stated; a narrow version partly holds**

Evidence against the strong form:

- Where time goes (Q1): JS action execution is not in the top costs. The dominant JS actions in velnor-new's workflows are `actions/checkout` (~6 s median [E] T10), `actions/cache/*` (1–2 s, missing), `actions/upload/download-artifact`, `jdx/mise-action` (2 s), `jdx/mr-boxington-action` (1 s) [E] T9b/C.2. Even **deleting** all of them saves <15 s of a 300–500 s job (~3%) [Est: sum of medians]. Native reimplementations cannot beat deletion.
- The official runner already forces every JS action onto bundled node24 (`HandlerFactory.cs`: `nodeData.NodeVersion = "node24"` [M]; [E] F.5.3), so the "interpreted" cost is V8 startup (~50–200 ms per action) plus the action's actual I/O, which is identical in Rust. Checkout's 6 s is git+network, not JavaScript.
- velnor-legacy built exactly this (26 native adapters, [E] E.1.4; verified `action.rs:378` `native_action_adapter` [M]) and recorded **no accepted speedup**: "no performance improvement accepted yet… optimisation iteration credit remains zero" [E] E.7. Its job-startup/cache-restore latencies were never even measured. The hypothesis has had its experiment; it produced no numbers.
- The one place native code genuinely wins: eliminating per-action **process + download + extract** overhead. The official Worker resolves and downloads every action tarball per job before step 1 (`ActionManager.cs`, [E] F.5.2). For velnor-gen workflows with a fixed, tiny action set, baking those into the runner image or implementing checkout/cache/artifact natively saves the launch-service round-trip and extraction, worth an estimated 2–8 s/job [Est]. Worth doing inside Lane N (Q3); not a reason to build Lane N.

Expected value if pursued alone: ~5–15 s per job, ~0 min off the critical path (jobs run in parallel; the longest job is compile-bound). Verdict: **fails** as a CI-speed strategy; **partly holds** as hygiene for the 5 actions we actually use.

### H2. "A from-scratch Rust-only GitHub-compatible runner beats reusing/wrapping the official runner and ARC" — **Partly holds (long-term), unproven (as the first move)**

For: the official runner's architecture cannot express our two biggest wins. It has no cache/artifact client at all (those live in JS actions via env-injected URLs [E] F.4.2), so cache locality can never be a runner property; it spawns a fresh .NET Worker per job and downloads actions per job [E] F.2.4/F.6; its dual expression evaluators (`PipelineTemplateEvaluatorWrapper` evaluate-and-compare [E] F.5.1) are compatibility scaffolding we would inherit forever. ARC adds a second cold-start chain (long-poll → AcquireJobs → patch replicas → JIT → pod → Listener poll [E] F.7.2; scaler formula verified `scaler.go:339` [M]) with no warm state by design. Wrapping these caps us at "official semantics minus queue".

Against (as step 1): compatibility is a **surface**, not a vibe. The wire protocol is undocumented and version-gated (GitHub stops assigning jobs 63 days after a newer runner release [E] F.6.1); a native runner must track broker/run-service/Results changes forever, with auth migration, lock renewal every 60 s (`JobDispatcher.cs:775` [M]), `already acquired`/`unavailable` semantics, mask hints, OIDC, annotations, step telemetry. Legacy proves feasibility (native lane existed, 8,172/8,222 tests pass [E] E.5) and also the cost: `executor.rs` 37k lines + `runner.rs` 37k lines, 60/100 recent commits `fix`, open incidents of inert supervisors and 600–700% idle CPU [E] E.6. velnor-new's own team already wrote the correct caution: native mode "MUST NOT be simulated by launching the official runner" and needs "one real native job in GitHub before adding more Actions features" (`docs/deferred/self-hosted-runner.md:22,297` [M]) — and then explicitly superseded the restricted interpreter as a prerequisite (:301) in favour of shipping scale-set mode first. That judgement was right and this proposal keeps it.

Expected numbers: Lane-O-only (official runner, our controller/caches) captures Q1's three big terms. Lane N additionally saves an estimated 5–20 s/job (no Worker spawn, no action downloads, no .NET) and, more importantly, enables runner-native cache verbs and build/test co-scheduling that YAML steps cannot express (Q5). That is a real but second-order win: ~1–2 min off a 5-min warm PR [Est], at the price of owning protocol drift. Verdict: build the **controller** from scratch in Rust (that part holds unconditionally — ARC must go); make the **executor** native only for the generated restricted profile, behind measurement gates.

### H3. "Kubernetes is the right base" — **Partly holds**

Holds: we need multi-org tenancy (3 scale sets today [M]), tainted pools for privileged dind, node-local storage, rolling image distribution, and scheduling across 5 heterogeneous workers in 2 sites. k3s already does this; live queue time on-cluster is 0.2 min median [E] — Kubernetes is not the bottleneck. Rewriting a scheduler would be waste.

Fails as currently used: ARC's pod-per-job with `emptyDir`-only volumes [M] throws away everything Kubernetes could keep warm; requests (2 CPU) unrelated to reality produce the burst imbalance in Q1.2; there are no topology labels, so the scheduler cannot know DE from FI (35 ms apart [E] B.5); no priority classes, no CPU manager, no reservations ([M] configz: `cpuManagerPolicy: none`, no kube/system reserved). "Kubernetes lets us use all hardware in parallel" is true only after we fix requests, placement and warmth (Q6, Q8a). Also note the control plane is a single i5 node with no HA [E] B.4.3 — acceptable for CI (jobs survive API blips via lock renewal), but the controller in Q3 must not depend on watch-stream perfection.

Verdict: keep k3s as the substrate; reject the ARC usage pattern on top of it.

### H4. "Local caches on our hardware, with placement that knows the node, beat remote caches" — **Holds (strongest hypothesis)**

Evidence: GitHub cache restore from pods measured 13.6–22.4 MB/s at 120 ms RTT to US blob storage [E] B.4.2; a 1.5 GiB state bundle at 20 MB/s costs ~75 s **if it existed**, which it doesn't (Q1.3). Local NVMe reads the same bytes at GB/s with zero RTT; the difference is not marginal, it is ~50×. mbx's within-job 40–74% hit rate with 0 persistence [E] shows key stability; persisting the store node-locally converts those hits across jobs. The crates mirror already proves the pattern at small scale: `cargo fetch` 3.5–6.1 s → 2.7–3.3 s [E] B.4.2, and it is a mere HTTP proxy — a content-addressed local store does strictly better. Storage is abundant: workers expose 3.8–50 TB imagefs [E] B.1.4 vs GitHub's 10 GB cap that forced the current baroque seal/attest/prune machinery ([E] C.3). Placement-awareness matters because cross-site fetch (35 ms + WireGuard over public internet [E] B.5) would surrender much of the win; same-node ≫ same-site ≫ cross-site ≫ GitHub. Numbers and hit-rate model in Q4.

One qualification: "local beats remote" holds for **our** single-tenant-ish cluster. It does not generalise to the generator's GitHub-hosted users (`docs/goals.md` targets are explicitly "on GitHub-hosted runners" [M]); Q4 keeps both worlds working.

### H5. "Workflows can skip what a change doesn't touch and run exactly the needed verification" — **Holds, and is under-exploited today**

velnor-new already does PR affected-selection (glob map + dependents, docs-only → no Rust jobs [E] C.4) and `docs/goals.md` design rule 2 specifies the correct semantics (unknown file → everything; lockfile → reachability) [M]. But three gaps surrender most of the value: (a) merge_group and push run **everything** regardless ([E] C.4) — selection is disabled exactly on the merge critical path; (b) selection is per-crate-job (39 jobs), so a one-crate change still pays Plan + Ledger + gates on hosted runners; (c) the new generator that fixes the job shape (5 workspace jobs, "number of jobs does not grow with crates", `docs/goals.md` targets [M]; golden at `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml` [M]) is not yet wired to this repo [E] C.6. Test-level selection (skip tests whose crate+deps are unchanged **and** whose binary is cache-identical) is sound only with content-addressed build outputs (mbx keys give us this); without that proof it would cut needed verification and is rejected. With it, H5 + H4 compound: unchanged ⇒ not compiled ⇒ not retested, with the cache key as the correctness argument. Verdict: holds; Q5 extends it to all events with tree-identity reuse.

## 3. Recommended runner architecture

I considered three whole-shape candidates and recommend the third.

- **Candidate A — pure native runner** (legacy native lane, modernised): one Rust binary speaks broker/run-service/Results and executes every job itself. Smallest steady-state latency, largest compatibility risk: it must reimplement composite inlining, container/service containers, OIDC, masking, file commands, and every future GitHub feature before any job using them can run. Legacy's own answer was to bolt on a scale-set lane delegating to the official runner anyway ([E] E.2.3) — i.e. pure-native collapsed into hybrid under contact with real workflows. Rejected as the sole lane.
- **Candidate B — official runner only, better controller** (PR #1324's Kubernetes runtime for velnor-host): keep the unmodified Runner.Listener/Worker, replace ARC with velnor-host managing pods, add node-local caches via volumes and hooks. Captures Q1's big three with zero protocol risk, but permanently forfeits runner-native cache verbs, pays per-job Worker spawn + action downloads, and cannot co-schedule build/test across jobs. Accepted as **phase 1**, rejected as the end state.
- **Candidate C (recommended) — one controller, two lanes**: a single Rust control plane owns scale-set sessions, journal, capacity, placement and caches; each acquired job is admitted to Lane N (native executor) if it matches the generated restricted profile, else Lane O (official runner in a managed pod). Admission is fail-safe: any doubt → Lane O. This is Candidate B that can grow Candidate A's wins job-class by job-class, with the official runner as the permanent correctness oracle and fallback.

### 3.1 Compatibility surface (what stays GitHub's)

| Surface | Decision | Why / source |
|---|---|---|
| Repo, PRs, merge queue, rulesets | Unchanged; `Required verdict` + `Required` stay the required checks | Brief constraint; live ruleset 24396608 verified [M] |
| Scale-set admin API (`_apis/runtime/runnerscalesets`, sessions, `acquirejobs`, `generatejitconfig`, api-version 6.0-preview) | Implemented by our controller (reuse velnor-new `velnor-runner-github`; paths verified `paths.rs:15,20` [M]) | This is how GitHub offers us jobs; ARC/scaleset client is the reference ([E] F.1.3) |
| Broker session + run-service `acquirejob`/`renewjob` (60 s)/`completejob` | Lane O: official runner does it. Lane N: we implement it, transcribed from runner v2.338.0 | Broker message is only a reference; job body comes from run service ([E] F.2.2) |
| Results twirp (step/job logs, summaries, step updates) + live feed | Lane O: official. Lane N: implement `results.services.receiver.Receiver` + `WorkflowStepsUpdate` | ([E] F.3.3); logs must stream live or PR UX regresses |
| Workflow YAML, expressions `${{ }}`, contexts, `if:`, matrix (GitHub-side expansion), file commands (`GITHUB_OUTPUT` etc.) | Fully honoured on Lane O; honoured for the admitted subset on Lane N | Matrix/needs/if at workflow level are resolved by GitHub before we see jobs; step-level `if:`/templates are ours to evaluate ([E] F.5.1) |
| Actions (`uses:`) | Lane O: all, via official download. Lane N: allowlisted native built-ins + composites expanded at admission + `run:` steps; anything else → Lane O | Mirrors legacy admission ([E] E.1.2) and velnor-new's deferred built-ins table ([E] D.6) |
| Cache/artifact/OIDC endpoints as seen by actions | Preserved by **our** in-cluster services speaking v1+v2 cache and artifact protocols (from legacy `gha_cache.rs`) injected via the same env vars the official Worker sets (`ACTIONS_CACHE_URL`, `ACTIONS_RESULTS_URL`, … [E] F.4.1) | Self-hosted job messages carry no `CacheServerUrl`, so unmodified actions silently no-op today ([M] `gha_cache.rs:1-12`); serving the protocol makes user YAML warm with zero edits |
| Runner version gate (63-day deprecation) | Lane O tracks official releases in the image; Lane N reports a current version and must re-qualify on each upstream protocol change | ([E] F.6.1); owner accepts break-fix-quickly (brief) |

Deliberately **not** compatible: legacy pipelines transport (AES-wrapped `PipelineAgentJobRequest` [E] F.2.1), classic `generate-jitconfig`, Windows/macOS jobs, node12/16/20 action runtimes (upstream itself forces node24 [M]), Docker actions built from Dockerfiles on Lane N (Lane O handles them). Dropping these is the owner's stated preference (newest APIs only) and removes whole subsystems.

### 3.2 Module sketch (types first; `todo` bodies ship as scaffold)

Crate layout follows the owner's in-flight move to one root workspace `crates/velnor-*` (brief, decided). New/changed crates only; generator crates are out of scope except where noted.

```text
crates/velnor-runner/                  (exists; keep journal/ownership discipline)
  velnor-runner-core        ids, capacity, lifecycle, CleanupProof      [reuse as-is]
  velnor-runner-github      scale-set wire types/paths, no HTTP         [reuse, extend]
  velnor-runner-host        controller: sessions, admission, placement  [evolve: add K8s runtime]
  velnor-runner-proto       NEW: broker/run-service/Results clients (Lane N)
  velnor-runner-exec        NEW: native step executor (Lane N)
  velnor-runner-expr        NEW: expression engine (from legacy, see 3.3)
  velnor-cache                  NEW: cache service (v1+v2) + artifact service + GC
  velnor-placement              NEW (or host module): warm-node scoring
  velnor-resource-probe         guest probe                              [reuse]
```

Core contracts (signatures are the design; implementations fill in per phase):

```rust
// Admission: total function, fail-safe to Lane O. Runs BEFORE any side effect.
pub enum Lane { Native(NativePlan), Official(OfficialPlan), Reject(Reason) }
pub fn admit(job: &AgentJobRequest, policy: &AdmissionPolicy) -> Lane;

// Runtime: one trait, two impls (Docker for Mac/local, Kubernetes for cluster).
// Rename of PR #1324's sketch: timeouts map to Unknown, never AbsentProven.
pub enum Observation<T> { Present(T), AbsentProven(AbsenceProof), Unknown }
#[async_trait] pub trait Runtime {
    async fn create_worker(&self, spec: &WorkerSpec) -> Result<WorkloadRef>;
    async fn deliver_jit(&self, w: &WorkloadRef, jit: &JitSecret) -> Result<()>;
    async fn wait(&self, w: &WorkloadRef) -> Result<ExitFact>;
    async fn delete(&self, w: &WorkloadRef, proof: &OwnershipProof) -> Result<Observation<()>>;
}

// Cache: content-addressed, trust-scoped. Broken cache = slow, never wrong.
pub struct CacheKey { family: Family, digest: [u8;32], trust: TrustScope }
#[async_trait] pub trait CacheStore {
    async fn get(&self, k: &CacheKey) -> Result<Option<BlobRef>>;   // ref-then-base lookup
    async fn put(&self, k: &CacheKey, src: &Path) -> Result<BlobRef>; // own-scope writes only
}

// Placement: pure scoring over (job cache needs × node warmth × load).
pub fn score(job: &JobNeeds, nodes: &[NodeFact]) -> Vec<(NodeId, Score)>;
```

The controller loop stays journal-first exactly as velnor-new specifies today: "Persist intent, then perform the external call, then record success or uncertainty. Do not hold a database transaction across HTTP or Docker" ([E] D.4.3). Kubernetes objects get the same treatment via UIDs + `preconditions.uid` deletes (PR #1324 s2.4 [E] D.9). One host process runs one runtime kind (`[runtime] kind = docker|kubernetes`, PR #1324 s7) — Mac/local stays Docker, cluster is Kubernetes, shared code above the trait.

### 3.3 What to take from each ancestor, and what not to copy

**From velnor-new (take):** the journal + capacity state machine (`WorkerState` Reserved…Released, `velnor-runner-core/src/lifecycle.rs:9-75` [E]), ownership proofs (`CleanupProof::covers`, delete-only-on-404-by-id), scale-set client, resource-probe, image-manifest/attestation verification, local mode as the developer inner loop. These 185k lines ([E] D.2, 430/430 tests pass) are the hardest-won correctness in either repo — crash recovery and "never delete what you don't own" are precisely what a multi-org shared cluster needs. **Not to copy:** the macOS-first framing as the cluster design centre (launchd, Keychain, OrbStack assumptions stay behind the Docker runtime cfg); `max_jobs = 1` default thinking — cluster slots are plentiful, admission quality is the scarce thing; the journal's historical-schema test cost (single tests >540 s [E] D.5) — cap schema-compat tests to fixtures, because runner-host test time is itself a top-3 CI cost (Q1.2).

**From velnor-legacy (take, by transplant with tests):** (1) the expression engine (`crates/velnor-runner/src/expression/`, 3,565 lines, transcribed from runner v2.337.0 DTExpressions2 [M] mod.rs header) — re-base citations to v2.338.0 and keep its 989-line test module; this is the single highest-value transplant, since expression correctness is where native runners silently diverge. (2) Native checkout (`checkout.rs` + `git_mirror.rs`: mirror fetch, token masking, restricted inputs [E] E.1.4) extended with sparse-checkout, which velnor-new jobs actually use (`/*` minus `docs/**` [E] C.1) and legacy rejected — the one required extension. (3) The cache service `gha_cache.rs` protocol + trust-chain design (ref-then-base, fork namespaces disjoint by hash domain [M] header) — reimplement against Q4's store API rather than lifting 17k lines wholesale. (4) Native rust-cache/sccache adapter semantics and `mbx_store.rs` per-slot layout (one store per trust scope×repo, per-slot `MBX_CACHE_DIR` because mbx serialises on a registrar flock [E] E.1.4). (5) `velnor-bench`'s honesty rules (nothing simulated, environment identity mandatory [E] E.7) as the phase-gate harness pattern. **Not to copy:** the 37k-line `executor.rs`/`runner.rs` monoliths (transplant behaviours behind the small `admit/exec` contracts above, not the files); `velnor-workflow` (318k lines — superseded by velnor-gen); Firecracker backend as a v1 dependency (narrower than Docker lane, no mbx, no cache transport [E] E.3 — revisit only if Q6 isolation measurements demand it); the legacy workflow-generator coupling. **Why legacy was archived:** no reason is recorded anywhere in repo or GitHub beyond "LEGACY, reference only… Current work: velnor-new" and the final generator-migration commits ([E] E.6; I verified the log tail [M]). I therefore treat archival as a **rewrite decision, not a verdict on the native approach** — but its incident history (inert slots, idle-CPU loops, rerun poisoning [E] E.6) is a bug catalogue Lane N's qualification suite must encode as regression tests, especially: results-service rerun ownership (#670), artifact per-run scoping (#671), idle slot supervision (#389/#408).

**From the official runner (take):** wire behaviours as specification — job-message shape (`AgentJobRequestMessage` [E] F.2.3), renewal/cancellation timing, Results upload sealing, mask hints, file-command semantics, `hashFiles` behaviour. Treat v2.338.0 source as the spec legacy's AGENTS.md demands ("protocol source of truth… Never guess" [E] E.0). **Not to copy:** Listener+Worker two-process .NET split with anonymous-pipe job transfer; evaluate-twice-and-compare template evaluation (we evaluate once, against transcribed semantics + differential tests vs Lane O); per-job action resolution via launch service (Lane N resolves at image-build/admission time from a pinned closure); `hashFiles` via spawning node (native glob+SHA, differential-tested); self-updater (our images are immutable; `disableUpdate` stays true as in velnor-new today [E] D.4.2).

**From ARC (take):** the scale-set session discipline (ack only after handling, `X-ScaleSetMaxCapacity` accounting [E] F.7.1) and the Go `scaleset` client as a second protocol reference. **Not to copy:** pod-per-job cold chain (we pre-create warm slots, Q6); `minRunners + assigned` as the only warmth (we keep **cache-warm** slots, not merely idle pods); dind-always (most jobs never touch Docker; privileged sidecars become an on-demand pool, Q6); 10 s unretried controller calls (journal-backed retries instead); EphemeralRunner CRD machinery — our controller creates pods directly under its own journal, because a second reconciler owning the same lifecycle is how you get two sources of truth for "did this job start".

### 3.4 The trust boundary (why Plan/verdict can leave hosted runners)

Hosted placement of Plan/verdict protects three things: merge-verdict integrity, `id-token`/`attestations: write`, and credentials ([M] RESULT.md). Lane design preserves all three on our hardware: (a) Plan and verdict run as **native controller-side computations**, not as jobs executing repo code — plan is `velnor-ci plan` over a read-only checkout verified by tree hash, verdict is report-merging over signed per-job ledgers; neither needs a general executor. (b) Where a job genuinely needs `id-token: write` (state attestation today; releases), it stays on a **dedicated tainted trusted pool** (no PR code, no dind, separate node or node partition) or on GitHub-hosted if the owner prefers — but off the PR critical path, because attestation publishes caches asynchronously (Q4) and releases are not merges. (c) Job `GITHUB_TOKEN`s stay short-lived and repo-scoped as GitHub issues them; our controller never holds a PAT on the job path (App key stays in the controller's secret only, as ARC already ensures [E] F.1.4). Fork PRs: Lane O/N both run them only after GitHub's `all_external_contributors` approval (already set [M] RESULT), in the untrusted pool with restore-only caches (Q4/Q6).

This contradicts `docs/goals.md:16` ("Jobs are always executed by GitHub's official runner") for Lane N jobs. Per brief rules I do not edit that file; it is Owner Question 1. Note goals' own promises (nothing-to-do ⇒ nothing runs; never twice; fast ≠ checked less) are **served** by this design — only the implementation sentence needs the owner's pen.

## 4. The cache system

Design principle: the cache is a **runner subsystem with content-addressed correctness**, not a workflow step shuffling tarballs through GitHub's 10 GB. GitHub Actions cache remains only as the hosted-runner fallback for velnor-gen's external users; on our cluster it is bypassed entirely.

### 4.1 What is cached, keyed how

| Family | Contents | Key (all hashes SHA-256, length-framed) | Size guide [E/Est] |
|---|---|---|---|
| Toolchain | rustup toolchain tree (1.99.1 minimal+clippy+rustfmt, 2 targets) | hash(channel manifest digest + components + targets) — immutable, verified once per node | ~1–2 GB/node, once |
| Tools | mise installs (nextest, mbx, bun, crane, gh, tofu, jactionlint…) | hash(tool pins + versions); baked into runner image **and** node store for out-of-image pins | 60–400 MB/entry today [E] |
| Crate sources | `registry/index+cache`, `git/db` for all 5 lockfiles | hash(sorted lockfile hashes) (= today's sources snapshot [E] C.3) | 86 MB [E] |
| Build outputs (mbx) | per-rustc-invocation outputs via mbx store | mbx's own input keys (paths normalised [M] mbx site) — **do not re-key** | GBs; node budget §4.4 |
| Target views | per-workspace `target/` materialisation | mbx target roots per slot (legacy layout [E] E.1.4) | reuse, not copy |
| Test binaries | nextest archives per (workspace, profile, features) | hash(mbx output closure + test list) | enables build-once/test-many (Q5) |
| Test results | per-binary pass/fail + durations (ledger) | hash(test binary digest + env class) | KBs; powers skip + shard balancing |
| Git objects | bare mirrors per repo | repo id; fetched incrementally | checkout becomes local clone+worktree |
| Container layers | job/test images, dind layers | OCI digests in node containerd + in-cluster registry | pre-pulled (§8a) |
| Job ledgers | signed per-job reports for verdict | run+job id | verdict without artifact round-trips |

Explicitly **not** cached as tarballs any more: whole `target/` directories and opaque `mbx cache export` bundles through GitHub cache. Both exist today only because GitHub cache was the transport ([E] C.3). With a store API, mbx reads/writes objects directly.

### 4.2 Where it lives: three tiers, strict locality order

1. **Node-local NVMe (primary).** Each worker dedicates its fast volume (§8a) to: toolchains, tools, sources, mbx store shards, git mirrors, containerd. Reads are `open()`/reflink, not HTTP. This tier alone should serve >90% of lookups (§4.5).
2. **Same-site shared tier (secondary).** A per-site object service (the `velnor-cache` binary over local NVMe on 2 nodes/site, or MinIO if operations prefer) holding sealed, trusted objects: sources, mbx objects published from trusted builds, test archives. Consulted on node miss; writes replicate lazily. DE jobs never read FI storage in the hot path (35 ms RTT [E] B.5 × thousands of small objects = death by latency; bulk prefetch only).
3. **Origin (last).** crates.io via the existing per-node nginx mirror (DaemonSet live [M]; wire `.velnor/rust-ci.toml`'s registry into **all** lanes — today only the unwired generator consumes it [E] C.3), rustup/static.rust-lang via a similar pull-through, GHCR via in-cluster registry mirror. GitHub Actions cache is **not** in the cluster read path.

The v1/v2 cache-protocol service (§3.1) fronts tiers 1–2 so unmodified `actions/cache` in third-party workflows also lands locally; velnor-gen workflows skip the protocol theatre and use native verbs (`velnor-ci cache` → store API).

### 4.3 Placement on warm nodes

Warmth is per (workspace-root × lock-hash), because that predicts mbx reuse. The controller keeps a per-node warmth index (which roots' stores are populated, store sizes, last-use) fed by slot agents — an extension of `velnor-resource-probe` [E] D.3. Scheduling: `score()` (§3.2) ranks nodes by (a) store present for job's root+lock, (b) same site as the job's siblings (test shards share the test archive), (c) current load/PSI, (d) trust pool match. Jobs carry `JobNeeds{root, lock_hash, est_cores, needs_docker, trust}` derived at Plan time — Plan already computes lock hashes for cache keys today ([E] C.3), so this is plumbing, not new analysis. Implementation via Kubernetes: controller sets `nodeAffinity` preferred terms (not required — a cold node beats queueing) plus pod topology spread for shards. Consistent-hashing fallback: `node = hash(root+lock) mod site_workers` gives stable warmth even before the index is smart, and naturally spreads the 5 workspaces across 5 workers. Anti-affinity cap: never place >40% of one run's shards on one node (a node loss then costs a minority of shards; Q6).

Slot/store layout per node (from legacy `mbx_store.rs` [E] E.1.4, kept because the flock constraint is real): one store per (trust scope × repo), `K=8` slots per store (§8a sizing), each slot an independent `MBX_CACHE_DIR`+target root so 8 jobs build concurrently without registrar serialisation; hardlink/reflink dedup across slots on XFS (§8a). Between jobs a slot is **scrubbed, not deleted**: workdir wiped, store retained. That single sentence is the difference from ARC.

### 4.4 Correctness and trust (PRs vs main vs forks)

Rules (extending [E] C.3 decisions and legacy's trust chain [M]):

- **Content-addressed ⇒ self-verifying.** Every object is stored under its hash; restore verifies hash before use. A poisoned or torn object can only cause a miss, never a wrong build — goals' "broken cache is slow, never wrong" becomes structural instead of procedural.
- **Write scopes.** Trusted builds (push to main, merge_group on the merge tree, scheduled) write the `main` scope. Same-repo PR builds write only their `pr-<n>` scope; they **read** main → own scope. Fork PRs write only `fork-<run>` scope, read main sources/toolchain **only if** the repo is public and no private-dependency outputs are in scope (goals never-compromise 4 [M]); private-dep families are simply absent from fork-visible scopes. No scope ever overwrites another; promotion PR→main happens only by re-keying identical digests after a trusted build reproduces them (mbx keys make this a no-op check).
- **No tip race.** Today's seal requires the producer's SHA to still be main's tip at save time ([E] C.3) — meaningless under content addressing: an object keyed by its inputs is valid regardless of what main did next. Delete the tip check, the 5-producer seal chain and the attestation gate on the read path. Keep attestation only as an audit log of what trusted builds published. This removes the Q1.3 failure class entirely rather than mitigating it.
- **Secrets hygiene.** Private-dependency fetch keeps today's dance (short-lived token, revoked before repo code, `--offline` thereafter, goals 4 [M]). Cache writes scan for credential-shaped paths as defence-in-depth, but the guarantee comes from scoping: anything fetched with a private token is tagged to a scope forks cannot read.
- **GC.** Per-node budget (e.g. 1.5 TB mbx + 200 GB sources/tools on the 3.8 TB nodes; proportionally more on 105/240 [Est from [E] B.1.4 sizes minus OS/containerd headroom]); LRU by last-access with a 14-day floor for main-scope objects, emergency reclaim only under disk pressure (legacy's 15-min-min-idle rule [E] E.1.4 is a sane floor). GC is a node agent, never on the job path (`MBX_GC_AUTO=0` stays).
- **Cache check.** Keep goals rule 3's visible `Cache` check: every run reports compiled-vs-should-have (§5) so warmth regressions are red in the PR, non-blocking. A silent cold cache is how we got here.

### 4.5 Expected hit rates [Est, with model]

Assumptions: typical PR touches 1–3 crates in 1 workspace; dependencies (~435–522 crates downloaded per top job today [E]) unchanged; toolchain pinned for weeks at a time.

| Family | Today | Warm-node target | Basis |
|---|---|---|---|
| Toolchain/tools | 0% restored [E] | ~100% (miss only on pin bump) | immutable keys, per-node install |
| Sources | 0/48 [E] | ≥98% (miss on lock change; incremental fetch covers delta) | 86 MB, lock-keyed |
| mbx objects (per affected workspace job) | 40–74% within-job only [E] | 85–95% of rustc invocations | within-job rate already 40–74% with **zero** history; adding main-scope history covers all unchanged deps; miss = changed crates + dependents |
| Test execution | 100% always run | run only binaries whose digest changed (≠ hit rate; §5) | content-addressed binaries |
| Checkout | full fetch ~6 s | local mirror clone <1 s | bare mirror per node |

Cold-node (new worker, evicted store) first job per workspace ≈ today's cold job minus downloads (sources from site tier). Warm-up is one job deep per (root×node) thanks to stable placement (§4.3) — with 5 roots × 5 nodes worst case 25 cold jobs after a full-cluster wipe, amortised in the first hour. If measured mbx hit rate lands below 75% on warm nodes, Phase 3's gate (§8) fails and we investigate key stability (paths, `SOURCE_DATE_EPOCH`, build-script env) before building more runner — the numbers, not enthusiasm, decide.

## 5. Workflow design for speed

The runner cannot fix a workflow that asks for the wrong work. Four changes, in dependency order: finish the generator takeover, extend selection to every event with tree-identity reuse, build-once/test-many with sharding, and stop paying hosted-queue latency for sub-second logic.

### 5.1 Finish velnor-gen; retire the 39-job shape for this repo

Today's `ci.yml` (2,898 lines, 39 `runs-on` [M]) is generated by the maintained-not-developed engine (`velnor-actions` 0.1.80 [E] C.6); velnor-gen's golden for this repo already shows the target shape: `rust-plan` + 5 workspace jobs (`rust-root`, `rust-ci_helper`, `rust-crates_velnor_runner`, `rust-generator`, `rust-local`) + audit/trim/required/cache [E] C.6, verified in the golden file [M]. Goals design rule 5 is the principle: "Each workspace starts as one job. A job is added for speed only when it shortens the wait by more than its own start-up time" [M]. The 29 per-crate jobs each re-pay checkout/mise/tool-prep/plan-download (~20–30 s [E] T9b) and each re-enters mbx cold-ish; 5 workspace jobs pay it 5 times and share one target view. Per-crate granularity moves **inside** the job, where `velnor-ci` runs fmt/clippy/test/doc per selected crate with one compile (goals rule 4: at most once as metadata + once in full [M]) — instead of today's per-job sequential "Build test executables" followed by parallel clippy/tests/doc that still duplicates metadata builds across jobs ([E] C.1 step layout).

Concretely for velnor-new: wire `rust-ci.yml` from velnor-gen as the required workflow, keep engine-generated non-Rust checks (deny/machete/lint) as tiny native controller checks (§3.4) or single cluster jobs, and delete the per-crate matrix. `check-maintenance-helpers` (today 8.4 runner-min, uncached, opaque [E]) is split by its actual inputs into the owning workspace jobs with caching — an 8-minute uncached grab-bag has no place in the new shape; its bun/python suites become separately selected checks.

### 5.2 Change-aware selection on every event + tree-identity reuse (no double/triple runs)

Keep today's PR selection semantics (glob map + dependents, unknown ⇒ all, `docs/**` non-inputs [E] C.4) and fix its three surrender points:

1. **merge_group selects too.** The merge tree differs from the PR head tree in 38/45 sampled cases [E] T7, so "reuse the PR verdict verbatim" is usually **wrong** — but "rerun all 39 jobs" is wasteful in the opposite direction. Correct rule: compute affected set from `diff(PR-tested-tree, merge-tree)` ∪ `diff(base, merge-tree)` dependents closure; run exactly that on the merge tree. When the PR was up to date (tree-identical, 7/45 [E]), the affected set is empty and merge_group is a ledger check, not a build. Required-verdict logic must accept "verified on tree X, X == merge tree" as green — a tree-hash comparison, not a re-run. This preserves confidence bit-for-bit: every green still means "this exact tree's selected checks passed".
2. **push verifies nothing twice.** 75% of merges already ran the identical SHA in merge_group [E] T7. Push-to-main becomes: confirm tree == merge-verified tree (always true under squash-queue HEADGREEN [M] ruleset parameters), publish caches to main scope, run the cheap post-merge smoke (generator freshness, docs) — **not** a third 39-job run. If push ever lands an unverified tree (queue bypass, admin push), it runs the full affected set then; the rule is tree-based, not event-based, so there is no hole.
3. **Batch the queue.** Ruleset today: `max_entries_to_build: 2, min_entries_to_merge_wait_minutes: 5, grouping HEADGREEN` [M]. With (1), each queue entry's incremental cost is small; raise `max_entries_to_build` to 5 and drop the 5-min minimum wait to 0–1 min once Phase 2 gates pass (§8) — the wait exists to amortise expensive runs that will no longer be expensive. Measure queue throughput (merges/hour) as a first-class metric; two-at-a-time building is a throughput ceiling no runner can fix.

Confidence guardrails, non-negotiable: selection still fails open to **everything** on unmapped files, toolchain/lock/workflow changes, and generator uncertainty (goals rule 2 [M]); a nightly/scheduled full run (all workspaces, all tests, cold-cache audit sample) catches selection bugs within 24 h; the visible non-blocking `Cache`/selection report (goals rule 3) posts selected-vs-skipped with reasons on every PR so a wrong skip is human-visible before merge, not after.

Cancellation waste (45% of PR runs [E]; 20/20 in my live sample [M]): keep `cancel-in-progress` for PRs ([M] `ci.yml:57-59`) but add a 20–30 s **push debounce** in the agent workflow guidance (batch rapid pushes) and make cancelled jobs release slots immediately (controller kills the slot scrub, doesn't wait for pod graceful termination). Do not "fix" this by queueing every push — agents iterating fast is the point; the fix is making the first 30 s of a run cheap (Plan is native, §5.4) so cancellation wastes little.

### 5.3 Build once, test many: sharding across the cluster

Today: `default_shards = 1` [M] `.velnor/config.toml:29-30`; nextest `--partition hash:i/n` support exists unused [E] C.4; `compiler_process_budget = test_process_budget = 2` [M] throttles a 96-thread node to 2 compiler processes per job — a hosted-runner setting fossilised into cluster config.

New shape per workspace job:

1. **Build** (one slot, warm node): `cargo nextest list --list-type binaries-only` equivalent builds all test binaries once through mbx; emit a nextest archive (+ binary digests) to the site tier. Clippy/doc/fmt run concurrently **in the same slot** sharing metadata artifacts.
2. **Test** (N shard slots, same site): shards pull the archive (node/site cache hit) and run `--partition` slices balanced by **recorded per-test durations** (ledger §4.1), not naive hash — hash sharding with 41 s outliers ([E] C.5 priorities) guarantees a straggler shard. Longest-first bin-packing; re-balance weekly from ledgers. Docker-needing groups (`docker-networks` max-threads 4 [E] C.5) go to dind-capable slots only (Q6).
3. **Skip unchanged binaries:** a test binary whose digest + env-class matches a recorded pass on an ancestor tree is not re-run. Soundness argument: identical binary + identical inputs (env class pins image, toolchain, fixtures hash) ⇒ identical result, modulo flakiness/time/network — so (a) flaky-marked and time/network-sensitive groups are **never** skipped, (b) a random 5% audit sample of skippable binaries re-runs every run and any audit failure invalidates skipping for that binary family, (c) nightly full run (§5.2) bounds worst-case staleness to 24 h. This is the only test-skipping I endorse; source-diff-based test guessing without binary identity is rejected.

Shard counts [Est]: runner-host tests 192–246 s median single-slot [E] → 6 shards ≈ 35–50 s + archive pull ~5 s. actions-cli tests 238 s [E] → 4 shards ≈ 60 s. Small workspaces (ci, local: ~10 s tests [E]) stay unsharded — sharding overhead exceeds gain (goals rule 5 again). The generator decides from measured ledgers and writes the reason into the workflow, per goals rule 5; humans don't hand-tune shard counts.

Budgets: replace fixed `=2` with slot-derived values: `CARGO_BUILD_JOBS = slot_cores`, nextest `test-threads = slot_cores` (§8a gives slot sizing: 12 cores/slot ⇒ 8 slots/node). mbx's own permit scheduler (site: 6 jobs peaked at 32 compilers with scheduler vs 162 without [M]) further caps oversubscription when slots share a node — enable it; today nothing does.

### 5.4 Kill the hosted micro-jobs

Plan (0.9 min), Ledger CLI build, Cargo Deny (0.4), Machete (0.2), Jactionlint (0.3), Alint (0.3), verdict (0.7), Required (0.1) [E] T4: total honest work ≈ 3 min, queue cost 6–57 min. In the new design: Plan and verdict are controller-native (§3.4, target <10 s each); deny/machete/lints are single-slot cluster checks in the trusted-readonly pool (no repo code execution beyond parsers; cargo-deny/machete read manifests); Ledger CLI is built once per (source-hash) and cached as a tool (§4.1), not rebuilt per run. The in-flight short-term fix (velnor-actions v0.1.81 moving hosted jobs to the cluster, brief "already decided") is the correct Phase 0 — this proposal assumes it lands and generalises it. **Nothing on the merge critical path runs on GitHub-hosted runners when we're done.** Source qualification / Job image candidate (573 runs/day combined [E] T11) move to the cluster's batch pool or get trigger-tightened; they must never again share a queue with merges.

### 5.5 Minimum honest time to verdict [Est]

Typical PR = 1 workspace affected, 1–3 crates changed, warm node, queue empty:

| Stage | Budget | Basis |
|---|---|---|
| GitHub webhook → scale-set message → acquire | 5–10 s | long-poll already open; ARC achieves 0.2 min queue today [E] |
| Native Plan (diff, select, keys) | 5–10 s | `velnor-ci plan` over mirror; today's Plan job body is 0.9 min **including** hosted setup [E] |
| Slot assign (warm pool, no pod create) | 1–3 s | pre-created slots (Q6) vs pod-create+JIT+Listener poll chain [E] F.7.2 |
| Checkout (local mirror) + toolchain (present) | 1–3 s | §4 |
| Build (mbx warm, changed crates only) | 30–90 s | 85–95% hits (§4.5) on a 60–500 s cold build [E] T10 |
| Clippy/fmt/doc (parallel, metadata mostly hit) | overlaps build | goals rule 4 |
| Tests (sharded, unchanged binaries skipped) | 40–120 s | §5.3 arithmetic |
| Ledger → native verdict → status | 5–10 s | no artifact round-trips |
| **Total push → verdict** | **≈ 2–4 min typical; 4–6 min with queueing/contention headroom** | sum + 50% slack |

Full-width PR (all 5 workspaces, lockfile change): builds dominate; 10–14 min [Est: today's 9–11 min cold span [E] with downloads/setup removed but genuinely-full compilation kept, plus sharding gains]. Docs-only PR: <30 s (no Rust jobs, goals target 1 min [M]). Merge after green PR: +0 s if tree-identical, else + affected-delta run (typically 1–4 min) + queue batching delay. These are targets to be **proven per phase** (§8), not promises; the floor is set by genuinely-changed compilation, which no runner design can cache away.

## 6. Kubernetes use

Keep k3s v1.37.2 (live [M]); change everything about how runner workloads use it. No changes are made from this session (brief rules) — this section is the design the owner applies.

### 6.1 Pod lifecycle: from pod-per-job to warm slot pools

Today: one ephemeral pod + privileged dind sidecar per job, `emptyDir` everything, ~485 pod creates/hour, dind startup-probe failures as background noise (57 Unhealthy events/hour) [E] B.3.4/B.6. Pod create → image present (good: 0 real pulls [E]) → init containers → dind ready (`RUNNER_WAIT_FOR_DOCKER_IN_SECONDS=120` [M]) → Listener JIT → job. Even at its best this chain costs tens of seconds and destroys all warmth.

New lifecycle: **long-lived slot pods** managed by our controller (not ARC, not a Deployment — controller-owned pods under the journal, replaced on image bump or taint). Anatomy:

- Per worker: 8 slots of 12 CPU / 14 GiB (§8a) = 40 cluster slots, plus per-org namespaces preserved (tailrocks/ChainArgos/jackin isolation [M] stays; slots are org-dedicated, never shared across orgs mid-lifetime).
- A slot pod contains: pause/agent container (our slot agent: probe + cache-index reporter + exec server), and **no runner until assigned**. Assignment injects the job: Lane O starts the official runner binary inside the slot's pre-created container (image already present, JIT via per-job Secret mounted tmpfs, never env/args — PR #1324 s4 [E]); Lane N's executor runs steps via the slot agent directly.
- Between jobs: scrub (kill process tree by cgroup, wipe workdir overlay, rotate per-job credentials, verify no leftover mounts/processes via probe) then return slot to pool. Scrub failure ⇒ slot pod is deleted and replaced (rare path pays pod-create; happy path never does). Every N jobs or T hours (suggest N=50/T=24 h [Est]) slots recycle anyway to bound drift.
- dind becomes a **separate on-demand pool**: only jobs whose plan declares Docker (runner-host docker groups, image builds [E] C.5) get a slot with a dind sidecar, on tainted nodes (`velnor.io/dind=true:NoSchedule`, brief's decided direction). Most slots run unprivileged with `RuntimeDefault` seccomp, drop ALL + the 14 caps velnor-host already whitelists ([E] D.4.1). This shrinks the privileged surface from "every job" to "the jobs that asked", and removes dind startup from the common path.

Pre-warming beyond slots: runner images pre-pulled on every worker (already true via import [E] B.3.3; formalise with a DaemonSet pre-pull + in-cluster registry §8a); toolchain/tools pre-seeded per node (§4); git mirrors kept fresh by a per-node fetcher (push webhook → fetch, else 60 s poll); mbx main-scope stores pre-warmed after each main publish by a low-priority "warm" task that materialises target views for the 5 roots on their hashed home nodes (§4.3).

### 6.2 Scheduling, affinity, sites

- Label nodes: `topology.kubernetes.io/zone=de|fi` (missing today [E] B.1.1), `velnor.io/site`, disk class, dind taint where applicable. Site inference today is IP-guessing ([E] B.0) — replace with explicit labels at apply time; I could not verify per-node site read-only for 105/5/190, **confirm before relying on it** (Owner Question 4 records the two doc-confirmed nodes).
- Slot pods: `PodTopologySpread` across nodes per org; job assignment (controller-side, §4.3 scoring) does warm-node choice — Kubernetes schedules **slots**, the controller schedules **jobs into slots**. This two-level scheme is the key structural choice: kube-scheduler never sees per-job churn, so its generic policies stop mattering for latency.
- Cross-site: a run's jobs pin to one site (Plan chooses the site with warmer stores + lower load); shards never straddle the 35 ms link for cache reads. Site-tier replication (main-scope objects) is async bulk transfer off the critical path. If a site dies, the other site's slots absorb load cold-ish (sources/tools replicated; mbx stores re-warm in ~1 job/workspace) — availability without synchronous cross-site anything.
- Scale: slots scale by org demand signals from scale-set statistics (`totalAvailableJobs` etc. [E] F.1.3) — controller grows/shrinks slot counts within node budgets; idle slots cost ~45 MiB each (observed idle pods [E] B.2.2), so keeping 40 warm costs <2 GiB cluster-wide. More servers later buy: linear slot growth until GitHub-side throughput (webhook/message delivery) or merge-queue `max_entries_to_build` binds; the next bottleneck after this design is queue policy (§5.2), not hardware. One more EPYC worker (+8 slots, +20% capacity) is the cheapest future lever; a third site buys little until DE/FI both saturate.

### 6.3 Isolation of untrusted PRs

Threat: PR code is arbitrary code execution by design; the assets are other tenants' caches, the App key, cloud metadata, and the cluster API. Layers (all standard, composed):

- **Pool separation:** `trusted` (push/merge/release; may write main-scope caches; holds no fork code ever), `pr` (same-repo PRs), `fork` (fork PRs post-approval). Separate namespaces + node taints for fork pool initially (1 worker or a CPU partition); merge pools only if utilisation data (§8) shows the split wastes >20% capacity — start strict, relax on evidence.
- **Pod:** no-token ServiceAccount (`automountServiceAccountToken: false`), user namespace / non-root runner user, seccomp RuntimeDefault (Localhost profile from velnor-host when Lane N matures), read-only rootfs + tmpfs workdir, pids/mem/cpu limits enforced (§8a), no hostPath except the node's cache volumes mounted **read-only for main-scope, read-write only into the job's own trust-slot directory** (slot agent mediates writes; job processes never get the store root writable — this is what makes cache poisoning hard even with code exec).
- **Network:** default-deny NetworkPolicy per pool; egress allowlist (GitHub APIs, crates mirror, site cache service, DNS); deny 169.254.169.254, node IPs, kube API, other pools' CIDRs (PR #1324 s6 [E]). Flannel/WireGuard carries only allowed flows; policies enforced regardless of CNI choice (§8a notes flannel's policy limits).
- **dind pool extra:** privileged only there, dedicated tainted nodes (brief decided), fork jobs **never** scheduled to dind slots (Docker-needing fork checks defer to post-approval same-repo re-run or are skipped-with-report — a visible reduced-verdict, never a silent pass; Required must distinguish "skipped: needs trusted Docker" from green).
- **Roadmap (measure first):** gVisor/Kata/Sysbox RuntimeClass for fork pool if escape-risk review demands it; Firecracker (legacy code exists [E] E.3) only if RuntimeClass overhead measurements disappoint. Do not pay microVM complexity before the threat model requires it — start with namespaces+policy+taints, which match the current ARC risk posture while shrinking privileged scope.

### 6.4 What stays ARC-shaped (nothing) and migration safety

ARC remains running untouched until Lane O slots pass qualification (§8 Phase 1 gate), serving a **separate scale set name** (e.g. `velnor` labels on a new set) so workflows cut over by label, repo by repo, with instant rollback by label flip. Running two controllers against one scale set is forbidden (double-acquire); cutover is per-scale-set. ChainArgos/jackin sets migrate last, after tailrocks soaks — multi-tenancy is a reason for caution, not speed.

## 7. mbx and blaze

### 7.1 mbx (Mr. Boxington 1.29.2 pinned [M] `mise.toml:20`): keep, wire properly, understand its limits

What mbx is (from its site, fetched this session [M]): a cargo shim wrapping each rustc invocation, keying outputs by inputs with checkout paths normalised to placeholders, restoring matching outputs instead of compiling, with a shared permit scheduler across concurrent builds and remote publish/restore for CI ("pushes to main publish… pull requests only restore"). Benchmark on its site: same checkout, empty target → 354/354 restored in 1.3 s; next-push CI 18.9 s → 9.2 s with 353/354 restored [M]. That is exactly the §4 build-output family, already integrated as `compile_driver = "mbx"` [M] `.velnor/config.toml:38` — the tragedy is purely operational: `backend: local`, `MBX_REMOTE_URL` empty, stores on `emptyDir` ([E] C.2/T9a), so the measured 40–74% within-job hits evaporate at job end.

Role in this design:

- **The** compile-cache engine for both lanes. Lane O jobs invoke it via the existing mise wrapper; Lane N invokes the same shim so keys stay identical across lanes (a job migrated O→N must hit the same objects — qualify this explicitly, §8).
- Point `MBX_REMOTE_URL` (or its cache-server equivalent per installed version — **verify the 1.29.2 remote protocol during Phase 0 spike**; the site describes GitHub-cache/S3/cache-server options [M], but I did not verify wire compatibility with a custom endpoint in this session) at the **site-tier cache service** (§4.2), with node-local store as L1. If mbx's remote cannot speak to our service without a shim, write the thinnest possible adapter in `velnor-cache` — do not fork mbx.
- Keep today's env discipline (`MBX_CC=0`, `SOURCE_DATE_EPOCH=0`, `MBX_GC_AUTO=0` [E] C.2) — key stability depends on it; add per-slot `MBX_CACHE_DIR` (§4.3) and enable the permit scheduler (§5.3).
- Delete `mbx cache export/import` bundle transport and `mbx clean` per job ([E] C.2) once stores persist; they exist solely to survive ephemerality.
- Watch-items: build-script execution modes (`MBX_BUILD_SCRIPT_EXECUTION` hybrid compat key [E]) are the likeliest key-instability source; Phase 0 measures hit rate per workspace before any runner work bets on it. If a workspace's build scripts defeat caching, fix the build scripts (declare inputs, no network/clock) rather than abandoning mbx — and report per-workspace rates in the Cache check so regressions are visible.

mbx vs sccache: sccache (absent today [M] grep: no `RUSTC_WRAPPER` anywhere) caches rustc outputs too but without cargo-level target management; running both double-keys the same work. Choose mbx (already pinned, measured within-job hits, cargo-aware); reserve sccache only for non-cargo C/C++ if a future need appears. Legacy's compat-mode sccache adapter ([E] E.1.4) stays a reference, not a dependency.

### 7.2 blaze: does not fit the pinned toolchain — reject for verdicts, allow as local pre-check

Facts (README fetched via `gh api` this session [M]): blaze is a daemonised Rust build system claiming 5–20× edit-rebuild speedups (v1 numbers; "v2 has not been measured yet"), keeping the project compiled in memory (few GB RAM), answering `build/check/run/test/bench` only. Decisive sentence: "**blaze is its own toolchain.** It builds every project with its own Rust, `blaze-2026-10-27` (today the nightly of that date), **whatever the project pins**; … Your toolchain and CI stay the final word."

That disqualifies it as a CI compiler for velnor-new three times over:

1. **Pin violation.** velnor-new pins 1.99.1 in `rust-toolchain.toml` [M] and hard-decision 4 makes mise/rust-toolchain agreement a generation-time invariant, failing on mismatch ([E] C.7). A tool that silently substitutes a nightly cannot produce merge verdicts — green would mean "verified by a compiler we don't ship", violating goals' "green means verified" and "with the project's own compiler" ([M] goals promises 2–3). Same confidence (brief) forbids it.
2. **Wrong workload shape.** Blaze's wins are edit-loop rebuilds with a warm in-memory daemon. CI slots are (scrubbed) fresh processes per job; a persistent per-slot blaze daemon is conceivable (§6 slots live long) but then cache correctness rests on blaze's incremental engine rather than content hashes — an unproven, single-vendor, pre-1.0 correctness surface for the merge gate.
3. **Coverage gaps.** No clippy/doc/nextest partitioning story in the README's command set; velnor CI needs all three with pinned semantics.

Permitted role: **developer/agent local pre-check** (`velnor-local` fast feedback before push), clearly labelled non-verdict, if a Phase 0 spike shows it builds velnor-new's workspaces at all. Revisit criteria (all required): blaze honours `rust-toolchain.toml` (or builds with a bit-identical rustc), v2 publishes reproducible benchmarks including cold builds, and a differential campaign (same commits via cargo vs blaze, compare test outcomes over ≥500 runs) shows zero verdict divergences. Until then, every blaze-green still needs cargo — so it can only ever be an accelerator in front of the real gate, never the gate. Budget zero runner-design surface for it now.

## 8a. Servers and cluster tuning

Current state first (all read-only; nothing changed from this session). Live [M]: 5 workers Ready, Debian 13, kernel 6.12.94+deb13-amd64, k3s v1.37.2+k3s1, containerd 2.3.7-k3s1; CPU capacity=allocatable=96, memory 131,579,520 Ki, hugepages 0, PodCIDR 10.42.x.0/24 (node velnor-5 describe). Kubelet configz [M]: `cpuManagerPolicy=none, topologyManagerPolicy=none, kubeReserved=null, systemReserved=null, evictionHard={imagefs 5%, nodefs 5%}, imageGCHigh=85, maxPods=110, cgroupDriver=systemd, failSwapOn=false`. Runner pod [M]: image `velnor-ci-runner:2026-11-16.2`, `IfNotPresent`, requests 2 CPU/4Gi, limit 32Gi mem, no CPU limit, JIT via Secret. Only StorageClass `local-path` [M]. Crates-mirror DaemonSet 6/6 [M]. From owner docs [E] B.1.4/B.0: OS on smallest NVMe (unmirrored), remaining disks in LVM VG `data`/LV `rancher` XFS `defaults,noatime` at `/var/lib/rancher`; per-node disks 237: 2×512 GB; 240: 2×3.84 TB + 3×15 TB; 105: 2×15 TB + 2×3.84 TB; 241/5/190: 2×3.84 TB [M inventory.md]; imagefs 3.8–50 TB vs nodefs 3.78 TB (different filesystems [E]). EPYC 9454P/9454 96 threads (48 cores × SMT‑2), 128 GB [M README]. Cross-site pod ping DE↔FI 35 ms [E] B.5; GH blob RTT 120 ms, 13.6–22.4 MB/s [E]. A BBR+128 MB buffer experiment gave no gain and was reverted [E] B.1.6 — do not retry blindly.

**Not observable read-only** (no ssh, no exec per brief rules): CPU governor/frequency, SMT enablement, NUMA/NPS layout, THP mode, `vm.*`/`fs.*` sysctls, NVMe model/stripe, flannel MTU, kube-proxy mode, where `emptyDir` physically lands ([E] B.1.6). Every recommendation touching these is therefore "measure on the host at apply time, then set" — I give the test and the expected direction, not a fake-precise percentage. Gains below are [Est] unless cited; rank is gain-per-effort for **this** Rust-CI workload. Rollback for every host change: keep the pre-change sysctl/kubelet/config snapshot in the runners repo, apply to **one worker**, soak a day of CI with per-node job-duration comparison (ledger §4), then fleet; any regression ⇒ restore snapshot on that node and stop.

### 8a.1 Ranked changes

| # | Change | Expected gain (evidence) | Risk | Rollback |
|---|---|---|---|---|
| 1 | **Honest slot resources + CPU limits.** Slots: request = limit = 12 CPU, 14 GiB (8 slots/worker: 8×12=96 threads, 8×14=112 GiB < 125 GiB, leaving ~13 GiB system/cache [Est]). Kills the 88-core burst starvation [M] and 3× memory overcommit (limits 306% [E] B.2.3). | Largest single tail fix: p90 job durations (runner-host tests p90 417–497 s vs 192–246 median [E]) are contention-driven; expect p90→~1.2× median [Est]. Throughput ~neutral, latency variance collapses. | A genuinely >12-core-parallel single job (full-workspace build) runs slower than today's accidental 96-core burst — mitigate with one 24-core "build" slot class per node (6×12 + 1×24 = 96) for build stages (§5.3). | Helm/values revert; slots are controller-owned so this is a config flip. |
| 2 | **Dedicated cache volume + containerd on fastest NVMe; XFS reflinks; tmpfs workdir.** Split LV: `cache` (XFS, `noatime,nodiscard`→scheduled `fstrim`, reflink=1 — default on modern mkfs.xfs, verify), `containerd`, keep OS separate. Slot workdir = tmpfs `emptyDir{medium: Memory, sizeLimit}` sized 8–16 GiB; `target` views reflink-copied from cache volume (copy-on-write, ~free until written). | Checkout+target materialisation ~6 s→<1 s [Est]; mbx object reads at NVMe speed vs overlay; eliminates small-file writeback stalls during builds. XFS over ext4 for many-small-files + reflinks; btrfs/ZFS rejected for now (reflinks yes, but operational novelty + CPU cost unmeasured on this fleet). | tmpfs workdir pressure if a job writes >limit (image builds) ⇒ dind/build slots keep disk workdirs. XFS reflink needs kernel ≥4.16 ✓ (6.12 [M]). | Mount/LV changes need node drain; keep old LV intact until soak passes; slots fall back to disk workdir by flag. |
| 3 | **kubeReserved/systemReserved + eviction sanity.** `kubeReserved=cpu=2,memory=4Gi,ephemeral-storage=20Gi`, `systemReserved=cpu=1,memory=2Gi` [Est starting point]; add `evictionHard memory.available<1Gi`; keep image GC 85/80. Today allocatable==capacity [M] so kubelet/containerd compete with 96-thread builds for their own survival. | Prevents the pathological case (kubelet starved during full-burst builds); small steady-state cost (3/96 cores). Gain is stability, not speed — but one kubelet stall costs more than 3 cores forever. | Over-reserving wastes CI capacity; tune from PSI (today near-zero [E] B.2.3 — after #1, re-measure). | kubelet config revert + restart per node. |
| 4 | **mold linker + CI codegen profile.** Bake mold into runner image; generated workflows set `RUSTFLAGS=-C link-arg=-fuse-ld=mold` (via velnor-gen, never hand-edited YAML) **after** an A/B on the two heaviest link targets (runner-host test binaries). CI profile: `debug=0` (already via env in one job [E] C.7 — make universal), `incremental=false` (already `CARGO_INCREMENTAL=0` [E]), keep `opt-level` defaults for test profile (correctness parity with dev). | Link is serial tail work: mold typically 3–8× vs GNU ld on large Rust binaries (upstream claims; **must A/B locally** — Phase 0). If link is 10–20% of build-step time [Est], saves 5–15 s/heavy job. lld is fallback if mold misbehaves with our link scripts. | Linker bugs produce wrong binaries ⇒ gate: full test suite under mold must match ld verdicts over the Phase‑0 corpus before default-on; keep per-workspace opt-out. | One env flag in generator; instantly reversible. |
| 5 | **CPU manager `static` + topology `single-numa-node` for Guaranteed slots** (requires #1 integer CPU + limits). Test both SMT-on (12 threads = 6 cores) pinning vs `nosmt`-style sibling avoidance on one node: build the same workspace 10× each way. EPYC 9454 (Genoa, chiplet) cross-CCD traffic hurts rustc; pinning a 12-thread slot inside one CCD's L3 should help, but **NPS/CCD layout is unverified [unknown above]** — measure `lscpu`/`numactl -H` at apply time before choosing spreads. | 0–15% build-time [Est, wide error bars — chiplet-locality dependent]; also stabilises tail (no migration). If measurement shows <3%, skip: complexity not worth it. | Static policy + topology can strand capacity (fragmentation) and complicates slot sizing; keep Burstable batch pool unpinned. | kubelet flags per node; drain, flip, compare. |
| 6 | **Governor `performance`, THP `madvise`, swap off for CI nodes, dirty-writeback tuning.** At apply time read current values first (unknown today). Set: `cpupower frequency-set -g performance` (EPYC via amd-pstate — verify driver), THP `madvise` (rustc/LLVM benefit from explicit hugepages; `always` risks compaction stalls under many small processes), `swapoff` + `vm.swappiness=0` (failSwapOn=false today [M] — CI must OOM visibly, never swap-thrash invisibly), `vm.dirty_background_ratio=5, dirty_ratio=15` starting point (bulk NVMe absorbs bursts; defaults 10/20 of 128 GB = 12–25 GB dirty is a stall bomb), `fs.file-max`/`inotify.max_user_watches` raised (cargo+nextest+watchers; verify current before raising), `kernel.sched_*`: leave defaults (6.12 EEVDF; no evidence to tune). | Governor: up to 5–10% if currently `schedutil`/`ondemand` with slow ramp on bursty rustc [Est]; ~0 if already performance. Dirty ratios: removes rare multi-second flush stalls (tail, not median). THP madvise: small, workload-dependent. | Performance governor: power/heat cost (Hetzner flat-rate: acceptable). Aggressive dirty ratios increase writeback CPU slightly. | sysctl.d file removal + reboot or live `sysctl -p` restore; all values snapshot-first (§8a intro). |
| 7 | **In-cluster registry (pull-through) + image pre-pull DaemonSet; crates/rustup mirrors as the only egress for tooling.** Registry mirror for ghcr/docker.io (containerd `hosts.toml`), pre-pull current runner+dind+test images on every worker (extends today's manual import [E]), extend crates-mirror pattern to `static.rust-lang.org`. containerd: keep overlayfs snapshotter (default); evaluate `stargz` only if image-pull ever reappears in profiles (today 0 real pulls [E] — don't optimise a zero). | New-node/image-bump rollout minutes→seconds; removes GHCR from job path. Steady-state gain ~0 (already IfNotPresent-hit) — this is operability, ranked below latency items honestly. | Mirror staleness ⇒ pin by digest everywhere (already practice [E]); mirror is cache, origin fallback stays. | Remove `hosts.toml` mirror lines; pre-pull DaemonSet delete. |
| 8 | **Network locality enforcement.** Keep WireGuard pod network; add zone labels (§6.2) + `trafficDistribution: PreferSameZone/Node` on cache services (crates-mirror already PreferSameNode per docs [E]); verify flannel MTU accounts for WireGuard overhead at apply time (unknown today — test 1500→1420-class settings with iperf3 pod-to-pod cross-site and same-site; keep whichever wins, expected effect small same-site since bulk traffic becomes node-local by design §4). Do **not** replace flannel/k3s networking now; Cilium (policy + observability) is a Phase‑4 candidate only if NetworkPolicy enforcement (§6.3) proves flannel insufficient — flannel itself doesn't enforce policy, so budget a policy engine (k3s embedded network-policy controller verification) in Phase 1. | Cross-site bulk avoidance already counted in §4; MTU fix 0–5% on remaining pod traffic [Est]. | CNI swap is the highest-blast-radius change on this list ⇒ deferred deliberately. | MTU: interface config revert. CNI: n/a (not doing it). |
| 9 | **PriorityClasses + scheduler hygiene.** Classes: `ci-trusted`(1000) > `ci-pr`(500) > `ci-batch`(100: qualification, image builds, nightly) with preemption only batch→pr/trusted. Slot pods themselves are long-lived so priority mostly governs slot (re)creation and batch jobs, not per-job latency. Node labels/taints: `velnor.io/pool=trusted|pr|fork|dind`, fork/dind tainted. k3s `local-path` apt only for throwaway PVCs; cache volumes are **node-local static PVs** (or direct hostPath via slot agent mediation §6.3), never `local-path` dynamic provisioning in the hot path. | Protects merge-path slots from batch floods (573 auxiliary runs/day [E]); gain is worst-case latency under load. | Priority misconfiguration can starve batch forever ⇒ batch gets guaranteed minimum slots (2/node off-peak) not just low priority. | Delete classes / revert values. |
| 10 | **Kernel/OS hygiene (low priority).** Stay on Debian 13 + 6.12 LTS (current [M]); `io_uring` nothing to tune for cargo; page-cache is our friend (128 GB, <40% used [E] — let sources/toolchains live in cache; `vm.vfs_cache_pressure` default). NUMA memory policy: default localalloc with CPU pinning (#5) implies node-local memory; without pinning, leave interleave alone (unmeasured). Revisit only with `perf`/PSI evidence. | ~0 expected without measurements saying otherwise. Listed to close the brief's enumeration honestly, not to churn kernels. | Chasing kernel knobs without profiles wastes the effort budget §8 needs elsewhere. | n/a. |

Build-specific summary binding §5+§8a together: slot 12 cores ⇒ `CARGO_BUILD_JOBS=12`, nextest threads 12, mbx permits node-capped at 96; memory per slot 14 GiB fits the observed heaviest build 13.4–16.8 GiB peak ([E] B.3.3/B.2.2) only marginally ⇒ build-class slots get 24 CPU/28 GiB (see #1) and test shards (leaner) 12/14. Test parallelism within a shard stays `num-cpus`-of-slot (cgroup-aware via limits — verify nextest respects cgroup CPU quota; if it reads host CPUs, set `test-threads` explicitly per slot class in generated config).

What more hardware buys (brief asks): +1 worker = +8 slots ≈ +20% throughput, ~zero latency change until slots saturate; faster single-core (newer EPYC/Zen gen) buys cold-build latency directly (rustc is partly serial) — one fast node as the "build" pool could beat two average nodes for p50; more NVMe per node buys nothing until cache working set exceeds ~1.5 TB/node (measure store growth in Phase 1 before believing any disk purchase). Do not buy hardware to fix Q1 — the queue was never hardware.

## 8. Phased plan, gates and risks

Rule for every phase: gate metrics come from **velnor-new's real runs** via `gh api` jobs data + job ledgers (method of [E] A), ≥50 PR runs or ≥3 days per measurement, medians reported with p90. A phase that misses its gate stops; we fix or abandon that line before building on it. Baselines [E]: PR wall median 26.7 min / p90 92.3 (scale-set era), PR runner-min 52.9, per-merge ≈3× that, cache hits ~0%, hosted-queue share 79% of bad-day wall.

### Phase 0 — Stop the bleeding (no new runner; days)

Work: land velnor-actions v0.1.81 moving the 8 hosted jobs to the cluster (already decided, brief); move/trigger-tighten Source qualification + Job image candidate off the hosted queue; wire crates mirror into generated workflows; set `MBX_REMOTE_URL` spike + per-workspace mbx hit-rate measurement with a throwaway persistent volume on one node; mold A/B (§8a #4); blaze build-ability spike (local only, §7.2); add ledger fields (per-job cache hits, per-test durations) if missing.
**Gate:** PR wall median ≤12 min, p90 ≤20 min over ≥3 days; hosted jobs on merge path = 0; measured warm-ish mbx hit rate reported per workspace (any number — Phase 3 bets need it). Runner-min target: ≤53 (no regression; Phase 0 buys wall, not work).

### Phase 1 — Our controller, Lane O slots (weeks; Candidate B)

Work: velnor-host Kubernetes runtime behind the `Runtime` trait (§3.2, PR #1324 steps 1–8 slimmed: trait + K8s pods + JIT Secret + ownership proofs); slot pools (§6.1) with official runner; §8a #1–#3 (honest resources, cache volume, reservations) on all workers; node-local stores v1 (toolchain/tools/sources/git mirrors persistent; mbx store per slot persisted); cache-protocol service (§3.1) so existing YAML caches work; new scale set + label cutover for velnor-new only, ARC fallback intact (§6.4). Qualification: same commit under ARC vs slots, verdicts must match over ≥100 runs incl. failure cases (deferred-doc standard: matching infra failure in both is still failure [E] D.6).
**Gate:** PR wall median ≤9 min; job-start→first-cargo ≤30 s median (goals target [M], now on our runners); sources/toolchain hit ≥95% (§4.5); zero verdict divergences vs ARC in qualification; cluster job queue p90 ≤1 min at 2× current load (synthetic burst + real peaks).

### Phase 2 — Verify once (workflow semantics; weeks, parallelisable with Phase 1 tail)

Work: velnor-gen `rust-ci.yml` becomes required (§5.1); tree-identity verdict reuse + merge_group affected-selection + push de-duplication (§5.2); native Plan/verdict (§5.4); queue settings ↑ (`max_entries_to_build` 5, min-wait ~0) **after** selection soak; nightly full run + selection report. Ruleset changes are owner-applied (this research changes no settings, per brief rules).
**Gate:** runner-min per **merge** (PR+queue+push summed) ≤60 (from ~160 [Est: 52.9+64.6+62 medians, different samples — re-baseline exactly in Phase 0 method]); merges/hour ≥2× baseline; zero cases of unverified tree merging (audit: nightly full run catches 100% of artificially injected selection bugs in a fault-injection exercise before enabling); PR wall median ≤7 min.

### Phase 3 — Build once, test many, warm by construction (weeks)

Work: nextest archives + duration-balanced sharding (§5.3); unchanged-binary skip with 5% audit; warm-node placement scoring (§4.3); site-tier cache service + async replication; §8a #5–#6 only if one-node measurements justify; maintenance-helpers dissolution (§5.1) completed here if not earlier.
**Gate:** typical-PR (1 workspace) wall median ≤5 min, p90 ≤8 min; mbx warm hit ≥85% of rustc invocations on PR jobs; test-phase wall for runner-host workspace ≤60 s (from 192–246 s [E]); audit-sample skip-failure rate = 0 (any hit ⇒ skipping auto-disables for that family — contained by design §5.3).

### Phase 4 — Lane N native executor (only if Phases 1–3 measurements still show executor overhead worth owning)

Work, in strict order: expression transplant + differential fuzzer vs Lane O (random expressions/contexts, ≥1M cases, zero divergences); `velnor-runner-proto` (broker/run-service/Results) proven by **one real native job end-to-end in GitHub** ([E] D.6 standard); native checkout (sparse added) + cache verbs + `run:` steps; admission for velnor-gen profile only; shadow mode (Lane N executes, Lane O verdict counts) ≥200 jobs, then verdict-flip per workspace. Legacy incident regressions (§3.3) as tests throughout.
**Gate to start:** measured Lane-O per-job overhead (Worker spawn + action download + setup theatre) ≥15% of warm PR wall — otherwise Lane N is vanity and the team stops at Phase 3 (a perfectly good outcome). **Gate to flip verdicts:** shadow divergences = 0 over ≥200 jobs incl. cancellations/reruns; warm typical-PR wall ≤4 min median; protocol-drift drill (simulate upstream version bump: time-to-requalify measured, target ≤1 day agent-time).

### Top risks, detection, containment

| Risk | Detection | Containment |
|---|---|---|
| GitHub protocol drift breaks Lane N (63-day gate [E]) | Canary native job hourly on a scratch repo; version-deprecation exit monitored (ARC already surfaces it [E] F.1.2) | Admission flips all jobs to Lane O by config (minutes); Lane O image bump is the only urgent path. Never let Lane N be the only lane. |
| Selection bug merges unverified code | Nightly full run; selection report on PR; fault-injection before Phase 2 gate | Kill-switch: generator flag → select-everything (one PR to restore); tree-hash verdicts make bad merges auditable after the fact. |
| Cache poisoning (PR writes corrupt trusted objects) | Content-hash verification on read; scope-write audit logs; canary rebuilds compare digests | Structural: PR scopes can't write main (§4.4); worst case = self-poisoned PR scope ⇒ wipe scope, jobs go cold-not-wrong. |
| Slot scrub leak (process/files survive into next job) | Slot agent post-scrub attestation (probe: process/mount census vs allowlist); random deep-inspection of recycled slots | Scrub-fail ⇒ pod delete; leak ⇒ quarantine node pool + incident review before re-enable; credentials are per-job regardless. |
| Controller bug double-acquires or loses jobs | Journal reconciliation vs GitHub state on startup (velnor-new discipline [E] D.4.5); acquired-vs-completed ledger diff alerting | Journal-first intents + lock expiry (GitHub reassigns); ARC scale set still warm during Phases 1–2 for label-flip rollback. |
| mbx key instability (hits ≪85%) | Phase 0 per-workspace measurement; Cache check red on regression | Fix build-script hygiene list; if a workspace can't cache, it pays cold builds while others speed up — no big-bang dependency. |
| Cluster single points (1 control-plane node, per-site loss) | Node/API health alerts; slot capacity headroom metric | Jobs survive API outage via 60 s renewals + journal; site loss degrades to single-site cold-ish (§6.2). HA control plane only if outage data demands it. |
| Test-skip hides flake/environment failure | 5% audit re-runs; nightly full; flake ledger (skip-eligible list excludes any test that ever flaked) | Per-family auto-disable on first audit divergence (§5.3); skipping is revocable per binary without deploy. |
| Multi-org blast radius (ChainArgos/jackin share nodes) | Per-org slot/queue dashboards; tailrocks-only until Phase 3 gate | Org-dedicated slots/namespaces; migrate orgs last (§6.4); resource quotas per namespace before any sharing change. |
| Effort sink: Lane N consumes the team (legacy's 800k-line gravity) | Phase 4 entry gate (overhead ≥15% proof required); LOC budget: executor+proto+expr ≤60k lines or stop-review [Est guardrail vs legacy's 74k in two files alone] | Phases 0–3 deliver ~90% of the win without Lane N; abandoning Phase 4 leaves a fast, supportable system. |

## Owner questions

1. **`docs/goals.md:16` amendment.** Lane N contradicts "Jobs are always executed by GitHub's official runner". Approve amending to lane language ("…by GitHub's official runner, or by Velnor's native executor for generated profiles it admits, with the official runner as fallback") — or cap this programme at Phase 3 (Lane O only)? The goals' promises/targets need no change either way; targets' "on GitHub-hosted runners" scoping should gain a self-hosted column when Phase 1 lands.
2. **Tree-identity verdicts.** Approve Required accepting "selected checks passed on tree-identical commit" (no re-run) for merge_group/push (§5.2)? This is the single biggest merge-latency lever and it is a trust-semantics decision, not an engineering one. Includes approving merge_group affected-selection (today: full run by policy [E] C.4).
3. **Queue + ruleset knobs.** Approve (Phase 2, after soak) `max_entries_to_build` 2→5 and `min_entries_to_merge_wait_minutes` 5→0–1 on ruleset 24396608 [M]? And Source qualification / Job image candidate: move to cluster batch pool, tighten triggers, or accept their queue externality explicitly?
4. **Node site truth.** Docs confirm velnor-240=DE, velnor-241=FI ([E] B.0); 105/5/190 are IP-inferred only. Confirm each worker's site for zone labels before §6.2 relies on them — wrong labels would silently pin "same-site" traffic across the 35 ms link.
5. **Trusted-pool placement for `id-token` work.** Releases/attestations: dedicated tainted in-cluster pool (§3.4) or remain GitHub-hosted forever (slower releases, zero merge impact either way)? Also: may Plan/verdict leave hosted runners as controller-native computations, accepting that verdict integrity then rests on our controller + signed ledgers rather than GitHub's machines?
6. **Fork-PR Docker checks.** §6.3 defers Docker-needing checks for fork PRs (no privileged slots for untrusted code). Confirm a visibly-reduced fork verdict ("deferred: needs trusted run") is acceptable, vs requiring forks to be re-run same-repo before merge under all circumstances.
7. **Test-binary skipping.** §5.3's skip-unchanged-binaries (with 5% audit + nightly full) trades a bounded, audited risk for minutes per PR. Approve the policy and the audit rate, or require every selected test to execute on every PR regardless of binary identity (cost: ~1–2 min on typical PRs [Est])?
8. **Phase 4 appetite.** If Phases 0–3 hit ≤5 min typical PRs, is Lane N's remaining ~1 min + protocol ownership worth an open-ended maintenance commitment — or should that effort go to generator features (goals roadmap #574) instead? My recommendation: decide **after** Phase 3 numbers, with the ≥15% overhead gate (§8) as the pre-agreed trigger, not by enthusiasm now.
9. **Hardware follow-on.** Approve in principle (no spend in this research): one additional EPYC worker when slot saturation — not queue anecdotes — shows in Phase 1+ metrics? And a one-node CPU-pinning/NPS measurement window (§8a #5) requiring brief node drains at apply time?

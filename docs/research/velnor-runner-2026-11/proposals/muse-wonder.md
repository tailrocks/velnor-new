# Proposal by Muse Wonder

| Field | Value |
|---|---|
| Model | Muse Wonder |
| Model id | `muse-wonder-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date / run | 2026-11-18 17:13–17:21 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

- The bottleneck today is not compilation speed: on the worst measured day (2026-11-18) 79% of PR wall time was queue wait on 8 GitHub-hosted gate jobs, while cluster jobs queued 0.2 min median [E]. Fixing placement alone recovers most of the regression.
- The next costs are duplicate verification (the same 39 jobs run for PR, merge_group and push; 75% of merges re-verify an identical SHA on push), cold caches (Cargo sources miss 48/48, toolchain miss 58/58, mbx remote unset, state restore skipped 3636/3636), and an unsharded, per-crate job fan-out whose critical path is ~9 min even with zero queue [E].
- Recommended design: keep GitHub as control plane and keep the official runner binary as the step executor for general workflows; replace ARC's generic orchestration in phases with a Velnor-owned scale-set listener/scheduler (reusing `velnor-runner-github` protocol types and the `velnor-host` journal/ownership discipline) that does cache-aware, warm-node placement on node-local NVMe caches. Add a native Rust fast lane only for the restricted `velnor-gen` profile (sequential `velnor-ci` run-steps), ported from velnor-legacy, behind a lab gate — never as the sole merge path until parity is proven.
- With gates on-cluster, push-duplicate removed, warm node-local caches, workspace-level jobs and nextest sharding, the minimum honest PR verdict is ~5 min median / ~8 min p90 at the same checks (estimated, arithmetic in §5), down from 26.7 min median / 92.3 min p90 in the scale-set era [E].
- Hypotheses: H4 (local cache) and H5 (change-aware workflows) hold; H1 (Rust-native actions) and H3 (Kubernetes) partly hold; H2 (from-scratch runner beats wrapping) fails as a general claim — legacy's 804k LOC and archive are the counter-evidence — and holds only for the restricted profile.
- Blaze must not gate merges: it ignores the pinned Rust 1.99.1 toolchain, ships its own nightly, has no clippy/fmt/doc/nextest parity and an incomplete borrow checker. Keep mbx as the compiler cache; point its remote at a cluster-local cache server.
- This design contradicts `docs/goals.md:16` ("jobs are always executed by GitHub's official runner") for the fast lane only; flagged as an owner question in §9. No cluster or GitHub state was changed; cluster reads were `get/describe/top` only.

## 1. Where does the time go today?

### 1.1 Method and provenance

Numbers below are tagged **[E]** (evidence pack, `gh api` window 2026-11-04..11-18, cluster snapshot 2026-11-18T16:40Z), **[M]** (my read-only re-measurement 2026-11-19, `kubectl get/describe/top` with `KUBECONFIG=~/Projects/tailrocks/velnor-runners/state/kubeconfig`), or **[Est]** (estimate with arithmetic shown). I re-ran no `gh` timing query; the evidence pack's sampling (200 runs via jobs API, 68 job logs, seeds documented in evidence §A Method) is more complete than anything I could redo in this session, and I verified its cluster claims live (§1.5). Where evidence and the brief's prior claims disagree, I trust the evidence pack's re-measurement (its "Discrepancies" table is explicit about this).

Two regimes must not be averaged: **hosted era** (11-04..11-14, all jobs on `ubuntu-26.04`) and **scale-set era** (11-15..11-18, 31 Rust/Check jobs on the cluster, 8 gate/lint jobs still hosted). Within the scale-set era, 11-18 is a distinct overload regime (T5/T6/T8 in evidence §A).

### 1.2 Wall time: queue dominates, and the queue is the hosted gates

| Quantity | Value | Source |
|---|---|---|
| PR wall, 15-day population (success+failure) | median 17.5 min, p90 60.0, max 297.7 (n=1382) | [E] T1 |
| PR wall, scale-set era sample (created→updated) | median 26.7, p90 92.3 (n=77); success-only 27.7 / 79.2 | [E] T2 |
| PR wall by day (population success median) | 11-16: 19.2; 11-17: 16.2; 11-18: 74.7 | [E] T1 |
| PR wall by day (sample) | 11-16: 23.2; 11-17: 16.4; 11-18: 77.8 | [E] T2 |
| Merge-group wall, scale-set era | median 19.9, p90 84.1 (n=50) | [E] T2 |
| Push wall, scale-set era | median 21.7, p90 59.2 (n=29) | [E] T2 |

The brief's "~92 min median, ~67 min queue" is **not** the typical day: 92 min is the scale-set-era p90, reached as a median only in the 11-18 ~01:00–13:00Z window (hourly medians 46–110 min, evidence T8) [E]. The correct statement is bimodal: ~16–23 min on 11-16/17, ~71–78 min on 11-18.

Decomposition on success runs (evidence T5, gate chain Plan → fan-out → Required verdict → Required):

| Day / event | Wall median | Sum of 3 gate queues | Fan-out span (Plan end → last Rust/Check end) |
|---|---|---|---|
| 11-16 PR (n=23) | 23.2 | 12.6 | 8.4 |
| 11-17 PR (n=13) | 16.0 | 5.8 | 9.3 |
| 11-18 PR (n=13) | 71.8 | 57.0 (79% of wall) | 10.7 |
| 11-18 merge_group (n=6) | 64.4 | 42.4 | 13.5 |

So even with **zero queue**, the current job graph needs ~10–11 min wall (Plan ~0.9 min + fan-out ~9 min + verdict). Queue explains the 11-18 disaster; the fan-out critical path explains why good days still take 16 min. Both must be fixed; fixing only queue leaves a ~11 min floor.

Job-level queue confirms the split (evidence T4): scale-set jobs queue median 0.2 / p90 0.3 min (max 4.5, n=3944); hosted jobs in the scale-set era queue median 4.1 / p90 23.6 / max 49.2 (n=1248). Per hosted job name (Plan, Ledger CLI, Cargo Deny, Jactionlint, Cargo Machete, Alint, Required verdict, Required) queue p90 is 21–26 min [E T4]. At the 11-18T16:54Z snapshot, 86 jobs were queued, all on hosted `ubuntu-26.04`, 0 on scale-set labels (61 CI, 23 Source qualification, 1 Job image candidate, 1 generator release) [E T11]. My live check on 11-19 found 27 tailrocks runner pods Running with continuous `Killing … Stopping container dind` churn and no queue visible on the cluster side **[M]** — consistent: the cluster is not the queue.

**Conclusion:** queueing = hosted gates only. Setup/downloads/compilation/tests happen almost entirely on the cluster already and are the *second* term, not the first.

### 1.3 Runner-minutes and the critical path jobs

Per run (scale-set era medians [E T2]): PR 52.9 runner-min, merge_group 64.6, push 62.0; 39 jobs/run. The brief's "~73 runner-min" sits between merge-group median and p90; PR is 52.9.

Top jobs per PR run (evidence T3, runner-min/run; durations median):

| Job | Runner-min/run | Duration med/p90 | Queue med |
|---|---|---|---|
| Check / maintenance-helpers | 8.41 | 8.5 / 9.9 min | 0.0 |
| Rust / velnor-actions-cli | 6.57 | 6.5 / 7.7 | 0.0 |
| Rust / velnor-runner-host | 6.23 | 5.5 / 10.1 | 0.2 |
| Rust / velnor-runner-host (local-mode) | 5.49 | 6.5 / 10.4 | 0.2 |
| Rust / velnor-actions-orchestrator | 3.13 | 3.0 / 4.0 | 0.2 |
| Rust / velnor-runner-cli | 2.93 | 3.0 / 3.7 | 0.2 |
| Plan (hosted) | 0.97 | 0.9 / 1.1 | 7.1 / p90 33.4 |

The two `velnor-runner-host` variants sum to 11.7 runner-min/PR run (the brief's "~18" holds only on 11-18: 8.8+8.9, evidence T6 — durations inflate under the day's contention, §1.5). Together with maintenance-helpers and actions-cli they are ~27 of 53 PR runner-min (51%) [Est: 8.41+6.57+6.23+5.49=26.7; 26.7/52.9≈50%].

Step breakdown (evidence T10, medians; steps in the parallel group are not additive):

- **maintenance-helpers** (508 s job): `Execute named check` 486 s = 96%. No cache steps at all; 104 crates compiled cold in 10/10 logs [E T9a]. This is a pure cold-compile job.
- **velnor-runner-host** (332 s): Build test executables 81 s, Clippy 70 s, Unit/integration tests 192 s (p90 417 s), Doc 60 s. Local-mode twin: 88 / 75 / 246 s (p90 497) / 65 s. Tests dominate and have the worst tail.
- **velnor-actions-cli** (393 s): Prepare image fixture 89 s (23%), tests 238 s (61%), Clippy 36 s, Doc 30 s.
- Small crates (velnor-ci 76 s job, velnor-local 67 s) still pay fixed setup: `Prepare pinned tools` alone is 9–11 s in every Rust job [E T10].

**Setup tax [Est]:** `Prepare pinned tools` median 10 s (p90 18) across 3636 scale-set jobs = 706 runner-min in the sample alone [E T9b]. Add `Setup Mise` 2 s, `Set up MBX` 1 s, `Restore Cargo sources` 1 s (which then misses, §1.4), `Verify/Prepare Rust` ~2 s: ~16 s/job × 39 jobs ≈ 10.4 runner-min per run, i.e. ~20% of PR runner-min (10.4/52.9) spent before any crate compiles. The runner image seed already cut `Prepare pinned tools` 28.4→10.6 s (RESULT.md, image .4) [E §B]; the remainder is structural — ephemeral pods (`emptyDir` only) cannot retain anything between jobs.

**Compilation vs tests:** for the heavy jobs, compile+clippy+doc ≈ test time; neither dominates alone. Cold compilation is inflated by cache misses (§1.4); test time is inflated by no sharding (`default_shards = 1`, `.velnor/config.toml:29`) and `test-threads = num-cpus` racing a 96-thread node shared with other pods (§1.5).

### 1.4 Cache transfer: effectively zero reuse across jobs

From 68 job logs (10 runs) and the steps API [E T9a/T9b]:

- Cargo sources cache: **not found 48/48** Rust-job logs; Plan (hosted) hits the same key family 10/10 (86 MB entry). The compat hash includes platform/ImageOS identity, so hosted-produced entries never match scale-set consumers — a key-design bug, not a capacity problem.
- Rust toolchain: **no restored toolchain 58/58** logs. Scale-set jobs reinstall into `$RUNNER_TEMP/velnor/rustup` per job; `mise-tools-v2-*` entries (60–401 MB) are restored only in hosted jobs (n=624) [E].
- mbx remote: `MBX_REMOTE_URL` empty **68/68**; 0 B downloaded/uploaded in all 96 mbx summaries. mbx local (per-job `$RUNNER_TEMP`) hits 40–74% of lookups *between the two passes inside one job* — that is intra-job reuse only; every job starts with an empty store on a fresh pod.
- State snapshot (mbx bundle via GitHub cache): `Restore state snapshot` **skipped 3636/3636** jobs. Seal successes in 18 recent runs: generator 6, ci-helper 4, root/local/runner **0** [E T9d]. Root (1457 MB, 11-07) and runner (883 MB, 11-07) entries are 11 days stale. Causes are mixed: tip-check failures (7) and producer Clippy/parallel-check failures (9) — the brief's "merge queue advances main" is only part of it [E].
- Consequence: each heavy Rust job compiles **481–497 crates** and downloads **435** crate files (runner-host); maintenance-helpers compiles 104 with zero restore steps [E T9a]. GitHub-cache restore bandwidth from pods is documented at 13.6–22.4 MB/s with 120 ms RTT to the US blob store (velnor-runners log, [E §B]) — even when it hits, an ~1.5 GB state bundle costs 70–110 s. Local NVMe (§6, §8a) is two orders of magnitude faster.

Cache is therefore not a tuning gap; the scale-set path has **no functioning cross-job cache at all**. This is the largest single lever after queue placement.

### 1.5 Duplicate work: three verifications per merge

Evidence T7 [E]: all 39 jobs execute for PR, merge_group **and** push events. 120/159 successful merge_group runs have a push CI run on the **identical SHA** — pure re-verification (75%). Only 7/45 sampled PR heads have a tree equal to the merge-group tree (34/45 PR heads were behind the queue base), so merge-group runs are usually *not* duplicates of PR runs; the push run usually *is* a duplicate of the merge-group run.

Arithmetic **[Est]**: per merge ≈ PR runs (median 3 per branch, 45% cancelled [E T7]) + 1 merge-group (64.6) + 0.75×push (62.0). Removing the redundant push verification saves ~62 runner-min × 0.75 ≈ 46 runner-min per merge (~26% of the 64.6+62+52.9≈180 runner-min PR+MG+push total), plus a full wall-time run no one waits on but which competes for the same nodes (T6 duration inflation).

Also: 1156/2558 PR runs (45%) were cancelled (superseded pushes) [E T1] — `cancel-in-progress` already exists for PRs (`ci.yml` concurrency, evidence §C); their runner-minutes are excluded from the samples, so true spend is higher.

### 1.6 Cluster utilisation: CPU-saturated, memory-idle, badly shaped

Evidence snapshot 11-18: worker CPU samples swing 18–95% within 100 s; top pods use up to 83.7 cores / 16.8 GiB; memory <40% everywhere [E §B]. My re-measurement 11-19 **[M]**: `kubectl top nodes` — velnor-105 84%, velnor-190 88%, velnor-240 88%, velnor-241 94%, velnor-5 95% (80–92 of 96 threads), memory 18–36%. `kubectl top pods -n arc-runners`: top pod 70.8 cores / 12.6 GiB, next 52.3, 47.1, 40.8 cores — a handful of pods burst across the whole node because pods request 2 CPU / 4 GiB with **no CPU limit** and 32 GiB memory limit (verified in the live pod spec **[M]**; allocatable == capacity, no kube/system reservations, `cpuManagerPolicy: none`, `topologyManagerPolicy: none` **[M** configz**]**). Memory limits are ~3× overcommitted (306% on describe) while requests sit at 10–18% CPU **[M]**.

The owner's "cluster sat at 18–29% CPU" note and "92–95%" evidence samples are both true at different hours; the load is bursty because ~27–40 ephemeral pods (plus ChainArgos 60-max and jackin 40-max scale sets sharing the same 5 workers [E]) chase the same merges. Contention visibly stretches durations through 11-18 (runner-host 3.7→6.1→8.8 min across 11-16/17/18 [E T6]). The problem is not lack of hardware: 5×96 threads = 480 threads (240 physical EPYC 9454 cores) and ~625 GiB RAM against a 53 runner-min PR payload. The problem is (a) 8 jobs not on the cluster at all, (b) no cache locality so every pod recompiles the world, and (c) no CPU shaping so parallel steps (`test-threads=num-cpus`, `.config/nextest.toml`) oversubscribe whichever node they land on.

## 2. Hypotheses: verdicts with evidence

Each verdict uses the same scale: holds, partly holds, fails, unproven. Numbers are **[E]** unless marked **[Est]** or **[M]**.

### H1 — Native Rust actions beat JavaScript actions by a wide margin: partly holds

The claim holds for cache and checkout. It fails as an account of job time.

A job spends its time in cargo, not in action startup. Evidence T10 shows the heavy jobs spend 60–96% of wall time in tests and one check command. The action steps around that work cost seconds. `Prepare pinned tools` costs 10 s median per job [E T9b]. Checkout costs 6 s median in maintenance-helpers [E T10]. Even if a native action cut both to zero, a 332 s `velnor-runner-host` job would save under 5%.

Three costs are real and a native path removes them:

- The runner downloads each action before the first step. It resolves the action, fetches a tarball and extracts it (`ActionManager.cs` in `actions/runner`, evidence §F.5.2). A cold pod pays this for every job.
- Each JavaScript action starts Node. The runner forces all JavaScript actions to `node24` (`HandlerFactory.cs:58`, evidence §F.5.3). Startup is tens to hundreds of milliseconds per action. The cost matters only when a job runs many small actions.
- `actions/cache` talks to a remote store. From the cluster pods the GitHub blob store sits at 120 ms RTT and 13.6–22.4 MB/s [E §B]. A native cache that reads node-local NVMe removes both the round trips and the transfer. This is the one large win, and it belongs to H4, not to language speed.

velnor-legacy already built the native set: 26 adapters, including checkout, cache, rust-cache and sccache (`action.rs:379`, evidence §E.1.4). It still ran allowlisted JavaScript actions in a `node:20-bookworm` or `node:24-bookworm` sidecar (`executor.rs:652`). It rejected the rest. The legacy code proves the adapters are buildable. It records no end-to-end speedup for them (evidence §E.7: "no performance improvement accepted yet").

**Expected numbers [Est]:** native checkout plus native cache restore on a warm node saves 15–40 s per Rust job versus download plus remote restore. Across 31 Rust jobs that is 8–20 runner-min per run. The saving comes from locality and from skipping downloads. It does not come from Rust versus JavaScript execution speed. A team that rewrites actions and keeps remote caches gains almost nothing.

Verdict: **partly holds**. Build native checkout, cache and artifact paths. Do not expect the rewrite alone to change wall time.

### H2 — A from-scratch Rust runner beats reusing or wrapping the official runner and ARC: fails as stated

As a general replacement, the claim fails. As a restricted fast lane, it holds. The distinction matters.

velnor-legacy is the test. It reached ~804k lines of Rust (evidence §E.1). Two files alone, `executor.rs` at 37,028 lines and `runner.rs` at 36,759 lines, carry job execution and protocol. It transcribed the expression engine from `actions/runner` (`expression/mod.rs:1`), built Docker and Firecracker backends, and passed 8,172 of 8,222 nextest tests in the evidence run (50 failures trace to the macOS host: missing `tomllib`, missing Docker, timing thresholds). It still could not run arbitrary workflows: no generic JavaScript runtime, Docker actions rejected, checkout inputs limited, submodules always false (evidence §E.1.2, §E.6). Its scale-set lane gave up and delegated the job to the unmodified official runner (`scaleset/worker/runner.rs:1`, evidence §E.2.3). The repo was archived on 2026-10-17 with no reason recorded beyond "reference only; current work: velnor-new" (evidence §E.0, §E.6). Git history shows 1,070 commits in September 2026 alone, 60 of the last 100 marked `fix`, plus open incidents for idle CPU at 600–700%, lost registrations and broker 409 loops (evidence §E.6). No archive note blames a GitHub protocol break. The cost that killed it was breadth: every workflow feature, every expression edge and every cache scope rule becomes your code forever.

The protocol surface explains the breadth. The official runner speaks three registration paths, two message transports (legacy pipelines and broker), run-service acquire, renew and complete, Results Twirp for logs and step updates, and a launch service for action resolution (evidence §F.1–F.4). GitHub stops assigning jobs to runners more than 63 days out of date (evidence §F.6.1). A native runner must track all of that. The owner accepts breakage when GitHub changes the protocol. That acceptance covers a narrow profile. It does not cover re-implementing `if:` evaluation, matrix expansion, composite inlining, container hooks, file commands, secret masking and the dual template evaluators the Worker runs today (`PipelineTemplateEvaluatorWrapper.cs`, evidence §F.5.1).

velnor-new makes the opposite bet today. `crates/velnor-runner` (~185k LOC, evidence §D.2) runs the official runner inside Docker and spends its complexity on journal, ownership proof and crash recovery instead. Its 430 tests pass on macOS (evidence §D.5). The V2-native stage in `docs/deferred/self-hosted-runner.md:19` — "replace the official runner for the restricted V1 profile with the real Rust protocol runtime" — is documented as superseded and not implemented (evidence §D.6). That history is a warning, not a veto: nobody has measured a native job end to end, so no speedup number exists for either side (evidence §E.7, §D.5).

**Expected numbers [Est]:** a native executor saves the Worker's fixed startup: action resolution and download, .NET process start, and per-step Node launches. On a warm pod I put that at 5–20 s per job. It saves nothing on cargo time, which dominates. A full re-implementation costs person-years and still fails the first workflow that uses an unlisted action. A restricted executor for `velnor-gen` workflows (sequential `run:` steps that call `velnor-ci`, §3) costs far less and captures the same 5–20 s on the jobs that matter.

Verdict: **fails** as a general runner. **Holds** only for the restricted profile in §3.4. Reuse the official runner binary. Own the scheduling, the cache and the placement. That is where the time is.

### H3 — Kubernetes is the right base: partly holds

Kubernetes gives capacity, isolation and a scheduler for free. It also adds the pod-per-job cold start that ARC pays today.

What holds: the cluster already runs 480 threads across five workers with one scale-set label (`velnor.io/runner=true` **[M]**). ARC queues cluster jobs at 0.2 min median [E T4]. No other base on hand gives bin-packing, taints, affinity and pod isolation without new code. k3s itself is not the bottleneck: image pulls in the evidence window were 0 real pulls in 1,424 `Pulled` events ("already present on machine") [E §B.6].

What does not hold: the claim that Kubernetes scheduling alone uses the hardware well. Today every job creates a pod, starts a privileged `docker:dind` sidecar, waits for the `docker info` startup probe (57 `Unhealthy` events in one hour, all dind not ready [E §B.6]), runs one job and dies. Volumes are all `emptyDir` (evidence §B.3.3 **[M]** confirms the live template: request 2 CPU and 4 GiB, memory limit 32 GiB, no CPU limit). Nothing survives, so cache warmth cannot exist, and the scheduler has no reason to prefer one node over another. The generic ARC chain adds more delay: listener long-poll, `AcquireJobs`, patch `EphemeralRunnerSet`, create `EphemeralRunner` objects in batches of 8, `GenerateJitRunnerConfig`, create the Secret, create the pod, then the pod long-polls again (evidence §F.7.2). Each hop is small. The sum is the gap between 0.2 min queue and instant start.

The two-site layout cuts the other way. Pod traffic crosses DE and FI over WireGuard at 35 ms one-way ping (evidence §B.5). Cache traffic that crosses sites pays that on every object. Kubernetes will happily schedule a FI pod that needs a DE cache.

Verdict: **partly holds**. Keep Kubernetes as the substrate. Do not keep its default job shape. The design needs warm pools, node-local state and site-aware placement (§6). A bare-metal supervisor without Kubernetes would have to rebuild exactly those pieces.

### H4 — Local caches beat remote caches; a runner with built-in, placement-aware caching is a big win: holds

This is the strongest hypothesis. The evidence supports it at every level.

- Bandwidth and latency: GitHub cache restore from pods runs at 13.6–22.4 MB/s with 120 ms RTT [E §B]. A 1.5 GB state bundle costs 70–110 s to restore **[Est: 1500 MB ÷ 15 MB/s ≈ 100 s]**. Node-local NVMe on the EPYC workers reads at GB/s scale. The same restore costs seconds.
- Current hit rate is zero where it matters: sources miss 48 of 48, toolchain restore misses 58 of 58, state restore skips 3636 of 3636, mbx remote transfers 0 bytes (§1.4) [E]. Any working local cache beats zero.
- Within one job, mbx already reuses 40–74% of lookups between passes [E T9a]. That reuse dies with the pod. A store that survives on the node extends the same hit rate across jobs.
- The crates mirror proves the locality effect at small scale: `cargo fetch --locked` (59 MB) drops from 3.5–6.1 s direct to 2.7–3.3 s through the node-local nginx mirror (RESULT.md, evidence §B.4.2). A full target store is the same effect at 100× the bytes.

Correctness does not block the design. `docs/goals.md:42` already states the rule: a broken cache is slow, never wrong, and never silent. Cargo and mbx key on content. A stale hit cannot produce a wrong build; it can only waste a restore. The trust rules in `docs/goals.md:43` (fork reads only, no private-dependency artifacts in shared cache) map cleanly onto per-scope node-local stores (§4).

**Expected numbers [Est]:** toolchain restore goes from ~30–60 s of per-job install (inside the 10 s `Prepare pinned tools` plus mise install steps) to under 2 s from a baked or node-cached toolchain. Sources restore goes from a full 435-crate download per heavy job to near zero. The mbx object hit rate across jobs should reach 85–95% on a second push to the same PR, where only edited crates miss (mbx documents 353 of 354 compilations restored on the next push in its `hk` benchmark; §7). Build steps that compile 481 crates cold should compile only the changed set. I estimate `velnor-runner-host` "Build test executables" falls from 81 s to 10–20 s on a warm node, and maintenance-helpers falls from 486 s to 60–120 s once its 104 crates stop recompiling cold. §4 gives the hit-rate table and §5 turns it into wall time.

Verdict: **holds**. Build the cache into the runner platform, key placement on cache warmth, and keep remote caches as fallback only.

### H5 — Workflows can skip work a change cannot affect and run only the needed verification: holds

The mechanism exists and works. The policy that uses it is timid.

`velnor-gen`'s predecessor already classifies each crate job as `run` or `not_affected` from the merge-base diff on `pull_request` (`.velnor/config.toml` `[[stacks.rust.inputs]]` maps globs to packages with `complete = true`; evidence §C.4). Docs-only changes select nothing because `non_inputs = ["docs/**"]` (`.velnor/config.toml:40`). Dependents of a changed crate are selected. A file Velnor cannot place selects everything. That is the correct fallback under `docs/goals.md:40`: skip only when the change provably cannot affect the check.

Three gaps waste the win today:

1. Selection applies only to `pull_request`. Push and merge_group run all 39 jobs (evidence §C.4). §1.5 shows the push run duplicates the merge-group run on the same SHA in 75% of merges.
2. Selection stops at the job. Inside a selected crate, every test runs. `cargo nextest --partition` exists in the generated argv but `default_shards = 1` (`.velnor/config.toml:29`), so no test-level split occurs.
3. The job shape fights selection. One job per crate (29 Rust jobs) pays the ~16 s setup tax 29 times (§1.3). `docs/goals.md:69` states the better shape: each workspace starts as one job, and a job is added only when it shortens the wait by more than its startup cost. The new `velnor-gen` golden output follows that shape: one job per Cargo root, five roots (evidence §C.6).

The risk is a wrong skip. The current design already contains it: unowned files select everything, `Required` rejects any `not_affected` skip on a full-run event, and a scheduled full run selects everything (`docs/goals.md:66`). Extending selection to merge_group needs a tree comparison, not a leap of faith (§5.2).

**Expected numbers [Est]:** a typical one-crate PR today still waits for the full 39-job fan-out to drain because `Required` needs every scheduled job. With per-root jobs, affected-only selection on all events and test sharding, the critical path for a one-crate change is one root job, not 29 crate jobs. §5.5 derives a 5 min median verdict from the measured step times. Runner-minutes fall further: skipping the duplicate push run alone saves ~46 runner-min per merge (§1.5).

Verdict: **holds**. It is the second-largest lever after cache locality, and the safest, because the proof rule already exists.

### Summary of verdicts

| Hypothesis | Verdict | Main evidence |
|---|---|---|
| H1 native Rust actions | partly holds | Job time sits in cargo (T10); win comes from locality, not language |
| H2 from-scratch runner | fails (general); holds (restricted profile) | Legacy 804k LOC, allowlist limits, archived; no measured speedup either way |
| H3 Kubernetes base | partly holds | 0.2 min cluster queue; pod-per-job cold start and cross-site cache cost |
| H4 local placement-aware cache | holds | 0% cross-job hits today; 120 ms and 15 MB/s remote versus node-local NVMe |
| H5 change-aware workflows | holds | Selection works on PRs; push and merge_group ignore it; shards unused |

## 3. Runner architecture: keep the protocol contract, replace the placement and cache layers

### 3.1 The compatibility contract stays

GitHub remains the control plane. Velnor accepts GitHub jobs, reads the same workflow YAML and reports the same check results. Concretely, keep these surfaces exact:

- The scale-set API sequence: create or refresh a session, long-poll messages, `acquirejobs`, then `generatejitconfig` (evidence §F.1.3). The message gives only a reference. The full job comes from run-service `acquirejob`. Renew the lock every 60 s. Send logs and timeline updates through the Results Twirp service (evidence §F.2–F.3).
- The workflow format: `on`, `jobs`, `needs`, `if:`, matrix, composite actions, service containers, file commands (`GITHUB_OUTPUT`, `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_STATE`, `GITHUB_STEP_SUMMARY`) and secret masking. The Worker evaluates `if:` and templates on the runner (evidence §F.5.1). Any design that asks authors to write a different YAML fails the brief.
- The check contract: `Required verdict` and `Required` stay the merge gate (ruleset `protect-main`, evidence §C.1). The merge queue stays `SQUASH` with `HEADGREEN` grouping. Velnor changes how fast the gate answers. It does not change what the gate means.

Drop legacy compatibility where the brief allows it: target only the broker and run-service flow (`UseV2Flow`), only `node24` for JavaScript actions, only current scale-set API version `6.0-preview`. Do not carry the legacy pipelines session, classic `config.sh` registration or `node12` and `node16` runtimes. The official runner already forces every JavaScript action to `node24` (`HandlerFactory.cs:58`). Match that and stop.

### 3.2 Inside the contract, three layers change

Split the system into three layers. Change the outer two. Keep the middle one boring.

**Layer A — Listener and scheduler (replace ARC).** Replace the ARC controller and listener with a Velnor-owned listener. The listener holds one scale-set session per org, long-polls for `JobAvailable`, calls `acquirejobs` and assigns each job to a warm pod on a node that already holds the needed cache (§4). It replaces ARC's chain of patch, batch create, JIT Secret and pod create (evidence §F.7.2) with a direct assignment to a pre-created pod from a warm pool (§6). Keep ARC running until the new listener passes the same jobs on `velnor-lab`. Roll back by scaling ARC up and the new listener down. Both speak the same GitHub API, so GitHub sees no change.

Port the protocol types from `crates/velnor-runner/crates/velnor-runner-github` (evidence §D.3): session open and refresh, message ack rules, `acquirejobs` request IDs and JIT generation already exist as pure wire types with no HTTP dependency. Add one HTTP client and the scheduler. Do not port ARC's generic reconcilers. They solve a multi-tenant problem Velnor does not have.

**Layer B — Step executor (keep the official runner binary).** Each job still runs the unmodified official `Runner.Listener` and `Runner.Worker` inside its pod. The Worker keeps action download, composite inlining, container handling, expression evaluation and log streaming (evidence §F.5). This choice looks timid. It is the load-bearing one: it keeps every workflow that runs today running tomorrow, including the 61 `actions/checkout`, 80 `actions/upload-artifact` and 55 `jdx/mise-action` uses across the repo (evidence §C.2). The cost is the .NET startup and Node launches, 5–20 s per job **[Est]**. §3.4 recovers that cost on one profile without betting the merge gate on it.

Fix two ARC-era choices inside this layer. First, pre-seed the action archive cache (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, evidence §F.5.2) in the runner image so the Worker skips the launch-service resolve and tarball download for pinned actions. Second, set the job-started hook (`ACTIONS_RUNNER_HOOK_JOB_STARTED`, evidence §F.5.4) to verify cache warmth and record the first-cargo-command timestamp. The hook gives a direct measure of the `docs/goals.md:57` target: job start to first cargo command in at most 30 s.

**Layer C — Node platform (new).** Add a per-node agent (DaemonSet) that owns everything that must survive pods: the toolchain store, the cargo registry cache, the mbx object stores, the crates mirror cache and the target-view slots (§4). The agent labels its node with cache digests, enforces disk budgets with least-recently-used eviction, and serves cache reads from local NVMe. Pods mount read-only views of the stores they need. This layer does not exist today: all four runner volumes are `emptyDir` (evidence §B.3.3). It carries the whole H4 win.

### 3.3 What to take and what to refuse, by source

| Source | Take | Refuse |
|---|---|---|
| `velnor-new/crates/velnor-runner` | Journal discipline: write intent, call out, record result; never hold a transaction across HTTP or Docker (evidence §D.4.3). Ownership proof: delete only an exact ID match; names never grant delete rights (§D.4.4). Crash rule: reconcile journal, live objects and GitHub state before advertising capacity (§D.4.5). `velnor-runner-github` wire types. | The macOS and Docker host shape. One host, one Docker engine and `max_jobs = 1` by default (§D.4.1) do not map to a five-worker cluster. The 156k LOC in `velnor-runner-host` buys Docker lifecycle safety that Kubernetes already provides. Reuse the invariants. Do not reuse the backend. |
| PR #1324 Kubernetes runtime | The `Runtime` trait split: one host, Docker and Kubernetes backends, journal gains `runtime_kind` only (evidence §D.9). One Pod per job with `runner` plus `dind` containers. Node-local PVC cache slots with node affinity. JIT in a per-Pod Secret, never in env, args or labels. | HostPath cache mounts past a spike (PR calls them "not recommended beyond a spike"). Any design where job cleanup can delete a shared cache PVC. |
| `velnor-legacy` | Expression engine (`expression/`, 3,565 lines, transcribed from `actions/runner`): reuse it only in the fast lane (§3.4). Native checkout logic and ref-then-base cache scoping (`gha_cache.rs:14`): reuse the scope rules in §4. Per-slot mbx layout (`mbx_store.rs:1`): one store per trust scope, one slot dir per runner slot, to avoid the registrar flock and the target-dir lock. Host-persistent `cache.rs` flock and GC pattern. | The general executor (`executor.rs`, 37k lines), the Firecracker backend and the allowlist as a product boundary. Legacy proves each works. Together they form the breadth that forced the archive (§2 H2). Do not re-import them as the default path. |
| Official `actions/runner` | The Worker, the Listener, JIT handling, renewal at 60 s, Results reporting, hooks. Pin the image digest. Track new releases inside the 63-day assignment window (evidence §F.6.1). | Self-update inside the pod. Registration already sets `disableUpdate: true` in velnor-new's spec (evidence §D.4.2). Keep it. Roll the image deliberately instead. |
| ARC | The scale-set session model and the `minRunners` semantics: idle runners plus assigned jobs, capped by `maxRunners` (evidence §F.7.3). The dind sidecar shape for jobs that need Docker. | The generic reconcile chain as the steady state (§3.2 Layer A). Kubernetes container mode: it forces every job into a job container and drops Dockerfile actions (evidence §F.7.4). Velnor jobs need dind, not that mode. |

### 3.4 The fast lane: a native executor for one profile only

Add a second executor beside the official Worker. Admit a job to it only when the job matches the `velnor-gen` profile:

- The workflow comes from `velnor-gen` and each job is a sequence of `run:` steps that call `velnor-ci` subcommands (`plan`, `prepare`, `gate`, `report`, `cache` in `ci-helper/crates/velnor-ci`, evidence §C.6).
- The job uses no `uses:` step except `actions/checkout`, `actions/cache` restore and save, `actions/upload-artifact` and `actions/download-artifact`. Translate those four to built-ins. The deferred design already lists this exact split (`docs/deferred/self-hosted-runner.md`, evidence §D.6).
- The job needs no service containers, no matrix fan-out beyond test shards (§5) and no Docker action.

Execute admitted jobs with a small Rust runtime: fetch the job through the same acquire path, run checkout against the node git mirror, restore cache from the node store, run each `velnor-ci` command in the pod, stream logs through Results, renew and complete. Port legacy's expression engine only for the `if:` and `env` subset this profile emits. Reject any other job before it runs a step and send it to the official Worker. A rejection costs one assignment, never a wrong result.

Gate the lane in order. First run it on `velnor-lab` (`docs/goals.md:76` requires lab proof first). Then admit one velnor-new root job in shadow mode: run both executors, compare step results and timings, gate on the official one. Then gate on the native one for that root. Expand root by root. If parity stalls, the lane stays an accelerator for feedback and never blocks a merge.

Gain **[Est]**: 5–20 s per admitted job from skipped action downloads, skipped .NET start and native cache restore. On five root jobs that is under 2 min of runner time per run. The gain is small next to §4 and §5. Fund the lane last (§8). Its real value is strategic: it proves Velnor can execute its own generated profile without the official binary, which is the precondition for any later claim on H2.

### 3.5 The goals.md conflict

`docs/goals.md:16` states: "Jobs are always executed by GitHub's official runner." Layers A to C obey that line. The fast lane breaks it for admitted jobs. Do not edit the file in this research task. §9 asks the owner to amend the line to cover the restricted profile, or to keep the lane in permanent shadow mode. The phased plan (§8) delivers every wall-time target without the lane, so the owner can refuse and lose only the 5–20 s per job.

## 4. Cache system: node-local first, site-shared second, GitHub last

### 4.1 What to cache

Cache five things. Each has a different key, lifetime and trust scope. Do not merge them into one store.

| Content | Key | Size today | Lifetime |
|---|---|---|---|
| Rust toolchain 1.99.1 with clippy and rustfmt | Toolchain channel plus component set plus target list (`rust-toolchain.toml`) | ~1–2 GB installed **[Est]** | Weeks. Changes only on a pin bump. |
| Mise tools (cargo-nextest, cargo-deny, cargo-machete, jactionlint, bun, crane, others) | Tool name plus pinned version plus sha256 (`.velnor/version-policy.toml`) | 60–401 MB per GitHub entry [E §C.3] | Weeks. |
| Cargo registry index and crate files | `Cargo.lock` hash per workspace root (five roots) | Sources bundle 86 MB compressed [E T9a]; 435 crate files per heavy job | Days to weeks. Lock changes are rare. |
| mbx compiler objects | Content hash of compiler inputs. mbx computes the key. Velnor chooses only the store. | State bundle 1.5 GB stored and 12 GB extracted per root at the cap [E §C.3] | Days. Evict least recently used. |
| Test results | Do not cache pass and fail outcomes. Cache only the built test binaries. Run the tests every time. | Part of the mbx and target store | Same as objects |

Do not cache `target/` as a tarball. The current design already removed `github-cache-mode: target` (evidence §C.3). Keep that call. Move compiled state as mbx objects, not as a directory snapshot. A target snapshot restores stale paths and stale fingerprints. An object store restores only the units whose inputs match.

Do not cache test outcomes across commits. A cached "passed" bit breaks `docs/goals.md:41`: green must mean the selected checks ran and passed on this tree. Cache the binaries. Execute the tests.

### 4.2 Where each tier lives

Use three tiers. Read in order. Write only to the tiers the trust scope allows (§4.4).

**Tier 0 — node-local NVMe (new, carries almost all hits).** Run one cache agent per worker as a DaemonSet. Give it a dedicated local volume on the worker NVMe (§8a covers the volume layout). Lay out the disk as legacy learned to: one store per trust scope, one slot directory per runner slot (`mbx_store.rs:1` in velnor-legacy). Set `MBX_CACHE_DIR` to the slot path on the node volume, not to `runner.temp`. Keep `RUSTUP_HOME` and the mise install dir on the same volume and share them read-only across slots on that node. Keep the cargo registry cache on the volume in the same layout cargo expects. Pods mount the views they need. The agent enforces a per-store disk budget and evicts least recently used entries when a store crosses it (port the flock plus GC pattern from legacy `cache.rs`).

Size the stores from measured data. Toolchain plus tools need under 5 GB per node. Registry needs under 5 GB. Give the mbx store 300–500 GB per node **[Est]**: five roots at 12 GB extracted each is 60 GB for one warm generation, and the budget must hold several PR generations per node before eviction bites. Each worker reports 3.78 TB of node filesystem and up to 50 TB of image filesystem on the two large workers **[M][E §B.1.4]**. Disk does not limit this design. Eviction policy does.

**Tier 1 — site-shared cache server (new).** Run one cache service per site (DE and FI). Point `MBX_REMOTE_URL` at the same-site service from every pod. The service stores mbx objects that miss on the node and serves them to other nodes in the site. Use the mbx cache server or an S3-compatible store on a worker with spare disk. Replicate nothing across sites on the hot path: the DE to FI link costs 35 ms per round trip (evidence §B.5), and a restore that crosses sites loses to a local recompile for small objects. Sync across sites in the background, off the critical path, so a cold node in either site can warm without compiling.

Keep the crates mirror in this tier. It already runs as a DaemonSet with `PreferSameNode` traffic and a 200 GB per-node cache (evidence §B.4.2). Fix its wiring instead of replacing it: the 2026-11-16 rollback happened because cargo read `/home/runner/.cargo/config.toml` only under `/home/runner` (RESULT.md). Set the registry replacement through `CARGO_HOME` config in the job env, which the new generator already describes (`.velnor/rust-ci.toml` `[crates_mirror]`, evidence §C.3), and verify with a build that runs outside `/home/runner`.

**Tier 2 — GitHub Actions cache (keep, demote).** Keep GitHub cache for hosted fallback jobs and for disaster recovery when the cluster is down. Stop using it for scale-set Rust jobs. Its measured restore speed from pods is 13.6–22.4 MB/s at 120 ms RTT [E §B]. A limiter check bounds its value: restoring the 1.5 GB state cap at 20 MB/s takes 75 s, which alone breaks the 30 s first-cargo target in `docs/goals.md:57`. No tuning of keys fixes a bandwidth limit. Fix the key bug anyway for the hosted path: the sources compat hash mixes platform identity, so hosted producers never match scale-set consumers (§1.4). Split the key into content (lock hashes) and platform, and let each consumer restore the newest entry whose content matches and whose platform it can use.

### 4.3 Placement: schedule the job onto its cache

A cache that the scheduler ignores is a lottery. Today all five workers look identical to the scheduler. Change that.

- The node agent publishes one label per cache family: the toolchain digest, the registry snapshot digest and the mbx store generation, for example `velnor.io/cache-toolchain=<digest>`. It updates the label only after the store verifies.
- The plan job (§5) emits the digests a run needs. The listener (Layer A, §3.2) turns them into a node affinity preference: prefer a node in the same site whose labels match, then any node in the site, then any node. Use preferred affinity, never required. A hard requirement turns a missing cache into a queue.
- Keep one fallback: a cold node compiles. Correctness never depends on a hit (`docs/goals.md:42`). Log the outcome as `warm`, `site` or `cold` per job so the hit rate is visible on every run.

Expect skew. Popular roots pull jobs onto few nodes. Bound it: when the warm node's queued jobs pass a threshold, spill to the site tier and accept the slower restore. Measure the spill rate. If spill passes 20% of jobs, add per-node store replication for the hottest root instead of widening affinity **[Est threshold; tune from the first month of logs]**.

### 4.4 Correctness and trust

Follow the rules the repo already wrote. Do not invent new ones.

- **Same-repo PR:** read Tier 0 and Tier 1. Write only to a PR-scoped slot on the node (store key includes the PR number). Only that PR reads the slot back (`docs/goals.md:72` carried into the new layout). This also bounds poisoning: a bad slot harms one PR.
- **Push to main and merge_group:** read all tiers. Only push to the protected default branch writes the shared Tier 0 generation and Tier 1. Merge groups never write shared state. Current policy already states this split (evidence §C.3: pull requests, forks and merge groups never write cache). Keep it.
- **Fork PR:** restore only. Give forks a separate slot family and a separate node label. Never mount the shared writable store into a fork job. Never cache any output built from a private dependency for a fork (`docs/goals.md:43`). The job-start hook (§3.2) enforces the mount set per trust scope. A job that asks for a store outside its scope fails before it runs repository code.
- **Content keys only:** mbx and cargo key on input content. A wrong or stale entry can waste time. It cannot change the program that runs, because the compiler still checks fingerprints before it reuses an object. The `Cache` check from `docs/goals.md:67` must still run: report crates compiled versus crates the warm store held, and turn red when a held crate compiles. A red Cache check blocks nothing. It stops silent cache rot.
- **Attestation:** the state seal chain attests bundles with `actions/attest` and verifies with `gh attestation verify` before a consumer trusts a key (evidence §C.3). Node-local stores need the same property with less machinery: the node agent signs each shared generation with a cluster key at write time and verifies the signature on mount. Drop the GitHub round trip. Keep the rule that only default-branch writes create shared generations.

### 4.5 Expected hit rates and the limiter behind each number

Report hit rate per run from runner one. Use crates compiled and crates downloaded (`docs/goals.md:67`) plus mbx hit counts. The table gives targets, the limiter that caps each one, and the check that proves the work happened. Treat each target as a claim to test with at least five runs per side before you trust it (median and range, alternating warm and cold nodes). A single warm run proves nothing.

| Store | Target | Limiter | Proof |
|---|---|---|---|
| Toolchain | 100% after first warm per pin | Pin changes reset the store. Nothing else misses. | First cargo command under 30 s, with 0 toolchain installs in the log. |
| Mise tools | 100% after first warm per version set | Version bumps reset entries one tool at a time. | `Prepare pinned tools` under 2 s median, down from 10 s [E T9b]. |
| Cargo sources | Over 95% of crate files | A `Cargo.lock` change misses only the changed crates. A full lock rewrite misses all. | Crates downloaded per heavy job falls from 435 [E T9a] to under 25 **[Est]**. |
| mbx objects, second push to same PR | 85–95% of compilations | Edited crates and their dependents always miss. A toolchain or flag change misses all. | mbx summary shows hits, misses and not-looked-up counts. Check that misses compiled and hits did not. |
| mbx objects, new PR off current main | 70–85% **[Est]** | The PR base ages. Each day behind main adds dependent misses. | Same mbx summary, split by base age. |
| Test binaries | Same rate as mbx objects | Same as objects. | `nextest list` builds in seconds. Tests still execute (§4.1). |

Check the arithmetic before you believe a target. Maintenance-helpers compiles 104 crates cold in 486 s [E T10]. If 90% hit, 10 crates compile. At the cold average of 4.7 s per crate that is 47 s, plus test time the cache cannot remove **[Est]**. So a 60–120 s warm job is consistent. A claim of 10 s would fail the limit check and would mean the tests did not run. Apply the same bound everywhere: a cache can remove compile and download time only. It cannot remove test execution, clippy analysis of changed crates or doc builds of changed crates.

Fix the state seal failures as part of this work, even though Tier 2 shrinks. Seven failures stop at `Check state tip` and nine at Clippy or parallel checks (evidence T9d). The tip check compares `GITHUB_SHA` to the live `main` ref after a chain of up to five 60 min producers (evidence §C.3). Shorten the chain or re-check the tip at seal time only. A seal that usually fails trains everyone to ignore the one signal that says the cache broke.

## 5. Workflow design: select less, shard the rest, verify each tree once

### 5.1 Move the gate jobs onto the cluster first

Eight jobs still run on GitHub-hosted `ubuntu-26.04`: Plan, Ledger CLI, Cargo Deny, Jactionlint, Cargo Machete, Alint, Required verdict and Required (evidence §C.1). Their durations are short (0.2–0.9 min [E T3]) and their queues are long (p90 21–26 min [E T4]). The runner ruleset needs only the two `Required` names. Nothing in those jobs needs a GitHub-hosted machine. The repo kept them hosted to keep credentials and signatures off shared nodes (RESULT.md: "Stays on hosted runners by design").

Move all eight to the scale set. Run them in pods with no Docker socket, no write token and a read-only cache mount. Plan and the lint jobs need network access to the API and to crates only. Required verdict needs the plan artifact and the per-crate reports. None needs `id-token: write` or `attestations: write`. Keep state seal producers, release and publish jobs hosted. Those hold the signing keys. The gate does not.

`velnor-actions` v0.1.81 already moves these jobs (per the owner's decided context). Treat that release as Phase 0 (§8) and measure it before you build anything else. Expected effect **[Est from E]**: the 11-18 PR pattern loses 57.0 min of gate queue and keeps the 10.7 min fan-out, so wall falls from 71.8 min to about 15 min. On an 11-17 pattern, wall falls from 16.0 min to about 10 min. The limiter after this move is the fan-out itself. That is what the rest of this section attacks.

### 5.2 Verify each tree once

Count verifications per tree, not per event. A tree that passed `Required` once needs no second full run.

- **Push to main:** when the push SHA equals a merge-group SHA that already passed, skip the full CI run. Keep a short push workflow that seals cache generations (§4.4), runs the freshness checks and publishes. Evidence T7 shows 120 of 159 merge_group runs share the SHA with a push run. The skip saves about 62 runner-min on 75% of merges **[Est: 62.0 median push runner-min, E T2]** and removes a full fan-out from the shared nodes.
- **Merge_group:** select by tree. Compare the merge-group tree to the PR head tree. When the trees match, reuse the PR run result: report the same checks without re-running jobs. When the trees differ, run the affected set for the delta between the PR head and the queue base, plus a full run on the batched queue head under `HEADGREEN` grouping (the queue already builds at most 2 entries per batch, evidence §C.1). Today only 7 of 45 sampled PRs share the tree (evidence T7), so reuse helps a minority. The delta run helps the rest: most queue deltas touch few crates on top of a tested PR head.
- **Pull_request:** keep the current selection. Extend it with the test-level rules in §5.3.

Guard each skip with the same proof rule as today: when the diff cannot map to crates, run everything (`docs/goals.md:40`). `Required` must still reject a `not_affected` skip on any event the owner marks full. Name those events in config (push to a release tag, scheduled run) instead of hard-coding `push` and `merge_group` as always-full.

### 5.3 Inside the job: compile once, shard tests

Adopt the `velnor-gen` shape. One job per Cargo root (root, `ci-helper`, `generator`, `local`, `crates/velnor-runner`) replaces 29 per-crate jobs (evidence §C.6). The shape follows `docs/goals.md:69`: a job exists only when it cuts the wait by more than its startup cost. Five jobs pay the ~16 s setup tax five times, not 29 times. That saves about 6 runner-min per run **[Est: 24 fewer jobs × 16 s ≈ 384 s]** and, more important, lets one job compile a workspace once for lint, doc and tests (`docs/goals.md:68`: at most one metadata compile and one full compile per crate per configuration). The per-crate layout compiles shared dependencies once per crate job. The per-root layout compiles them once per root.

Then shard the tests inside each root:

- Build test binaries once per root. Hand the same binaries to every shard. `velnor-gen` already writes one build step per root (evidence §C.6).
- Split with `cargo nextest run --partition hash:k/n`. The generated argv already supports it (evidence §C.4). Choose `n` per root from measured test time: split until the longest shard sits near 60–90 s. From evidence T10 medians: `velnor-runner-host` tests take 192 s (p90 417 s), so use 4 shards. `velnor-actions-cli` tests take 238 s, so use 4 shards. Small roots stay at 1 shard. Store the shard counts in `.velnor/config.toml` `[test_sharding]` and revisit them when the p90 shard passes 120 s.
- Run clippy, fmt and doc as parallel steps inside the root job, as today. Keep `background: true` groups. The steps overlap on the same warm target dir, so the parallel group costs the max step, not the sum.
- Set `CARGO_BUILD_JOBS` and nextest `test-threads` from the pod CPU allocation (§6, §8a), not from node size. Today `.velnor/config.toml:26` caps `compiler_process_budget` at 2 while `.config/nextest.toml` sets `test-threads = num-cpus`. On a shared 96-thread node the first starves single-crate builds and the second oversubscribes. Give each pod a fixed CPU budget and derive both numbers from it.

Keep Docker-needing tests in their own group. `velnor-runner-host` carries the `docker-networks` test group (max 4 threads, evidence §C.5) and `local-mode` needs a second configuration. Run those shards only on dind nodes (§6). Pure unit shards run on nodes without the privileged sidecar.

### 5.4 The plan job must stay small

Plan decides selection, cache digests and shard counts. Today it runs 20 steps on a hosted runner and takes 0.9 min plus queue [E T3]. On the cluster with a warm toolchain it should finish in under 30 s **[Est]**. Hold it to three rules:

- Plan reads the diff and config only. It never compiles. A plan that compiles becomes the critical path.
- Plan emits machine-readable outputs only: affected roots, shard counts, cache digests and the `not_affected` list. `velnor-ci plan` already owns this logic (evidence §C.6). Keep logic in Rust, one command per `run:` step (`docs/goals.md:73`).
- The plan and required jobs run the released helper, never the helper under test (`docs/goals.md:76`). Pin the version in `.velnor/generator.lock` as today.

### 5.5 Minimum honest time to verdict

Build the floor from measured steps. Assume Phase 0 to 2 (§8): gates on cluster, warm Tier 0 cache, per-root jobs, sharded tests. Times are medians **[Est from E T10]** for a typical PR that touches one or two crates in one root. Ranges use the p90 step times where the tail matters.

| Stage | Median | Notes |
|---|---|---|
| Listener assigns a warm pod | 0.2 min | Measured cluster queue today [E T4]. Warm pool (§6) holds this under load. |
| Plan on cluster | 0.5 min | 0.9 min hosted duration today, warm toolchain, no compile. |
| Affected root job: checkout plus restore | 0.3 min | Node-local restore (§4). Target: first cargo under 30 s from job start (`docs/goals.md:57`). |
| Build test binaries (warm) | 0.3 min | 81 s cold for runner-host. Warm compiles only changed crates. |
| Longest test shard | 1.0 min | 192 s of tests in 4 shards is 48 s, plus per-shard start cost. p90 shard reaches 1.7 min (417 s ÷ 4). |
| Clippy and doc (parallel with tests) | 0 min on path | Runs inside the same job in parallel. Clippy on changed crates bounds at ~70 s cold (T10). It overlaps the shards. |
| Required verdict plus Required | 0.3 min | 0.7 and 0.1 min durations today [E T4], now without hosted queue. |
| **Total, typical PR** | **~4–5 min** | p90 ~8 min when the p90 test tail and a cold node combine. |

State the bound plainly. No workflow change beats the sum of plan, one warm build and one test shard. A PR that touches `crates/velnor-runner-host` and its dependents pays more: two roots on the path, each with its own shard tail. Put that case at 6–9 min **[Est]**. A full-workspace change (toolchain file, lock file, workflow file) selects everything under `docs/goals.md:66` and pays the longest root chain. Put the full case at 8–12 min **[Est]**. Anyone who promises under 3 min for the typical case must show which stage vanished and prove the tests still ran (count executed tests per run and compare to the unsharded baseline).

Compare honestly to today. Scale-set era PR median is 26.7 min and p90 is 92.3 min [E T2]. A 5 min median is a 5× cut. Runner-minutes fall from 52.9 to roughly 20–25 per PR **[Est]**: setup tax drops with five jobs instead of 39, warm builds compile only changed crates, and shard pods run shorter. The push skip (§5.2) saves a separate ~46 runner-min per merge.

## 6. Kubernetes use: warm pods, shaped CPUs, site-aware placement

### 6.1 Pod lifecycle: pre-create the sandbox, assign the job late

Today each job pays the full pod birth: schedule, pull check, init containers, dind start, probe wait, runner start, JIT registration, long-poll. The evidence window shows the cost shape: 485 `Scheduled` and 489 `Killing` events in one hour, 57 dind probe failures (evidence §B.6). My live sample shows the same churn **[M]**.

Invert the order. Keep a pool of pre-created pods per org. Each pooled pod has the runner image, the dind sidecar already running (or absent, for jobs that need no Docker), the cache mounts in place and the Listener process idle. The listener (Layer A) holds the pods. When GitHub offers a job, the listener picks a pooled pod, delivers the JIT config through a per-pod Secret, and starts the one job. The pod runs one job and returns to the pool for a fresh worker process, or dies and the pool refills in the background. Isolation comes from one job per worker process and a fresh `_work` dir per job, not from pod death. Jobs that run repository code never share a worker process.

Size the pool from the queue data. Tailrocks needs enough idle pods to absorb a 39-job fan-out without queueing: hold 12–16 warm pods for tailrocks, 4 for ChainArgos, 2 for jackin **[Est from E: 26 desired replicas at snapshot, 0.2 min median queue]**. Idle pods cost about 45 MiB each (evidence §B.3.3). The cost is dind memory and node cache slots, not RAM. Scale the pool with assigned jobs up to the per-org max (60, 60, 40 today). Refill eagerly after each assignment so the pool never drains during a fan-out.

Keep ephemeral pods as the fallback for jobs that need a pristine machine (release qualification, cache producers). Give them a separate label. Do not let fallback jobs steal warm slots.

### 6.2 Resource shape: give each pod a real CPU budget

The current shape requests 2 CPU and 4 GiB, limits memory at 32 GiB and sets no CPU limit (evidence §B.3.3, confirmed live **[M]**). One pod then bursts to 70–84 cores (§1.6) and starves its neighbours. Fix the shape per job class:

| Class | CPU request and limit | Memory request and limit | Use |
|---|---|---|---|
| Gate and lint (Plan, Required, deny, machete) | 2 and 2 | 2 GiB and 4 GiB | Short, single-threaded checks. |
| Build and test shard | 8 and 8 | 8 GiB and 16 GiB | Default Rust shard. `CARGO_BUILD_JOBS=8`, nextest threads 8. |
| Heavy shard (runner-host Docker tests) | 16 and 16 | 16 GiB and 32 GiB | The `docker-networks` group and local-mode. |
| Cache producer and seal | 8 and 16 | 8 GiB and 32 GiB | Burst to compile a full generation. Never gates a PR. |

Set request equal to limit for CPU on build pods (Guaranteed quality of service). With the kubelet CPU manager in `static` policy (§8a), Guaranteed pods get exclusive cores. Cap concurrency per node from the budget: a 96-thread worker holds at most ten 8-CPU shards plus system pods. That bound ends the 70-core burst and the T6 duration inflation. It also makes shard time repeatable, which §5.3 needs.

Derive cargo parallelism from the pod budget. Pass `CARGO_BUILD_JOBS` and the nextest thread count as env from the pod spec. Remove the fixed `compiler_process_budget = 2` cap (`.velnor/config.toml:26`) for cluster jobs. Keep a cap for hosted fallback, where the runner gives 4 cores.

### 6.3 Placement, affinity and sites

- Add topology labels. Nodes carry no `topology.kubernetes.io/zone` today **[M]**. Label DE workers `velnor.io/site=de` and FI workers `velnor.io/site=fi`. Require same-site placement for a run's jobs unless a site drains. A run that scatters across the 35 ms link pays it on every Tier 1 read (§4.2).
- Prefer warm nodes. The listener adds the cache affinity from §4.3 as a preferred node affinity (weight high for toolchain plus root store, lower for registry). Never use required affinity for cache. A cold start beats a queue.
- Spread shards. Add pod anti-affinity so the shards of one root land on different nodes. Four shards on one node share one store (good) but also share one memory bus. Measure both ways for one week. Keep the faster one and record the numbers.
- Taint the dind pool. Jobs that need Docker carry a privileged sidecar. Taint the nodes that allow it (`velnor.io/dind=true:NoSchedule`) and tolerate the taint only from Heavy and producer pods. The owner's decided context already accepts privileged dind on dedicated tainted nodes. Shrink the set over time as more tests drop the Docker need. Plan, lint and pure unit shards run on untainted nodes with `RuntimeDefault` seccomp, dropped capabilities and no socket.
- Fork fence. Schedule fork PR jobs onto nodes whose cache slots hold only the fork family (§4.4). Label those nodes `velnor.io/trust=fork`. A fork job never lands on a node that holds a writable shared generation.

### 6.4 Isolation for untrusted PRs

Keep the current trust order. Same-repo PR code runs on the shared nodes with PR-scoped cache writes. Fork code runs restore-only, on fork-labelled nodes, with no Docker socket unless the job class requires it, no write token (`permissions: contents: read` already in `ci.yml`) and egress limited by NetworkPolicy: allow the API, the crates mirror and the site cache service; deny the cloud metadata address and the Kubernetes API from job pods. The job-start hook checks the trust scope before the first repository command runs. A mismatch fails the job in seconds. That fail-fast cost is the price of the check. Log it.

Service containers and Docker actions stay on the official Worker path (§3.2). Run them only in dind pods. The fast lane (§3.4) admits no such job.

### 6.5 Scaling across workers and orgs

Three orgs share five workers (tailrocks max 60, ChainArgos max 60, jackin max 40, evidence §B.3.2). The sum of maxima (160) exceeds what the nodes hold at the §6.2 shape (about 50 build pods). Keep per-org maxima as burst caps. Add a cluster-level cap per org from node allocatable CPU, so one org's fan-out cannot starve the others. Give tailrocks priority during its merge bursts through PriorityClasses: gate jobs highest, build shards next, producers lowest and preemptible. Priority decides who waits. It must never preempt a running test shard mid-run; set producers as the only preemptible class.

Add workers before raising maxima. Each EPYC worker adds 96 threads and one warm store replica. A new worker pays one cold generation per root (compile once, then share through Tier 1). §8a lists what a new worker needs on day one. Cross-site: keep both sites active. Drain one site for maintenance by labelling its nodes unschedulable for new assignments; running jobs finish. Do not stretch one run across sites to chase idle cores. The 35 ms link costs more than the wait.

Measure three numbers per week: queue median per org (target under 0.5 min), warm-hit share per root (§4.5) and shard p90 per root (§5.3). When queue rises while nodes sit under 70% CPU, the pool or the affinity is wrong, not the hardware.

## 7. mbx and blaze: keep mbx as the compiler cache, keep blaze out of the gate

### 7.1 mbx today

velnor-new already wraps cargo with mbx. Each Rust job sets `MBX_CACHE_DIR` under `runner.temp`, runs `jdx/mr-boxington-action` with `backend: local`, imports a state bundle with `mbx cache import` and cleans at job end (evidence §C.2). Job env sets `MBX_BUILD_SCRIPT_EXECUTION=1`, `MBX_SHARE_OUT_DIR=1`, `MBX_CC=0` and `SOURCE_DATE_EPOCH=0`. The wrapper compiles through the mbx shim, and nextest runs through the same shim (`crates/velnor-actions-rust/src/argv.rs`, evidence §C.2).

The setup wastes the tool. The cache dir dies with the pod. The remote URL stays empty in 68 of 68 logs (evidence T9a). The state bundle that should seed the local store skips restore in 3636 of 3636 jobs (§1.4). What survives is intra-job reuse: 40–74% of lookups hit between the two passes inside one job. mbx works. Its store has nowhere to live.

### 7.2 mbx in the new design

Give mbx the node store and the site remote. Change four settings and nothing else:

- Set `MBX_CACHE_DIR` to the per-slot path on the node volume (§4.2 Tier 0). Keep one slot per runner slot. mbx serialises on a registrar flock and cargo locks the target dir, so slots must not share one dir (legacy `mbx_store.rs:1` learned this; keep the lesson).
- Set `MBX_REMOTE_URL` to the same-site cache service (§4.2 Tier 1). Push to main publishes objects. PRs restore. Forks restore from the fork family only. This matches the flow mbx documents for CI: pushes publish, pull requests restore (mbx site, CI and sharing guide).
- Drop the GitHub-cache state bundle for cluster jobs once Tier 0 proves its hit rate. Keep `mbx cache import` for hosted fallback only. The bundle costs a 70–110 s remote restore (§4.2) to seed a store that the node already holds.
- Keep `MBX_BUILD_SCRIPT_EXECUTION` split as today: skeleton at `1`, real members at `0` in producers (evidence §C.3). Build scripts run code. Their outputs cache only under the hybrid key the repo already computes. Do not widen it.

Expected gain **[Est, bounded]**: mbx documents one measured next-push case: cargo alone 18.9 s, cargo with mbx 9.2 s, with 353 of 354 compilations restored (hk benchmark, via the mbx site). Do not transfer that ratio to velnor-new. Transfer the mechanism: on a second push, only edited crates and dependents compile. For `velnor-runner-host`, cold build compiles 481 crates (evidence T9a). A one-crate edit with 20 dependents compiles about 21 crates warm. At the cold average rate that bounds the build step at roughly 10–20 s, down from 81 s. The limiter is dependent count, not mbx speed. Report the real dependent count per run from `cargo tree` instead of assuming it.

Watch one risk: mbx and the workspace target lanes (`target/velnor-clippy`, `velnor-doc`, evidence §C.2) must agree on fingerprints. A lane that changes flags misses every object. Keep flags stable per lane and record the flag hash in the run report. A sudden miss spike then points at the flag change, not at mbx.

### 7.3 sccache: do not stack a second compiler cache

The repo has no sccache today (no `RUSTC_WRAPPER` anywhere, evidence §C.2). Legacy supports sccache only as an explicit compat mode when a job asks for `mozilla-actions/sccache-action`, and its docs name transparent mbx as the default path (evidence §E.1.4). Follow that call. mbx caches at the cargo-unit level with path rewriting across checkouts. sccache caches single `rustc` calls and needs its own remote store. Running both pays two key computations for one hit. Pick mbx. Revisit only if a workload appears that mbx cannot key (for example standalone `rustc` calls outside cargo).

### 7.4 Blaze: what it is

Blaze is a separate compiler and daemon. It loads the project once, holds it compiled in memory and rebuilds only edited functions (blaze README: "blaze's daemon keeps your project compiled in memory, with no cargo or rustc under it"). It answers `build`, `check`, `run`, `test` and `bench`. Other cargo commands fail with `no such command`: no `fmt`, no `clippy`, no `doc` (blaze `docs/usage.md`, "Use it on your project"). It ships its own linker and compiles through Cranelift or LLVM from MIR.

Its speed claims are real and narrow. The README reports v1 numbers (v2 unmeasured): edits rebuild 10× faster on macOS arm64, 24× on Linux aarch64 and 11× on Linux x86-64 on average over real projects, with single-function edits at 56 ms to 451 ms on Linux x86-64. Those numbers measure edit-to-rebuild on a warm daemon. CI measures push-to-verdict on a cold checkout. The two workloads share almost nothing.

### 7.5 Blaze against velnor-new's pinned toolchain: it does not fit

Three facts block blaze from the merge gate. Each one alone blocks it.

1. **Blaze ignores the project pin.** velnor-new pins Rust 1.99.1 in `rust-toolchain.toml`, in `mise.toml` and in `.velnor/rust-toolchain.toml`, and generation fails when the sources disagree (evidence §C.7, hard decision 4). Blaze "builds every project with its own Rust, `blaze-2026-10-27` (today the nightly of that date), whatever the project pins" (blaze README, "Good to know"). Its toolchain doc states the rule without exception: blaze "ignores rust-toolchain, rust-toolchain.toml, rustup's settings and `RUSTUP_TOOLCHAIN`, and never runs or asks rustc" (`docs/toolchain.md`). A blaze build therefore verifies a different compiler than the one that ships. `docs/goals.md:36` requires every check to run "with the project's own compiler, features and settings". Blaze breaks that line by design.
2. **The checks do not exist.** velnor-new gates on clippy with pedantic lints as errors (root `Cargo.toml` `[workspace.lints]`), `cargo fmt --check`, `cargo doc` and `cargo nextest run --locked --offline --profile ci` (evidence §C.5, §C.7). Blaze runs none of clippy, fmt or doc and replaces the test runner with its own. A blaze-only gate silently drops three of the four check kinds.
3. **The checker is incomplete.** Blaze docs state the borrow checker on `main` "finds about two thirds of rustc's borrow errors" and instruct: "Keep `cargo check` or your CI as the final word" (`docs/usage.md`, "What works and what doesn't yet"). Memory cost runs to several GB per small project held in the daemon, and disk use exceeds cargo's (same doc). A CI pod that must hold five roots in daemons pays that per root, per slot.

Add the operational facts: blaze supports macOS on Apple silicon and Linux, needs its 125–160 MB toolchain download per host, pins per workspace through `[workspace.metadata.blaze] version` and refuses a build when a `RUSTC_WORKSPACE_WRAPPER` is set (blaze `docs/usage.md`). None of these is hard to satisfy. They buy nothing for the gate, because facts 1 to 3 already decide.

**Role for blaze:** local development only. A developer who runs `blaze build` and `blaze test` in a worktree gets sub-second feedback and still pushes to the same cargo gate. Offer it as an optional mise tool. Never mark a blaze check as required. Never let a blaze result satisfy `Required`. If the owner wants blaze data in CI, run one non-required job per PR that reports blaze versus cargo verdict agreement. Collect the disagreement rate for a month. Revisit only when blaze honours the project pin or the project moves its pin to the blaze nightly, and only with agreement above 99.9% and the missing check kinds covered elsewhere **[Est thresholds for a future decision, not a current claim]**.

Do not confuse blaze with the linker question. Blaze links with its own linker as part of its daemon. §8a covers `mold` under cargo, which keeps rustc 1.99.1 as the compiler. That change fits the pin. Blaze does not.

## 8a. Servers and cluster tuning: rank by gain per effort, change nothing from this session

Read this section as a work list for the owner to run on the nodes. This session changed no host and no cluster object. Every item names the current state first. Current state comes from evidence §B and from my read-only checks on 2026-11-19 **[M]**: `kubectl get nodes`, `describe nodes`, `top nodes`, `top pods`, `get --raw …/proxy/configz`, `get sc,pvc,ds,deploy`. Items the API cannot show (governor, SMT, NUMA, THP, sysctls, MTU) say so and name the node command that settles them during a maintenance window.

Cluster facts that bound every item: five workers, EPYC 9454 or 9454P, 96 threads and ~125 GiB each, Debian 13 (trixie), kernel 6.12.94, k3s v1.37.2 with containerd 2.3.7 **[M]**. Allocatable equals capacity on every worker. The kubelet sets no `kubeReserved`, no `systemReserved`, `cpuManagerPolicy: none`, `topologyManagerPolicy: none`, eviction hard at 5% free on `nodefs` and `imagefs`, image GC at 85% and 80% **[M configz]**. Workers carry one label, `velnor.io/runner=true`, and no taints **[M]**. Storage is one `local-path` StorageClass and zero PVCs. Runner pods use `emptyDir` only **[M]**. CPU runs at 84–95% on workers while memory sits at 18–36% **[M]**. The limiter is CPU scheduling and cache locality. Memory and disk capacity do not limit CI today.

### Ranked changes

Change one item at a time. Measure PR wall median, shard p90 and node CPU for at least five runs before and after, alternating nothing (the change is cluster-wide, so compare matched hours). Roll back the item, not the whole list, when its number does not move.

| Rank | Change | Expected gain and limiter | Risk | Roll back |
|---|---|---|---|---|
| 1 | Give build pods Guaranteed CPU (request equals limit, §6.2) and set kubelet `cpuManagerPolicy: static`, `topologyManagerPolicy: single-numa-node` with `cpuManagerPolicyOptions: full-pcpus-only`. Reserve `systemReserved` 2 CPU and 4 GiB, `kubeReserved` 1 CPU and 2 GiB on each worker. | Ends the 70-core burst (§1.6). Shard time stops stretching under load (T6 pattern). Gain shows as p90, not median: hold shard p90 within 20% of median under full fan-out **[Est target]**. The limiter moves from scheduler contention to real compile work. | Static pinning wastes cores when pods idle. Warm pods (§6.1) idle by design, so pin only build pods. Gate pods stay shared. | Set both policies back to `none`, clear reservations, restart kubelet per node. Pods lose pinning at once. |
| 2 | Create the node cache volume (§4.2). Carve a dedicated local volume per worker from the `data` LVM for Tier 0 stores. Keep XFS with `noatime` (already the mount for `/var/lib/rancher`, evidence §B.1.4). Mount with `nodiratime` as well. Give the cache agent a fixed path and a disk budget per store. | Carries §4. Without persistent volume the cache win is zero. With it, restore reads local NVMe instead of a 15 MB/s remote (limiter in §4.2). | A full cache volume pressures `nodefs` eviction. Keep the volume separate from the kubelet root and set the agent budget at 70% of the volume. | Unmount the volume and point `MBX_CACHE_DIR` back to `runner.temp`. Jobs run cold. Nothing else changes. |
| 3 | Set the linker to `mold` for CI builds. Add `-C link-arg=-fuse-ld=mold` through `RUSTFLAGS` in cluster job env only. Keep the host default linker for release builds until measured. | Link time drops on large test binaries. Bound the claim: linking is one stage of the build step. If linking takes 20% of an 81 s build, the best case saves 16 s **[Est; measure with `cargo build --timings`]**. Small crates gain nothing. | `mold` needs glibc and arch support on every worker. A bad linker flag fails every build at once. | Remove the env from the job template. Next pods link with `cc` again. |
| 4 | Derive `CARGO_BUILD_JOBS` and nextest threads from the pod CPU limit (§6.2). Delete the fixed `compiler_process_budget = 2` for cluster jobs (`.velnor/config.toml:26`). | A single-crate build stops crawling at 2 jobs on an 8-CPU pod. Expect build steps to scale until memory or the crate graph limits them. Measure per root: stop raising jobs when wall stops falling or RAM per pod passes 12 GiB **[Est stop rule]**. | Too many jobs thrash memory. The 32 GiB pod limit already saw 16.8 GiB peaks (evidence §B.2.2). The pod limit in §6.2 contains the thrash. | Restore the env values in the job template. |
| 5 | Pre-pull and pin every image. Keep `imagePullPolicy: IfNotPresent` with digest pins. Run a per-node pre-pull DaemonSet for the runner image and `docker:dind`. Add a local registry mirror (one registry per site) for `ghcr.io` and Docker Hub. | Pull time already measures zero in the evidence window (0 real pulls in 1,424 events [E]). The gain is insurance: a new worker or a new tag never stalls a fan-out on a cold pull. | A mirror serves a stale tag when upstream moves. Pin digests, so staleness fails loud. | Point image refs back to `ghcr.io`. Delete the mirror. |
| 6 | Label sites and pin runs to one site (§6.3). Set flannel WireGuard MTU to fit the underlay (test 1380, then 1420) and record `ping` loss during a fan-out. Keep Tier 1 traffic same-site (§4.2). | Removes 35 ms per cross-site round trip from restores that stray. MTU fix removes fragmentation stalls when they exist. Gain is unproven until measured: capture MTU and loss first. | A wrong MTU black-holes large packets. Change one node, run a fan-out, then roll out. | Restore the k3s flannel config and restart agents node by node. |
| 7 | Set CPU governor to `performance` on workers during a maintenance window. Check first with `cpupower frequency-info`. Disable deep C-states for build nodes only if frequency traces show down-clocking under load. | Sustained all-core turbo holds shard time flat. Gain is small when the governor already boosts under load. Expect 0–5% **[Est]**. Measure. Do not assume. | Higher idle power and heat. No correctness risk. | Set the governor unit back to `schedutil`. |
| 8 | Read NUMA before you pin to it. Run `lscpu` and `numactl --hardware` on one worker. EPYC 9454 is a single-socket part (48 cores, 96 threads). If the BIOS exposes NPS4, each node has four NUMA nodes. Keep `topologyManagerPolicy: single-numa-node` (rank 1) so a pod's CPUs and memory stay in one node. Test SMT: run one heavy root with siblings shared versus `full-pcpus-only`. | Memory-local shards stop paying cross-node latency. The limiter is memory bandwidth on large links and test working sets. Expect the win in Heavy shards only **[Est]**. | Wrong topology reads waste a maintenance window. Code does not change. | Topology policy back to `none`. |
| 9 | Raise file limits for build pods. Set pod `ulimit` nofile to 65536 through the container runtime config, and host `fs.inotify.max_user_watches` to 524288, `fs.file-max` to at least 1,000,000. Check current values with `sysctl` on a node first. The API does not show them. | Removes "too many open files" and watcher failures under parallel cargo and file watchers. Gain is failure removal, not speed. | High limits let a runaway job hold more handles. The pod PID and CPU limits still bound it. | Restore the sysctl file and restart kubelet. |
| 10 | Tune writeback for build churn. Set `vm.dirty_background_ratio` to 5 and `vm.dirty_ratio` to 15 on workers. Keep swap off for build nodes (`failSwapOn` already tolerates swap; confirm with `swapon --show`). Set transparent hugepages to `madvise`, never `always`. Check with `cat /sys/kernel/mm/transparent_hugepage/enabled`. | Smooths the burst when cargo writes thousands of small files. THP on `always` stalls large rustc allocations. Expect fewer tail stalls, not a median shift **[Est]**. | Aggressive writeback raises I/O during checkpoints. Values here stay conservative. | Restore `/etc/sysctl.d` values and run `sysctl --system`. |
| 11 | Move build scratch to tmpfs only where it helps. Mount `emptyDir` with `medium: Memory` and a `sizeLimit` for `/tmp` in build pods. Keep `target` and all caches on NVMe. tmpfs target dies with the pod and breaks §4. | `/tmp` churn (test temp files, linker temp) stops hitting the disk. Gain is small. Test one root before and after. | tmpfs eats RAM. Memory sits at 18–36% **[M]**, so headroom exists. The `sizeLimit` caps the damage. | Drop `medium: Memory` from the template. |
| 12 | Set containerd snapshotter and GC for CI churn. Keep the default overlayfs snapshotter. Raise image GC headroom on cache nodes only after rank 2 lands, so GC never evicts the runner image during a fan-out. Set kubelet `imageGCHighThresholdPercent` to 90 on workers with the large disks (velnor-240 holds ~50 TB imagefs, velnor-105 ~34 TB [E §B.1.4]). Small-disk workers keep 85. | Fewer mid-fan-out image evictions. No speed gain on the happy path. | A full disk evicts pods. The 5% `nodefs` hard eviction stays as the guard. | Restore 85 and 80 in the kubelet config. |
| 13 | Replace `local-path` only if PVC latency shows up. Measure cache volume I/O with `iostat` during a warm restore. When local-path (which provisions under `/var/lib/rancher/k3s/storage`) bottlenecks, move Tier 0 to a direct local volume through a simple local-volume provisioner or a static PV per node. Keep one StorageClass per purpose. | Local-path adds a provisioner hop once per volume, not per read. Expect no gain until measurement says otherwise. This rank sits last for that reason. | A new provisioner is new failure surface. Static PVs need manual cleanup. | Point new stores back to `local-path`. Old volumes keep serving. |

### OS items that need a node shell, not kubectl

The API hides governor, SMT, NUMA, THP, sysctls and MTU (evidence §B.1.6 lists the same gap). During one maintenance window per worker, record: `lscpu`, `numactl --hardware`, `cpupower frequency-info`, `cat /sys/kernel/mm/transparent_hugepage/enabled`, `sysctl vm.dirty_ratio vm.dirty_background_ratio fs.file-max fs.inotify.max_user_watches`, `ip link show flannel.1`, `lsblk` and the LVM layout. File the output with the cluster docs. Today `RN/` holds no governor, SMT, NUMA or THP text at all (evidence §B.1.6). One BBR plus 128 MB buffer experiment ran on 2026-11-16, gave no gain and was reverted (evidence §B.1.6). Do not repeat it without a new hypothesis.

Kernel and scheduler: stay on Debian 13 with kernel 6.12.94. Do not chase a newer kernel for CI speed without a profile that names the win. The default scheduler serves build pods well once CPUs pin (rank 1). Test `vm.swappiness` at 1 only when a node shows swap use. CI nodes should not swap.

Filesystem detail for many small files: XFS suits the cargo workload (many creates, hard links, reflink copies where the tool uses them). Keep it. ext4 also works and buys nothing here. btrfs and ZFS add snapshot features this design does not need, and their copy-on-write costs writes on every cargo output. Stay on XFS with `noatime,nodiratime`. Format any new cache volume with a larger inode ratio only when inode use passes 70%: workers report 234M inodes on `nodefs` today [E §B.1.4], so inodes do not limit the design.

### Build settings that pair with the host changes

- Keep `CARGO_INCREMENTAL=0` in CI (already set, evidence §C.7). Incremental state ties a build to one machine and poisons the object store.
- Set `CARGO_PROFILE_DEV_DEBUG=0` for CI test builds where the job sets it today only for ledger-cli (evidence §C.7). Debug info costs link time and disk. Keep full debug on hosted fallback until measured there.
- Do not set global `codegen-units` overrides. Let each workspace keep its profile. Measure one root with `codegen-units=1` release-style builds only for release jobs, never for test shards.
- Keep one compiler cache: mbx (§7.3). Host tuning must not add sccache behind it.

### What more hardware buys

Buy workers, not larger workers. Each EPYC worker adds 96 threads, ~125 GiB RAM and one warm cache replica. At the §6.2 shape it adds about ten build shards of parallel capacity. Two more workers (seven total) lift tailrocks headroom enough to hold the warm pool plus two full fan-outs without queue **[Est from §6.1 pool math]**. A second control-plane node buys availability, not speed: the single control plane (velnor-237, i5-13500) holds the API and etcd, and its loss stops scheduling while jobs keep running. Add one standby control-plane node when the owner values maintenance without downtime. Faster NVMe buys little: current disks sit far from saturation (node filesystems use 44–110 GB of 3.78 TB [E §B.1.4], and the limiter in §1 is CPU). RAM buys nothing until pod memory limits rise. Spend on workers first, control-plane redundancy second.

## 8. Phased plan: each phase ends at a measured gate

Run the phases in order. Each phase ships, measures on velnor-new for at least 20 merges, and holds its target before the next phase starts. Measure the same way every time: `gh api` jobs for the run window, report PR wall median and p90, runner-minutes per run, queue per job class, crates compiled and downloaded, mbx hits, and executed test count. Report median and range over at least five matched runs. A phase that misses its target stops. Fix it or roll it back. Do not stack the next phase on a miss.

Baseline for all targets (scale-set era, evidence T2): PR wall median 26.7 min, p90 92.3 min; PR runner-minutes 52.9; cluster queue 0.2 min; hosted gate queue p90 21–26 min; crates downloaded 435 per heavy job; first cargo well over 30 s on cold pods.

### Phase 0 — Move the gate onto the cluster (days, no new code)

Ship `velnor-actions` v0.1.81. Move Plan, Ledger CLI, Cargo Deny, Jactionlint, Cargo Machete, Alint, Required verdict and Required to the scale set in locked-down pods (§5.1). Keep state seal, release and publish hosted.

- **Target:** PR wall median under 15 min and p90 under 30 min over 20 merges. Hosted queue in CI reaches zero. Fan-out span stays near 9 min.
- **Detect failure:** any `Required` job waits over 2 min for a pod, or the gate pods gain a Docker socket or write token in review. Audit the pod spec in the same PR.
- **Contain:** revert the workflow labels to `ubuntu-26.04`. GitHub queues the jobs as today. No data loss.

### Phase 1 — Stop verifying the same tree twice (days, workflow only)

Add the push skip and the merge-group tree comparison (§5.2). Keep a nightly scheduled full run that selects everything (`docs/goals.md:66`).

- **Target:** push full runs fall by at least 60% (from 75% duplicate toward under 15% that carry new trees). Runner-minutes per merge fall by 35–45. PR wall does not move.
- **Detect failure:** a push skip fires on a tree that never passed. Log the tree hash, the source run ID and the verdict for every skip. Sample five skips per week and open the source run.
- **Contain:** one config flag restores full runs on every event.

### Phase 2 — Node-local cache and warm placement (weeks, the big build)

Ship the node cache agent, Tier 0 stores, Tier 1 site servers, cache labels and preferred affinity (§4, §6.3). Wire `MBX_REMOTE_URL`, move `MBX_CACHE_DIR` to node slots, bake the toolchain into the runner image and fix the crates mirror through `CARGO_HOME` config (§7.2). Apply §8a ranks 1, 2 and 4 in the same window, since placement without CPU shaping still thrashes.

- **Target:** first cargo under 30 s at p90 (the `docs/goals.md:57` target). Crates downloaded per heavy job under 25. mbx hit rate over 70% on new PRs and over 85% on second pushes (§4.5). `velnor-runner-host` build step under 25 s median. PR wall median under 10 min.
- **Detect failure:** the `Cache` check (`docs/goals.md:67`) turns red, or crates compiled stays flat while hit claims rise. Either signal means the reported work did not happen. Count executed compilations, not store size.
- **Contain:** point `MBX_CACHE_DIR` back to `runner.temp` and clear `MBX_REMOTE_URL`. Jobs run cold at Phase 1 speed. Stores stay on disk for diagnosis.

### Phase 3 — Per-root jobs and test shards (weeks, generator work)

Move velnor-new onto `velnor-gen` output for the Rust part: five root jobs, one binary build per root, `nextest --partition` shards per §5.3, mold in cluster env (§8a rank 3). Prove on `velnor-lab` first (`docs/goals.md:76`). Keep the engine workflows for non-Rust checks until they match.

- **Target:** PR wall median at 5 min and p90 at 8 min for the typical PR (§5.5). Runner-minutes per PR at 20–25. Executed test count per root equals the unsharded count. Shard p90 under 120 s per root.
- **Detect failure:** executed tests drop, or shard p90 rises while CPU sits idle. Both point at partition or budget errors. Compare `nextest list` counts in the same run report.
- **Contain:** set `default_shards` back to 1 in `.velnor/config.toml`. Root jobs run whole at Phase 2 speed.

### Phase 4 — Replace ARC with the Velnor listener and warm pool (weeks)

Ship Layer A (§3.2): the Rust listener on `velnor-runner-github` types, warm pod pool (§6.1), PriorityClasses and per-org caps (§6.5). Run it beside ARC on a second scale set for `velnor-lab`, then move tailrocks orgs one at a time. Apply the remaining §8a ranks as separate measured changes.

- **Target:** assignment to first step under 10 s at p90, queue median under 0.5 min per org during a double fan-out. PR wall holds Phase 3 numbers with lower variance (p90 within 1.5× median).
- **Detect failure:** listener session churn (refresh loops, 401 storms) or pods stuck in refill. Alert on session refresh rate and pool depth. The legacy incident list (broker 409 loops, lost registrations, evidence §E.6) names the exact failure shapes to alarm on.
- **Contain:** scale ARC back up and the listener to zero. GitHub sees the same scale-set name throughout.

### Phase 5 — Native fast lane, optional (months, gated)

Build the restricted executor (§3.4). Admit one root at a time, shadow first, gate second. Stop here when the owner refuses the `goals.md` amendment (§3.5). Phases 0 to 4 already meet the wall targets.

- **Target:** admitted jobs start cargo 5–20 s sooner than the official Worker path, with identical step verdicts over 100 shadow jobs per root. No target on PR wall beyond 0.5 min.
- **Detect failure:** any verdict split between executors on the same tree. One split halts expansion for that root.
- **Contain:** route the root back to the official Worker by label. The lane never holds unique state.

### Top risks across all phases

| Risk | How it shows | Containment |
|---|---|---|
| Wrong skip merges untested code | A file maps to no crate, or a tree compare hashes the wrong ref | Unmapped files select everything (§5.2). Nightly full run. `Required` audit logs every skip with its proof. |
| Cache serves a stale or poisoned object | `Cache` check red, or builds pass locally and fail on a cold node | Content keys plus default-branch-only shared writes (§4.4). Weekly cold-node full run per root. |
| Warm affinity overloads one node | Queue rises while other nodes idle | Preferred affinity only, spill threshold in §4.3, per-org caps in §6.5. |
| GitHub changes the protocol | Acquire or renew errors spike after a runner release window | Pin the runner image. Upgrade inside the 63-day window on `velnor-lab` first. Official Worker path stays as fallback for every job. |
| dind privilege escape from PR code | Unexpected egress or host mount in a job pod | Tainted dind pool, NetworkPolicy, no host socket, hook scope check (§6.4). Fork jobs never share nodes with writable stores. |
| Three orgs starve each other | One org queue grows during another org burst | Per-org caps and priorities (§6.5). Report queue per org weekly. |

Buy no hardware for Phases 0 to 3. The current five workers hold the load once caches hit (§1.6). Revisit hardware at Phase 4 with queue data in hand: two more workers when double fan-outs queue over 1 min at p90 with nodes over 85% CPU.

## Owner questions

1. **`docs/goals.md:16` and the fast lane.** The line states that jobs always run on GitHub's official runner. Phases 0 to 4 obey it. Phase 5 breaks it for `velnor-gen` profile jobs only. Amend the line to allow the restricted profile, keep Phase 5 in permanent shadow mode, or drop Phase 5? The wall targets in this proposal do not need Phase 5.
2. **Gate trust on shared nodes.** Phase 0 moves Plan, Required verdict and Required onto the cluster in pods with no socket and no write token. Keep that boundary, or keep Required hosted and accept its queue (p90 21–26 min [E]) as the price of the boundary?
3. **Push verification.** Phase 1 skips the full push run when the merge-group run already passed the same tree. Keep a short push workflow for cache sealing and freshness only, or keep one full push run per merge as a second opinion at ~62 runner-min each?
4. **Cache write scope.** This design lets same-repo PRs write only PR-scoped node slots, and only default-branch pushes write shared generations. Confirm that scope, or allow merge groups to write shared generations for faster queue batches at higher poisoning risk?
5. **dind lifetime.** Privileged dind stays on tainted nodes for Heavy jobs. Fund work to move Docker-needing tests (`docker-networks` group in `velnor-runner-host`) off dind, or accept the tainted pool as permanent?
6. **Org fairness.** Tailrocks, ChainArgos and jackin share five workers. Set hard per-org CPU caps that can idle nodes while another org queues, or keep burst sharing with priorities only?

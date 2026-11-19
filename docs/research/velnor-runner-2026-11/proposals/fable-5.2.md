# Proposal by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-18, 61 min |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

- **The runner binary is not the bottleneck; the system around it is.** Push-to-merge median was 39 min on 11-16, 62 min on 11-17 and 402 min on 11-18. On 11-18, 282 of those minutes passed between entering the merge queue and merging: the queue builds 2 entries at a time, and one run took 64–72 min, about 73% of it in three serial waits for GitHub-hosted runners.
- **The hosted wait is self-inflicted.** The org is on GitHub's Free plan (20 concurrent hosted jobs). Our own `Source qualification` workflow uses about 2,400 hosted-seconds per run on every PR event.
- **Without any queue a run still takes 10.9 min and 53–65 runner-minutes.** 43.7% of Rust-job seconds are fixed per-job overhead (49 s × 29 jobs), every job compiles cold (4.4× duplicate compilation per run), and three test suites set the critical path. One merged PR costs about 347 runner-minutes, and a full run fails about 10% of the time on a commit that already passed.
- **Change-aware selection does not work today.** The heavy jobs ran in 97.4% of PR runs. A one-file test change in `ci-helper` ran 12 jobs, including both runner-host suites.
- **Hypotheses:** H1 fails as stated, H2 and H3 partly hold, H4 holds and is the strongest, H5 holds in principle but is not delivered.
- **Design:** keep GitHub's protocol and YAML. Replace ARC's pod-per-job with a Rust dispatcher and node agent that run jobs in warm, CPU-isolated slots over node-local, trust-scoped caches. Run the unmodified official runner first, the official Worker under a Velnor listener second, and a native Rust executor last, each with automatic fallback.
- **The cache is the product:** one mbx store per node and trust scope, pre-provisioned toolchains, local artifact and cache endpoints, and a test-verdict cache whose inputs the sandbox enforces, so a skip is a proof.
- **Honest minimum to verdict:** about 85 s for the median PR (47% touch only `generator/` or `ci-helper/`), about 2 min for runner PRs after test fixes, 5–6 min for engine PRs until two slow suites are fixed.
- **Targets:** push-to-merge P50 ≤ 30 min after week 1 with no new runner, ≤ 6 min with the node agent, ≤ 3 min with the verdict cache.
- **blaze does not fit** the pinned toolchain: it compiles with its own nightly and ignores `rust-toolchain.toml` by design. mbx stays and moves to a node-persistent store.

Source tags used below: **[M]** I measured it in this session and name the method; **[E]** taken from the evidence pack; **[D]** owner's cluster notes; **[est]** my estimate, with the arithmetic. Local builds ran on an Apple M5 Max (18 cores) with load average 15–97 from other work, so I report CPU-seconds where wall time would mislead. Scripts and raw data are in `scripts/` and `raw/` next to this file.

## 1. Where does the time go today?

At a glance, in the brief's terms. Details and sources follow.

| Part | On the wall clock of one PR run | Share of runner-minutes |
|---|---|---|
| Queueing | 40% over the window, 73–79% on 11-18 (hosted gates) [M, E]; plus 14 s per job on our cluster [M] | none |
| Setup (tools, "Set up job", teardown) | about 1.5 min of the 10.9 min floor; the Plan and verdict jobs are mostly setup [M logs] | 24.8% [M] |
| Downloads (checkout, crates, artifacts) | about 1 min: checkouts and 31 report artifacts [M logs] | 14.5% [M] |
| Compilation | 3–6 min: the longest job does four cold builds [M log] | 16.0% in build steps, plus builds inside the two rows below [M] |
| Tests, with clippy and doc beside them | about 4 min in the longest job; 3–9 min in the runner-host jobs beside it [M, E] | 28.5% [M] |
| `maintenance-helpers` (four builds, then tests) | 8.5 min median; it is the longest job in 44 of 51 runs [E, M] | 14.3% [M] |
| Cache transfer from GitHub | none on the path | 1.0% [M] |
| Duplicate work | – | 4.4× compile duplication inside a run; 7 runs per merged PR [M] |

### 1.1 Push to merge is dominated by the merge queue backlog

[M] GraphQL timelines of the 200 most recently merged PRs (`scripts/prt.py`); minutes, median.

| Merged on | PRs | Last push → merged (p75) | Last push → queue add | Queue add → merged | Queue depth median / max |
|---|---|---|---|---|---|
| 11-14 (no merge queue) | 25 | 24.2 (29.7) | – | – | – |
| 11-16 | 68 | 38.8 (56.9) | 19.4 | 18.8 | 2 / 7 |
| 11-17 | 42 | 62.1 (111.5) | 45.2 | 15.3 | 3 / 6 |
| 11-18 (to 17:00Z) | 34 | 402.3 (510.3) | 107.0 | 281.7 | 6 / 25 |

- The queue-add time tracks the PR run (19.4 min against a 23.2 min run on 11-16), which fits auto-merge: the PR enters the queue when its checks pass.
- Ruleset 24396608 has `max_entries_to_build: 2`, `grouping_strategy: HEADGREEN` [M `gh api`]. Service rate is about 2 merges per merge_group run [est]: 7.5 per hour at a 15.9 min run (11-16) and 1.9 per hour at 64.4 min (11-18). Demand reaches 7–8 merges in one hour [M], so on a slow day the queue grows without bound.
- 24 of 142 queued PRs (17%) were queued more than once [M]. 14 of 50 sampled merge_group runs failed [M re-analysis]: Alint 5, runner-host (local-mode) tests 4, velnor-gen tests 3, velnor-ci tests 2.
- **A full run fails about 10% of the time on a commit that is already verified.** Since 11-16, 96 of 97 completed push runs were on a SHA with a green merge_group run, and 10 of those 96 failed anyway [M runs list]. That is the flake rate, measured on identical commits, and each such failure inside the queue ejects a PR.
- One real failure took 28 min to report: run 44357962729 failed Alint ("Rust source exceeds 400 physical lines") in 26 s of work, after 153 s of hosted queue, and the verdict arrived only after `Required verdict` queued another 247 s [M].

### 1.2 One run is a serial chain of three hosted waits around a 9-minute fan-out

[M] re-analysis of the jobs JSON for 200 sampled runs (`scripts/a1.py`); success runs; minutes, median. It reproduces evidence table T5.

| Day | Event | n | Wall | Plan queue | Fan-out span | Verdict queue | Required queue |
|---|---|---|---|---|---|---|---|
| 11-16 | PR | 23 | 23.2 | 5.9 | 8.4 | 1.9 | 2.1 |
| 11-17 | PR | 13 | 16.0 | 2.3 | 9.3 | 0.7 | 1.2 |
| 11-18 | PR | 13 | 71.8 | 21.6 | 10.7 | 23.3 | 15.3 |
| 11-18 | merge_group | 6 | 64.4 | 15.3 | 13.5 | 15.3 | 5.3 |

- Over all 116 scale-set-era success runs the three gate queues sum to a median 8.0 min, 40% of wall; on 11-18 the share is 73% [M].
- **Floor with no hosted queue:** 12 runs had all three gate queues under 0.5 min. Their wall median is 10.9 min (9.6–16.7) [M].
- Fresh run 44345031627 (merge_group, created 16:15Z): 2,866 s in total; Plan queued 1,188 s, the fan-out took 769 s, `Required verdict` queued 666 s and `Required` 150 s. 70% was hosted queue [M].
- The longest fan-out job was `Check / maintenance-helpers` in 44 of 51 successful PR runs [M].
- The gate jobs themselves waste time even unqueued [M job logs]. Plan ran 45 s: checkout 11.2 s, `mise install rust` 8.1 s, "Select attested state" 16 s, the actual plan 1.6 s. `Required verdict` ran 49 s, 32.7 s of it downloading 31 report artifacts. `Ledger CLI` builds its tool on every run (22 s).

**Why hosted runners queue.** `gh api orgs/tailrocks` reports plan `free`; GitHub's documented limit for that plan is 20 concurrent hosted jobs (docs.github.com/en/actions/reference/limits). The page does not say whether public repositories are exempt; 86 queued jobs with waits of 20–40 min [E] say they are not. Hosted load per run [M, 5 runs each]:

| Workflow | Trigger | Hosted-seconds per successful run | Runs on 11-18 [E] |
|---|---|---|---|
| Source qualification | every PR event (`source-qualification.yml:4`) | 2,392–2,484 (three jobs of 7–16 min) | 267 |
| Job image candidate | push to any branch (`job-image-candidate.yml:4`) | 284–371 | 306 |
| State seal | push to main | 581–1,122 | 19 |
| CI (8 hosted jobs) | PR, merge_group, push | about 230 [E 3.78 runner-min] | 348 |

### 1.3 Runner-minutes: half of every Rust job is fixed overhead

[M] step timestamps of 5,037 executed scale-set jobs, 9,031 runner-minutes (`scripts/a3.py`).

| Category | Share |
|---|---|
| Parallel group: tests, with clippy, doc and fmt inside the same interval | 28.5% |
| Tool setup: mise, Rust toolchain, mbx, verifications | 15.1% |
| `maintenance-helpers` named check | 14.3% |
| Compile: "Build test executables" | 13.3% |
| Artifacts: plan, ledger CLI, reports, up and down | 7.0% |
| Between steps and job teardown | 5.8% |
| Checkout | 4.1% |
| "Set up job" (action downloads) | 3.9% |
| `cargo fetch` (crate downloads) | 3.4% |
| Fixture build in `velnor-actions-cli` | 2.7% |
| Other steps, GitHub cache restore | 2.1% |

- A Rust job has a median 49 s of fixed overhead (mean 50.7, p90 64) and a median 17 s of real work (mean 65). Fixed overhead is **43.7% of Rust-job seconds**. Per full run: 24.3 min of overhead against 24.9 min of work [M].
- The 49 s is made of: tool install, mostly the Rust toolchain, 9.8 s, crate fetch 4.6 s, end-of-job finalisation 4.7 s, "Set up job" 4.2 s, checkout 4.2 s, ledger download 3.2 s, report upload 2.9 s, plan download 2.5 s, mise setup 2.4 s, and about 10 s of small steps [M step means].
- Job start on our cluster: 14 s median for the 84% of jobs whose pod is created on demand (3.3 s until the pod is scheduled, 5.6 s until the runner container starts, which matches the dind sidecar's 5 s startup-probe period [E], 5.0 s until the runner has a session and the job). It is 3 s for a pre-existing idle pod [M: 93 jobs joined to pod events by runner name].

### 1.4 Three test suites and one cold build set the critical path

- **velnor-runner-host tests are CPU-bound and growing fast** [M logs]. On 11-16 13:37Z: 232 tests, 72 s wall, 1,857 s summed test time. On 11-18 16:31Z: 382 tests, 534 s wall, 19,605 s summed; 62 tests ran over 120 s; the slowest took 534 s. nextest runs 96 tests at once (`crates/velnor-runner/.config/nextest.toml:15`), and one pod reached 82.6 cores with a 2-CPU request [M `kubectl top`].
- I ran one of those tests alone: 179 s wall, 142.5 s user CPU, 1.03 × 10¹² instructions, zero block I/O. `sample` shows the time in `turso_parser` AST clone and drop, the allocator and SQL parsing. It is schema work in a debug build, not sleeping and not disk [M]. Open PRs #1333 and #1349 report the same cause and cut 40–45%.
- **velnor-actions-cli:** 223 tests, 323.6 s wall. One binary, `image_publication_public`, holds 79 tests with 3,529 s summed; the slowest is 323.5 s. A fixture build adds 89–109 s before the tests [M log, E T10].
- **maintenance-helpers** (770 s in my log): a silent debug build of the CLI (about 117 s), a release check (67 s), two more debug builds (143 s and 54 s), all capped at 2 build jobs (issue #1305, comment 6980497787) and with no cache, then 54 tests in 257.6 s [M log 134954375115].
- **No compile result crosses a job boundary.** In run 44345028211 all 29 Rust jobs report "0 B downloaded", and in the 21 that build tests the build pass has 0 hits (for example "0 hits, 0 misses, 404 not looked up … 7.1 GiB stored locally"). Hits in later passes come only from what the same job compiled seconds earlier. Summed over the run, jobs compiled 2,106 crate builds for 475 distinct crate versions: **4.4× duplication**. Six crates were compiled in 27 jobs [M all 39 logs, `scripts/dupcompile.py`].
- In that run, of 5,032 job-seconds, cargo builds took 2,012 s and nextest 1,838 s [M].

### 1.5 Duplicate verification

- Per merged PR (142 PRs merged 11-16 to 11-18): 4.77 PR-event runs (2.62 success, 0.87 failure, 1.28 cancelled), 1.25 merge_group runs and 1 push run [M runs list joined to PRs].
- Cost per merged PR [est]: 3.49 completed PR runs × 52.9 + 1.28 cancelled × 15 (assumed) + 1.25 × 64.6 + 1 × 62.0 = **347 runner-minutes**.
- 120 of 159 successful merge_group runs have a push run on the identical SHA [E]. That third run verifies nothing new.
- The root workspace also compiles `velnor-runner-host` (34.6 s in my cargo timings), so that crate is built under two lockfiles in every full run [M].

### 1.6 Our cluster is idle or saturated, rarely in between

- 17:24Z: all five workers at 0–1% CPU with 4 idle runners, while 7 runs were queued on GitHub [M].
- 17:37Z: velnor-240 at 86 cores, CPU pressure "some" 8.4% (10 s average), I/O pressure 1.3% [M kubelet summary].
- 17:50Z: velnor-240 68%, velnor-241 51%, the other three 4–12%, 46 runner pods. The scheduler packs by the 2-CPU request and cannot see real use [M].
- Since boot, velnor-240 accumulated 7,963 s of CPU pressure, 1,081 s of I/O pressure and 80 s of memory pressure [M cAdvisor]. CPU is the contended resource.
- Job time rises with load: runner-host median 3.7, 6.1 and 8.8 min on 11-16, 11-17 and 11-18 [E T6]; 691–757 s in my fresh runs.
- `generator/` and `ci-helper/` kill any test after 2 s (`generator/.config/nextest.toml:25`, `ci-helper/.config/nextest.toml:11`). Under CPU pressure that is a source of the queue failures in 1.1.

### 1.7 Where I differ from the brief or the evidence pack

- "A PR run takes ~92 min": true for 11-18 only. The number that matters is push-to-merge, and it was 402 min that day because of the merge queue. Neither the brief nor the pack measures it.
- "Cluster at 18–29% CPU" and the pack's "92–95%": both are snapshots of a bimodal load (0–1% and 86 cores within 15 minutes).
- "PRs skip unaffected jobs": in practice the heavy jobs run in 97.4% of PR runs (section 5).
- "State restore skipped in 3636 of 3636 jobs": I agree for scale-set jobs. Four runs on 11-15 before the switch restored it on hosted runners.
- Live jobs report runner 2.338.2, two patch releases after the cloned 2.338.0 [M log line].
- Why velnor-legacy was archived: I found no recorded reason either. My inference from its history: 2,231 commits in 4.5 months, 804k lines, 60 of the last 100 commits are fixes, a scale-set lane that delegates to the official runner was added on 2026-09-18, and the generator moved to velnor-new. It is an inference.

## 2. The owner's hypotheses

| # | Hypothesis | Verdict | Expected numbers |
|---|---|---|---|
| H1 | Rust everywhere, native actions instead of JavaScript, is dramatically faster because native beats interpreted | **Fails as stated.** The category is worth attacking for another reason | Interpreter cost about 0.6% of job seconds. Local built-ins or local endpoints remove about 18 s per job |
| H2 | A from-scratch Rust runner beats reusing the official runner and ARC | **Partly holds.** Replacing ARC and the listener: yes. Replacing the Worker: last and small | Dispatch 14 s → under 1 s. Native executor saves about 20 s of wall per run |
| H3 | Kubernetes is the right base | **Partly holds.** Right for the fleet, wrong unit for jobs | Pod-per-job costs 14 s per job and forbids warm state |
| H4 | Local caches on our hardware beat remote caches | **Holds** | Warm build 8.8 s against 22.5 s cold; two concurrent cold builds cost 1.07× one; remote bundle 40–107 s to fetch |
| H5 | Workflows can skip what a change does not touch | **Holds in principle, not delivered today** | 58% of PRs need only small roots: about 85 s instead of 10.9 min |

**H1.** In run 44345028211, `uses:` steps took 573 s of 4,807 job-seconds (11.9%), plus 148 s of "Set up job" [M `scripts/actiontime.py`]. Node starts in 87–147 ms per action. With about 7 action calls per job, interpretation is about 28 s of 4,807 s, 0.6% [est]. The rest is network: checkout spent 0.75 s fetching and 1.5 s on lazy blob fetches of 2.6 s; `mise-action` downloads the same mise binary in 4.6 s per job; each artifact call is 2.7–3.1 s of round trips; six action tarballs are downloaded per job. Locality removes this, in any language. The official runner already has `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (`actions-runner/src/Runner.Common/Constants.cs:293`, used in `src/Runner.Worker/ActionManager.cs:1230-1295`).

**H2.** ARC's limits are architectural: one pod per job, the Kubernetes API in the dispatch path, no placement control, nothing survives. Those must go. The official runner's own cost is small: 4.2 s "Set up job" (most of it removable by configuration) and 4.7 s of finalisation. A native executor removes perhaps 6–8 s per job. With 3 serial stages that is about 20 s per run [est], under 4% of today's floor and 15–25% of the final target. velnor-legacy shows the price: about 50–60k lines of product code for the native lane, and a tail of protocol bugs (its issues #670–#672 on logs and artifacts, #389 and #408 on registration).

**H3.** Kubernetes is good at deploying and upgrading the agents and mirrors. It is not what makes hardware usable in parallel: the cluster sat at 0–1% while runs queued behind hosted gates, and at 68%/51%/4% across nodes a few minutes later. Parallelism is limited by the workflow's serial chain and by missing admission control.

**H4.** Remote: GitHub cache restores at 13.6–22.4 MB/s from our pods with 120 ms round trips [D `state/log.md:31`], so a 0.88–1.46 GB state bundle [E T9c] takes 40–107 s before a 26–66 s import [M hosted-era steps]. A cold build of the runner workspace takes 73–105 s [M logs]. The remote cache is about as slow as compiling. Local, measured on the `generator` workspace with a scratch mbx store: cold 22.5 s; a second checkout at another path 8.8 s with 117 of 118 hits; two concurrent cold builds sharing the store 63.7 CPU-seconds in total against 59.5 for one. A one-function edit in the 157k-line `velnor-runner-host` rebuilt in 10.9 s with a warm incremental target, against 82–99 s for a full compile of that crate under the same machine load [M]. One caveat: a shared tier over 1 GbE moves 7 GB in about 64 s [est 110 MB/s], so placement on a warm node matters more than a fast remote.

**H5.** The plan artifact of run 44308933193 explains the failure: eight packages carry `undeclared_reads` warnings (a build script, or a test that reads a file outside its crate), so the planner always selects them [M plan.json]. Those eight include every heavy Rust job. The fix is to enforce inputs, not to guess them (section 5).

## 3. The runner architecture I recommend

### 3.1 What stays compatible with GitHub

- Workflow YAML, expressions, contexts, `needs`, job outputs, `permissions`, file commands and workflow commands.
- The scale-set API: sessions, long-poll messages, `acquirejobs`, `generatejitconfig`, capacity header.
- The runner protocol: broker session, run-service `acquirejob`, `renewjob` every 60 s, `completejob`, Results service for logs and step status.
- The check names `Required verdict` and `Required` that the ruleset requires.
- Labels `[velnor, ubuntu-26.04-scale-set]`, so no workflow has to change to move.

### 3.2 What is redesigned inside

```
GitHub: scale-set API, broker, run service, results            (unchanged)
   │
velnor-dispatch   Rust, on the control node, active + standby
   │  owns the scale-set session and a pool of pre-registered JIT runners
   │  advertises capacity = free slots, so GitHub queues instead of our nodes thrashing
   ▼
velnor-node       Rust agent, one per worker
   ├─ slots: cgroup v2 with elastic cpusets, namespaces; a microVM lane later
   ├─ storage: tools (read-only), crate registry, git mirror, mbx store per trust scope,
   │           stable workspaces, verdicts, run artifacts — all on one reflink filesystem
   ├─ local endpoints: Actions cache and artifact API, crates mirror, image mirror
   ├─ L0 executor: unmodified official runner (`run.sh --jitconfig`)        Phase 2
   ├─ L1 executor: official `Runner.Worker` under the Velnor listener        Phase 2b
   └─ L2 executor: native Rust step executor for generated jobs              Phase 3
```

1. **Slots, not pods.** A slot is a long-lived, pre-warmed execution place with a reserved CPU set. A job gets a fresh copy-on-write view of warm state in well under a second. Nothing a job writes reaches the next job unless a trusted run promotes it.
2. **Admission at the source.** The dispatcher sends `X-ScaleSetMaxCapacity` equal to free slots. Jobs wait at GitHub, in order, instead of 30 pods fighting for 96 threads.
3. **A gate lane on the control node.** `Plan`, `Required verdict` and `Required` run on velnor-237 (20 threads, 0–6% used [M]) in reserved slots. They never queue behind builds, and no PR code runs on that node.
4. **L0: the unmodified official runner in a slot.** The agent registers a JIT runner in the background while the slot is idle and starts `run.sh` inside it. A job-started hook (`ACTIONS_RUNNER_HOOK_JOB_STARTED`; velnor-new already ships `velnor-job-start-hook` for authorisation) tells the agent the job's repository and event, and the agent mounts the caches for that trust scope. This needs no runner-protocol code, only the scale-set client.
5. **L1: late binding.** The listener half of the runner is small: session, acknowledge, `acquirejob`, renewals, and handing the job message to the Worker. The hand-off is a pipe with a 4-byte type and a length-prefixed string (`src/Runner.Common/ProcessChannel.cs:12-19`, `src/Runner.Listener/JobDispatcher.cs:480`). The Worker completes the job itself, and it needs only the runner's settings file, so the runner's private key never enters the sandbox. If Velnor owns the listener, it sees the job message before choosing a node, so it can send each job to the node that is warm for it. The Worker stays GitHub's binary and runs every action.
6. **L2: native executor.** For jobs whose steps are all in the generator's fixed list, a Rust executor runs the steps directly. Any other job falls back to L1 automatically. velnor-legacy rejected such jobs; that cliff is what forced it to add a second lane.
7. **Crash-only.** The node owns everything under its cgroup subtree and its storage root. On restart the agent kills that subtree, drops all per-job layers, fails the lost jobs through `completejob` and re-registers. No journal.

### 3.3 What to take and what not to copy

| Source | Take | Do not copy |
|---|---|---|
| velnor-new | `velnor-gen` and `velnor-ci` (plan, inputs audit, gate, cache telemetry); `velnor-runner-github` wire types (6.9k lines, no HTTP dependency [E]); JIT over a file descriptor, never env or argv; the job-authority check at job start; `velnor-local` (stand-in, cache server with GitHub's rules, recorder) as the conformance bench; the resource-probe parsers | The Turso journal and per-object ownership proofs in the cluster runtime (`journal/` 62.7k lines, `launch/` 47.6k, schema version 52 at `journal/schema.rs:71`): it protects foreign Docker objects on a Mac, and its tests are the largest CPU consumer in CI. One job per crate. `MBX_CACHE_DIR` under `RUNNER_TEMP`. State bundles through GitHub's cache |
| velnor-legacy | `protocol.rs` (broker, run service, OAuth, JIT decode, results; `:7021`, `:7256`, `:7409`, `:7430`); `scaleset/`; the expression engine; `job_message.rs`, `script_step.rs`, `workflow_command.rs`; `checkout.rs` with `git_mirror.rs`; `gha_cache.rs`. I built the crate (89.8 s) and ran these modules: 1,152 of 1,153 tests pass, and the one failure is my PATH [M] | Allowlist admission that rejects; Node sidecar containers; the 21.6k-line Docker socket proxy; 37k-line files; several daemons per host |
| Official runner | `Runner.Worker` as the compatibility executor; job hooks; the action archive cache; its behaviour as the oracle in differential tests | A new registration and listener process per job; the blank-filesystem assumption; remote-only cache and artifacts |
| ARC | Scale-set usage, acknowledge-after-handled, App auth with no token in the job, the idle-runner idea | Pod per job through CRDs; privileged dind sidecar with a 5 s probe and a 630 MB copy per pod [M]; request-based placement |

Lessons recorded in legacy's own code that the cache must respect: never restore a target with pinned modification times (`stable_workspace.rs:1-10`: "pinned mtimes let stale artifacts read as fresh"); give every store a budget checked on every use (one scope grew to 31 GiB, same file; a host carried 144 GiB of dead trees, `mbx_store.rs:14-19`).

Two documents in velnor-new point the same way and I would adjust both:

- **`docs/proposed/kubernetes-runtime.md` (PR #1324).** Its cache part is right: node-local volumes and placement by warmth. I would change its unit (it keeps one pod with dind per job) and not carry the journal over.
- **`docs/deferred/self-hosted-runner.md`, stage "V2-native".** L2 is that stage, with one difference: a job it cannot run falls back to the official Worker instead of failing admission. Its rule "one real native job must complete in GitHub before adding more features" should stay.

### 3.4 Size limits

I would set budgets before any code is written: dispatcher ≤ 8k lines, node agent ≤ 15k, native executor ≤ 12k, tests excluded. Both earlier runners grew past the point where an agent could hold them in view. A budget is the cheapest guard against a third time.

## 4. The cache system

All of it lives on one XFS filesystem per node (reflink is on by default), so "restore" is a reflink or an overlay mount, not a copy.

| Family | Contents | Key | Lives | Written by | Expected hit rate |
|---|---|---|---|---|---|
| Tools | Rust 1.99.1 with components and targets (593 MB installed [M]), mise tools, helper binaries | Digest of the pinned set | Node, read-only mount | Agent, verified once per node | 100% except on a version bump |
| Actions | Tarballs of the pinned actions | Commit SHA | Node, read-only | Agent | 100% |
| Crate sources | Registry index and `.crate` files | Crate name, version, checksum | Node; the existing nginx mirror fills it | Mirror | ≥ 99%; 10.5% of PRs touch a lockfile [M] |
| Git | Bare mirror per repository | Object id | Node | Agent fetch | 100%; fetch is a delta |
| Compile artifacts | mbx store: rustc, clippy, rustdoc and link outputs | mbx action key (content of inputs, path-normalised) | Node store; a cache server per site behind it | Trusted runs to the trusted scope; a PR to its own overlay | ≥ 95% of rustc invocations on a PR run, ≥ 99% on a merge_group run with no interleaved change [est: a PR changes 1–6 of about 30 workspace crates and none of about 445 dependencies] |
| Stable workspace | Checkout and target directory per slot, repository and trust scope | Path | Node | The slot | Same-SHA rebuild compiles nothing |
| Test verdicts | Pass record per test executable | Executable digest + test name + enforced read set + environment + sandbox image | Node, replicated to the site server | The agent, never job code | ≥ 90% of test binaries on a typical PR [est from the PR mix in section 5] |
| Run artifacts | Plan, reports, nextest archives | Run id and name | Node; GitHub copy uploaded in the background | Jobs of that run | – |
| Container images | Job image, test images | Digest | Node; k3s embedded registry mirror | Agent | 100% after first pull |

**Sizes.** A runner-workspace target is 5.9–7.1 GB [M], the root one 9.0 GB [M]. With profiles and a few lockfile generations I expect 30–60 GB per repository and trust scope per node [est]. The workers have 3.8–50 TB of XFS holding 5–8 GB today [M].

**Placement.** Phase 2 gives every node its own warm store; with one main repository and 5 nodes each node holds everything. From Phase 2b the dispatcher picks the node by warmth (the store's last-seen lockfile and main generation), then by free CPU, then by site. After each merge the built artifacts are published to the site server and other nodes prefetch them at idle priority (`mbx prefetch`).

**Trust.**

- *Trusted scope:* push to main, merge_group and schedule. Only these write the trusted store and promote a workspace.
- *PR scope:* a same-repository PR reads the trusted store and writes its own overlay, dropped when the PR closes. This is design rule 8 of `docs/goals.md`.
- *Fork PRs:* no cluster access at first; the workflow routes them to GitHub-hosted runners.
- *Organisations:* tailrocks, ChainArgos and jackin never share a store or a slot view.
- *Releases:* no shared cache, as today.
- *Verdict records* are written by the agent, which launches the test and reads its exit status. Job code cannot write one.

**Correctness.** Design rule 3 already demands that the cache proves itself. Three checks make that concrete:

1. Each run reports hits, misses and "should have been a hit"; the `Cache` check turns red on a miss the store should have served.
2. Each run re-executes a random 2% of cached verdicts and recompiles a 1% sample with `MBX_VERIFY=1`. A disagreement fails the run and disables that family for that scope.
3. The nightly full run uses no verdict cache at all and compares.

## 5. Workflow design for speed

### 5.1 Selection that is a proof

- Measured today [M]: `maintenance-helpers`, `velnor-actions-cli`, `-orchestrator`, `-workflow-renderer`, `velnor-runner-host` and `-cli` executed in 97.4% of 78 PR runs. PR #1312 changed only `ci-helper/crates/velnor-ci/tests/it/release_preflight.rs` and ran 12 jobs.
- The helper already audits compile-time reads from cargo's dep-info, and says what it cannot see: "Files that tests read at run time, and files a build script reads without naming them … are not seen here" (`ci-helper/crates/velnor-ci/src/cli.rs:214`).
- The runner can close that gap. A slot runs build scripts and tests under a deny-by-default file policy (Landlock on kernel 6.12; to verify on the nodes): the package's own directory, its declared inputs, the toolchain and the target directory. An undeclared read fails loudly. A recording mode writes the declarations. Then "this package reads nothing else" is enforced, and the planner may trust it.
- Network is off by default in the sandbox. A test group that needs Docker or the network declares it (design rule 6) and is never verdict-cached.
- With that, the verdict cache extends selection below the package: an unchanged test executable with unchanged allowed inputs is not re-run. The engine crates are "maintained, not developed", and 86% of PRs do not touch them [M], so their slow suites would be served from the cache.

### 5.2 What merged PRs actually touch

[M] changed files of the last 200 merged PRs.

| Class | Share | Needs |
|---|---|---|
| `generator/` and/or `ci-helper/` only | 47.0% | Two small roots |
| Runner workspace only | 25.0% (24% touch `velnor-runner-host`) | Runner root |
| Engine (root crates) only | 8.5% (14% touch it at all) | Root workspace |
| `local/` only | 7.5% | One small root |
| No Rust root | 3.5% | Lint only |
| Mixed | 8.5% | Several roots |

### 5.3 Job shape, sharding, parallelism

- Use the shape `velnor-gen` already emits: one plan job, one job per Cargo root, one required job. That removes 24 of 29 fixed overheads per run. When the single root workspace lands it becomes one job with package-level selection, and the double compile of `velnor-runner-host` disappears.
- Shard only where the arithmetic says so. A suite's wall is at least max(slowest single test, summed test time ÷ threads). For runner-host on 11-18 the first term was 534 s, so sharding could not have helped. After the fixes, the agent splits a suite across free slots with a nextest archive when summed time ÷ slot threads exceeds about 30 s, using recorded timings (design rule 5).
- Set a test time budget and enforce it in CI: no single test over 10 s, a suite's critical path at most 60 s. Today 62 tests exceed 120 s under load.
- `ci-helper` runs 781 tests on one thread in 55 s (`ci-helper/.config/nextest.toml:5`). With isolated CPUs it can run in parallel; I expect about 8 s on 8 threads [est], to be proven.

### 5.4 No double runs

- **Push to main after a queue merge:** skip. The SHA already carries a green `Required` from the merge_group run (120 of 159 merge_group runs [E]; 96 of 97 push runs since 11-16 [M]). Remove the `push` trigger from CI, or make it a no-op when that check exists. Saves about 62 runner-minutes per merge.
- **merge_group after the PR run:** GitHub requires both. Make the second one cheap. The PR run tests the merge ref. If no interleaved merge touched the same crates, every compile and every verdict in the merge_group run is a cache hit, so it finishes in well under a minute [est]. It is still a full verification: every result is either executed now or backed by an identical execution.
- **Let merge_group runs write the trusted store.** They build exactly the commit that becomes main. Today's rule that merge groups never write exists because of GitHub's cache scoping, which no longer applies to our own store. This needs the owner's decision.
- **Cancelled runs** (27% of PR runs per merged PR [M]) stop costing much once a run is 1–2 min.

### 5.5 The minimum honest time to verdict

[est] with the Phase 3 design and today's tests. Dispatch and GitHub latencies are about 10 s in total; plan 2 s; verdict 2 s.

| Class | Build | Tests | Total | Basis |
|---|---|---|---|---|
| generator / ci-helper (47%) | 10 s | 55 s serial `velnor-ci`; 1 s `velnor-gen` | **about 85 s**; about 45 s if `velnor-ci` tests run in parallel | Warm rebuild 8.1–8.8 s [M]; 626 tests in 1.0 s and 781 in 55 s [M logs] |
| runner (25%) | 60 s without incremental | 35 s after #1333, #1349 and optimised dependencies; 72–148 s today on a quiet node | **about 2 min**; 3–4 min until the fixes land | Host crate compiles in 35–99 s on the M5 depending on load [M]; test CPU −66% with optimised dependencies [M], −45% in #1349 |
| engine (8.5–14%) | 60–70 s | 240–324 s (`image_publication_public`), 258 s (`archive-guard`) in a second slot | **5–6 min** until those suites are fixed | [M logs] |
| local, lint only (11%) | under 10 s | under 5 s | **20–30 s** | [M logs: 225 tests in 1.9 s] |

Median PR: about 85 s. Mean over the mix: about 2 min [est: 0.47 × 85 + 0.11 × 25 + 0.25 × 120 + 0.14 × 340]. Push-to-merge adds one merge_group run of 30–60 s when nothing interleaves.

## 6. Kubernetes use

- **Role.** k3s stays as the fleet layer: it deploys and upgrades `velnor-node` (a privileged DaemonSet on the tainted build pool), the dispatcher on the control node, the mirrors and the site cache servers. It is not in the per-job path.
- **Pod lifecycle.** No pod per job. The agent pod is long-lived; job sandboxes are namespaces and cgroups it creates directly. Measured pod-per-job cost is 14 s per job start [M]. A later option is one unprivileged pod per slot with `hostUsers: false`, which gives nested sandboxes without host privilege.
- **Pre-warming.** Slots keep tools, registry, git mirror and stores mounted. After every merge to main, a low-priority warmer builds the base of each root on each node and publishes to the site server.
- **Node affinity.** Decided by the dispatcher after it has seen the job, not by kube-scheduler labels.
- **Both sites.** Add `topology.kubernetes.io/region` labels (none exist [E]). All jobs of one run stay in one site, because DE to FI is 35 ms over the public internet [D]. Each site has its own cache server; the two replicate in the background. The dispatcher prefers the site that is warm and spills to the other when the first has no free slot.
- **Isolation of untrusted PRs.** Three tiers. Same-repository PRs: namespaces, seccomp, no host mounts, PR-scoped overlay, network egress allow-list. Fork PRs: routed to GitHub-hosted runners by the generated `runs-on` expression until a microVM lane exists (velnor-legacy's Firecracker backend is the reference). Gate jobs: control node only. Note that today no worker is tainted and all five run privileged dind [E], so the "dedicated tainted nodes" decision is not yet in force.
- **Scaling.** Each EPYC has 48 cores in 8 L3 groups of 6 [M 48 physical cores; grouping from the CPU's 256 MB L3, to verify with `lscpu`]. I would reserve one group for the system and light jobs and offer the rest as elastic slots: a job owns its base cores and may borrow idle ones, which the agent takes back at the next dispatch. That gives about 15 heavy and 5 medium slots on the five workers. Expected demand after Phase 2 is about one busy slot on average at 20 runs per hour [est: 20 × 3 slot-minutes ÷ 60]. Other organisations get their own quota.
- **ARC stays installed** for ChainArgos and jackin until they move, and as a fallback for tailrocks, scaled to its minimum.

## 7. The role of mbx and blaze

**mbx is the compile layer.** It caches at the compiler boundary by content, including check, clippy, rustdoc and host links; it restores by reflink where the filesystem allows; it has a machine-wide scheduler for CPU and memory that containers sharing a cache directory also share; and it de-duplicates identical work in flight (mr-boxington.jdx.dev, "How it works" and "Parallel builds"). My measurement agrees: two concurrent cold builds cost 1.07× one [M]. What to change:

- Point `MBX_CACHE_DIR` at the node's trust-scoped store instead of `RUNNER_TEMP`. Today every job starts empty.
- Set the scheduler budget to the node (`scheduler.cpus`, `scheduler.memory`) and turn on test scheduling (`scheduler.tests`) so 96-thread test runs from several jobs stop colliding. Note that nextest's `slow-timeout` then counts queue time, so test scheduling cannot be used for the two suites with 2 s limits; they need isolated CPUs instead.
- Run the reference `mbx-cache` server once per site, behind the node stores. It authorises by OIDC claims (event and ref), which matches the trust rules in section 4.
- Keep mbx's CI default of no incremental compilation. Its "eager incremental" mode in a stable workspace is worth one experiment for PR feedback only (10.9 s against 82–99 s on the host crate [M]); the merge_group verdict should stay non-incremental.
- Risk: mbx moved from 1.12 to 1.29 in two months and is one vendor. Guard with the `MBX_VERIFY=1` sample and a pinned version per node.

**blaze does not fit the pinned toolchain.** Its documentation is explicit: "blaze is its own Rust toolchain. It builds every project with one toolchain that blaze controls … `blaze-2026-10-27` today"; it "ignores rust-toolchain, rust-toolchain.toml, rustup's settings and `RUSTUP_TOOLCHAIN`, and never runs or asks rustc" (`docs/toolchain.md`). Its borrow checker "finds about two thirds of rustc's borrow errors", it has no `clippy`, `doc` or `fmt`, and the README says "Keep `cargo check` or your CI as the final word". velnor-new pins 1.99.1 and promises checks "with the project's own compiler, features and settings". A blaze result therefore cannot count toward `Required`.

- Role now: none in the required path.
- Possible later role: an advisory early signal, or test ordering from its planned "tests selected by the code hashes they reached". Both shorten time-to-red, not time-to-green.
- Revisit when blaze can build with the project's toolchain or publishes equivalence evidence.

## 8a. Servers and cluster tuning

Current state, measured read-only: Debian 13, kernel 6.12.94, k3s 1.37.2, containerd 2.3.7 [E]; 48 physical cores, 96 threads, one socket, SMT on, 125.5 GiB, no swap [M cAdvisor]; kubelet with no reservations, CPU manager `none`, no memory eviction threshold [E]. Not observable without a shell on the node: governor, C-states, transparent hugepages, sysctls, MTU. For those I give the check to run first.

Ranked by gain per effort.

| # | Change | Evidence | Expected gain | Risk | Rollback |
|---|---|---|---|---|---|
| 1 | Bake the pinned action tarballs into the runner image and set `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`. Then put the Rust toolchain and the mise binary on the node, mounted read-only (this second half needs a small generator change, `state/RESULT.md:115`) | Action downloads 4.2 s, toolchain install 9.8 s, mise download 4.6 s per job [M] | 4 s per job at once, about 18 s with the toolchain; 9 runner-min per 31-job run | Stale tools after a pin bump: verify the digest at job start, as the workflow already does | Remove the env var and the mounts |
| 2 | Keep a warm pool: raise ARC `minRunners` from 4 until the agent replaces it | 14 s on demand against 3 s idle [M] | 11 s per job start; about 30 s per run on the chain | 45 MiB per idle pod [D] | Lower the value |
| 3 | Stop CPU oversubscription. First step: a CPU limit of about 32 on every runner pod, so cargo and nextest size themselves to it. Then two runner sizes with requests = limits. Then the agent's cpusets and capacity header | One pod at 82.6 cores on a 2-CPU request; job time 2× on busy days; CPU pressure 8.4%; 2 s test timeouts; 10% flake rate on verified commits [M, E] | Removes the 1.5–2.4× slowdown at peak and most timeout failures in the queue | A quota can throttle a job that starts more threads than its limit; fewer concurrent jobs if sized too coarsely | Revert the pod template |
| 4 | Build dependencies of the runner workspace at `opt-level = 3` without debug assertions in the test profile | Slowest-class test: 142.5 → 47.9 CPU-s, 1.03 × 10¹² → 2.67 × 10¹¹ instructions [M]. `turso_core` is already at 1 (`crates/velnor-runner/Cargo.toml:75`) | About −66% CPU in the largest suite | Dependencies compile slower once per lockfile (764 CPU-s in my rebuild, which included the host crate); it changes the project's profile | Remove the profile lines |
| 5 | Move job work directories onto the XFS data volume, same filesystem as the stores (kubelet root directory or a hostPath/local volume) | Work volumes of 6.3–21.7 GB sit on the OS disk, ext4 (`PLAN.md:45`), 68 GB used; the 3.8–50 TB XFS volume holds 5–8 GB [M] | Enables reflink restores and every node-local cache; no direct speed-up | Kubelet restart, node drain | Remount the old path |
| 6 | Remove `--jobs 2` and `build.jobs=2` from maintenance tasks once CPU is isolated | Four builds at 2 jobs: about 117 s, 67 s, 143 s, 54 s [M] | About −3 min on the current critical-path job [est] | Memory peak on hosted 4-CPU runners: keep the cap there | Restore the flag |
| 7 | Kubelet: reserve 2 cores and 8 GiB for system and kube, add `memory.available` eviction, set a pod PID limit | Allocatable equals capacity; memory limits overcommitted 3×; `podPidsLimit -1` [E] | Stability under saturation | Slightly less capacity | Revert kubelet args |
| 8 | Lower debuginfo for CI test builds (`line-tables-only` or off) in the project profile | `generator` workspace: 52.5 → 47.0 CPU-s and 870 → 507 MB with debuginfo off; 692 MB with line tables [M, one run each on a loaded machine] | About −10% compile CPU, −20 to −42% target bytes | Poorer backtraces with debuginfo off | Remove the profile line |
| 9 | CPU frequency policy: check `cpupower frequency-info`; if `amd-pstate` is in a power-saving preference, set performance | Not observable from here | 2–8% on bursty compiles [est from general experience, to measure] | Power and heat | Restore the preference |
| 10 | Transparent hugepages `always` for compilers (or `MALLOC_CONF=thp:always` for rustc) | Not observable from here | 2–6% compile CPU [est, to measure with an A/B on the runner workspace] | Compaction stalls, memory bloat | sysctl back |
| 11 | Image distribution: enable k3s's embedded registry mirror and a pull-through cache for dind | Images are imported by hand on each node [D] | Removes a manual step; faster Docker tests if they pull | Mirror staleness | Disable the flag |
| 12 | Stripe the data volume on velnor-240 and -105 | The LV is linear over 3–4 disks (`PLAN.md:76`) | Under 2% today: I/O pressure is 0.4% of uptime against 2.9% for CPU [M] | Rebuild of the volume | – |
| 13 | Raise inotify and file-descriptor limits; set `vm.dirty_*` so short-lived job files rarely hit disk | Not observable | Fewer flakes; small speed gain | More unsynced data on crash, harmless for scratch | sysctl back |

Decisions that need no change, with the reason:

- **SMT:** keep it on, and hand out whole cores so a job only shares a core with itself.
- **NUMA:** one socket, nothing to pin. Align slots to L3 groups instead.
- **Swap:** none and memory pressure is 80 s in three days; leave it.
- **tmpfs for targets:** no. A target is 6–22 GB and the node has 125 GiB; NVMe is not the limit.
- **Filesystem:** XFS with reflink plus overlayfs. btrfs or ZFS snapshots are convenient but weaker for many small files and fsync; ext4 has no reflink.
- **Linker:** the pinned toolchain ships `rust-lld` [M], the default linker on x86-64 Linux since Rust 1.90 (release notes; confirm with one verbose link line). mold or wild would save a few seconds per job at most.
- **sccache:** no. mbx covers it and a second wrapper conflicts.
- **`CARGO_BUILD_JOBS` and test threads:** set neither. Cargo and nextest read the CPUs the cgroup allows, so a cpuset or a limit sizes them. The mbx scheduler holds the machine-wide budget.
- **Memory per slot:** the heaviest job peaked at 16.8 GiB [E] and one compiler process at 7.6 GB on the M5 [M]. A 24-thread slot with a 32 GiB limit fits three per node with room to spare.
- **CPU manager `static`:** the right policy if slots become pods. With the agent managing cpusets itself, leave it `none`.
- **Priority classes:** add four: gate, trusted (main and merge_group), PR, warmers. Warmers are preemptible.
- **Network:** keep cache traffic on the node (`PreferSameNode`, as the crates mirror does). Keep GitHub's blob store off the hot path (7% of runner time today). MTU: leave flannel's default and verify there is no fragmentation between sites.
- **containerd snapshotter, image GC, flannel, local-path, overcommit, kernel 6.12:** defaults are fine; none is on the job path after Phase 2.

**What more hardware would buy.** Not more EPYC cores: after Phase 2 the cluster is mostly idle. The critical path is one rustc on one big crate and one slow test, so single-thread speed is what shortens it. Two or three high-clock desktop-class nodes would cut the critical path by roughly a third [est from typical single-thread benchmark ratios of about 1.6×; to confirm with one rented machine].

## 8. Phased plan

Metrics: *PR wall* is run created to `Required` completed, success runs, median and p90 over at least 50 runs. *Push-to-merge* comes from PR timelines. *Runner-minutes* is the sum of job durations. *Compile hit rate* is mbx hits ÷ (hits + misses + not looked up) over a run. *Flake rate* is the share of push runs that fail on a SHA with a green merge_group run. Baseline: PR wall 16–23 min on good days and 72 on 11-18; push-to-merge 39–402 min; 53–65 runner-min per run; 347 per merged PR; no compile result reused across jobs; flake rate 10.4%.

| Phase | What ships | Target at the end |
|---|---|---|
| **0. Week 1, configuration only** | Gate jobs off hosted runners (PR #1332, in flight) with reserved capacity; warm pool and action tarball cache (8a #1, #2); a CPU limit on runner pods (8a #3); skip the push re-run; merge #1333 and #1349 plus the profile line (8a #4); narrow `Source qualification` and `Job image candidate` triggers or move them to the cluster; owner raises `max_entries_to_build` from 2 to 5 | PR wall P50 ≤ 11 min and P90 ≤ 16 min on any day (floor today 10.9 [M]); push-to-merge P50 ≤ 30 min; flake rate ≤ 5%; runner-min per merged PR ≤ 280 |
| **1. Weeks 2–4, caches under the current runner** | `velnor-gen` workflow for this repository (5 root jobs); toolchain on the node; work directories and one mbx store per node and trust scope on XFS, or the `mbx-cache` server on each node; crates mirror wired; `check-*` work folded into root jobs; merge_group selects what the change selects | PR wall P50 ≤ 4 min, P90 ≤ 10 min; compile hit rate ≥ 90% on PR runs; ≤ 8 runner-min for a median PR run; push-to-merge P50 ≤ 12 min; flake rate ≤ 3% |
| **2. Weeks 4–8, Velnor dispatcher and node agent, official runner inside (L0)** | Slots with elastic cpusets; capacity header; gate lane on the control node; copy-on-write workspaces; local artifact and cache endpoints; crash-only recovery; then L1 for late binding, after a spike that runs one real job on `velnor-lab` through a Velnor listener and the official Worker | Job dispatch P50 ≤ 1.5 s; fixed overhead ≤ 8 s per job; PR wall P50 ≤ 2 min, P90 ≤ 7 min (≤ 4 if the slow suites meet the 60 s budget); merge_group wall P50 ≤ 90 s; push-to-merge P50 ≤ 6 min; ≤ 25 runner-min per merged PR |
| **3. Weeks 8–14, proofs and the native lane** | Enforced inputs in the sandbox; verdict cache with the 2% re-execution sample; elastic test sharding; native executor (L2) for generated jobs with automatic fallback | PR wall P50 ≤ 90 s (stretch 45 s); merge_group wall P50 ≤ 40 s; verdict hit rate ≥ 90% of test binaries; push-to-merge P50 ≤ 3 min, P90 ≤ 8 min; ≤ 12 runner-min per merged PR |
| **4. Later** | MicroVM lane for fork PRs; second-site standby dispatcher; other organisations onto Velnor; high-clock nodes if the experiment pays | Fork PRs on the cluster with no shared kernel; no single point of failure |

Each phase ends with one week of measurements posted on #1305 before the next begins. If a target is missed, the next phase does not start.

**Top risks.**

| Risk | How it is detected | How it is contained |
|---|---|---|
| GitHub changes the protocol or the runner version window (63 days [E]) | A canary job on `velnor-lab` every 5 minutes through each executor; differential runs against the official runner in `velnor-local` | Per-slot fallback L2 → L1 → L0, then ARC, then hosted. L0 uses only the scale-set API that ARC itself uses |
| A cache returns a wrong result | `Cache` check; 2% verdict re-execution and 1% `MBX_VERIFY` per run; nightly run with no verdict cache | Trust-scoped stores; a generation counter that invalidates a family at once; a family disables itself on the first disagreement |
| The sandbox misses an input | Enforcement, not observation: an undeclared read fails the test. Nightly full run | Docker and network groups are never cached; the package's own sources are always part of the key |
| PR code poisons trusted state | Trusted store hashes checked by the sample above | PR writes go to an overlay; verdicts are written only by the agent; gate jobs run on a node with no PR code |
| Contention flakes return | CPU pressure per slot and merge_group failure rate on a dashboard | Capacity header and cpusets; test budget rule |
| Control node is a single point of failure | Dispatcher heartbeat | Renewals are done by the node that runs the job, so running jobs survive; standby dispatcher in the other site in Phase 4 |
| The new runner grows like the last two | Line budgets in CI (section 3.4) | Crash-only design, no journal, one protocol version pinned per release |
| Privileged dind lets a job own the node | – | Taint the build pool now; unprivileged slots with user namespaces in Phase 2; microVM for forks |

**Comments on what is already decided.**

- *One root workspace.* Good for the cache: one lockfile, one store generation, and `velnor-runner-host` is no longer compiled under two lockfiles. It needs package-level selection inside one job, and the generator's "check alone" steps keep catching feature unification.
- *Release assets with attestations.* Keep release builds off shared caches, on GitHub-hosted runners or a no-cache slot.
- *Org-level GitHub App.* The dispatcher needs organisation self-hosted-runner write and Actions read. Add Actions write only if you want infrastructure failures re-run automatically.
- *Privileged dind on dedicated tainted nodes.* Not in force yet (no worker is tainted). In the slot design a per-slot Docker daemon persists, so test images stay warm.
- *velnor-actions v0.1.81.* The right first step. Give the gate jobs their own small scale set or reserved idle runners; otherwise `Required` can queue behind 31 fan-out pods per run at `maxRunners` 60.

## Owner questions

1. **`docs/goals.md:16`** says jobs are always executed by GitHub's official runner. L0 keeps that. L1 keeps the official Worker but replaces the listener. L2 does not keep it. Which of L1 and L2 do you allow?
2. **`docs/goals.md`, "Inside GitHub's free limits".** I assume the generated workflow must stay correct and within the limits on hosted runners, and that everything in this proposal is an addition that switches on only when a Velnor node is present. Is that the intended reading?
3. **Verdict cache.** May a PR or merge_group run count a test as passed because the identical executable with identical enforced inputs passed earlier on this cluster, with the 2% sample and the nightly full run as audit? Without this, the minimum for engine and runner PRs stays bounded by their suites.
4. **Merge-group trust.** May merge_group runs write the trusted cache? Today's cache decision says merge groups never write.
5. **Project profile.** May the runner workspace's test profile optimise dependencies and lower debuginfo? It changes "the project's settings", so it is your call, not a CI flag.
6. **Merge queue and triggers.** Will you raise `max_entries_to_build` to 5, and drop CI's `push` trigger on main (or let it become a cached no-op)?
7. **Hosted pool.** `Source qualification` uses about 2,400 hosted-seconds per run on every PR event, and `Job image candidate` runs on every branch push. Move them to the cluster, or narrow their triggers?
8. **PR #1324.** It carries the journal and ownership proofs to Kubernetes as "one runner, two runtimes". I recommend a crash-only node agent for the cluster and keeping the journal for the Mac path only. Do you accept two runtimes with different recovery models?
9. **Other organisations.** ChainArgos (15 private repositories) and jackin share these nodes. Separate stores and quotas are in the design; do you want them on Velnor at all, or left on ARC?
10. **Fork PRs.** Route them to GitHub-hosted runners until a microVM lane exists?
11. **Test time budget.** Enforce "no test over 10 s, suite critical path under 60 s" as a required check? It is the only way the P90 targets hold.
12. **Hardware.** May I propose a one-month rental of one high-clock node to measure the critical-path gain before any purchase?
13. **blaze.** Ruled out for required checks. Do you want an advisory experiment on `velnor-lab`, or nothing?

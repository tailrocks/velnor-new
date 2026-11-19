# Proposal by Gemini 4 Argon

| Field | Value |
|---|---|
| Model | Gemini 4 Argon |
| Model id | `gemini-4-argon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high (model maximum; max rejected) |
| Date / run | 2026-11-18 17:13–17:54 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

Your goal is to eliminate CI/CD waiting as the bottleneck for AI-driven development on `tailrocks/velnor-new`, cutting push-to-merge latency from 26.7 to 77.8 minutes down to under 2.5 minutes at the same verification confidence.
The problem is not JavaScript action interpretation or `.NET` listener startup, which together take 4 to 6 seconds per job (measured, `evidence.md` T9b).
Instead, four bottlenecks dominate:
- 79% of 2026-11-18 PR wall time (57.0 of 71.8 minutes, `evidence.md` T5) is queue wait on 8 GitHub-hosted gate jobs.
- 100% of scale-set Cargo source and state caches miss due to three path, `ImageOS`, and `event.json` bugs we traced in `.github/workflows/ci.yml:329-715` and `actions/runner`.
- All pod `emptyDir` scratch lands on the unstriped 3.5 TB OS disk instead of the 50 TB XFS volume (`kubectl` stats summary).
- Every merge runs the full 39-job suite three times (`evidence.md` T7).

The smallest design that reaches the physical floor replaces ARC with `velnor-host`'s Rust Kubernetes runtime and per-node warm slot agent, keeps the unmodified official `Runner.Listener` and `Runner.Worker` inside pre-warmed pods (`docs/goals.md:16`), mounts node-local XFS reflinked `mbx` and Cargo caches backed by a per-site `mbx-cache` daemon, consolidates the repository into one root workspace (`generator/` and `velnor-ci` model) with sharded `cargo-nextest` partitions, and skips duplicate `push`-to-`main` verification when the commit SHA matches a green `merge_group` run.
`throughput checkpoint: n/a, read-only investigation`.

## 1. Where the time goes today

Every number below comes from the 2026-11-04 to 2026-11-18 dataset in `evidence.md` Section A (3,009 population CI runs in T1, 200 sampled runs via the jobs and steps APIs in T2 through T10, and 68 parsed job logs in T9a), supplemented by our direct inspection of live jobs (`gh api` on run `44354317267` and job `134981831502`) and live cluster nodes (`kubectl describe nodes` and `/api/v1/nodes/velnor-240/proxy/stats/summary`).

### Wall-clock breakdown of a pull request run

In the scale-set era (2026-11-15 to 2026-11-18, `n=77` sampled PR runs, `evidence.md` T2), median PR wall time is 26.7 minutes and p90 is 92.3 minutes. Because GitHub-hosted runner contention spiked on 2026-11-18, the data splits into two operating regimes (`evidence.md` T5):

| Stage on the critical path | Normal scale-set days (11-16 / 11-17 med, `n=36` success PRs, `evidence.md` T5) | Contended day (11-18 med, `n=13` success PRs, `evidence.md` T5) | Share of 11-18 wall time (inferred from T5) | Where it runs today (`.github/workflows/ci.yml`) |
|---|---:|---:|---:|---|
| 1. `Plan` queue wait | 5.9 min / 2.3 min | 21.6 min | 30.1% | GitHub-hosted `ubuntu-26.04` (`ci.yml:369`) |
| 2. `Plan` execution | 0.9 min | 0.9 min | 1.3% | GitHub-hosted `ubuntu-26.04` (`evidence.md` T3) |
| 3. Scale-set fan-out span (`Plan` end to last `Rust`/`Check` job end) | 8.4 min / 9.3 min | 10.7 min | 14.9% | Hetzner scale set `[velnor, ubuntu-26.04-scale-set]` (`evidence.md` T5) |
| 4. `Required verdict` queue wait | 1.9 min / 0.7 min | 23.3 min | 32.5% | GitHub-hosted `ubuntu-26.04` (`ci.yml:516`) |
| 5. `Required verdict` execution | 0.7 min | 0.7 min | 1.0% | GitHub-hosted `ubuntu-26.04` (`evidence.md` T4) |
| 6. `Required` gate queue wait | 2.1 min / 1.2 min | 15.3 min | 21.3% | GitHub-hosted `ubuntu-26.04` (`ci.yml:616`) |
| 7. `Required` gate execution | 0.1 min | 0.1 min | 0.1% | GitHub-hosted `ubuntu-26.04` (`evidence.md` T4) |
| **Total PR run wall time (median)** | **23.2 min / 16.0 min** | **71.8 min** | **100.0%** | Sum of 3 hosted queues: 12.6 min (11-16), 5.8 min (11-17), 57.0 min (11-18) |

Three facts stand out from this breakdown:

1. **Hosted queue wait dominates wall time.**
   On 2026-11-18, 57.0 of 71.8 median PR wall minutes (79.4%, `evidence.md` T5) were spent waiting for GitHub-hosted `ubuntu-26.04` runners to start `Plan`, `Required verdict`, and `Required`.
   At 2026-11-18T16:54Z, 86 jobs were queued on `ubuntu-26.04` (61 from `CI`, 23 from `Source qualification`, 1 from `Job image candidate`, and 1 from `generator release`), while 0 jobs were queued on the Hetzner scale set (`evidence.md` T11).
   On that day the repository created 348 `CI` runs, 306 `Job image candidate` runs (triggered on every branch push because `job-image-candidate.yml` has `push: {}`, `evidence.md:399, 629`), and 267 `Source qualification` runs (`evidence.md:399`), saturating the organization's GitHub-hosted concurrency quota.
2. **Scale-set queue wait is near zero, but pod startup costs 12 to 18 seconds.**
   Across 3,944 scale-set jobs, queue wait (`created_at` to `started_at`) is 0.2 minutes (12 seconds) at the median and 0.3 minutes (18 seconds) at p90 (`evidence.md` T4).
   Because ARC keeps only `minRunners: 4` idle pods for `tailrocks` (`evidence.md:542`) and a full run fans out 31 scale-set jobs at once, 27 pods must be cold-created per run through ARC's 8-step scale-up chain (`evidence.md:1404-1412`), waiting for the privileged `docker:dind` sidecar to boot (57 `Unhealthy` startup-probe events logged in one hour, `evidence.md:600-603`).
3. **Once `Plan` finishes, the fan-out wall time (8.4 to 10.7 minutes) is bounded by the four slowest scale-set jobs.**
   Those jobs are `Check / maintenance-helpers` (8.5 min median, 9.6 min on 11-18), `Rust / velnor-runner-host (local-mode)` (6.5 min median, 8.9 min on 11-18), `Rust / velnor-actions-cli` (6.5 min median, 6.9 min on 11-18), and `Rust / velnor-runner-host` (5.5 min median, 8.8 min on 11-18) (`evidence.md` T3, T6).

### Runner-minutes breakdown inside jobs

A full run executes 39 jobs and consumes 52.9 median runner-minutes on `pull_request`, 64.6 on `merge_group`, and 62.0 on `push` (`evidence.md` T2).
The top 6 scale-set jobs account for 32.8 of the 52.9 PR runner-minutes (62.0%, inferred by summing T3 `runner-min/run`).
Breaking down the step durations across all sampled scale-set jobs (`evidence.md` T9a, T9b, T10) shows where execution time actually goes:

1. **JavaScript action wrappers (`~12 s` per job, `~6.2` runner-min across 31 jobs, of which `~2.1` runner-min is pure JS runtime overhead):**
   - `actions/checkout@v7`: 6 s median (p90 8 s, `evidence.md` T10), of which ~4.5 to 5 s is native `/usr/bin/git fetch` over HTTPS.
   - `jdx/mise-action@v5`: 2 s median (p90 5 s, `evidence.md` T9b).
   - `actions/cache/restore@v6`: 1 s median on miss, 2 s on hit (`evidence.md` T9b).
   - `jdx/mr-boxington-action@v1`: 1 s median (p90 2 s, `evidence.md` T9b).
   - `actions/download-artifact@v8` and `actions/upload-artifact@v7`: ~2 s combined.
   - `Post Set up MBX` and `Post Setup Mise`: 0 s median (p90 1 s, `evidence.md` T9b).
   - The official runner `v2.338.0` bundles `node24` (`NodeUtil.cs:7`, `evidence.md:1344`), and the JS actions are thin wrappers around native binaries (`git`, `mise`, `mbx`, `zstd`).
2. **Toolchain download and verification (`13 s` median / `22 s` p90 per job, `~6.7` runner-min per run, 100% miss rate in 58/58 logs):**
   - `Prepare pinned tools` takes 10 s median (p90 18 s, 706 runner-minutes across the sample in `evidence.md` T9b), `Prepare Rust components` takes 1 s (p90 2 s), and `Verify restored Rust toolchain` + `Verify Rust channel manifest` take 2 s.
   - Root cause: `ghcr.io/tailrocks/velnor-ci-runner:2026-11-16.2` omits Rust 1.99.1 (`RESULT.md:115`). `.github/workflows/ci.yml:315` sets `RUSTUP_HOME=${{ runner.temp }}/velnor/rustup` (inside an ephemeral `emptyDir`), and `.github/scripts/velnor-tools-cache-identity.sh:2` explicitly disables the tools cache on `scale-set:*` (`case "$VELNOR_CACHE_LANE" in scale-set:*) disable scale_set_image_unqualified ;; esac`). Every scale-set job downloads and verifies Rust 1.99.1, `clippy`, and `rustfmt` from `static.rust-lang.org`.
3. **Cargo crate downloads (`4 to 8 s` per job, `~2.5` runner-min per run, 435 to 522 crates downloaded per job, 0/48 cache hits on scale set):**
   - **Bug 1 traced in `.github/workflows/ci.yml:329-339, 667-673`:** `sources-producer` (the State seal workflow 304`) and `Plan` (`ci.yml:369, 463`) run on GitHub-hosted `ubuntu-26.04`, where `$RUNNER_TEMP` is `/home/runner/work/_temp`. Scale-set pods (`ci.yml:628, 672`) use `workFolder: _work`, where `$RUNNER_TEMP` is `/home/runner/_work/_temp`.
   - GitHub's `@actions/cache` (`actions/cache/restore@v6`) scopes every cache lookup by `(key, version)` where `version` is the SHA-256 of the resolved `path` strings plus compression method. Because `/home/runner/_work/_temp/velnor/cargo/...` differs by one character from `/home/runner/work/_temp/velnor/cargo/...`, the `version` hash differs and `Restore Cargo sources` misses in 48 of 48 scale-set logs while `Plan` hits the exact same `sources_key` in 10 of 10 logs (`evidence.md` T9a).
   - In addition, the in-cluster `velnor-system/crates-mirror` DaemonSet (`evidence.md:575-576`) is unused by `velnor-actions` 0.1.80 workflows.
4. **Compiled state cache transfer (`0 s`, skipped in 3,636 of 3,636 scale-set jobs, `evidence.md` T9b):**
   - **Bug 2 traced in `.github/workflows/ci.yml:445, 707`:** `Compute state snapshot key` hashes `ImageOS=${ImageOS-}`, `store=$RUNNER_TEMP/velnor/mbx`, and `checkout=$GITHUB_WORKSPACE` into `compat`. In the State seal workflow (`runs-on: ubuntu-26.04`), `ImageOS` is `ubuntu24`, `store` is `/home/runner/work/_temp/velnor/mbx`, and `checkout` is `/home/runner/work/velnor-new/velnor-new`. On scale-set pods (`ci.yml`), `ImageOS` is `velnor-arc-ubuntu24` (`evidence.md:552`), `store` is `/home/runner/_work/_temp/velnor/mbx`, and `checkout` is `/home/runner/_work/velnor-new/velnor-new`. Therefore, the scale-set pod's `compat` digest never matches any attested key in `VELNOR_STATE_ATTESTED`, leaving `restore_key` empty and skipping `Restore state snapshot` in 3,636 of 3,636 jobs (even for `ci-helper` and `generator`, which have attested cache entries on `main`, `evidence.md` T9c).
   - **Bug 3 traced in the State seal workflow job `134981831502` and `actions/runner`:** `State seal` never sealed `root` or `crates-velnor-runner` (0/18 runs in `evidence.md` T9d). Seven of 16 `State producer` failures exited at `Check state tip` because `main` advanced while queued, and **all 9 remaining failures crashed at `Clippy` / `Wait for parallel checks` with exit code 125 (`velnor-actions: internal request failed: internal: producer_event_invalid`, measured from job `134981831502` log at `2026-11-18T17:14:22.496Z`)**.
   - Inspecting `crates/velnor-actions-orchestrator/src/task_producer.rs:99-106` and `actions/runner/src/Runner.Worker/ActionRunner.cs:141` (`ExecutionContext.cs:1333-1335`) reveals why: `ActionRunner.RunAsync` calls `File.WriteAllText(workflowFile, gitHubEvent)` on `_temp/_github_workflow/event.json` at the start of every action step, truncating `event.json` to 0 bytes in-place at the exact millisecond a concurrent `background: true` step (`declared-task-0`) reads `GITHUB_EVENT_PATH`!
5. **Cold Rust compilation (`13 s to 89 s` per `rust-*` job, `~240 s` in `maintenance-helpers`, `~22.0` runner-min per run):**
   - Because every pod starts with an empty `$RUNNER_TEMP/velnor/mbx`, `MBX_REMOTE_URL` is empty in 68 of 68 logs (`evidence.md` T9a), and state restore skips in 3,636 of 3,636 jobs, every job compiles its 104 to 497 crates cold (`evidence.md` T9a). `mbx` only achieves intra-job hits between the sequential build lane and the parallel `clippy`/`doc` lanes inside the same pod (40% to 74% hit share, `evidence.md` T9a).
   - In `Check / maintenance-helpers` (`mise.toml:90-193`), 6 separate `cargo` invocations compile 104 crates in both debug and `--release` modes with `--config build.jobs=2` (using only 2 of 96 threads on an EPYC 9454P) and no `mbx` or cache.
6. **Test execution (`10 s to 246 s` median, up to `497 s` p90, `~15.5` runner-min per run):**
   - Concentrated in `velnor-runner-host (local-mode)` (246 s median, 497 s p90), `velnor-actions-cli` (238 s median, 284 s p90), `velnor-runner-host` (192 s median, 417 s p90), and `velnor-actions-orchestrator` (78 s median, 104 s p90) (`evidence.md` T10).
   - As PR `#1333` and PR `#1349` measured (`gh pr view 1333` and `1349`), ~80% of CPU in `velnor-runner-host`'s slowest restart, retirement, and JIT-conflict tests was spent quadratically validating and replaying 47 historical Turso SQLite schema migrations per test process.
   - In addition, `velnor-runner-host` and `velnor-runner-host (local-mode)` execute the same 430 base tests twice on every run, and nextest sharding is unused (`default_shards = 1` in `.velnor/config.toml:30`).

### Duplicate work across the merge lifecycle

Applying **Principle: Explain the Number**, the cost of landing one PR is three full 39-job runs (`evidence.md` T7):
- `pull_request` runs all 39 jobs (2,558 runs across 454 branches, median 3.0 runs per branch, 45% cancelled).
- `merge_group` runs all 39 jobs again (193 runs across 150 PRs).
- `push` to `main` runs all 39 jobs a third time: in 120 of 159 successful `merge_group` runs (75.5%, `evidence.md:207`), the `push` run executes on the **identical commit SHA** that `merge_group` just verified, wasting 62.0 median runner-minutes per merge (`evidence.md` T2).

## 2. Verdict on the owner's hypotheses

Applying **Principle: Attack the Premise** and **Principle: Explain the Number**, we test each hypothesis against measured step durations and Amdahl's law limits (`benchmark-checklist` Question 3: removing a step that takes $p$ of the run can speed up the run by at most $p$).

### Hypothesis 1: "Building everything in Rust, including the runner itself and native Rust versions of the actions we use instead of JavaScript actions, will speed up CI dramatically, because native code beats interpreted code."

- **Verdict: Fails** (for interpreter speed); **Partly holds only when an action's storage/network path is changed** (locality, not language CPU speed).
- **Evidence and expected numbers:**
  - Across all scale-set jobs in `evidence.md` T9b and T10, the JavaScript action steps (`actions/checkout@v7` 6 s, `jdx/mise-action@v5` 2 s, `actions/cache/restore@v6` 1 s, `jdx/mr-boxington-action@v1` 1 s, `actions/download-artifact@v8` + `actions/upload-artifact@v7` ~2 s, and post steps 0 to 1 s) take **10 to 13 seconds median** out of a 332 to 508 second critical-path job.
  - Of those 10 to 13 seconds, ~4.5 to 5 s is native `/usr/bin/git fetch` network I/O inside `actions/checkout` and ~1.5 s is artifact HTTP transfer. Actual Node 24 (`NODE24_VERSION="24.22.3"`, `actions/runner/src/Misc/externals.sh:9`) JavaScript startup and execution across all 6 action steps is **3 to 5 seconds per job** (estimated by subtracting native git/HTTP subprocess I/O from `T9b` step medians).
  - By Amdahl's law, cutting 3 to 5 seconds of Node 24 CPU to 0 seconds speeds up a 382-second job by **3 to 5 seconds (1.0% to 1.3% of job time, and <0.2% of a 71.8-minute PR run)**.
  - Inspecting `velnor-legacy`'s 26 native Rust action adapters (`velnor-legacy/crates/velnor-runner/src/action.rs:378-408`, `checkout.rs:47-52`, `git_mirror.rs:1-100`, `gha_cache.rs:1-60`) confirms that the adapters only saved time where they replaced remote GitHub network transfers with host-local git mirrors and host-local disk caches. `velnor-legacy`'s own performance ledger (`plans/ci-performance/README.md:3`, `evidence.md:1099`) recorded zero accepted CI speedup from its native runner and adapters.
  - You capture the full 5 s git checkout savings and 60 to 100 s cache restore savings without rewriting JS actions by pre-seeding `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (`ActionManager.cs:1236-1290`), mounting a node-local git reference mirror, and running a node/site-local cache server.

### Hypothesis 2: "A from-scratch, Rust-only, GitHub-compatible runner beats reusing or wrapping GitHub's official runner and ARC."

- **Verdict: Partly holds.** It **holds strongly for replacing ARC** with Velnor's own Rust Scale-Set Controller and Node Agent (`velnor-host` + `KubernetesRuntime`), and **fails for replacing GitHub's official in-pod `Runner.Listener` / `Runner.Worker` binary** with a from-scratch Rust step and expression engine.
- **Evidence and expected numbers:**
  - **Why replacing ARC holds (saves 10 to 16 s of pod startup + 80 to 150 s of cold compile per job):**
    1. ARC (`actions/actions-runner-controller` 0.15.1) creates one throwaway Pod per job with only `emptyDir: {}` volumes and a cold privileged `docker:dind` sidecar (`evidence.md:556-557`), destroying all compiled artifacts, toolchains, and Docker layers on job completion.
    2. ARC's reactive 8-step scale-up path (`ghalistener` poll -> `AcquireJobs` -> patch `EphemeralRunnerSet` -> reconciler batch-8 creates `EphemeralRunner` -> `GenerateJitRunnerConfig` -> create `Secret` -> schedule `Pod` -> boot `dind` + `Runner.Listener`, `evidence.md:1404-1412`) costs **12 s median / 18 s p90 queue-to-start latency** (`evidence.md` T4) and caused 57 `dind` startup-probe failures in 1 hour (`evidence.md:600-603`).
    3. ARC has no awareness of cache warmth, trust scopes, or node affinity.
    Replacing ARC with `velnor-host` (`crates/velnor-runner-*` extended with `KubernetesRuntime` per PR `#1324`, `docs/proposed/kubernetes-runtime.md`) lets Velnor keep a pre-warmed standby pool of pods with ready `dind` engines and route jobs to warm node slots, cutting pickup time from **12 to 18 s down to <1.5 s** (estimated from warm pod standby vs cold pod + `dind` init).
  - **Why replacing the official in-pod `Runner.Worker` fails (saves <0.5 s per job at severe reliability cost):**
    1. On a warm pod, the official `.NET` `Runner.Listener` + `Runner.Worker` startup path between receiving `RunnerJobRequest` and running Step 1 consists of two mandatory network calls (`/acquirejob` and `/renewjob`) plus spawning `Runner.Worker` over an anonymous pipe (~100 to 200 ms of `.NET` process startup, `JobDispatcher.cs:380-480`, `evidence.md:1361-1370`). With `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` pre-seeded in the image, action download time is 0 ms (`ActionManager.cs:1236-1290`). Replacing `.NET` with Rust inside the pod saves **<0.5 seconds per job**.
    2. `velnor-legacy` proved the maintenance cost of a custom Rust `Runner.Worker`: 337,384 lines of Rust (`executor.rs` 37,028 LOC, `runner.rs` 36,759 LOC, `protocol.rs` 18,419 LOC, `evidence.md:978-992`), 1,070 commits in September 2026 with 60 of the last 100 commits being bug fixes (`evidence.md:1092`), severe incidents in Twirp `ResultsService` log ownership on reruns (`#670`), lost artifacts (`#311`, `#671`), broken failed-job log uploads (`#672`), 600 to 700% idle CPU spin loops (`#408`), and incompatibility with non-allowlisted actions (`evidence.md:1089-1091`). Ultimately `velnor-legacy` had to add `crates/velnor-runner/src/scaleset/worker/runner.rs:694` to delegate jobs back to the unmodified official runner (`evidence.md:1040`).
    3. GitHub enforces a 63-day runner version deprecation cliff (`AccessDeniedException(ErrorCode==1)`, `Runner.Listener/Program.cs:142-167`, `evidence.md:1385`) and runs dual template evaluators (`EvaluateAndCompare`, `PipelineTemplateEvaluatorWrapper.cs:63-75`, `evidence.md:1328`). Keeping the unmodified official runner binary inside the pod preserves 100% compatibility and obeys `docs/goals.md:16`.

### Hypothesis 3: "Kubernetes is the right base: it schedules and scales pods, so it lets us use all of our hardware in parallel."

- **Verdict: Holds** (when paired with kubelet CPU/storage tuning and cache-affinity placement; default k3s settings currently degrade build times under load).
- **Evidence and expected numbers:**
  - Across the 5 EPYC 9454/9454P workers (480 threads, 627.5 GiB RAM, 96 TB NVMe across DE and FI, `evidence.md:439-447`), Kubernetes runs 40+ concurrent pods (`evidence.md:564`), driving active worker CPU to **91% to 95%** (`evidence.md:505-512`) and keeping scale-set queue wait at **0.2 min median** (`evidence.md` T4).
  - However, un-tuned k3s and ARC defaults cause **1.2x to 2.4x job duration inflation under load** (`evidence.md` T6: `velnor-runner-host` median duration jumps from 3.7 min on 11-16 to 8.8 min on 11-18; `local-mode` from 5.9 min to 8.9 min) because pods have no CPU limit (`evidence.md:554`), `cpuManagerPolicy` is `none` (`evidence.md:474`), one pod grabs 83.7 of 96 cores (`evidence.md:517`), and `/var/lib/kubelet` writes all pod `emptyDir` scratch to the single 3.5 TB OS disk (`nodefs`) while the 50 TB XFS volume (`imagefs`) sits 99.98% empty (`evidence.md:481-492`).

### Hypothesis 4: "Local caches on hardware we control beat remote caches. A runner that ships a proper caching system built in, where the cache layout knows which server a job runs on, is a big win."

- **Verdict: Holds strongly.** This is the largest build-time lever once the hosted queue wait is removed.
- **Evidence and expected numbers:**
  - GitHub Actions remote cache restore from cluster pods was measured at **13.6 to 22.4 MB/s with 120 ms RTT** to GitHub's US Azure Blob store (`velnor-runners/state/log.md:31`, `evidence.md:577`), taking **65 to 107 seconds** to download a 1.457 GB state bundle (`evidence.md` T9c) under a strict 10 GB repo limit.
  - Because of the three cache bugs we identified in Section 1, cross-job remote cache hit rate on scale-set pods today is **0% for Cargo sources (0/48), 0% for toolchains (0/58), and 0% for `mbx` state snapshots (3,636/3,636 skipped)**.
  - In contrast, a warm local `mbx` store with `SOURCE_DATE_EPOCH=0` compiles `velnor-runner-cli` in **15.5 s warm vs 72.5 s cold (4.7x faster, 1,062 hits / 2 misses)** and hits **98.2% to 98.3% of units on leaf changes and 94.6% on core-crate changes** (`docs/proposed/compiled-state-persistence.md:135-149`). Mounting a node-local XFS reflinked cache slot transfers **0 bytes over the network** and restores in **<1 second**, saving **60 to 110 seconds per heavy job**.

### Hypothesis 5: "Workflows can be written for maximum speed: skip what a change doesn't touch, and run only and exactly the verification it needs, as fast as possible."

- **Verdict: Holds strongly.** Workflow topology, change selection, workspace consolidation, test sharding, and deduplicating triple runs reduce wall time and runner-minutes by **5x to 15x**.
- **Evidence and expected numbers:**
  - Moving the 8 hosted gate/lint jobs onto the cluster saves **5.8 to 57.0 minutes of queue wait per PR** (`evidence.md` T5).
  - Skipping the duplicate `push`-to-`main` verification when `GITHUB_SHA` matches a green `merge_group` run saves **62.0 median runner-minutes on 75.5% of merges** (`evidence.md` T2, T7).
  - Deduplicating `velnor-runner-host` and `velnor-runner-host (local-mode)` test runs saves **6.23 runner-minutes per run** (`evidence.md` T3).
  - Landing PRs `#1333` and `#1349` plus sharding the 4 heavy test suites across 4 parallel nextest partitions (`--partition hash:i/4`, `crates/velnor-actions-rust/src/argv.rs:158-159`) cuts the test step wall time from **246 s median (497 s p90) down to ~35 to 45 s** (estimated arithmetic: $246\text{ s} \times 0.55 / 4 = 33.8\text{ s}$ plus ~10 s overhead).

## 3. Recommended runner architecture

Applying **Principle: Redesign from First Principles** and **Principle: Foundational Thinking**, we draw a strict architectural boundary: **keep GitHub's official runner binary inside the job pod as the protocol and step execution engine (`docs/goals.md:16`), and replace ARC with Velnor's own Rust Scale-Set Controller (`velnor-host`) and per-node Rust Slot Agent (`velnor-node-agent`) to own pod pre-warming, cache-affinity placement, node-local persistent storage slots, and journaled cleanup proofs.**

```
+-----------------------------------------------------------------------------------+
| GitHub Actions Service (Scale Set API v6.0-preview, Broker, Run Service, Results) |
+-----------------------------------------+-----------------------------------------+
                                          | Long-poll session, AcquireJobs, GenerateJitConfig
                                          v
+-----------------------------------------------------------------------------------+
| velnor-host (Rust Controller on k3s control-plane or dedicated Deployment)        |
| - velnor-runner-github: multi-org Scale Set sessions (tailrocks, ChainArgos, etc) |
| - velnor-runner-core: WorkerState machine, CleanupProof, TrustClass admission     |
| - Turso embedded SQLite journal (linear schema check #1333, UID ownership proofs) |
| - Cache-Affinity Scheduler: maps (repo, workspace_root, trust_scope) -> warm node |
| - Standby Pool Manager: keeps M pre-warmed Pod pairs (Runner + DinD) per node     |
+--------------------+-----------------------------------------+--------------------+
                     | k8s API (Pod bind, JIT Secret delivery) |
        +------------+------------+               +------------+------------+
        v                         v               v                         v
+-------------------------------+   +-------------------------------+
| Worker Node (DE site, EPYC)   |   | Worker Node (FI site, EPYC)   |
| /var/lib/rancher (XFS reflink)|   | /var/lib/rancher (XFS reflink)|
|                               |   |                               |
| +---------------------------+ |   | +---------------------------+ |
| | velnor-node-agent (Rust)  | |   | | velnor-node-agent (Rust)  | |
| | - Slot Lease Ledger (K=6) | |   | | - Slot Lease Ledger (K=6) | |
| | - Local Git Bare Mirror   | |   | | - Local Git Bare Mirror   | |
| | - Local gha_cache server  | |   | | - Local gha_cache server  | |
| | - crates-mirror DaemonSet | |   | | - crates-mirror DaemonSet | |
| +-------------+-------------+ |   | +-------------+-------------+ |
|               | bind mounts   |   |               | bind mounts   |
| +-------------v-------------+ |   | +-------------v-------------+ |
| | Warm Job Pod (Slot s)     | |   | | Site L2: mbx-cache server | |
| | - Official Runner v2.338  | |   | | (DE on velnor-240 50TB,   | |
| | - Pre-booted DinD sidecar | |   | |  FI on velnor-241 3.8TB)  | |
| | - Pre-baked Rust 1.99.1   | |   | +---------------------------+ |
| | - Pre-seeded Action Cache | |   +-------------------------------+
| +---------------------------+ |
+-------------------------------+
```

### What stays compatible with GitHub from the outside

1. **Workflow YAML and Check Runs API:** Repositories keep standard `.github/workflows/*.yml` generated by `velnor-gen` (and `velnor-actions` during migration), standard `runs-on: [velnor, ubuntu-26.04-scale-set]` labels, standard `Required` status checks, and GitHub's merge queue.
2. **Scale-Set Control Protocol:** `velnor-runner-github` (`crates/velnor-runner-github/src/`) speaks GitHub's `_apis/runtime/runnerscalesets` (`api-version=6.0-preview`) protocol: message session creation, 2-minute long-poll with `X-ScaleSetMaxCapacity`, `acquirejobs`, `generatejitconfig`, and replay-safe message deletion (`docs/proposed/macos-scaleset-runner.md:120-165`).
3. **In-Pod Job Execution and Reporting:** Inside each job pod, the unmodified official GitHub Actions runner (`actions/runner` v2.338.0, `Runner.Listener` + `Runner.Worker`) consumes the JIT config, connects to GitHub's Broker (`ServerUrlV2`), calls Run Service `acquirejob`, renews the 60 s job lock (`JobDispatcher.cs:775-776`), streams console lines and step summaries to the Twirp `ResultsService` (`ResultsHttpClient.cs:610-632`), evaluates expressions (`PipelineTemplateEvaluatorWrapper.cs`), runs composite, JavaScript (`node24`), Docker, and `run:` steps, and posts `CompleteJobRequest` (`JobRunner.cs:290-332`). This satisfies `docs/goals.md:16` ("Jobs are always executed by GitHub's official runner") without needing an owner exemption.

### What is redesigned inside

1. **Replace ARC (`ghalistener` + 4 Go controllers) with `velnor-host` (`KubernetesRuntime`) and `velnor-node-agent`:**
   - Building on PR `#1324` (`docs/proposed/kubernetes-runtime.md`), `velnor-runner-host` abstracts container and storage lifecycle behind the `Runtime` trait (`DockerRuntime` for macOS `velnor-local` and `KubernetesRuntime` for the Hetzner k3s cluster).
   - Instead of ARC creating a cold pod and cold `docker:dind` sidecar *after* `JobAvailable` arrives, `velnor-host` maintains a **pre-warmed standby pool** of pods across the workers (2 to 3 pre-warmed pods per worker node, with `docker:dind` already booted and passing its `docker info` probe, and the `runner` container waiting on an atomic FIFO/file channel for its JIT config and slot binding).
   - When `JobAvailable` arrives on the scale-set session, `velnor-host` classifies the job's `(ownerName, repositoryName, eventName, jobWorkflowRef, jobDisplayName)` from the scale-set message (`actions/scaleset/types.go:21-57`, `evidence.md:1181`), selects the warm node and slot via the cache-affinity table, writes the per-job JIT config and slot lease, and signals the pre-warmed pod to `exec` `Runner.Listener`. Pod creation and `dind` boot move completely off the critical path (saving 12 to 16 seconds per job).
2. **Eliminate path and environment drift between hosted and scale-set runners:**
   - Standardize the runner workspace and temp layout so `GITHUB_WORKSPACE` and `RUNNER_TEMP` never diverge across lanes, or symlink `/home/runner/work` to `/home/runner/_work` and unify `ImageOS` / `compat` inputs in `velnor-gen` and `velnor-ci` (`crates/velnor-model/src/cache_layout.rs:52-100` already drops `RUNNER_TEMP` and `ImageOS` from cache keys, keying only on `RUNNER_OS`, `RUNNER_ARCH`, `compat`, and `Cargo.lock`).
3. **Intercept slow WAN operations transparently inside the pod without replacing `Runner.Worker`:**
   - **Zero-download Actions (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`):** The official runner (`ActionManager.cs:1236-1290`, `evidence.md:1338`) supports a local directory of pre-downloaded action tarballs (`owner/repo/ref.tar.gz`). We bake the 11 pinned action tarballs from `.velnor/version-policy.toml` into `/opt/velnor/action-archive-cache` in the runner image and export `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archive-cache`. `PrepareActionsAsync` then resolves all actions locally with 0 network downloads.
   - **Pre-baked, pre-verified Rust 1.99.1 toolchain:** Bake `rust@1.99.1` (`minimal` + `clippy` + `rustfmt` + `x86_64-unknown-linux-gnu` + `aarch64-unknown-linux-gnu`, plus the verified `channel-rust-1.99.1.toml` and tree digest) and `mold@2.42.0` into an immutable read-only directory mounted at `/opt/velnor/seed` (`ro` non-overlay bind mount with `PROVENANCE` matching `.github/actions/velnor-tool-seed/action.yml:32`). That cuts `Prepare pinned tools` from 10 to 18 s down to **<0.5 s** across all 31 jobs (saving 706 runner-minutes across the sample in `evidence.md` T9b).
   - **Node-local Git Bare Reference Mirror:** `velnor-node-agent` maintains a bare git mirror per repository at `/var/lib/rancher/velnor-cache/git/<owner>/<repo>.git`, updated asynchronously and mounted read-only into pods at `/opt/velnor/git-mirrors`. Populating `.git/objects/info/alternates` via `velnor-job-start-hook` (`ACTIONS_RUNNER_HOOK_JOB_STARTED`) or system `/etc/gitconfig` (`url."/opt/velnor/git-mirrors/".insteadOf`) lets unmodified `actions/checkout@v7` fetch objects from local NVMe in **<0.8 s** instead of 6 to 8 s over HTTPS.
   - **Node-local `gha_cache` service (`actions/cache` v1/v2 transparent acceleration):** Port `velnor-legacy/crates/velnor-runner/src/gha_cache.rs` (trimmed to the core ~2,500 LOC v1 `_apis/artifactcache` + v2 Twirp `CacheService` HTTP server) into `velnor-node-agent`. For jobs that use `actions/cache@v6` (including `velnor-gen`'s `Restore the target cache` and `Restore the sources cache`, `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml:163-182`), redirecting `ACTIONS_CACHE_URL` and `ACTIONS_RESULTS_URL` (for `CacheService` Twirp routes while proxying log/step Twirp routes to GitHub) gives **1.5+ GB/s local NVMe cache restores** instead of 13.6 to 22.4 MB/s from US Azure Blob, with no 10 GB cap.

### What to take and what NOT to copy from each codebase

1. **From `velnor-new` (`crates/velnor-runner-*`, `generator/`, `ci-helper/`):**
   - **Take:**
     - `velnor-runner-core` (`crates/velnor-runner-core/src/lifecycle.rs:9-75`, `ownership.rs:1-60`): `WorkerState` state machine and `CleanupProof` discipline.
     - `velnor-runner-github` (`crates/velnor-runner-github/src/session/`, `acquire.rs`): pure wire-protocol types for Scale Set `v6.0-preview`.
     - `velnor-runner-host` + PR `#1324` (`docs/proposed/kubernetes-runtime.md:80-214`): `Runtime` trait, UID precondition deletion, Turso journal (`src/journal/`) with PR `#1333` linear schema validation and PR `#1349` cached test templates.
     - `velnor-resource-probe` (`src/job_hook.rs`, `storage_init.rs`): `velnor-job-start-hook` (`ACTIONS_RUNNER_HOOK_JOB_STARTED`) and `velnor-storage-init`.
     - `velnor-gen` + `velnor-ci` (`generator/`, `ci-helper/`): workspace-grouped job layout (`docs/goals.md:69`), crate-read analysis, and `crates-mirror` wrapper (`crates/velnor-ci/src/io/crates_mirror.rs:15-68`).
   - **Do NOT copy:**
     - `velnor-actions` 0.1.80's 29 per-crate jobs or the State seal workflow two-phase stub+real producer (`docs/proposed/compiled-state-persistence.md:229-243`), which fails 16/18 times on `root` and `velnor-runner` (`evidence.md` T9d) and is superseded by `velnor-gen` + node/site-local caches.
     - `compiler_process_budget = 2` / `build.jobs=2` (`.velnor/config.toml:26`, `mise.toml:90-193`) on 96-thread EPYC workers.
     - Hashing `$RUNNER_TEMP` or `ImageOS` into cache keys (`ci.yml:329, 445, 707`), which caused the 100% cache miss rate on scale-set pods.
2. **From `velnor-legacy` (`~/.cache/ci-ambition/refs/velnor-legacy`):**
   - **Take:**
     - `mbx_store.rs` (`crates/velnor-runner/src/mbx_store.rs:1-93`): per-slot directory layout (`<store>/slots/<slot>` for `MBX_CACHE_DIR` and `<store>/targets/slots/<slot>` for `MBX_TARGET_ROOT`) that prevents `mbx` registrar flock and Cargo target-dir lock contention across concurrent jobs on the same node, plus repository-scoped GC roots.
     - `gha_cache.rs` (`crates/velnor-runner/src/gha_cache.rs:1-98`): content-addressed local cache server speaking v1 `_apis/artifactcache` and v2 Twirp `CacheService` with `TrustClass` fork-headed namespace chains (`fork-` vs `repo-` hash domains, read-through to base ref, write-isolated to head scope).
     - `git_mirror.rs` (`crates/velnor-runner/src/git_mirror.rs:1-120`): bare git mirror management with flock serialization.
     - `velnor-bench` (`crates/velnor-bench/README.md`): discipline of measuring real end-to-end stages rather than synthetic `cargo`-only scripts.
   - **Do NOT copy:**
     - The 337k-LOC native step executor (`executor.rs` 37,028 LOC, `runner.rs` 36,759 LOC, `protocol.rs` 18,419 LOC, `action.rs` 6,978 LOC) or the 26 native action adapters, which caused 60% fix-commit churn, broke on Twirp `ResultsService` reruns (`#670`) and artifact scoping (`#671`), and blocked non-allowlisted actions.
     - `docker_lease.rs` (21,607 LOC) socket-splicing proxy; per-pod isolated DinD or Sysbox/Kata sandbox with UID-verified pod teardown is simpler and does not hang BuildKit (`#348`).
3. **From Official Runner (`actions/runner` v2.338.0) and ARC (`actions/actions-runner-controller` 0.15.1):**
   - **Take:**
     - The unmodified official runner binary (`run.sh` / `Runner.Listener`) and its extension points: `ACTIONS_RUNNER_INPUT_JITCONFIG` (`Runner.cs:234-253`), `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (`ActionManager.cs:1236-1290`), and `ACTIONS_RUNNER_HOOK_JOB_STARTED` / `COMPLETED` (`JobExtension.cs:304-313, 573-580`).
     - The Go `actions/scaleset` reference behavior (`scaleset/client.go`, `session_client.go`) and Kubernetes `Secret` `ownerReferences` pattern (`resourcebuilder.go:908-915`).
   - **Do NOT copy:**
     - Reading `GITHUB_EVENT_PATH` (`_temp/_github_workflow/event.json`) inside `background: true` parallel steps without copying `event.json` once in a pre-parallel step, because `ActionRunner.cs:141` (`ExecutionContext.cs:1335`) calls non-atomic `File.WriteAllText` on `event.json` at the start of every action step.
     - ARC's reactive pod-per-job creation without a pre-warmed standby pool (`scaler.go:135-186`), stateless `emptyDir`-only pod template (`evidence.md:557`), or `containerMode: kubernetes` hooks (`evidence.md:1428-1436`).

## 4. The cache system

Applying **Principle: Separate Before Serializing Shared State**, we design a three-tier cache hierarchy (L0 baked image/seed, L1 node-local persistent NVMe slots on XFS with reflinks, and L2 same-site `mbx-cache` + `gha_cache` service) where concurrent jobs never contend on a shared mutable lock (`mbx` registrar flock or Cargo `target/` lock) and untrusted fork PRs can never poison or observe trusted write slots.

### What is cached, where it lives, and expected hit rates

1. **Toolchains and CLI tools (L0 Image + Node Read-Only Seed):**
   - **Contents (~1.8 GB):** Rust `1.99.1` (`minimal` + `clippy`, `rustfmt`, `rust-lld`, `x86_64` and `aarch64` targets: ~1.1 GB), `mise` `2026.11.1`, `mbx` `1.29.2`, `cargo-nextest` `0.9.151`, `mold` `2.42.0`, `cargo-deny` `0.20.2`, `cargo-machete` `0.9.2`, `jactionlint` `2.4.0`, `shellcheck` `0.11.0`, `bun` `1.4.4`, `opentofu` `1.13.8`, `crane` `0.22.1`, `gh` `2.103.3`, `cosign` `3.1.4`, `velnor-ci` `0.1.81`, and `velnor-actions` `0.1.80/0.1.81`.
   - **Where it lives:** Baked into `velnor-ci-runner` image AND mounted read-only from `/var/lib/rancher/velnor-cache/seed` (`ro` bind mount at `/opt/velnor/seed` with `/opt/velnor/seed/PROVENANCE` set to `velnor-host-seed-v1`). Workflows set `RUSTUP_HOME=/opt/velnor/seed/rustup` (or XFS reflink-clone into `$RUNNER_TEMP/velnor/rustup` in 50 ms).
   - **Expected hit rate:** **100%** on normal runs (falls back to L2/network only on the single PR that bumps `rust-toolchain.toml` or `mise.toml`). Restore time: **<0.2 s** (vs 10 to 18 s today).
2. **Action tarballs (L0 Image):**
   - **Contents (~45 MB):** Exact SHA-pinned tarballs of all 11 actions in `.velnor/version-policy.toml`.
   - **Where it lives:** `/opt/velnor/action-archive-cache` (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`), read directly by `Runner.Worker` (`ActionManager.cs:1236`).
   - **Expected hit rate:** **100%**. Restore time: **0.0 s**.
3. **Git repository mirror (L1 Node-local NVMe):**
   - **Contents (~250 MB per repo):** Bare git mirror of `tailrocks/velnor-new.git` at `/var/lib/rancher/velnor-cache/git/<owner>/<repo>.git`, updated by `velnor-node-agent` and mounted `ro` into pods at `/opt/velnor/git-mirrors`.
   - **Expected hit rate:** **100% on commit history**; checkout fetches only the tip delta (<50 KB). Checkout time: **0.8 to 1.5 s** (vs 6 to 8 s today).
4. **Crate sources and sparse index (L1 Node-local DaemonSet + Shared Registry Cache):**
   - **Contents (~59 MB for `velnor-new`, 200 GB cap):** `/var/lib/rancher/crates-mirror` (`velnor-system/crates-mirror` nginx DaemonSet, `PreferSameNode`) plus per-slot extracted sources in `/var/lib/rancher/velnor-cache/slots/<trust>/<repo>/<slot>/cargo`.
   - **Where it lives:** Activated globally via pod environment variables (`CARGO_SOURCE_CRATES_IO_REPLACE_WITH=velnor-crates-mirror`, `CARGO_SOURCE_VELNOR_CRATES_MIRROR_REGISTRY=sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/`, `CARGO_HTTP_CAINFO=/opt/velnor/crates-mirror/ca-bundle.pem`).
   - **Expected hit rate:** **99.5%+** (100% when `Cargo.lock` is unchanged). Restore/fetch time: **0.0 s when slot is warm**, **2.7 s on cold slot via local mirror** (`RESULT.md:134`).
5. **Compiled build artifacts and `mbx` store (L1 Node-local Per-Slot + L2 Site-local Server):**
   - **Contents:** Content-addressed `mbx` compiler action/object store (~1.4 GB compressed / ~8.3 GB raw for root, ~0.9 GB / ~3.5 GB for `velnor-runner`, `docs/proposed/compiled-state-persistence.md:136-143`) plus per-slot managed `target/` directories.
   - **Where it lives:**
     - **L1 (Node-local NVMe Per-Slot):** `/var/lib/rancher/velnor-cache/slots/<trust>/<repo>/<slot>/mbx` (mounted at fixed container path `/var/cache/velnor/mbx` as `MBX_CACHE_DIR`) and `.../target` (`CARGO_TARGET_DIR` / `MBX_TARGET_ROOT`). Exclusive per-slot lease (`K = 6` slots per node) ensures at most one active pod uses slot `s` at a time (`velnor-legacy/crates/velnor-runner/src/mbx_store.rs:6-13`).
     - **L2 (Site-local Shared Server):** `mbx-cache` (`jdx/mr-boxington-cache`) + `gha_cache` service on `velnor-240` (DE, 50 TB NVMe) and `velnor-241` (FI, 3.8 TB NVMe), behind a Kubernetes Service with `trafficDistribution: PreferSameZone`.
   - **Expected hit rate:** **95% to 99% unit hit rate on PR edits** (measured in `compiled-state-persistence.md:144-147`: 98.2% on leaf change, 94.6% on core-crate change, 98.3% on no-change run). L1 warm slot restore takes **0.0 s network transfer and <1 s XFS reflink restore**; L1 cold slot / L2 warm site hit takes **2 to 4 s over 10 Gbps LAN** (vs 65 to 107 s from GitHub US blob).
6. **DinD container image layers (L1 Node-local Per-Slot):**
   - **Contents:** `/var/lib/docker` `overlay2` layers for container integration tests (`velnor-runner-host`, `velnor-local`).
   - **Where it lives:** `/var/lib/rancher/velnor-cache/slots/<trust>/<repo>/<slot>/docker` (scoped by `<trust>`).
   - **Expected hit rate:** **98%+ layer hit rate**, eliminating test image pulls inside DinD.
7. **Test timings ledger (L2 Site Server):**
   - **Contents:** `.velnor/rust-ci-timings.json` persisted by `velnor-ci timings` (`crates/velnor-ci/src/timings.rs`) to balance nextest shard partitions within **±10%** wall time.

### How jobs are placed on warm nodes (cache-affinity scheduling)

In ARC today, pods carry no affinity hints (`evidence.md:558`), scattering jobs across DE and FI nodes. In our design, `velnor-host` places jobs via a deterministic two-level cache-affinity scheduler:

1. **Derive `cache_group` and `trust_pool` at job offer time:** From the scale-set `JobAvailable` message fields `(ownerName, repositoryName, jobDisplayName, eventName, workflowRunId)` (`actions/scaleset/types.go:21-57`, `evidence.md:1181`), `velnor-host` computes `cache_group` (e.g. `tailrocks/velnor-new/rust-root`) and `trust_pool` (`trusted-main`, `trusted-pr`, or `untrusted-fork`).
2. **Rendezvous hashing + Turso-journaled MRU slot table:**
   - Each `cache_group` maps via Rendezvous hashing to a primary site (e.g. DE nodes `velnor-240` and `velnor-105` plus FI node `velnor-241` for `tailrocks`; FI nodes `velnor-5` and `velnor-190` for `ChainArgos` and `jackin-project`, with automatic overflow).
   - **Rule 1 (Warm Idle Slot):** Pick the `Idle` slot on any node in the primary site that most recently built the exact `cache_group`.
   - **Rule 2 (Same-Site Free Slot + XFS Reflink Clone):** If all warm slots for `cache_group` are busy (concurrent PRs), pick an `Idle` slot on the same node (or same site) with free CPU headroom (`allocated_cpu + job_cpu <= 88` of 96 cores). `velnor-node-agent` initializes the slot by `cp --reflink=always -a` from the node's committed base snapshot (~0.5 s on XFS) and pulls any missing objects from the same-site L2 `mbx-cache` server over LAN (~2 to 4 s), never crossing the 35 ms DE-to-FI WireGuard link.
   - **Rule 3 (Cross-Site Overflow):** If the primary site exceeds 85% CPU allocation, place the job on the secondary site, whose L2 `mbx-cache` server receives asynchronous background replication from the primary site.

### How correctness and trust are kept (`main` vs same-repo PRs vs fork PRs)

`docs/goals.md:41-44` ("A broken cache is slow, never wrong... Secrets never reach the build... A pull request from this repository writes a cache only into its own scope. A pull request from a fork gets no token and only reads caches") is enforced at three boundaries:

1. **Resolving fork provenance before mounting any slot:**
   - Scale-set `JobAvailable` messages do not carry `head.repo.fork` (`evidence.md:1041`). Before `velnor-node-agent` mounts a cache slot into a pod (or via `velnor-job-start-hook` in `ACTIONS_RUNNER_HOOK_JOB_STARTED` reading `GITHUB_EVENT_PATH`, cached by `workflowRunId` in `velnor-host`), the run is classified into `TrustedMain` (`push` to protected `main` or `merge_group`), `SameRepoPR` (`pull_request` where `head.repo.full_name == base.repo.full_name` and `fork == false`), or `UntrustedFork` (`fork == true` or unknown).
2. **Zero-copy XFS reflink isolation between `main`, same-repo PRs, and forks:**
   - **Base (`TrustedMain`) snapshots:** Only `TrustedMain` jobs can commit new base snapshots to `/var/lib/rancher/velnor-cache/base/<repo>/<workspace_root>/gen-<sha>` (via atomic `cp --reflink=always -a` after the build step succeeds, taking ~0.5 s on XFS) and hold `read-write` tokens for the L2 `mbx-cache` base namespace.
   - **Same-repo PRs (`SameRepoPR`):** Receive an isolated per-PR slot directory `/var/lib/rancher/velnor-cache/pr/<repo>/pr-<number>/slot-<s>`. On the PR's first push, `velnor-node-agent` initializes it via `cp --reflink=always -a` from the newest `base/.../gen-*` snapshot (~0.5 s, 0 data blocks copied). On subsequent pushes to the same PR, the PR reuses and updates its own `pr-<number>/slot-<s>` directory (`docs/goals.md:56, 72`), and can never mutate `base/` or another PR's directory. When the PR closes or merges, `velnor-node-agent` deletes `pr-<number>/`.
   - **Untrusted Fork PRs (`UntrustedFork`):** Receive a disposable `cp --reflink=always -a` clone of `base/.../gen-*` in `/var/lib/rancher/velnor-cache/ephemeral/job-<id>/` that is **unconditionally deleted on job exit**, plus a **read-only** L2 `mbx-cache` token. A fork PR can never write to `base/`, `pr/`, or L2 cache, and its DinD storage is wiped on job exit.
3. **Cache self-proof (`docs/goals.md:67`, Design Rule 3):**
   - `velnor-ci` (`crates/velnor-ci/src/observed_units.rs` and `gate.rs`) checks Cargo's JSON `--message-format` (`fresh: true|false`) and `mbx` hit/miss counters against the planned selection, turning the non-blocking `Cache` status check red if an unchanged crate was recompiled from a warm cache.

## 5. Workflow design for speed

Applying **Principle: Laziness Protocol** and **Principle: Redesign from First Principles**, we redesign the workflow around `docs/goals.md` Design Rules 2, 4, 5, and 6 and the owner's single root workspace consolidation (`crates/velnor-*`, `brief-v1.md:67`).

### 1. Move all gate and lint jobs onto the cluster and fix `state seal` path drift

In `evidence.md` T5, moving the 8 hosted jobs (`plan`, `ledger-cli`, `alint`, `cargo-deny`, `cargo-machete`, `jactionlint`, `required`, `required-gate`) to `[velnor, ubuntu-26.04-scale-set]` (as PR `#1332` and `velnor-actions v0.1.81` begin doing) removes **57.0 minutes of median PR queue wait on contended days** and **5.8 to 12.6 minutes on normal days**.
- **Critical fix beyond PR `#1332`:** PR `#1332` keeps the State seal workflow on hosted `ubuntu-26.04`. As we proved in Section 1, hosted jobs use `/home/runner/work/_temp` and `ImageOS=ubuntu24` while scale-set pods use `/home/runner/_work/_temp` and `ImageOS=velnor-arc-ubuntu24`, which causes 100% cache misses across the boundary (`ci.yml:329, 445, 707`). Until the State seal workflow is retired by L1/L2 cluster caching, unify `/home/runner/work` -> `/home/runner/_work` and `ImageOS` (or move the State seal workflow's `plan`, `sources-producer`, and `state-producer-*` onto the scale set too).
- Also restrict `job-image-candidate.yml:4` (currently `push: {}`, which triggered **306 hosted runs on 2026-11-18**, `evidence.md:399`) to `branches: [main]` or `paths: ['images/**', 'crates/velnor-runner/**']`, and move `source-qualification.yml` (267 runs on 11-18) onto the scale set.

### 2. Consolidate from 39 per-crate jobs to an 8-job Build-Once + Sharded-Test DAG

Today's 39-job `ci.yml` pays pod setup, checkout, tool verification, and dependency compilation 29 times across 29 `rust-*` jobs, plus a 30th uncached compile in `check-maintenance-helpers` and a 31st in `ledger-cli`.
Under `velnor-gen` (`docs/goals.md:68-70`: "Compile at most twice... Test binaries are built once for each configuration and handed to every test group that runs it") and the unified root workspace (`crates/velnor-*`), the workflow collapses to **8 fast jobs** (fewer when change selection skips unaffected groups):

```
                           +-----------------------+
                           | 1. Plan & Fast Lints  |  (~12 s on warm scale-set pod)
                           | - velnor-ci plan      |
                           | - jactionlint, alint  |
                           | - cargo-deny, machete |
                           | - generator-samples   |
                           +-----------+-----------+
                                       |
             +-------------------------+-------------------------+
             | (if Rust crates selected)                         | (if clippy/doc selected)
             v                                                   v
+-----------------------------------------+         +-----------------------------------------+
| 2. Build Test Binaries (Warm Slot, 16c) |         | 3. Lint & Docs (Warm Slot, 16c)         |
| - cargo fmt --all --check               |         | - cargo clippy --workspace --all-targets|
| - cargo nextest archive / build-bins    |         | - cargo doc --workspace --no-deps       |
|   (or node-local shared target/ view)   |         | - check-native-image-inventory          |
| (~18-28 s incremental, ~55 s full warm) |         | (~15-25 s incremental, ~45 s full warm) |
+------------+---------------+------------+         +--------------------+--------------------+
             |               |                                           |
     +-------+-------+-------+-------+-------+                           |
     v               v               v       v                           |
+----------+   +-----------+   +-----------+ +-----------+               |
| 4a. Test |   | 4b. Test  |   | 4c. Test  | | 4d. Test  |               |
| Shard 1  |   | Shard 2   |   | Shard 3   | | Shard 4   |               |
| (runner- |   | (runner-  |   | (actions- | | (fast     |               |
| host 1/2 |   | host 2/2  |   | cli +     | | crates:   |               |
| + local- |   | + local-  |   | archive-  | | ci, gen,  |               |
| mode 1/2)|   | mode 2/2) |   | guard +   | | orch,     |               |
| (~35 s)  |   | (~35 s)   |   | maint)    | | local...) |               |
+----+-----+   +-----+-----+   | (~35 s)   | | (~25 s)   |               |
     |               |         +-----+-----+ +-----+-----+               |
     +---------------+---------------+-------------+---------------------+
                                     |
                                     v
                         +-----------------------+
                         | 5. Required Verdict   |  (~3 s on scale-set pod)
                         +-----------------------+
```

Every verification check from today's 39 jobs runs, without the duplication:
1. **Combine the 5 static lints with `Plan`:** `Cargo Machete` (12 s median, `evidence.md` T4), `Jactionlint` (18 s), `Alint` (18 s), `Cargo Deny` (24 s), and `lint-generator-samples` all run on pre-baked tools in parallel on 16 cores in **~12 to 15 seconds**, saving 5 pod startups.
2. **Delete the serial `Ledger CLI` job (`ci.yml:275-366`):** `Ledger CLI` adds a 54-second serial barrier before every `rust-*` job to build `velnor-actions` from HEAD. Under `docs/goals.md:76` (Design Rule 12), the workflow uses the released `velnor-ci` (`0.1.81`) binary seeded in `/opt/velnor/seed/generator/`, removing the 54-second barrier completely.
3. **Fold `Check / maintenance-helpers` (`mise.toml:97-200`, 508 s median -> ~20 s) into the shared workspace build and Test Shard 3:** `velnor-actions-freshness` (40 tests) and `velnor-archive-guard` (55 tests, `evidence.md:766`) compile once in Stage 2 with warm `mbx` cache and 16 cores (instead of cold at `build.jobs=2`), and run in Shard 3 alongside `bun test ./scripts/tag-ruleset-drift.test.ts`, `python3 scripts/test-*.py`, and `pin-mirror` (reusing the compiled `velnor-actions` binary).

### 3. Change-aware selection without false full-runs

1. **Decouple unit test inputs from catch-all repository globs:** Today `.velnor/config.toml:120-238` maps `crates/**` and `crates/**/src/lib.rs` to `velnor-archive-guard`, `velnor-actions-orchestrator`, `velnor-actions-freshness`, `velnor-actions-mise`, and `check-maintenance-helpers`, causing 77 of 77 sampled PR runs to trigger all jobs (`evidence.md` T3). Point crate unit tests at `fixtures/` rather than scanning the live repository tree, and run the fast (<5 s) repository-wide policy checks (`pin-mirror`, `check-native-image-inventory`) in Stage 3. Then editing a single crate selects **only** that crate and its reverse dependents.
2. **Smart `-p` vs `--workspace` selection (`docs/hard-decisions.md` Decision 11):** `velnor-ci plan` compares the resolved Cargo feature set of the selected packages under `-p` against the `--workspace` feature graph using `cargo metadata --format-version 1` (<100 ms). Whenever the feature sets match, Stage 2 compiles and tests only `-p <selected_packages>`; when they differ, it uses `--workspace` (which still hits 98%+ of warm units in the node-local `mbx`/`target` slot).

### 4. Test optimization and sharding across the cluster

1. **Eliminate the 80% SQLite migration tax in `velnor-runner-host` tests:** Land PR `#1333` (linear schema validation, `-38%` CPU on 42 tests) and PR `#1349` (file-locked historical v45-v48 template copy, `-45%` CPU on restart/retirement tests), and extend `historical_template.rs` to cache a pre-migrated **v51 (current schema)** SQLite template file so every test starting from an empty current journal copies a 64 KB file on `tmpfs` instead of running 51 DDL migrations.
2. **Deduplicate `velnor-runner-host` and `velnor-runner-host (local-mode)`:** Instead of running the 430 base tests of `velnor-runner-host` twice (`6.23 + 5.49 = 11.72` runner-min in `evidence.md` T3, `17.7` min on 11-18 in `evidence.md` T6), run `cargo check -p velnor-runner-host` (proving the non-`local-mode` release compile gate) plus **one** `cargo nextest run -p velnor-runner-host --features local-mode` sharded across 2 pods (`Shard 1` and `Shard 2`).
3. **Shard test execution across 4 parallel pods:** Using nextest partitioning (`--partition hash:i/N` or duration-balanced test groups from `.velnor/rust-ci-timings.json`) and raising `docker-networks` `max-threads` in `.config/nextest.toml` (`evidence.md:753`) from `4` to `12` on a 16-core pod cuts the longest test stage from **246 s median (497 s p90) down to ~35 to 45 s**.

### 5. Eliminate double and triple runs across PR, `merge_group`, and `push`

1. **Skip duplicate `push`-to-`main` verification when `GITHUB_SHA` matches a green `merge_group` run:** In 120 of 159 merges (75.5%, `evidence.md` T7), `push` to `main` runs on the exact same `GITHUB_SHA` that just passed `merge_group`. In `velnor-ci plan`, on `push` to `main`, query `GET /repos/{owner}/{repo}/commits/{GITHUB_SHA}/check-runs?check_name=Required`. When a green `Required` check from `merge_group` exists for `GITHUB_SHA`, skip re-running verification jobs (`push` finishes in **<15 s**, saving 62.0 runner-minutes per merge).
2. **Use change-aware selection on `merge_group` (`docs/goals.md:86` default) and tree-SHA verdict reuse:** Because `merge_group` builds on top of a green `main` (or green predecessor queue entry), running change-aware selection on `first_parent..HEAD` (backed by the nightly scheduled full run, `docs/goals.md:87`) tests only what the queued PR(s) can affect. Furthermore, when `git rev-parse HEAD^{tree}` equals the PR head's tree SHA (which already has a green `Required` check from the PR run), `Plan` can verify tree equivalence and complete in **<15 s**.

### Minimum honest time to verdict for a typical `velnor-new` PR

Estimated arithmetic from measured `T9b`/`T10` baselines and `compiled-state-persistence.md:149` warm probe ratios, on warm L1 node slots with 16-core pods and `mold`:

| PR Scenario | Stage 1: Pickup + `Plan` & Lints | Stage 2/3: Warm Incremental Build + `Clippy`/`Doc` (parallel) | Stage 4: Sharded `nextest` Execution | Stage 5: `Required` Verdict | **Total Honest Push-to-Verdict Wall Time** |
|---|---:|---:|---:|---:|---:|
| **Docs-only PR** (`docs/**`) | 1.5 s + 10 s | 0 s (skipped) | 0 s (skipped) | 2.5 s | **~14 seconds** (`docs/goals.md:53` target is <60 s) |
| **Typical single-crate / leaf PR** (`velnor-ci`, `velnor-gen`, `velnor-local`, or sub-crate) | 1.5 s + 12 s | 1.5 s + 14 s (`mbx` hits 98.2% of units; compiles 1 to 2 crates + links with `mold`) | 10 to 15 s (runs in same pod right after build) | 2.5 s | **~42 to 50 seconds** |
| **Heavy crate PR** (`velnor-runner-host` or `velnor-actions-cli`) | 1.5 s + 12 s | 1.5 s + 28 s (compiles test bins with warm deps + `mold`) | 1.5 s + 38 s (parallel shards after PR `#1333` + `#1349` + v51 template + `tmpfs`) | 2.5 s | **~85 to 105 seconds (~1.5 to 1.8 minutes)** |
| **Full-workspace / lockfile PR** (runs all tests across all packages, warm L1 cache) | 1.5 s + 15 s | 1.5 s + 55 s | 1.5 s + 45 s (4 parallel shards) | 2.5 s | **~120 to 135 seconds (~2.0 to 2.2 minutes)** |

## 6. Kubernetes architecture and pod lifecycle

Applying **Principle: Foundational Thinking** and **Principle: Make Operations Idempotent**, we configure Kubernetes so pods start in <1.5 seconds, never thrash CPU or NUMA domains, and strictly isolate untrusted fork PRs.

### 1. Pod lifecycle and pre-warming

Today ARC creates a pod *after* a job arrives, waiting 12 to 18 seconds for `init-dind-externals`, `dockerd` startup, and the 5-second `docker info` startup probe (`evidence.md:556`).
Under `velnor-host` (`KubernetesRuntime`):
- `velnor-host` maintains **`M = 2` pre-warmed standby pods per worker node** (10 across the cluster) with `restartPolicy: Never` and `PriorityClass: velnor-standby-warm`.
- In each standby pod, `init-dind-externals` has already completed, `dind` (`dockerd --mtu=1420`) is already running and healthy, and `/usr/local/bin/velnor-runner-start` in the `runner` container has pre-faulted `/home/runner/bin/Runner.Listener` into page cache and is waiting on `/run/velnor-jit/assign.fifo`.
- When `JobAvailable` arrives:
  1. `velnor-host` calls `AcquireJobs` and `GenerateJitConfig`, selects the warm node and slot `s`, instructs `velnor-node-agent` to bind-mount slot `s` into the standby pod's shared work volume, raises the pod's priority, and delivers the single-use JIT config into a mode-`0600` tmpfs file (`Runtime::deliver`, `docs/proposed/kubernetes-runtime.md:135, 359`).
  2. The standby pod immediately `exec`s `Runner.Listener` (<1.5 s total pickup time), while `velnor-host` spawns a replacement standby pod in the background.
  3. When `runner` container terminates (`state.terminated.exitCode`), `velnor-host` records completion in the Turso journal, tells `velnor-node-agent` to snapshot/release slot `s`, deletes the Pod with `preconditions.uid` (`kubernetes-runtime.md:201`), and proves absence via quorum `GET` 404 before releasing the slot permit.

### 2. Right-sizing pods to the EPYC 9454 geometry (`K = 6` slots per worker)

Today pods request `cpu: 2, memory: 4Gi` with `limits: { memory: 32Gi }` and **no CPU limit** (`evidence.md:554`), allowing up to 30 pods on one 96-thread node where a single pod grabs 83.7 cores (`evidence.md:517`) and inflates sibling build times by 2.4x (`evidence.md` T6).
- Each AMD EPYC 9454/9454P worker has **48 physical Zen 4 cores / 96 threads across 8 CCDs** (6 cores / 12 threads and 32 MB L3 cache per CCD) and **125.5 GiB RAM**.
- Reserve **6 threads and 13.5 GiB RAM** per worker for OS, kubelet, containerd, WireGuard, `crates-mirror`, `velnor-node-agent`, and site L2 `mbx-cache`.
- Divide the remaining **90 threads and 112 GiB RAM** into **`K = 6` heavy build/test slots per worker** (`30` concurrent heavy slots across the 5 workers), giving each heavy pod **15 threads and 18 GiB RAM** (`requests: { cpu: "14", memory: "16Gi" }`, `limits: { cpu: "16", memory: "20Gi" }`, plus `CARGO_BUILD_JOBS=16` and `NEXTEST_TEST_THREADS=16`).
- Add a lightweight `fast-gate` pod class (`requests: { cpu: "2", memory: "2Gi" }`, `limits: { cpu: "4", memory: "4Gi" }`, `PriorityClass: velnor-gate-critical`) without a `dind` sidecar for `Plan`, `Required verdict`, and `Required`, so gates always start in <1 second even during a 30-pod cluster burst.

### 3. Node affinity and scaling across all 5 workers and both sites (DE and FI)

- Add topology labels to every node: `topology.kubernetes.io/zone=de` and `velnor.io/storage-tier=large` on `velnor-240` (50 TB) and `velnor-105` (34.6 TB); `topology.kubernetes.io/zone=fi` and `velnor.io/storage-tier=standard` on `velnor-241`, `velnor-5`, and `velnor-190` (3.8 TB each).
- Deploy one L2 `mbx-cache` + `gha_cache` server in DE (`velnor-240`) and one in FI (`velnor-241`) behind a Service with `trafficDistribution: PreferSameZone`. Pods never pull cache blobs across the 35 ms DE-to-FI WireGuard link; the two L2 servers replicate committed `main` blobs asynchronously in the background.
- Give `tailrocks` repos primary cache affinity to `velnor-240`, `velnor-105` (DE), and `velnor-241` (FI) (18 heavy 16-core slots), and give `ChainArgos` and `jackin-project` primary affinity to `velnor-5` and `velnor-190` (FI, 12 heavy 16-core slots), with automatic overflow to any idle node when a site exceeds 85% utilization.

### 4. Isolation of untrusted PRs

1. **Omit the privileged `dind` sidecar on non-Docker jobs:** 80% of jobs (`Plan`, lints, `velnor-gen`, `velnor-ci`, `velnor-actions-*` pure unit tests) do not use Docker. Running them in a single-container pod without `dind` allows Pod Security Admission `restricted` (`allowPrivilegeEscalation: false`, `capabilities.drop: [ALL]`, `seccompProfile: Localhost/runner-v1.json`, `automountServiceAccountToken: false`).
2. **Replace privileged `dind` with `RuntimeClass: sysbox-runc` (or Kata Containers):** Installing Sysbox (`sysbox-runc`) on the worker nodes lets Docker-in-Docker pods run `dockerd` inside a rootless Linux user namespace without `privileged: true`. Until Sysbox lands in Phase 3, keep privileged `dind` fenced to same-repository runs (`EPHEMERAL_CHECK_ADMISSION_CONDITION` from PR `#1332`) and schedule fork PRs into a dedicated `velnor-runners-untrusted` namespace with a default-deny `NetworkPolicy` (blocking `10.42.0.0/16`, `10.43.0.0/16` except DNS/read-only cache, `169.254.169.254/32`, and `5.9.55.237:6443`, `docs/proposed/kubernetes-runtime.md:399-402`) and ephemeral reflinked cache slots wiped on exit.

## 7. The role of mbx and blaze

### 1. Why `blaze` does not fit `velnor-new`'s pinned toolchain as a CI verifier

- **Verdict:** `blaze` (`https://github.com/indexable-inc/blaze`) **cannot** be used as `velnor-new`'s CI verification compiler, because it violates `velnor-new`'s pinned Rust `1.99.1` toolchain contract (`docs/hard-decisions.md:232-268`) and verification promise (`docs/goals.md:36`).
- **Evidence from `indexable-inc/blaze` documentation (`gh api repos/indexable-inc/blaze/contents/docs/toolchain.md` and `docs/usage.md`):**
  1. **Ignores `rust-toolchain.toml` and uses its own pinned nightly (`docs/toolchain.md`):** `blaze` states: *"blaze is its own Rust toolchain. It builds every project with one toolchain that blaze controls, names and ships, whatever the project pins: `blaze-2026-10-27` today (`1.101.0-nightly (16d1ffac1 2026-10-26)`). ... blaze uses the language and std its own binary ships, ignores `rust-toolchain`, `rust-toolchain.toml`, rustup's settings and `RUSTUP_TOOLCHAIN`, and never runs or asks `rustc`."*
  2. **Incomplete borrow checker (`docs/usage.md`):** `blaze` states: *"The borrow checker is new: 0.1.0 has none, so an edit with a borrow error builds and runs; the one on `main` finds about two thirds of rustc's borrow errors. Keep `cargo check` or your CI as the final word."*
  3. **No `clippy`, `fmt`, or `doc` support (`docs/usage.md`):** `blaze` only accepts `build`, `check`, `run`, `test`, and `bench`. *"any other (`fmt`, `clippy`, `doc`, `tree`, ...) fails with `no such command`, and none runs cargo."*
  4. **Conflicts with `docs/goals.md:36` (Promise 3):** *"Fast never means checked less. Every check still runs when something it reads changes, with the project's own compiler, features and settings."*
- **Where `blaze` is useful:**
  - **Local agent pre-check before pushing:** AI agents editing code on local workstations can use `blaze`'s persistent in-memory daemon (`blaze check` / `blaze test`) for 50 to 450 ms feedback on syntax and type errors (`indexable-inc/blaze` `README.md`) before running `cargo clippy` and `cargo nextest`.
  - **Cache key design lesson (`docs/v2/plan.md`):** Like `blaze`'s path-free BLAKE3 content keys, `velnor-new` must keep container paths (`MBX_CACHE_DIR=/var/cache/velnor/mbx`, `GITHUB_WORKSPACE=/home/runner/_work/velnor-new/velnor-new`) fixed and remove ephemeral `$RUNNER_TEMP` and `ImageOS` variations from cache keys.

### 2. How `mbx` (`1.29.2`) and `mbx-cache` power Velnor's CI cache

- **`mbx` is the primary compiler-cache and linker driver for Velnor CI.** It wraps the exact pinned `rustc 1.99.1` and `cargo` (`mise.toml:13-20, 43-45`), caches `.rlib`, `.rmeta`, proc-macro `.so`, and build-script outputs by content hash, supports `clippy`, `doc`, and `nextest`, deduplicates concurrent compilations, and manages fast linkers (`MBX_LINKER=mold@2.42.0` or `rust-lld`, `jdx/mr-boxington` `docs/linkers.md`).
- **Four concrete changes to how `velnor-new` runs `mbx`:**
  1. **Mount a persistent node-local `MBX_CACHE_DIR=/var/cache/velnor/mbx` per slot** instead of ephemeral `${{ runner.temp }}/velnor/mbx` (`ci.yml:678`). Because `/var/cache/velnor/mbx` is identical across every pod and node, `MBX_SHARE_OUT_DIR=1` (`docs/proposed/compiled-state-persistence.md:185`) hits 100% of `OUT_DIR`-dependent crates (`serde_core`, `serde`, `thiserror`).
  2. **Deploy `mbx-cache` (`jdx/mr-boxington-cache`) in DE and FI (`MBX_REMOTE_URL=http://mbx-cache.velnor-system.svc.cluster.local:8080`)** with `MBX_REMOTE_NAMESPACE=tailrocks/velnor-new`. Trusted `main`/`merge_group` runs get `read-write` tokens; PR runs get `read-only` tokens (plus their node-local L1 slot). This retires the 2,201-line the State seal workflow workflow completely.
  3. **Remove `mbx clean` from job teardown (`ci.yml:1774-1777`) on persistent slots** so Cargo's own fingerprint check (`fresh: true`) reuses the slot's managed `target/` directory without even spawning the `mbx` rustc wrapper for unchanged crates.
  4. **Set `MBX_LINKER=mold@2.42.0`** (with `mold` 2.42.0 pre-installed in `/opt/velnor/seed` and `clang` already in the runner image, `RESULT.md:91`).

## 8a. Servers, storage, network, Kubernetes, and build tuning

### Current state measured read-only (`kubectl describe nodes`, `configz`, `stats/summary`, and `velnor-runners/state/*`)

From our read-only cluster queries (`kubectl describe nodes`, `kubectl top nodes`, `kubectl get --raw /api/v1/nodes/velnor-240/proxy/stats/summary`, and `evidence.md` Section B):
- **Hardware:** 5 workers (`velnor-240`, `velnor-105` in DE; `velnor-241`, `velnor-5`, `velnor-190` in FI), each with an AMD EPYC 9454/9454P (48 cores / 96 threads, 125.5 GiB RAM), plus 1 control plane (`velnor-237` in DE, i5-13500, 20 threads, 62.6 GiB RAM, tainted `NoSchedule`). All run Debian 13 (trixie), kernel `6.12.94+deb13-amd64`, k3s `v1.37.2+k3s1`, containerd `2.3.7-k3s1`.
- **Measured storage defect (`/api/v1/nodes/velnor-240/proxy/stats/summary`):**
  - `nodefs` (`/` on the single 3.5 TB OS NVMe disk, formatted `ext4` in `PLAN.md:45`) reports capacity `3,777,979,830,272 B` (3.78 TB). Because k3s started with default `--root-dir` (`/var/lib/kubelet`), **all pod `emptyDir: {}` volumes (`work` at `/home/runner/_work` and `tmp` at `/tmp`) live on the single `ext4` OS disk!**
  - Meanwhile, `imagefs` (`/var/lib/rancher`, XFS `noatime` LVM volume: `49.9 TB` on `velnor-240`, `34.6 TB` on `velnor-105`, `3.84 TB` on `velnor-241/5/190`) uses only **4.8 GB to 8 GB (<0.02% used)** because only containerd image layers live there.
  - Furthermore, `PLAN.md:76` ran `lvcreate -l 100%FREE -n rancher data` without `-i <num_pv>`, creating a **linear (concatenated)** LVM volume on `velnor-240` (4 data NVMes) and `velnor-105` (3 data NVMes) instead of striping across drives.
- **Measured kubelet config (`configz`):** `kubeReserved: none`, `systemReserved: none`, `cpuManagerPolicy: none`, `topologyManagerPolicy: none`, `evictionHard: {imagefs.available: 5%, nodefs.available: 5%}` (no `memory.available` threshold), `podPidsLimit: -1`, `failSwapOn: false`.

### 1. The OS (Debian 13, kernel `6.12.94+deb13-amd64`, EPYC 9454)

- **CPU governor:** Switch `amd-pstate-epp` from `powersave` (`balance_performance`) to `performance` (`cpupower frequency-set -g performance`), pinning all 48 Zen 4 cores at max boost clock (3.65 to 3.8 GHz) and eliminating 1 to 5 ms frequency ramp lag on bursty `rustc` process spawns.
- **SMT and NUMA layout:** Keep **SMT ON (96 threads)** (+18% to 25% throughput on mixed `rustc`/LLVM and I/O-bound `nextest` workloads when total threads are capped at 90 via pod limits). Pair with kubelet `cpu-manager-policy=static` and `topology-manager-policy=best-effort` so each 16-thread build pod stays within 1 to 2 adjacent Zen 4 CCDs (32 to 64 MB shared L3 cache and local DDR5 memory controller).
- **Transparent Hugepages (THP):** Set `/sys/kernel/mm/transparent_hugepage/enabled = madvise` and `defrag = defer+madvise` so LLVM arenas, `mold`, and `zstd --long=30` get 2 MiB TLB pages without synchronous compaction stalls.
- **Swap, overcommit, and `vm.dirty_*` writeback:** Disable swap (`swapoff -a`), set `vm.swappiness = 1`, `vm.overcommit_memory = 1` (preventing spurious `ENOMEM` when multi-process `nextest` suites `fork()`), and raise dirty writeback thresholds (`vm.dirty_background_ratio = 10`, `vm.dirty_ratio = 25`, `vm.dirty_expire_centisecs = 6000`, `vm.dirty_writeback_centisecs = 1500`) so short-lived compiler artifacts stay in the 128 GB page cache without flushing to NVMe.
- **File descriptors, inotify, and PIDs (`/etc/sysctl.d/99-velnor-ci.conf`):** Set `fs.file-max = 4194304`, `DefaultLimitNOFILE = 1048576`, `fs.inotify.max_user_watches = 1048576`, `fs.inotify.max_user_instances = 8192`, `kernel.pid_max = 4194304`, `vm.max_map_count = 1048576`, and `kernel.sched_autogroup_enabled = 0`.

### 2. Storage (NVMe layout, XFS reflinks, `tmpfs` scratch, containerd)

- **Move `/var/lib/kubelet` and all cache slots onto `/var/lib/rancher` (XFS `noatime,nodiratime,logbsize=256k` with `reflink=1`):** Bind-mount `/var/lib/rancher/k3s/kubelet` to `/var/lib/kubelet` (or set `--kubelet-arg=root-dir=/var/lib/rancher/k3s/kubelet`) and place all cache slots under `/var/lib/rancher/velnor-cache/`. Debian 13 `mkfs.xfs` enables `reflink=1` by default, giving `mbx` and `velnor-node-agent` sub-second `FICLONE` (`cp --reflink=always`) target snapshots without ZFS memory overhead or btrfs lock contention.
- **LVM RAID0 striping on multi-disk workers (`velnor-240` and `velnor-105`):** Create a striped LV (`lvcreate -i 3 -I 64k -L 12T -n fast_ci data` across the three 14 TB NVMes on `velnor-240`, and `-i 2 -I 64k -L 12T` across the two 14 TB NVMes on `velnor-105`) for `/var/lib/rancher/velnor-cache`, multiplying NVMe bandwidth to 6 to 10 GB/s.
- **`tmpfs` (`emptyDir: { medium: Memory, sizeLimit: 8Gi }`) for `/tmp` (`$RUNNER_TEMP`) and `/dev/shm`:** Workers use only 15% to 38% of their 125.5 GiB RAM under 95% CPU load (`evidence.md:513`), leaving 75+ GiB RAM idle on every worker. Putting `/tmp` (`$RUNNER_TEMP`) on `tmpfs` runs all 430 `velnor-runner-host` Turso SQLite test databases and compiler temporaries in DDR5 RAM with zero disk `fsync` cost (saving 30 to 60 s on `velnor-runner-host` tests alone).
- **Containerd and cluster registry mirror:** Keep `overlayfs` with `discard_unpacked_layers = false` and deploy a cluster-local OCI registry mirror (`registry:2` or `zot` on `velnor-240` + `/etc/rancher/k3s/registries.yaml`) so runner and DinD test images pull at LAN speed without manual `import-pod.sh` tar uploads.

### 3. Network (DE-to-FI WireGuard, MTU, and local mirrors)

- **Keep all cache and registry traffic site-local (`PreferSameZone`):** Label DE and FI nodes with `topology.kubernetes.io/zone=de|fi` and run site-local L2 `mbx-cache` + `gha_cache` instances on `velnor-240` (DE) and `velnor-241` (FI) so pods never wait on the 35 ms DE-to-FI WireGuard link.
- **Fix DinD MTU mismatch over Flannel WireGuard (`--mtu=1420`):** Flannel `wireguard-native` uses `MTU 1420` (1500 minus 60 B WireGuard/UDP/IPv4 header), while `docker:dind` defaults its inner `docker0` bridge to `MTU 1500`, causing packet drops or fragmentation on large TLS frames. Pass `--mtu=1420` to `dockerd` in the `dind` container args (`evidence.md:556`).
- **Activate `crates-mirror` via pod environment variables:** Inject `CARGO_SOURCE_CRATES_IO_REPLACE_WITH=velnor-crates-mirror`, `CARGO_SOURCE_VELNOR_CRATES_MIRROR_REGISTRY=sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/`, and `CARGO_HTTP_CAINFO=/opt/velnor/crates-mirror/ca-bundle.pem` into every pod spec so Cargo uses the local nginx DaemonSet even when invoked from `/tmp` with a custom `CARGO_HOME` (`RESULT.md:131`).

### 4. Kubernetes (kubelet reservations, CPU manager, priority classes)

- **Kubelet flags (`/etc/rancher/k3s/config.yaml`):** Set `system-reserved=cpu=2,memory=4Gi,ephemeral-storage=20Gi`, `kube-reserved=cpu=2,memory=4Gi,ephemeral-storage=10Gi`, `eviction-hard=memory.available<4Gi,nodefs.available<5%,imagefs.available<5%,nodefs.inodesFree<5%`, `cpu-manager-policy=static`, `topology-manager-policy=best-effort`, `pod-pids-limit=32768`, and `image-gc-high-threshold=90`.
- **PriorityClasses:** Add `velnor-gate-critical` (`1000000`) for `Plan` and `Required`, `velnor-ci-build` (`100000`) for active build/test pods, and `velnor-standby-warm` (`1000`) for pre-warmed standby pods.

### 5. Build-specific tuning (linker, `CARGO_BUILD_JOBS`, profiles)

- **Linker (`mold@2.42.0` / `rust-lld`):** Set `MBX_LINKER=mold@2.42.0` (with `mold` 2.42.0 in `/opt/velnor/seed`), cutting link times for the 350+ test binaries by 3x to 5x over GNU `ld.bfd`.
- **`CARGO_BUILD_JOBS` and `limits.cpu`:** Set `limits.cpu: "16"`, `CARGO_BUILD_JOBS=16`, and `NEXTEST_TEST_THREADS=16` on heavy build/test pods, and remove `--config build.jobs=2` from `mise.toml:90-193` and `compiler_process_budget = 2` from `.velnor/config.toml:26`. This fixes both the 2-thread starvation in `maintenance-helpers` and the 96-thread cgroup over-spawning in uncapped pods.
- **Dependency debuginfo (`debug = 0`):** Set `[profile.dev.package."*"] debug = 0` and `[profile.test.package."*"] debug = 0` in `Cargo.toml` (keeping `debug = "line-tables-only"` on workspace crates), shrinking compiled artifacts by **43%** (`docs/proposed/compiled-state-persistence.md:160`) and cutting disk I/O and link time by 25% to 40%.

### Ranked table of host, cluster, and build changes (by gain per effort)

| Rank | Change | Expected gain (with evidence / arithmetic) | Effort | Risk | Rollback command / procedure |
|---:|---|---|---|---|---|
| **1** | **Activate node-local `crates-mirror` via pod env vars** (`CARGO_SOURCE_CRATES_IO_REPLACE_WITH`, `CARGO_HTTP_CAINFO`) and fix `/home/runner/work` symlink + `ImageOS` match | Eliminates 435 to 522 crate downloads per job; `cargo fetch` drops from 3.5 to 6.1 s down to 0 to 2.7 s (`RESULT.md:134`) and stops `maintenance-helpers` offline failures | 30 min | Low (`crates-mirror` DaemonSet already passes 60/60 HA checks, `log.md:32`) | Remove the 3 `CARGO_SOURCE_*` / `CARGO_HTTP_CAINFO` env vars from the runner pod template. |
| **2** | **Bake Rust `1.99.1` (`clippy`, `rustfmt`), `mold@2.42.0`, `velnor-ci`, and `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`** into `/opt/velnor/seed` (`ro` bind mount) | Cuts `Prepare pinned tools` + `Prepare Rust components` + action downloads from 13 to 22 s down to **<0.5 s per job** (saves ~780 runner-min across 156 runs, `evidence.md` T9b) | 2 hours | Low (verified by `velnor-verify-restored-rust` pinned tree digest) | Revert runner image tag to `2026-11-16.2` and remove `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`. |
| **3** | **Mount `/tmp` (`$RUNNER_TEMP`) and `/dev/shm` as `emptyDir: { medium: Memory, sizeLimit: 8Gi }` (`tmpfs`)** using the idle 75+ GiB RAM per worker | Cuts Turso/SQLite fsync latency in 430 `velnor-runner-host` tests and compiler scratch I/O; saves **30 to 60 s on `velnor-runner-host` test wall time** (estimated from 430 per-test DB lifecycles) | 15 min | Low (workers use <38% of 125.5 GiB RAM under peak load, `evidence.md:513`) | Change `emptyDir.medium` from `Memory` back to `""` in the pod spec. |
| **4** | **Move kubelet `root-dir` and persistent cache slots to `/var/lib/rancher` (XFS `noatime`, `reflink=1`)** instead of `/` (`nodefs` single-disk `ext4`) | Moves all pod I/O off the 3.5 TB OS disk onto the 3.8 to 50 TB XFS data volume; enables `<0.5 s` `cp --reflink=always` target snapshots (`kubectl` stats summary proof) | 1 hour | Low (one worker at a time; pods reschedule onto the other 4 workers) | Unmount `/var/lib/kubelet` and restart `k3s-agent` on the node. |
| **5** | **Fix `CARGO_BUILD_JOBS` / `limits.cpu: "16"` (`K=6` slots/node) and remove `build.jobs=2`** from `mise.toml:90-193` and `.velnor/config.toml:26` | Cuts `Check / maintenance-helpers` compile time by **3x to 4x** (using 16 threads instead of 2) and prevents 96-thread cgroup thrashing across concurrent pods (`evidence.md` T6: 3.7 min vs 8.8 min under contention) | 30 min | Low | Restore `build.jobs=2` in `mise.toml` and remove `limits.cpu` from the pod spec. |
| **6** | **Enable `mold@2.42.0` (`MBX_LINKER=mold@2.42.0`) and `debug = 0` for registry dependencies (`debug = "line-tables-only"` for workspace)** | Shrinks compiled artifacts by **43%** (`compiled-state-persistence.md:160`) and speeds up linking 350+ test binaries by **3x to 5x** (saving 20 to 45 s per job) | 1 hour | Low (causes one cold cache generation when keys roll) | Revert `[profile.dev.package."*"]` and unset `MBX_LINKER`. |
| **7** | **Deploy site-local L2 `mbx-cache` (`jdx/mr-boxington-cache`) + `gha_cache` service** in DE (`velnor-240`) and FI (`velnor-241`) with `PreferSameZone` | Replaces 13.6 to 22.4 MB/s US Azure Blob downloads (`120 ms RTT`, 10 GB limit) with **1 to 2 GB/s LAN cache transfers** (`<0.2 ms RTT`, TB capacity) and retires the State seal workflow | 4 hours | Low (`mbx` falls back to local build if remote is unreachable) | Unset `MBX_REMOTE_URL` / `ACTIONS_CACHE_URL`; jobs fall back to L1 or cold build. |
| **8** | **Configure kubelet `cpu-manager-policy=static`, `topology-manager-policy=best-effort`, reservations, `PriorityClass`, and DinD `--mtu=1420`** | Pins build pods to whole Zen 4 CCDs (L3 cache + NUMA locality, ~10 to 15% faster compile), guarantees <1 s `Plan`/`Required` gate scheduling, and stops DinD WireGuard packet fragmentation | 2 hours | Medium (requires draining one node at a time when enabling `static` CPU manager) | Remove `cpu-manager-policy` line from `/etc/rancher/k3s/config.yaml`, delete `cpu_manager_state`, restart `k3s-agent`. |
| **9** | **Host OS tuning (`cpupower performance` governor, `THP=madvise`, `vm.dirty_ratio=25`, `inotify`/`nofile` limits)** | Eliminates Zen 4 frequency ramp lag on bursty `rustc` spawns (~5 to 8% compile speedup) and prevents disk writeback stalls and inotify exhaustion | 1 hour | Low | `sysctl --system` after removing `/etc/sysctl.d/99-velnor-ci.conf`; set governor back to `powersave`. |
| **10** | **Restripe LVM data volume (`lvcreate -i 3 -I 64k`) on `velnor-240` and `velnor-105`** | Multiplies NVMe sequential bandwidth and high-QD IOPS by **2x to 3x** on the two multi-disk DE nodes | 2 hours | Medium (wipes local containerd images on that node; re-pulled from local registry mirror) | Recreate linear LV with `lvcreate -l 100%FREE -n rancher data`. |

### What buying more hardware later would buy

Today's 5 EPYC 9454 workers provide **480 threads, 627.5 GiB RAM, and 96 TB NVMe**, which equals **30 concurrent 16-thread build/test slots**. Once a consolidated `velnor-new` PR run uses 6 to 8 slots for ~1.5 minutes (~10 to 12 slot-minutes per PR), today's 5 workers can execute **150+ PR runs per hour** with zero queue wait. Buying more worker nodes today is unnecessary. The only hardware worth buying later is **two small control-plane nodes in DE (~40 EUR/month each)** to turn the single `velnor-237` k3s server (`evidence.md:584`) into a 3-node HA embedded-etcd control plane.

## 8. Phased implementation plan and risk containment

Applying **Principle: Sequence Work into Verifiable Units**, every phase ends in a measured improvement on live `velnor-new` runs before the next phase begins.

### Phase 0: Baseline and immediate queue/cache unblocking (Days 1 to 2, on existing ARC cluster)

- **Deliverables:**
  1. Land PR `#1332` / `velnor-actions v0.1.81` to move `plan`, `ledger-cli`, `alint`, `cargo-deny`, `cargo-machete`, `jactionlint`, `required`, and `required-gate` onto `[velnor, ubuntu-26.04-scale-set]`, and fix the `/home/runner/work` -> `/home/runner/_work` symlink + `ImageOS` match (or run the State seal workflow producers on the scale set) so scale-set jobs stop missing `sources` and `state` caches.
  2. Inject `crates-mirror` env vars (`CARGO_SOURCE_CRATES_IO_REPLACE_WITH`, `CARGO_HTTP_CAINFO`) into the ARC pod spec.
  3. Bake Rust `1.99.1` (`clippy`, `rustfmt`), `mold@2.42.0`, `velnor-ci`, and `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` into `velnor-ci-runner:2026-11-19.1`.
  4. Switch pod `/tmp` (`$RUNNER_TEMP`) to `emptyDir: { medium: Memory, sizeLimit: 8Gi }`, set `limits.cpu: "16"`, `CARGO_BUILD_JOBS=16`, DinD `--mtu=1420`, and remove `build.jobs=2` from `mise.toml`.
  5. Land PR `#1333` (linear journal schema check) and PR `#1349` (cached historical journal templates), and restrict `job-image-candidate.yml` to `main` / image paths.
- **Measured numeric targets on `velnor-new`:**
  - **PR median wall time:** 71.8 min (11-18) / 26.7 min (window) -> **<= 6.5 min**
  - **Gate queue wait:** 57.0 min -> **<= 0.3 min**
  - **Runner-minutes per PR run:** 52.9 min -> **<= 28.0 min**
  - **Toolchain and Cargo sources cache hit %:** 0% -> **>= 98%**
- **Top risks, detection, and containment:**
  - *Risk:* `tmpfs` `/tmp` exceeds 8 GiB on a large crate job. *Detection:* Kubelet `Evicted` event or `No space left on device` in step log. *Containment:* Workers have 75+ GiB free RAM; raise `sizeLimit` to `16Gi` and move `MBX_CACHE_DIR` onto `/var/lib/rancher`.
  - *Risk:* Fork PR bypasses `required-gate` when moved to scale set. *Detection:* `required_gate_fork_tests.rs` in PR `#1332`. *Containment:* Keep the `required-gate-fork` hosted twin that fails closed on forks.

### Phase 1: Host, storage, and site-local L2 `mbx-cache` tuning (Days 3 to 5)

- **Deliverables:**
  1. Move `/var/lib/kubelet` onto `/var/lib/rancher/k3s/kubelet` (XFS `noatime`, `reflink=1`) on all 5 workers (one node at a time) and apply OS sysctls, `performance` CPU governor, kubelet `cpu-manager-policy=static`, and `PriorityClass` `velnor-gate-critical`.
  2. Deploy site-local L2 `mbx-cache` (`jdx/mr-boxington-cache`) in DE (`velnor-240`) and FI (`velnor-241`) with `trafficDistribution: PreferSameZone`, `MBX_REMOTE_URL`, and `MBX_LINKER=mold@2.42.0`.
  3. Set `[profile.dev.package."*"] debug = 0` (`debug = "line-tables-only"` on workspace crates).
  4. Deduplicate `velnor-runner-host` and `velnor-runner-host (local-mode)` tests into one `--features local-mode` nextest run plus one `--no-default-features` check, and skip duplicate `push`-to-`main` runs when `GITHUB_SHA` already has a green `merge_group` `Required` check.
- **Measured numeric targets on `velnor-new`:**
  - **PR median wall time:** 6.5 min -> **<= 3.5 min**
  - **Runner-minutes per PR run:** 28.0 min -> **<= 14.0 min**
  - **Duplicate `push` runner-min after `merge_group`:** 62.0 min -> **<= 0.5 min**
  - **`mbx` object cache hit % across runs:** 0% cross-run -> **>= 92%**
- **Top risks, detection, and containment:**
  - *Risk:* L2 `mbx-cache` pod restarts or becomes unreachable during a job. *Detection:* `mbx[cache]` warning line in job logs + `Cache` status check (`docs/goals.md:67`). *Containment:* `mbx` automatically falls back to local compilation on remote errors; CI never fails on a cache miss.
  - *Risk:* `ActionRunner.cs:141` `event.json` truncation race in `background: true` steps. *Detection:* Exit code 125 (`producer_event_invalid`). *Containment:* Copy `GITHUB_EVENT_PATH` once before `background: true` steps, and retire the State seal workflow once L2 `mbx-cache` is live.

### Phase 2: Workflow consolidation (`velnor-gen` / `velnor-ci`) and test sharding (Days 6 to 10)

- **Deliverables:**
  1. Complete the root workspace consolidation (`crates/velnor-*`, `brief-v1.md:67`) and switch `velnor-new` from `velnor-actions` 0.1.80's 39-job `ci.yml` to `velnor-gen` + `velnor-ci` (`docs/roadmap.md` issue `#1290`).
  2. Implement the 8-job Build-Once + 4-Shard Nextest DAG (Section 5), folding `maintenance-helpers` (`velnor-archive-guard` and `velnor-actions-freshness`) into the shared workspace build and sharded test runners, and deleting the serial `Ledger CLI` job.
  3. Narrow over-broad input globs so single-crate PRs select only affected packages and their reverse dependencies.
- **Measured numeric targets on `velnor-new`:**
  - **PR median wall time (typical single-crate PR):** **<= 1.2 min (72 s)**
  - **PR median wall time (heavy / full-workspace PR):** **<= 2.2 min (132 s)**
  - **Runner-minutes per typical PR:** **<= 4.5 min** (full run: **<= 9.5 min**)
  - **Jobs per run:** 39 -> **<= 8**
- **Top risks, detection, and containment:**
  - *Risk:* Change-aware selection misses an undeclared cross-crate file read. *Detection:* `velnor-ci` `rust-inputs-audit` (`crates/velnor-ci/src/io/audit/`) traces build/test reads via `dep-info` and fails closed on unmapped reads. *Containment:* Scheduled nightly full run + `rust-inputs-audit` gate.

### Phase 3: Replace ARC with `velnor-host` (`KubernetesRuntime`) and `velnor-node-agent` L1 reflink slots (Days 11 to 18)

- **Deliverables:**
  1. Land `KubernetesRuntime` in `crates/velnor-runner-*` (PR `#1324` sequence: `Runtime` trait, Pod UID absence proofs, pre-warmed standby pod pool, `PriorityClass`, and cache-affinity scheduler).
  2. Deploy `velnor-node-agent` DaemonSet managing `K = 6` persistent XFS reflinked slots per worker (`/var/lib/rancher/velnor-cache/slots/<trust>/<repo>/<slot>`), node-local bare git mirror (`/opt/velnor/git-mirrors`), and node-local `gha_cache` v1/v2 service.
  3. Enable `cp --reflink=always` base-to-PR slot snapshotting so same-repo PRs start from a 0-byte-transfer reflink clone of `main`'s warm `target/` and keep their own PR slot across pushes.
  4. Add `RuntimeClass: sysbox-runc` (or Kata) so DinD jobs run without `privileged: true`, and uninstall ARC via `helm uninstall`.
- **Measured numeric targets on `velnor-new`:**
  - **Pod pickup wait (`created` to `started`):** 12 to 18 s -> **<= 1.5 s**
  - **PR median wall time (typical PR):** **<= 45 s**
  - **PR median wall time (heavy / full-workspace PR):** **<= 95 s (~1.6 min)**
  - **L1 warm-slot cache hit %:** **>= 97%** (0 bytes transferred over network)
- **Top risks, detection, and containment:**
  - *Risk:* Crash or restart of `velnor-host` leaves orphan pods or locked cache slots. *Detection:* `velnor-host` startup `reconcile_gate` (`crates/velnor-runner-host/src/launch/gate.rs`) + `velnor-runner-qualification` G0 to G8 suite. *Containment:* Every Pod and slot lease carries an `ownerReference` and Pod UID precondition in the Turso journal; rollback to ARC takes 1 command (`helm install`) if qualification fails.

## Owner questions

1. **`docs/goals.md` line 16 ("Jobs are always executed by GitHub's official runner"):** Our recommended design **preserves** `docs/goals.md:16` by keeping the unmodified official `Runner.Listener` and `Runner.Worker` binary inside pre-warmed pods while replacing ARC with `velnor-host` (`KubernetesRuntime`) and `velnor-node-agent` outside the official runner. Because our measurements in Section 2 prove that replacing `Runner.Worker` with a custom Rust step interpreter would save <0.5 seconds per job at huge maintenance cost, we recommend **keeping `docs/goals.md:16` as written**. Do you confirm keeping the official runner binary inside the pod as the step executor?
2. **Retiring the State seal workflow in favor of node-local XFS reflink slots + in-cluster L2 `mbx-cache` (`docs/hard-decisions.md` Decision 1 and `docs/proposed/cache-decisions.md` Decision 2):** On our cluster, running a per-site `mbx-cache` server (`MBX_REMOTE_URL`) plus node-local XFS reflinked `target/` and `mbx` slots eliminates the 10 GB GitHub cache limit, the 60-minute `State producer` jobs, and the 16/18 failure rate of the State seal workflow. For `velnor-new`'s own CI on the cluster, do you approve turning off the State seal workflow (`compiled_state = "off"` under `velnor-actions`, and migrating to `velnor-gen` + node-local/L2 cluster cache), while keeping GitHub Actions cache export only for external consumer repositories that run on GitHub-hosted runners?
3. **Skipping duplicate `push`-to-`main` verification when `merge_group` already verified the exact `GITHUB_SHA` (`docs/hard-decisions.md` Decision 3 and `docs/goals.md:86`):** Today 120 of 159 merges run the full 39-job suite a third time on `push` to `main` on the identical commit SHA that just passed `merge_group` (`evidence.md` T7). Do you approve having `Plan` treat a `push` to `main` as already verified when a green `Required` check from `merge_group` exists on the exact same `GITHUB_SHA`, and enabling change-aware selection on `merge_group` (`docs/goals.md:86` default) backed by a nightly full run?
4. **Deduplicating `velnor-runner-host` and `velnor-runner-host (local-mode)` (`.velnor/config.toml:240-250`):** Running `cargo nextest` on `velnor-runner-host` twice (once with default features and once with `--features local-mode`) runs the 430 base tests twice on every run (11.7 to 17.7 runner-minutes total). Do you approve replacing the duplicate default-feature test run with a fast `cargo check -p velnor-runner-host` (proving the non-`local-mode` release build gate) plus one sharded `cargo nextest run -p velnor-runner-host --features local-mode` run?
5. **Restriping `/var/lib/rancher` and moving `/var/lib/kubelet` on the 5 Hetzner workers:** Moving `/var/lib/kubelet` from the single OS disk (`/` `ext4`) onto `/var/lib/rancher` (XFS `noatime`, `reflink=1`) and enabling kubelet `cpu-manager-policy=static` requires draining one worker node at a time for ~5 minutes each (and restriping LVM on `velnor-240` and `velnor-105` re-initializes `/var/lib/rancher` on those two nodes). Do you approve scheduling a rolling 1-node-at-a-time maintenance window on the cluster to apply the storage and kubelet changes in Phase 1?

# Proposal by Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon |
| Model id | `gemini-4-carbon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high (model maximum; max rejected) |
| Date / run | 2026-11-18 17:13–18:14 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

Waiting on CI/CD now dominates `velnor-new` delivery because eight small gate jobs sit in GitHub's 20-runner hosted queue for up to 57 minutes while five 96-thread Hetzner EPYC workers sit near 0% CPU.
When jobs finally reach the cluster, every pod starts cold with an unused Docker-in-Docker daemon, misses 100% of Cargo source and compiled-state caches due to hosted-versus-cluster path mismatches, recompiles shared dependencies across 29 per-crate pods under 2,592-thread CPU oversubscription, and repeats the full 39-job run three times per merge.
JavaScript and C# interpreter overhead accounts for only 2.0 to 2.5 seconds per job, so rewriting actions in Rust for CPU speed misses the real bottleneck.
Moving all CI jobs onto the cluster, finishing the single-workspace Cargo migration, removing duplicate `merge_group` and `push` runs, fixing four slow test suites, and cloning warm `target/` trees locally via XFS reflinks (`cp --reflink=always`) cuts typical push-to-merge latency from 34 to 141 minutes today down to 40 to 75 seconds at the same verification confidence.
Across four measured phases, Velnor should replace ARC and per-job DinD pods with `velnor-coordinator`, `velnor-node`, and `velnor-worker`: a Rust runner stack that pins jobs to 12-thread EPYC CCDs, runs standard jobs in warm rootless namespaces, isolates Docker and fork jobs in snapshot-restored KVM microVMs, executes unmodified `node24` actions in-sandbox, and keeps `OfficialRunner` as an instant protocol fallback.

## 1. Where the time goes today

### Re-measuring the baseline claims

Code reading and API measurements across 3,009 `ci.yml` runs from `2026-11-04` to `2026-11-18`
(evidence pack T1 to T11) plus live GitHub and cluster queries on `2026-11-18` refine several claims
from the problem statement:

| Prior claim | Re-measured fact and source | Verdict |
|---|---|---|
| PR run ~92 min median, ~67 min queue wait | Scale-set era (`11-15` to `11-18`, `n = 77` PR runs, evidence pack T2) has a **26.7 min median** and **92.3 min p90**. On `11-16` and `11-17`, PR medians were **23.2 min** and **16.0 min** (`12.6` and `5.8 min` gate queue, evidence pack T5). Only on `11-18` did PR median hit **74.7 min** (`71.8 min` in the T5 sample, with **57.0 min [79.4%]** serial gate queue) after 1,000 workflow runs saturated GitHub's 20-job hosted org limit (evidence pack T1, T5, T11). | 92 min is the window p90, and held as a median only during `11-18` hosted saturation. |
| CI runs twice per merge | Full 39-job `ci.yml` runs **three times** per merge: on `pull_request` (median `3.0` runs per branch, `45%` cancelled), on `merge_group`, and on `push` to `main`, where **120 of 159 (75.5%)** successful `merge_group` commits re-ran all 39 jobs on the identical commit SHA (evidence pack T7). In addition, ruleset `24396608` (`gh api repos/tailrocks/velnor-new/rulesets/24396608`) enforces `min_entries_to_merge_wait_minutes: 5` and `max_entries_to_build: 2`, adding 5.0 dead minutes before an unbatched PR even starts `merge_group`. | Understated: three full runs plus a 5 min merge-queue timer. |
| ~73 runner-min per run | Scale-set medians are **52.9 runner-min** on `pull_request` (`p90 63.9`), **64.6** on `merge_group` (`p90 81.0`), and **62.0** on `push` (`p90 74.3`, evidence pack T2). Across all three events, one merged PR consumes **179.5 median runner-min** (estimated sum of event medians, excluding cancelled PR runs and `source-qualification.yml`). | 73 sits between `merge_group` median and p90; per-merge cost is ~180 runner-min. |
| Cluster sat at 18% to 29% CPU | Utilisation swings from **0% when blocked on hosted gates** to **92% to 95% during bursts** (evidence pack B.2.1). At `2026-11-18T17:27Z` (measured live via `kubectl top nodes` and `gh api .../actions/runs`), all five 96-thread EPYC workers sat at **0.06% CPU (`40m` to `79m` of `96,000m`) and 0% to 2% RAM** while `merge_group` run `44357962729`, `push` run `44357950423`, and `pull_request` run `44354764249` waited on hosted `ubuntu-26.04` gates. | Confirmed idle during hosted stalls, burst-contended during fan-out. |
| State seal never succeeded because merge queue advances `main` | the State seal workflow **did** succeed on `11-18` for `generator` (`6/18` runs, 131 MB entry `58c2291a`) and `ci-helper` (`4/18` runs, 163 MB entry `31c...`, evidence pack T9c, T9d), while failing `16/18` on `root` and `velnor-runner` (`7` at `Check state tip`, `9` at `Clippy`/checks). Yet scale-set jobs still skipped `Restore state snapshot` **3,636 of 3,636 times** due to a `StateCompat` environment mismatch proved below. | Partly refuted: tip-race breaks `root`/`runner`, while a key mismatch breaks 100% of restores. |

Combining the stages above, honest push-to-merge wall time today is **33.8 min** on a quiet day
(`16.0` PR + `5.0` merge-queue wait + `12.8` `merge_group`, sum of `11-17` medians from evidence pack
T5 and ruleset `24396608`) and **141.2 min** under `11-18` contention (`71.8 + 5.0 + 64.4`).

### Seven-bucket breakdown of time and compute

| Bucket | Contended PR wall (`71.8m`, `11-18`) | Quiet PR wall (`16.0m`, `11-17`) | Runner-min / PR run (`52.9m` med) | Measured components and sources |
|---|---:|---:|---:|---|
| **1. Queueing** | **57.0 min (79.4%)** | **5.8 min (36.3%)** | `0.0` | Three serial hosted gates on `ubuntu-26.04`: `Plan` (`21.6m` med on `11-18`), `Required verdict` (`23.3m`), `Required` (`15.3m`, evidence pack T5). All 8 hosted jobs cost only **3.78 runner-min** combined (`#1305` comment `6980386757`). Scale-set jobs queue **12 s median (`0.2m`), 18 s p90** (`n = 3,944`, evidence pack T4). |
| **2. Pod and step setup** | **~0.5 min (0.7%)** | **~0.5 min (3.1%)** | **~9.0** (`17%`) | Per scale-set job (measured from job `134779761482` millisecond log): `Set up job` `4.88 s` (`4.17 s` sequentially downloading 6 uncached JS action tarballs, `0.66 s` `runnerresolve`, `0.045 s` C# init), `Checkout` `2.77 s`, `Setup Mise` `1.56 s`, `Acquire Velnor` `1.09 s`, `Download plan` `2.63 s`, `Download ledger` + `Upload reports` `6.73 s`, paid across ~31 scale-set jobs. |
| **3. Tool and crate downloads** | **~0.3 min (0.4%)** | **~0.3 min (1.9%)** | **~8.5** (`16%`) | Rust `1.99.1` is omitted from `velnor-ci-runner:2026-11-16.2` ([state/RESULT.md:115](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L115)) and `Restore Mise tools` is omitted on scale-set jobs ([crates/velnor-actions-workflow-renderer/src/cache_p08_runtime_identity.rs:230-240](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-workflow-renderer/src/cache_p08_runtime_identity.rs#L230-L240)), costing `12.2 s` per Rust job (`~5.9` runner-min/run, `706` runner-min in sample, evidence pack T9b). Cargo sources miss `48/48`, fetching `435` to `522` `.crate` archives from `crates.io` (`4.12 s` in job `134779761482`) while the in-cluster `crates-mirror` sits unused. |
| **4. Cache transfer** | **~0.02 min (<0.1%)** | **~0.02 min (0.1%)** | **~0.5** (`1%`) | `Restore Cargo sources` 404s in `0.62 s`/job; `Restore state snapshot` skips `3,636/3,636` (`0 s`); `MBX_REMOTE_URL` is empty `68/68` (evidence pack T9a, T9b). When remote `target/` caching was tested on `velnor-new` in `#585` (comment `6856187529`), restoring **1.92 GB** over transatlantic HTTPS (`13.6` to `22.4 MB/s`, [state/log.md:31](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/log.md#L31)) took **44 to 61 s** and saving took **24 to 28 s**. |
| **5. Compilation, lint, and doc** | **~2.0 min (2.8%)** | **~2.5 min (15.6%)** | **~17.5** (`33%`) | Cold compilation of `104` to `497` crates across 29 isolated `emptyDir` pods (evidence pack T9a). `Build test executables` takes `81 s` med (`94.7 s` in `134779761482`, storing `7.0 GiB` in local `mbx`), followed by `Clippy` (`70` to `77 s`) and `Documentation` (`60` to `66 s`) recompiling `124` to `155` crates. `Check / maintenance-helpers` compiles 104 crates three times (`dev`, `release`, examples) in `181 s` with `--config build.jobs=2` (`#1305` comment `6980497787`). |
| **6. Test execution** | **~6.5 min (9.1%)** | **~6.0 min (37.5%)** | **~17.4** (`33%`) | Dominated by four jobs (evidence pack T10): `velnor-runner-host` (`192 s` med, `338.2 s` in `134779761482`), `velnor-runner-host (local-mode)` (`246 s` med), `velnor-actions-cli` (`238 s` med + `89 s` fixture prep), and `Check / maintenance-helpers` (`252 s` for 54 tests). All other crates test in `1` to `78 s`. |
| **7. Duplicate work** | *(inside 2 to 6)* | *(inside 2 to 6)* | **~24.0 intra-run** (`~45%`) | Intra-run: `velnor-runner-host (local-mode)` rebuilds 448 crates and reruns all 348 default tests just to add 2 `local-mode` binaries (`5.5` to `9.2` runner-min); 29 crate pods repeat setup and compile overlapping crates from 5 `Cargo.lock` files (`1,160` external lock entries vs **527** unique `crate@version` pairs, measured via lockfile scan at `b4a1357ceb`). Inter-run: `merge_group` and `push` repeat all 39 jobs. |

### Why scale-set caches miss 100% of the time in code

Both cache failures stem from one architectural split:
[crates/velnor-actions-contract/src/workflow/lanes_placement.rs:27-29](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-contract/src/workflow/lanes_placement.rs#L27-L29)
and
[crates/velnor-actions-workflow-renderer/src/render_state_seal.rs:278-311](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-workflow-renderer/src/render_state_seal.rs#L278-L311)
force `Plan` and all 17 the State seal workflow producer/sealer jobs onto hosted `ubuntu-26.04`
(`RUNNER_TEMP=/home/runner/work/_temp`, `GITHUB_WORKSPACE=/home/runner/work/velnor-new/velnor-new`,
`ImageOS=ubuntu26`), whereas verification jobs run on `[velnor, ubuntu-26.04-scale-set]`
(`RUNNER_TEMP=/home/runner/_work/_temp` using `_work` instead of `work`,
`GITHUB_WORKSPACE=/home/runner/_work/velnor-new/velnor-new`, `ImageOS=velnor-arc-ubuntu24` per
[state/values.yaml:20-21](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/values.yaml#L20-L21)).

1. **Cargo sources (`48/48` misses).** `@actions/cache` v6.1.0 computes
   `version = sha256(resolved_paths + compression)` and filters strictly by `entry.version == version`
   before key matching
   ([crates/velnor-cache-rules/src/lookup.rs:28-32](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-cache-rules/src/lookup.rs#L28-L32)).
   Hosted `sources-producer` saves `${{ runner.temp }}/velnor/cargo/...` under
   `/home/runner/work/_temp`, so hosted `Plan` hits `10/10` while scale-set jobs compute `version`
   over `/home/runner/_work/_temp` and miss `48/48`.
2. **State snapshots (`3,636/3,636` skips).**
   [crates/velnor-actions-contract/src/workflow/cache_family/state.rs:224-227](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-contract/src/workflow/cache_family/state.rs#L224-L227)
   emits `ImageOS=${ImageOS-}`, `store=$RUNNER_TEMP/velnor/mbx`, and `checkout=$GITHUB_WORKSPACE`.
   Because none starts with `$`,
   [crates/velnor-actions-workflow-renderer/src/state_fragments.rs:15-21](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-workflow-renderer/src/state_fragments.rs#L15-L21)
   wraps them in double quotes so the shell expands them into `StateCompat`. Even when
   the State seal workflow succeeded for `generator` and `ci-helper` on `11-18`, scale-set jobs computed a
   different `compat` hash from hosted `Plan`, matched zero attested keys, and skipped restore. Note
   that open PR `#1332` moves `ci.yml` gates to the scale set while keeping the State seal workflow hosted,
   which leaves this 100% miss intact until `velnor-gen` replaces both workflows.

## 2. Evaluation of the hypotheses

Applying `Explain the Number` (every speedup claim is bounded by the measured time of the component it
replaces), here is the verdict on each hypothesis.

### Hypothesis 1: Rewriting the runner and JS actions in Rust speeds up CI dramatically because native code beats interpreted code

- **Verdict: Fails** on the "native CPU beats interpreted JS/C#" mechanism. **Partly holds** only
  where native runner data paths avoid network round-trips and archive downloads, saving **~12 to 15 s
  per job** rather than minutes.
- **Evidence and expected numbers.**
  - Millisecond log analysis of job `134779761482` (`481 s` total duration, measured via
    `gh api .../jobs/134779761482/logs`) shows C# `Runner.Worker` local directory init takes **45.4
    ms** (`11:48:15.361Z` to `15.407Z`), and Node24 V8 cold boot plus JS setup in
    `actions/checkout@v7` up to `Syncing repository` takes **87.3 ms** (`11:48:20.305Z` to `20.393Z`).
  - Across all 7 `node24` step spawns in that job (`Checkout`, `Setup Mise`, `Download plan`,
    `Restore Cargo sources`, `Set up MBX`, `Download ledger`, `Upload reports`, plus `Post` hooks),
    total step time is **16.65 s**, of which **~14.2 s** is git/HTTPS network I/O (`120 ms` RTT to US
    GitHub endpoints) and **~2.0 to 2.5 s** (~300 ms per spawn) is Node24 startup and JS execution.
  - Cutting JS and C# CPU execution to zero saves **2.0 to 2.5 s out of 481 s (0.5%)** on the job and
    **<0.3%** on a `16 to 72 min` PR run.
  - `velnor-legacy` tested this exact premise by reimplementing 26 JS actions in Rust inside
    [crates/velnor-runner/src/executor.rs:8477-12950](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/executor.rs#L8477-L12950)
    (`37,028` LOC) and isolating remaining JS actions in `node:24-bookworm` sidecar containers
    ([crates/velnor-runner/src/container.rs:806-961](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/container.rs#L806-L961)).
    That split broke tool-installing actions
    ([crates/velnor-runner/src/action.rs:410-434](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/action.rs#L410-L434)),
    required a fallback to the official runner, and produced **0 accepted CI speedup**
    ([evidence.md](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md) E.7).
  - **Expected gain from what actually matters.** Caching action tarballs locally saves **4.17 s/job**
    (`Set up job` sequential downloads), and serving git objects, caches, and artifacts from local
    NVMe saves **~8 to 11 s/job** of WAN transfers (**~12 to 15 s total** on the critical path once
    jobs are consolidated).

### Hypothesis 2: A from-scratch, Rust-only, GitHub-compatible runner beats reusing or wrapping GitHub's official runner and ARC

- **Verdict: Partly holds.** Replacing ARC and per-job DinD pod creation with Velnor's own Rust
  orchestrator wins decisively (**~10 to 16 s** faster startup plus warm-cache node routing).
  Replacing `actions/runner` (`Runner.Listener` + `Runner.Worker`) inside the sandbox yields a
  smaller, genuine **~2.5 to 4.5 s** startup gain, provided the Rust worker runs standard `node24` JS
  actions in-sandbox instead of cloning marketplace actions in Rust.
- **Evidence and expected numbers.**
  - **Why ARC loses (`12 s` med, `18 s` p90, `270 s` max queue, evidence pack T4).**
    - Code reading of
      [controllers/actions.github.com/ephemeralrunnerset_controller.go:323-357](file:///Users/donbeave/.cache/ci-ambition/refs/arc/controllers/actions.github.com/ephemeralrunnerset_controller.go#L323-L357)
      shows ARC **defers scale-up for two reconcile cycles** whenever finished `EphemeralRunner`
      objects exist, batches creates at `runnerBatchConcurrency = 8` (`:68`), and runs 4 sequential
      K8s/GitHub calls per runner
      ([controllers/actions.github.com/ephemeralrunner_controller.go:285-406](file:///Users/donbeave/.cache/ci-ambition/refs/arc/controllers/actions.github.com/ephemeralrunner_controller.go#L285-L406)).
    - Live pod timing (`kubectl get pods -n arc-runners -o json`, `n = 4` pods at `2026-11-18T17:27Z`)
      shows **6 to 7 s** from `PodScheduled` to `runnerStarted` on every pod: **1 to 2 s** in
      `init-dind-externals` copying Node24
      ([charts/gha-runner-scale-set/templates/_helpers.tpl:250-261](file:///Users/donbeave/.cache/ci-ambition/refs/arc/charts/gha-runner-scale-set/templates/_helpers.tpl#L250-L261))
      plus **3 to 4 s** waiting on `dind`'s `startupProbe` because `periodSeconds: 5`
      ([charts/gha-runner-scale-set/templates/_helpers.tpl:276-284](file:///Users/donbeave/.cache/ci-ambition/refs/arc/charts/gha-runner-scale-set/templates/_helpers.tpl#L276-L284))
      sleeps 5 s after the `t = 0` probe misses `dockerd` startup, even
      though 28 of 31 jobs never use Docker.
  - **Why wrapping `actions/runner` via hooks loses.** `ContainerHookManager`
    ([src/Runner.Worker/Container/ContainerHooks/ContainerHookManager.cs:38-131](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Container/ContainerHooks/ContainerHookManager.cs#L38-L131))
    requires `container:` jobs (`ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER=true`), breaks `docker build`
    and Testcontainers, adds a subprocess spawn per step, and retains 100% of `Runner.Listener` and
    `Runner.Worker` startup.
  - **What replacing `actions/runner` saves.** In
    [src/Runner.Listener/JobDispatcher.cs:395-406](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Listener/JobDispatcher.cs#L395-L406),
    `Runner.Listener` **blocks synchronously on `POST /renewjob` before spawning `Runner.Worker`**,
    boots a second .NET CLR process over anonymous pipes (serializing `AgentJobRequestMessage` three
    times, [src/Runner.Worker/Worker.cs:62-101](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Worker.cs#L62-L101)),
    waits up to `1,000 ms` on a `500 ms` timer when OIDC is enabled
    ([src/Runner.Worker/JobRunner.cs:214-222](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobRunner.cs#L214-L222)),
    downloads actions at concurrency = 1
    ([src/Runner.Worker/ActionManager.cs:235-270](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/ActionManager.cs#L235-L270)),
    and spawns `node24` synchronously for `hashFiles()`
    ([src/Runner.Worker/Expressions/HashFilesFunction.cs:73-111](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Expressions/HashFilesFunction.cs#L73-L111)).
    A single static Rust worker removes **~2.5 to 4.5 s** of startup latency per job.

### Hypothesis 3: Kubernetes is the right base because it schedules and scales pods across all hardware

- **Verdict: Partly holds** as the cluster base for long-lived daemons, networking, secrets, and
  multi-org namespaces. **Fails** if `kube-scheduler` and per-job Pod creation sit in the hot
  execution path.
- **Evidence and expected numbers.**
  - Current k3s pod scheduling (`requests: 2 CPU`, no CPU limit, `cpuManagerPolicy: none`, evidence
    pack B.1.3, B.2.3) let individual pods burst to **83.7 of 96 cores** (evidence pack B.2.2).
    Because `cpu.max` is unlimited, `cargo`, `mbx`, and `nextest` (`test-threads = "num-cpus"`,
    [.config/nextest.toml:9](file:///Users/donbeave/Projects/tailrocks/velnor-new/.config/nextest.toml#L9))
    each see `96` CPUs, spawning up to
    `9 pods * 3 background steps * 96 threads = 2,592 runnable threads` on one node and slowing
    `velnor-runner-host` **2.4x**, from **3.7 min** (`11-16`) to **8.8 min** (`11-18`, evidence pack
    T6).
  - `kube-scheduler` has no live index of per-branch XFS snapshots, lacks per-container `pids_limit`
    and tmpfs inode caps ([PR #1324](https://github.com/tailrocks/velnor-new/pull/1324),
    `crates/velnor-runner-host/src/container_engine/kubernetes.rs:263-266`), and
    adds `6 to 16 s` per cold pod.
  - **Expected gain.** Keeping k3s to run `velnor-coordinator`, one `velnor-node` DaemonSet per
    worker, `crates-mirror`, and site CAS while `velnor-node` spawns warm sandboxes directly cuts
    dispatch latency from **12 to 18 s** down to **<0.2 s**.

### Hypothesis 4: Local caches on controlled hardware with server-aware placement beat remote caches

- **Verdict: Holds strongly.** Once hosted queueing and duplicate runs are removed, node-local
  Copy-on-Write caching is the largest build-speed win.
- **Evidence and expected numbers.**
  - On `velnor-new` in `#585` (comment `6856187529`), restoring a **1.92 GB** remote cache entry
    (`14.7 GB` extracted) took **44 to 61 s** and saving took **24 to 28 s**, making a `warm` run
    (**345 s**) slower than a `cold` run (**291 s**).
  - Our five workers hold **95.8 TB** of XFS NVMe
    ([evidence.md](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md) B.1.4) versus GitHub's
    **10 GB** limit (**9,580x** larger). Cloning an uncompressed 14.7 GB `target/` tree (~45,000 files) on local
    XFS via `cp --reflink=always -a` (`FICLONE` ioctl, zero data blocks copied) takes **0.4 to 1.2 s**
    (estimated at 40k to 100k metadata extents/s on Linux 6.12 NVMe, **40x to 100x faster** than
    remote restore), keeps `.rlib`/`.rmeta` pages warm in 125.5 GiB RAM, and lets plain `cargo` +
    `velnor-ci inputs restore` compile **0 unchanged crates in <1 s** (`#585` warm result: `0` crates
    in `0.3 s`).

### Hypothesis 5: Workflows can skip unaffected work and run only required verification at maximum speed

- **Verdict: Holds strongly.** Workflow and ruleset changes deliver the largest absolute drop in
  push-to-merge latency and cluster load.
- **Evidence and expected numbers.**
  - Moving the 8 hosted jobs (`3.78` runner-min) to the cluster removes **57.0 min** of `11-18` queue
    wait (evidence pack T5).
  - Skipping `push` test re-runs on `main` when `merge_group` passed on the same SHA removes **62.0
    median runner-min** per merge (evidence pack T2, T7).
  - Setting `min_entries_to_merge_wait_minutes = 0` removes **5.0 min** of idle merge-queue sleep.
  - Consolidating 5 workspaces (`1,160` lock entries) into 1 workspace (`527` unique external crates),
    running affected-only `merge_group` diffs relative to green `main`
    ([docs/goals.md:86](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L86)),
    filtering `local-mode` to its 2 specific test binaries (`saves 5.5 to 9.2` runner-min), and fixing
    + LPT-sharding the 4 slow test suites brings typical PR wall time from **16 to 75 min** down to
    **24 to 38 s** (arithmetic in Section 5).

## 3. Recommended runner architecture

Guided by `Attack the Premise`, `Foundational Thinking`, and `Laziness Protocol`, Velnor should
**accelerate control and data planes in native Rust while executing unmodified `node24` JavaScript
actions inside the job sandbox**, avoiding `velnor-legacy`'s 37k-line native-action trap.

### External GitHub compatibility contract

1. **Scale-set control plane (`api-version=6.0-preview`).** `velnor-coordinator` authenticates with
   the org-level GitHub App, exchanges `RemoteAuth` registration tokens for scale-set admin JWTs,
   opens the long-poll `/_apis/runtime/runnerscalesets/{id}/sessions` queue (`X-ScaleSetMaxCapacity`),
   and calls `/acquirejobs` and `/generatejitconfig` concurrently
   ([crates/velnor-runner-github/src/lib.rs:12-54](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner-github/src/lib.rs#L12-L54)).
2. **Execution and reporting plane (`v2.338.0` wire contract).**
   - `velnor-worker` receives the decoded JIT identity in memory over a one-shot pipe/vsock (never
     persisting `.credentials_rsaparams` to workspace disk), mints the RS256 OAuth assertion, calls
     `POST {run_service_url}/acquirejob`, starts async `POST {vss_url}/renewjob` every 45 s **in
     parallel** with job setup, and reports final status via `POST {vss_url}/completejob`
     ([src/Sdk/RSWebApi/RunServiceHttpClient.cs:58-184](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs#L58-L184)).
   - Because 100% of GitHub.com scale-set jobs arrive as V2 `RunnerJobRequest`, `velnor-worker`
     follows `resultsServiceOnly: true`
     ([src/Runner.Worker/JobRunner.cs:85](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobRunner.cs#L85)):
     it omits legacy AzDO timeline calls, posts step states to Twirp
     `WorkflowStepUpdateService/WorkflowStepsUpdate`, streams console lines over `FeedStreamUrl`
     WebSocket, and uploads step/job logs in 4 MiB streaming chunks via
     `Receiver/GetStepLogsSignedBlobURL` + Azure `AppendBlob` (`?comp=appendblock&seal=true`) over
     `rustls` HTTP/2 pools.
   - Enforces upstream's exact environment boundary: all steps get the 39 allowlisted `GITHUB_*` and
     `RUNNER_*` variables
     ([src/Runner.Worker/GitHubContext.cs:9-50](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/GitHubContext.cs#L9-L50)),
     while `ACTIONS_RUNTIME_TOKEN`, `ACTIONS_RUNTIME_URL`, `ACTIONS_RESULTS_URL`, and
     `ACTIONS_CACHE_URL` are injected **only** into `uses:` action steps
     ([src/Runner.Worker/Handlers/NodeScriptActionHandler.cs:54-93](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs#L54-L93)),
     never into `run:` shell steps.
3. **In-sandbox `node24` and composite execution.** `velnor-worker` batch-resolves action SHAs via
   `POST .../runnerresolve/actions`
   ([src/Sdk/WebApi/WebApi/LaunchHttpClient.cs:35-85](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/WebApi/WebApi/LaunchHttpClient.cs#L35-L85)),
   reflinks pre-extracted action directories from `velnor-node`'s CAS into `_work/_actions` in
   `<20 ms`, expands `composite` steps, and runs `node24` actions using `/opt/velnor/node24/bin/node`
   inside the **same** rootfs, UID, and `PATH` as `run:` steps.
4. **Dual-engine safety valve (`EngineMode::Native | EngineMode::Official`).** Because sandbox
   creation, XFS CoW `target/` cloning, git mirror seeding, and local `CacheService` v2 wiring happen
   inside `velnor-node` *before* the worker starts, `velnor-node` can launch either `velnor-worker`
   (`Native`) or `/home/runner/run.sh` (`Official`). Any breaking GitHub wire change is contained in
   `<60 s` by toggling `engine_mode = "official"` while agents update `velnor-worker`.

### Core internal components and data model

```rust
pub enum TrustClass {
    TrustedMain,
    SameRepoPr { pr_number: u64 },
    MergeGroup,
    UntrustedFork,
}

pub enum SandboxKind {
    RootlessNamespace,
    KvmMicroVm { enable_guest_dockerd: bool },
}

pub enum EngineMode {
    NativeRust,
    OfficialRunner,
}

pub struct SnapshotKey {
    pub repo_id: u64,
    pub compat_sha256: [u8; 32],
    pub lock_sha256: [u8; 32],
    pub visibility: CacheVisibility,
}

pub struct JobDispatchSpec {
    pub job_id: String,
    pub trust: TrustClass,
    pub sandbox: SandboxKind,
    pub engine: EngineMode,
    pub ccd_count: u8,
    pub base_snapshot: SnapshotKey,
}
```

1. **`velnor-coordinator` (stateless K8s Deployment + leader Lease).** Owns Scale Set polling,
   classifies `TrustClass` via `velnor-runner-github`'s `workflow_run` verifier before dispatch,
   scores the 5 workers using live `NodeHeartbeat` state (`active_ccds`, `SnapshotKey` inventory,
   `site`), and dispatches `JobDispatchSpec` over mTLS gRPC in `<150 ms`.
2. **`velnor-node` (1 DaemonSet agent per EPYC worker, `<1.5k`-LOC `schemalane` SQLite journal).**
   Owns **7 CCD-pinned 12-thread slots** (`SlotId 0..6`, dynamically coalescable into 24-thread pairs
   for heavy builds), maintains a warm standby pool of single-use sandboxes, manages
   `/var/lib/rancher/velnor/{cas,snapshots}`, and runs the site-local Twirp `CacheService` v2 and
   `ArtifactService` v4 endpoint.
3. **Two sandbox classes (`SandboxKind`).**
   - **`RootlessNamespace` (~90% of jobs, trusted, no Docker needed).** Linux user, mount, pid, ipc,
     uts, net, and cgroupv2 container spawned via `youki` or `containerd` over a pre-reflinked rootfs
     in **`<50 ms`**, with **zero `docker:dind` sidecar**.
   - **`KvmMicroVm` (jobs needing `docker.sock` and all `UntrustedFork` jobs).** **Cloud Hypervisor or
     Firecracker** VM on `/dev/kvm`, restored in **`~100 ms`** from a memory snapshot where guest
     `dockerd` is already running and `/var/lib/docker` plus workspace sit on a `dm-snapshot` CoW
     `virtio-blk` image seeded with base container layers and read-only cache blocks. Every sandbox or
     VM is destroyed immediately on job exit and replenished asynchronously.

### Reuse and rejection matrix across the four codebases

- **Take from `velnor-new`.**
  - `velnor-runner-github` (`7,864` LOC,
    [crates/velnor-runner-github/src/poll.rs:205-308](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner-github/src/poll.rs#L205-L308)):
    transport-pure Scale Set `6.0-preview` state machines, `runnerRequestId == 0` synthetic SHA-256
    deduplication, poison-batch quarantine, and `workflow_run` PR/fork verification.
  - `velnor-runner-core` (`883` LOC), `velnor-runner-attestation` (`1,773` LOC), and
    `velnor-runner-image-manifest` (`2,569` LOC).
  - `velnor-cache-rules` (`728` LOC) and `velnor-local` (`16,384` LOC): exact GitHub Cache v2
    `(scope, key, version)` lookup engine and `runner.server` differential conformance harness.
  - `velnor-gen` and `velnor-ci` (`generator/`, `ci-helper/`): workspace planning, `#595` `Cargo.lock`
    graph diffing, `inputs save/restore` mtime normalization
    ([crates/velnor-ci/src/inputs.rs:138-240](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-ci/src/inputs.rs#L138-L240)),
    `crates-mirror` wrapper, and `report --check` cache verification.
- **Do NOT copy from `velnor-new`.**
  - `velnor-runner-host`'s **171,054-line / 1,002-file monolith**, especially `src/journal/`
    (`64,000+` lines across 382 files implementing 52 imperative SQLite migrations and 50 triggers on
    `turso`). Replace with a `<1,500`-line `schemalane`-managed journal tracking only in-flight
    `SlotLease` and sandbox UIDs.
  - `velnor-actions` (`crates/velnor-actions-*`, `322k` LOC): 1-job-per-crate YAML, `$RUNNER_TEMP`
    cache paths, `ImageOS` gates, and the State seal workflow.
- **Take from `velnor-legacy` (and lessons from its archive).**
  - `velnor-legacy` (`804k` LOC, archived `2026-10-17` after `velnor-new` took over workflow
    generation) hit severe operational incidents: wedged inert slot children (`#389`), `600% to 700%`
    idle CPU from per-slot polling daemons and 2 s `PRAGMA synchronous=FULL` writes (`#408`), V2
    `Plan.ScopeIdentifier == null` breaking trust classification (`#683`), over-strict
    `workflow_run_backend_id` validation dropping all 19 artifacts on job redelivery (`#670` to `#672`
    / PRs `#681`, `#684`), and sidecar JS actions losing installed tools.
  - **Lift cleanly.** `expression/` (`3,565` LOC shunting-yard `.NET` `Expressions2` port at
    [crates/velnor-runner/src/expression/mod.rs:1-60](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/expression/mod.rs#L1-L60),
    adding upstream's 1 MiB `MemoryCounter` and converting recursive `write_json` at
    [crates/velnor-runner/src/expression/eval.rs:532](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/expression/eval.rs#L532)
    to an explicit heap stack),
    [crates/velnor-runner/src/workflow_command.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/workflow_command.rs)
    (`1,521` LOC),
    [crates/velnor-runner/src/command_files.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/command_files.rs)
    (`481` LOC),
    [crates/velnor-runner/src/script_step.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/script_step.rs)
    (`2,160` LOC), parallel `rayon` `hashFiles()`
    ([crates/velnor-runner/src/executor.rs:16189-16445](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/executor.rs#L16189-L16445)),
    the 11-encoder Aho-Corasick `Masker`
    ([crates/velnor-runner/src/runner.rs:13166-13397](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/runner.rs#L13166-L13397),
    instantiated fresh per job so `OpsSink` never leaks across jobs), and `RunService`/`Twirp` wire
    types from
    [crates/velnor-runner/src/protocol.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/protocol.rs).
- **Do NOT copy from `velnor-legacy`.**
  - `executor.rs`'s 26 native action clones (`37k` LOC), `node:24-bookworm` sidecars,
    [crates/velnor-runner/src/docker_lease.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/docker_lease.rs)'s
    `21.6k`-LOC Docker socket proxy over a shared host `dockerd`, `curl` subprocess spawns for TLS
    fingerprinting
    ([crates/velnor-runner/src/protocol.rs:1449](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/protocol.rs#L1449)),
    [crates/velnor-runner/src/gha_cache.rs](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/gha_cache.rs)'s
    `17.1k`-LOC custom 5-tier file-lock hierarchy, or Firecracker's empty cache transport
    ([crates/velnor-runner/src/execution/backend.rs:129-137](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/execution/backend.rs#L129-L137)).
- **Take vs reject from `actions/runner` and `ARC`.**
  - Copy wire protocol rules only (`resultsServiceOnly`, `LaunchHttpClient`, 39-key env split,
    `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` in Phase 1). Reject ARC's 4-CRD controller chain,
    finished-runner scale-up deferral, universal DinD + `5 s` `startupProbe`, `.NET` dual-process pipe
    handoff, synchronous pre-worker `renewjob` barrier, and single-threaded action tarball downloads.

## 4. The cache system

Following `Separate Before Serializing Shared State`, concurrent jobs never share a mutable directory
or lockfile (avoiding `velnor-legacy`
[crates/velnor-runner/src/mbx_store.rs:4-20](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/mbx_store.rs#L4-L20)'s
144 GiB per-slot blowup). All shared cache tiers on `/var/lib/rancher/velnor` are **read-only
immutable directories (`0555`)**, and each job receives an isolated writable workspace via XFS
`FICLONE` reflinks.

### What is cached, where it lives, and expected hit rates

1. **Toolchains, CLI tools, and helper binaries (`L0` node-local read-only CAS, `100%` hit rate,
   `<10 ms`).**
   - Stored under `/var/lib/rancher/velnor/cas/tools/<sha256>` on every worker and bind-mounted
     read-only at `/opt/velnor`: Rust `1.99.1` (`rustc`, `cargo`, `clippy`, `rustfmt`, `rust-lld`),
     `mold`, all pinned `mise` tools (`cargo-nextest`, `cargo-deny`, `cargo-machete`, `jactionlint`,
     `shellcheck`, `bun`, `opentofu`, `crane`, `cosign`, `gh`), `node24`, and `velnor-ci`. Eliminates
     the **12.2 s/job** Rust install tax across all Rust jobs.
2. **GitHub Action archives (`L0` node-local read-only CAS, `>99.5%` hit rate, `<20 ms`).**
   - Stored unpacked under `.../cas/actions/<owner>/<repo>/<40-hex-sha>`, populated single-flight on
     miss, and reflinked into `_work/_actions` before Step 1. Eliminates the **4.17 s/job** sequential
     tarball download in `Set up job`.
3. **Git repository objects and Cargo crate sources (`L1` node-local mirrors + CAS, `>99%` hit rate,
   `0.15 to 0.30 s`).**
   - Bare Git mirror `.../cas/git/<repo_id>.git` updates single-flight (`~150 ms` delta fetch) and
     seeds `$GITHUB_WORKSPACE/.git` via reflinked packfiles with `persist-credentials: false`.
   - Extracted `.crate` directories in `.../cas/cargo-src/<lock_sha>` reflink into
     `~/.cargo/registry`, backed by the existing node-local `velnor-system/crates-mirror` nginx
     DaemonSet (`PreferSameNode`, 200 GB cache,
     [state/RESULT.md:132](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L132))
     for newly added crates.
4. **Build artifacts: Cargo `target/` + `inputs.tsv` (`L1` node-local XFS CoW snapshots + `L2`
   same-site LAN CAS, `92% to 96%` hit on 1st PR push, `>96%` on 2nd+ PR push, `0.5 to 1.2 s`
   restore).**
   - Immutable directory snapshots live at
     `/var/lib/rancher/velnor/snapshots/<repo_id>/<scope>/<compat_hash>/<commit_sha>/target` alongside
     `target/.velnor/inputs.tsv` (`(path, git_blob_sha, mtime_ns)`).
   - Before Step 1, `velnor-node` runs `cp --reflink=always -a` into `$GITHUB_WORKSPACE/target` from
     `scope = pr-<num>` (if present on that node for a `pull_request` job) or else the newest
     `scope = main` snapshot matching `compat_hash`, then runs `velnor-ci inputs restore` (`<50 ms`)
     so unchanged files keep exact pre-checkout mtimes.
   - **Site-local `L2` CAS (`DE` and `FI`).** One lightweight Rust HTTP/2 service per site speaks
     GitHub Actions Twirp `CacheService` v2 + `ArtifactService` v4 (pointed to by
     `ACTIONS_RESULTS_URL` / `ACTIONS_CACHE_URL` for Cache/Artifact RPCs while forwarding log/step
     Twirp calls to GitHub). That keeps unmodified third-party workflows using `actions/cache` and
     intra-run `upload-artifact` (such as sharded nextest binaries) running at **1 to 2 GB/s
     intra-site Hetzner LAN speed (`<0.5 ms` RTT)** instead of `14 to 22 MB/s` transatlantic WAN.
     Whenever `main` seals a new `target/` snapshot on one worker, `velnor-node` asynchronously
     streams a `zstd` delta pack to the other 4 workers in the background (`~4 s` same-site, `~12 s`
     DE-to-FI) so all 5 workers stay warm on `main`.
5. **Test results (tree-verdict receipts only, `0%` cross-change caching by design).**
   - Arbitrary test-result caching across code edits that reach a crate is **rejected** to preserve
     [docs/goals.md:36](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L36)
     Promise 3; unaffected crates already skip tests via `velnor-ci plan`. The sole safe test-result
     reuse is an **attested Tree-Verdict Receipt** keyed by
     `(git_tree_sha, workflow_sha, toolchain_sha)`, which skips re-running identical `merge_group`
     (`15.6%` today, rising to `~50%` once merges finish in ~1 min) and `push`-to-`main` (`75.5%` to
     `100%`) commits.

### Warm-node placement, trust isolation, and eviction

- **Warm-node placement.** `velnor-coordinator` scores each worker $n \in \{1..5\}$ as:
  `score(n) = 100 * has_pr_snapshot(n) + 60 * has_exact_lock_main(n) + 40 * has_compat_main(n) + 25 * same_site_as_run(n) - 18 * active_ccds(n)`.
  Repeat pushes to PR `#N` stick to the worker holding `pr-<N>` (`+100`) unless that worker has `>= 4`
  more busy CCDs than an idle worker with a warm `main` snapshot (`+60`), preventing queue hotspots
  while keeping all shards of one `run_id` in the same site (`DE` or `FI`).
- **Trust and secret boundaries
  ([docs/goals.md:44](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L44)).**
  - `compat_hash` binds
    `(OS, arch, distro_id, rustc_commit_hash, cargo_profile_flags, relative_workspace)` and **never**
    `$RUNNER_TEMP` or `ImageOS`.
  - Any `push` to `main` proven reachable from `refs/heads/main` (`ancestor-or-equal`, eliminating
    the State seal workflow's `head == tip` race in `#1305` comment `6980735625`) promotes a `0555` read-only
    `main` snapshot once compilation succeeds, even if a test step later fails.
  - Same-repo `pull_request` builds read `pr-<N>` then `main`, and write **only** to `pr-<N>` (1
    generation retained per open PR, deleted on PR close or 48 h TTL). `merge_group` reads `main` only
    and writes nothing.
  - Private dependencies (`velnor-deps` App token,
    [docs/hard-decisions.md:388-421](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/hard-decisions.md#L388-L421))
    are fetched and revoked before repository code runs, and any snapshot containing private-dep
    artifacts is tagged `visibility = internal` so public fork microVMs receive only public `main`
    block seeds (`dm-snapshot` read-only, all guest writes destroyed on VM exit).
- **Bounded disk budget.** Enforced via a `500 GiB` XFS project quota (`prjquota`) on
  `/var/lib/rancher/velnor` per worker (`<14%` of the smallest `3.8 TB` worker disk), retaining 3
  `main` snapshots per repo plus active open PR overlays.

## 5. Workflow design for speed

### Five concrete workflow and test-suite changes

1. **Finish unifying into one root Cargo workspace (`crates/velnor-*`) and cut over to `velnor-gen`
   (`#1290`).**
   - Lockfile analysis at `HEAD` (`b4a1357ceb`) counts `1,160` external package entries across the 5
     current `Cargo.lock` files (`415` root, `440` runner, `129` ci-helper, `94` generator, `82`
     local), of which **527 `crate@version` pairs are unique**. Unifying into one root workspace
     deletes **54.6% (`633`)** of duplicate dependency entries and collapses 29 per-crate `ci.yml`
     jobs (and 5 `velnor-gen` root lanes) into **one workspace pipeline**.
2. **Merge the 8 hosted gate/lint jobs into one cluster `plan-and-static` job (`t = 0`).**
   - Run `velnor-ci plan --explain`, `rust-inputs-audit`, `jactionlint`, `alint`,
     `cargo fmt --all --check`, `cargo deny`, and `cargo machete` in parallel inside a single warm
     12-thread cluster slot (`3.5 to 5.5 s` total, zero `ubuntu-26.04` queue wait).
   - Keep `velnor-ci`'s exact soundness guarantees: `#595` lockfile graph diffing (`0` missed packages
     across 50 replayed commits in `#586`), `CheckAlone` per-crate feature checks (`+1 s` median,
     `#585` M4), and fail-closed full selection on any unmapped file or tool/toolchain edit.
3. **Compile at most twice in parallel (`rust-lint` and `rust-build`), with adaptive inline-vs-LPT
   test sharding.**
   - **`rust-lint` (24 threads = 2 CCDs, CoW `target-lint/`).** Runs
     `cargo clippy --workspace --all-targets --locked -- -D warnings`, `CheckAlone` on selected
     crates, and `cargo doc --no-deps --locked` when doc-bearing crates are selected.
   - **`rust-build` (24 threads = 2 CCDs, CoW `target-test/`).** Builds selected test binaries.
     - **Inline execution (`predicted_test_s <= 25 s`).** `#585` Measurement 9 proved sharding short
       test runs adds `19 to 25 s` of archive/transfer overhead. For the ~70% of PRs whose selected
       tests take `<= 25 s` (`generator`, `ci-helper`, `velnor-local`, `velnor-runner-github`, leaf
       crates), `rust-build` executes `cargo nextest run -E "$TEST_FILTER"` directly in-slot.
     - **LPT duration-balanced sharding (`predicted_test_s > 25 s`).** Replaces
       `nextest --partition hash:i/n` (which put `6/6` slow tests into one shard in `#585` M9) with
       Longest-Processing-Time (LPT) bin-packing over `.velnor/rust-ci-timings.json` across **$K = 4$
       to $8$ same-site slots**, routing pure Rust shards to `RootlessNamespace` slots and only the
       Docker test group to `KvmMicroVm`.
4. **Fix the four slow test suites at their architectural root (keeping 100% of assertions).**
   - **`velnor-runner-host` 52-version SQLite migrations.** Land linear schema validation (`PR #1333`:
     `1,040 s` down to `644 s` on 42 tests) and historical template caching (`PR #1349`: `-45%` CPU),
     then baseline-squash the 52 migrations with `schemalane` so `Journal::open` stops replaying 51
     DDL steps across 110 test sites (`50 to 60 s` per test in `job-134779761482` drops to
     `3 to 6 s`).
   - **`runner_image_qualification_public` hardcoded sleeps.** Inject configurable timeout parameters
     into
     [crates/velnor-runner-host/tests/runner_image_qualification_public.rs:312-396](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner-host/tests/runner_image_qualification_public.rs#L312-L396)
     (`100 ms` fault deadlines in place of 8 serial `6 to 12 s` wall-clock sleeps), cutting **~95 s**
     of idle sleep to **<3 s**.
   - **`local-mode` duplicate test sweep.** `local-mode = ["tokio/signal"]`
     ([crates/velnor-runner-host/Cargo.toml:12-14](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner-host/Cargo.toml#L12-L14))
     has zero negative `cfg` gates, so build workspace tests once with `--features local-mode` (or
     filter the `local-mode` shard to `-E 'binary(local_pair_public) | binary(local_host_public)'`)
     and remove the random-nonce Dockerfile layer on static helper binaries in
     [crates/velnor-runner-host/tests/local_pair_public/support.rs:184-188](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner-host/tests/local_pair_public/support.rs#L184-L188),
     saving **5.5 to 9.2 runner-min** per run.
   - **`Check / maintenance-helpers`.** Move `velnor-archive-guard` and `velnor-actions-freshness`
     into the unified cached workspace run, deleting
     [mise.toml:90-199](file:///Users/donbeave/Projects/tailrocks/velnor-new/mise.toml#L90-L199)'s
     uncached temp `CARGO_HOME`, `--config build.jobs=2`, `--jobs 2`, and
     `threads-required = "num-test-threads"` serialization (`508 s` drops to `~20 s`).
5. **Eliminate double and triple runs across push-to-merge.**
   - Set `min_entries_to_merge_wait_minutes = 0` and `max_entries_to_build = 5` in ruleset `24396608`
     (`saves 5.0 min` wall).
   - **Tree-match fast-pass on `merge_group`.** When `merge_group` commit tree SHA equals the
     already-green PR head tree SHA, `plan-and-static` verifies the signed check receipt and passes
     `merge_group` in **~8 s**.
   - **Affected-only `merge_group` when `main` advanced.** Diff `merge_group` against its green `main`
     first parent and test **only the affected crate closure**
     ([docs/goals.md:86](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L86)
     default), backed by a **6-hourly or nightly full scheduled run** (`mode = all`).
   - **No test re-run on `push` to `main`.** When `GITHUB_SHA` already passed `merge_group`, `push`
     exits green in **~8 s** and runs only an async background `main` XFS snapshot seal. Restrict
     `job-image-candidate.yml` (`306` runs on `11-18`) from `push: {}` to `images/**` and runner-crate
     PRs plus `main`.

### Honest minimum time to verdict (estimated critical-path arithmetic)

| `velnor-new` PR class | Dispatch + `plan-and-static` | XFS CoW clone + compile/link (`24` threads, `mold`) | Test execution (inline or 6 LPT shards) | `Required` gate | **PR wall to verdict** | **Push-to-merge wall** (`min_wait = 0`) |
|---|---:|---:|---:|---:|---:|---:|
| **Docs / non-Rust edit** | `0.5s + 4.5s` | `0s` (skipped) | `0s` (skipped) | `1.5s` | **~7 to 10 s** | **~15 to 22 s** |
| **Typical leaf / 1-subsystem PR** (~70% of PRs: `generator`, `ci-helper`, `local`, leaf crate) | `0.5s + 5.0s` | `1.0s` CoW + `10 to 16s` compile/clippy | `6 to 14s` inline nextest | `1.5s` | **~24 to 38 s** | **~35 s** (tree match) to **~75 s** (`main` moved) |
| **Heavy hub / `velnor-runner-host` PR** (full workspace affected, 6 LPT shards) | `0.5s + 5.5s` | `1.2s` CoW + `18 to 26s` compile | `25 to 38s` (slowest of 6 shards) | `1.5s` | **~52 to 73 s** | **~62 s** (tree match) to **~140 s** (`main` moved) |
| **Cold `Cargo.lock` bump** (all 527 crates cold on 48 threads) | `0.5s + 5.5s` | `0.5s` mirror + `75 to 100s` compile | `30 to 40s` (6 shards) | `1.5s` | **~1.9 to 2.5 min** | **~2.1 to 4.5 min** |

## 6. Kubernetes use

### Pod lifecycle and pre-warming

- **Phase 1 and 2 (immediate K8s Pod split).** Replace the single DinD scale set with two pools:
  1. `velnor-fast` (`minRunners: 12`, **no `dind` sidecar**, **no `init-dind-externals`**, Guaranteed
     `12 CPU / 16Gi`): runs `plan-and-static`, `rust-lint`, `rust-build`, and all non-Docker test
     shards with **0 to 1 s** startup.
  2. `velnor-docker` (`minRunners: 4`, `dind` with `startupProbe.periodSeconds: 1` instead of `5`, XFS
     volume at `/var/lib/docker`): runs only Docker-requiring test shards and image candidate builds.
- **Phase 3 and 4 (`velnor-node` DaemonSet).** Kubernetes schedules exactly **one long-lived
  `velnor-node` DaemonSet pod per EPYC worker** (plus `crates-mirror` and site CAS). `velnor-node`
  pre-warms 7 CCD-aligned 12-thread slots per node (`35` slots cluster-wide, or `15` 24-thread + `5`
  12-thread slots): rootless namespaces (`<50 ms`) for standard jobs and snapshot-restored KVM
  microVMs (`~100 ms` with guest `dockerd` already booted) for Docker and fork jobs. **No sandbox ever
  runs two jobs:** on exit, its CoW filesystem or VM overlay is destroyed in `<50 ms` and replaced
  from the immutable seed in the background.

### Node affinity, untrusted PR isolation, and multi-site scaling

- **Site topology and affinity.** Label `velnor-240` and `velnor-105` as
  `topology.kubernetes.io/region=fsn1` (`DE`: `192` threads, `84.5 TB` NVMe) and `velnor-241`,
  `velnor-5`, and `velnor-190` as `topology.kubernetes.io/region=hel1` (`FI`: `288` threads, `11.5 TB`
  NVMe). `velnor-coordinator` co-locates all shards of a run within one site so nextest archives and
  L2 cache hits stay on `<0.5 ms` Hetzner site LANs instead of crossing the `35 ms` DE-to-FI WireGuard
  link.
- **Untrusted PRs and multi-org isolation.** The cluster serves `tailrocks`, `ChainArgos` (15 private
  repos), and `jackin-project`
  ([state/RESULT.md:84-89](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L84-L89)).
  Fork PRs run strictly inside **KVM microVMs** (`/dev/kvm` AMD-V hardware isolation, separate guest
  kernel, `automountServiceAccountToken: false`, `NetworkPolicy` dropping `10.42.0.0/16`,
  `10.43.0.0/16`, `169.254.169.254`, and `5.9.55.237:6443`, read-only `dm-snapshot` public cache seed,
  zero tokens). Until Phase 3 KVM lands, keep fork PRs fenced off privileged DinD workers via PR
  `#1332`'s hosted gate twin (plus `all_external_contributors` approval).
- **What buying more servers would buy.** Today's 480 EPYC threads easily absorb `velnor-new`
  (`~96 to 120` peak threads for `40 s` per heavy run) with >70% headroom for `ChainArgos` and
  `jackin-project`, so **more worker servers buy 0 s of single-PR latency today**. Buying **two small
  Hetzner nodes (~10 EUR/mo each)** buys **3-node etcd HA for `velnor-237`** (currently a single point
  of failure, [README.md:12](file:///Users/donbeave/Projects/tailrocks/velnor-runners/README.md#L12));
  additional EPYC workers only pay off once concurrent multi-org demand regularly exceeds ~25
  simultaneous workspace builds.

## 7. Role of mbx and blaze

### `blaze` does NOT fit `velnor-new`'s pinned toolchain

Primary source inspection of `indexable-inc/blaze`
(`gh api repos/indexable-inc/blaze/contents/docs/{toolchain,usage,v2/plan}.md`) and `velnor-new`
experiment `#652` (comment `6874232403`, recorded in `#586`) rules `blaze` out for CI gates:

1. **Toolchain incompatibility.** `blaze` is a standalone incremental Rust compiler (built on
   `rust-analyzer` HIR/MIR, Cranelift/LLVM `23.1.4`, and DBSP) that **ships its own daily Rust
   nightly** (`blaze-2026-10-27` / `rustc 1.101.0-nightly`) and **ignores `rust-toolchain.toml`,
   `rustup`, and `RUSTUP_TOOLCHAIN`** (`docs/toolchain.md:12-26,262-265`). Its own docs state: *"Keep
   `cargo build`/`cargo test` with the project's toolchain in CI. blaze is for the edit loop; the
   project's toolchain decides"* (`docs/toolchain.md:272-273`). Adopting it in CI would break
   [docs/goals.md:36](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L36) Promise
   3 and
   [docs/hard-decisions.md:232-265](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/hard-decisions.md#L232-L265)
   (`Rust 1.99.1`).
2. **Missing gates and partial borrow checking.** `blaze` fails with `no such command` on `clippy`,
   `fmt`, and `doc` (`docs/usage.md:320-324`), does not run `cargo nextest` (`#652` run
   `43535845704`), and notes its borrow checker catches *"about two thirds of rustc's borrow errors"*
   (`docs/usage.md:401-403`).
3. **Measured ephemeral CI regression (`#652`).** On `velnor-lab` (`blaze v0.20261105.4`),
   `blaze-signal.yml` took **248 s (x64)** and **319 s (arm)** on a second leaf push versus plain
   cargo's **38 s** (`6.5x` and `8.4x` slower, `14.4 GB` RSS), and `blaze-signal-state.yml` failed **8
   of 8** restored runs (`0` tests built) because `blaze` counted restored XFS page cache (`14.4 GB`)
   as memory used by others.
4. **Sensible scope.** Keep `blaze` out of CI. Its only role is optional local `blaze serve` daemons
   on agent development machines for `<200 ms` edit-loop type feedback before pushing.

### Role of `mbx` (Mr. Boxington)

In `#585` (comment `6856187529`, recorded in `#586`), plain `cargo` + `velnor-ci inputs restore` beat
all 9 `mbx` configurations (`v1.29.2` and `v1.30.0`):

- On `warm` runs, **plain `cargo` compiled `0` crates in 3 of 3 runs (`0.3 s`)**, whereas **every
  `mbx` variant recompiled `10 to 68` crates** (`V8` `MBX_SHARE_OUT_DIR=0` best at `10`, default `14`,
  caused by external `OUT_DIR` paths triggering `21 StaleDepFingerprint` cascades plus unnormalized
  checkout mtimes), and `mbx` compile counts agreed with Cargo's log in only **1 of 163 runs**.
- **Recommendation.** Retire `mbx cache export/import` bundles (the State seal workflow) and run **plain
  `cargo` directly over node-local XFS CoW `target/` clones + `velnor-ci inputs restore`** (matching
  `velnor-gen`
  [crates/velnor-rust/src/compat.rs:9](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-rust/src/compat.rs#L9)).
  Because a 48-thread EPYC node cold-builds all 527 external crates in `~80 to 100 s` only on rare
  `Cargo.lock` bumps, an L2 compiler cache (`sccache` or `mbx-cache` with `MBX_SHARE_OUT_DIR=0`)
  affects `<5%` of runs; test `sccache` vs `mbx-cache` (`MBX_SHARE_OUT_DIR=0`) on `velnor-lab` only
  for cold `lock` scenarios, adopting one only if warm CoW builds still compile **0 crates**.

## 8a. Servers and cluster tuning

### Read-only cluster measurement (`2026-11-18`)

Measured via read-only `kubectl get/describe/top`, `/proxy/configz`, and `/proxy/stats/summary`
(evidence pack B and live checks) plus
[PLAN.md:44-80](file:///Users/donbeave/Projects/tailrocks/velnor-runners/PLAN.md#L44-L80) and
[state/inventory.md:1-57](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/inventory.md#L1-L57):

- **Nodes.** 5 workers (`4x EPYC 9454P`, `1x EPYC 9454`: 48 Zen 4 cores / 96 threads, **8 CCDs x 6
  cores [12 threads] x 32 MB L3 = 256 MB L3**, `125.5 GiB` allocatable RAM) + `velnor-237` control
  plane (`i5-13500`, 20 threads, `62.6 GiB` RAM), all running Debian 13 (`trixie`), kernel
  `6.12.94+deb13-amd64`, `k3s v1.37.2+k3s1`, `containerd 2.3.7-k3s1`.
- **Observed configuration defects.**
  1. **`kubelet` `rootDirectory` is `null` (`configz` on `velnor-240` and `velnor-5`).** Kubelet
     defaults to `/var/lib/kubelet` on `/` (`ext4` on single OS disk `nvme0n1`, `nodefs = 3,777 GB`,
     mounted without `noatime`), so **100% of pod `emptyDir` scratch I/O (`_work`, `/tmp`,
     `dind-sock`, `dind-externals`) hammers the single OS `ext4` disk** while `/var/lib/rancher`
     (`imagefs = 34.6 TB` on `105`, `49.9 TB` on `240`) holds only **6 to 8 GB** of images.
  2. **LVM `data/rancher` is linear, not striped ([PLAN.md:76](file:///Users/donbeave/Projects/tailrocks/velnor-runners/PLAN.md#L76)).**
     `lvcreate -l 100%FREE -n rancher data` omitted `-i <stripes>`, concatenating the 3 to 4 data
     NVMes on `105` and `240` linearly so one drive takes 100% of I/O while 2 to 3 NVMes sit idle.
  3. **Zero CPU or memory isolation (`configz`).** `cpuManagerPolicy: "none"`,
     `topologyManagerPolicy: "none"`, `kubeReserved: null`, `systemReserved: null`, `evictionHard` has
     no `memory.available`, `podPidsLimit: -1`, pods set `requests: 2 CPU` with **no CPU limit** and
     `306%` memory limit overcommit (`385 GiB`), and `dind` mounts no volume at `/var/lib/docker`.
- **Unobservable read-only without host exec or SSH.** Active `amd-pstate` governor, BIOS `NPS` mode,
  THP mode, sysctls, and `flannel-wg` MTU.

### Layer-by-layer analysis

- **The OS (Debian 13, kernel `6.12.94`).**
  - *CPU governor and `amd-pstate`.* Boot with `amd_pstate=active` and set
    `scaling_governor=performance` (`energy_performance_preference=performance`). Rust builds spawn
    hundreds of 20 to 80 ms `rustc`, proc-macro, and test processes. Leaving `schedutil` or `ondemand`
    active costs **5% to 10%** wall time in frequency ramp delay.
  - *SMT and NUMA or CCD layout on EPYC 9454.* Keep **SMT enabled (96 threads)** for a 15% to 25%
    throughput gain on parallel crate builds, while enforcing `full-pcpus-only=true` so sibling
    threads of one Zen 4 core never split across pods. Each of the 8 CCDs has 6 cores (12 threads,
    such as `0-5,48-53`) sharing one 32 MB L3 cache. Pinning 12-thread or 24-thread slots to whole
    CCDs avoids cross-CCD Infinity Fabric L3 misses without needing a BIOS reboot. Setting BIOS
    `NPS=4` (2 CCDs and 3 DDR5 channels per NUMA node, paired with
    `topologyManagerPolicy: single-numa-node`) is an optional later 2% to 5% refinement requiring
    Hetzner KVM console access.
  - *Transparent HugePages (THP).* Set `enabled=madvise` and `defrag=defer+madvise`. Setting `always`
    with synchronous defrag stalls short-lived compiler forks, whereas `jemalloc`, `mimalloc`, and
    LLVM explicitly `madvise` large arenas.
  - *Swap and overcommit.* Run `swapoff -a`, keep `vm.overcommit_memory=0`, and set kubelet
    `evictionHard.memory.available=4Gi` so OOM never hits `k3s-agent` or `containerd`.
  - *File descriptors, PIDs, and `inotify`.* Set `fs.inotify.max_user_watches=1048576`,
    `fs.inotify.max_user_instances=8192`, `fs.file-max=4194304`, `kernel.pid_max=4194304`, container
    `nofile=1048576`, and kubelet `podPidsLimit=16384`.
  - *`vm.dirty_*` writeback and scheduler.* Set `vm.dirty_background_bytes=2147483648` (2 GiB),
    `vm.dirty_bytes=8589934592` (8 GiB), and `vm.dirty_expire_centisecs=6000` (60 s) so transient
    build files stay in RAM page cache without hitting Linux's default 25.6 GB (20%) writeback stall
    wall. Keep Debian 13's `6.12.94` LTS kernel (EEVDF scheduler, `io_uring`, mature XFS reflinks).
- **Storage.**
  - *Filesystem choice (XFS vs ext4 vs Btrfs vs ZFS).* **XFS (`crc=1,finobt=1,reflink=1`, mounted
    `noatime,nodiratime,logbufs=8,logbsize=256k`) is the optimal choice**, and is already formatted on
    `/var/lib/rancher`. `ext4` lacks `FICLONE` reflinks, forcing slow physical copies of 14.7 GB
    `target/` trees. `Btrfs` suffers severe extent-tree lock contention under parallel
    multi-thousand-file `rustc` and linker unlinks. `ZFS` duplicates memory between ARC and Linux page
    cache, adds out-of-tree DKMS friction on Debian kernel upgrades, and has higher small-file
    metadata latency than XFS.
  - *NVMe layout and RAID0 striping.* Move kubelet
    (`--kubelet-arg=root-dir=/var/lib/rancher/k3s/kubelet`) and all Velnor caches onto
    `/var/lib/rancher`. On multi-NVMe workers (`velnor-240` with 4 data NVMes, `velnor-105` with 3),
    carve a **RAID0 striped LV** (`lvcreate -i 4 -I 128k -L 10T -n velnor_fast data` on `240`,
    `-i 3 -I 128k -L 8T` on `105`) for `3x to 4x` IOPS and bandwidth. On 2-disk workers (`241`, `5`,
    `190`), `/var/lib/rancher` already has a dedicated `3.5 TiB` data NVMe isolated from the OS disk
    once kubelet `root-dir` moves.
  - *`tmpfs`, containerd snapshotter, and image mirror.* Mount `/tmp` (`RUSTC` and `TMPDIR` scratch)
    as `emptyDir: {medium: Memory, sizeLimit: 8Gi}` (nodes use `<40%` of 125.5 GiB RAM today, evidence
    pack B.2.1). Configure containerd `overlayfs` snapshotter with `discard_unpacked_layers = false`
    (retains blobs for Spegel) and deploy **Spegel** stateless P2P OCI mirror across workers so images
    pulled once distribute over LAN without `import-pod.sh`.
- **Network.**
  - Keep all cache, nextest archive, and `crates-mirror` traffic **node-local or same-site (`fsn1` vs
    `hel1`)**, never crossing the `35 ms` DE-to-FI WireGuard tunnel synchronously. Verify `flannel-wg`
    MTU is `1420` (`1500` eth0 minus `60 to 80 B` WireGuard header) with iptables
    `TCPMSS --clamp-mss-to-pmtu` to prevent TLS HTTP/2 fragmentation stalls. Enable `crates-mirror`
    globally and cache Rust toolchains in L0 `/opt/velnor`.
- **Kubernetes and k3s defaults.**
  - Apply the following kubelet drop-in (`/etc/rancher/k3s/kubelet.config`) on all five EPYC workers:

```yaml
apiVersion: kubelet.config.k8s.io/v1beta1
kind: KubeletConfiguration
cpuManagerPolicy: static
cpuManagerPolicyOptions:
  full-pcpus-only: "true"
  prefer-align-cpus-by-uncorecache: "true"
topologyManagerPolicy: best-effort
reservedSystemCPUs: "0-5,48-53"
evictionHard:
  memory.available: "4Gi"
  nodefs.available: "5%"
  imagefs.available: "5%"
podPidsLimit: 16384
imageGCHighThresholdPercent: 90
imageGCLowThresholdPercent: 80
failSwapOn: true
```

  - Reserve CCD 0 (`0-5,48-53`), leaving **7 clean 12-thread CCDs (`84` threads)**. Use **Guaranteed
    QoS (`requests == limits`: `12 CPU / 16Gi` or `24 CPU / 32Gi`)** and PriorityClasses
    (`velnor-merge-queue` `100000` > `velnor-pr` `50000` > `velnor-background` `1000` with
    `preemptionPolicy: Never`).
  - *Suitability of k3s defaults.* `flannel` `wireguard-native` works well *only* when heavy cache
    traffic stays intra-node or same-site, because WireGuard encrypts cross-node pod traffic in kernel
    softirq. k3s's `local-path` provisioner creates plain directories without quota or reflink
    support, so `velnor-node`'s direct XFS `prjquota` and `FICLONE` manager replaces `local-path`
    PVCs. Single-server embedded storage on `velnor-237` is a single point of failure and should move
    to 3-node embedded etcd HA when budget allows.
- **Build-specific.**
  - *Linker.* **Rust `1.99.1` already uses `rust-lld` by default on `x86_64-unknown-linux-gnu`**
    (enabled upstream in Rust `1.90.0`), so switching away from GNU `bfd` is already active. Adding
    **`mold`** (`-C link-arg=-fuse-ld=mold` on `x86_64-unknown-linux-gnu`) yields a further **1.3x to
    1.6x speedup on pure link steps (~3 to 7 s)** across `velnor-runner-host`'s 18 test binaries.
  - *`rustc` CI profile.* Configure root `Cargo.toml` and `.cargo/config.toml` as follows:

```toml
[profile.ci]
inherits = "test"
debug = "line-tables-only"
incremental = false

[profile.ci.package."*"]
debug = false
```

  - In `#585` M5, stripping third-party debuginfo cut cache size by **24%** (`75.5 MB` down to
    `57.4 MB`) and cuts DWARF emission and relocation time by **18% to 30%** while preserving full
    `file:line` panic backtraces.
  - *`CARGO_BUILD_JOBS` and test threads.* With Guaranteed pod CPU limits (`12` or `24`), `cargo` and
    `nextest` read cgroup v2 `cpu.max` accurately (`12` or `24` instead of `96`). Export
    `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` explicitly to match the slot's pinned thread count,
    and cap `docker-networks` test concurrency at `4` inside guest `dockerd` (`16 GiB` RAM per
    12-thread slot and `32 GiB` on 24-thread build slots cover the observed `13.4 to 16.8 GiB` peak).

### Ranked summary table (by gain per effort)

| Rank | Change | Expected gain and evidence | Risk | Rollback |
|---:|---|---|---|---|
| **1** | **Guaranteed CCD-sized pods (`12` or `24` CPU, `requests == limits`) + kubelet `static` CPUManager (`full-pcpus-only`, `prefer-align-cpus-by-uncorecache`) + `reservedSystemCPUs` CCD 0 + `evictionHard.memory.available=4Gi`.** | **Removes the 2.4x contention slowdown** (`3.7m` vs `8.8m` on `velnor-runner-host`, evidence pack T6) and saves **10% to 20%** L3 miss overhead (estimated from 32 MB per-CCD locality vs 2,592-thread thrash). | Low (caps concurrency at 7 fast 12-vCPU slots/node instead of 30 thrashing pods). | Remove kubelet flags, delete `cpu_manager_state`, restart `k3s-agent`, revert pod limits. |
| **2** | **Move kubelet `--root-dir` + Velnor caches onto `/var/lib/rancher` XFS (`reflink=1,noatime`), mount `/tmp` on `8Gi` `tmpfs`, and mount XFS at `/var/lib/docker`.** | Moves all pod I/O off OS `ext4` disk; unlocks **0.5 to 1.2 s XFS CoW clones** of 14.7 GB `target/` (saving **44 to 61 s** vs WAN cache, `#585`); cuts `/tmp` `.rcgu.o` syncs **5% to 10%**. | Low (`7 * 8Gi = 56Gi` worst-case tmpfs inside `125.5 GiB` RAM). | Revert `--kubelet-arg=root-dir` and `/tmp` `medium: Memory`. |
| **3** | **`[profile.ci]` (`line-tables-only`, `debug = false` on deps) + `mold` linker on Linux x86_64.** | `-24%` `target/` bytes (measured `#585` M5), **18% to 30% faster cold compile/link** (`~15 to 25 s`), plus **~3 to 7 s** link savings from `mold` over default `rust-lld`. | Very low (`file:line` backtraces kept; Linux-gated flag). | Revert `Cargo.toml` profile and `.cargo/config.toml` rustflags. |
| **4** | **OS sysctls & CPU power: `amd_pstate=active` + `performance` governor, THP `madvise`, `vm.dirty_bytes=8G` / `background=2G`, `inotify`/`file-max`/`pid_max`, `swapoff -a`.** | **5% to 12% faster build/test wall time** on short-lived compiler/test spawns; eliminates THP compaction pauses and 25 GB writeback stalls. | Low (+40 to 60 W idle draw on flat-rate dedicated servers). | Remove `/etc/sysctl.d/99-velnor.conf` and kernel arg; `cpupower -g schedutil`. |
| **5** | **Topology labels (`fsn1`/`hel1`), PriorityClasses, global `crates-mirror`, Spegel P2P OCI mirror, and `1420` WireGuard MTU + MSS clamping.** | `cargo fetch` at **2.7 to 3.3 s** (measured [state/RESULT.md:134](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L134)), zero `35 ms` cross-site cache transfers, zero manual `import-pod.sh` image loads. | Very low (`preemptionPolicy: Never` avoids killing live jobs). | Delete labels, PriorityClasses, and Spegel release; unset mirror config. |
| **6** | **Striped RAID0 LVM LV (`-i 4` on `240`, `-i 3` on `105`, `-I 128k`) for `/var/lib/rancher/velnor` (+ optional later BIOS `NPS4`).** | **3x to 4x NVMe bandwidth and IOPS** on `velnor-240` and `velnor-105` (`~14 to 22 GB/s` sequential read across 3 to 4 Gen4 NVMes vs 1 linear drive). | Medium (cordon/drain 1 node at a time to create striped LV). | Unmount striped LV and point `/var/lib/rancher/velnor` back to linear LV. |

## 8. Phased delivery plan and risk containment

Ordered by `Sequence Work into Verifiable Units` and `Subtract Before You Add`, every phase ships a
standalone measurable win on `velnor-new`.

| Phase | Scope summary | PR wall median / p90 target | Push-to-merge median target | Runner-min / merge target | Warm `target/` hit target |
|---|---|---:|---:|---:|---:|
| **Baseline today** | `velnor-actions` 0.1.80, 8 hosted gates, DinD everywhere, broken caches, 3x 39-job runs | `26.7m / 92.3m` (`74.7m` on `11-18`) | `33.8m` quiet / `141.2m` contended | `179.5` | `0%` (`0/3,636`) |
| **Phase 1 (Week 1)** | Move all CI gates to cluster; split `velnor-fast` (no DinD) vs `velnor-docker` (`periodSeconds: 1`); cut over to `velnor-gen` (`#1290`); bake `rust 1.99.1` + `mold`; wire `crates-mirror`; `min_wait = 0`; skip `push` re-test after `merge_group`; filter `local-mode`; land `#1333`, `#1349`, mock-sleep fix, delete the State seal workflow & `maintenance-helpers` `--jobs 2` | **<= 4.5m / 7.0m** | **<= 9.0 min** | **<= 38** | **>= 80%** |
| **Phase 2 (Weeks 2-3)** | Finish 1-workspace `crates/velnor-*` (`527` unique deps); `[profile.ci]` (`debug=0` deps); Rank 1-5 host/k3s CCD & XFS tuning; site-local Twirp `CacheService` v2 + XFS CoW `target/` seeding; LPT 6-way test sharding; affected-only `merge_group` + nightly full run | **<= 1.5m (typical) / 2.2m (full)** | **<= 2.5 min** | **<= 14 (typical) / 26 (full)** | **>= 92%** (`0` crates on warm) |
| **Phase 3 (Weeks 4-6)** | Ship `velnor-coordinator` + `velnor-node` (`schemalane` journal, retire 171k-LOC `velnor-runner-host` & ARC); warm `RootlessNamespace` (`<50 ms`) + snapshot-restored KVM `MicroVm` (`~100 ms`); PR-sticky `pr-<N>` CoW overlays + 5-node async `main` sync; striped LVM on `240`/`105`; tree-equal `merge_group` fast-pass | **<= 50s (typical) / 85s (full)** | **<= 75 to 105 s** | **<= 6.0** | **>= 95%** (1st push) / **>= 97%** (2nd+ push) |
| **Phase 4 (Weeks 7-9)** | Ship static `velnor-worker` (`~10k` LOC: lifted `expression/` + `MemoryCounter`, `RunService`, streaming `AppendBlob` `Results`, parallel `hashFiles`, in-sandbox `node24`), proven on `velnor-local` differential suite with `OfficialRunner` fallback kept live | **<= 30 to 38s (typical) / 65s (full)** | **<= 45 to 75 s** | **<= 4.5** | **>= 96%** (1st push) / **>= 98%** (2nd+ push) |

### Top five risks, detection, and containment

1. **GitHub modifies undocumented `RunService` or Twirp `Results` wire fields (Phase 4).**
   - *Detection.* Strict status/schema counters in `velnor-worker` page immediately on any
     `400`/`422`/decode failure; nightly canary runs execute against `velnor-lab`.
   - *Containment.* Set `engine_mode = "official"` in `velnor-coordinator` (`<60 s` live reload). Jobs
     run on the pinned official `actions/runner` binary inside the exact same warm sandboxes and XFS
     CoW caches while AI agents update `velnor-worker`.
2. **CoW snapshot corruption, mtime drift, or cross-scope cache pollution.**
   - *Detection.* `velnor-ci report --check` flags unexpected crate rebuilds on the `Cache` check;
     `main` snapshots are `chmod 0555` immutable and verified against `inputs.tsv` digests.
   - *Containment.* Any verification error deletes the suspect snapshot, marks `Cache` red, and falls
     back to a clean cold build ([docs/goals.md:36](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L36) Never-Compromise #3). Fork jobs run exclusively in
     KVM microVMs over `dm-snapshot` read-only public block seeds whose writes vanish at VM exit.
3. **Undeclared cross-crate file read missed by affected-only `merge_group` selection.**
   - *Detection.* `rust-inputs-audit` checks reads on every PR, and the 6-hourly/nightly scheduled
     workflow runs `mode = all` across the entire workspace.
   - *Containment.* Any scheduled full-run failure opens a P0 issue, sets `.velnor/rust-ci.toml`
     `merge_group = "all"` in one commit until the missing input mapping is added, and prevents silent
     `main` drift.
4. **Privilege escalation from untrusted fork PRs or cross-org jobs on shared bare-metal workers.**
   - *Detection.* Automated `velnor-runner-qualification` probes verify capability drops, `/proc`
     masking, `NetworkPolicy` blocks on `10.42.0.0/16`, `10.43.0.0/16`, `169.254.169.254`, and
     `5.9.55.237:6443`, and read-only mounts before any worker admits jobs.
   - *Containment.* Fork PRs never touch rootless containers or shared DinD; they stay fenced off the
     cluster (PR `#1332` hosted gate twin + `all_external_contributors`) until KVM microVM hardware
     isolation passes qualification in Phase 3.
5. **Control-plane `velnor-237` outage or NVMe exhaustion from leaked PR snapshots.**
   - *Detection.* `500 GiB` XFS `prjquota` on `/var/lib/rancher/velnor` triggers automatic LRU
     eviction of closed/48h-old `pr-<N>` snapshots at `400 GiB` (`80%`); node-exporter alerts on
     `velnor-237` health.
   - *Containment.* Un-acknowledged scale-set messages stay safely queued on GitHub
     ([actions/scaleset/session_client.go:100-103](https://github.com/actions/scaleset/blob/main/session_client.go#L100-L103)) across `velnor-coordinator` restarts; adding 2 small k3s server
     nodes removes the `velnor-237` SPOF, and `.velnor/rust-ci.toml` can fall back to `ubuntu-26.04`
     in one commit if Hetzner networking ever partitions completely.

## Owner questions

1. **Updating `docs/goals.md` for self-hosted CoW caches and Phase 4 native execution.**
   [docs/goals.md:16](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L16) and Rule
   11 ([docs/goals.md:74](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L74))
   require that *"Jobs are always executed by GitHub's official runner"*, and
   [docs/goals.md:45](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L45) binds
   the design to *"GitHub's 10 GB of cache and on standard hosted runners."* Do you approve updating
   [docs/goals.md](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md) so hosted mode
   preserves those limits for external users while self-hosted cluster mode uses >10 GB node-local XFS
   CoW snapshots and Phase 4's `velnor-worker` native Rust runtime (with `OfficialRunner` retained as
   local-mode oracle and fallback)?
2. **Merge-queue ruleset and selection policy ([docs/goals.md:86](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L86) vs [docs/hard-decisions.md:108](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/hard-decisions.md#L108)).**
   Do you approve (a) changing ruleset `24396608` to `min_entries_to_merge_wait_minutes = 0` and
   `max_entries_to_build = 5`, (b) skipping `push`-to-`main` test execution when `merge_group` passed
   on the identical commit SHA, and (c) enabling [docs/goals.md:86](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L86)'s default affected-only selection on
   `merge_group` (plus tree-equal fast-pass) backed by 6-hourly/nightly full workspace runs once
   `velnor-gen` owns CI?
3. **Replacing `velnor-runner-host`'s 171k-LOC monolith instead of extending it via PR `#1324`.** PR
   `#1324` proposes porting `velnor-runner-host` (`171k` LOC, `64k` in 52-version SQLite
   migrations/triggers) onto one-Pod-per-job Kubernetes + `ReadWriteOncePod` PVCs. Do you approve
   freezing `velnor-runner-host` for macOS `velnor-local` only (and trimming it to a thin Docker
   adapter) while building the clean `velnor-coordinator` + `velnor-node` (`schemalane` state store +
   warm rootless/KVM sandboxes + XFS reflinks) architecture for Linux/cluster execution?
4. **Fork PR policy prior to Phase 3 KVM microVMs.** Until KVM microVM isolation ships in Phase 3,
   should fork PRs remain fenced off the cluster behind `all_external_contributors` approval and PR
   `#1332`'s hosted gate twin, or should we dedicate and taint one worker (`velnor-190`) as an
   untrusted pool now?
5. **Control-plane HA (`velnor-237`).** Once the zero-spend research constraint ends, do you approve
   adding two small Hetzner instances (~20 EUR/mo total) to turn `velnor-237` (`5.9.55.237`) into a
   3-node HA k3s control plane for `tailrocks`, `ChainArgos`, and `jackin-project`?

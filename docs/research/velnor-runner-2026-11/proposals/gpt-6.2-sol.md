# Proposal by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-18 17:13–17:49 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

Move the eight Linux CI control jobs onto the cluster first. Then cut redundant build work, keep compiler outputs near execution, and fix the longest individual tests. The current runner can deliver these gains.
Fresh GitHub records for 24 first-attempt PR runs on November 18 show 50.5 min median wall time. The 18 successful runs spent a median 41.1 min in hosted queues on the critical path. A replay with the same job work and short queues gives 12.0 min to the required check.
Build Velnor as a Rust scheduler and execution service with disposable job environments and persistent, verified caches. Ship with GitHub's worker. Qualify a native worker on a separate scale set and promote it only if a controlled trial shows a material gain at equal confidence.
Keep required results for both the PR and the actual merge group. Run each independent obligation once per distinct verified input set. On main, validate the merge evidence, run main-specific work, and take the full path when the evidence is missing.
The warm target is 2–4 min to a typical code PR verdict and 2–4 min for a typical merge group. That gives 4.5–10 min of machine time from push to merge for an already approved PR with an available queue slot. This is a budget, with the assumptions in §5.
The most important constraint after routing is contention. Live worker CPU ranged from 4% to 93% across two samples. Today's CPU requests give the scheduler a poor picture of load, while measured CI tails include a single runner-host or image-publication test taking several minutes.

## 1. Where the time goes

### Run wall time and who is waiting

All measured dates below are UTC. The local source snapshot is velnor-new `95de9b704b`. Legacy is `1b9fc038`,
actions/runner is `7afce9edc8`, and ARC is `89f34c4f78`. Measurements from this session use read-only GitHub and
Kubernetes calls and a scratch copy of the archived runner.

- **Historical population, recomputed.** The evidence-pack run list for Nov 4–18 yields PR p50/p90 of 17.5/60.0 min
  over 1,382 completed success or failure runs. Merge group is 22.8/87.3 min over 190. Push is 16.2/39.7 min over 231.
  This uses `run_started_at → updated_at`. The independent script also counts 1,156 cancellations out of 2,558 PR
  events, or 45.2%. `ci-pack-analysis.txt:1`, `evidence.md:42–74`.
- **Before and during the incident.** The evidence-pack job sample gives scale-set-era PR wall p50/p90 of 26.7/92.3
  min, measured from run creation. The Nov 17 and Nov 18 population medians for successful PRs were 16.2 and 74.7 min.
  On Nov 18 the pack finds a 57 min median sum of the Plan and two Required queues in 13 successful PRs. The original
  92/67/73 min figures describe a more constrained cohort and time window. The 73.2 runner-minute baseline explicitly
  used push and merge-group runs. `evidence.md:75–103,185–196`, `~/.cache/velnor-p3-ci-baseline.md:3–50`.
  [Original measurements](https://github.com/tailrocks/velnor-new/issues/574#issuecomment-6969131218).
- **Fresh, first-attempt cohort.** I fetched the latest completed Nov 18 first attempts, stratified by event and
  conclusion, then fetched each attempt's jobs and step timestamps. The sample has 18 success and 6 failure PR runs,
  12 plus 4 merge groups, and 10 plus 2 pushes. It is a snapshot weighted toward recent completions. The job pages all
  arrived, and it excludes cancelled attempts. `ci-measure.ts`, `ci-collect.log`, `ci-live-analysis.txt:2–15`.

| Event | Runs | Wall p50/p90, min | Runner-min p50/p90 | Jobs listed / executed, p50 | Hosted job queue p50/p90 | Cluster job queue p50/p90 |
|---|---:|---:|---:|---:|---:|---:|
| PR | 24 | 50.5 / 61.8 | 57.8 / 78.2 | 39 / 25 | 14.6 / 28.0 min | 13 / 17 s |
| Merge group | 16 | 58.6 / 86.4 | 76.7 / 93.2 | 39 / 39 | 20.0 / 35.4 min | 16 / 118 s |
| Push | 12 | 85.1 / 146.0 | 71.1 / 82.7 | 39 / 39 | 17.2 / 39.5 min | 14 / 24.8 s |

Wall is run creation to update, job queue is job creation to start, and runner-minutes sum start-to-finish for jobs
with an actual runner and a non-skipped conclusion. The historical raw job data likewise gives a PR median of 25
executed jobs and 14 skipped. The pack's statement that the PR always executes all 39 conflates job records with
execution. The generated job conditions agree with the corrected count. `ci-pack-analysis.txt:7`,
`evidence.md:206–213,401–409`, `.github/workflows/ci.yml:147–214,626–649`.

### Follow the last prerequisite at each join

GitHub starts Plan and Ledger CLI in parallel, and also starts four lints independently. Rust jobs need both Plan and
Ledger CLI. Named checks need Plan. Required verdict waits for the named checks, Rust jobs, Plan, and lints. Required
then waits for the verdict. Tracing backward from Required on each successful run picks the last prerequisite at each
join. Skipped jobs contribute no runner time.

- In the 18 fresh successful PRs the medians were 50.8 min creation-to-Required, 41.1 min of hosted critical-path
  queue, 12.5 s of cluster queue, and 10.2 min executing critical-path jobs. These are separate medians. Ledger CLI
  started the critical path in 8 of 18 and maintenance-helpers ended the fan-out on it in 4 of 18. In the older
  recomputation maintenance-helpers accounted for 34 of 51. On three fresh successes a lint was the last required
  prerequisite.
- Run 44324327696 is one exact decomposition. Hosted queue 2,423 s + cluster queue 2 s + job execution 662 s +
  residual 4 s = 3,091 s to Required. Residual here is scheduler and timestamp spacing outside the jobs' queue
  intervals. [Run 44324327696](https://github.com/tailrocks/velnor-new/actions/runs/44324327696).
- Replaying the DAG with every hosted job given the historical cluster median of 12 s from the pack, with observed job
  durations and cluster waits fixed, gives fresh PR p50/p90 of 12.0/14.1 min and merge-group 13.2/19.5 min. This is a
  routing counterfactual. It excludes extra contention and assumes new work clears any independent hosted limits.
  `ci-live-analysis.txt:4,13`, `evidence.md:164–175`.
- The pack snapshot at 16:54 found 86 queued, including 61 CI, all on hosted labels. My 17:24:31–17:24:48 scan of 14
  runs found 16 queued jobs, all hosted, including 13 CI. The 16 include a macOS release job. REST collection spans a
  time window, and a job can start during it. The cluster merge-group p90 above shows that short cluster waits are
  already workload-dependent. `evidence.md:387–399`, `ci-queue-later.log`.
- To link CI to actual merges, I verified the landed commit via REST for the first 12 successful, merged candidates in
  the pack's pair list. Nine had both an exact landed merge-group SHA and a prior successful PR run for the final PR
  head. Median PR CI creation to merge was 44.3 min (2,657 s), from 24.9 to 529.4 min. The median gap from PR CI
  completion to merge-group creation was 9.5 min (569 s). That gap can include review and enqueue time. This small
  sample uses run creation as a proxy for the last push. In PR #990 the timeline was 876 s PR CI + 45 s between runs +
  1,938 s from group creation to merge = 2,859 s. `ci-pairs.jsonl:1–10`.
  [PR #990](https://github.com/tailrocks/velnor-new/pull/990),
  [PR run 43895228206](https://github.com/tailrocks/velnor-new/actions/runs/43895228206),
  [merge-group run 43897999932](https://github.com/tailrocks/velnor-new/actions/runs/43897999932).

### Setup, downloads, compile, test, and transfer

Fresh successful PR steps from `ci-live-analysis.txt:5–11` are p50 seconds. Plan, Ledger CLI and Required verdict have
18 observations each. The four heavy jobs have 17 each. Overlapping step durations can exceed wall time.

| Job | Wall | Key work inside it |
|---|---:|---|
| Plan | 62 | Checkout 13, mise setup 1, pin tooling 8, restore sources 2, publish plan 1. The job also verifies inputs, chooses cache state, and plans. |
| Ledger CLI | 52.5 | Restore mise 9, fetch sources 3, build helper 21, upload helper 1. Rust jobs consume the helper artifact. |
| Maintenance helpers | 544 | Setup job 4, checkout 6, execute named check 518, upload reports 3. |
| Actions CLI | 409 | Setup job 5, checkout 5, pin tooling 10, fetch sources 6, fixture 86, build test executables 4, tests 260, clippy 39, docs 30. |
| Runner-host default | 494 | Setup job 6, checkout 4, pin tooling 10, test build 94, tests 343, clippy 76, docs 64. |
| Runner-host local-mode | 500 | Setup job 5, checkout 4, pin tooling 9, test build 81, tests 290, clippy 77, docs 65. |
| Required verdict | 34.5 | Download all expected reports 13, merge 4. Report download p90 is 32.6 s, including per-artifact overhead. |

`Unit and integration tests` and `Wait for parallel checks` have the same reported duration for the heavy rows. The
jobs schedule checks concurrently. Keep wall and summed step work distinct. The GitHub `Set up job` step starts after
assignment and can include action resolution. The jobs API cannot separate Node startup, broker round-trips, pod
readiness, link time inside Cargo, or time decompressing an archive. Future instrumentation should record those
boundaries. `.github/workflows/ci.yml:965–989,2592–2620`, `evidence.md:401–408`.

I parsed timestamps and nextest summaries in the pack's maintenance logs. Between observed phase markers the medians
were 83.7 s for pin-mirror (7 logs), 45.8 s for Linux image inventory (7), 106.6 s for diagnostics builds (8), and 5.4
s to fetch sources across workspaces (8). Archive-guard's own nextest summary was 211.9 s median over 8 completed
summaries, with a 55.2 s median longest passing test. These are phase buckets, so the pin-mirror bucket includes a
Cargo invocation and policy work. The fresh step median of 518 s shows the size of the remaining hot path.
`ci-pack-log-analysis.txt:2–3`, `mise.toml:97–199`.
[Maintenance job 134191252252](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191252252).

Some heavy suites already have a much longer serial floor than a shard average would suggest. In the same run one
Actions CLI image-publication test took 274.3 s and one runner-host test took 270.8 s, with the corresponding
local-mode test at 203.4 s. These are reported single-test times for one run under live load. Smaller shards alone
cannot eliminate them. The open journal PRs offer measured CPU reductions on particular tests, including 17.9 → 10.3
CPU s for `Journal::open` in #1333. Use those results to choose experiments. They do not establish a fleet speedup.
[CLI job 134191420345](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191420345),
[host job 134191420609](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191420609),
[local-mode job 134191420582](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191420582),
[PR #1333](https://github.com/tailrocks/velnor-new/pull/1333),
[PR #1349](https://github.com/tailrocks/velnor-new/pull/1349).

### Cache misses and repeated work

- In the pack logs I independently count 48 of 48 Rust source misses and 10 of 10 Plan hits. A fresh 25-log sample
  reproduced 15 of 15 Rust misses, 5 of 5 Plan hits, and 15 of 15 Rust logs with an empty `MBX_REMOTE_URL`. The
  scale-set source miss takes about a second to report in the pack. In job 134191420609 the subsequent fetch ran 4.54
  s and downloaded 435 crates. Pack logs contain 481 median `Compiling` banner lines for runner-host. Those counts
  include repeated compiler banners for the same package. `ci-pack-log-analysis.txt:1–8`,
  `ci-live-log-analysis.txt:1–6`, `evidence.md:323–351`.
- The same cache key on hosted and ARC uses a different effective `runner.temp` directory. Cache version incorporates
  paths and compression. The #574 same-run trace shows a hosted hit and a scale-set miss for the matching key. Use the
  same runner class and canonical cache path for producer and consumer. The path/version mismatch explains this
  observed pair. Other misses can still have different causes. `.github/workflows/ci.yml:332–338,703–707`.
  [Same-key trace](https://github.com/tailrocks/velnor-new/issues/574#issuecomment-6969256836),
  [GitHub cache matching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#cache-key-matching).
- Pack observations show zero MBX remote transfer, no Rust toolchain *restore* in 58 logs, and 3,636 skipped snapshot
  restores. The cluster image already seeds tools. A missing Rust restore is therefore evidence about the cache path
  and says little by itself about network download time. The actual pin-tool steps are in the table. The 18 recent
  state-seal runs include 6 generator and 4 CI helper seals, with zero root, local and runner seals. Tip advancement
  and producer test or clippy failures both occur. The workflow-level green result alone does not establish a usable
  seal. `evidence.md:323–385`.
  [State-seal trace and compatibility keys](https://github.com/tailrocks/velnor-new/issues/1305#issuecomment-6980735625).
- I recomputed that 120 of 159 successful merge groups had a push CI run on the same SHA, or 75.5%. The pack's smaller
  comparison found 7 of 45 identical PR-head and merge-group trees, using PR heads read later. This is an upper-level
  check on source duplication. It cannot show identical event inputs or identify the tree actually checked out by a
  historical PR merge-ref job. For one representative accounting exercise, 52.9 PR + 64.6 merge-group + 62.0 push
  runner-min = 179.5 runner-min for three runs using the pack's medians. Removing interchangeable push work can
  reclaim up to that 62.0 per matching push, minus the evidence and main-only work. It frees capacity even though push
  happens after merge. `ci-pack-analysis.txt:1–4`, `evidence.md:75–93,206–213`.

## 2. Hypothesis verdicts

| Hypothesis | Verdict | Evidence and expected size |
|---|---|---|
| 1. Rust for the runner and actions makes CI dramatically faster | **Fails** as a general performance claim | Existing steps already use native Git, Cargo, nextest, and a Rust helper. For a heavy Rust job the measured checkout, mise setup, cache lookup, and MBX setup medians sum to about 4 + 2 + 1 + 2 = 9 s before allowing for network and process work they still have to do. That is under 2% of the 494 s host job. One-off startup and action prefetch savings could plausibly be 0–10 s on a critical path. Ask for a bigger result only when an adapter eliminates actual I/O or repeated work. |
| 2. A from-scratch Rust runner beats both ARC and the official worker | **Unproven** | ARC's acquire, Kubernetes reconciliation, and JIT cold path is real, but the observed PR cluster queue p50/p90 is 13/17 s. Removing every second of queue on a three-dispatch critical path at 13 s each gives an illustrative upper bound of 39 s for that component. Pod isolation, caching and CPU reservations can change under the official worker. Require an additional paired fleet improvement before promoting a native executor. |
| 3. Kubernetes lets us use the existing hardware in parallel | **Partly holds** | It already starts PR jobs within 13 s median on the cluster. Its scheduler places requests and those requests hide real demand. Observed jobs reached 83.7 logical CPU equivalents while requesting 2. The initial budget in §6 admits 4 heavy slots per worker or 20 fleet-wide if every worker is eligible. Its purpose is predictable execution and queue tails. It does not create CPU or validate cache locality. |
| 4. Local caches and warm-node placement pay | **Holds** as a hybrid design | The owner measured GitHub cache traffic at 13.6–22.4 MB/s. At that rate an 883 MB archive takes 39–65 s over the wire, and a 1,457 MB archive takes 65–107 s. Small sources downloads already take only seconds. Expect one to three minutes of timed-path savings where warm compilation work applies, conditional on the cache and job targets in §4. A shared per-site miss path makes hits useful after placement changes and loss of a worker. |
| 5. Workflow design can omit provably irrelevant work and reduce wall time | **Holds** | The current planner already skips a median of 14 of 39 PR jobs. Improve the dependency map and exact check coverage, carry test executables once, shrink the longest tests, and avoid repeating compatible push obligations. A budget of 15–25 s plan and dispatch + 35–70 s build + 50–120 s test + 10–20 s verdict is 110–235 s. It requires resolving the observed multi-minute individual tests. |

The Rust choices still make sense for typed protocol state, integration with the existing runner, and a small
distribution. Qualify native checkout or a cache adapter for functional value. Keep equivalent official action
behavior in the comparison so an omitted operation cannot look like a speedup. The archive status and code below give
no controlled native-versus-official speed result. `velnor-legacy/plans/ci-performance/README.md:3–29`.

## 3. Runner architecture and compatibility

### Choose the executor independently of the scheduler

| Route | What it buys | Main limitation | Decision |
|---|---|---|---|
| ARC and GitHub's worker, with a better workflow and images | The queue and most build gains on existing infrastructure | Placement sees coarse runner classes and ARC continues to reconcile one runner pod per job | Ship this first and measure it as the control. |
| Rust Velnor scheduler, GitHub worker on Velnor's disposable runtime | Control of admission, warm placement, cache permissions, lifecycle and reporting measurements | GitHub still defines the worker's behavior. Increased controller ownership needs an operational payoff. | Recommended fleet architecture. Trial it next to ARC using separate scale-set names. |
| Rust scheduler and native Rust worker | Control of the whole local execution path, with finer profiling, warm action and environment reuse | Full workflow behavior and wire compatibility become Velnor's responsibility | Qualify this as a third configuration on the same workload. Promote proven job profiles based on the threshold in §8. |

GitHub still plans event triggers, reusable workflows, matrices, job conditions, queue and required checks. It assigns
a job and receives its outcome. Standard Actions YAML remains the source interface. `velnor-gen` produces it and can
resolve repository-specific build needs. A remote worker consumes GitHub's materialized job message, evaluates
runner-side templates and steps, and reads the resolved actions' `action.yml` files. Parsing a workflow in local mode
is a separate concern. This preserves the GitHub server's semantics and avoids scheduling the same DAG in two
products. `actions-runner/src/Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs:27–47,134–193`,
`actions-runner/src/Runner.Worker/JobRunner.cs:68–86`.

### Separate capacity, job identity, and the sandbox

1. **Control plane.** A Rust service owns the org App credential, one fenced active listener per scale set, admission
   policy, and the slot inventory. Size, trust, OS, and Docker capability belong to workflow labels and restricted
   runner groups. Publish available capacity from actual execution and cleanup state. Use scale-set suggestions and
   acquire only when capacity is reserved. Reconcile acquired IDs against replayed messages and update the cursor
   after the state transition is durable. The API associates requests with a scale set. Assume assignment to eligible
   slots within that set until a feed field with stronger semantics is proven. Cache-family placement is possible at
   the class level before assignment and at finer resolution only after Velnor can identify the actual job.
2. **Worker and runtime.** A warm slot is an image and a clean disposable environment. A job gets its own workspace,
   process tree, PID and network ownership, and short-lived credential. Keep a shallow queue of pre-pulled or
   precreated empty slots. Recycle the slot after proof of cleanup. Keep the sandbox launch separate from the worker
   selection. The default backend launches the unmodified official Listener/Worker. The native backend uses the same
   job context and evidence contract. A warm pod reacquires capacity only through a clean new sandbox and ephemeral
   runner registration. Avoid resetting and reusing a job-bearing process.
3. **Ownership and recovery.** Persist reservation, acquire intent, confirmed or uncertain acquire, JIT claim,
   provisioned sandbox identity, running lease, reporting, and cleanup. Use a compare-and-set generation or fencing
   epoch on each controller or node ownership grant. Bind Pod UID or sandbox identity, node, attempt, runner ID and
   the journal record. Retry a timed-out write only after reconciling evidence. Keep an uncertain job and its capacity
   quarantined until the broker/runner and runtime observations agree. Report accepted completion before retirement
   and stop stale workers from publishing artifacts or completing a new attempt. Test a crash at every outbound
   mutation and a lost response after server acceptance.
4. **Cache and telemetry services.** Run these outside job sandboxes. The cache service owns durable files and
   authorizes reads and writes. Capture monotonic spans for poll, acquire, JIT, image/unpack, schedule, ready, job
   receipt, first renewal, first step, source downloads, compiler work, test execution, report and cleanup. Carry
   run/job/attempt, pool, node, cache state and image provenance. Redact tokens and cap log, diagnostic, and wire-body
   sizes. A failed reporting path stays visible as infrastructure failure or unresolved work.

### Protocol and runtime contract

- Use the current GitHub.com scale-set path. The reference Go client has session open/refresh/delete, cursor-based
  long polling and advertised capacity, `acquirejobs`, ack and `generatejitconfig`. Treat these operations
  separately from the worker's credential and lease. Inspect retry, conflict, throttling, and stale-session semantics
  in each release. On Nov 18 the release API returned `actions/scaleset` v0.5.0, `actions/runner` v2.338.2, and ARC
  chart v0.15.1. GitHub.com documents a 63-day update window and can enforce critical updates sooner. Stage image and
  protocol bumps in the lab as they arrive. [Scale-set client](https://github.com/actions/scaleset/tree/v0.5.0),
  [runner release](https://github.com/actions/runner/releases/tag/v2.338.2),
  [runner update policy](https://docs.github.com/en/actions/reference/runners/self-hosted-runners#runner-software-updates).
- For a native worker, register the ephemeral JIT identity, use the broker reference and run-service `acquirejob`,
  acquire the first renewal, and maintain a lease independent of the build. The reference worker renews every 60 s.
  Match the source for accounting IDs, outcome, cancellation grace, retry classifications, and completion. Run the
  Results client for step and job logs, timeline, summaries and annotations, and send job outputs to the run service.
  A terminal outcome includes reporting evidence and the identity of the accepted completion. Preserve cancellation
  semantics for post-steps. `actions-runner/src/Runner.Listener/Runner.cs:699–800`,
  `actions-runner/src/Runner.Listener/JobDispatcher.cs:380–480,748–840`,
  `actions-runner/src/Runner.Worker/JobRunner.cs:290–357`,
  `actions-runner/src/Sdk/WebApi/WebApi/ResultsHttpClient.cs:610–632`.
- Evaluate expressions on typed GitHub values and reproduce GitHub's status-function defaults, null/coercion,
  contexts, timeouts, `continue-on-error`, file commands, pre/main/post and composite scope. Resolve action downloads
  via the launch service. Separate JS, composite, script and Docker/service execution. JavaScript should run under
  GitHub's selected Node version with the correct workspace and file commands. Preserve GitHub-provided runtime
  environment for actions, including artifact, cache and OIDC endpoints and tokens where authorized. These are service
  APIs invoked by actions. In the official worker's script handler, runtime token exposure differs from Node actions.
  Verify least privilege, masking, and signed-URL redirect boundaries with real jobs.
  `actions-runner/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs:54–93,120–129`,
  `actions-runner/src/Runner.Worker/Handlers/ScriptHandler.cs:313–327`,
  `actions-runner/src/Runner.Worker/ActionManager.cs:1060–1092`.
- Start native support with the generated Linux Rust profile. Run standard pinned JS actions first. A built-in adapter
  must match an exact action commit, supported inputs, outputs, credential behavior, failure behavior, and post-steps.
  If the requested action or job feature is outside the supported profile, assign to an official lane through the
  generator or pre-dispatch routing. A native worker that discovers an unknown feature after acquiring the job must
  fail explicitly or use a service-supported defer operation if one is proven. Starting a second worker after the job
  acquires its lease creates an ambiguous owner.
- Use GitHub to compare official and native on the same source, action pins, sandbox, trust, and input graph. Compare
  step conclusions, file commands, artifacts, topology, check output, error and cancellation states. Include instance
  restarts, job reruns, service containers, and action updates. Admission of a production job profile requires real
  end-to-end recordings. A mock validates parsing and state transitions. Its observations stop at that boundary.

### Sources to carry forward

- **velnor-new.** Reuse the Rust wire types, provisioning intent, lifecycle and ownership proofs, image qualification
  and attestation, and local-mode comparators. Keep the documented exact-source and credential separation. The host
  runs the official runner in a Docker pair. Reuse the invariants while replacing Docker-backed object identity with a
  Pod UID or sandbox generation. Use one instance of the execution policy and leave Docker support for developer
  machines. `velnor-new/crates/velnor-runner/crates/velnor-runner-core/src/lifecycle.rs:9–75`,
  `velnor-new/crates/velnor-runner/crates/velnor-runner-core/src/ownership.rs:21–42`,
  `velnor-new/docs/deferred/self-hosted-runner.md:3–22,276–302`.
- **PR #1324.** The Kubernetes runtime proposal was absent from the checked-out main tree. I read it at PR head
  `7e8b0805`. Its Pod UID proofs and separate privilege for provisioning fit the design. Its per-node claims are a
  candidate for scratch volumes. Prefer service-mediated immutable reads for shared caches. `ReadWriteOncePod`
  requires CSI support and a claim attached to one node can strand a waiting job. Delay a general runtime trait
  migration until the actual K8s and Docker operations prove the shape.
  `docs/proposed/kubernetes-runtime.md@7e8b0805:124–208,240–312,389–416`.
  [PR #1324](https://github.com/tailrocks/velnor-new/pull/1324),
  [Kubernetes volume access modes](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#access-modes).
- **velnor-legacy.** Read the expression lexer/parser/value code, protocol state and Results fixtures, script and
  composite runner, checkout git-mirror tests, cache namespace rules, and Docker/Firecracker boundaries. The old
  native action resolver supports a closed allowlist with restricted checkout and composite coverage. The scale-set
  lane delegates to the official runner. Treat expression and wire tests as a starting corpus. Avoid importing the
  large combined executor, a parallel GitHub workflow scheduler, a different generic cache service for every action,
  or per-action JS containers that discard tool or workspace changes.
  `velnor-legacy/crates/velnor-runner/src/expression/mod.rs:1–15`,
  `velnor-legacy/crates/velnor-runner/src/action.rs:378–460`,
  `velnor-legacy/crates/velnor-runner/src/mbx_store.rs:1–26`,
  `velnor-legacy/content/docs/guides/execution.mdx:118–158`,
  `velnor-legacy/content/docs/concepts/architecture.mdx:137–147`.
- **Official runner and ARC.** Use their current implementation and fixtures to test runner behavior and provisioning.
  Retain ARC's ordered ack and reconciliation discipline and the disposable identity. The slow path has acquire,
  replica patch, creation of an EphemeralRunner object, JIT, Pod and worker startup. Instrument each before replacing
  it. Avoid using one controller's serial path to perform work that can happen while idle. ARC's Kubernetes container
  hooks require additional pod permissions and differ from Docker on Dockerfile actions and service networking. Apply
  them only to qualified profiles. `arc/cmd/ghalistener/scaler/scaler.go:135–186,263–339`,
  `arc/controllers/actions.github.com/ephemeralrunner_controller.go:834–953`.
  [Kubernetes container-hook limitations](https://github.com/actions/runner-container-hooks/blob/main/packages/k8s/README.md).

### What the archive tells us

A locked offline build on the local arm64 Mac used rustc 1.98.1, a fresh target and prepopulated Cargo downloads. It
finished in 172.55 s after I populated the scratch Cargo cache with the pinned Git dependency. With the repo's nextest
0.9.148 I ran its expression, action, scale-set, protocol, MBX-store, and cache-service library tests. All 628 passed,
2,653 were outside the filter or skipped, and nextest reported 28.369 s of testing. The test command including
compilation took 204.75 s. The Mac was under other load. These are one-off validation timings. The pack's independent
workspace run reported 8,172 of 8,222 tests passing with environment and timing failures and unresolved assertion
failures. KVM, Linux container isolation, and genuine GitHub execution parity remain untested here.
`legacy-build.log:410–415`, `legacy-nextest.log:156–164`, `velnor-legacy/mise.toml:10,45`, `evidence.md:1058–1077`.

Legacy's final commits `459159ea` and `1b9fc038` switch it to the successor's generator after consolidation commits
`84d41850` and `e24509dc`. GitHub's archive metadata points to velnor-new as current work. I found no explicit
archive rationale in the terminal commits, history searches for the successor/archive, or the repository docs beyond
that succession. Issues concerning registration loops and result or artifact handling show the maintenance burden of
the old design. They establish exposure and operational cost. The archival cause beyond succession remains unknown.
[Archived repository](https://github.com/tailrocks/velnor-legacy),
[registration incident](https://github.com/tailrocks/velnor-legacy/issues/408),
[rerun reporting incident](https://github.com/tailrocks/velnor-legacy/issues/670).

## 4. Cache architecture

### Put immutable outputs near jobs

| Kind | Node-local tier | Shared and fallback tiers | Correctness conditions |
|---|---|---|---|
| Toolchains and tools | Seed approved runner images and immutable mounted tool bundles. Verify tool paths selected by mise as well as component presence. | Replicate pinned manifests and digests per site. Use a verified upstream download for a missing public blob. | Lock exact rustc, Cargo, rust-std, clippy, rustfmt, mise, nextest, mbx and target components. Keep staged and previous generations during rollout. |
| Git and crate sources | Local bare Git object mirror and a verified Cargo registry/git seed. Materialize a clean checkout at the assigned SHA and a private Cargo home for each job. | Keep same-site sparse proxy and immutable source blobs. Fetch upstream or fail when offline-required inputs cannot be resolved. | Verify object and crate checksums. Scope private git objects by repo and policy. Fetch private inputs with a short-lived scoped read credential before build execution, then revoke it. |
| Rust compiler artifacts | Per-node MBX object store plus a private Cargo target. Reuse an immutable base with a private copy-on-write view where the format allows it. | Same-site MBX endpoint or object store for misses, with asynchronous replication of promoted objects. GitHub cache is a small hosted fallback for portable data. | Include actual toolchain, target, compiler invocation and input identity. Use Cargo's validation on target-state restores. Benchmark actions/cache transfers separately. |
| Test executables and fixtures | Build each required configuration once. Place read-only test archives on the node or in a same-site content store. | Shards fetch exact digests. Public job outputs and evidence remain visible in GitHub. | Bind feature sets, environment ABI, version and tested source, artifact identity and full shard inventory. Preserve runner images and fixture paths used by each test. |
| Test results | Store signed evidence of completed obligations in the control plane. | Retain the supported GitHub check and links to run reports. | Reuse a pass only when the complete inputs and policy match and its producer is trusted for that consumer. Keep the initial gate on fresh execution for every selected test. |

Build keys include actual rustc and sysroot digests, target and CPU features, image/tool generation, Cargo features
and dependency graph, profile, compiler and linker flags and versions, build-script outputs, proc macros, native
libraries/headers, and observed external inputs. A change to path remapping, a fixture or an environment input can
invalidate a reuse claim even when the Rust files match. In a workflow where a tool cannot account for its reads, make
that operation uncacheable or constrain its inputs and prove the resulting key covers them. Mount only the needed
contents. Cargo's freshness and MBX lookup telemetry are independent proofs.

Use tenant and trust namespaces for authorization and strong object digests for integrity. They solve different
problems. Compute the directory or manifest hash before publishing, stage content by digest, revalidate, then
atomically publish a manifest and provenance record. Readers verify manifests and digest on ingest, materialize to
private scratch safely, and avoid running cache-supplied scripts during restore. Lease snapshots/object generations
while used, reserve bytes and inodes, collect unreachable entries by size and age, and distinguish a legitimate miss
from corrupt data. A corrupt object takes the cold path and produces a diagnostic. Archive pruning and producer
promotion need independent locks and clear owners.

### Make trust explicit

- Treat same-repo PR and merge-group code as untrusted execution as well as forks. A merge-group check establishes a
  verdict about its exact integration tree. It never grants producer credentials to that code.
- A trusted post-merge producer or supervised trusted publisher creates the main object namespace from immutable
  source, pinned build inputs, and an authenticated workflow. Where policy permits, verified bytes from a known good
  staged producer can be promoted by the supervisor after merge. A job cannot promote its own bytes or sign its own
  success.
- Serve public main objects read-only to untrusted jobs. Same-repo PR writes, if enabled under the new generator
  policy, go only to the scoped PR store and can be read only from that PR under the same trust. Keep old-engine PRs
  read-only while its current policy is active. Forks get public read-only objects and private writable scratch that
  is destroyed. Always separate private dependencies and derived outputs from anything a fork can read. No registry,
  Git, Kubernetes API, signing, or runner management credentials are inherited from a prior job.
  `docs/goals.md:38–45,71–76`, `.velnor/config.toml:6–14`.
  [GitHub cache visibility and branch restrictions](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache).
- Do not put the shared writable store behind a job's host mount. Give the cache server credentials to its storage and
  give the job narrowly scoped API access. A read-only mount protects the mounted filesystem from normal writes. An
  independent boundary is still required for co-tenancy with privileged jobs.
- Replace the tip-sensitive snapshot seals with manifests keyed to verified inputs and the immutable protected-branch
  commit. A newer main tip can coexist with an older successful producer. Use reachability and provenance
  authorization at publish time and compatibility at read time. Stop writes for superseded generations when useful for
  space. Preserve old seals until parity is measured, then remove the producer and prune jobs that the new receipts
  replace.

### Place jobs based on their expected time to finish

Choose an eligible site and node by `predicted slot wait + provision + required cache transfer + residual work`. Hard
filters enforce trust, architecture, services and memory. Soft scoring considers known usable bytes, estimated compute
left, recent evictions and node pressure. Cache-inventory digests are control-plane data. Use stable labels for
capability classes or site and a bounded node-inventory index for cache generations. Fall through to another worker
when predicted wait outweighs a warm hit. For an illustrative 800 MB artifact, assumed local materialization at 800
MB/s is 1 s and assumed same-site transfer at 100 MB/s is 8 s before receipt overhead. A warm node waiting 20 s loses
to the idle node under those assumptions. Do not include an unmeasured bandwidth assumption in the initial placement
policy. Collect these times first and use work-conserving weighted queues with age to limit starvation.

### Define hit rates and latency budgets

- **Tools:** target at least 97% of eligible jobs using image or node bytes only. This follows a planning assumption
  of 98% correct generation placement × 99% local availability. Expected initial saving is 5–8 s of the measured 9–10
  s pinned-tools step when setup and checks still take 2–4 s. Track manifests resolved as well as bytes.
- **Public sources:** target 90% of eligible jobs with no external source fetch. Budget 10% for new lock entries and
  cold or rejected data in steady state. A remaining 4–7 s cold fetch usually has low wall effect, but many concurrent
  jobs repeat it. Expect 4–7 s saving per hit and no transfer of already present crates. Take actual rate from
  per-blob and per-job telemetry.
- **Compiler work:** plan for 70–85% of eligible unchanged-unit lookups satisfied locally, then half to three quarters
  of the misses satisfied by the site tier. The combined target is about 85–96% by `L1 + (1 − L1) × L2`. Start with
  lower exits in §8. These denominators exclude Cargo-fresh units and report bypasses separately. The pack's 40–74% is
  hits as a share of reported candidate entries including `not-looked-up`, within ephemeral jobs. It gives no
  cross-run hit rate. Measure newly compiled units and elapsed time as well as hits. If an 81–94 s test build spends
  30–60 s on reusable work, saving 85% of that buys 26–51 s before restore overhead. Larger wins require removing
  duplicate builds or warming the two maintenance build phases. `evidence.md:323–351`, `ci-live-analysis.txt:10–11`.
- **Test results:** target 0% skipped on the grounds of previous pass at initial rollout. The CI gate first proves
  selection and task execution. A later approved exact-evidence mode can save up to the equivalent portion of the 62.0
  median push runner-min on matching pushes. For PR to merge group the pack's 7/45 tree-equality observation is
  insufficient to predict a result hit. Different integration inputs, policies, features, reruns, environments and
  event contracts bound it. Instrument eligible versus accepted reuse before enabling it.
- **Transfer:** the 86 MB sources archive at 13.6–22.4 MB/s costs 3.8–6.3 s at those rates before extraction, in the
  same range as direct fetching on these nodes. Prefer object reuse and site fetch for tiny misses. For large state,
  log transfer and unpack separately, use hit time only when both beat rebuild, and prefer streaming concurrent
  materialization with a wait budget.

## 5. Workflow design for the minimum honest verdict

### Make selection a property of an obligation

Generate an input graph for each check. Begin with a pinned release of the planner and the verified event. For Rust,
combine Cargo metadata and reverse dependencies with features, targets, profiles, lockfile edges, build scripts, proc
macros, test assets, configuration, and the data read by lint and doc tools. Record check-level edges for generated
files, shell and Bun scripts, Docker contexts and image policies. Preserve both names on a rename and account for
deletions, submodules, file modes, and symlinks. Widen coverage when the event range, generated graph, lockfile
mapping, or an external input is unknown.

- Candidate identity comes from the actual checkout and trusted event. On PRs record head, base, merge base and tested
  tree. On merge groups read base and head from the payload and check the exact combined integration tree, including
  entries ahead of the PR. A manifest order, feature-policy, shared tool or workflow change can widen the scope to the
  workspace or the repo. A change outside the graph can select zero Rust work only with enforced exclusions and known
  readers. `ci-helper/crates/velnor-ci/src/event.rs:21–50`, `docs/goals.md:40–43,63–76`.
- Audit every continuously selected check against real input reading. The maintenance block currently combines
  independent inventory and policy work, diagnostic examples, all-workspace fetch and archive-guard tests beneath one
  selector. Give each activity a declared input set. Keep unknown paths conservative. A shared input still selects
  each affected activity. For new selectors, compare with a full independent run on a representative corpus. Inspect
  reports of both skipped and executed tasks. Introduce synthetic changes in fixtures, build scripts, lock edges and
  workspace configuration that must be caught. A new or unidentified reader broadens the graph until it is classified.
  `.velnor/config.toml:282–956`, `mise.toml:97–199`.
- Keep the released plan and gate outside the change being judged. The job which builds the candidate Ledger CLI must
  still prove that candidate works. Use an authenticated released verifier for the ledger so Rust jobs can get started
  as soon as their plan arrives. If a check's semantics require the candidate binary, verify that independently using
  the existing checkout identity and artifact checks. Reports bind run, attempt, SHA/tree, verified plan digest,
  allowed configuration, and selected task IDs. Gate on completeness, actual passing outcomes and expected test
  counts. Missing, duplicate, stale, cancelled, and scheduled-but-skipped reports are failures of the verdict.
- Initially retain `Required verdict` and `Required`. The main ruleset reported both names via the GitHub REST API.
  The current final guard handles the case where GitHub reports a skipped check as successful. Keep cancellation
  protection and verify the identity of the workflow producing the checks. After shadow parity, replace the two jobs
  with one always-evaluated required reducer that cannot make a cancelled or missing report green. Move the branch
  rules at the same time as the authority, and run a real cancellation and partial-rerun qualification first.
  `.github/workflows/ci.yml:514–625`, `github-main-rules.jsonl:5`, `docs/hard-decisions.md:218–229`.
  [GitHub ruleset checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-status-checks-to-pass-before-merging).

### Build once per configuration, then distribute the tests that need it

Use the `velnor-gen` shadow as the place to change Rust workflow shape. The checked-in workflow is generated by
v0.1.80 of the old engine. The shadow `rust-ci.yml` had three failures and one cancellation in the API on Nov 18.
Treat it as a vehicle for parity work. The new renderer already has an archive flow and workspace commands. The
workspace consolidation should remove many isolated Cargo roots, but it can change feature unification and input
paths. Shadow compare the package and feature obligations, docs, clippy, doctests, direct harnesses, local-mode and
Cargo deny/machete. Keep checks for previously meaningful feature configurations even if Cargo's workspace union hides
a missing feature. `.github/workflows/ci.yml:1`, `Cargo.toml:21`,
`generator/crates/velnor-render/src/cargo.rs:73–180`, `docs/hard-decisions.md:486–506`.
[Shadow PR #1223](https://github.com/tailrocks/velnor-new/pull/1223),
[shadow run 44193541357](https://github.com/tailrocks/velnor-new/actions/runs/44193541357).

- Keep a small workspace job for work whose dispatch and transfer costs exceed the benefit of splitting it. For the
  slow jobs, build nextest executables once for each target/feature/profile tuple. Archive and transfer by digest on
  the same site or use a local reflink. Keep docs and clippy's metadata work where that choice preserves the checks
  and avoids duplicate compilation. Keep `cargo test --doc` and direct harnesses explicit. A nextest archive covers
  neither doctest execution nor a separate command silently. For example, keep both runner-host configurations from
  `.velnor/config.toml:240–249`. [Nextest archive requirements](https://www.nexte.st/docs/ci-features/archiving/).
- Start with a measured test manifest and pinned nextest, which is already 0.9.151 in `mise.toml:24`. Use nextest
  partitions for large, uniform suites. For the slow host and CLI suites, pack the expected tests by duration and
  resource needs. Assign one immutable set per shard. Size the jobs so an isolated test's concurrency or Docker
  requirement fits. The reducer verifies disjoint expected test sets and their union, rerun identity and explicit
  approved exclusions. Process-local, paused-clock, and Docker tests still run within the intended environment. Use
  p90 durations for slow or scarce-service cases and combine tiny suites.
  [Nextest partitioning](https://www.nexte.st/docs/ci-features/partitioning/).
- Fix the long individual tests before raising shard count. Qualify the historical-journal template and one-time
  schema-validation changes using the actual migration and corruption assertions. Cache setup fixtures when their
  immutable inputs are identical, leaving per-case mutation and the real validation path inside the test. For the CLI,
  split independent cases that repeatedly create source and image-publication fixtures. Build the exact candidate
  fixture once by digest outside the individual tests, then give each case a private copy. Preserve the identity and
  negative assertions. For maintenance, give pin-mirror and diagnostics the same safe compiler cache and schedule
  independent checks without waiting for archive-guard. Keep the whole publication and archive guard obligation.
- Choose shard count by critical time. Four perfectly balanced shards would take 343/4 = 86 s from the fresh default
  runner-host test-step median only if the work were divisible and all four had equivalent resources. One observed 271
  s test already exceeds that estimate. Shard selection begins once the longest indivisible work fits the target. For
  maintenance, the historical 212 s suite and 55 s longest-test median suggest a 90–130 s test-stage target after
  balancing and allowing overhead. The fresh five-log cohort has 263 s suite and 77 s longest-test medians, so reserve
  headroom. Build/archive plus upload plus each shard's download and provision are measured against the unsharded
  candidate. `ci-pack-log-analysis.txt:2–3`, `ci-live-log-analysis.txt:2–3`.

### Keep the merge queue authoritative

The same required check names must be produced on both events. GitHub checks are coupled, and PRs enter the merge
queue after passing their required checks. A PR green says something about the tested PR inputs. The merge group is a
fresh decision for an integrated candidate. Keep both events and use the same planning, inventory and check logic.
Diff against the authenticated base for merge groups once the new generator policy is approved. Run the full matrix on
uncertain scope and explicit full campaigns.
[GitHub's merge-queue contract](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue).

Re-execute group-sensitive checks and changed obligations. Reuse exact immutable *build outputs* even when event
identity differs if the build key accounts for all inputs. For test-result reuse later, require a prior verdict for
the same check revision, trust, flags, relevant source, dependency and external inputs, and environment. Git SHA is
required when a test reads it. An unchanged checkout tree is insufficient if a task reads Git metadata, event inputs,
advisory time, credentials or infrastructure. For cross-event equivalence, use a trusted receipt and a check-specific
proof that the event fields are irrelevant. Use actual GitHub check reporting for the new commit and keep a visible
record of fresh versus reused checks with links. Full shadow runs remain the control until the policy is proven.

On a protected main push, verify queue provenance, actual landed commit or proven equivalent obligation inputs, exact
test plan identity, and the relevant accepted checks. Run newly relevant main-only verification, release triggers, and
cache promotion. A non-queue push, different effective inputs, missing evidence, or manual bypass takes the normal
full checks. Existing release gates look for exact-SHA default-branch push evidence. Keep a real push receipt and
qualify the release by digest and provenance before changing any trigger. Release-plz tag policy, archive checksums
and per-archive attestations remain gates. Never give private publisher credentials to the build being measured.
`ci-helper/crates/velnor-ci/src/gate.rs:44–156`, `docs/goals.md:76–77`.

Use cancellation on superseded PRs and preempt only safely retryable non-gating work. Restrict all-branch image
candidate jobs to their input graph and move independent qualification, cache priming and release work to their own
priority lanes. Keep full periodic runs, nightly or scheduled integration, release platforms, and deliberately forced
cold samples with visible outcomes. They detect dependency-selection and cache gaps and keep publication coverage.
They are a check on the online policy, which must itself stay conservative. `.github/workflows/ci.yml:57–59`,
`.github/workflows/job-image-candidate.yml:1–35`, `.github/workflows/source-qualification.yml:1–37`.

### Budget to verdict

For a **warm ordinary code PR**, the target budget in seconds is `15–25` for plan and dispatch + `35–70` for the
longest required build and materialization + `50–120` for the slowest selected test or other check + `10–20` for
reporting. That sums to 110–235 s, rounded to 2–4 min. Build and shard timers must establish these values. A no-Rust
PR targets 1–2 min with a small plan, required lints and reducer. For a first cold, broad, or Docker-heavy run budget
5–8 min to verdict while the slow suites are being improved. Estimate that class from 1 min orchestration + 1.5–2.5
min build/setup + 2.5–4.5 min checks. The 2–4 min outcome is conditional on per-test reductions and warm capacity.

For an already approved PR with an open merge slot, add 2–4 min for the typical warm merge-group checks and assume
0.25–1 min each for enqueue/dispatch and GitHub finalization. `2–4 + 2–4 + 0.25–1 + 0.25–1 = 4.5–10` min. This is
elapsed CI-related machine time for that scenario. Review wait, re-formed groups and busy-queue admission add time.
Track actual push, required check, queue entry, group creation, group verdict and merged timestamps as separate
events. The live ruleset had a build concurrency of 2, maximum merge count 10, minimum 1, check timeout 90 min, and a
5 min wait setting if the minimum is unmet. Do not add a blanket 5 min to a nonempty group with minimum 1. Treat build
concurrency as the limit to tune against real admission and cluster pressure. `github-main-rules.jsonl:6`.
[GitHub merge limits](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue#managing-a-merge-queue).

## 6. Kubernetes, capacity, and isolation

- Keep k3s as the placement and lifecycle API. Use separate runtime classes for small shell/control jobs, ordinary
  Rust workers, and trusted Docker-heavy work. Reserve space for control and verdict jobs, such as two small slots per
  site to start, and promote merge groups above ordinary PR fan-out. Avoid a privileged sidecar on plan, lint and
  ordinary compile jobs. For workloads that exercise a real Docker daemon, run the real tests in a qualified isolated
  runtime. Benchmark Kubernetes hooks only where their supported behavior covers the job profile.
- Start with the ARC path at min warm slots sized to measured simultaneous gate arrivals. Pre-pull by digest, keep
  clean spare pods, assign once, and delete after evidence is collected. When building Velnor's controller, pre-create
  sandboxes with an identity-free image and defer job credentials until assignment. A transient busy slot retains its
  lease across controller reconnects. An idle slot is safe to remove. Handle job cancellation, Pod termination and
  node loss as distinct outcomes. Permission to reuse capacity depends on observed cleanup or a fenced recycled node.
- Use explicit DE/FI labels, taints for trusted Docker and untrusted CI, and named runner groups or scale sets with
  repository allowlists. Place heavy compile and test shards near each other to avoid cross-site artifact fan-out.
  Distribute unrelated PRs across sites and workers. Put a reserve in each site so outage or congestion reroutes
  eligible jobs. Rebuild locally if a site's large cache cannot be fetched within the transfer budget. The single
  control plane remains a fleet availability limit. Start with backup and recovery and fenced listeners. A later
  requirement for independent site availability favors separate site control planes or a quorum design in enough
  failure domains. If a site loses control-plane contact, finish only jobs with valid leases and an authorized
  reporting path, and stop new claims until state reconciles.
  [K3s distributed cluster guidance](https://docs.k3s.io/networking/distributed-multicloud).
- The repository is public according to GitHub REST. Fork code, same-repo PR code, and merge-group programs receive no
  host runtime socket, shared writable mounts, provider routes, cluster token or publisher credentials. Give each job
  a read-only root where possible, non-root identity, constrained cgroups, a job-scoped isolated writable volume and
  network policy. Keep log and cache services outside its credentials. A taint supplies placement. Use hardware-backed
  isolation such as Kata or Firecracker for hostile code where KVM is available and the qualification suite passes.
  Keep publicly writable or otherwise untrusted work on verified GitHub-hosted isolation until its self-hosted class
  is ready. Trusted privileged dind belongs on a dedicated cleared node pool without other tenants or secrets. Merging
  a PR does not by itself make its candidate jobs safe to execute with release privileges.
  [GitHub secure-use guidance for public repos](https://docs.github.com/en/actions/reference/security/secure-use#hardening-for-self-hosted-runners).
- Bound advertised capacity by per-class CPU, memory, disk and image availability. With measured 96 threads and about
  125 GiB per worker, start a canary that reserves 8 logical CPUs and 12 GiB for host/runtime headroom. For a heavy
  slot assume 16 logical CPUs and 24 GiB including sandbox overhead. Then `min(floor(88/16),floor(113/24)) = 4` heavy
  slots per worker and 20 when all five workers are eligible. Dedicated pools, gate reserves and other tenants reduce
  that upper bound. Begin with a small-job class around 2 CPU and 2–4 GiB, calibrated by RSS and CPU profiles.
  Measured executable footprints drive future capacities. ARC's max replicas across orgs are administrative limits,
  not simultaneous heavy jobs.
- More servers buy burst capacity, hard separation of public/fork jobs, warm failover, and a second copy of a hot site
  cache. Under the same slot assumptions a sixth equivalent eligible worker buys up to four heavy slots. That is +20%
  over 20. It yields no speedup for an isolated 274 s test. Add physical capacity only once wait and saturation remain
  high at equal test/lint scope after load control. A dedicated isolation pool can justify additional hardware
  independently of latency.

## 7. MBX and Blaze

### MBX

`mise.toml:6–24` pins Rust 1.99.1, MBX 1.29.2 and nextest 0.9.151. The action is pinned at v1.7.3 in
`.velnor/version-policy.toml:78–81`. The latest stable MBX release API returned v1.30.0 on Nov 18. Test pinned 1.29.2
first under real pipeline flags and pack formats, then test an upgrade separately.
[MBX releases](https://github.com/jdx/mr-boxington/releases/tag/v1.30.0),
[pinned action inputs](https://github.com/jdx/mr-boxington-action/blob/v1.7.3/action.yml).

Use MBX for per-invocation compiler outputs and the measured variants of native-link or managed-target reuse it
supports. Qualify with the same locked dependency graph and feature sets. Run node-local and site stores with distinct
per-job writable targets, leases and quotas. Legacy's per-slot separation addressed its registrar flock and Cargo's
target lock. Verify the current MBX build's concurrency before deciding whether the daemon, store, or only the target
needs separate writable roots. Retain Cargo-fresh units, MBX-eligible attempts, remote and local hits, bypass reason,
and time saved as distinct counters. `velnor-legacy/crates/velnor-runner/src/mbx_store.rs:1–26`.
[MBX cache modes](https://mr-boxington.jdx.dev/github-action).

Use the action's local mode for persistent local storage and remote mode for site MBX, with read-only enforcement at
the storage boundary. Avoid overlapping GitHub target payload and MBX remote management for the same job. Upstream
documents that its GitHub target payload clears the remote URL and disables native link caching. The installed cluster
uses the local backend and loses its directory on teardown. Keep trusted publish narrow, and use `mbx doctor`, trace,
and elapsed timings to verify configuration. The vendor's hk benchmark had a warm next push of about 9.2 s compared
with 18.9 s for Cargo, a vendor measurement on another project. It shows why an object-level experiment is worth doing
and sets no Velnor speed claim. [MBX action behavior](https://mr-boxington.jdx.dev/github-action),
[MBX benchmarks](https://mr-boxington.jdx.dev/benchmarks).

Benchmark sccache as a mutually exclusive fallback against the same Cargo invocations and warm/cold workload. It can
share compile results across hosts and help native C/C++ dependencies. Rust incremental builds and binaries that
invoke the linker have cacheability limitations. Avoid stacking wrappers or relying on a high request-hit percentage
while Velnor's hardest work bypasses them.
[sccache Rust caveats](https://github.com/mozilla/sccache/blob/main/docs/Rust.md).

### Blaze

Blaze's latest release observed on Nov 18 was v0.20261027.0. Both that release's `harness/toolchain` and main at
`6e3d4da7` name `nightly-2026-10-27`. velnor-new's stable pin is 1.99.1. Blaze uses its own toolchain and
build/execution paths, and its toolchain compatibility documentation says that a project's pin does not select the
toolchain. Support for compiling some stable-compatible source is a different claim from parity with that stable
compiler. It is ineligible for the required gate under the current toolchain decision.
[Blaze release](https://github.com/indexable-inc/blaze/releases/tag/v0.20261027.0),
[release toolchain pin](https://github.com/indexable-inc/blaze/blob/v0.20261027.0/harness/toolchain),
[pinned Blaze toolchain docs](https://github.com/indexable-inc/blaze/blob/6e3d4da7482130bfa5f1903d89042c6b17e0d8ef/docs/toolchain.md).

Use Blaze for optional local edit/test feedback and a non-gating experiment. Track cold, warm same-checkout, and
clean-worktree edit latency with real fixture and feature coverage. Compare output and test failures with Cargo and
the stable pin. Its hot or incremental vendor numbers say little about cold CI on a fresh integration tree. Until the
stable toolchain and cargo/nextest/clippy/doc behavior are qualified, the official verdict comes from the pinned Cargo
path. `rust-toolchain.toml:4–8`, `docs/hard-decisions.md:232–285`.

## 8a. Server and cluster tuning

### What the nodes report

I ran `kubectl describe nodes`, `get nodes`, `top nodes`, and read-only kubelet `configz` and `stats/summary`
queries. The filtered observations are in `cluster-describe.txt`, `cluster-live.txt` and `cluster-config.txt`. There
are five Ready workers with 96 logical CPUs each, about 125 GiB schedulable memory and one Ready control node. Workers
report Debian 13, kernel `6.12.94+deb13-amd64`, containerd `2.3.7-k3s1` and k3s `v1.37.2+k3s1`. The workers lack
site labels and taints. Their allocatable CPU and memory equal capacity. Current runner sets request 2 CPU and 4 GiB,
limit runner memory to 32 GiB, and omit dind resources. Describe showed 8.1–18.1 CPUs requested per worker, 12–28% of
memory requested and 102–230% of memory limited. The currently configured min/max pairs are 4/60, 4/60 and 2/40.

At 17:16:10 and 17:16:54 I observed worker CPU range 5–93% and 4–92%, respectively. Summed use from `top` is about
247/480 and 337/480 logical CPU equivalents, or 51.5% and 70.1%. Worker memory top ranged from 4% to 47% across the
samples. The pack measured a previous 83.7-CPU runner pod and 16.8 GiB usage, and earlier owner notes observed a much
quieter 18–29% CPU. Short job queues can coexist with saturation of particular workers. CPU burst and CI step latency
are reasons to test resource controls, with no assumption of uniform load. `cluster-live.txt:1–7`,
`cluster-config.txt:1–7`, `evidence.md:502–532`.

All five `configz` reads show CPU, topology and memory management off, no reservations, no effective Pod PID cap,
`failSwapOn: false`, empty memory-swap config and hard disk signals at 5%. Image GC high/low is 85/80%. Image pulls
are not serialized. Node filesystems report 3.78 TB. Image filesystem capacity is 3.84 TB on three workers, 34.56 TB
and 49.93 TB on the large workers. Container and image filesystems reported the same capacity and used bytes per node.
The owner notes say XFS `noatime` on the LVM data volume and one separate OS disk. They list two to five NVMe disks
per worker. These observations do not identify the emptyDir disk, LVM segment layout, reflink flag, NVMe thermal
profile, actual swap, host limits, SMT state, BIOS NPS, or current governor and MTU. Verify those with the owner's
host inventory and subsequent operator-run measurements before applying host tuning. The owner documents 35 ms from an
FI pod to a DE pod and 120 ms to the GitHub cache blob endpoint. Both are previous observations.
`cluster-config.txt:8–17`, `velnor-runners/README.md:13,20–42`, `velnor-runners/state/RESULT.md:77–101,130–135`,
`evidence.md:580–598`.
[AMD EPYC 9454P specifications](https://www.amd.com/en/products/processors/server/epyc/4th-generation-9004-and-8004-series/amd-epyc-9454p.html).

### Changes in gain-per-effort order

Numbers in the following rows are experiment budgets, capacity estimates, or limits from the indicated evidence.
Budgeted gains overlap. Approve a change on repeated Linux CI on a canary worker with the same inputs and concurrency.
Read executable profiles, cgroup throttling and CPU/memory/I/O pressure, device queue, thermal and NUMA counters, run
outcomes, and end-to-end p50/p90. Roll back via the prior version of the manifests or host config on a drained worker
when a change alters runtime state. There were no configuration changes during this research.

| Priority and change | Expected gain and basis | Risk and rollback |
|---|---|---|
| 1. Give every container a realistic request. Add non-preempting priority for gates and cache consumers. Separate gate, heavy, background and trusted Docker classes by labels and taints. Set per-org admission and same-site preference. Use the budgets in §6 and give dind explicit resources when present. | First aim for 30–90 s off heavy-job p90 under concurrent load. This budget is a fraction of the approximately 186 s between the fresh default host job's p50 and p90. That spread includes variation other than CPU. Gate-class cluster wait target p90 ≤30 s. | Oversized reservations queue work. Undersized slots starve builds or evict pods. Compare active jobs, cgroup throttling, real allocator peaks and gate wait before widening. Restore previous classes and change only one workload group at a time. |
| 2. Fit Cargo and tests to their slots. Set `CARGO_BUILD_JOBS`, MBX's compiler permit limit, and nextest groups/`--test-threads` deliberately. Sweep 4/8/12 compile permits and 2/4/8 test threads in the assumed 16-thread slot. Budget compilation, clippy/docs and linker threads together. Start heavy tests lower than unit tests based on RSS and daemon use. The old named check pins 2 jobs. | Estimate 30–70 s if about 140 s of its pin and diagnostic work can run at 1.3–2x, giving 32–70 s per `140 × (1 − 1/speedup)`. Use memory and sharable-work measurements because native tools and linkers may have their own parallelism. Per-test fixture reuse is a separate change. | Higher settings can increase CPU run-queue, allocator pressure or test flakes. Preserve test pass and identity sets, restore old process caps on contention or missing work, and verify tail as well as median. `mise.toml:97–158,189–193`, `.velnor/config.toml:25–30`. |
| 3. Automate pre-pull of digest-pinned runner and fixture images and mirror them per site. Use k3s `registries.yaml` for containerd and distinct mirror config for dind or BuildKit. Seed toolchains and cache contents before admitting a new node. Keep overlayfs as the containerd baseline and maintain separate budget/GC for images, compiler cache and job scratch. | The pack's 1,424 image pull events were hits, so the expected routine image-pull gain is near zero today. Gains prevent cold regressions. At an assumed 100 MB/s, 1 GB of actual layer transfer is 10 s plus unpack, per missed image. Keep image readiness under the queue budget. | Misconfigured mirrors can change source or fail pulls. Require content digests and image verification. Restore previous registry routes, image generations and snapshotter configuration. Set image GC from working-set size and disk headroom and track repulls before changing 85/80. [K3s registry configuration](https://docs.k3s.io/installation/private-registry), [Kubernetes images](https://kubernetes.io/docs/concepts/containers/images/). |
| 4. Keep cache, target and scratch on native NVMe. Start on existing XFS with `noatime`. Verify reflink and overlayfs `d_type` support. Give containerd and compiler store distinct quotas or volumes and keep `target` out of the container writable layer. For small workers keep separate OS and data disks. On large workers measure independent device queues before choosing LVM striping. | Warm materialization can avoid tens of seconds of transfer per large restore, as in §4. For filesystem-only changes budget 0–15 s on a 300 s path if pressure measurements identify at most a 5% metadata/I/O share. Existing XFS is the control. | RAID0 or a linear multi-device span can lose more than one cache on a disk fault. Replicate canonical bytes per site and rebuild disposable caches. Compare ext4 with the current XFS for metadata; ext4 lacks ordinary reflink support. Test btrfs/ZFS only if metadata, CoW and ARC memory overhead are competitive on the exact workload. Apply mount, volume or RAID changes on a fresh canary and reroute to the prior worker. [OverlayFS filesystem requirements](https://docs.kernel.org/filesystems/overlayfs.html), [Kubernetes local ephemeral storage](https://kubernetes.io/docs/concepts/storage/ephemeral-storage/). |
| 5. Keep registry, Cargo, toolchain and result traffic at the nearest effective tier. Keep WireGuard and Flannel. Keep the existing sparse mirror opt-in and configure the actual job `CARGO_HOME` with its pinned CA. Mirror verified toolchain components and registry manifests, then verify the exact bytes after fetch. | Owner's 59 MB fetch was 3.5–6.1 s direct versus 2.7–3.3 s with the mirror. The overlapping-range difference supports a small candidate gain, about 0.2–3.4 s comparing the extremes, with independently measured build time required for adoption. Site locality matters most for many objects or large binaries. | Split DNS, wrong Cargo home or mirror errors can change inputs or break offline runs. Restore the direct source configuration, keep trust and lock verification, and use site replicas. The owner already reverted an image-wide proxy config that failed outside `/home/runner`. [K3s Flannel options](https://docs.k3s.io/networking/basic-network-options). |
| 6. Use the pinned toolchain's linker baseline, then test mold. Confirm linker in the job image by recording the actual invocation and follow rustc's fallback. Stable Rust has tried lld on `x86_64-unknown-linux-gnu` by default since 1.90. Measure the compiler and linker separately. | If link takes 30 s of the 94 s default test build and mold cuts that half, the saving is 15 s. A 10–45 s link share would yield 5–22.5 s at that hypothetical 2x. First measure with Cargo timings and process spans. | ELF/linker flags, debug symbols, native inputs and release reproducibility can change. Pin linker and flags in cache keys, qualify behavior and build IDs, and restore the toolchain default on mismatch. [Rust's LLD default](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable/), [mold](https://github.com/rui314/mold). |
| 7. Pilot CPU Manager `static` with whole physical cores when the worker shape permits. Place integer requests equal to limits on all containers required for Guaranteed QoS. Start topology manager with `best-effort`, then try `restricted` or `single-numa-node` only for jobs that fit. Test memory manager placement and alignment together. Keep host housekeeping on separate CPUs. | For a 300 s busy job, a tentative 0–10% topology and contention improvement is 0–30 s. Compare sibling occupancy, NUMA memory placement, CFS throttling, CPU migration and tail under matched concurrency. The node reports 96 threads but NUMA domains and SMT policy need confirmation. | Fragmented reservations and bad memory placement can strand jobs. Compare with realistic CPU requests and no quota on an admitted shared pool. If the run caps remove bursts or fragment capacity, revert policy and kubelet state using a drained canary. [CPU management](https://kubernetes.io/docs/tasks/administer-cluster/cpu-management-policies/), [topology management](https://kubernetes.io/docs/tasks/administer-cluster/topology-manager/), [memory management](https://kubernetes.io/docs/tasks/administer-cluster/memory-manager/). |
| 8. Reserve kubelet and OS capacity, split across kube/system reservations, and leave disk and memory eviction headroom. Try the §6 reservation total and a starting `memory.available` hard watermark around 4 GiB with 10% node/image disk headroom and inode alerts. Add storage requests/limits, scratch limits and per-job PIDs from observed peaks. Keep protected gates off the default local-path allocation path. | Expected steady median gain is 0 s. Value is avoiding loss and recovery of a job, which can cost a full 6–9 min heavy execution based on the job ranges. Reconcile kubelet accounting with actual node layout so cache GC happens before eviction. | Premature evictions or lower concurrency. Compare node condition, kubelet events, cgroup memory events and CI cancellations. Restore previous kubelet and priority settings on the canary. Use local-path for disposable node-local scratch where it fits. A local PV or cache failure can miss and rebuild. [Kubernetes reservations](https://kubernetes.io/docs/tasks/administer-cluster/reserve-compute-resources/), [node-pressure eviction](https://kubernetes.io/docs/concepts/scheduling-eviction/node-pressure-eviction/). |
| 9. Compare an explicit CI profile after qualification. Test reduced debug info, Cargo incremental off in genuinely cold object-cache lanes, and an appropriate `codegen-units` sweep instead of a global flag. Keep release settings, debug diagnostics and explicit tests for intentional profile differences. `turso_core` has a dev opt-level override today. | An experiment reducing build and link work by 10–20% of the observed 81–94 s test-build stage would save 8–19 s. Optimize profiles only after identifying the dominant work and warming the new cache generation. | Profile changes can mask optimization-sensitive timing, overflow and diagnostic bugs, or oversubscribe CPU. Compare required behaviors, preserve checked cfg/feature semantics and restore previous flags and cache generation. `crates/velnor-runner/Cargo.toml:74–75`, `.github/workflows/ci.yml:343–353`. [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html). |
| 10. Measure CPU frequencies and thermals under real builds. Keep SMT enabled initially, assign full sibling sets for isolated slots, and compare an AMD P-State performance preference with the current policy. Inspect socket, NPS, CCD and memory layout. Use NUMA-local CPUs and memory for fitting builds. Compare firmware NPS modes only if jobs fit and the workload is NUMA-limited. Keep Debian's supported kernel and scheduler defaults until a profile shows a problem. | A hypothetical 5% frequency improvement applied to 180 s of CPU-bound work saves about 9 s. Initial governor, boost and memory locality are unknown. SMT can improve fleet throughput and harm a particular linker or latency-sensitive test. Prefer per-slot placement to a fleet BIOS switch. | Thermal throttling, increased power, fairness and fragmentation. Measure delivered work and frequency together. Restore governor, BIOS settings, placement policy or prior supported kernel on the canary. [Linux amd-pstate](https://www.kernel.org/doc/html/v6.12/admin-guide/pm/amd-pstate.html), [AMD EPYC workload tuning](https://docs.amd.com/v/u/en-US/58011-epyc-9004-tg-bios-and-workload). |
| 11. Test memory and writeback settings only when counters identify stalls. Keep THP `madvise` as a canary option. Compare compaction and major faults. Prefer memory-bounded jobs without swap I/O. Measure actual swap and use the kubelet swap policy accordingly. Keep ordinary heuristic overcommit unless failed allocations justify a documented change. Examine `vm.dirty_background_bytes` and `vm.dirty_bytes` as bounded alternatives to large percentages. Use capped tmpfs for small temp files, with charge included in Pod memory. | Expect 0 s on a healthy NVMe/no-pressure baseline. A 10 s p90 writeback or compaction stall can save at most 10 s under this change. Time memory copying and archive unpack. Avoid putting whole Cargo target trees on tmpfs. | Compaction latency, earlier OOM, weak dirty limits or RAM consumed by temp files. Compare PSI, warm and cold runs, and job eviction. Restore the prior sysctls, swap setting, THP mode and disk-backed `emptyDir`. [Kernel THP settings](https://www.kernel.org/doc/html/v6.12/admin-guide/mm/transhuge.html), [kernel VM sysctls](https://kernel.org/doc/html/latest/admin-guide/sysctl/vm.html). |
| 12. Verify network MTU and host FD and inotify limits. Measure path MTU, TCP retransmits and object latency within a site and over WireGuard, including any inner container bridge. Size MTU from the measured underlay and encapsulation. Inspect actual `LimitNOFILE`/RLIMIT, `fs.file-max`, and inotify watch and instance high water before raising them for the service that needs it. | Expected median gain 0 s until retries, fragmentation or limit errors appear. At the documented 35 ms inter-site RTT, 100 serialized object RTTs already cost 3.5 s before payload. Batch and keep work local. The owner's BBR and buffer experiment was reverted without a gain. | Wrong MTU can black-hole payloads, and broad limits raise memory or FD pressure. Restore per-site MTU or service limits, route to a known-good site, and repeat real cache transfers. [K3s network options](https://docs.k3s.io/networking/basic-network-options), [kernel fs sysctls](https://docs.kernel.org/admin-guide/sysctl/fs.html). |

## 8. Phased rollout and exit criteria

Start from a control ledger with representative docs, ordinary Rust, runner/Docker, workflow/toolchain, and multi-PR
merge-group edits. Collect the existing ARC plus official-runner cohort, then pair by source, execution profile,
actual tests, toolchain, image, cache generation, site and offered load. Within each class compare interleaved cold
and warm runs, publish p50/p90 and ranges, and include failed and cancelled work in fleet minutes. For an initial
campaign use at least 30 paired ordinary PR observations and at least 10 repeated heavy/cold observations as planning
sample minimums. Hold comparisons open when the spread exceeds the improvement. Measure full machine-time phases
separately from review and enqueue delays.

| Phase | Ship and verify | Numeric exit targets on velnor-new |
|---|---|---|
| 1. Route | Land the independent v0.1.81 Linux routing change through the owner workflow, reserve capacity for controls, and collect real timings. Preserve both protected check identities and stage a public-fork isolation path. | Typical incident-cohort PR p50 ≤13 min and p90 ≤20 min; gate-class cluster wait p90 ≤30 s. The 12.0/14.1 min queue-only replay provides the estimate with room for other load. |
| 2. Warm and bound | Adopt realistic requests, separate site/size classes, make Cargo source paths canonical, measure the site store and tool seeds, fix maintenance repeat compiles, and qualify the journal and repeated-fixture improvements. Keep the existing workflow as the control. | Ordinary PR p50 ≤9 min and p90 ≤14 min; p50 ≤45 runner-min versus 57.8 fresh. ≥97% tools local, ≥90% public sources no external fetch, ≥70% of eligible unchanged compile units hit. Savings budget is 13 → 9 min from reclaiming roughly 2 min from maintenance compilation when critical plus 2 min of tested contention, fixture or test-root improvements on the hottest path. |
| 3. Reduce the verified graph | Finish generator shadow, then qualify fail-closed selection, one test build per configuration, adaptive shards, and a lean reducer. Retain doc, feature, direct harness and release obligations. Accept each test refactor from its own paired evidence. | Ordinary warm PR and merge group p50 ≤4 min, p90 ≤8 min, PR p50 ≤27 runner-min. Section 5 supplies the 110–235 s budget. Plan for at most 8 ordinary executed jobs averaging 3 min plus 3 min of control work, or 27 runner-min. More mixed cohorts get their own targets. |
| 4. Stop triple execution | Ship exact merge evidence for compatible post-merge consumers. Keep the full path for groups or main pushes with additional inputs or incomparable evidence. Prove exact-SHA release qualification and cache promotion. | On eligible same-SHA pushes p50 ≤10 runner-min versus 62.0 historical. That is about 52 runner-min reclaimed per eligible push. Keep warm ordinary PR p50 ≤4 min and test the estimated 4.5–10 min push-to-merge scenario. On direct or ambiguous pushes, retain full verification. |
| 5. Advance the runner boundary only on a win | Compare ARC, Velnor scheduling the official worker, and a native worker on separate canary sets. Establish real GitHub behavioral parity for every promoted profile. Roll back by assigning new runs to the prior set. Allow old leases to finish. | After identical workloads reach Phase 3, require a further ≥30 s p90 verdict improvement or ≥15% more verified jobs per hour at equal tail and confidence. Set an assignment-to-first-step p90 budget ≤10 s on a pre-warmed slot, measured by internal spans. Conditional phase. If neither fleet metric improves, keep the last qualified executor and retain the measured result. |

Make phase exits depend on validated test inventories and report identities as well as latency. For performance
numbers above, Explain the Number drove two choices. The queue counterfactual changes the measured queue component and
makes no claim about build CPU. The sharding budget uses the measured longest test as a floor. For correctness, Prove
It Works drives same-tree shadow runs, direct comparison of required check conclusions, and a real release receipt
before retiring a workflow.

| Risk | Detect | Contain |
|---|---|---|
| Unsafe omission or low-confidence green | Full shadow comparisons, changes targeted at each declared input edge, independent expected-versus-executed report and test inventories, periodic full actual merge groups | Widen to full selection on unknowns and restore the previous authoritative gate. Preserve enough evidence to reproduce any discrepancy. |
| Cache poisoning, stale provenance or private data exposure | Verify manifests, trust partition, byte digests and lease/attestation identity. Compare forced cold outputs and test sets. Monitor rejected cross-scope reads or writes. | Quarantine the generation, disable the write path, fall back to a clean build, and revoke implicated credentials if identity policy is violated. |
| Protocol changes, unclear acquisition or lost results | One real GitHub canary per promoted profile and upstream update. Alert on renew failures, replays, unacknowledged completions, incorrect annotations and version rejections. Exercise cancellation, force-stop and loss independently. | Fence the controller, close admission on the affected scale set, keep existing leases supervised, and move new work to ARC and the official worker. |
| Site failure, saturation or repeated cold starts | By-site acquisition, ready and completion p90; cgroup pressure and OOM; compiler concurrency, image evictions and cache transfer tails | Keep gate reserve and background throttles. Move admissions to the available site and rebuild missing caches. Restore prior worker budgets or link/profile settings if the cohort regresses. |
| Privileged code reaches the host or another job | Qualify the boundary and no-token service account on the real runtime. Audit egress and mount policy and attempt identity. Track isolation conformance under worker and image upgrades. | Keep unqualified fork or hostile workloads on isolated hosted lanes. Quarantine an affected pool and rebuild it from pinned images before taking new work. |
| Release wrong SHA or broken main evidence | Compare successful push and queue evidence for the actual landed SHA, including release-trigger, artifact and attestations end to end | Keep the current release gate during shadow. On missing comparable evidence, run full push verification and keep publication closed until it passes. |

## Owner questions

1. May a promoted, qualified native worker execute the explicitly supported Linux jobs? That changes
   `docs/goals.md:16` and the official-runner premise of local verification at line 75. The historical V2-native
   description in `docs/deferred/self-hosted-runner.md:276–302` does not grant that authority. The immediate gains are
   available under the current rule.
2. Approve the `velnor-gen` cutover as the boundary for changing job shape and merge-group selection? The old engine's
   one-job-per-crate, full non-PR and no baseline reuse rules are in `docs/hard-decisions.md:21–90,101–113,223–229`.
   Also decide the stable workspace-wide compile scope in the new generator under decision 11 at line 486. The default
   proposal keeps that scope during migration, preserves feature-specific verification, and treats later scope
   reduction as a measured policy change.
3. Approve supervised same-input evidence for post-merge push verification and, separately, for cross-event
   test-result reuse? The plan first keeps fresh required verification on the merge group and uses push evidence only
   for independent release consumers and compatible duplicate work. Exact cache keys for compiler artifacts can ship
   independently of that policy.
4. Which workers may be dedicated to trusted dind or hardware-isolated fork work, and how much inter-org capacity must
   remain reserved? The slot estimate assumes all five workers are eligible. A public repository and the current
   shared untainted deployment constrain actual placement.
5. Is the private cluster a project-specific acceleration path alongside the portable hosted generator?
   `docs/goals.md:45,49–59` retains the hosted-runner and GitHub cache targets. Keep standard Actions compatibility
   and those goals for portable output. The shared site cache and warm-slot policy apply to the selected self-hosted
   fleet.

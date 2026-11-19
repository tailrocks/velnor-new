# Review by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

# Cross-critique of proposals E, F and H

Sources: `evidence.md` (the pack, cited by section or table), the four repositories at the pack's commits, and three kinds of check I ran for this review.

- **Code reads** in velnor-new, actions/runner and velnor-runners. Cited as `path:line`.
- **One job log**, fetched read-only: job 134981831502 of run 44354317267.
- **My own re-measurements** from the proposal round, marked "[mine]". They come from the pack's raw jobs JSON, pod events and local builds on an M5 Max. They are not in the pack, so treat them as one member's numbers.

## Proposal E

### 1. Weakest assumptions

1. **The 2–4 min warm verdict (110–235 s) needs a 50–120 s test stage.** E itself measured single tests at 274 s and 271 s. The pack saw one runner-host test over 540 s on macOS (D.5).
   - [mine] One slow test in isolation takes 179 s wall and 142 CPU-s with no block I/O. It is CPU-bound work in a debug build, so sharding cannot shorten it.
   - E says the target is conditional on test fixes, but no phase exit names them. The Phase 3 target of ≤4 min can only be met if the journal-test PRs (#1333, #1349) deliver what they claim, and the pack did not test that (D.5).
   - What settles it: per-test CPU-seconds for the 20 slowest tests, before and after those PRs, on one EPYC slot.
2. **The 35–70 s "longest required build" is at or under the floor for runner PRs.** [mine] 24% of the last 200 merged PRs touch `velnor-runner-host`.
   - [mine] Its lib took 82.5 s and its test lib 99 s under load, and 34.6 s for the lib under light load. Clippy is a second pass over the same 157k lines.
   - What settles it: `cargo build --timings` for a one-function edit in that crate, warm dependencies, in a 16-thread cgroup on a worker.
3. **A Velnor scheduler is "the recommended fleet architecture", but E never says what it buys over ARC.**
   - E's own protocol note says assignment is to a scale set, not a slot, so finer placement is impossible until Velnor can identify the job.
   - That leaves admission and warm slots as the benefit. Both are available under ARC through `minRunners` and a pod template with a node-local volume.
   - The recommendation needs one number: the verdict seconds or jobs per hour that ARC cannot reach.
4. **Phase 2 applies "realistic requests" before Phase 3 cuts the job count.** E budgets 4 heavy slots per worker, 20 in the fleet. A full run still fans out 31 cluster jobs (pack C.1), and several runs overlap.
   - E does not say which of the 31 jobs are heavy.
   - If most are, Phase 2 adds queueing and misses its own target of p50 ≤ 9 min.
5. **The Phase 1 targets assume the cluster absorbs the moved work at today's 12 s queue.** E states this assumption.
   - The eight CI jobs are small. `Source qualification` is not: [mine] about 2,400 hosted-seconds per run, 267 runs on 11-18.
   - Durations already rise with load: runner-host went from 3.7 to 8.8 min median between 11-16 and 11-18 (T6).

### 2. Factual errors

I found no number in E that contradicts the pack or the code. Three imprecisions:

- **Dispatch count.** The H2 bound uses "a three-dispatch critical path" (39 s). After the gates move, the path has four serial dispatches: Plan or Ledger CLI, the slowest fan-out job, Required verdict, Required. That is about 52 s at 13 s each.
- **Setup share.** The H1 arithmetic (9 s of setup, under 2% of a 494 s job) uses the heaviest job only. [mine] Across all Rust jobs the fixed overhead is 49 s median per job and 43.8% of Rust-job seconds. H1 still fails on the wall clock, but the per-job overhead is a runner-minute cost E does not count.
- **Runner version.** E cites v2.338.2 from the release API. The pack and `.velnor/runner.lock` pin 2.338.0. This is not an error, but the two should not be mixed in one design.

### 3. Missed bottlenecks or risks

- **The merge queue is missing from the model.** E's 4.5–10 min "push to merge" assumes an open slot.
  - [mine] On 11-18 the median from last queue entry to merge was 282 min, with `max_entries_to_build = 2`. 17% of queued PRs were re-queued at least once.
  - E has no target for queue throughput.
- **Flakes.** [mine] 10 of 96 push runs failed on a commit whose merge-group run was green. The pack shows 14 of 50 merge-group runs failing (T2). With two required runs per merge, false failures cost more than any second E budgets. E has no flake target and no quarantine rule.
- **Cancelled work.** E counts the 45% of PR runs that are cancelled but draws nothing from it. [mine] One merged PR costs about 4.8 PR runs and 347 runner-minutes.
- **Suite growth.** [mine] The runner-host suite grew from 232 to 382 tests in two days, and its summed test time grew tenfold. Without a per-test time budget, any target erodes within weeks.
- **No effort or order of work.** The plan lists what to qualify. It does not say what gets built, by whom, or in how many PRs. PR #1324 sizes the Kubernetes runtime at 12 PRs, two of them large and high-risk (pack D.9).

### 4. Ideas worth taking

- **Executed-job recount.** PR runs execute a median 25 of 39 jobs. The pack's "39" counts job records, including skipped ones. [mine] My recount agrees: 17 fan-out jobs plus 8 hosted.
- **DAG replay for the routing counterfactual** (12.0 / 14.1 min). It is the right way to state what Phase 0 buys, and it matches my zero-queue floor [mine: 10.9 min over 12 runs].
- **Shard by the longest indivisible test, and fix long tests before raising the shard count.** F and H both get this wrong.
- **Assignment semantics.** The scale-set API gives placement at class level only until proven otherwise. See Cross-cutting.
- **One clean sandbox per job; never reset and reuse a job-bearing process.**
- **A numeric gate for the native executor**: a further ≥30 s on p90, or ≥15% more verified jobs per hour, in a paired trial with ARC kept as the control.
- **Verdict hygiene.**
  - The plan and gate come from a released version.
  - Reports bind run, attempt, tree and plan digest.
  - A missing, duplicate, stale or skipped report fails the verdict.
- **Restore a cache only when transfer plus unpack beats the rebuild.** At 13.6–22.4 MB/s an 883 MB bundle costs 39–65 s.
- **The linker baseline is already lld.** Rust has defaulted to it on x86-64 Linux since 1.90, so measure before adding mold.
- **The 5-minute merge wait does not apply** while `min_entries_to_merge` is 1.
- **A nextest archive covers neither doctests nor direct harnesses.**

### 5. Verdict

E is the most careful of the three and the least wrong. It would not reach the minimum, because it is a qualification protocol more than a design: almost every mechanism is a range to be measured, and nothing says which node-local store is built first or how a job gets its slot. Its targets of 2–4 min are defensible only after test fixes that sit in no phase gate. The single biggest flaw is that it models CI runs and not the merge pipeline. The queue depth of 2 and the 10% false-failure rate are the largest terms in push-to-merge on a bad day, and E does not address either.

## Proposal F

### 1. Weakest assumptions

1. **"Kubernetes schedules slots, the controller schedules jobs into slots."** Under the unmodified official runner the controller cannot do that.
   - `generatejitconfig` takes `{name, workFolder}` and no job id (pack F.1.3, `client.go:765-796`). GitHub assigns an acquired job to whichever registered runner of the set polls.
   - The message carries `jobDisplayName` and `requestLabels` (`types.go:21-57`), not the root or lock hash that F's `JobNeeds` requires.
   - Per-job warm placement therefore needs either one scale set per cache class or a Velnor-owned listener. F puts placement in Phase 3 and its own protocol client in Phase 4.
   - What settles it: acquire two jobs of different classes at once, start two JIT runners on chosen nodes, and read `runnerName` in the `JobStarted` messages.
2. **Native Plan and verdict in Phase 2** (§3.4, §5.4: "controller-side computations, not jobs").
   - Every fan-out job gates on `needs.plan.outputs.not_affected_jobs` (pack C.1). `Required verdict` and `Required` are workflow jobs that ruleset 24396608 names.
   - For GitHub to see those jobs finish with outputs, something must acquire them from the run service and report through Results. That is the Phase 4 protocol client, gated on a ≥15% overhead proof.
   - Posting through the Checks API instead cannot supply `needs` outputs.
   - The Phase 2 gate of ≤7 min therefore depends on work F defers and may cancel.
3. **Scrubbed, long-lived slot pods that run PR code.**
   - The gain is pod creation, and it is small: [mine] 5.6 s median from scheduled to runner started over 433 pods, and 14 s queue for on-demand pods against 3 s for idle ones.
   - Pre-created single-use pods get the same gain.
   - Reuse rests on a process and mount census being perfect. Legacy issue #275 records a persistent store poisoned by an interrupted compile (pack E.6).
4. **Runner-host tests in 35–50 s with six shards.** The estimate divides the step median (192–246 s) by six.
   - Single tests take 270 s (E) to over 540 s (pack D.5).
   - F's own 12-CPU limit replaces today's 96 test threads.
   - F says to "cap schema-compat tests to fixtures" in §3.3. No phase lists that work, yet the Phase 3 gate of ≤60 s depends on it.
5. **Skipping unchanged test binaries.** The proof is "identical binary plus env class".
   - It needs link output that is reproducible bit for bit, which nobody has measured here.
   - It ignores inputs outside the binary: fixtures, Docker images, and helper binaries the tests spawn, such as `velnor-local-test-job-hook`.
   - The payoff is small. A binary is unchanged mostly when its workspace is untouched, and selection already skips that case.
   - What settles it: digest stability over two clean builds, and a count of skips beyond selection across the last 200 PRs.
6. **"The within-job 40–74% hit rate proves the keys are stable."** It does not. Passes inside one job share the path and the environment.
   - The repository has better evidence: 98.2% and 94.6% hits, but also 19 registry misses per warm run without `SOURCE_DATE_EPOCH`, and 80.2% for the skeleton consumer (`docs/proposed/compiled-state-persistence.md:144-150`).
   - The conclusion is probably right. The inference is not.

### 2. Factual errors

- **"All 39 jobs run on pull_request, merge_group and push" (§1.4).** PR runs execute a median 25. The pack's own method note says "ran" includes 0 s jobs, and each job has `if: !contains(needs.plan.outputs.not_affected_jobs, …)`.
- **"`compiler_process_budget = 2` throttles a 96-thread node to 2 compiler processes per job" (§5.3).** The budget "bounds the preview producer's `build_jobs`; it does not change crate jobs by itself" (`docs/implemented/native-rust-crate-jobs.md:85-87`).
  - Only the maintenance tasks are capped, by `--config build.jobs=2` in `mise.toml:90-193`.
  - The claim also contradicts F's §1.2, which reports pods at 88 cores.
- **Mold "3–8× vs GNU ld".** The baseline for 1.99.1 is rust-lld, so the 5–15 s per heavy job is measured against the wrong linker.
- **Slot arithmetic.** Eight slots of 12 CPUs with request equal to limit use all 96 threads. Tuning item 3 reserves 3 CPUs for kube and system, so the eighth slot never schedules. The variant with one 24-CPU slot also sums to 96.
- **The 14 GiB memory limit.** The heaviest observed build peaks at 13.4–16.8 GiB (pack B.3.3, B.2.2). F calls this marginal. At 16.8 GiB the pod is killed.
- **Dropping the merge wait from 5 min to 0–1 min.** `min_entries_to_merge` is 1 (pack C.1), so the wait never applies to a non-empty group. The change buys nothing.
- **Push tree equals merge-verified tree: "always true".** F's own source gives 120 of 159 (T7). The other 25% need an explanation before a skip rule relies on "always".
- **The §5.2 selection formula.** The affected set is `diff(PR tree, merge tree) ∪ diff(base, merge tree)`. The second term is the whole PR, so the set is never empty for a real PR. That contradicts "tree-identical means a ledger check, not a build".

### 3. Missed bottlenecks or risks

- **A deeper queue amplifies flakes.** F raises `max_entries_to_build` from 2 to 5 under HEADGREEN. With about 10% false failures per full run [mine], each failure ejects an entry and rebuilds every group behind it. The flake rate must become a gate before the queue deepens. F has no flake metric.
- **The new controller lives in the slowest crate.** Phase 1 puts a Kubernetes runtime, slot pools and a cache service into `velnor-runner-host`.
  - That crate has about 60 open correctness issues, including #1303 ("reconcile_gate never observes GitHub") and #1206 (pack D.10).
  - Its tests already set the critical path. More code and tests there make CI slower while the fix is being built.
  - F gives no budget for this.
- **Hard CPU limits slow the uncontended case.** [mine] A cold runner-workspace build is 696 CPU-s, so at least 58 s at 12 perfect threads, with a serial chain of about 170 s through `turso_core` and the host lib. F admits this. Its gates measure medians on a busy cluster and would not show it.
- **tmpfs workdirs.** They count against the pod's memory limit. With a 14 GiB limit and an 8–16 GiB tmpfs, the build and its scratch compete for the same memory.

### 4. Ideas worth taking

- **Two lanes with fail-safe admission.** The official runner stays as permanent oracle and fallback. Cutover is by a separate scale-set name, with label-flip rollback. Never run two controllers on one set.
- **The Phase 4 entry gate.** Measured official-runner overhead must be ≥15% of the warm PR wall, with a lines-of-code budget, and stopping at Phase 3 is a stated good outcome.
- **Delete the tip check.** An object keyed by its inputs stays valid whatever main does next. Attestation becomes an audit log. Trust is carried by write scopes: main, per-PR, per-fork.
- **dind as an on-demand pool** for jobs that declare Docker, not a sidecar on every job.
- **Consistent-hash placement** from root and lock hash to node. It works at class level without per-job binding.
- **Duration-balanced shards** from recorded per-test times, not hash partitions.
- **A tree-based rule for the push skip**, not an event-based one, with fault injection against the nightly full run before selection is enabled on merge groups.
- **Phase 0 measures the cross-job mbx hit rate** on one node with a throwaway volume, before any runner work bets on it.
- **Auxiliary workflows leave the merge path's queue.** `Source qualification` and `Job image candidate` made up 573 runs on 11-18.
- **Legacy's incidents (#670, #671, #389, #408) become regression tests** for any native lane.

### 5. Verdict

F is the most complete architecture and its ordering of levers (queue, duplication, cache, then execution) matches the evidence. It would plausibly reach 5–6 min for a typical PR. The 2–4 min figure depends on test fixes that sit in no gate. The single biggest flaw is a hidden dependency: per-job warm placement and native Plan and verdict both need Velnor to own the job protocol, which F defers to Phase 4 and makes optional. Phases 2 and 3 as written cannot be delivered on the official runner. The slot design also takes a security risk (scrubbed reuse) to save about 6 s.

## Proposal H

### 1. Weakest assumptions

1. **The time-to-verdict table is below the measured floors.** It claims 42–50 s typical, 85–105 s heavy and 120–135 s for a lockfile PR.
   - *Heavy build, 28 s.* [mine] The runner-host lib alone took 34.6–82.5 s on an M5 Max, the test lib 99 s under load, and 26 test binaries relink after any edit.
   - *Tests, 38 s.* The arithmetic is `246 s × 0.55 / 4`, which treats wall time as divisible. One test takes 179 s alone [mine]. The 0.55 comes from PR descriptions the pack did not verify (D.5).
   - *Lockfile PR, 55 s build.* [mine] A cold runner-workspace build is 696 CPU-s with a serial chain of about 170 s.
   - What settles it: one timed run of each scenario on a worker in a 16-thread cgroup.
2. **Binding a cache slot into a running standby pod, raising its priority, and choosing dind and trust class after the job is known.**
   - Pod volumes and priority are immutable after creation. A late mount needs a privileged node agent that mounts into the pod's namespace through mount propagation. H never says so.
   - Standby pods exist before the trust class is known. The "untrusted namespace with NetworkPolicy" therefore needs its own pool per trust class and per dind variant, not two pods per node.
3. **Pickup in ≤1.5 s.** The standby pod has not started `Runner.Listener`. It waits on a FIFO for the JIT config.
   - After the job arrives it still pays `generatejitconfig`, the .NET start, the broker session and `acquirejob`.
   - [mine] ARC's idle runners, already registered and polling, start a job in 3 s median.
   - ARC reaches that 3 s path for the whole fan-out by raising `minRunners` above 4, at about 45 MiB per idle pod (pack B.2.2).
   - What settles it: time the four steps on one pre-created pod.
4. **Per-job cache affinity from `JobAvailable`.** This has the same binding gap as F. The JIT config is per runner, not per job. With 8 to 31 jobs acquired at once, GitHub decides which runner gets which job.
5. **A reflink clone of the base snapshot in "~0.5 s", and Cargo reusing `target/` as fresh.**
   - `cp --reflink=always -a` clones per file. The root union is 8.31e9 B raw (`compiled-state-persistence.md:140`) across tens of thousands of files. Nobody has timed it.
   - Cargo fingerprints workspace sources by mtime. A fresh `actions/checkout` rewrites mtimes, so workspace crates are dirty on every job unless the checkout persists too. "Without even spawning the mbx wrapper" holds for registry crates only.
6. **The symlink fix for Bug 1.** `actions/cache` hashes the literal path strings into the cache version, and they come from `${{ runner.temp }}` (`ci.yml` Restore Cargo sources steps, lines ~338 and ~672).
   - A symlink from `/home/runner/work` to `/home/runner/_work` leaves `runner.temp` unchanged on the scale set, so the versions still differ.
   - Only H's alternative works: produce on the scale set, or use runner-independent paths as issue #574 already proposes.
   - The prize is small either way: a 4.5 s fetch saved, or a 39–107 s bundle download to save a 73–105 s cold build [mine].
7. **The schedule.** H replaces ARC with an unwritten Kubernetes runtime, a node agent and Sysbox in "days 11 to 18". Effort figures of "15 min" and "30 min" for cluster-wide changes are not estimates.

### 2. Factual errors

- **Summary: "push-to-merge latency from 26.7 to 77.8 minutes".** Those are PR run wall medians (T2). H never measures push-to-merge. [mine] It was 39, 62 and 402 min on 11-16, 11-17 and 11-18.
- **"4 to 6 seconds per job (measured, T9b)"** for JavaScript and .NET listener startup. T9b holds no listener or node timing. It is an estimate.
- **`/tmp` is not `$RUNNER_TEMP`.** The pod mounts `tmp` at `/tmp` and `work` at `/home/runner/_work` (pack B.3.3). H's own Bug 1 puts `RUNNER_TEMP` at `/home/runner/_work/_temp`.
  - tmpfs on `/tmp` moves neither Cargo home, rustup, the mbx store nor test scratch.
  - If it did, 8 Gi would not hold them: [mine] one job log shows a 7.1 GiB mbx store.
- **"tmpfs saves 30 to 60 s of fsync in runner-host tests".** [mine] The slow test does no block I/O and burns 142 s of user CPU. Node I/O pressure is 1.27%.
- **"77 of 77 sampled PR runs trigger all jobs."** T3 counts job records. The median executed is 25 of 39. What is true: [mine] the ten heavy jobs ran in 97.4% of PR runs.
- **"The 430 base tests of velnor-runner-host run twice."** 430 is the whole runner workspace on macOS (pack D.5). The host lib has 195.
- **Static CPU manager with requests 14 and limits 16.** The static policy gives exclusive CPUs only to Guaranteed pods with equal integer requests and limits. These pods are Burstable and stay in the shared pool. Sixteen threads also do not fit a 12-thread CCD.
- **Mold "3x to 5x over GNU ld.bfd".** Same wrong baseline as F.
- **"Switch `amd-pstate-epp` from `powersave`".** The governor is not observable read-only (pack B.1.6). H states a guess as the current state.
- **Hosted `ImageOS` "is `ubuntu24`".** For `ubuntu-26.04` the generated script expects `ubuntu26` (`.github/scripts/velnor-tools-cache-identity.sh`). The mismatch H describes still exists.
- **Removing `compiler_process_budget = 2`** does nothing for crate jobs (see F). The `mise.toml` part of the same recommendation is right.
- **Action archive layout.** It is `<dir>/<owner_repo>/<sha>.tar.gz` (`ActionManager.cs:1241,1285`), not `owner/repo/ref.tar.gz`.
- **Phase 0 target: PR median ≤6.5 min in two days on ARC.** The zero-queue floor today is 10.9 min [mine], or 12.0 min by E's replay, set by an 8.5 min job.

### 3. Missed bottlenecks or risks

- **The merge stage is absent.** The headline "under 2.5 minutes" covers one PR run. A merge also needs a merge-group run, the queue at depth 2, and a pass through a 10% flake rate.
- **The local-mode dedup cuts verification.** H replaces the default-feature test run with `cargo check`.
  - `local-mode` gates code on production paths (`storage_plan.rs:32,116`, `config/job_authority.rs:140`).
  - The release build refuses the feature (pack D.3), so the shipped configuration would never run its tests.
  - H raises it as an owner question but has already counted the saving in its targets.
- **Raising `docker-networks` max-threads from 4 to 12.** The limit protects Docker network tests (`crates/velnor-runner/.config/nextest.toml:20`). H gives no evidence that 12 is safe.
- **Persistent `/var/lib/docker` per trust slot.** Runner-host tests assert on Docker object ownership and absence. A daemon that carries state between jobs can break them or hide leaks.
- **Per-PR slot directories.** There were 454 PR branches in two weeks. Each directory pins a PR to one node and diverges from its base. H gives no size budget and no fallback when the node is busy.
- **`vm.overcommit_memory = 1` and higher dirty ratios** arrive with no measurement. F recommends the opposite on the dirty ratios, with no measurement either.

### 4. Ideas worth taking

H found the most concrete defects of the three. I verified these:

- **The `event.json` race.** The official runner rewrites the event file at the start of every step: `ActionRunner.cs:143` calls `WriteWebhookPayload`, which does a non-atomic `File.WriteAllText` (`ExecutionContext.cs:1335`).
  - `task_producer.rs:99-106` reads `GITHUB_EVENT_PATH` inside a background step and fails with `producer_event_invalid` on a truncated read.
  - Job 134981831502 shows exactly that line at 17:14:22.496Z, with exit code 125.
  - "All 9 failures" is extrapolated from this one log. The fix is cheap: snapshot the event file before the parallel group, or retry the parse.
  - Any other background step that reads the event file is exposed, so check the CI flakes for this.
- **Bug 2: seals made on hosted runners can never match on the scale set.** The compat key hashes `ImageOS`, the store path and the checkout path (pack C.3).
  - This explains what the tip race does not: ci-helper was sealed on 11-18 at 16:24, and restores were still skipped in 3,636 of 3,636 jobs.
  - Fixing the tip check alone would buy nothing.
- **`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`** (`ActionManager.cs:1232-1291`). It removes action tarball downloads with the unmodified runner. [mine] That is about 5 s of `Set up job` per job.
- **Rust in the image or a read-only node seed.** The scale-set tools cache is disabled by design (`scale-set:*) disable scale_set_image_unqualified`), and `RESULT.md:115` confirms Rust is not baked in. `Prepare pinned tools` cost 706 runner-minutes in the sample (T9b).
- **A node-local git mirror through alternates or `insteadOf`,** with unmodified `actions/checkout`.
- **A local `gha_cache` endpoint** so unmodified `actions/cache` restores from local disk.
- **A `fast-gate` pod class** without dind, with its own priority class, for Plan and the two Required jobs.
- **The crates mirror through `CARGO_SOURCE_*` environment variables.** This survives a custom `CARGO_HOME`, which is what broke the image-wide config on 11-16.
- **Scratch is on the OS disk.** `emptyDir` lands on nodefs while the data volume is empty, and the volume is linear, not striped (`PLAN.md:76`). The value is room for node caches, not I/O speed.
- **Drop the serial `Ledger CLI` job** by using the released helper (goals rule 12).
- **Remove `build.jobs=2` from the maintenance tasks.**
- **Restrict `job-image-candidate.yml`'s `push: {}`.**
- **Keep the official Worker,** so `docs/goals.md:16` stands unchanged.

### 5. Verdict

H is the best defect hunt and the worst forecast. Its Phase 0 list would give real gains this week on the existing cluster. Its design would not reach its own numbers: the targets sit below the compile and test floors, the Phase 0 target sits below today's zero-queue floor, and the merge stage is not modelled. The single biggest flaw is that estimates are presented as measurements. Several mechanisms (late bind mounts, mutable pod priority, static CPU pinning for Burstable pods, tmpfs for `RUNNER_TEMP`) contradict Kubernetes or the pod spec, so the plan would stall on first contact.

## Cross-cutting

### Where the three agree

- H1 fails as stated. Interpreter time is a few seconds per job.
- The hosted gate queue is the first fix, and the duplicate push run on the same SHA is the second.
- ARC's pod-per-job with `emptyDir` is why every cluster job is cold. Node-local caches are the main build lever.
- Kubernetes stays. The official runner executes jobs first. A native executor is rejected (H) or gated on a measured win (E, F).
- mbx stays and gets a persistent store. blaze is out for verdicts because it ignores the pin.
- `velnor-gen` replaces the 39-job shape. Build once, shard the tests, keep a nightly full run.
- Take ideas and tests from legacy, not its executor.

The evidence supports all of this. The agreement to replace ARC with a Rust controller is weaker than it looks. The pack shows ARC failing at persistence and warmth, and both are template settings: a node-local volume in the pod template and a higher `minRunners`. E's order is the one the evidence supports: fix caches, resources and warm pods under ARC, measure, then decide.

### Where they conflict

| Question | E | F | H | Evidence favours |
|---|---|---|---|---|
| Native executor | Paired trial, numeric gate | Phase 4, gated at ≥15% | Never | E and F. H's "<0.5 s saving" is unmeasured, and so is F's "5–20 s". |
| Pod reuse | Single-use sandbox | Scrub and reuse | Single-use, pre-booted | E and H. Pod start is 5.6 s median [mine]. |
| CPU control | Realistic requests, measure quota | Request equals limit, 12 | Requests 14, limits 16, static | E. T6 shows contention, but nobody measured the quota's effect. |
| Slots per worker | 4 × 16 CPU, 24 GiB | 8 × 12 CPU, 14 GiB | 6 × 16 CPU, 20 GiB | E or H. F's 14 GiB is below the 16.8 GiB peak. |
| Workdir on tmpfs | No | Yes | Yes | E. The slow test is CPU-bound and tmpfs is charged to pod memory. |
| Dirty-page ratios | Only if stalls are measured | Lower | Raise | E. I/O pressure is 1.27% [mine]. |
| Merge group | Fresh and full first, selection after shadow | Delta selection and tree reuse | Selection and tree reuse | Selection. It is already the generator's default (`docs/goals.md:86`). Tree reuse hits only 7 of 45 (T7). |
| Skipping tests | 0% at rollout | Skip unchanged binaries | Drop the default-feature run | E. |
| Mold | Measure against lld | Counts gains against ld | Counts gains against ld | E. |
| Typical verdict | 2–4 min | 2–4 min, 4–6 with headroom | 42–105 s | E and F, after test fixes. H is below the floor. |

### Hypotheses I now judge differently

- **H2 narrows.** I rated it "partly holds", for replacing ARC. E's assignment point, set against F's and H's placement designs, shows that the one part that must be Velnor's own for per-job placement is the listener half: session, `acquirejob` and the hand-off. The Worker and the controller are not required for it. Nothing in the three proposals shows a controller rewrite buying seconds that ARC with a changed pod template cannot. As a speed lever I now call replacing ARC unproven.
- **H4 splits.** "Local beats remote" holds more strongly than I had it. H's compat analysis and E's transfer arithmetic show the GitHub-cache path cannot be repaired into usefulness: restoring costs as much as rebuilding. "The layout knows which server a job runs on" is weaker. With five nodes and stores of a few GB, the main-scope store can sit on every node, and per-job placement is not available under the official runner anyway. That half is unproven.
- **H1, H3 and H5 are unchanged.** H's findings add to the case against H1: the archive cache, the git mirror and a local cache endpoint give the I/O gains of "native actions" with the unmodified runner.

### What none of the three has

- **A flake budget.** [mine] One run in ten fails on a commit that already passed.
- **A queue-throughput target.**
- **A per-test time budget** to stop suite growth.
- **An answer for the feedback loop** in which the runner crate being rebuilt is the slowest thing in CI.

---

## Key (added by the manager after the review)

- Proposal E: GPT-6.2-Sol
- Proposal F: Muse Wonder Full
- Proposal H: Gemini 4 Argon

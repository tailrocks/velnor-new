# Review by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

## Proposal F

### Weakest assumptions

1. **Per-offer selection between the official and native lanes (§3).** Scale-set acquisition reserves demand at the set level. JIT configuration takes a runner name and work folder, with no job selector. The expanded job request arrives after the runner receives a broker reference and calls the run service. F's `admit(&AgentJobRequest)` therefore runs too late to choose which Listener gets the job, and handing an already acquired job to a new official Listener has no demonstrated handoff. Use workflow-level routing to separate scale sets or prove a single acquire followed by a pre-execution handoff to an official Worker. Exercise mixed supported and unsupported jobs arriving together, cancellations, reruns and an ambiguous acquire before counting the fallback as available. `evidence.md` F.1.3, F.2.2, F.7.2. ARC `cmd/ghalistener/scaler/scaler.go:135`. Official `src/Runner.Listener/JobDispatcher.cs:393`.

2. **Tree or binary equivalence preserves the current confidence (§5.2–5.3).** The delta rule has an internal hole. If the PR tree equals the merge tree, the first diff is empty, while the second diff from base is still the PR's change. Returning an empty set requires proof that the old run discharged every relevant obligation. Identical tracked files can still produce different results with event, refs, history, settings, features, fixture images or services. A test binary digest leaves runtime reads out. Sampling 5% and running nightly can reveal an incorrect skip after merge. Settle this by comparing full and candidate plans and results in shadow mode, including same-tree/different-event pairs, base advances, a previously skipped check and a fixture-only change. Reuse needs a successful proof for the specific check and candidate inputs. `velnor-new/docs/hard-decisions.md:188–228`, `docs/implemented/affected-selection.md:265`, `.github/workflows/ci.yml:400,526–563`.

3. **Reusable slot pods have a safe and cheap reset (§6.1).** A new JIT identity and a process/mount census leave workspace state, writeable container layers and dind state as separate cleanup concerns. A running pod also cannot gain a new per-job Secret volume through a regular Pod update. The proposal contradicts its immediate-release promise by requiring proven scrub completion. Measure ready inventory, assignment through first step, cancellation-to-safe-reuse and replacement p95. Require cross-job residue and controller/node-loss tests before sharing a sandbox between jobs. Start with disposable job pods and keep cache persistence outside them. `velnor-legacy/crates/velnor-runner/src/trust_scope.rs:1–14`. `evidence.md` D.4, D.9, B.3.3. [Kubernetes Pod updates](https://kubernetes.io/docs/concepts/workloads/pods/#pod-update-and-replacement).

4. **Persistence alone buys 85–95% hits and one job per root warms a node (§4.3–4.5).** Within-job reuse proves reuse within the observed job settings. Per-slot stores, changing features and build-script inputs create separate cold paths. A lockfile is a useful placement hint and a poor predictor of which expensive outputs are available. A blob digest checks the bytes received, while a trusted producer must establish the action-key-to-output mapping. F would give merge-group code a main-scope write path ahead of protected-main acceptance. Prove hit rate, CPU saved, bytes, transfer time and isolation with the pinned mbx across real PR updates, feature variants, slot overflow and node loss. `evidence.md` A/T9a and C.3. `velnor-new/docs/proposed/compiled-state-persistence.md:135–149`. `velnor-legacy/crates/velnor-runner/src/mbx_store.rs:4–13`.

5. **The critical test stage divides into six shards and the gate can move into the controller (§5.3–5.5).** The largest current test is itself longer than F's entire 35–50 s shard estimate. Shards distribute test cases and leave a single case intact. Separately, the controller-side Plan/verdict needs a contract for GitHub job outputs, both required checks, reruns and signed evidence. Current verdicts read expected jobs and attempt-scoped artifacts, including the lints. Split or fix the measured outliers before timing archive-based sharding. Require a real merge queue qualification that covers missing, failed, skipped and cancelled obligations before replacing the gate. Jobs `134191420609`, `134191420582`, `134191420345`. `velnor-new/.github/workflows/ci.yml:501–512,522–625`.

### Factual errors

- **All 39 jobs run on PRs (§1.4).** T2 counts listed jobs and the pack's method includes near-zero jobs. The workflows explicitly skip unaffected jobs. `local-mode` appears in 59 of 77 sampled PRs and T4 lists 850 scale-set entries without a runner group. Thirty-nine describes the declared shape and the median count, with fewer obligations executed on affected PRs. `evidence.md` A/T2–T4, A/Method, C.4. `velnor-new/.github/workflows/ci.yml:212–214,649`.

- **The hosted critical path starts with Plan alone (§1.1).** Ledger CLI is a parallel prerequisite to Rust jobs. A late Ledger CLI can hold fan-out after Plan finishes. T5's Plan-to-last-Rust/Check interval includes that possibility. `velnor-new/.github/workflows/ci.yml:275–366,646–649`. `evidence.md` A/T5, C.1.

- **The official Worker always pays an archive download (§2, §3).** At v2.338.0, remote download-info resolution remains, while an exact cached archive or an enabled pre-extracted cache can avoid downloading the archive. H and C can use the same optimization with the official Worker. Official `src/Runner.Worker/ActionManager.cs:1024–1073,1235–1290`.

- **Every HEADGREEN push has a verified tree (§5.2).** T7 matches 120 of 159 successful merge-group runs by SHA and supplies no pushed-tree audit for the remainder. A successful queue build need never land. GitHub's wait setting concerns reaching a minimum merge-group size, which the sampled ruleset sets to one. Lowering five minutes cannot be booked as a per-merge saving. `evidence.md` A/T7, C.1. [GitHub merge queue settings](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue#configuring-continuous-integration-ci-workflows-for-merge-queues).

- **Eight Guaranteed 12-CPU slots fit with F's reservations (§8a).** Both published mixes consume 96 CPU. Three CPUs reserved leave 93 before other workloads. The 14 GiB cap is also below the observed 16.8 GiB runner-container sample, a likely source of retries if applied to all jobs. `evidence.md` B.1.2, B.2.2, B.3.3.

### Missed bottlenecks or risks

- A merge-group provenance label gives pre-merge code trusted cache authority under F's policy. F also gives forks an isolated write scope in §4.3–4.4 and describes restore-only fork access in §3.4. Adopt a single authenticated publisher on protected main, expiry for disposable PR outputs and explicit denial of private cached content to forks. Legacy had to resolve trust before shared store paths were chosen. `velnor-legacy/crates/velnor-runner/src/trust_scope.rs:1–14`, `gha_cache.rs:25–41`. `evidence.md` C.3.

- Privileged dind workers need a protected scheduling boundary before warm pods are shared. A deferred Docker obligation on a fork needs a route that executes it before merge. Approval alone does not establish code provenance or complete the check. F identifies reduced fork verdicts but leaves their merge semantics open.

- Archive packaging, fan-out transfer and contention for dind slots can add a second critical path. Track archive bytes, creation and runnable-shard latency, then compare those costs with splitting the loops and running more cases within one already warm pod. A plan to accelerate merges needs queue residence and enqueue-to-merge p90 as gates. PR wall and average merges/hour alone can conceal head-of-line stalls.

### Ideas worth taking

- Move gates first while keeping ARC available on a different label. Preserve the journal's intent/call/record ordering and UID-based absence proofs in any later controller. Those features protect cleanup during ambiguous failures. `evidence.md` D.4, D.9, F.7.1.

- Use immutable tools and sources, same-site cache fallback and bounded waiting for warmth. Measure cold startup and the build hit by a real PR before funding custom cache protocol emulation. The benchmark gate for a native lane is worth retaining once its reference is an optimized official path.

- Report selection decisions and compile reuse separately. Give the required check an explicit obligation record and allow a cache miss to trigger a build. The non-blocking Cache report makes lost warmth visible without changing what green means. `velnor-new/docs/goals.md:36–44,66–69`.

### Verdict

F would remove the measured hosted delay and much of cold setup, but its 4–6 minute typical PR verdict leaves the merge queue outside the main target. Its biggest flaw is passing merge candidates by inferred equivalence before establishing which obligations the earlier green run actually discharged. The performance plan needs an ARC baseline with warm resources, full merge-group verification and measured end-to-end latency. Treat the native lane as a later experiment with explicit routing.

## Proposal H

### Weakest assumptions

1. **The controller can direct an offered job to a selected warm pod in under 1.5 s (§3, §6).** The offer fields can inform how many pods to prepare. They contain no binding token that forces the broker to give that job to the selected JIT runner. The offers lack fork-head provenance too. The job-start hook runs after pod and runner selection. Show the actual assigned job, selected cache scope and actual pod when several jobs of different trust arrive together. Measure GitHub assignment, JIT, registration, broker poll and checkout separately. `evidence.md` F.1.3, F.2.2, F.7.2. Official `src/Runner.Worker/JobExtension.cs:303–313`. ARC `controllers/actions.github.com/ephemeralrunner_controller.go:834–860`.

2. **A passing local-mode suite and default cargo check preserve both test configurations (§5.4).** `local-mode` enables a dependency feature, adds code behind cfg gates and unlocks integration test targets. A default check gives compile checking over the requested targets, while default-mode execution and code generation are separate obligations. Inventory targets, features, flags and resulting test behaviors across both lanes. Deduplicate only cases with equivalent execution contracts and retain build/link and runtime checks for the configurations they verify. `velnor-new/crates/velnor-runner/crates/velnor-runner-host/Cargo.toml:12–13,59–78`, `src/lib.rs:42`. `evidence.md` C.5, D.5.

3. **The cache repairs recover realistic Linux hits and remove the storage bottleneck (§1, §4, §8a).** Distinct absolute paths and `ImageOS` are plausible compatibility-key causes. A symlink may preserve the spelled path. Rewriting compatibility fields can conceal real image, flag or filesystem differences. The `event.json` rewrite establishes a race candidate. One error string does not identify all nine producer failures or prove truncation in that job. Compare redacted producer/consumer compatibility inputs and cache version identifiers, reproduce restore on the actual image pair, and match rejected payload reads to the concurrent write. Measure mounts, write latency, PSI and the journal's CPU profile before changing storage. `evidence.md` A/T9a–d, B.1.4–1.6, C.3. Official `src/Runner.Worker/ActionRunner.cs:141`, `ExecutionContext.cs:1322–1336`. `velnor-new/crates/velnor-actions-orchestrator/src/task_producer.rs:99–111`.

4. **The full DAG reaches 120–135 s with current coverage (§5.2–5.5).** Four shards cannot turn the longest test into a quarter of its wall time. The diagram gives runner-host only two shards and packs actions-cli with archive checks. The 12–15 s Plan/lint budget also undershoots an existing 24 s Cargo Deny median before adding generator samples. Show identical per-check coverage and full-workflow timings with archive I/O, both status jobs, the slow tests, and cold/contended builds included. Include a lockfile/toolchain invalidation lane. `evidence.md` A/T4, A/T10, C.5. Jobs `134191420345` and `134191420609`. `velnor-new/.config/nextest.toml:16–19`.

5. **An official executable confers full compatibility through cache and runtime changes (§2–§6).** Custom hooks, JIT delivery, privileged bind mounts, local artifact endpoints and runtime sandboxes add behavior around it. `ACTIONS_RESULTS_URL` also serves official runner reporting. An endpoint proxy has to preserve log, summary, artifact and result semantics plus authentication. Settle this with a no-proxy control and end-to-end parity on job containers, service containers, composite post steps, failures and partial reruns. `evidence.md` F.3–F.5, F.7.4. Legacy `crates/velnor-runner/src/gha_cache.rs:3–41`.

### Factual errors

- **26.7–77.8 minutes is current push-to-merge and under 2.5 minutes is the design endpoint (§Summary).** The numbers are PR run wall medians for the scale-set sample and its 11-18 slice. H's table ends at PR verdict and has no enqueue or merge-group leg. T5's 57 minutes is the median total for three serial gate queues. Eight jobs are hosted. `evidence.md` A/T2, T5.

- **T4 measured pod startup at 12–18 s and all 77 PRs triggered all jobs (§1, §5).** T4 measures job created-to-started. ARC creates to assigned demand plus idle stock, so a fixed 27 cold pods per run cannot be derived by subtracting four from 31. T3 records varying execution counts and job guards implement skips. The 430 passing tests cited for each runner-host job were across the entire workspace on macOS with default features. `evidence.md` A/T3–T4, A/Method, D.5, F.7.2–7.3. `velnor-new/.github/workflows/ci.yml:214,649`.

- **Archive lookup is `owner/repo/ref.tar.gz` and preparation becomes instantaneous (§3–§4).** The implementation uses resolved `owner_repo/<sha>.tar.gz` on Linux, still resolves download info, and must copy/extract an archive hit unless using the pre-extracted path. Official `src/Runner.Worker/ActionManager.cs:1024–1073,1235–1315`.

- **`/tmp` is `RUNNER_TEMP` and stats summary proves disk placement and sub-second reflinks (§1, §8a).** The runner derives its temp directory from the work directory, and the observed pod has distinct `work` and `tmp` volumes. The pack distinguishes imagefs from nodefs and leaves the physical emptyDir mount and stripe layout unverified. First measure actual mounts and wait times. Official `src/Runner.Common/HostContext.cs:397`. `evidence.md` B.1.4–1.6, B.3.3.

- **Static CPU Manager assigns exclusive CPUs to the proposed 14-request/16-limit heavy pods (§6, §8a).** Those settings give Burstable QoS. Static exclusivity requires a Guaranteed pod and integer CPU requests. Even six 16-CPU limits would allow 96 CPUs of concurrent demand before reserves or dind. H also plans to raise the priority of an already-created standby pod, a field absent from the ordinary Pod update surface. [Kubernetes resource managers](https://kubernetes.io/docs/concepts/policy/node-resource-managers/), [Pod update rules](https://kubernetes.io/docs/concepts/workloads/pods/#pod-update-and-replacement).

- **Linker savings use a GNU bfd baseline (§8a).** The pinned compiler is Rust 1.99.1. The standard x86-64 GNU Linux target has defaulted to LLD since 1.90. Verify the effective linker under mbx, then compare mold to that binary on the same inputs. H's authenticated non-loopback HTTP mbx URL also conflicts with current upstream client requirements. Verify the exact pinned client and use TLS before treating L2 as working. `evidence.md` C.7. [Rust 1.90 release notes](https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/#lld-is-now-the-default-linker-on-x86_64-unknown-linux-gnu), [mbx cache server documentation](https://mr-boxington.jdx.dev/cache-server).

### Missed bottlenecks or risks

- SHA equality plus a `Required` check name does not establish event, trusted workflow identity, attempt or obligations. The documented check-run query can filter a check name, while H's predicate still needs provenance from the corresponding Actions run and artifact. PR-to-merge-group reuse also inherits affected PR skips. Current Policy Decision 3 keeps full runs on both protected events. `velnor-new/docs/hard-decisions.md:199–228`. [GitHub check-run API](https://docs.github.com/en/rest/checks/runs#list-check-runs-for-a-git-reference).

- H's hosted fork twin addresses a gate failure mode. It still needs an equivalent execution route for Docker obligations and a public/private data boundary around fork-visible reflink clones. A clone is isolated for writes but exposes its initial contents to its reader. `PreferSameZone` expresses a preference, and one cache endpoint per site creates both local failure and WAN fallback cases. The 2–4 s transfer estimate assumes unmeasured LAN speed. `evidence.md` B.5. [Kubernetes traffic distribution](https://kubernetes.io/docs/concepts/services-networking/service/#traffic-distribution).

- Removing Ledger CLI also removes the current on-head helper path from the crate jobs. The artifact carries both a binary digest and a source SHA. A released helper can run the new generator's protocol, but qualification must still exercise changes to the helper and its ledger behavior before release. `velnor-new/.github/workflows/ci.yml:275–365,647,759–767`.

- At 18 days the plan changes the workflow, runtime, storage and verification policy and uninstalls ARC. A one-command Helm install cannot promise to recover routing, actual warm inventory or in-flight work. A separate rollback label with ARC maintained during the soak is a cleaner experiment. The current runner version's fast path also needs direct timing before the proposal can claim a universal sub-0.5 s limit on native gains.

### Ideas worth taking

- Keep both official binaries during the first phases. Preseed pinned action archives and tools and move gates off the hosted queue using the existing controller. This captures savings with an unchanged execution baseline.

- Investigate the state restore mismatch and event-file rewrite as distinct problems. Take the snapshot of verified event data before parallel read-only consumers if the race reproduces. Record key compatibility and restore outcomes before making changes to the trust model.

- Build test executables once per compatible feature/configuration and use disposable pods with controller-owned leases and UID deletion checks. Preserve evidence for each configuration and add only shards that shorten measured critical-path time. `evidence.md` C.5, D.9.

### Verdict

H retains the strongest compatibility baseline in the executor and then spends confidence in the workflow. Its biggest flaw is the substitution of a default-feature cargo check plus a local-mode run for two distinct test configurations while claiming equivalent verification. The sub-2.5 minute claim does not cover push to merge, and the full-width verdict depends on unmeasured CPU, storage and test speedups. Keep its official-runner baseline and cache investigations. Require proof per check before taking the proposed shortcuts.

## Proposal C

### Weakest assumptions

1. **The historical fanotify read set determines the next verdict (§5.2–5.3).** A file-open trace on checkout and cache mounts misses failed lookups, metadata checks, some inherited inputs, environment/time, child mounts and external state. A previously empty fixture directory can gain a test input while all old file content hashes stay identical. C also needs to distinguish reads satisfied by the cache from the source/build inputs required for a cold execution. Evaluate reusable jobs in a constrained environment with declared inputs enforced, explicit directory/absence dependencies, and fail-closed overflow and attribution checks. Shadow full runs with these perturbations before allowing any green check to use history. `velnor-new/docs/hard-decisions.md:188–228`, `ci-helper/crates/velnor-ci/src/io/audit/dep_info.rs:16–44`. [Linux fanotify limitations](https://man7.org/linux/man-pages/man7/fanotify.7.html#BUGS).

2. **The 88% package calculation predicts genuine merge-group reuse and a 2–7 minute push-to-merge (§5, §8).** C flags its estimate as a package approximation. It also says Docker and network-dependent jobs always execute, then budgets some groups on every verdict being reusable. Package identity excludes several long named checks, configured variants and pending handoffs. The 19 s and 26 s medians are samples of two GitHub intervals, with the first unable to account for backlog before a particular build starts. Replay the exact checkout and queue-base history, preserve check inventory and run attempts, then shadow-score reusable check-seconds on live matched runs. Measure PR, enqueue, queue residence, speculative rebuild and merge separately. `evidence.md` A/T5, T7 and C.1. C §1.1, §5.7.

3. **A detached official Worker gives exact runner parity (§3).** The proposal's dispatch point is feasible in principle because it receives the full job after acquire. A Worker alone leaves C owning the Listener's first-renewal gate, cancellation/kill deadlines, identity/session recovery, crash completion and upgrade behavior. Two replicas with a Kubernetes Lease do not by themselves make the journal available, keep ownership exclusive through a partition, or preserve credential lifetimes for active Workers. Prove takeover and completion on a disposable set under real partial failures at every lifecycle boundary. Official `src/Runner.Listener/JobDispatcher.cs:393–405,473–548,700–718,734–790,1219`, `src/Runner.Worker/JobRunner.cs:290–332`. `evidence.md` D.4, F.2–F.3.

4. **A shared trusted mbx store plus a per-PR store behaves as a concurrent, read-only layered L1 (§4, §7).** Legacy used per-slot cache and target roots because the registrar flock and Cargo target lock contend. Current documentation for shared permits is useful, but gives no proof here that pinned 1.29.2 reads across a protected read-only L1 and a writable PR L1 or that the lock path scales on this fleet. The product of unit hits also misses the expensive residual work. Verify layering through the supported interface and test concurrent cold and warm jobs, prefetch lag, bytes, CPU-minutes and cancellation. Budget trusted warm-up after main changes across all five workers. Legacy `crates/velnor-runner/src/mbx_store.rs:4–13`. `velnor-new/mise.toml:20`. `evidence.md` A/T9a, C.3.

5. **Splitting slow tests removes their observed duration (§5.5–5.6).** `274 / 40` gives about seven seconds for an average case. Even ideal 16-way parallelism needs multiple waves and has a lower bound near 17 seconds for those cases alone at unchanged CPU demand, before fixtures and the remaining 115–136 s tests. Contention and serialized external resources may dominate after refactoring. Measure isolated cases and packed runs on Linux with the intended CPU/dind envelope. Shard on a measured end-to-end comparison including archive movement and slot queues. Jobs `134191420345`, `134894988058`. `velnor-new/crates/velnor-actions-cli/tests/image_publication_public.rs:86`, `crates/velnor-runner/.config/nextest.toml:15–24`.

### Factual errors

- **120 of 159 pushes repeated a verified SHA, and push reuse reaches 100% (§1.5, §4–§5).** The denominator is successful merge-group runs. T7 reports 257 pushes and no validated tree matching for the remaining successful groups or bypass/direct pushes. Keep a verification path for missing or incompatible evidence and count exact matches by actual landed push. `evidence.md` A/T7, A/Method.

- **Either the dispatcher or the Worker completes the run-service job (§3).** The diagram journals complete in the dispatcher and the lifecycle assigns it to the Worker. On the upstream normal path, the Worker uploads the job result, and the Listener handles failure paths and final coordination. The production design must give normal and exceptional completion distinct owners and settle what happens when the GitHub result arrives before the read-set store commits. Official `src/Runner.Worker/JobRunner.cs:274–332`, `src/Runner.Listener/JobDispatcher.cs:700–718,1219`.

- **The 11-17 final-push budget can attribute its residual to extra PR CI rounds (§1.5).** Earlier pushes precede the chosen start boundary, which is the first detectable run creation at the final head. Attempts on that final head could contribute, if joined and timed. Subtracting unrelated sample medians gives no measured residual attribution. The 85.1-minute statistic is valuable as C's reported proxy over 98 merged PRs. It does not include unmerged work and does not timestamp the actual Git push. C §1.1–1.5. `evidence.md` A/Method, T7.

- **Phase 4's comparison uses the same interval (§3, §8).** The roughly 17 s baseline adds 12 s created-to-started and about 5 s from runner setup. Assignment-to-first-step p90 needs a new assignment timestamp and a matched percentile. Likewise, `nodeName` bypasses the scheduler. With 1.2× admission and realistic requests, it can send a pod to a kubelet that cannot accommodate it. C §1.4, §6, §8. `evidence.md` A/T4. [Kubernetes node assignment](https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/#nodename).

### Missed bottlenecks or risks

- Six concurrent speculative groups can amplify invalidation when a queue entry fails, conflicts or is removed. HEADGREEN evaluates a prospective group head. Grouping and effective throughput depend on the queue and on available build slots, so test two against six under measured bursts. Auto-merge and dropping unnecessary catch-up pushes target the human/agent delay first. Validate that the current rules actually enqueue auto-merge candidates and measure the effect. `evidence.md` C.1. [GitHub merge queue behavior](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue).

- Site pinning makes local artifact reuse simpler and gives each site a smaller effective capacity pool. A single L2 or hot volume failure per site needs a cold rebuild path with deadline and artifact recovery rules. The same trusted builder warmed on each main advance can compete with merges and serve stale prediction indexes. Measure it as offered load and coalesce warm-up work.

- Reducing runner-host into 6–10 crates is a layout hypothesis. Crate boundaries can add more compile units, link work and critical dependency edges. Compare a representative edit workload after fixing journal preparation and serial tests before treating a split as required for Phase 3. The local macOS debuginfo probe does not settle the Linux link cost.

- Forks and unknown jobs need routing before assignment, with equivalent required suites on the hosted route and no pre-merge dependence on a trusted cluster rerun. C's summary keeps a safe default destination and its deployment diagram needs an explicit GitHub workflow route. Keep the current check's failure semantics through both routes.

### Ideas worth taking

- Use the end-to-end metric and keep the reported 98-PR timeline as a baseline with its caveats. Investigate delays between green, enqueue and merge, alongside runner execution. C is the only proposal that budgets those stages explicitly.

- Fix the measured critical path with ARC in place. The CLI fixture and loop, the runner-host journal and the maintenance-helper compilation and serialized goldens merit concrete work before blanket sharding or an executor rewrite. In jobs `134191420609` and `134191420582`, the reported slow test nearly accounts for the full runner-host test step.

- Use single-use sandboxes, scope cache authority to a protected-main publisher and keep ARC rollback. Preserve a per-job shadow decision log, declaring uncertain obligations ineligible for reuse. Adopt the tracer as evidence to improve input declarations before considering it as an authority to skip.

### Verdict

C is the strongest starting point. It targets push to merge, identifies slow code that the runner cannot accelerate, and defers its own dispatcher until after an ARC-based phase. Its biggest flaw is that the read tracer cannot establish that a prior success applies to the new job. The 88% reuse and 2–7 minute landing claims should remain hypotheses. Keep complete merge-group verification while measuring C's early phases and make reuse a separate qualification.

## Cross-cutting

- **Agreement.** Move the hosted gates first, reduce repeated setup, use the pinned compiler and keep the cluster as the execution substrate. All three identify unconfigured cross-job compilation reuse and reject blaze for the merge verdict. Start with a separate trusted gate path, official Listener and Worker, scoped immutable inputs and disposable job pods. T4–T5 locate the large delay. T9 distinguishes absent restores from cache-transfer costs. Blaze uses its own toolchain. `evidence.md` A/T4–T5, A/T9, C.7. [Blaze's toolchain policy](https://github.com/indexable-inc/blaze/blob/main/AGENTS.md).

- **Controller choice.** H keeps both official processes, F adds an optional native executor behind an unsupported same-set handoff, and C replaces the Listener before it replaces the Worker. The observed startup cost gives H's execution boundary and C's ARC-first sequence the strongest case today. ARC already supports idle runners and custom pod templates. Test that baseline with warm storage and real admission controls. Only build the replacement when residual measured assignment, placement or isolation limits justify owning these protocol edges. `evidence.md` E.6–E.7, F.1–F.2, F.6–F.7.

- **Verification choice.** C's selective reuse has the best direction and lacks a sufficient proof mechanism. F's tree/binary equivalence and H's same-tree or feature-superset shortcuts give weaker evidence. Start with H's narrow exact-SHA post-merge dedup idea, gated by matching trusted workflow/run, attempt, full check configuration and the policy for event-specific work. It can free capacity for future merges. Keep full merge-group runs pending owner approval and demonstrated equivalence of every reused obligation. `velnor-new/docs/hard-decisions.md:199–228`. `evidence.md` A/T7, C.4.

- **Parallelism and cache choice.** C's profile-first test work outranks H's fixed four shards and F's assumed six-way speedup. F/H's per-slot stores and C's shared store are competing deployment hypotheses. Compare them with the pinned mbx under real concurrency. Prefer bounded affinity, site-local fallback and protected-main publication. Measure whole-stage p50/p90, CPU spent, cache bytes and cancel waste, including under load. Keep phase gates on push-to-merge distributions as well as actual checks completed. `evidence.md` A/T6, A/T9–T10, B.2, C.5. Legacy `crates/velnor-runner/src/mbx_store.rs:4–13`.

- **Hypotheses after cross-critique.** H1 fails as a dramatic language-only speedup. H2 drops to unproven even for ARC replacement as a latency requirement, and a native executor has the weakest evidence. H3 partly holds, with realistic resource admission and isolation as conditions. H4 partly holds as stated. Local reuse has strong evidence as a direction and custom runner-integrated caching and affinity still need a comparison against uniformly warmed nodes with site-local fallback. H5 holds for proven unaffected work. Confidence in the proposals' numerical claims for safe merge-group reuse falls after reading them. `evidence.md` A/T4–T7, A/T9–T10, D.8, E.7, F.7.

The review uses the supplied evidence, cited source code and upstream documentation. The proposals' private timing scripts were outside the review scope. Their reported extra measurements are identified as such where used.

---

## Key (added by the manager after the review)

- Proposal F: Muse Wonder Full
- Proposal H: Gemini 4 Argon
- Proposal C: Claude Opus 5.5

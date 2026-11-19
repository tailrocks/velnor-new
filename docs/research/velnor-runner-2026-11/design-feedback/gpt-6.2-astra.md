# Design feedback by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

## Verdict

No. Move the gates and repair the slow tests now. The proposed ledger can certify work that the merge candidate still needs.

- **Ambition scores 8/10** because the design tackles verification reuse but leaves the biggest head start, computing while agents edit and the queue forms, until phases 4 and 5.
- **Soundness scores 6/10** because the official Worker and single-use pods preserve valuable guarantees, while transferable self-reports and an oversized warm pool undermine the promised result.

## Correctness and confidence holes

**Critical. A passing PR can cover less than the queue requires.** C1 quantifies over whatever the planner selected. The PR planner can legitimately skip a crate that the full merge-group policy requires, even on identical bytes.
Compute the required inventory again from the protected policy, actual event, queue base and checked-out candidate. Match evidence per obligation, configuration and expected test inventory. Rerun any obligation with a missing report or an unexpected zero-test result.
Keep the default and `local-mode` builds, stand-alone crate checks, doctests, direct harnesses and named maintenance checks distinct. An unknown dependency must widen the **current** run before `Required` can pass. A red, non-blocking `Selection` check arrives too late. `docs/hard-decisions.md:199–204`, `docs/goals.md:40–42`, `ci-helper/crates/velnor-ci/src/io/plan.rs:35–92`, draft §§1.2, 4.3, 6.2.

**Critical. A red queue prefix has already landed.** The ruleset uses `HEADGREEN`, `SQUASH` and groups of up to ten. On November 18, PR #1276's `Required verdict` failed at 13:44:03 UTC. The combined candidate for #1037 passed at 13:57:03. Both PRs merged at 13:57:15. The two commits sit consecutively on `main`, with #1276 (`d2eaecb3a1`) the parent of #1037 (`c0d0b51181`). The final `Required` job on the prefix failed at 14:00:16. Its only merge-group CI run was red. [Prefix run 44307877021](https://github.com/tailrocks/velnor-new/actions/runs/44307877021), [combined run 44308250346](https://github.com/tailrocks/velnor-new/actions/runs/44308250346).
GitHub's group-level rule therefore falls short of C1's per-commit promise. Require evidence for each landed prefix before reporting the final group green, or use `ALLGREEN` and verify each prefix under its actual policy. Preserve batching and parallel execution. Reproduce the red-prefix, green-head sequence as a cutover test. Decision S3, draft §§1.2, 6.5. [GitHub merge queue grouping semantics](https://docs.github.com/en/graphql/reference/pulls#mergequeuegroupingstrategy).

**Critical. Exact-tree receipts can carry unverified claims.** C4 permits self-reported results and the scope table makes them reusable by other runs. A passing PR under one event, filter or attempt can then discharge an obligation under another.
Have the verifier own the invocation, executable provenance, configuration, case inventory and terminal result for every transferable pass. Bind the repository ID, trusted policy revision, materialized inputs, platform and producer attempt. Include commit, history and event fields wherever observable. Keep signing authority outside the privileged Docker workload's trust boundary.
Run event-sensitive, live-advisory, real-time and publication duties fresh. Require promised outputs and artifacts before consuming a receipt. A later authoritative red or cancellation supersedes an older green for that attempt chain. The existing plan reads the event and its commit range. `ci-helper/crates/velnor-ci/src/io/plan.rs:177–191`, draft §§1.2 C4–C5, 3.5, 4.1–4.3.

**Critical. A hidden input can produce a passing test.** The draft assumes an undeclared read fails the test. The generator golden test enumerates the sample directories, checks only that the set is nonempty, and tests each visible sample. A mount exposing yesterday's samples can hide a newly added broken sample while all visible samples pass.
Key the complete directory membership from the candidate, modes, symlink targets, relevant environment and runtime image. Compare the restricted run against the ordinary run. An unaccounted lookup, incomplete trace or uncontrolled source of variation makes that obligation ineligible. Run the unmodified full obligation on a miss.
The build needs its own source-to-binary proof. A digest identifies the bytes received even when a stale build yielded them. Read tracing is diagnostic evidence for work that actually ran. `generator/crates/velnor-gen/tests/it/golden.rs:19–41,215–224`, `ci-helper/crates/velnor-ci/src/cli.rs:214`, decisions I037/N02.

**High. The new verdict can break image publication.** The publisher requires the latest `ci.yml` push run on `main` for the exact source SHA to have succeeded. That attempt must contain exactly one Actions **job** named `Required`, completed and successful. An App check with that display name leaves the job missing. A second job with the same name makes publication fail even if that job was skipped. The renderer in `7088ebbe31` creates exactly such a hosted fork twin when the gate is fenced. `crates/velnor-actions-workflow-renderer/src/required_gate.rs:119–142`.
Keep a thin push workflow with one real job of that name. Have it validate the landed SHA against the trusted merge-group evidence and execute any fresh duties. Alternatively migrate the publisher and its revalidation together. Exercise actual candidate-to-publication qualification in the lab, including a stale successful attempt, a skipped namesake, a direct push and fork admission.
Binding the ruleset to the shared Actions App authenticates the integration. The verifier must also authenticate the workflow and attempt. `crates/velnor-actions-orchestrator/src/image_maintenance/publication_github/ci.rs:50–70,84–90,105–142,202–217`, decisions S3, draft §§6.4, 13.6.

**High. The warm pod requests exceed physical RAM.** On a 125.5 GiB worker, reserve 6 GiB for the system, 2 GiB for eviction and 8 GiB for four gates. That leaves 109.5 GiB, or three 32 GiB builds total. Reserving a 24 GiB Docker pod leaves two. Four running builds alone request 128 GiB.
With three ordinary and two Docker workers, the ceiling is **13 build pods, idle and active combined**, before other tenants or services. Even at the observed 45 MiB idle RSS, Kubernetes accounts for the full request. Count each idle or running reservation once and back tenant guarantees with reserved capacity. A namespace quota caps use. Evidence §B.0–2.2, draft §7. [Kubernetes resource quotas](https://kubernetes.io/docs/concepts/policy/resource-quotas/).

**High. Admission can hide a queue and lose an acquisition.** The scale-set SDK sends its simultaneous runner maximum in the capacity header. The draft sends free slots and starts the wait clock at the offer. At eight slots with six busy, sending two needs server-side validation. A broker withholding offers could improve that clock while developers wait longer.
Measure GitHub-ready to first useful work, splitting ready-to-offer, offer-to-acquire, lease and start. Count free physical slots independently. Persist acquisition intent before the network call. Fence delivery by attempt, node and pod UID. On a lost acknowledgement, reconcile the original owner before allowing work or a receipt. Require proof the old process stopped before permitting a replacement.
Compare fixed total and dynamic capacity in the lab under mixed demand. Interrupt after remote acquire, after handoff and after remote completion but before local acknowledgement. Require one authoritative terminal result and zero duplicate side effects. `scaleset/listener/listener.go:20–21,159–164`, `arc/cmd/ghalistener/scaler/scaler.go:149–173`, `actions-runner/src/Runner.Listener/JobDispatcher.cs:393–417`, draft §§3.3, 3.7.

**High. The ten discordant runs need a diagnosis.** The 10/124 sample conditions on a green merge group and a later push on the same SHA. Different event inputs, runner load or a real intermittent defect can explain a red. Preserve the first failure while investigating the cause of a green repeat.
Classify paired results in both directions by obligation and environment. Preserve reds and keep required coverage blocking until an equivalent replacement passes. For a defect that appears with independent probability 10% per execution, two trials detect it with probability 19%. One trial detects it with probability 10%. Budget fresh repetitions for obligations that need this signal.
At 50 merges the proposed 5% sampler yields 2.5 pairs in expectation. With independent trials, zero disagreements in 299 gives a one-sided 95% upper bound just below 1%. Correlation reduces the evidence. Sample both outcomes, include peak load and keep the 1% claim provisional until enough pairs accumulate. Decisions S1/N14, draft §§1.2–1.3. Legacy counted 30 earlier failed attempts hidden by later successes. `velnor-legacy/plans/ci-performance/reliability.md:39–43`.

**High. The audit can compare a bad cache with itself.** The job can generate files during compilation, before the draft's post-build snapshot. Restoring that directory and the same saved mtimes for both the original run and its audit can reproduce a stale result twice. Three no-op builds in #585 demonstrate warmth.
Keep PR snapshots within the original writer's trust scope. A higher-trust run starts from a trusted build or independently verified outputs. Quiesce writers before sealing a generation. Give the differential audit a clean checkout, empty target and independent expected inventory. Exercise changed, deleted and newly present inputs, including build-script and feature changes. Rust still installs per job today. `ci-helper/crates/velnor-ci/src/io/inputs.rs:102–145`, `velnor-runners/state/RESULT.md:115`, draft §§3.4, 5.3. [Cargo build-script inputs and outputs](https://doc.rust-lang.org/cargo/reference/build-scripts.html).

**High. A partition can keep revoked receipts active.** Section 4.4 promises to disable a class on a disagreement, while §5.1 permits asynchronous replication and receipts live seven days. A partitioned site can still see yesterday's permission.
Give each reuse policy a revocable generation. Recheck that generation at the protected verdict, fence in-flight consumers, and run fresh when the authority is unavailable. Index pending candidates by receipt so revocation can invalidate them before merge. One small cross-site decision costs roughly one 35 ms round trip when healthy, against minutes of verification. Evidence §B.5, draft §§3.7, 4.2, 4.4, 5.1.

## Where the design is too timid

**1. Start verification on an immutable edit snapshot.** Phase 5 waits for an agent to request preflight. The earliest useful trigger is the first stable input for a costly obligation. Let the agent send changed Git objects to a private scratch ref. A trusted Linux worker can compile a root while the agent edits another. The official PR event still selects the obligations and authorizes the verdict.
Give each eligible computation one running producer. A later PR or queue run can join it before it finishes. Superseding one push detaches that consumer. Cancel the producer only after its last eligible consumer leaves. Retain an independent execution wherever repetition is part of the test.
For a 95 s obligation with 70 s of useful lead and 8 s to bind and report the real candidate, the estimate is `max(0,95−70)+8 = 33 s` after push, versus 103 s starting at push. A late edit to a required input spends that lead.
**Two-day test.** Record the immutable snapshots and push times for 50 consecutive final heads. Run the most expensive deterministic root on spare capacity. Count hits that finished before the verdict needed them, CPU spent on abandoned edits, and first-edit-to-merge as well as push-to-merge. Advance if matched pushes save ≥30 s median and foreground P90 holds. Draft M8 and N07 give no lead-time distribution.

**2. Predict the ordered queue prefix.** For the second PR in line, GitHub can test `main + A + B`. The draft predicts `main + B`. Use GitHub's preceding queue candidate as a possible base and update on enqueues, removals and new heads. Speculate first on the inputs shared by both likely candidates. [GitHub merge queue sequence](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue).

I replayed the evidence pack's 45 sampled groups with `git merge-tree` in a scratch object store. At each cutoff I took the latest observed PR run on the sampled branch and the latest recorded push to `main`. For an alternative base I used the newest distinct candidate for another PR created after that push and still running or already successful. Its actual place in the queue requires a separate timeline check.

| Lead before group creation | Latest recorded main | Main or one predecessor | Main or two predecessors |
|---|---:|---:|---:|
| 60 s | 16/45 | 36/45 with 71 candidate lookups | 37/45 with 84 |
| 300 s | 14/45 | 24/45 with 66 candidate lookups | 25/45 with 80 |

With the eventual base and the subsequently captured PR head, 41/45 trees matched. The replay uses run creation as a visibility proxy and knows which PR eventually queued. A separate read of the 44 PR timelines found that only **9/45** target entries were already queued at the 60 s cutoff, versus 45/45 at group creation. Predicting all eligible open PRs would incur additional misses.
At one minute's lead, 36/45 hits could hide **at most 48 s per sampled group on average** from fully overlappable work. A third base bought one additional match for 13 more lookups. Receipt eligibility and spare CPU still need measurement. The replay uses `~/.cache/ci-ambition/evidence/ci-raw/{dup_pairs.json,runs_ci.tsv}`, evidence §A T7 and decisions S4/N03.
**Two-day test.** Shadow the first two choices using actual webhook delivery times for every open eligible PR, including those that never enqueue. Compare predicted and checked-out tree, ready-by-deadline hits, and speculative CPU per useful second. Admit heavy work only within a spare-capacity budget. A mismatch falls back to execution on GitHub's actual candidate.

**3. Build a trusted generation once.** Five nodes rebuilding the same root spend capacity that PRs can use. Build the root once per compatible platform in a clean builder. Publish an immutable file manifest and changed extents or files. Coalesce intermediate `main` advances. Nodes prefetch likely generations and can build locally on a miss.
At eight merges per hour touching one root, five builders perform 40 incremental builds. One builder performs eight, saving 80% of **warmer build CPU** if their work is equal. One per site performs 16, saving 60%. A 100 MB delta costs at least 0.8 s on a 1 Gbit/s link. A whole 9 GB target costs at least 72 s before unpacking. Draft §§5.2–5.4, decisions I030/I044.
**Two-day test.** Replay 20 consecutive landed deltas on two workers. Compare local rebuild, verified delta transfer and coalesced warming under matched load. Record bytes, CPU, time to a usable generation and the next cold job's finish. Validate with a clean rebuild at fixed paths. Reject replication for any root where transfer delays foreground jobs or platform fingerprints disagree.

**4. Remove the serial plan and verdict job hops from generated workflows.** A released verifier can run the plan and dispatch the selected lanes inside one real Actions job, then join their results. Keep separate jobs wherever permissions, publication, outputs or user-authored `needs` make them observable. This experiment works under ARC.
The draft budgets 10 s for Plan and 10 s for Required. Keeping 7 s of planner work and 3 s to report would recover about `20−7−3 = 10 s` plus any intervening scheduling delay. This saving applies after hosted queues have been removed. Draft §§6.1, 6.6, decisions I013/N06.
**Two-day test.** Generate both graphs for the same 30 candidates in the lab. Compare inventories, output values, failure annotations, failed and cancelled lane handling, and wall time. Require zero semantic differences, a real successful push job for publication, and ≥10 s median savings before the ruleset cutover.

## The hardest problems

### 1. Get the entire required test lane under its budget

I removed the two named loop tests from the recorded timings and held every other case at its observed duration. All 28 sampled jobs still contained a passing test longer than a minute. Rescheduling can change those durations.

| Suite | Jobs with case timings | Median longest case today | Median longest case left | P90 longest case left |
|---|---:|---:|---:|---:|
| actions-cli | 10 | 260.7 s | 132.3 s | 141.5 s |
| runner-host default | 10 | 233.6 s | 124.8 s | 281.5 s |
| runner-host local-mode | 8 | 229.0 s | 137.6 s | 484.1 s |

The cohort is every row in the evidence pack's job manifest whose third column is exactly `Rust / velnor-actions-cli`, `Rust / velnor-runner-host` or `Rust / velnor-runner-host (local-mode)`. Open `logs/<second-column job ID>.log` beside the manifest. From nextest's PASS rows, exclude `public_authority_rejections_preserve_registry_inputs_and_foreign_resources` and `an_initializer_that_was_not_proven_absent_is_not_claimed`, take the maximum per job and interpolate percentiles. [Sample manifest](/Users/donbeave/.cache/ci-ambition/evidence/ci-raw/log_jobs.tsv).
Job `134649364528` still has a **924.8 s** receipt-integrity test that constructs the same pre-corruption state four times. The CLI has separate cases that publish a fixture family before testing a read-only recheck. Jobs `134649364381` and `134649364587`. [Runner receipt integrity test](/Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner/crates/velnor-runner-host/src/host_controller/loop/polling/restart_tests/bootstrap/receipt_integrity.rs:86), [CLI publication recheck](/Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-cli/tests/image_publication_public/family_publication.rs:61).

Build the expensive valid prefix once using the real code. Clone the closed, checkpointed journal or immutable publication artifacts into a private fixture for each case. Keep the production reopen, corruption rejection, registry readback and preserved-file assertions in each case. Exercise fresh migrations, historical upgrades and real process deadlines as separate required obligations when affected.
At eight available lanes, 40 equally costly CLI cases at `261/40` seconds each still need five waves, about 33 s. Six equally costly journal cases at `271/6` each still take 45 s in parallel. Reaching 15 s needs about a threefold reduction in the work of a journal case. The CLI also spends 89 s preparing its fixture. Jobs `134191420609`, `134191420345`, evidence §A T10.
Profile setup, assertion and teardown on quiet and loaded Linux workers. Measure cgroup CPU-seconds alongside elapsed case time. Rank all cases again after each change. Compare the two suites on historical journals and deliberately invalid fixtures. Preserve exclusive test groups and both build configurations.
Distribute binaries selectively. Moving the 1.48 GB archive to four workers in five seconds from one sender needs at least `1.48×8×4/5 = 9.47 Gbit/s` before extraction. Fetch the changed executable and its verified fixtures by digest. Require every expected case exactly once and measure the last case to finish. Decision I045, draft M6.

### 2. Make the first minute useful before GitHub assigns a runner

The replay of run `44307369619` found 497 s before the first job record. On the 13 successful PRs sampled on November 18 that interval had a P90 of 25.45 min. It is distinct from ready-to-runner queueing. A hot Worker cannot recover those seconds. `audit/replay.txt`, comparison §Evidence pack errata.

Begin work from the authenticated push or PR webhook, then rendezvous with the authoritative PR or queue SHA. The critical path is approximately `max(time to eligible evidence, time to authoritative candidate) + time to report`. A 20 s offer delay can coexist with a 45 s verdict. Keeping one Worker alive per idle pod by restarting it every 25 s would cost up to `[5×(4+2)+2]×3600/25 = 4,608` idle starts/hour at the draft's nominal pool size. Time a newly launched Worker in a pre-created pod first. Draft §§3.5, 7, M1/M3.
A small-PR budget worth testing is 8 s to receive the event, 4 s to dispatch, 10 s to rebuild, 30 s for the longest parallel lane and 5 s to report, or **57 s**. Of 200 merged PRs, 47% were generator/helper-only and 11% local-only or outside a Rust root. If all of the latter make it and the heavier groups take longer, about `(50−11)/47 = 83%` of the generator/helper group must meet the minute. Measure that distribution across the full cohort. [Workload sample](/Users/donbeave/.cache/ci-ambition/panel/draft/proposals/fable-5.2.md:269).

### 3. Carry verification across a moving base

Have the verifier start with the last certified base inventory and construct the candidate inventory, including added and removed tests, feature sets and policies. Each obligation must have a fresh pass or a still-valid proof that its complete inputs and semantics match. The service can inherit an eligible passing obligation from a trusted base even when **neither** the PR nor the whole queue tree matches it.
Start with one deterministic comparison, such as checking rendered bytes against the complete golden fixture for a sample. Vary the sample list, a symlink, a sibling file, an environment variable, the helper and the candidate's base. The fast verifier and a clean full verifier must reject the same bad changes. Unsupported classes run normally. Reuse for publication and real concurrency tests stays at zero.
Measure saved **critical-path seconds** and all-obligation coverage. An 88% package overlap says little if the remaining 12% contains the 260 s test. Always rebuild the actual combined candidate's inventory, including all predecessors. Evidence §A T7, decisions S4/I063, comparison §Numbers that conflict.

### 4. Give ready jobs capacity at burst time

Across all five workers, the three CPU snapshots averaged **60%, 82% and 79%** of 480 logical CPUs. Individual workers reached 92–95%. In the last sample the DE pair used 177.4/192 CPUs while an FI worker used 23.7/96. A fixed site rule can strand useful capacity. Evidence §B.0, §B.2.1.

Reserve gate capacity and a merge build slot at each site within real CPU and memory budgets. Give each tenant a floor. Lend unused capacity to interruptible warming and speculation. Let a portable build cross sites when the wait it avoids exceeds measured transfer and rebuild cost. Keep privileged Docker work on its designated nodes.
Under ARC, use separate scale-set labels for reserved work. GitHub assigns the identity, and its JIT request has only a name and work folder. For finer placement, let the recipient of one acquired job choose where its Worker or verified test subprocess runs. Time that handoff and test cancellation before relying on affinity savings. Evidence §F.1.3, `arc/cmd/ghalistener/scaler/scaler.go:149–173`.
Replay the busiest hour with all three tenants and a worker unavailable. Compare queue depth 2, 4 and 8. At eight entries/hour, depth two needs a mean build residence below `2×60/8 = 15 min` to keep up under the one-entry-per-build model. Ejections and repeated prefixes consume that budget. Raise depth only while completed merges/hour increases and other tenants' tail latency holds.
Define the operational promise as zero median dispatcher backlog and ≤5 s P90 from GitHub-ready to start for gate and merge work inside the measured arrival envelope. Publish overflow and waits outside that envelope as well. A burst of four 32 GiB builds on one ordinary worker already exceeds its three slots. Draft §§3.4, 6.5, 7, decisions I075/I079/I092.

### 5. Keep concurrency defects observable without serial reruns

Separate application deadlines from test-harness deadlines. Drive the application's clock and synchronization events explicitly where they define the contract. Retain real OS process, signal and timeout integration cases on isolated capacity. The current runner already freezes Tokio time while a fixture holds a stalled request. `crates/velnor-runner/crates/velnor-runner-host/src/host_controller/loop/polling/restart_tests/removed.rs:117–150` at `09f030b542`.
For a test whose value comes from varying schedules, run independent seeds on the real candidate and record them. Two 12 s executions on separate slots can finish in about 12 s of wall time for 24 slot-seconds. Retain a failing seed as a blocking regression. Replay a week of discordant SHAs with the same event, image and pins on quiet and loaded workers. Classify product defect, harness defect, infrastructure failure or unresolved. Retain both outcomes through diagnosis.

## Decisions you would overturn

- **I053, I055, N01.** Replace the SHA/tree fast-pass as written with event-specific inventory reconciliation and observed receipts. Keep the thin `main` push witness that publication consumes. Evidence T7 counts 120 duplicate pushes among 159 green groups. Its 7/45 comparison uses PR heads fetched later. Neither number counts eligible obligations. `publication_github/ci.rs:105–142,202–217`.
- **I054, I069.** Remove the two-week clean read audit as authority to narrow every merge group. Enable independently closed obligations as soon as they qualify. Keep uncertain ones required. The golden test can pass for weeks with an incomplete sample directory. `golden.rs:215–224`, `docs/hard-decisions.md:199–204`.
- **I015, I075.** Replace the free-slot header and automatic depth increases with a tested total-capacity advertisement and measured admission limits. Gate depth on sustainable merges/hour, wasted predecessor work, tenant P90 and true ready-to-start delay. Ruleset 24396608, evidence §§A T5, B.2, F.1.3.
- **I030, I044.** Replace the mandatory per-node rebuild on every merge with one certified generation per compatibility class and demand-driven replication. Compare one build per site against local rebuilds using the same main deltas. Evidence §B.2, draft §5.3.
- **I003, N05.** Make dispatcher replacement conditional on demonstrated custody, admission or placement benefits. Remove the 25 s idle Worker recycling plan unless a measured startup win pays its fleet cost. Start the protocol fault tests immediately. ARC already starts jobs on idle runners in about 3 s. Decisions I081 and S6.
- **I013, N06.** Test a single real Actions verification job under the generator now. Keep a controller-completed job restricted to a declared pure computation whose required outputs are reproduced. Arbitrary jobs can have post-steps, uploads and other observable effects. `actions-runner/src/Runner.Worker/JobRunner.cs:290–332`.
- **N03, N07, N09, I116.** Move the queue-prefix, pre-push and webhook experiments into phase 0. They need read-only event history and an isolated verifier. The 36/45 replay justifies trying them while the fleet work proceeds. Their results can change which dispatcher features merit implementation.
- **I119, N14.** Call S1 a discordance rate. Withhold the 1% false-failure claim until paired observations and cause classification support it. Quarantine can retire a blocker only after equivalent required coverage exists. Decisions S1/S2, `docs/goals.md:36–42`.

## Targets

I would use these as phase exit criteria. Each pair is P50/P90 in minutes. Cost is mean verification runner-minutes per merged PR. The cache column is the minimum cross-job compile hit rate. Every latency target covers the repository mix, including cold starts and forks.

| Phase | Commitment | PR verdict | Push to merge | Runner-min/merge | Compile hits |
|---|---|---:|---:|---:|---:|
| 0 · gates and queue | Commit | 13/28 | 35/100 | ≤320 | ≥0% |
| | Stretch | 11/20 | 25/60 | ≤285 | ≥0% |
| 1 · fixtures and classes | Commit | 8/18 | 18/45 | ≤175 | ≥0% |
| | Stretch | 6/12 | 13/30 | ≤135 | ≥25% |
| 2 · root jobs and trusted snapshots | Commit | 4/10 | 10/25 | ≤80 | ≥70% |
| | Stretch | 3/7 | 7/16 | ≤55 | ≥85% |
| 3 · admission and verified exact-input reuse | Commit | 2.5/7 | 9/22 | ≤50 | ≥80% |
| | Stretch | 1.5/4.5 | 4.5/11 | ≤32 | ≥90% |
| 4 · verified closures and queue prefixes | Commit | 1.5/5 | 5/13 | ≤40 | ≥85% |
| | Stretch | 1/3 | 3/7 | ≤22 | ≥93% |
| 5 · verification during editing | Commit | 1/4 | 4/10 | ≤35 | ≥88% |
| | Stretch | 0.5/2 | 2/5 | ≤18 | ≥95% |

**Basis.** Cluster-era PR run wall time was 26.7/92.3 min. The 85.1/456.6 min push-to-merge baseline starts at first CI creation on the final head (98 merged PRs). The 50-successful-PR routing replay gave 11.37/18.68 min to verdict with service times held fixed. Two such median passes plus the observed 19 s and 26 s GitHub handoffs give a planning example of 23.5 min before other waits. Measure the joint distribution from timelines. Evidence §A T2, `audit/replay.txt`. [Historical push-to-merge sample](/Users/donbeave/.cache/ci-ambition/panel/draft/proposals/opus-5.5.md:35).
The phase 1 cost budget allows, for example, 2.5 PR attempts × 40 runner-minutes + 50 for the group + 25 for other verification = 175. Phase 3's **9/22 min** commitment retains full merge-group coverage. The later targets depend on removing the measured test floors, reusing trustworthy builds and finishing work before the push. A 20 s verdict is a separate stretch for a preflight hit. The timing and tree replays use historical data. Startup, transfer and post-rewrite estimates still need matched worker runs.

**Cache denominator.** Count each required Cargo unit once per build at first use. Credit a hit only when its usable output came from an earlier job. Include changed units and cold builds. Publish the numerator, denominator, root, profile and CPU-seconds saved. Report fully skipped obligations separately. Phase 0 targets zero cross-job compile hits under the existing storage design. Phase 1's 25% stretch requires the early snapshot pilot. Track phase 1 toolchain hits separately at ≥95% after seeding.

**Clocks and accounting.** Time the PR verdict from the last push to the authoritative terminal red or green. Record push acknowledgement, webhook receipt, both required checks while both apply, queue entry and removal, and actual merge time. Include human approval delays in push-to-merge. Show the already-approved cohort alongside it. Count red verdicts and report superseded, open and abandoned heads so losing a slow run is visible.
Charge foreground jobs, remote execution slots, cancelled attempts, discarded predictions, warmers and audits to the repository that consumed them. Count each occupied execution slot once. Publish idle reserved core-hours, actual CPU-seconds and GiB-minutes alongside runner-minutes. A single Actions job fanning out to ten occupied one-minute workers spends ten worker-minutes on those workers.
Use consecutive cohorts over at least a week including peak hours, with ≥200 merges or an explicitly provisional P90. Reconstruct the old cost over the same accounting boundary. The draft's 285–347 runner-minutes is an estimate. A missed confidence gate keeps fresh verification mandatory regardless of latency. Draft §§1.1, 10, decisions I117/I118.

## Anything missing

**A set of changes the verifier must reject.** Give the new verifier a corpus of known-bad changes with green controls. Include a newly added failing golden sample, an omitted `local-mode` case, a stale publication attempt, and a broken queue prefix repaired by the next PR. Record which obligation fails under clean ordinary CI. Replay the corpus after every planner, sandbox or verifier upgrade, including with warm caches. A candidate implementation that passes one bad change loses authority for that class. This tests the ability to find a defect even during weeks when every real PR happens to be green.

**A site-loss test for the protected verdict.** The proposed FI standby obtains a Kubernetes Lease from a cluster with its single server, `velnor-237`, in the site identified as DE in the inventory. ARC shares that control plane. Losing that site also removes the API the standby needs for leadership. Test a control-plane partition while a job completes and a receipt is revoked. Fence new verdicts, preserve completed evidence, and time recovery. For uninterrupted admission through either site loss, place quorum votes in independent failure domains or provide a separately fenced external authority. Evidence §B.0, draft §§7, 14.

**Time to the first actionable failure.** The population had 419 failed and 1,156 cancelled PR runs out of 2,558. A last-push-to-merge improvement can coexist with slower feedback on every preceding edit. Alongside the required green, time the first reproducible failing case and retain its inputs and logs for the author. Run likely failures early while the full required set continues. Report first-push-to-merge and failed attempts per merged change as guardrails. Evidence §A T7.

# Is this the fastest runner? Critique by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Velnor speed review · tailrocks/velnor-new · 19 November 2026

## Verdict

**No. “The fastest runner ever” is unsupported, even for this repository.** Velnor has a measured advantage for one operation on this hardware: an unchanged local Cargo build over an XFS clone took 0.56 s including the clone, against 92 s cold. No end-to-end Velnor race has been reported. [Cluster experiment, P1–P2][cluster]
The historical GitHub-hosted sample finished PR runs in 15.8/22.0 min P50/P90. The later hybrid ARC sample took 26.7/92.3 min. Different changes, dates, workflow sizes and hosted queues prevent ranking even those two systems. [Evidence T2–T5][evidence]
Warm ARC, Buildkite-style persistent agents, Namespace cache-volume runners and Bazel/RBE all have credible ways to remove parts of the measured delay. None has a matched result here. Owner decision 20 supplies the strongest control: run GitHub Official and Native behind the same Velnor scheduler. [Owner decisions][owner]
The claim worth earning is **fastest measured push-to-merge on a specified velnor-new workload, at a stated resource budget, with the same verification contract**. An executor-speed claim requires a separate executor race.

## Where the time goes, per stage

Pairs below are P50/P90 unless labelled otherwise. M means measured, E estimated and T target. The forward column describes the design's *labelled generator/helper* example, 47% of 200 historical merged PRs. [Architecture §§1–4][arch] [PR classes §5.2][classes]

| Stage | What was timed | What Velnor budgets, and what still has to happen |
|---|---|---|
| Event and planning | Architecture reports 1–2 s push → run and 2/226 s run → first job. Ruling V5 separately measured 3/4 s main ref update → push run in 656 observations. Hosted Plan waited 5.5 min median across events. | 1–2 s webhook E, 10 s reconciliation interval. Budget the actual delivery, exact subject fetch, merge-tree, inventory and ledger lookup. Neither job clock measures a webhook. |
| Acquire | 13/27 s created → started on the scale set, 1,124 jobs M. An idle runner started within 3 s in 30%. | PR/group obligations use the App path. Release and scheduled Actions jobs retain acquisition. Their ≤1/2 s start T begins at the broker message. |
| Start sandbox | About 6 s ARC pod start across 436 starts M, plus Docker readiness where needed. | ≤0.3 s T. The measured 0.25 s clones a target directory. Pool admission, mounts, isolation, readiness and a second eligible node need their own timings. Pod startup can overlap acquisition. |
| Checkout | 6/8 s per job M, Plan about 12 s. | ≤1 s T, including a full tree check. The 0.1–0.3 s hash E for 110 MiB needs a small-file walk, modes, symlinks and actual cache-fault costs. |
| Tool preparation | 10/18 s M, plus about 2 s mise and 4.4 s action downloads per job. | Approximately 0 s E with the verified store mounted. Include a new image or toolchain in cold cohorts. |
| Dependency fetch | 3.5–6.1 s per job M, 0/48 source-cache hits. | 0 s warm, roughly 3 s on a changed lockfile E for **fetch alone**. A new dependency can trigger a much longer compile. |
| Compile | Runner-host executables 81 s, helper 14 s M. The 8.5 min job is maintenance-helpers, whose named check takes 486 s median across events. | ≤25 s T, 15 s example E. On the idle runner-workspace build, one lock edit rebuilt 11 crates in 67.4 s. An empty second node took 39.9 s to restore a previously built closure. Neither measures a typical helper edit. |
| Test | Helper 10/62 s, runner-host 192/417 s M. Removing two loop tests still left median longest cases of 125–138 s in the three sampled heavy lanes. | ≤60 s T, 10 s example E. New keys require two passing witnesses on distinct nodes. Include always-fresh, Docker and full-tree tests wherever selected. |
| Report results | A 1.48 GB archive upload took 36–43 s and Required verdict ran about 49 s M. | ≤1 s T. Time the last signed receipt through durable validation. Moving a test binary to another worker still costs bytes before it can execute. |
| Publish required status | Required verdict queued 1.6/24.3 min, then Required queued 1.9/21.4 min M across events. | ≤1 s T from the last receipt. Reporting and publication share that boundary. Measure through GitHub observing the correct App status. |
| Enqueue and verify group | Green → enqueue 7.5 min median, 0/40 recent merges used auto-merge. Enqueue → group run creation 19 s median. Group run 19.9/84.1 min M. | ≤30 s T, about 25 s E. A group derives its own complete vector. Even a tree hit can owe fresh work, and a tools-release group must qualify its shipped bytes. |
| Land | Group run finished → merge 26 s median M. | Design carries 26 s forward. Queue position, ejections and GitHub accepting the ref update remain outside executor control. |

**End-to-end clocks.** The 26.7/92.3 min PR proxy includes 27 failed runs among 77. The 50 successes alone took 27.7/79.2 min.
The 85/457 min landing proxy starts at the first observable CI run at the final SHA of 98 merged PRs.
Neither baseline observes exactly the proposed start and end events. Historical CI consumption is 285–347 runner-min/merge E. [Architecture §1][arch] [Evidence T2][evidence]

With whole-tree keys and the second build cold, Velnor estimates 6/18 min verdict, 9/30 min landing and 110 runner-min/merge.
The goal is 90/240 s verdict, 180/480 s landing and ≤50 runner-min. Stretch is 30/100 s, 100/240 s and ≤20 runner-min.
The illustrative 35 s verdict and 85 s landing are estimates for one class. All goal and stretch percentiles await a joint trace. [Architecture §§3, 4, 10.2][arch]

**The floor depends on the request.** After the authoritative inputs are available, a green waits for the longest outstanding dependency chain.
For each new obligation that includes the **later** of the two qualified witness completions. Within a runnable test wave, elapsed time is at least the longest indivisible case and total CPU work divided by available CPU capacity, whichever is larger.
Admission, prerequisite compile/link steps, serial resource groups and the final status can raise that bound. On a complete hit with no fresh work, the remaining path is subject resolution, proof validation and status publication.
Landing additionally needs the actual queue subject and GitHub's merge. The two observed medians, 19 + 26 ≈ 45 s, give a planning allowance, never a lower bound or even a measured median of their sum.
For a fresh group, add any required work still outstanding when the group exists. Per-job setup multiplied by 39 is slot time. Quantiles of those jobs cannot be added to recover PR wall time. [Architecture §§3, 6.2–6.3, 8.3][arch]

Release has another clock. Tools took 190–434 s and product releases 4,105–6,238 s historically. The ~15 s promotion E starts with qualified bytes ready.
Product dispatch can also wait for the group-SHA build and the cold landed-commit audit (A1), whose P90 target is two hours. Measure dispatch → published/read back as well as ready bytes → published. [Architecture §§6.5, 8.4][arch] [Ruling Q6][rulings]

## The strongest objections

1. **Most of the apparent win can accrue to an official runner. Likelihood high.**
   On 18 November, three hosted gate queues consumed a median 57.0 min in 13 successful PR runs with median wall 71.8 min. The cluster fan-out span was 10.7 min.
   The decision log measured interpreter overhead at 87–147 ms per action. Phase 0 alone projects a two-verdict agent cycle of 94 → 34 min, before Native is deployed.
   Move the gates and enable the same merge policy in the control, then change only the executor. Report the marginal wall-time delta on the same DAG and separately on Actions jobs. [Evidence T5][evidence] [Decision log H1][decisions] [Delivery §3.4][plan]

2. **The fastest path assumes too many reusable results. Likelihood high for some classes, unknown for the median.**
   In the source census, 79/86 files using `CARGO_MANIFEST_DIR` leave their package, 70 test files enumerate directories and 11 read the clock. The design anticipates whole-tree keys for most tests.
   A cache hit executes no test, so an access trace collected during executions cannot establish what would have been observed on a hit. A narrowed key needs a reviewed dependency proof covering absent files, directory membership, tools and nondeterministic inputs.
   Replay 300 real groups. Count whole-tree hits, complete vector hits, fresh obligations and time of the last miss separately. Re-execute every proposed hit in shadow against the full-tree cold inventory. Keep unproved classes fresh or whole-tree. [Architecture §§1.2, 6.1–6.5][arch] [Rulings Q5, Q16][rulings]

3. **Two simultaneous witnesses can share a blind spot. Likelihood unmeasured, consequence decisive.**
   Ten of 124 push runs failed on a SHA whose group had passed. That is a one-direction disagreement rate. It establishes neither independent trials nor which conclusion was correct.
   For a defect manifesting with probability 0.2, independent twins detect 36%, perfectly correlated twins only 20%. Reusing an old passing pair across ten requests supplies no further opportunity to catch intermittent behaviour. A class with uncontrolled variation must run fresh. Audit A1 can detect a bad landing after the fact.
   Compare simultaneous twins with time-separated runs of the same failing seeds under shared peak load, across different generations and test orders. Preserve the group rerun where equivalent detection has not been established.
   Zero disagreements in 3,000 independent representative comparisons bounds their rate at about 0.1% at one-sided 95% confidence. Replays of one defect family are correlated. A zero-false-green policy still needs a soundness argument and fail-closed execution. [Architecture §§1.2, 10.3][arch] [Ruling Q12][rulings]

4. **The claimed test floor mixes historical cases, suite totals and code scheduled for deletion. Likelihood confirmed in the source.**
   The 924.8 s receipt-integrity case lives inside `velnor-runner-host`, which §8.5 says will be deleted while also listing the case among work that stays. The test rebuilds its pre-corruption state four times.
   The quoted 679 s is a suite figure. The “~45,000 Git processes” explanation multiplies by all candidate globs, although only accepted globs enter the Git branch.
   Enumerating the generator at `20ea1f3406` gives 6,077 distinct candidates. Only 69 fit the grammar (6 one-segment, 36 two-segment, 27 three-segment). At seven direct Git calls each plus the explicit final star check, that path has at most 490 direct calls, excluding fixture setup and Git's internal children.
   Trace the actual test process tree and per-case CPU/wall, verify which assertions survive migration, and remeasure at the same revision on every runner. A deletion gets its own before/after inventory. [Differential test][git-test] [Grammar][grammar] [Receipt test][receipt] [28-job analysis][floor]

5. **One reserved merge slot cannot guarantee a two-node merge start. Likelihood high under saturation.**
   A request stays in one site. Each new key needs two distinct nodes, but each site reserves one 16-CPU build slot for merge plus a separate A1 slot. Some workers reached 92–95% CPU. Each fully read XFS clone populated 14.29 GB of its own page cache.
   A first witness starting immediately can conceal a long wait for the second, especially with a cold-build requirement or one worker drained.
   Replay actual bursts with A1 and other organisations running. Record eligibility → *second* witness start, slowest witness completion, memory pressure, A1 lag and completed merges/hour. [Architecture §§6.2, 9][arch] [Evidence §B][evidence] [Cluster E1][cluster]

6. **Queue hits and PR mix do not support a repository P90. Likelihood high.**
   Just 7/45 observed groups had the PR head tree. The 60–75% future estimate depends on auto-enqueue and verifying against the queue tip. The 172-entry replay measured quiet windows, including 74% at 2.5 min. It did not execute group obligations.
   Even at a 75% hit rate, if all misses are slower, overall P90 falls at the **60th percentile of misses**. A 47% helper/generator class alone cannot establish the repository median either.
   Shadow exact group SHAs at actual arrival times. Include ejections, multiple entries and missed webhooks. Reweight by post-deletion PR mix and publish miss-conditioned P50/P90 plus the combined observed distribution. [Evidence T7][evidence] [Ruling Q11][rulings] [PR classes][classes]

7. **The compute budget omits an identifiable occupied-slot interval. Likelihood high, magnitude unmeasured.**
   Section 4 estimates 37 runner-min/merge for the light class, including 10 for A1 and 8 for product preparation. Section 8.1 also has an Actions `Required` job polling A1 for up to 2.5 h, with per-SHA concurrency.
   When the audit and waiter occupy different slots, both count under §1.1. A 14-minute average occupied wait per merged PR would take 37 above the ≤50 goal before heavier classes enter. Count pushes and merges separately when one push covers multiple commits.
   Join actual lease intervals, including waiters, sampling, abandoned groups, product builds and mirror fills. Show CPU-seconds, GiB-minutes, network bytes and cost alongside occupied minutes. [Architecture §§1.1, 4, 8.1][arch]

## Missed speed

These are conditional gains to test against v3. Their benefits overlap and apply to different PR classes.

- **Keep the full Git differential and eliminate repeated setup.** Prepare a sealed no-checkout repository with both config values set and its sparse-checkout path resolved. Give each accepted glob a private clone of that fixture. Still write each actual pattern and run real Git `read-tree`, sparse listing and guard listing.
  On the current passing grammar, 70 evaluations including the final star check can replace four direct Git launches each with a local fixture clone, **up to 280 fewer direct launches**. The time saved is their measured cost minus clone cost. Compare every output set with the original, including new accepted patterns. [Differential test][git-test]
- **Release linked test binaries to the scheduler as they become ready.** Keep each binary and its invoked tools bound by digest to its own witness. Release only binaries whose fixtures and build-time dependencies are complete. A later build or inventory failure still fails the entire gate.
  If 20 s of tests currently wait behind 30 s of independent remaining links, overlap saves **at most 20 s** for that witness. Record link-ready, first-test and last-test timestamps and test the CPU contention this creates. Preserve order-sensitive groups and private mutable directories. [Architecture §§5.5, 6.2, 8.5][arch]
- **Reserve witness pairs by node when a merge miss is forecast.** Let restartable work borrow the second reservation while no eligible merge needs it. Keep A1's reservation intact.
  If the second node becomes free 25 s after the first, eliminating that wait can save **up to 25 s per affected group**. At 30% misses the ceiling is 7.5 s per merge on average under those assumptions. Reject the policy if displaced PRs or cold audits cost more than the landing gain. [Architecture §§4, 9][arch]
- **Have the push reporter perform eligible A1 work.** Let it claim the same durable per-commit audit work with an exclusive lease, still using the node agent's cold execution and signed evidence. Preserve reserved capacity, first-parent gap reconciliation and the required source-bound Actions check.
  For a single-commit push where reporter and audit would overlap for 10 min, using one executing slot throughout could save **10 occupied minutes**, with no speed credit for publishing earlier. Measure multi-commit pushes and reporter retries before adopting. [Architecture §§6.5, 8.1, 8.4][arch]
- **Choose the warm-node wait threshold from real queue observations.** The design proposes this experiment but supplies no operating threshold. For the exact P2/M3 workload, warm local clone + build costs 0.56 s and remote restore 39.9 s. With an immediately available equivalent remote worker, the illustrative wait crossover is **39.34 s** before transfer variance and provenance checks.
  Against the measured 92 s cold build that restore saved **52.1 s**. Fit separate crossovers for changed code, site bandwidth, verified generations and two-node availability. The Mac experiment rebuilt six workspace crates on new paths, and the RustFS Mac restore spent ~17 s extracting against ~1.2 s fetching. A storage hit alone predicts little. [Cluster P1–M3][cluster] [Mac experiment][mac] [RustFS R2][rustfs]

## What to measure first to earn the “fastest” claim

1. **Freeze the confidence contract before timing.** Pin workload revisions, expected IDs, features, Git/toolchain versions, timeouts, assertions, released planner and both exact subjects. Include new tests, changed helper binaries, missing outcomes, Docker, lockfile changes and known real failures.
   **Pass** only when all contracted cases agree with the independent cold oracle on both backends, every planted semantic fault is caught, every witness is accounted for, and unknown inputs force fresh work. A missed clippy deny already disqualifies the tested Blaze shortcut. Any unexplained false green disqualifies that configuration. [Architecture §§1.2, 5.7, 10.1][arch] [Blaze B5][blaze]

2. **Run two paired races.** First run identical job graphs on Native and GitHub Official with separate executor-keyed caches, equal warm state and the same hardware. Then compare complete pipelines with identical obligations, auto-enqueue, ALLGREEN, labels, witnesses, failure policy and charged pre-push work.
   Use the following controls. For Buildkite and RBE, include the cost of binding results to the required GitHub subjects. A partial RBE port earns only a subgraph result.

   | Comparator | What it tests |
   |---|---|
   | GitHub-hosted standard and a named larger Linux shape | Whether capacity and faster compute close the gap. Keep the Free-plan congestion result visible on its own. [Runner sizes][hosted] |
   | Tuned ARC on the same workers | What remains after moving hosted gates, keeping idle pods, and giving jobs isolated clones of local state. [ARC configuration][arc] |
   | Buildkite-style persistent agents | Whether warm agents with local disks and parallel command steps can deliver the same checks sooner. Isolate each checkout and witness. [Buildkite parallel agents][buildkite] |
   | Namespace runners | Whether per-job NVMe cache-volume clones deliver comparable warm builds. A mounted store still needs the same content and trust checks. [Namespace cache volumes][namespace] |
   | Bazel with remote build execution (RBE) | Whether distributing hermetic compile and test actions across workers shortens the longest changed build. Include remote queuing, transfer, two independent executions and materialisation. [Bazel remote execution][bazel] |

   **Pass** the attribution check when every timed result names the subject, inventory, resource cap, cache history, offered load and clock. Publish a fixed-resource comparison and, separately, the fastest tuned configuration actually tested for each system with its cost.

3. **Replay real arrivals and validate with live paired events.** Use at least 300 merges spanning peak and quiet periods, and at least 200 matched heads for a headline P90. Pair by event and randomise equivalent host/time blocks. Include failures, cancellations, supersessions, group rebuilds and backlog. Never prewarm from future commits.
   Record label/accepted push → PR verdict, first actionable red, accepted final push → merge including label wait, group-create → green, and requested release → verified publication. Report pre-push verification start separately. A replay alone cannot measure a provider's live dispatch or GitHub queue latency.
   **Pass** the design goal only at ≤90/240 s verdict, ≤180/480 s landing, ≤50 charged runner-min/merge and ≤1% paired false failures at goal under the stated sampling rule. Show intervals and per-class results. Treat unavailable or unfinished changes as outcomes. [Architecture §§1, 10.2][arch] [Decision N40][decisions]

4. **Stress the paths most likely to reverse the ranking.** Cross unchanged, small edit, root/manifest edit and cold toolchain with warm/empty local state, one versus multiple occupied nodes, foreground contention, one lost worker and an ejected predecessor. Include A1 and release preparation at production priority.
   **Pass** only if actual two-witness coverage holds, audit P90 is ≤2 h under the design's 95% load scenario, and backlog drains at the offered arrival rate. Show empirical complete-hit rate and the critical path distribution on misses. Disclose any class requiring wide execution. [Architecture §§6.5, 9, 10.3][arch]

5. **Predeclare the publication rule.** For the primary push-to-merge clock, compute paired, arrival-block bootstrap intervals for the differences of the systems' P50s and P90s, preserving bursts and shared queue history. Adjust simultaneous 95% intervals for the named comparisons.
   **Pass** a narrow “fastest measured” claim when both intervals lie strictly below zero against *every named, confidence-qualified comparator* at the declared budget. Publish runner-only results separately. A tie, an unrun comparator or a win confined to the light class limits the claim accordingly.

[arch]: ../../pr-worktree/docs/proposed/velnor-runner-architecture.md
[evidence]: ../../pr-worktree/docs/research/velnor-runner-2026-11/evidence.md
[decisions]: ../../pr-worktree/docs/research/velnor-runner-2026-11/decisions.md
[plan]: ../../pr-worktree/docs/research/velnor-runner-2026-11/delivery-plan.md
[owner]: ../../pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md
[rulings]: ../../pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md
[classes]: ../../pr-worktree/docs/research/velnor-runner-2026-11/proposals/fable-5.2.md
[floor]: ../../pr-worktree/docs/research/velnor-runner-2026-11/design-feedback/gpt-6.2-astra.md
[cluster]: ../../pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-cluster.md
[mac]: ../../pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-mac.md
[rustfs]: ../../pr-worktree/docs/research/velnor-runner-2026-11/experiments/rustfs.md
[blaze]: ../../pr-worktree/docs/research/velnor-runner-2026-11/experiments/blaze.md
[git-test]: ../../pr-worktree/crates/velnor-actions-orchestrator/tests/workflow_configuration_public/non_inputs_differential.rs
[grammar]: ../../pr-worktree/crates/velnor-actions-contract/src/config/rust_non_inputs.rs
[receipt]: ../../pr-worktree/crates/velnor-runner/crates/velnor-runner-host/src/host_controller/loop/polling/restart_tests/bootstrap/receipt_integrity.rs
[hosted]: https://docs.github.com/en/actions/reference/runners/larger-runners
[arc]: https://docs.github.com/en/actions/how-tos/manage-runners/use-actions-runner-controller/deploy-runner-scale-sets
[buildkite]: https://buildkite.com/docs/pipelines/best-practices/parallel-builds
[namespace]: https://github.com/namespacelabs/nscloud-cache-action
[bazel]: https://bazel.build/remote/rbe

# Review by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

## Proposal A

A is the best starting point for shortening push-to-merge at today's confidence. Move the hosted gates, keep the official executor, and make the expensive checks faster. Its five-minute figure describes a projected PR verdict. It leaves the time from that verdict to merge uncosted.

Source conventions below use A, B and G line numbers from the supplied proposals. T1–T11 and lettered sections refer to the shared evidence pack. Velnor workflow citations use the study revision `b4a1357ceb` so later changes to main do not alter the comparison.

### Weakest assumptions

1. **The listener can send a particular job to the pod with its cache.** A:211, 286 and 384 assume the scheduler controls that assignment while retaining the unmodified official Listener.
   ARC acquires request IDs for a *scale set*, then scales from the total assigned count. Its JIT request specifies a runner name and optional work folder. It carries no job ID. The broker later delivers the assignment.
   Demonstrate two concurrent jobs with different cache requirements landing on the intended workers, including after cancellation. Until then, use pool-level routing or mount a correctly scoped cache after assignment. (`arc/cmd/ghalistener/scaler/scaler.go:149,173`, `arc/controllers/actions.github.com/ephemeralrunner_controller.go:842`, evidence F.2.)

2. **Four hash shards cut the test wall to one quarter.** A:345–370 divides a suite's elapsed time by four even though nextest already ran tests concurrently.
   The project's sharding trial put all six slow tests on one side. For the larger velnor-new run, uploading the test archive alone took 36–43 seconds.
   Compare inline execution with duration-balanced shards on the workers, charging for build, transfer, startup and the longest individual test. Verify identical test identities and feature configurations. [Experiment #585, measurements 7–9](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529)

3. **A content key makes stale compiled output harmless.** A:161 and 298 treat this as guaranteed. Cargo also uses file timestamps. Legacy removed timestamp pinning after it let stale artifacts appear fresh. Signing a generation establishes its producer, while the compiler still depends on the completeness of its input model.
   Compare cold and warm outputs following changes to generated files, features and build-script inputs. Count cache hits separately from the cold CPU time those hits actually save. (`velnor-legacy/crates/velnor-runner/src/stable_workspace.rs:3`, `ci-helper/crates/velnor-ci/src/cli.rs:214`.)

4. **A green PR head tree covers the queue candidate.** A:333 compares the candidate with the PR head. The checked-in workflow uses the default PR checkout, which tests the synthetic merge ref. T7 fetched PR heads later and explicitly warns that they may have moved.
   Record the actual checkout tree, selected checks, configuration and producer of the green result. Replay those receipts against the queue candidates. The 7/45 head-tree matches in T7 do not measure reusable verification. (`.github/workflows/ci.yml:399`, `docs/hard-decisions.md:103`.) [GitHub's PR checkout semantics](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#how-the-merge-branch-affects-your-workflow)

### Factual errors

- A:68 calls maintenance-helpers a pure cold compile and A:314 uses all 486 seconds of the named check as compile time. In run `44305693843`, 54 tests consumed 4 min 13 s of an 8 min 38 s job. Even a free compile leaves more test time in that run than the proposed 60–120 s warm job. [Job analysis for #1305](https://github.com/tailrocks/velnor-new/issues/1305#issuecomment-6980497787)
- A:217 says the action archive cache skips action resolution. The Worker obtains download information first, then looks up the archive using the resolved SHA. Credit the saved download only. (`actions-runner/src/Runner.Worker/ActionManager.cs:492,1232,1285`.)
- A:80–87 attributes the Cargo source misses to consumers hashing their own `ImageOS`. The Rust jobs actually take the sources key from hosted Plan. Their cache paths *do* expand under each runner's temp directory. B identifies this separate cache-version dimension. Compare producer and consumer versions for the same key before changing it. Compiled-state compatibility has different inputs again. (`.github/workflows/ci.yml:386,671–673,705–711`, T9a.)
- A:233–236 describes a generated profile admitting four action families. The generator's own test requires `jdx/mise-action`, outside that list. Measure how many emitted jobs actually qualify before assigning the native lane a time saving. (`generator/crates/velnor-gen/tests/it/job_setup.rs:22`.)
- A:395–401 calls its build pods Guaranteed despite unequal memory requests and limits. The proposed 8 GiB request and 16 GiB limit do not qualify for the exclusive-CPU behavior used in the estimate. [Kubernetes QoS criteria](https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/#guaranteed)
- The displayed median stages at A:365–372 add to **2.6 minutes**. Four to five minutes may be a reasonable allowance, but the table does not say what consumes the remaining 1.4–2.4 minutes. Neither total establishes a p90.

### Missed bottlenecks or risks

- Resetting a Worker process and its checkout leaves other pod state alive. A:384 allows the same pod to return to the pool with its DinD daemon. Pre-create single-use pods and retain only separately owned, scoped caches. This preserves warmth without trusting one job to clean up for the next.
- A's primary metric misses queue backlog, ejections and retries. On November 18, G's merged-PR timeline sample puts the median queue-entry-to-merge interval at 281.7 minutes. Removing a post-merge push run saves capacity. Its effect on PR latency depends on whether the freed capacity serves a waiting check. (G:35–47, T5, T7.)
- A puts cache-aware placement in Phase 2 and delivers the listener in Phase 4. For the intervening phases, report the locality ARC actually achieves. A warm-hit target that depends on later placement cannot validate the earlier rollout. (A:281–289, 537–559.)

### Ideas worth taking

- Give Plan and both required gates reserved cluster capacity. On November 18 the three serial waits consumed a median 57.0 minutes in a 71.8-minute successful PR run. Preserve the independent final verdict and its required check names. (T5, evidence C.1.)
- Retain A's default-branch-only publication of shared cache state, its per-PR writable scope, and its cold fallback. Keep release and private-dependency material out of fork-readable stores. (`docs/goals.md:40–43`, A:291–299.)
- Keep the official Worker as the reference. Adopt the write-intent, ownership and reconciliation rules for external operations. Require a real lab job and matched results before moving even one generated root onto a native executor. (A:225, 241, evidence D.4 and E.7.)

### Verdict

Adopt A's early phases. Its biggest flaw is optimizing PR verdict time as a proxy for push-to-merge. The evidence supports moving the gates and preserving execution semantics. It does not establish A's shard timings, job-directed placement, or a minimum merge time. A can deliver a defensible first improvement without depending on any of those three claims.

## Proposal B

B identifies concrete work to remove, including slow journal setup and unnecessary Docker startup. Its 40–75-second headline depends on narrowing verification in ways that already have a counterexample in the source.

### Weakest assumptions

1. **Two integration binaries cover the local-mode feature.** B:498–504 infers coverage from the absence of negative `cfg` gates. The feature also enables a library module with its own policy unit tests. Filtering to the two named integration binaries drops those tests. Building everything with the feature enabled instead leaves the default configuration untested.
   Compare nextest inventories under both configurations, including library tests, and retain standalone compilation for each promised feature set. (`crates/velnor-runner/crates/velnor-runner-host/src/lib.rs:42`, `src/local/policy_tests.rs:112–145` in that crate.)

2. **A signed tree receipt can replace full queue verification.** B:418–425 keys it by tree, workflow and toolchain. PR and queue events currently select different obligations. A receipt for a partial PR plan can have all three matching keys and still lack checks required by the queue.
   Match the tested merge ref, required task inventory, feature sets, resolved tools and relevant event inputs under a trusted verifier. Keep the full path whenever coverage differs. A nightly run would detect a bad skip after merge. (`docs/hard-decisions.md:103–110`, `.github/workflows/ci.yml:590–604`, T7.)

3. **A warm Cargo snapshot restores in 0.5–1.2 seconds and covers over 92% of first pushes.** B:399–408 extrapolates from a lab warm build. A reflink avoids copying file data but still creates directory entries and inodes. Cargo must validate the restored state. A count of unique dependency versions also omits feature, target, profile and compiler differences.
   Race reflinked targets against persistent mbx on matched EPYC jobs. Include a first PR push, a later push, a changed base and a cold node. Record clone time, first cargo, actual rebuilds and final verdict. [Experiment #585](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529)

4. **The native Worker can parallelize lease acquisition and recover through an instant fallback.** B:219–249 moves initial renewal alongside setup and estimates under a minute to switch engines. The official listener explicitly abandons dispatch if the first renewal fails. An engine toggle can route future jobs. Recovering an already acquired attempt requires ownership, cancellation and completion rules.
   Demonstrate one terminal conclusion and no duplicate repository execution across a lost renewal, cancellation and redelivery. (`actions-runner/src/Runner.Listener/JobDispatcher.cs:393–405`, `docs/deferred/self-hosted-runner.md:293`.)

### Factual errors

- B:22, 197 and 511 charge five dead minutes before an unbatched queue build. I checked ruleset `24396608`. Its minimum merge size is **one**, and its build concurrency is **two**. The timeout concerns waiting for a minimum group size after checks pass. It supplies no fixed five-minute saving here. [GitHub merge-queue settings](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
- B:176–185 labels 14.7 GB the extracted size of a 1.92 GB cache. The source measured the job's **total disk growth**. Its zero-crate, 0.3-second plain-Cargo result came from the lab. The measured warm velnor-new run used mbx, compiled 46 crates and took 345 seconds. The report explicitly leaves plain Cargo at velnor-new scale unverified. [Original comparison, measurements 2 and 7](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529)
- B:27–29 presents sums of event medians as push-to-merge wall time. Its quiet and busy estimates of 33.8 and 141.2 minutes exclude backlog and retries. G reports 62.1 and 402.3 minutes for PRs actually merged on those two days. The cohorts differ, and only the timeline measures the requested interval. Likewise B's 179.5 runner-minutes buys one median run of each kind. T7 records multiple PR runs and repeated queue entries.
- B:797–799 says `inputs.tsv` digests verify the snapshot. That file records tracked **source** paths, content identities, modification times and modes. It contains no digest of a restored test executable or compiler output. Verification of the snapshot payload needs separate evidence. (`ci-helper/crates/velnor-ci/src/inputs.rs:138–151`.)

### Missed bottlenecks or risks

- B:465–471 budgets 3.5–5.5 seconds for planning and static checks together. The inputs audit requires current all-targets build inventories across Cargo roots and can reject an incomplete build. B also folds eight jobs into an initial stage although today's two required checks depend on final reports. Time the build, audit and downstream verdict explicitly. (`ci-helper/crates/velnor-ci/src/cli.rs:201–215`, `.github/workflows/ci.yml:580,619`.)
- B:487–509 leaves the long `image_publication_public` tests in the nominal full-run path. G reports one test at 323.5 seconds, already longer than B's 65-second full-verdict target. Shortening the qualification timeout tests is a separate change. Their assertions cover lower *and* upper elapsed-time bounds, which a faster fixture must still exercise. (G:102, B:786, `crates/velnor-runner/crates/velnor-runner-host/tests/runner_image_qualification_public.rs:388–393`.)
- B's site-local artifact endpoint has to preserve visibility, finalization and retry behavior for GitHub consumers. Local transfer speed does not settle whether a subsequent hosted job or a rerun can fetch the right report. Legacy had actual rerun and artifact-ownership incidents. Test a mixed hosted/cluster workflow through a retry before putting this service in the gate. (B:409–417, evidence E.6, `.github/workflows/ci.yml:580–585`.)

### Ideas worth taking

- Keep short suites inline. Use historical test durations for longer suites and compare the slowest shard plus archive cost against one-job execution. The measured hash imbalance is a reason to try duration balancing. It establishes no universal 25-second cutoff. [Sharding trial](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529)
- Separate Docker-requiring jobs from ordinary builds, and prewarm both according to demand. This targets the observed DinD preparation cost without changing the checks. Take B's proposal to run unmodified Node actions in the same job environment if a native Worker later earns its place. (Evidence B.6 and E.1, B:239–244, 538–543.)
- Carry forward plain Cargo with immutable, trust-scoped targets as a serious alternative to mbx. Keep per-package feature checks even after workspace consolidation. The lab specimen has two packages that pass workspace Clippy and fail when checked alone. [Experiment #585, measurement 4](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529)

### Verdict

B would cut queue and build time, but its headline does not preserve today's confidence. The biggest flaw is treating superficially duplicate checks as equivalent. The missing local-mode library tests are a concrete loss, and the receipt can also certify work a queue event never received. Adopt the isolated optimizations and require a coverage-preserving replay before using B's timing model to select an architecture.

## Proposal G

G measures the right outcome. Its November 18 sample of 34 merged PRs had a 402.3-minute median from last push to merge, including a separately measured 281.7-minute median from queue entry to merge. That distinction explains why faster Rust builds alone cannot deliver the stated goal. (G:33–46.)

### Weakest assumptions

1. **Enforcing file reads makes cached test passes equivalent to running the tests.** G:219–252 excludes Docker and network tests, but allowed directories, the clock, scheduling, randomness and shared process state can still affect the result. The existing inputs audit sees only part of the build and explicitly excludes runtime reads.
   Define a deterministic test class, bind the contents of all allowed inputs and the execution policy, and run independent cached-versus-fresh comparisons. A 2% audit has only a 2% chance of catching a single bad reuse on that occasion. (`ci-helper/crates/velnor-ci/src/cli.rs:214`, `docs/goals.md:36,41`.)

2. **A restarted agent can fail every lost job without a journal.** Killing a cgroup establishes local process cleanup. The remote completion call requires the plan ID, job ID and an authorized service connection. G:187 does not specify how to recover these or distinguish a lost completion response from an uncompleted attempt.
   Exercise restarts after acquire, after execution and after sending completion. Prove that the next agent can reconcile status before admitting another attempt. A small durable lease record could suffice. (`actions-runner/src/Runner.Worker/JobRunner.cs:64,332`, evidence D.4.5 and E.6.)

3. **A merge group can finish in 30–60 seconds by consuming the PR's work.** G:231–236 assigns PR writes to private overlays, then G:277 assumes the queue hits those verdicts. The trust policy supplies no independently verified promotion path. Letting a merge group write shared state introduces another issue: a candidate can fail or be rebuilt and never reach main.
   Trace one real PR through a changed base, then a rejected queue candidate. Show which trusted objects remain reusable and when a successful result becomes eligible for publication. (T7, G:46, 278.)

4. **The projected 85-second verdict survives the workload mix.** G:148 measures a warm generator build on a loaded M5. The 1.07× concurrent-build comparison measures total CPU consumption. G:287–292 needs elapsed EPYC times, including slow tests, feature builds and static checks. File-path classes alone do not identify each PR's selected verification.
   Replay the sampled changes through the conservative planner and measure the class distributions on workers at quiet and peak load. Include mixed PRs, cancelled work and queue re-entry. Report time for PRs still waiting at the observation cutoff separately.

### Factual errors

- G:45 turns `max_entries_to_build=2` into roughly two merges per group run. It limits concurrent build requests. The ruleset separately permits 1–10 PRs to merge. Two builds can overlap, and a failed earlier entry can invalidate later work. The quoted 1.9 merges/hour is a model, not a measured service rate. [GitHub merge-queue settings](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
- G:121 says generator and ci-helper kill *any* test after two seconds. The study revision has 10-second generator overrides and 30-second periods with four periods allowed for particular ci-helper tests. Several mirror tests also require exclusive scheduling. This weakens the extrapolation from 55 seconds on one thread to eight seconds on eight. (`generator/.config/nextest.toml:25–29`, `ci-helper/.config/nextest.toml:19–31`.)
- G:47, 363 defines 10/96 failed same-SHA pushes as the flake rate. It measures disagreement between two event runs. I confirmed failures in runner-host tests on push `44207443352` and in maintenance-helpers on `43839742761`. Those records do not classify the cause. G's Phase 0 then removes the very push reruns used as the denominator for its ≤5% target. Keep a randomized paired rerun sample and classify the failing step.

### Missed bottlenecks or risks

- G's count of 12 jobs for PR #1312 needs a denominator. I checked run `44308933193`. Ten Rust jobs and two named checks succeeded, plus eight hosted jobs, for **20 successes and 19 skips**. Both runner-host variants ran for over six minutes. The example supports G's selection diagnosis, and the other eight jobs still count toward time and capacity. (G:8, 248.)
- G:183 reserves the control-plane node for gates on the premise that no PR code runs there. Both Plan and Required verdict check out the candidate, and the workflow itself can change. A released helper alone cannot guarantee the provenance of every executable step. Reserve worker capacity for gates until the full gate definition is independently authorized. (`.github/workflows/ci.yml:399,565`, evidence B.1.1.)
- G:182 treats the capacity header as free slots and assumes ordered assignment. ARC passes its configured maximum to the listener and separately scales on assigned jobs. The distinction matters with occupied slots and multiple organizations. Validate how many assignments GitHub actually issues under partial occupancy before depending on this for admission. (`arc/cmd/ghalistener/main.go:99–102`, `arc/cmd/ghalistener/scaler/scaler.go:339`.)
- G:222 keys artifacts by run ID and name and uploads to GitHub in the background. The workflow includes the attempt in artifact names and fetches expected reports before declaring success. Preserve attempt attribution and wait for durable publication whenever a downstream GitHub consumer depends on it. (`.github/workflows/ci.yml:506–513,580–585,606–612`, evidence E.6.)
- G:181 promises a clean view for each job, while G:393 keeps one Docker daemon alive per slot. Reusing immutable image layers is compatible with that promise. Persistent containers, volumes and daemon state need their own per-job boundary and cleanup proof. Include that cost in the warm-slot measurement.

### Ideas worth taking

- Instrument push, first failing check, queue entry, each candidate and merge. G's Alint example is independently reproducible: run `44357962729` exposed the lint failure at 17:25:32 UTC and completed `Required` at 17:50:15. Earlier actionable feedback would have saved nearly 25 minutes even with the same final gate.
- Profile individual tests before adding shards. G found CPU time in schema construction and a single runner-host test taking 534 seconds under load. Compare fixture reuse and schema-validation improvements with identical assertions. Preserve the project profile unless a profile change is separately accepted and measured. (G:98–103, evidence D.10, PRs #1333 and #1349.)
- Use G's comparison of on-demand and already-idle runners as the baseline for an executor rewrite. The reported startup gap is 14 versus 3 seconds. Measure what remains after a warm pool, baked actions and local tools are available to the official Worker. (G:94–96, `actions-runner/src/Runner.Worker/ActionManager.cs:1232`.)

### Verdict

G supplies the best diagnosis of the queue and the strongest case for speeding up the tests themselves. Its three-minute push-to-merge target depends on counting old test passes as current verification. The input and trust guarantees needed for that shortcut remain unproved. Adopt its measurements and test work now. Keep verdict reuse experimental until the eligible test class and every promotion across trust boundaries have demonstrated equivalence.

## Cross-cutting

**What the evidence settles.** All three proposals identify hosted gate waits, cold builds and repeated work on main. The November 18 snapshot had 86 hosted jobs queued and zero on scale-set labels. The scale-set median PR run over the broader sample was 26.7 minutes, with a 92.3-minute p90. Gate placement is the first change justified across these regimes. Exact-SHA push reuse can release capacity once the trusted queue result covers every check that push would run. Retain a full path for unmatched pushes and the push-only seal or release duties. (T2, T5, T7, T11.)

All three also keep blaze outside the required gate. The project's own experiment found that nextest still used Cargo to build the test executables. Keep the pinned compiler, Clippy, docs and nextest as the reference workload in every comparison. [Velnor blaze experiment #652](https://github.com/tailrocks/velnor-new/issues/652#issuecomment-6874232403)

**Which design choices survive comparison**

| Decision | Judgment |
|---|---|
| Executor and placement | Start with A's official Worker and single-use warm pods. Use G's listener-to-Worker experiment to test job-directed placement. Compare against equally warm ARC pools. B's general native Worker needs an additional, measured advantage after that comparison. All three identify the official-runner constraint in `docs/goals.md:16`. |
| Compiled state | A and G choose mbx. B chooses plain Cargo plus reflinked targets. The lab favors plain Cargo for an unchanged tree. G's different-checkout experiment supports persistent mbx. Neither establishes a winner for first pushes, changed bases and busy EPYC workers. Run both against the same revisions and disk budget. [Primary cache measurements](https://github.com/tailrocks/velnor-new/issues/585#issuecomment-6856187529) |
| Skips and sharding | Use A's default of executing selected tests, B's feature-isolated builds and duration-balanced sharding, and G's measurement of the longest single test. Preserve a full queue check wherever input ownership, tested tree or feature coverage is uncertain. Compare executed test IDs and configurations, since equal counts can mask an omitted test. |
| Cluster tuning | Reserve capacity for gates and separate Docker workloads first. The evidence shows intermittent CPU saturation and memory headroom. It does not establish whether THP, RAID striping, extra workers or faster cores improve merge latency. A loaded worker experiment with cold, warm and concurrent jobs can distinguish them. (Evidence B.1.4, B.1.6, B.2.) |

**Revised hypothesis judgments**

- **H1 fails as the explanation for a dramatic speedup.** B's job trace and G's step breakdown put most recoverable action time in downloads, tool setup and artifact transfer. The official Worker can benefit from locality too. (B:85–105, G:94–96, T10.)
- **H2 remains unproven, including for the restricted executor.** Legacy demonstrates implementation feasibility and real compatibility costs. No proposal measures a native win against the same official Worker with equally warm inputs, CPU and scheduling. (Evidence E.6–E.7.)
- **H3 partly holds.** Keep Kubernetes for fleet management. G's warm-versus-cold pod comparison weakens the claim that removing Kubernetes from every dispatch is necessary. The residual cost and the value of late placement are measurable separately.
- **H4 holds for local reuse.** The proposals change my judgment about *what* to cache: the choice between compiler objects and a validated Cargo target remains open. Credit elapsed time saved on the critical path alongside hit rate. (T9, G:148, experiment #585.)
- **H5 holds for proven selection.** PR #1312 shows the cost of conservative selection. The feature-only library tests show the cost of relaxing it incorrectly. Today's evidence supports removing demonstrated duplication. Broader tree and test-verdict reuse needs a complete account of the work it replaces.

**Decision.** Take A's compatibility boundary, B's cache comparison and G's push-to-merge instrumentation. Judge each change by last-push-to-merge p50 and p90 under comparable arrivals, alongside required-check coverage, failed attempts and runner-minutes per merged PR. None of the three proposals has yet demonstrated a minimum at equal confidence.

---

## Key (added by the manager after the review)

- Proposal A: Muse Wonder
- Proposal B: Gemini 4 Carbon
- Proposal G: Claude Fable 5.2

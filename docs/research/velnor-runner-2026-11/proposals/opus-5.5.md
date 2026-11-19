# Proposal by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-18, 47 min |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

- **The metric that matters is bad, and mostly waiting.** Over the 98 most recent merged PRs (merged 11-16 to 11-18), the time from last push to merge has a median of 85 min (p75 339, p90 457). On 11-18 the median was 400 min. Most of that is queueing. The org is on GitHub Free, which allows 20 concurrent hosted jobs, and four tiny gate jobs wait for those slots. The merge queue builds only 2 entries at a time, while each merge-group run took 64–82 min on 11-18. Agents add a median of 7.5 min between a green PR and enqueueing, and none of the 100 PRs used auto-merge.
- **The work itself is about 10 min per run, and code problems set it.** The `maintenance-helpers` check compiles with `build.jobs=2` and runs serialized golden tests; it is the last job in 102 of 154 runs. The runner-host test step lasts exactly as long as one test that loops over 6 cases: 124 s when I ran it alone on an M5 Max, 203–271 s in CI, spent in turso SQL parsing. The actions-cli test step is one test that loops over 40 cases (258–274 s). No runner fixes these.
- **Verdicts on the owner's hypotheses.** H5 (change-aware workflows) holds and is the largest lever. H4 (local caches) holds. H3 (Kubernetes) holds only partly: it is a good substrate, but it is not where the speed comes from. H2 (from-scratch runner) holds partly: rewrite the dispatcher and sandbox layer; leave the step executor for later. H1 (Rust-native actions) fails as stated: JavaScript action steps take a median of 17 s per job, mostly waiting on the network, so locality matters and the implementation language does not.
- **The design.** Keep GitHub's protocol and workflow YAML. Replace ARC with a Rust `velnor-dispatch` and a `velnor-node` agent that own:
  - warm sandboxes, placed using cache affinity and trust class;
  - node-local caches with read-only trusted stores: tools, crate sources, a git mirror, and an mbx object store, plus one mbx server per site;
  - per-job read tracing.

  GitHub's official `Runner.Worker` stays as the executor until a native executor shows a measured gain.
- **No more duplicate verification.** The merge queue should reuse a job's PR verdict whenever the job's exact input set is unchanged; I estimate this removes 88% of merge-group package-jobs. Stop re-running CI on push to main: 120 of 159 such pushes carry the same SHA the merge queue already tested. Turn on auto-merge and set `max_entries_to_build` to 6.
- **Targets.** Push-to-merge median of 10 min or less (from 85) and p90 of 20 min or less. PR verdict in about 1–3.5 min. Runner-minutes per merge down by about 80–88%. Five phases, each with a numeric gate, over about 8–10 weeks.
- **Owner questions.** `docs/goals.md:16` (jobs run only on the official runner), hard decision 3 (merge queue and push always run in full), `docs/goals.md:45` (free limits), and the agents' merge procedure.

Provenance tags: **[M]** I measured it in this session (method given); **[EP]** taken from the evidence pack; **[Est]** an estimate with its arithmetic shown; **[Doc]** vendor documentation. My scripts are in `~/.cache/ci-ambition/panel/opus-5.5/work/` (`p2m.py`, `critpath.py`, `jobsteps.py`, `closure.py`, `reuse.py`, `bench/*.sh`). They reuse the raw job JSON and logs in `~/.cache/ci-ambition/evidence/ci-raw/` plus about 60 new read-only API calls.

## 1. Where does the time go today?

### 1.1 Push to merge, measured end to end

**[M]** Method:
- I took the 100 most recently merged PRs from a GraphQL read with timeline events (added to merge queue, removed from merge queue, merged) and joined them with the 3,009-run CI list.
- "Push" is the creation time of the first PR run on the PR's final head SHA.
- 98 PRs had complete data. Script: `work/p2m.py`.

| Segment (minutes) | median | p25 | p75 | p90 |
|---|---|---|---|---|
| Last push → PR CI finished (success) | 30.4 | 16.0 | 71.1 | 99.3 |
| PR green → added to merge queue | 7.5 | 1.8 | 32.0 | 87.9 |
| Added to queue → merged | 26.5 | 14.8 | 100.4 | 368.2 |
| … of which the merge-group run itself | 24.4 | 13.6 | 45.5 | 75.8 |
| **Last push → merged** | **85.1** | 46.7 | 339.0 | 456.6 |

- **By merge day, last push → merged (median):** 11-16: 50.6 min (n=24). 11-17: 56.7 min (n=42). 11-18: 399.5 min (n=32), with enqueue → merge at 278.4 min. **[M]**
- **Merge-queue capacity is the 11-18 failure.** The ruleset sets `max_entries_to_build: 2`, `grouping_strategy: HEADGREEN` and `min_entries_to_merge: 1` (`gh api repos/tailrocks/velnor-new/rulesets/24396608`) **[M]**. With merge-group runs of 64–82 min on 11-18 **[EP T5]**, the queue merges about 2 PRs every 70 min, roughly 1.7 per hour **[Est]**. Agents enqueued faster than that, so a backlog formed.
- **GitHub's fixed latencies are small.** Enqueue → merge-group run created has a median of 19 s; merge-group run finished → merged has a median of 26 s **[M]**. The floor for the queue part is therefore about 45 s plus the merge-group run.
- **Agent process:**
  - 0 of 100 PRs enabled auto-merge **[M]**.
  - Median of 3 PR runs per merged PR **[M]**.
  - 7 of 15 sampled PRs contained commits that merged `main` into the branch, 17 such commits in total **[M]**. Each one triggers a full PR CI run, although the merge queue already tests the merged result.
  - Queue removals: 9 for failed checks, 7 of them followed by a new commit, so the failures were mostly real; 8 for merge conflicts **[M]**.

### 1.2 One CI run: the critical path

**[M]** I recomputed this from the evidence pack's raw job JSON: 112 successful runs in the scale-set era. Chain: (Plan ‖ Ledger CLI) → fan-out → Required verdict → Required. Script: `work/critpath.py`. All values are medians in minutes.

| PR runs | n | wall | Plan/Ledger incl. queue (queue) | fan-out on cluster | verdict queue + run | Required queue |
|---|---|---|---|---|---|---|
| 11-16 | 23 | 23.2 | 7.8 (6.9) | 8.1 | 1.9 + 0.6 | 2.1 |
| 11-17 | 13 | 16.0 | 5.6 (4.7) | 8.5 | 0.7 + 0.6 | 1.2 |
| 11-18 | 13 | 71.8 | 27.4 (26.3) | 9.3 | 23.3 + 0.5 | 15.3 |
| all | 50 | 27.7 (p90 79.1) | 10.3 (9.5) | 8.5 | 2.5 + 0.6 | 2.5 |

- **Merge-group runs:** wall 16.0 min, fan-out 8.3 min. **Push runs:** wall 19.4 min, fan-out 8.0 min. **[M]**
- **Execution on the path is about 10 min:** Plan about 0.9, fan-out 8.5, verdict 0.7. Everything else is queueing on hosted runners: 41% of the PR wall on 11-17 and about 80–90% on 11-18. The evidence pack puts it at 79% counting only Plan's queue; Ledger CLI often queued longer.
- **Why the hosted queue exists:**
  - `gh api orgs/tailrocks` reports `plan: free` **[M]**. GitHub Free allows 20 concurrent standard hosted jobs per account [Doc: docs.github.com/en/actions/reference/limits].
  - Other workflows compete for those 20 slots. On 11-18, Source qualification had 267 runs at about 24 runner-min each, roughly 6,400 hosted minutes; Job image candidate had 306 runs at about 3.4 min each, roughly 1,040 minutes **[Est from a 10-run jobs sample each, `work/gh/jobs_*`]**.
  - Scale-set jobs queue a median of 0.2 min **[EP T4]**. The cluster was never what made jobs wait to start.

### 1.3 Inside the fan-out

**[M] (`work/jobsteps.py`, 154 runs):** The last scale-set job to finish was `Check / maintenance-helpers` in 102 runs, `velnor-actions-cli` in 24, `runner-host (local-mode)` in 15 and `runner-host` in 13.

**`maintenance-helpers` (median 508 s).** Phase timestamps from job logs 134191252252 and 134894816764 **[M]**:

| Phase | Duration |
|---|---|
| `pin-mirror`: dev build of velnor-actions-cli at `build.jobs=2` | 83–86 s |
| Native inventory: `check --release` at `build.jobs=2` | 44–52 s |
| Diagnostics: two builds at `build.jobs=2` | 88–109 s + 23–37 s |
| velnor-archive-guard nextest | 211–246 s |

- The `-j2` settings are at `mise.toml:90,108,128,137,148,157,170,193`. On 96-thread nodes they are the main cost of this job.
- The nextest phase is serial because 12 golden tests set `threads-required = "num-test-threads"` (`.config/nextest.toml:16-19`).

**`runner-host` (median 330 s).** Setup 39 s, building test executables 81 s, tests 192 s (p90 417), teardown 13 s. Clippy (68 s) and docs (59 s) run in parallel with the tests **[M]**.

The test step lasts as long as its single slowest test, `launch::bootstrap::unbound_retirement::tests::shape::an_initializer_that_was_not_proven_absent_is_not_claimed` (`crates/velnor-runner/crates/velnor-runner-host/src/launch/bootstrap/unbound_retirement/tests/shape.rs:98`):
- In job 134191420609 the step took 270.9 s and that test took 270.8 s. In job 134191420582 the step took 203.5 s and the test 203.4 s **[M, logs]**.
- Run alone on an M5 Max, it took 124.5 s wall and 108 s of user CPU. One iteration of its helper takes 21.8 s, and the test runs six iterations **[M, `bench/slowtest.sh`]**.
- A CPU sample (macOS `sample`) shows the time going to `turso_parser` AST `clone`/`drop`, `parse_expr`, `Connection::prepare` and `translate` **[M]**.
- So the cost is SQL statement preparation inside the embedded journal. That matches open PRs #1333 (validate the schema once) and #1349 (cache historical-schema journals).
- The next slowest tests (75–118 s) are also journal-heavy **[M, logs]**.

**`velnor-actions-cli` (median 393 s).** Setup is 132 s, of which "Prepare image publication fixture" takes 89 s **[M]**. The test step (258–274 s) again lasts as long as one test, `image_publication_public::public_authority_rejections_preserve_registry_inputs_and_foreign_resources` (`crates/velnor-actions-cli/tests/image_publication_public.rs:86`). That test loops over 40 rejection cases one after another, about 6.5 s each. The next four tests take 115–136 s **[M, logs 134191420345 and 134894988058]**.

The pattern is the same in both jobs. nextest runs separate tests in parallel, but the cases inside one `#[test]` run one after another, so the slowest test sets the length of the test step.

### 1.4 Setup, downloads and the runner's own overhead

**[M]** Over 3,591 Rust crate jobs on the scale set (`jobsteps.py`):

| Item | Median per job | Share of crate-job runner time |
|---|---|---|
| JavaScript action steps (checkout, mise, mbx, artifacts, cache) | 17 s | 15.7% |
| The runner's own steps ("Set up job", "Complete job") | 5 s | 4.1% |
| Tool provisioning (`mise install`, rustup components, verification) | 14 s | 13.9% |
| `cargo fetch` (sources cache always misses) | 4 s | 4.2% |

- Across all scale-set jobs, setup is 31.8% and teardown 9.7% of runner time (2,595 and 790 of 8,172 runner-min in the sample) **[M]**.
- **"Set up job"** took 5.3 s in log 134191420609 (lines 14–19), of which 4.4 s were 6 sequential action tarball downloads **[M]**.
- **ARC pod start:** from pod scheduled to last container started, the median is 6 s and the max 16 s over 436 pods in the last hour, gated by the dind startup probe (`kubectl get events`, read-only) **[M]**. The scale-set job queue median is 12 s **[EP]**.
- **Plan job (54 s):** "Select attested state" takes 18 s, a full-history checkout 12 s and tool preparation 8 s, while the plan itself takes 2 s **[M, steps]**.
- **Required verdict:** its step "Download every expected matrix artifact", which fetches the per-job reports, takes 22 s **[M]**.
- **Downloads in every Rust job:** 435–522 crates (sources cache "not found" in 48 of 48 logs) and a fresh Rust install (no restored toolchain in 58 of 58) **[EP]**.

### 1.5 Compilation, cache transfer and duplicate work

- **Compilation is cold in every job.** The first mbx pass shows "0 hits, 404 compilations, 0 B downloaded". Later passes in the same job reuse 40–74% **[EP]**.
- **Cache transfer is zero bytes on the scale set:** no remote cache, and "Restore state snapshot" was skipped in 3,636 of 3,636 jobs **[EP]**. If the GitHub cache were used, it delivers 13.6–22.4 MB/s from pods **[EP]**. The 1,457 MB root state bundle would then take 65–107 s to download **[Est: 1457/22.4, 1457/13.6]**.
- **Duplicate work:**
  - **Three verifications per merge** (PR, merge group, push). 120 of 159 push runs test the identical merge-group SHA **[EP T7]**.
  - **Within one full run,** each job builds its own dependency closure. The closures sum to 2,260 package compilations for 463 unique packages, a factor of 4.9; in the root workspace alone, 893 for 152 (5.9×) **[M, `closure.py` over `cargo metadata` of the 5 workspaces, x86_64 Linux]**.
  - **Three compile passes per job** (test, clippy, doc). `runner-host` and `runner-host (local-mode)` each compile the same 404–407 crates **[EP]**.
  - **Cancellations:** 45% of PR runs are cancelled by newer pushes, and catch-up merges of `main` add about 1.1 full PR runs per PR (§1.1) **[EP, M]**.
- **Cluster contention:** pods request 2 CPU with no limit, and one pod used 84 cores **[EP]**. On busy days job durations grow: runner-host 3.7 → 8.8 min and maintenance-helpers 8.0 → 9.6 min from 11-16 to 11-18 **[EP T6]**. At 17:23Z on 11-18 the workers were at 0–4% CPU **[M, `kubectl top`]**. Load is bursty, not constantly high.

**Approximate breakdown of a 57-minute push-to-merge on a normal day (11-17) [Est, assembled from §1.1–1.4]:**
- Hosted-gate queueing: about 7 min in the PR run and about 2 min in the merge-group run.
- Fan-out execution: about 8.5 min per run, about 17 min in total. Of each 8.5 min, about 4.5 min is `-j2` compilation, about 4 min serialized golden tests, and about 0.7 min setup.
- Agent enqueue delay: about 7.5 min.
- GitHub's merge latencies: about 0.75 min.
- Extra PR CI rounds and retries: the remaining roughly 20 min.

Within those minutes, the largest duplicate share is work a change could not affect: full merge-group runs, unselected crates and the push re-run.

## 2. Which hypotheses hold?

| # | Hypothesis | Verdict | Evidence and expected numbers |
|---|---|---|---|
| H1 | Rust everywhere, including native actions, is dramatically faster | **Fails as stated** | Compilation and tests are already native code (rustc, nextest, test binaries). JavaScript action steps take a median of 17 s per job (15.7%), and they are bound by network round-trips to GitHub (120 ms RTT [EP]), not by interpretation. Node.js startup is about 0.1 s per action [Est], under 1 s per job. Locality saves the time, and it works in any language: a git mirror, preinstalled tools, the official runner's `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (`src/Runner.Worker/ActionManager.cs:1230-1290`) and local artifacts cut setup from 37–45 s to 8 s or less per job [Est: 17+14+4+4 s removable]. A native executor saves only about 2–3 s more per job hop [Est, §3]. |
| H2 | A from-scratch Rust runner beats wrapping the official runner and ARC | **Partly holds** | Overhead per job today: about 12 s ARC queue (6 s pod start [M], the rest control loop and registration) plus 5 s "Set up job", about 17 s in all. A tuned official setup (warm pods, action archive cache) gets to about 3–4 s, and a native runner to about 1 s [Est]. A PR verdict has 4 job hops, so the saving is about 60 s against today and about 10 s against the tuned setup. What a Velnor-owned dispatcher and sandbox enable is placing a job after it is assigned (by cache affinity and trust class), mounting storage per job by trust scope, and tracing which files each job reads; §3 has the details. The legacy native lane was not a measured win: 804k LOC, 2,231 commits, "no performance improvement accepted yet" [EP §E.7], and operations incidents (#389, #408, #670–672). velnor-new explicitly rejected it ("Rejected wholesale: native interpreter", `docs/implemented/macos-scaleset/legacy-disposition.md:65`). GitHub also keeps shipping runner features, such as `background:` steps (`BackgroundStepCoordinator.cs`, in use in `ci.yml`) and forced node24. A native executor must chase each of them. The Worker gets them by updating the bundled runner, which the 63-day rule forces anyway. Build the dispatcher; make the step executor optional and gated. |
| H3 | Kubernetes is the right base and lets us use all hardware in parallel | **Partly holds** | Parallelism was never limited by Kubernetes: scale-set jobs start in a median of 0.2 min [EP]. The waits were on GitHub-hosted slots. Kubernetes defaults work against CI speed: emptyDir only (0 B of cache across jobs), placement that ignores caches, 2-CPU requests with no limit (1.2–2.4× slower jobs on busy days [EP T6]), no kubelet reservations, and dind-gated pod start (6 s [M]). It is a good substrate for DaemonSets, RBAC, NetworkPolicy, RuntimeClass and metrics, as long as Velnor does the placement and owns node-local volumes. |
| H4 | Local caches beat remote; a cache layout that knows the node is a big win | **Holds** | Cross-job and cross-run compile reuse on the scale set is 0% today [EP]. The GitHub cache runs at 13.6–22.4 MB/s, while local NVMe reflink copies or bind mounts take under 1 s. Expected: dependency compile-unit hits of 95% or more on PRs, against 0% today [Est, §4]. Two caveats. If every node is pre-warmed from `main`, node affinity is a second-order gain worth seconds, because only PR-specific objects move. And trust must be enforced: legacy's fork job poisoned `$CARGO_HOME/bin` (`velnor-legacy/crates/velnor-runner/src/trust_scope.rs:1-14`). |
| H5 | Workflows can skip what a change does not touch and run exactly what it needs | **Holds; largest lever after queueing** | PR runs execute a median of 17 of 31 jobs today, because the globs are coarse (`crates/**` → archive-guard and its dependents, `.velnor/config.toml:120-149`). The package graph alone would select a median of 3 packages per PR [M, `reuse.py`]. Merge-group runs execute all 31 jobs, but reusing verdicts on identical inputs would rerun 342 of 2,910 package-jobs across 97 merge-group runs (−88%). The median merge-group run would rerun 2 packages, and 24 of 97 would rerun none [M, package-level approximation]. Push runs repeat the merge group's SHA (120/159). To keep the same confidence, this needs per-job read verification, the precondition named in `docs/hard-decisions.md:199-204` (§5). |

## 3. Runner architecture

### 3.1 Principles

1. **Look like GitHub from the outside.**
   - The scale-set API: sessions, long-poll, `acquirejobs`, `generatejitconfig`.
   - JIT runner identities and broker sessions.
   - Run-service `acquirejob`, `renewjob` and `completejob`.
   - Results twirp for logs and step updates.
   - The workflow YAML and the check names "Required verdict" and "Required".
   - The merge queue, OIDC and `GITHUB_TOKEN` semantics.
   - Reporting a runner version within 63 days of the latest [EP §F.6.1].
2. **Inside, own everything that makes a job slow:** where it runs, what is already on that disk, how fast the sandbox starts, and what it is allowed to write.
3. **Keep a fully compatible executor at every stage.** Every workflow construct, including new ones such as `background:` steps (`src/Runner.Worker/BackgroundStepCoordinator.cs`), keeps working.

### 3.2 Target components

```text
GitHub (unchanged)  ──  scale-set API · broker · run service · Results · checks · merge queue
        ▲  sessions, acquirejobs, JIT identities, renewjob (every 60 s), results
        │
velnor-dispatch  (Rust; Deployment, 2 replicas, k8s Lease leader; replaces ARC listener+controller and Runner.Listener)
  · GitHub App auth; one scale set per (org, trust pool); holds runner identities and broker sessions
  · reads the job message *before* choosing where it runs: trust class (event, head vs base repo; fail closed),
    cache family (root/crate), needs (docker, CPU), PR scope
  · placement score = warm L1 for this family/scope + free CPU − site penalty; waits ≤3 s for a warm node
  · journal (intent → call → record) for acquire/renew/complete, adapted from velnor-new's runner
        │
velnor-node  (Rust; DaemonSet on every worker; privileged infra, never runs job code itself)
  · L1 stores on /var/lib/velnor (dedicated XFS, reflink=1, on the data NVMe): tools, crate sources, git mirror,
    action archives, mbx object store, run-local artifacts
  · warm sandbox pool per class: plain | docker (dind already up) | untrusted; single-use, recreated off the critical path
  · mounts caches by trust scope: trusted stores read-only; per-PR scope read-write; per-job ephemeral upper dir
  · records the job's read set (fanotify on the checkout and cache mounts) → verdict key (§5)
  · trusted builder: on each main advance, builds main into the trusted mbx namespace (the only trusted writer)
        │
executors (inside the sandbox)
  · default: GitHub's official Runner.Worker, fed the acquired job message over its pipe
    (`spawnclient`, `src/Runner.Listener/JobDispatcher.cs:427-502`; it authenticates with the job's own
    SystemVssConnection token, `src/Runner.Worker/JobRunner.cs:64-69`)
  · optional, later: velnor-exec, a native executor for the velnor-gen workflow subset, with the Worker as fallback
per site:  velnor-cache L2 = mbx cache server (filesystem backend on NVMe, GitHub-OIDC write rules) + verdict store
```

**Why this split.** In the scale-set protocol the client cannot bind an acquired job to a particular runner; GitHub gives any acquired job to any idle runner of the set (`actions/scaleset` `generatejitconfig` takes `{name, workFolder}` only) [EP §F.1.3].
- With ARC and the official Listener, a job lands wherever the next pod polls, so placement cannot follow the job's cache family or trust class.
- If `velnor-dispatch` holds the runner identities and sessions, it sees the job first and then chooses the sandbox. That gives cache-affine placement and per-job storage binding without changing anything GitHub sees.
- Keeping `Runner.Worker` keeps every workflow semantic exactly as GitHub implements it.

**Target job lifecycle (Stage B) [Est].**
1. The `JobAvailable` message arrives on the scale-set session, and the dispatcher calls `acquirejobs` (about 0.2–0.4 s at 120 ms RTT).
2. GitHub assigns the job to one of the dispatcher's JIT identities. The dispatcher receives `RunnerJobRequest`, calls `acquirejob`, and now holds the full job message (about 0.3 s).
3. It classifies the job (trust class, family, needs) and picks a node, which takes milliseconds. The node agent hands over a warm sandbox of the right class and bind-mounts the stores for the job's trust scope (about 0.2 s).
4. The Worker starts with the job message on its pipe (about 1 s for .NET startup). The dispatcher renews the job lock every 60 s, as the Listener does (`JobDispatcher.cs:734`).
5. Steps run. Checkout comes from the node mirror, tools are read-only mounts, and cargo uses the mbx L1 store.
6. The Worker completes the job against the run service. The node agent stores the read set and the verdict key, then destroys the sandbox.

**Failure handling.**
- The dispatcher writes "intent → call → record" to its journal around acquire, renew and complete, the discipline velnor-new's runner already uses (`docs/proposed/macos-scaleset-runner.md` §4).
- A restarted dispatcher re-opens its sessions and resumes lock renewal for running Workers before their `lockedUntil` expires.
- A lost Worker leaves the job to GitHub's lock expiry. The job is never re-run silently, so the legacy rule holds that "`already acquired` and `unavailable` MUST NOT be handled by rerunning repository code" (`docs/deferred/self-hosted-runner.md` §6).

### 3.3 Stages (each one removes measured time)

| Stage | What changes | What stays | Job start overhead |
|---|---|---|---|
| A | ARC stays. Add the `velnor-node` DaemonSet and pod-template mounts (read-only tools, sources, git mirror, action archive cache; mbx through a node-local server). Warm pods per class via `minRunners`. Gate jobs move to the cluster. | Official Listener and Worker | about 17 s → 6–8 s [Est: 6 s pod start removed by idle pods; 4.4 s downloads removed] |
| B | `velnor-dispatch` replaces ARC and the Listener; jobs are placed after assignment; mounts follow the trust scope | Official Worker | → about 3 s (Worker spawn about 1 s [Est], mounts and sandbox under 1 s) |
| C (optional, owner decision) | `velnor-exec` runs velnor-gen jobs natively; anything else goes to the Worker | Worker as fallback | → about 1 s [Est] |

### 3.4 What to take and what not to copy

| Source | Take | Do not copy |
|---|---|---|
| velnor-new runner | Scale-set wire types with no HTTP dependency (`velnor-runner-github`, 6.9k LOC). The journal rule "persist intent, call, record". Ownership and absence proofs mapped to pod UIDs (PR #1324 §2.4). The job-authority policy and job-start hook (trust fencing). The resource probe and the qualification suite approach. | Docker-only launch machinery and macOS LaunchAgent/Keychain specifics on the cluster path. `max_jobs` = 1 per host. The 157k-LOC `velnor-runner-host` monolith (split it, §5). PR #1324's one `ReadWriteOncePod` cache volume per (node, family): it serializes jobs, while mbx is built for concurrent sharing (permits and deduplication) [Doc]. |
| velnor-legacy | Reference code, adapted with tests: protocol client for broker, run service and Results (`protocol.rs`); expression engine transcribed from actions/runner (`src/expression/`, 3.5k LOC); per-job trust class from the job message (`trust_class.rs`); mbx store per (trust scope, repo) (`mbx_store.rs:1-12`); git mirror with a protected `refs/velnor` namespace (`git_mirror.rs`); native checkout and cache adapters for velnor-exec | The 37k-line `executor.rs` and `runner.rs`. The slot/supervisor fleet (#389 inert slots, #408 idle CPU at 600–700%). Firecracker (untrusted work goes to hosted runners instead). Node-image sidecars for JavaScript actions. The Velnor-hosted GitHub cache emulation (`gha_cache.rs`, 17k LOC), unless a workflow outside velnor-gen needs it. |
| actions/runner | Protocol ground truth; `Runner.Worker` as the executor; `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` and symlinked actions; job hooks | One Listener process and pod per job; downloading actions per job; self-update in sandboxes (the dispatcher upgrades the bundled Worker when it receives `RunnerRefresh`) |
| ARC | Acknowledge a message only after the scale decision; demand = `totalAssignedJobs`; `X-ScaleSetMaxCapacity`; JIT config never in env or argv | Cold pod per job with emptyDir state; placement blind to caches; `minRunners` as the only warm pool; dind sidecar in every pod; replica patching through the controller (`scaler.go:339`); 10 s service deadline with no client retry |

**Conflict to flag.** Stage A complies with `docs/goals.md:16`. Stage B keeps GitHub's `Runner.Worker` but replaces `Runner.Listener`. Stage C runs some jobs without the official runner. Both B and C are owner questions.

## 4. The cache system

| Cache | Content | Where | Only writer | Readers | Key | Expected hit |
|---|---|---|---|---|---|---|
| Tools | rustup 1.99.1, clippy and rustfmt, mise tools (nextest, mbx, jactionlint, shellcheck, bun, tofu, crane) | L1 read-only `/var/lib/velnor/tools/<digest>`, plus the runner image | `velnor-node`, from checksums in `mise.lock` and `rust-toolchain.toml` | all jobs | digest of the lock files | about 100% after the first job per node and version |
| Action archives | tarballs of the pinned action SHAs (`.velnor/version-policy.toml`) | L1 read-only | `velnor-node` | Worker, through `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` | `owner_repo/<sha>` | 100% (saves 4.4 s per job [M]) |
| Crate sources | registry index, cache and git db | L1 read-only, fed by the existing `crates-mirror` DaemonSet | `velnor-node`, when a lock file changes | all jobs (`--offline`) | `Cargo.lock` digests | 99% or more |
| Git objects | bare mirror per repo | L1 | `velnor-node`, fetching on each push event | checkout via `git clone --shared` or a worktree (under 1 s, against 3–12 s [M]) | commit SHA | about 100% (fetches only the delta) |
| Compiled units | mbx content-addressed objects and prediction manifests | L1: one shared store per node per trust scope. L2: mbx server per site. | Trusted scope: the `velnor-node` builder on each `main` advance. PR scope `pr/<n>`: that PR's own jobs. | Each job reads its own scope plus trusted | mbx portable keys | Dependencies about 100%; workspace crates miss only when they changed or a dependency changed |
| Test images | images that docker tests pull | node registry pull-through mirror | the mirror | dind (`--registry-mirror`) | digest | about 100%; also avoids Docker Hub rate limits |
| Verdicts | the result of a job, keyed by its inputs | L2 verdict store, linked to the GitHub run | `velnor-dispatch`/`velnor-node` after the job ends; never the job | the plan (velnor-ci) | hash of (contents of every file the job read, toolchain, tools, image digest, command, environment) | merge group: about 88% of package-jobs; push: 100% [Est, §5] |
| Run-local artifacts | plan, reports, nextest archives | L1, falling back to L2 | jobs of the run | jobs of the same run | run id + attempt | not applicable (removes the 22 s report download [M]) |

**Placement on warm nodes.**
- `velnor-node` reports what it holds: the trusted `main` SHA it is warm at, per cache family, and the PR scopes it has.
- `velnor-dispatch` prefers a node warm for the job's family and holding the PR's scope. It waits at most 3 s for one, then takes the least-loaded node in the same site.
- Because the trusted builder pre-warms every node on each merge, a node chosen without affinity costs only the L2 transfer of the PR's own objects, a few hundred MB at most.
  - That transfer takes about 3–5 s at 1 Gbit/s [Est: 300 MB / 110 MB/s]. The NIC speed is unmeasured.
  - Measure it with `iperf3` in Phase 0.

**Correctness and trust.** The rules follow `docs/goals.md:42-43`: a broken cache is slow, never wrong, and a PR writes only its own scope.
1. Content addressing: mbx verifies each object's digest before using it [Doc: mr-boxington.jdx.dev/how-it-works].
2. Only the node builder writes the trusted namespace, building `main` commits. The L2 accepts trusted writes from two credentials only:
   - the builder's own Kubernetes service-account token, which no sandbox ever receives;
   - a GitHub OIDC token with `event_name=push` and `ref=refs/heads/main`, if a push job ever writes. This is the mbx server's rule model [Doc: /cache-server].
3. Sandboxes get tools, sources, action archives and the trusted mbx store as read-only bind mounts. This prevents the legacy failure, where a job wrote into a store that a later job put on its `PATH` (`trust_scope.rs:1-14`).
4. PR scope: read-write only for that PR's jobs, which delivers goals rule 8 (`docs/goals.md:72`). Merge-group runs read their PR's scope and write nothing durable. Scopes are dropped 7 days after the PR closes.
5. Forks and unknown trust classes:
   - no writes, no secrets, no OIDC;
   - they run on GitHub-hosted runners: the repository is public, so standard runners are free;
   - on the cluster they are refused before any step runs (velnor-new's job-start hook pattern).
6. "Never wrong":
   - a nightly canary builds and tests with caches disabled and compares the results;
   - `MBX_VERIFY` runs on a 1% sample of compilations;
   - a run-level `VELNOR_CACHE=off` switch exists;
   - the key schema has a version.
7. Garbage collection: LRU within a disk budget per store, for example 1 TB of mbx on the 3.5 TB nodes and 10 TB on velnor-240 and velnor-105. Content-addressed keys mean no "tip of main" race. That race made 7 of 16 state-producer runs fail [EP T9d].

**Sizing [Est].**
- One cold runner-host test build makes a 3.7 GB target directory with full debuginfo [M]. One job's mbx store held 7.0 GiB [EP baseline].
- L1 on each node must therefore hold the trusted state of a few recent `main` commits across five roots, plus the active PR scopes:
  - roughly 200–500 GB, which fits easily on the 3.5 TB data disks;
  - L2 per site holds about 1–2 TB.
- None of this counts against GitHub's 10 GB cache.

**Expected hit rates [Est].**
- Tools, actions and sources: at least 99%, missing only on version changes.
- Compiled units: in a runner-workspace PR, 2–3 of about 282 units in the runner-host test build change (runner-host and its dependents), so more than 99% of units hit. The misses still cost about 30 s of compilation (§5.7).
- Today the first pass hits 0% across jobs [EP].

## 5. Workflow design for speed

### 5.1 Selection

Move velnor-new to velnor-gen (#1290); its `velnor-ci inputs` audit already reads cargo dep-info (`ci-helper/crates/velnor-ci/src/cli.rs:200-216`).
- Select by package graph plus measured reads, not by coarse globs.
- Expected: 3 packages for the median PR, 6 at p75 and all 30 at p90, when infra files change [M, `reuse.py`]. Today's median is 17 of 31 jobs [M].
- Today's rule maps any change under `crates/**` to archive-guard and its dependents (`.velnor/config.toml:120-149`), and maps the whole runner workspace to `velnor-actions-cli` (`.velnor/config.toml:42-50`).

### 5.2 Per-job read verification

Hard decision 3 keeps full runs on push and merge group as the backstop, "until in-job read verification exists" (`docs/hard-decisions.md:199-204`).
- `velnor-node` provides that check: fanotify on the sandbox's checkout and cache mounts records every file opened by any process of the job, whether a compile, a build script, a test or a spawned tool. Each open is attributed to the job through the process's cgroup, because the read-only cache mounts are shared between jobs.
- The verdict key hashes the contents of exactly that read set, plus the toolchain, tool digests, image digest, command and environment.
- A job that reads the network or an undeclared external source is marked non-reusable (always runs). The docker tests and the image-publication fixture are examples.

### 5.3 No double runs

- **Merge group:** run only the jobs whose verdict key has never passed. My estimate is 342 of 2,910 package-jobs (−88%); the median merge-group run needs 2 packages and 24 of 97 need none [M, `reuse.py`].
  - Caveat: this is a package-level approximation. Root files conservatively select everything.
  - Runtime cross-crate reads are missed, so the tracer's real numbers will be higher. Phase 3 measures them.
- **Push to main:** skip CI when the same SHA, or the same tree, passed the merge-group run (120 of 159 [EP]). The node builder's post-merge warm-up writes the trusted cache, so it replaces both the push run and the state seal workflow.
- **Merge queue settings:**
  - `max_entries_to_build` 2 → 6: cluster capacity is ample, and 11-18 showed that 2 cannot keep up.
  - Agents enable auto-merge when they open a PR, which removes the 7.5 min median, 88 min p90, green-to-enqueue delay [M].
  - Drop the "branch must be 0 behind main" step from the agents' procedure (in `HANDOFF.md` and the merge procedure of the agent sessions). The queue tests the merged tree, so catch-up merges only buy another full PR run.
- **Cancelled runs:** keep `cancel-in-progress` for PRs (`.github/workflows/ci.yml:57-59`). With warm caches a cancelled run has still filled the PR scope with compiled objects, so its work is not wasted.

### 5.4 Job shape

- One job per Cargo root (velnor-gen), with lanes: build once, then clippy, docs and tests in parallel.
- A job that requires Docker runs in a docker-class sandbox; all others run without dind.
- Merge the hosted lint jobs (deny, machete, alint, jactionlint) into the plan job's sandbox. They take 0.2–0.4 min of work each [EP].
- Keep two required check names only because the ruleset demands them.
  - Running "Required verdict" and "Required" as one job would save one hop, about 3 s once Stage B lands.
  - That requires an owner-approved ruleset change.

### 5.5 Fix the critical-path code

| Fix | Effect |
|---|---|
| `maintenance-helpers`: use `CARGO_BUILD_JOBS` = the sandbox's CPUs | Its compile phases take about 4.5 min at `-j2` and 96 threads are idle [M]. Expect about 1 min with a warm cache. |
| Parallelize or isolate the 12 golden tests (per-test temp roots instead of `threads-required = num-test-threads`) | Removes 211–246 s of serial tests [M]. |
| Fix turso statement preparation in the journal (#1333/#1349) and stop opening a fresh journal per test iteration | The runner-host test step goes from 203–271 s to the next slowest test. |
| Split looped tests into one test per case: the 40-case actions-cli test, the 6-case runner-host test, and the 115–136 s `family_publication` tests | nextest can then run the cases in parallel, so a test step lasts about as long as one case, about 7 s, instead of 258–274 s [Est: 274 s / 40 cases]. |
| Split `velnor-runner-host` (157k LOC, 85% of its workspace [EP]) into 6–10 crates | Rebuilding the crate alone costs 110–150 CPU-s [M]; a change then recompiles one slice. |

### 5.6 Test sharding

- Shard a suite only when its total test CPU divided by the sandbox's cores exceeds its longest test. After the fixes above, no velnor-new suite qualifies [Est].
- When one does, build a nextest archive once and run `--partition count:i/N` in docker or plain sandboxes on the same site. Today `default_shards = 1` (`.velnor/config.toml:29-30`).
- Give test lanes enough cores instead of oversubscribing: `--test-threads` = the cores allotted to the sandbox.

### 5.7 Minimum honest time to verdict

**[Est, after Phase 3]** 43% of merged PRs touch the runner workspace; 41% generator, 27% ci-helper [M, area count over 100 merged PRs].

| Step | Runner-workspace PR | generator / ci-helper PR |
|---|---|---|
| GitHub creates the run and the plan job | about 3 s | about 3 s |
| Plan: start, mirror checkout, `velnor-ci plan` (2 s today [M]), outputs | 5–15 s | 5–15 s |
| Root job: start and setup with read-only mounts | 3–8 s | 3–8 s |
| Compile the changed crates with warm dependencies | 30–40 s. Upper bound from CI log 134191420609: the turso + runner-host + test-link window is 33 s on an EPYC node. Splitting the crate lowers this. | 10–20 s |
| Tests (clippy and docs in parallel) | 30–120 s, depending on how far the journal fix goes | 10–30 s |
| Required verdict | 5–10 s | 5–10 s |
| **PR verdict** | **about 1.5–3.5 min** | **about 0.7–1.5 min** |
| Enqueue (auto-merge) + merge-group creation [M: 19 s] + merge-group run (0.3 min if every verdict is reused) + merge [M: 26 s] | 1–4 min | 1–3 min |
| **Push → merged** | **about 3–7 min** | **about 2–5 min** |

Against today's median of 85 min, that is 12–25× faster, and every job whose inputs changed still runs, with the project's own compiler and flags.

**Runner-minutes per merge [Est].**
- Today: 3 PR runs × 52.9 + 64.6 (merge group) + 62.0 (push) ≈ 285 runner-min [EP medians; 3 PR runs per merged PR, M].
- After Phase 3: 3 × 10–15 + about 5 + 0 ≈ 35–50 runner-min, which is 82–88% less.

## 6. Kubernetes use

- **Pod lifecycle.**
  - Each sandbox is a single-use pod. Recycling happens off the critical path.
  - Sandboxes are created ahead of demand. When the plan job starts, `velnor-dispatch` already knows the fan-out, so it pre-creates that many sandboxes on the target nodes.
  - There are three classes: `plain` (no dind), `docker` (dind already passed `docker info`, avoiding the 6 s gate [M]) and `untrusted`.
  - Requests match what a job uses: compile and test sandboxes request 16–32 CPU with `CARGO_BUILD_JOBS` and nextest threads set to match. Memory requests cover the observed 13.4–16.8 GiB peak [EP].
  - The dispatcher admits jobs only while committed CPU on the node stays at or below about 1.2× allocatable.
- **Node affinity.** The dispatcher decides placement (§4), not the default scheduler. Pods are created with `nodeName`, or with required affinity to the chosen node, on a tainted pool `velnor.io/pool=ci`.
- **Priority.**
  - `PriorityClass` order: `velnor-mergequeue` > `velnor-pr` > `velnor-warm` (the trusted builder) > `velnor-batch` (qualification, release).
  - The dispatcher serves merge-group jobs first. Merge-group throughput is what limits merges.
- **Isolation of untrusted PRs.**
  - Forks and unknown trust classes go to GitHub-hosted runners. That is free for this public repo, and PRs are limited to collaborators, so the volume is about 0.
  - If cluster capacity is ever needed for them: a separate tainted node and a gVisor `RuntimeClass`, read-only trusted caches, no secrets, and a NetworkPolicy egress allow-list (GitHub, crates mirror) that denies the API server and metadata endpoints.
  - Trusted dind stays privileged on dedicated tainted nodes, as the owner decided.
- **Both sites.**
  - Four of the five workers are site-mapped only by IP inference [EP §B.0]: DE is velnor-240 and velnor-105; FI is velnor-241, velnor-5 and velnor-190.
  - There is one L2 per site: DE on velnor-240 (46 TB data LV) and FI on velnor-241.
  - The dispatcher keeps a run's jobs inside one site, so run-local artifacts and PR scopes do not cross the WireGuard link (35 ms RTT [EP]). It spreads runs across both sites by load.
  - Add `topology.kubernetes.io/region` labels. None exist today [EP].
- **What more servers would buy.**
  - **More EPYC nodes:** little. After deduplication, CPU demand is small: about 400 runs a day × about 15 CPU-min ≈ 100 CPU-hours a day, against about 11,500 thread-hours available [Est].
  - **High-clock nodes:** big. A Ryzen 9950X-class machine boosts to about 5.7 GHz against 3.8 GHz for the EPYC 9454P (AMD specifications, quoted from memory and not fetched). That shortens the single-crate compile path, which is serial in the rustc frontend, by about 1.3–1.5× [Est].
  - **Two small servers** for an HA k3s control plane, to remove the single point of failure (velnor-237).
  - **10 GbE within each site**, if the uplinks are 1 GbE, for faster L2 transfers.

## 7. The role of mbx and blaze

**mbx: keep it as the compile cache engine, and change how it is deployed.**
- Today every job has a private store in `RUNNER_TEMP` (`MBX_CACHE_DIR=${{ runner.temp }}/velnor/mbx`) and no remote. The first pass therefore has no predictions: "404 compilations had no usable prior inputs or matching prediction".
- Within a job, 243 of 411 compilations in pass 3 are "not looked up" [EP baseline §5].
- mbx already supports what the design needs [Doc: mr-boxington.jdx.dev]:
  - concurrent builds sharing one store, with a shared permit pool and deduplication of identical in-flight compilations;
  - portable keys (checkout paths become placeholders);
  - remotes: a cache server with OIDC and per-namespace grants, S3, or the GitHub cache;
  - writes only from protected-branch pushes; PRs and the merge queue are read-only;
  - prefetch from lockfile history.
- So: one store per node per trust scope, an mbx cache server per site as L2, the node builder as the trusted writer, and `mbx prefetch` when a node warms.
- Two limits:
  - The reference server does not implement "action promises", so it cannot deduplicate cold compilations across nodes [Doc: /cache-server]. Deduplication across jobs on one node still works through the shared local store.
  - mbx is a young tool with essentially one maintainer. Guard against it with a nightly cold canary, the `MBX_VERIFY` sample, and the run-level off switch (§4).

**blaze: not a CI compiler for velnor-new.** Its README says it "builds with its own Rust, whatever rustup or the project pins". That toolchain is `blaze-2026-10-27` with Cranelift codegen, and the README says "rustc and cargo are references, not specs" [Doc: github.com/indexable-inc/blaze README and `docs/v2/plan.md`].
- That contradicts the pinned 1.99.1 (`rust-toolchain.toml`) and goals promise 3, "with the project's own compiler" (`docs/goals.md:36`).
- Its CI and remote-build features are `status: plan`.
- The fit to the pinned toolchain is "no", on the authors' own terms: "Your toolchain and CI stay the final word".
- A permitted use: an optional inner loop for agents before they push (`blaze check` and `blaze test` for faster local failure). 16% of PR runs fail [EP]; each failure caught locally saves one CI round.
- I did not install or measure it on this machine, which was shared and loaded (load average 40–60 on 18 cores).

## 8a. Servers and cluster tuning

**Current state, read-only [EP §B, M]:**
- Debian 13 with kernel 6.12.94; k3s 1.37.2 with flannel `wireguard-native`.
- The kubelet has no `kube-reserved`/`system-reserved`, the CPU manager and topology manager are `none`, eviction is only `nodefs`/`imagefs` at 5%, `podPidsLimit` is -1 and swap is 0.
- 96 logical CPUs per worker, 48 physical, 1 socket: SMT is on (cadvisor `machine_cpu_physical_cores` 48) [M].
- Runner pods request 2 CPU with no limit.
- emptyDirs sit on the single, unmirrored OS NVMe (`nodefs` 3.78 TB). containerd uses the XFS LVM data volume (velnor-240: 46 TB linear over 4 disks).
- Rust 1.99 already links with rust-lld on x86_64 Linux; it has been the default since Rust 1.90 [Doc: blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable].
- **Unknown read-only:** CPU governor, NPS mode, THP, sysctls, NIC speed, the dind storage driver, and RAID/stripe layout.

**Measure these first (owner, on a node):** `cpupower frequency-info`, `lscpu` and `numactl -H`, `cat /sys/kernel/mm/transparent_hugepage/enabled`, `ethtool <nic>`, `iperf3` within a site and across sites, `fio` on the data LV, `xfs_info /var/lib/rancher` (reflink), and `docker info` inside a dind job.

**Changes, ranked by gain per effort:**

| # | Change | Expected gain (evidence) | Risk | Rollback |
|---|---|---|---|---|
| 1 | Set `CARGO_BUILD_JOBS` and nextest threads to the sandbox's CPUs; remove `build.jobs=2` / `--jobs 2` (`mise.toml:90-193`) | `maintenance-helpers` goes from about 8.5 min to about 5 min; its `-j2` compile phases are about 4.5 min [M]. The fan-out is then set by actions-cli (6.5 min) until its looped test is split (§5.5). | Memory peaks per pod (16.8 GiB seen [EP]); set the memory request to match | Revert the config |
| 2 | Pre-seed the action archive cache in the runner image (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`) | −4.4 s per job [M] | None; the runner falls back to downloading | Unset the env var |
| 3 | Put the Rust toolchain and mise tools in a read-only node store or the image | −9 to −11 s per job ("Prepare pinned tools" [M]); "whole lane 112 → 87 s" when mise tools were baked into the image [EP RESULT] | A stale tool; mitigated by keying on the lock-file digest | Point `RUSTUP_HOME` back at `RUNNER_TEMP` |
| 4 | Kubelet `system-reserved`/`kube-reserved` (about 2 CPU, 6 GiB), `evictionHard memory.available=4Gi`, `podPidsLimit=32768`; realistic CPU requests (16–32 for compile jobs) and dispatcher admission | Busy-day slowdown of 1.2–2.4× falls back toward quiet-day durations [EP T6]; fewer dind probe failures (25 in the last hour [M]); protects kubelet and containerd | Lower packing; more scale-set queueing at peaks until deduplication lands | k3s agent args or Helm values |
| 5 | A dedicated cache volume `/var/lib/velnor`: XFS with `reflink=1,noatime`, LVM-striped over the data NVMes (raid0 is fine because it holds cache only); move `kubelet --root-dir`/emptyDir to the data volume | Removes the single OS NVMe as a hotspot; reflink copies of warm target or cache snapshots take about 1 s instead of copying GBs [Est]. Gains build on §4. | Losing a disk loses only cache (rebuilt) | Reinstall the LV; repoint mounts |
| 6 | Debuginfo in CI: `line-tables-only` for workspace crates and `none` for dependencies (what velnor-gen already writes) | Rebuilding only runner-host [M, macOS, `bench/debuginfo.sh`]: full = 151.5 CPU-s and 45.8 s wall (mean of 2); line-tables-only = 121.2 CPU-s (−20%), 44.9 s; none = 110.5 CPU-s (−27%), 39.4 s (−14%). On Linux, where DWARF is linked into each of about 19 test binaries, I expect more [Est]. | Poorer backtraces (line tables keep file:line) | Environment variable |
| 7 | One node-local pull-through registry mirror for dind (`--registry-mirror`); keep the crates mirror and wire it in (#966) | Image pulls inside tests become local; avoids Docker Hub anonymous limits; `cargo fetch` 3.5–6.1 s → 2.7–3.3 s [EP] | The mirror becomes a dependency (fall back to upstream) | Remove the flag |
| 8 | CPU `performance` governor / EPP `performance` (if the driver is amd-pstate-epp) | About 3–10% on short bursty compiles [Est, literature, unverified here]; measure before and after on the canary | Power | `cpupower frequency-set -g <old>` |
| 9 | CoreDNS with 2 replicas plus NodeLocal DNSCache | Fewer stalls in DNS-heavy steps such as crates and GitHub calls; reliability (CoreDNS has 1 replica today [EP]) | Low | Delete the DaemonSet |
| 10 | HA control plane (3 k3s servers) | Reliability: one server stops all CI if it fails | Cost of 2 small servers | Not needed |
| 11 | `fs.inotify.max_user_instances` ≥ 8192, `max_user_watches` ≥ 1M, `nofile` ≥ 1M for containerd | Prevents "too many open files" with dense pods | Low | sysctl |
| 12 | mold instead of rust-lld | Small: links are already lld and take ≤ 5.5 s per test unit including codegen [M, `cargo --timings`] | A second linker to maintain | `RUSTFLAGS` |
| 13 | THP `always` for build pods, `vm.dirty_*` tuning | Unproven for this workload; rustc is often cited at a few % | Memory bloat | sysctl |
| 14 | Keep SMT on; no NUMA pinning on single-socket NPS1. A static CPU manager with `full-pcpus-only` only for timing-sensitive test sandboxes. | SMT adds compile throughput; static pinning loses the ability to burst | Lost burst capacity if used broadly | Kubelet flag |

**Considered and left as they are:**
- **Kernel and scheduler:** 6.12 with EEVDF. No tuning has a shown gain for this workload.
- **Overcommit:** memory limits are about 3× overcommitted [EP]. Fix it with real requests plus eviction (row 4), not `vm.overcommit_memory`. Swap stays off.
- **Filesystems:**
  - btrfs has reflinks, but suffers under heavy small-file churn and fragmentation.
  - ZFS's ARC competes with builds for RAM, and its block cloning is young.
  - XFS with reflink already serves overlayfs (`ftype=1`) and copy-on-write snapshots.
- **tmpfs:** use it only for `/tmp` in test sandboxes. Target directories (3.7 GB per runner-host build [M]) would eat RAM. My CPU sample shows the slow journal tests are CPU-bound, so tmpfs would not fix them.
- **sccache:** redundant with mbx.
- **`CARGO_INCREMENTAL=0`:** correct for CI (incremental bypasses the mbx cache [Doc]).
- **codegen-units:** keep the default 256 for dev and test builds. Heavy dependencies already get `opt-level=3` where needed (`Cargo.toml:69-73`).

**Kubernetes defaults:**
- `local-path` is fine for node-local PVs.
- Flannel `wireguard-native` is required between sites. Its MTU of 1420 on a 1500 underlay is acceptable.
- Image GC at 85/80% is fine with tens of TB.
- `serializeImagePulls=false` is fine.
- Network: keep cache traffic within a site (§6) and measure the NIC speed first.

## 8. Phased plan

| Phase | Weeks | Work | Gate (measured on velnor-new, attempt-1 runs, against the 11-16..18 baseline) |
|---|---|---|---|
| 0: stop waiting | 0–1 | Merge #1332 / v0.1.81 (gate jobs on the cluster); agents enable auto-merge and drop the "0 behind" step; `max_entries_to_build` 2 → 6; run Source qualification on the cluster or only in the merge queue; action archive cache and Rust in the image; `-j` fix (8a #1–3) | PR CI wall median ≤ 12 min (from 27.7), p90 ≤ 16 (79.1); hosted gate queue ≤ 1 min; push-to-merge median ≤ 30 min (85.1), p90 ≤ 60 (456.6) |
| 1: fix the critical path in code | 1–3 | Journal statement-preparation fix (#1333/#1349); split looped tests into one test per case (actions-cli, runner-host); parallel golden tests; skip CI on push to main when the merge group passed the same SHA or tree; kubelet reservations and CPU requests (8a #4); split `velnor-runner-host` (starts here, finishes in Phase 3) | Fan-out span median ≤ 5 min (8.5); runner-host test step ≤ 90 s (192–246); runs per merge 3 → 2; busy-day duration inflation ≤ 1.2× |
| 2: warm nodes (ARC stays) | 2–5 | `velnor-node` DaemonSet: read-only tools, sources, git mirror; shared mbx store per node and trust scope; mbx server per site; trusted builder on each `main` advance; registry mirror; cache volume (8a #5) | Per-job setup ≤ 8 s (37–45); mbx first-pass hit ≥ 90% of units (0%); PR CI wall median ≤ 6 min; runner-min per PR run ≤ 20 (52.9) |
| 3: verify only what changed | 3–7 | velnor-new on velnor-gen (#1290); read tracer in `velnor-node`; verdict store; merge-group reuse (owner approval of hard decision 3) | Merge group runs ≤ 20% of package-jobs (100%); push-to-merge median ≤ 10 min, p90 ≤ 20; 0 push re-verifications; nightly cold canary agrees 100% |
| 4: own the dispatch | 6–10 | `velnor-dispatch` replaces ARC and the Listener, with the official Worker; placement after assignment; trust-scoped mounts; ARC kept as an emergency fallback label | Assignment → first step p90 ≤ 3 s (about 17 today [M]); PR verdict median ≤ 3 min; runner PR ≤ 3.5 min |
| 5 (optional) | after 10 | `velnor-exec` for the velnor-gen subset, with the Worker as fallback and differential runs (same job on both executors, identical step results and outputs) | Adopt only if it saves ≥ 10% of the median verdict time in an A/B on velnor-lab and then velnor-new |

**Top risks: how they are detected and contained.**

| Risk | Detection | Containment |
|---|---|---|
| A stale or false verdict reuse, from an input the tracer missed | Nightly full run without reuse compared with the reused verdicts; 2% of reusable jobs re-executed at random; the Required verdict lists each reused job with its source run; jobs that touch the network or Docker are never reused | `VELNOR_REUSE=off` falls back to a full merge-group run; key schema version bump |
| Cache poisoning | Read-only mounts make the trusted store unwritable from jobs; an L2 write audit (OIDC subject per write); the cold canary | Drop the namespace; rebuild from `main` (minutes with the builder) |
| GitHub protocol change breaks the dispatcher (Phase 4+) | A canary job every 10 min through each path; alerts on acquire, renew or complete errors; track `RunnerRefresh` | Route labels back to ARC and the official Listener, which stay installed |
| mbx defect | `MBX_VERIFY` sampling; the cold canary; mbx stats reported per job | `MBX_REMOTE_MODE=off` and fresh stores |
| Cluster or control-plane outage | Health checks; a dispatcher queue-age SLO (p90 start ≤ 3 s) | Regenerate the workflows with the hosted profile (documented rollback in `velnor-runners/README.md`); HA control plane |
| Merge-queue throughput collapse (like 11-18) | Alert when enqueue → merged p90 exceeds 20 min or queue depth exceeds 6 | Raise `max_entries_to_build`; prioritize merge-group jobs |
| Flaky timing tests under contention (2 s nextest kill timeouts in generator and ci-helper) | Flaky-rate metric per test | CPU admission (8a #4); quarantine and fix the test; never raise the timeout to hide it (`docs/roadmap.md` rule) |

## Owner questions

1. **`docs/goals.md:16`** ("Jobs are always executed by GitHub's official runner").
   - Phase 4 keeps GitHub's `Runner.Worker` but replaces `Runner.Listener` with `velnor-dispatch`. Is that allowed?
   - Phase 5 runs velnor-gen jobs without the official runner. Is that allowed, if gated on a measured ≥ 10% gain?
2. **Hard decision 3** (`docs/hard-decisions.md:208-210, 223-228`: merge group and push always run in full; affected mode there "must not be reintroduced").
   - Do you approve replacing that backstop with per-job read verification and verdict reuse, which is the precondition named at lines 199-204?
   - Separately, do you approve skipping the push run when the merge group already passed the identical SHA or tree (120 of 159)?
3. **`docs/goals.md:45`** ("Inside GitHub's free limits… does not depend on paid storage or larger machines").
   - Do you accept that velnor-new's own CI depends on node-local caches on owned servers, provided the generated workflows still work unchanged on GitHub-hosted runners?
4. **Agent procedure.** Should agents enable auto-merge on every PR, drop the "0 behind main" step and stop catch-up merges of `main`? Should `max_entries_to_build` go from 2 to 6?
5. **Source qualification** runs on every PR event (267 runs and about 6,400 hosted minutes on 11-18 [Est]). Should it move to the cluster or run only in the merge queue?
6. **Splitting `velnor-runner-host`** (157k LOC) into 6–10 crates for compile speed: approved?
7. **Fork and unknown-trust jobs** always go to GitHub-hosted runners and are refused on the cluster: approved?
8. **Hardware.** Should you buy 2 small servers for an HA control plane, and 1–2 high-clock nodes for the critical-path lane? Are the worker uplinks 1 or 10 Gbit/s?

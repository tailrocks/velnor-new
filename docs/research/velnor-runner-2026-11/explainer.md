# Velnor in plain words

This is a short, plain guide to the final design of Velnor. The full design is
[velnor-runner-architecture.md](../../proposed/velnor-runner-architecture.md). The work plan is
[delivery-plan.md](delivery-plan.md); every decision and its evidence is in
[decisions.md](decisions.md); the open questions for the owner are in
[owner-questions.md](owner-questions.md).

**How to read the numbers.** Every number has a label:

- **(measured)**: observed on velnor-new, its cluster or in an experiment.
- **(measured, proxy)**: measured, but on a stand-in clock. Today's push-to-merge starts at the first
  CI run on the final commit, because GitHub does not record the push time.
- **(estimate)**: worked out from measured parts. The arithmetic is in the design.
- **(target)**: a goal. It counts as met only when measurements under real load support it.

**Nothing has run end to end yet.** Every Velnor figure below is an estimate or a target until the
benchmarks in section 9 pass.

## 1. The idea in one paragraph

Today a change to velnor-new waits more than it works. It waits for GitHub's hosted machines, it
runs the same full CI three times (on the PR, in the merge queue and again on `main`), and every
job starts cold: it downloads the code, the tools and the crates, and compiles everything again.
Velnor is a CI runner written in Rust that runs the same GitHub Actions workflows on the project's
own machines. It is fast because it does each piece of work once, keeps the results warm next to
the machine that needs them, and reuses a result only when it can prove that nothing that could
change the result has changed. CI runs only when someone asks: a PR label, or an agent's
`velnor verify`. Agents check their work on the cluster before they push, so when the label
arrives the answer is mostly already there. Correctness comes first: a fast answer that could be
wrong is not allowed, and a false green (a pass that should have been a failure) has zero
tolerance.

## 2. Today versus Velnor: one PR's journey

The PR below is the typical case: it changes only `generator/` or `ci-helper/` (47% of PRs). The
"Velnor" column is the goal for day 10 of the plan; the estimate in brackets is what the design's
arithmetic gives.

| # | Stage | Today (measured) | Velnor goal (target), [estimate] | What removes the time |
|---|---|---|---|---|
| 1 | Event reaches CI | push to run 1–2 s; first job 2 s median, 226 s p90; the Plan job waits 5.5 min (median) for a hosted machine | webhook 1–2 s; the safety re-check within 10 s | Verification starts from the label. No GitHub job sits in the way. |
| 2 | A machine accepts the job | 13 s median, 27 s p90 (n = 1,124) | not on the path | PR checks are no longer GitHub jobs, so nothing is "acquired". |
| 3 | The job environment starts | about 6 s per pod, plus a Docker check | ≤ 1 s P90 when a warm sandbox is free | Sandboxes are made in advance (a "warm pool"); the disk clone inside takes 0.25 s (measured). Only the three Docker test binaries pay a few seconds for their own Docker daemon. |
| 4 | Checkout | 6 s per job, × 31–39 jobs per run | ≤ 1 s | Each machine keeps a copy of the repository, clones it locally, then checks every file. Checking 5,164 files took 0.43 s on eight threads (measured, proxy: on a Mac). |
| 5 | Tools and toolchain | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | A read-only store of pinned tools on each machine. |
| 6 | Crate downloads | 3.5–6.1 s per job; the crate cache was used 0 of 48 times | 0 s (≈ 3 s if `Cargo.lock` changed) | A crates mirror per site and a crate store per machine. |
| 7 | Compile | 196–497 crates per job; 8.5 min on the critical path | ≤ 25 s [15 s] | Start from a verified, already-built tree; compile only what changed. An unchanged tree built 0 crates in 0.31 s (measured). A change that adds a source file (64% of recent commits, measured) still rebuilds its folder cold until a lab proves the narrower rule. |
| 8 | Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s | ≤ 60 s [10 s] | Run only tests whose inputs changed, twice in parallel on two machines; fix the slowest tests. |
| 9 | Report results | a 1.48 GB upload, 36–43 s; the verdict job 49 s | ≤ 1 s | Results stream as they happen; binaries are stored once by content. |
| 10 | Required check turns green | waits 1.6 / 24.3 min for a hosted machine, then 1.9 / 21.4 min to run | ≤ 1 s after the last result | Velnor's App writes one status, `Velnor / Verified`, directly. |
| 11 | Merge queue | green to enqueue 7.5 min (nobody used auto-merge: 0 of 40); enqueue to group 19 s; group run 19.9 / 84.1 min | ≤ 30 s [25 s] | Auto-merge; the queue reuses the PR's results when the tree is the same (about 60–75% of groups, estimate; 16% today). |
| 12 | Merge | 26 s | 26 s | This is GitHub's own merge queue. Velnor does not change it. |

**Totals.**

| | PR verdict, P50 / P90 | Push-to-merge, P50 / P90 | Runner-minutes per merged PR |
|---|---|---|---|
| Today | 26.7 / 92.3 min (measured, proxy) | 85 / 457 min (measured, proxy) | 285–347 (estimate, CI only) |
| First full Velnor version (day 5.2) | ≈ 6 / 18 min (estimate) | ≈ 9 / 30 min (estimate) | ≈ 110 (estimate) |
| Goal (day 10) | ≤ 90 s / ≤ 4 min (target) [≈ 35 s] | ≤ 3 / ≤ 8 min (target) [≈ 85 s] | ≤ 50 (target) [≈ 38] |
| Stretch (days 16–18) | 30 s / 100 s (target) | 100 s / 4 min (target) | ≤ 20 (target) |

P50 is the middle PR. P90 is the slow one in ten. Push-to-merge is reported twice: gross, and net of
GitHub's own queue time, which no runner controls.

## 3. Why it is fast

Today's time has four causes (measured): **waiting** (on the worst day, 57.0 of a 71.8-minute run
was gate jobs waiting for GitHub's free plan, which allows 20 hosted jobs at once), **repetition**
(the same work in three runs), **cold jobs** (nothing survives a job) and **slow tests**. Velnor
removes work rather than speeding it up, with five mechanisms. Most of the speed comes from the
verification system (the label flow, the ledger of results, mirrors and warm builds), not from the
Rust executor: PR and merge-group checks run no GitHub Actions job at all.

### 3.1 Nothing runs that doesn't need to

- **The label flow.** A push starts nothing. CI starts when someone adds the `velnor:verify` label
  to a PR that is ready for review. Today 45% of PR runs (1,156 of 2,558) were cancelled before they
  finished (measured), and a merged PR had 4.77 PR runs on average (measured).
- **No re-runs on an unchanged head.** Editing the PR text started 18.3% of PR runs (469), each
  costing 52.9 runner-minutes (measured). Velnor drops that trigger.
- **Build once, verify once.** Today the same commit is fully tested three times (120 of 159
  merges, measured). Velnor splits CI into small units called *obligations* (one lint, one test
  binary, one doc build). Each has a *key*: a fingerprint of every input it reads. A key is verified
  once, and its record is reused wherever the same key appears: across pushes, across PRs, in the
  merge queue, on `main` and at release.
- **Reuse across PRs.** Today each crate compiles 4.4 times per run (2,106 builds of 475 crates;
  measured).
- **A new push never waits.** The previous run is cancelled at once. Today a new push waited up to
  2,203 s for the old run to stop (measured).
- **Releases promote, never rebuild.** Today the tools release takes 190–434 s and a product release
  4,105–6,238 s (measured). With promotion it is zero compiles and about 15 s (estimate).

### 3.2 Nothing waits

- **Velnor's own dispatcher** places and starts jobs; ARC (the Kubernetes controller that runs
  GitHub's runner today) goes away. Job start: ≤ 1 s P50, ≤ 2 s P90 (target), against 13 s / 27 s
  today (measured).
- **Warm environments,** sized by a floor plus the plan's own forecast of demand. Idle ones hold
  little memory and are resized when claimed.
- **No GitHub-hosted queue.** Today it cost the Plan job 5.5 minutes at the median and the verdict
  job up to 24.3 minutes at P90 (measured).
- **No waiting between green and merge.** Agents turn on auto-merge. Today the median PR waited 7.5
  minutes between green and entering the queue (measured).
- **A reserved lane for merges.** One 16-CPU build slot and 2 gate CPUs per site (about 7.7% of
  CPU). A merge group that must run something gets a pair of warm slots, so its second run never
  waits behind the first.

### 3.3 Nothing is fetched twice

- **Node mirrors.** Each machine keeps a copy of the repository, crates, Rust toolchains, mise tools
  and container images. Each new version crosses the internet about once per site. Saved: about 5 s
  of checkout, 4.5 s of crates and 14 s of tools per job; at most about 62 runner-hours a day today
  (estimate, an upper bound).
- **Local caches.** `actions/cache` works exactly as on GitHub, but the data sits on the machine's
  own fast disk (L0), then on the site's storage cluster (L1), then on the other site (L2). Today
  one 1.92 GB restore from GitHub's cache took 44–61 s (measured), longer than the build it saved.
- **Prefetch.** When a cache key can be computed before the job (for example from `Cargo.lock`),
  Velnor warms the local copy first.
- **RustFS per site.** RustFS is an S3-compatible storage server written in Rust. It holds only
  large blobs, because unpacking costs more than fetching: a 6,053-file registry took about 1.2 s to
  download and about 17 s to unpack, against about 5 s to clone an unpacked copy (measured on a
  loaded Mac, indicative). Saves are split into content chunks, so a one-crate lockfile change
  uploaded about 1% of the bytes (measured, one pair).

### 3.4 Nothing is rebuilt

- **Warm snapshots.** A *generation* is a full build output that has been checked against a cold
  rebuild. A job gets a disk clone of it. On the runner workspace (357 crates, measured on the
  cluster): a cold build takes 92 s; the clone takes 0.25 s and 2.5 MB; the build after it takes
  0.31 s with 0 crates to compile. Today's copy on the ext4 disk takes 8.6 s and 10.75 GiB.
- **Only the change compiles.** Velnor compares files with the generation's file list by content,
  not by timestamp.
- **Release promotion by digest.** A *digest* is a hash of the exact bytes. The release job checks
  the digest, verifies the attestation and publishes.

### 3.5 The native executor

- **No Node.** GitHub's official runner starts Node.js for each JavaScript action (87–147 ms each,
  measured) and spends 4–5 s setting itself up per job (measured, ARC era). Velnor Native
  implements the actions velnor-new uses directly in Rust.
- **Local state.** Native actions read the machine's mirror, tool store and caches directly.
- **No Docker unless needed.** Today every pod starts a Docker daemon (3–4 s per job, 57 probe
  failures an hour; measured). Only the three test binaries that need Docker get a private daemon.
- **Honest scope.** The executor saves seconds on the jobs that stay GitHub jobs (release, image,
  scheduled), not on PR verdicts. The gain is unmeasured. Velnor will claim an executor speed-up
  only if it is at least 5 s per job at P50 on at least three job types (benchmark S2). The official
  runner stays as the reference, and you can choose it per job type.

## 4. How agents use it

The agents that write velnor-new's code check their work through Velnor, on warm cluster machines,
in the same sealed sandbox the gate uses. So their checks count, and the label later finds the work
done. This arrives at about day 6.5. The gate's meaning does not change.

- **`velnor cargo`.** It takes the cargo wrapper slot in `mise.toml` (replacing `mbx`, which
  reused 0 of 10 workspace units locally; measured). When a command matches a CI check, such as
  `cargo nextest run -p velnor-ci`, it asks the ledger first, then runs on a warm cluster machine
  and shows a trust tier. Anything else runs on the Mac as before. The first release only counts
  and records commands, so the real usage is known before anything depends on it.
- **`velnord`.** A small daemon per user keeps state, adding at most 50 ms per call at P90
  (target). It snapshots the worktree as a Git tree (tracked and untracked files, never ignored
  ones) and sends only the new objects to the cluster's mirror. A later commit or push of the same
  tree has the same key.
- **`velnor verify`.** It runs the tree, merged with the merge-queue tip, on two cluster machines:
  both required runs. Any two qualified runs count, for example a builder agent's and its reviewer
  agent's.
- **`velnor ship`.** It verifies, pushes, adds the label with the agent's own identity and turns on
  auto-merge. Shipping is an explicit act, so it respects decision 12 (CI only on request).
- **Helpers.** `velnor resolve` asks the ledger what is already done, without running anything
  (44 of 107 agent sessions ran cargo before editing anything; measured). `velnor wait` follows the
  verdict and, on the first red, prints the failing test and a `velnor repro` command. A local quick
  pass on the Mac races the cluster to show reds early; its green is only provisional.
- **Where it runs.** Counting commands go to the cluster when that is within 1 s of the Mac's
  speed (the Mac's nextest P50 is 7.2 s; measured). Agent work runs in an `interactive` class with a
  budget of 30 CPU-minutes an hour per identity, never touching the merge, gate or audit lanes.

**The trust tiers,** shown on every result:

| Tier | Where it ran | Counts at the gate? |
|---|---|---|
| T0 | The Mac, plain | No: advice |
| T1 | A sealed Linux sandbox on the Mac | No: advice |
| T2 | A cluster machine, the gate's sealed sandbox | Yes: fills one of a key's two run slots |
| T3 | On `main`, after the cold audit | Yes: trusted |

**Why the PR verdict becomes mostly a lookup.** When the label arrives, Velnor works out the keys
for the PR head. Keys `verify` already ran are read from the ledger. Only misses, and checks that
read the clock or network, run. So the verdict's floor is the slowest clock-or-network check plus
about 3 s (estimate), and about 3 s for the helper and generator class once their clock reads are
made injectable (work item T08). The target for the first 50 shipped PRs is label to green within
10 s at P50. The work is not free: it runs earlier, and its cost is charged once, to whoever asked.

## 5. Why it stays correct

The rule is simple: if a shortcut could turn a real failure green, it is not used.

- **One gate, and only `success` passes.** A GitHub App writes one commit status, `Velnor /
  Verified`, the only required check. No Actions job is required, because GitHub counts a *skipped*
  required job as passing. A missing status, `pending`, `failure` and `error` all block. The bypass
  list is empty. Today an admin bypass carried 7 of 136 merges in three days (measured).
- **Every landed commit is checked (ALLGREEN).** Today three commits whose own required check
  failed sit on `main` (measured).
- **Two independent runs.** Every new result needs two passing runs: on two machines, in two
  attempts, with a different test order, over two different verified builds or one cold build. Any
  failure blocks, with no automatic retry. A bug that fails with probability p is caught with
  probability 1 − (1 − p)², as today. Agents' Mac results never fill a slot.
- **Cold audits.** After a commit lands, Velnor rebuilds it from nothing and re-runs its changed
  checks (audit A1), within 2 hours at P90 (target). A commit not audited after 6 hours pages the
  owner and holds releases. A nightly audit (A2) moves the clock 8 and 30 days ahead.
- **Keys cover every input.** Sources, toolchain, dependency versions, environment, flags,
  features, the binaries a test runs, data files, the machine's kernel and CPU, and the executor.
  Every check runs on the full tree, because a trimmed tree turned a real failure green (measured,
  reproduced). A file-access trace never proves a key complete: in one reproduction a test's
  hash-map order picked a different fixture each run, so a trace-based lookup passed a tree on
  which 12 of 40 fresh runs fail (measured). Narrower keys need a reviewed contract.
- **New shortcuts earn trust slowly.** A new kind of reuse runs in shadow, with every reuse
  re-executed, until about 3,000 clean comparisons (that bounds the error near 0.1% at 95%, never
  zero). After that 2% are still re-run. The first disagreement switches that kind of reuse off.
- **Canaries for known false greens.** Permanent test PRs that must never be mergeable are checked
  every 10 minutes; if one becomes mergeable, Velnor stops writing `success` and pages the owner.
  Other canaries re-create past bugs every hour.
- **Conformance against the official runner.** At least 240 behaviour, 65 fault and 30 refusal
  cases, plus real velnor-new jobs, run through both executors on both platforms. They must agree
  100%, and at least 95% of about 30 planted bugs must be caught. This runs on every executor change,
  nightly, and within 14 days of each official runner release.
- **Zero tolerance for false greens; a budget for false reds.** A *false red* is a failure that
  passes on a rerun (a flake). Today 8.1% of full runs had one (measured); two runs at that rate
  would fail falsely 15.5% of the time. The budget per verified record is 3% at first and 1% at the
  goal. Flaky tests are counted, never retried. A flaky test is fixed, or quarantined only when
  another blocking check is shown to catch the same planted fault.

## 6. The honest limits

### 6.1 What no runner can remove

- **GitHub's merge queue.** 19 s from enqueue to group and 26 s to merge (measured medians under
  load; not a proven floor): about 45 s per merge, about half the goal estimate. Owner decision 7
  keeps GitHub's queue; a Velnor merge train is open question D18.
- **The longest single test.** A test cannot be split across machines. The 924.8 s test is deleted
  with the old runner at day 6.5. What stays: a 679 s suite, a 132 s test and looped tests of
  258–274 s (measured). Only fixing the tests helps; a high-clock machine might give 1.3–1.5×
  (estimate, open question D20). Goal: no test over 60 s (stretch 15 s).
- **Evidence that needs real traffic.** 3,000 clean comparisons per kind of reuse, 300 PR heads and
  100 merge groups of parity, 300 rerun pairs for a flake rate. At today's volume 100 merge groups
  take about 44 hours (estimate). More agents cannot make them arrive faster.

### 6.2 The floors for this repository

- **With the label alone,** for the helper and generator class: about 20–35 s to the verdict
  (webhook and planning about 2 s, one crate compile 8–20 s, tests 2–10 s), so push-to-merge floors
  near 65–80 s with GitHub's share (estimate).
- **With `velnor verify` first:** the verdict is a lookup, about 3 s once clock reads are injected
  (estimate). Push-to-merge is then mostly GitHub's 45 s.
- **The P90 has no such floor.** About 20% of recent commits (42 of 214, measured) touched a path
  that every check depends on (`.github/`, `.velnor/`, root manifests). For those, the slowest
  check sets the verdict.

### 6.3 Why the targets are what they are

- The goal (verdict ≤ 90 s P50, ≤ 4 min P90; push-to-merge ≤ 3 min P50, ≤ 8 min P90) sits above the
  ≈ 35 s / ≈ 85 s estimate on purpose: P90 must absorb misses, cold builds, queue ejections and a
  busy cluster.
- It needs per-folder keys (after the file-access audit), compile reuse proven (3,000 warm-against-
  cold comparisons per Cargo root) and the slow tests fixed.
- Until compile reuse is proven, the second run builds cold, so day 5.2 is slower: about 3 minutes
  of cold build plus about 2.5 minutes for the longest test binary (estimate).
- The stretch also needs per-package keys and stamping the commit id after linking.

## 7. How Velnor compares

Facts about other systems come from the [landscape dossier](runner-landscape.md) (mostly vendor
claims) and the [Dagger comparison](velnor-vs-dagger.md). No other system has been raced on this
repository, so every "leads" below is a prediction until benchmark S3 runs.

| System | What it does well | Where Velnor should lead | Where Velnor doesn't lead (yet) |
|---|---|---|---|
| GitHub-hosted runners | No operations; a clean VM per job; larger machines on demand; GitHub's own merge queue | The 20-job queue (57 of 71.8 min, measured), three full runs per merge, cold jobs; "very likely 10–40× faster" push-to-merge (estimate, against a handicapped baseline) | Velnor must be run and maintained on the project's own cluster; nothing measured yet |
| ARC (today) | Runs the official runner exactly; Kubernetes-native | 13 s / 27 s acquire, about 6 s pod start, a Docker daemon in every pod, 8.6 s disk copies, three runs per merge (all measured) | ARC is mature; Velnor's executor must prove conformance; the executor-only gain is unmeasured (S2) |
| Buildkite-style persistent agents | Long-lived agents, git mirrors, warm local state, timing-based test splits, flake quarantine | Predicted tie on compile and tests (same cargo, same CPUs); a lead only through result reuse and having no Actions job on the verdict | Unmeasured. The stand-in arm (a strong conventional baseline on the same workers) keeps the Actions hop, so it bounds them only partly |
| Blacksmith, Depot, Namespace-style rented runners | High-clock CPUs (for example Ryzen 7950X at 5.7 GHz), caches next to the machine, NVMe cache volumes, standby pools; vendors claim about 2× | Predicted ahead on push-to-merge: they still start from an Actions job and repeat the run for PR, group and push | Predicted behind per thread: the workers are EPYC 9454P throughput parts (1.3–1.5×, estimate). No claim until a rented arm runs (open question D22) |
| Bazel / Buck2 with remote execution | Cache and execution per action, hermetic by construction, many remote workers; wins cold and wide builds | Warm local cargo should win small edits; no rewrite of the build; stricter audits of reuse | Velnor's finer keys must be earned by enrolment; RBE gets them by construction. Cold, wide builds are excluded from any claim unless the optional RBE arm runs |
| Dagger | Typed pipelines in eight languages; the same engine on laptop and CI; a content-addressed cache; traces; a working product today | Only observed, witnessed results count (Dagger reuses whatever its engine computed); a per-commit gate where only `success` passes; clock and network handled by the system; reuse is audited; tests are counted; existing YAML unchanged | Dagger exists and runs today; Velnor is a design. Velnor borrows its structured keys, checks by name and OpenTelemetry traces |

## 8. The honest speed claim

- **Today's claim:** Velnor is *designed to cut velnor-new's push-to-merge, very likely 10–40×
  faster than its historical hosted and ARC CI* (estimate, against handicapped baselines).
- **After the benchmarks pass:** *the fastest measured push-to-merge on a specified velnor-new
  workload, at a stated resource budget, with the same verification contract.*
- **Never claimed:** "the fastest runner ever". A tie, an arm not run, or a class lost narrows the
  sentence; one false green or skipped check disqualifies the result.

## 9. Proving the speed

The rules are frozen first: workload versions, expected test IDs, toolchain, timeouts and both
exact subjects; every arm must agree with an independent cold run. The workload is 300 historical
PR heads replayed at their real times, plus the trace of 2026-11-18. Every self-hosted arm gets
the same two FI workers to itself; rented arms get at most the same vCPUs. Every arm reports CPU
time, memory, bytes and cost.

| # | Benchmark | What it shows | Pass |
|---|---|---|---|
| S1 | Planted failures | 100 failing edits on every fast path (old-timestamp edit, hidden build-script input, new sibling file, clock shift, stale build, seeded flakes) | 100% red, or caught at the expected rate; it gates every other run |
| S2 | Control race | Native against Official on the same warm machines, ≥ 30 interleaved pairs per job type at 15% and 80% CPU; also today's ARC with phase 0 | Per-type differences with intervals; an executor claim only at ≥ 5 s per job |
| S3 | Head-to-head | Today's CI; a strong conventional baseline on the same workers; Velnor with Official; Velnor Native; Velnor Native with agents' `verify`; rented arms if D22 says yes | The publication rule below, on push-to-merge (gross and net) and the verdict |
| S4 | Loaded replay | The busy day of 2026-11-18 with other organisations' load, audits and release work; then a manifest edit, a cold toolchain, a lost worker | Verdict P90 ≤ 1.5× the quiet value; merge-lane wait P90 ≤ 10 s |
| S5 | Census | How many checks read the clock or network, and how many keys stay whole-tree, on `main` and 300 heads | Shares published; goals restated from them |
| S6 | Warm compile | 300 real diffs per Cargo root over a clone, idle and at 70–90% load, against a cold control | Slower run P90 ≤ 25 s (≤ 40 s loaded); outputs equal to cold |

**Publication rule** (fixed in advance): Velnor is called faster than an arm only if, for each PR
class, the 95% intervals for both the P50 and the P90 difference sit entirely below zero. The claim
is planned for about day 15 (day 19 at P90), and only against the arms that ran.

## 10. What gets built, by whom, by when

Agents do all the building: 12 builders, 4 reviewers and 1 orchestrator, around the clock. The work
is about 1,292 agent-hours of building and 120 of review, about 4.5 days of fleet time (estimate).
So coding does not set the dates; CI throughput, reviews, owner decisions and real-traffic evidence
do. "Day N" counts from the start; P90 dates are in brackets. Figures are estimates, except the goal
and stretch rows, which are targets.

| Day | Milestone (and what it deletes) | Verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR |
|---|---|---|---|---|
| Today | Baseline (measured, proxy) | 26.7 / 92.3 min | 85 / 457 min | 285–347 |
| 0 | Phase 0: safer ruleset, gate jobs off hosted machines, auto-merge. Deletes `edited`, admin bypass, fork job, `-j2` caps, macOS legs | ≤ 12 / ≤ 20 min | ≤ 35 / ≤ 60 min | ≤ 170 |
| 1.2 | First conformance runs on Docker, both executors | ≤ 11 / ≤ 18 min | ≤ 30 / ≤ 55 min | ≤ 160 |
| 2.1 (3) | Cohort 1 (gate jobs) on Velnor Native on Kubernetes | ≤ 9 / ≤ 16 min | ≤ 25 / ≤ 45 min | ≤ 150 |
| 3.2 (4.5) | Cohort 2 (the 26 crate jobs) | ≤ 6 / ≤ 10 min | ≤ 15 / ≤ 28 min | ≤ 120 |
| 3.8 (5.3) | Cohort 3 (Docker tests on private daemons) | ≤ 5 / ≤ 9 min | ≤ 13 / ≤ 25 min | ≤ 110 |
| 4.2 (6.5) | The App gate is required; the label flow is on | ≤ 5 / ≤ 9 min | ≤ 12 / ≤ 22 min | ≤ 110 |
| 4.8 (7) | Cohort 4 (releases) on Velnor | — | — | — |
| 5.2 (7.5) | Velnor checks PRs and groups itself; audit A1 replaces the full push run | ≈ 6 / 18 min | ≈ 9 / 30 min | ≈ 110 |
| 6.5 (9) | Final deletion of ARC and the old runner code (the official runner image is kept); `velnor verify` and `ship` for agents | ≈ 4 / 12 min | ≈ 7 / 22 min | ≈ 90 |
| 7.5 (10) | Compile reuse proven on the helper and generator | ≈ 2.5 / 8 min | ≈ 5 / 15 min | ≈ 80 |
| 10 (13.5) | **Goal**: per-folder keys, slow tests fixed | **≤ 90 s / ≤ 4 min** | **≤ 3 / ≤ 8 min** | **≤ 50** |
| 15 (19) | **Speed claim published**, if S1–S6 pass | — | — | — |
| 16–18 | **Stretch**: per-package keys, longest test ≤ 15 s, commit stamped after link | 30 s / 100 s | 100 s / 4 min | ≤ 20 |

Day 5.2 is briefly slower than day 4.2 because the second run builds cold until compile reuse is
proven. A milestone that misses its numbers holds the next one until the miss is explained.

## 11. What is still open

The plan runs on a default for each until you answer. The full sheet, with examples, stakes and
what each blocks, is [owner-questions.md](owner-questions.md).

- **Hour 0–12:** D1 (release the engine that moves gate jobs off hosted machines), D9 (pre-approve
  CI-only engine releases), D2 (auto-merge, no catch-up merges), D3 (the first ruleset update), D8
  (deploy identity and node changes), D7 (the `velnor-verify` App and an agent identity).
- **Days 1–3:** D11 (cutover counts), D15 (recovery from a bad Velnor release), D16 (a label added
  during draft), D4 (which expression evaluator), D17 (who may remove a test), D19 (compile before
  the label: recommended no).
- **Days 4–7:** D20 (rent a high-clock machine), D22 (rent comparator runners for the benchmark),
  D24 (agent work above other organisations' PR work), D21 (one compile for both runs: recommended
  no).
- **Days 8–10:** D14 (delete the engine crates), D18 (a Velnor merge train), D23 (automatic
  verification of unpushed commits), D25 (agents working inside cluster sandboxes), D26 (a native
  arm64 lane).

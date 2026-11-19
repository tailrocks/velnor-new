# Velnor in plain words

This is a short, plain guide to the final design of Velnor. The full design is
[velnor-runner-architecture.md](../../proposed/velnor-runner-architecture.md). The work plan is
[delivery-plan.md](delivery-plan.md), and every decision and its evidence is in
[decisions.md](decisions.md) and [evidence.md](evidence.md).

**How to read the numbers.** Every number has a label:

- **(measured)**: observed on velnor-new, its cluster or in an experiment.
- **(measured, proxy)**: measured, but on a stand-in clock. For example, today's push-to-merge
  starts at the first CI run on the final commit, because GitHub does not record the push time.
- **(estimate)**: worked out from measured parts. The arithmetic is in the design.
- **(target)**: a goal. It counts as met only when measurements under real load support it.

## 1. The idea in one paragraph

Today a change to velnor-new waits more than it works. It waits for GitHub's hosted machines, it
runs the same full CI three times (on the PR, in the merge queue and again on `main`), and every
job starts cold: it downloads the code, the tools and the crates, and compiles everything again.
Velnor is a CI runner written in Rust that runs the same GitHub Actions workflows on the project's
own machines. It is fast because it does each piece of work once, keeps the results warm next to
the machine that needs them, and reuses a result only when it can prove that nothing that could
change the result has changed. CI runs only when you ask for it, with a PR label. Correctness comes
first: a fast answer that could be wrong is not allowed. A false green (a pass that should have
been a failure) has zero tolerance.

## 2. Today versus Velnor: one PR's journey

The PR below is the typical case: it changes only `generator/` or `ci-helper/` (47% of PRs). The
"Velnor" column is the goal for day 10 of the plan; the estimate in brackets is what the design's
arithmetic gives.

| # | Stage | Today (measured) | Velnor goal (target), [estimate] | What removes the time |
|---|---|---|---|---|
| 1 | Event reaches CI | push to run 1–2 s; first job 2 s median, 226 s p90; the Plan job waits 5.5 min (median) for a hosted machine | webhook 1–2 s; the safety re-check within 10 s | Verification starts from the label. No GitHub job sits in the way. |
| 2 | A machine accepts the job | 13 s median, 27 s p90 (n = 1,124) | not on the path | PR checks are no longer GitHub jobs, so nothing is "acquired". |
| 3 | The job environment starts | about 6 s per pod, plus a Docker check | ≤ 0.3 s | Environments are created in advance (a "warm pool"); a disk clone takes 0.25 s (measured). |
| 4 | Checkout | 6 s per job, × 31–39 jobs per run | ≤ 1 s | Each machine keeps a copy of the repository and clones it locally, then checks every file. |
| 5 | Tools and toolchain | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | A read-only store of pinned tools on each machine. |
| 6 | Crate downloads | 3.5–6.1 s per job; the crate cache was used 0 of 48 times | 0 s (≈ 3 s if `Cargo.lock` changed) | A crates mirror per site and a crate store per machine. |
| 7 | Compile | 196–497 crates per job; 8.5 min on the critical path | ≤ 25 s [15 s] | Start from a verified, already-built tree; compile only what the change touches. An unchanged tree built 0 crates in 0.31 s (measured). |
| 8 | Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s | ≤ 60 s [10 s] | Run only tests whose inputs changed, twice in parallel on two machines; fix the slowest tests. |
| 9 | Report results | a 1.48 GB upload, 36–43 s; the verdict job 49 s | ≤ 1 s | Results stream as they happen; binaries are stored once by content. |
| 10 | Required check turns green | waits 1.6 / 24.3 min for a hosted machine, then 1.9 / 21.4 min to run | ≤ 1 s after the last result | Velnor's App writes one status, `Velnor / Verified`, directly. |
| 11 | Merge queue | green to enqueue 7.5 min (nobody used auto-merge: 0 of 40); enqueue to group 19 s; group run 19.9 / 84.1 min | ≤ 30 s [25 s] | Auto-merge; the queue reuses the PR's results when the tree is the same (about 60–75% of groups, estimate; 16% today). |
| 12 | Merge | 26 s | 26 s | This is GitHub's own merge queue. Velnor cannot change it. |

**Totals.**

| | PR verdict, P50 / P90 | Push-to-merge, P50 / P90 |
|---|---|---|
| Today | 26.7 / 92.3 min (measured, proxy) | 85 / 457 min (measured, proxy) |
| First full Velnor version (day 5.2) | ≈ 6 / 18 min (estimate) | ≈ 9 / 30 min (estimate) |
| Goal (day 10) | ≤ 90 s / ≤ 4 min (target) [≈ 35 s] | ≤ 3 / ≤ 8 min (target) [≈ 85 s] |
| Stretch (days 16–18) | 30 s / 100 s (target) | 100 s / 4 min (target) |

P50 is the middle PR. P90 is the slow one in ten. Runner-minutes per merged PR go from 285–347
today (estimate, CI only) to about 110 at day 5.2 (estimate) and 50 or fewer at the goal (target).

## 3. Why it is fast

Today's time has four causes (measured): **waiting** (on the worst day, 57.0 of a 71.8-minute run
was gate jobs waiting for GitHub's free plan, which allows 20 hosted jobs at once), **repetition**
(the same work in three runs), **cold jobs** (nothing survives a job) and **slow tests**. Velnor
attacks each one with five mechanisms.

### 3.1 Nothing runs that doesn't need to

- **The label flow.** A push starts nothing. CI starts when someone adds the `velnor:verify` label
  to a PR that is ready for review. Today 45% of PR runs (1,156 of 2,558) were cancelled before they
  finished (measured), and a merged PR had 4.77 PR runs on average (measured).
- **No re-runs on an unchanged head.** Editing the PR text started 18.3% of PR runs (469), each
  costing 52.9 runner-minutes (measured). Velnor drops that trigger. A rebase that leaves the tree
  the same gets its answer from the record in seconds.
- **Build once, verify once.** Today the same commit is fully tested three times: on the PR, in the
  merge queue, and on `main` (120 of 159 merges, measured). Velnor splits CI into small units it
  calls *obligations* (one lint, one test binary, one doc build, and so on). Each obligation has a
  *key*: a fingerprint of every input it reads. Each key is verified once and its record is reused
  wherever the same key appears.
- **Reuse across PRs.** Two PRs, a stacked PR, or a PR and its merge queue entry share any record
  with the same key. Today each crate compiles 4.4 times per run (2,106 builds of 475 crates;
  measured).
- **A new push never waits.** The previous run is cancelled at once. Today a new push waited up to
  2,203 s for the old run to stop (measured).
- **Releases promote, never rebuild.** A release publishes the exact bytes the merge queue built
  and checked. Today the tools release takes 190–434 s and a product release 4,105–6,238 s
  (measured). With promotion it is zero compiles and about 15 s (estimate).

### 3.2 Nothing waits

- **Velnor's own dispatcher** places and starts jobs. ARC (the Kubernetes controller that runs
  GitHub's runner today) goes away. The target for a job start is ≤ 1 s P50 and ≤ 2 s P90 (target),
  against 13 s / 27 s today (measured).
- **Warm environments.** Sandboxes are created before work arrives, so start-up is ≤ 0.3 s
  (target) instead of about 6 s (measured).
- **No GitHub-hosted queue.** Every job runs on the project's cluster. Today the hosted queue cost
  the Plan job 5.5 minutes at the median and the verdict job up to 24.3 minutes at P90 (measured).
- **No waiting between green and merge.** Agents turn on auto-merge when they open a PR. Today the
  median PR waited 7.5 minutes between green and entering the queue (measured).
- **A reserved lane for merges.** One 16-CPU build slot and 2 gate CPUs per site are kept for merge
  work, so a busy cluster never blocks the queue (about 7.7% of CPU).

### 3.3 Nothing is fetched twice

- **Node mirrors.** Each machine keeps a copy of the repository. Checkout falls from 6 s to ≤ 1 s
  (target). The same idea covers crates, Rust toolchains, mise tools and container images: each new
  version crosses the internet about once per site.
- **What it saves.** At most about 62 runner-hours a day, about a fifth of today's total (estimate,
  an upper bound): about 5 s per job on checkout, 4.5 s on crates and 14 s on tools.
- **Local caches.** `actions/cache` works exactly as on GitHub, but the data sits on the machine's
  own fast disk (L0), then on the site's storage cluster (L1), then on the other site (L2). Today
  one 1.92 GB restore from GitHub's cache took 44–61 s (measured), longer than the build it saved.
- **Prefetch.** When a cache key can be computed before the job starts (for example from
  `Cargo.lock`), Velnor warms the local copy first.
- **RustFS per site.** RustFS is an S3-compatible storage server written in Rust. Each site gets
  its own cluster. It holds only large blobs (cache chunks, artifacts, release bytes), because
  unpacking costs more than fetching: a 190 MiB registry took about 1.2 s to download and about
  17 s to unpack, against about 5 s to clone an unpacked copy (measured on a loaded Mac,
  indicative). Saves are split into content chunks, so a one-crate lockfile change uploaded about
  1% of the bytes (4 of 419 chunks; measured, one pair).

### 3.4 Nothing is rebuilt

- **Warm snapshots.** Velnor keeps a *generation*: a full build output that has been checked
  against a cold rebuild. A job gets a disk clone of it. On the runner workspace (357 crates,
  measured on the cluster): a cold build takes 92 s; the clone takes 0.25 s and 2.5 MB; the build
  after it takes 0.31 s with 0 crates to compile. Today's copy on the ext4 disk takes 8.6 s and
  10.75 GiB.
- **Only the change compiles.** Velnor compares the request's files with the generation's file list
  by content, not by timestamp. Files that changed are rebuilt; everything else is reused.
- **Release promotion by digest.** A *digest* is a hash of the exact bytes. The release job checks
  the digest, verifies the attestation and publishes. It compiles nothing.

### 3.5 The native executor

- **No Node.** GitHub's official runner starts Node.js for each JavaScript action (87–147 ms each,
  measured) and spends 4–5 s setting itself up per job (measured, ARC era). Velnor Native
  implements the actions velnor-new uses directly in Rust.
- **Local state.** The native actions read the machine's mirror, tool store and caches directly,
  instead of downloading from GitHub.
- **No Docker unless needed.** Today every pod starts a Docker daemon (3–4 s per job, with 57 probe
  failures an hour; measured). Velnor gives a private Docker daemon only to the three test binaries
  that need one.
- **The official runner stays as an option.** It is the reference for testing, and you can choose
  it per job type. The real speed difference per job type is not yet measured; work item O04
  measures it.

## 4. Why it stays correct

The rule is simple: if a shortcut could turn a real failure green, it is not used.

- **One gate, and only `success` passes.** A GitHub App writes one commit status, `Velnor /
  Verified`. It is the only required check. No GitHub Actions job is required, because GitHub
  counts a *skipped* required job as passing. A missing status, `pending`, `failure` and `error`
  all block. Nobody can bypass the rule: the bypass list is empty. Today an admin bypass carried 7
  of 136 merges in three days (measured).
- **Every landed commit is checked (ALLGREEN).** Today three commits whose own required check failed
  sit on `main` (measured). The new queue setting checks every commit in a group.
- **Two independent runs.** Every new result needs two passing runs before it counts: on two
  machines, in two attempts, with a different test order, and with one built cold. Any failure
  blocks, and there is no automatic retry. A bug that fails with probability p is caught with
  probability 1 − (1 − p)², the same as today.
- **Cold audits.** After a commit lands on `main`, Velnor rebuilds it from nothing and re-runs its
  changed checks (audit A1), within 2 hours at P90 (target). A commit not audited after 6 hours
  pages the owner and holds releases. A nightly audit (A2) runs with the clock moved 8 and 30 days
  ahead, to catch date bugs.
- **Keys cover every input.** A key includes the whole source tree, toolchain, dependency versions,
  environment, flags, features, the binaries a test runs, data files, the machine's kernel and CPU,
  and which executor ran it. Every check runs on the full tree, because a trimmed tree was shown to
  turn a real failure green (measured, reproduced). Narrower keys (per folder) come only after a
  file-access audit proves what a check reads. Work that reads the network, the clock or random
  numbers runs fresh every time.
- **New shortcuts earn trust slowly.** A new kind of reuse runs in shadow, with every reuse
  re-executed, until about 3,000 clean comparisons. After that 2% are still re-run. The first
  disagreement switches that kind of reuse off at once.
- **Canaries for known false greens.** Permanent test PRs that must never be mergeable are checked
  every 10 minutes. If one becomes mergeable, Velnor stops writing `success` and pages the owner.
  Other canaries re-create past bugs every hour, such as a source file with an old timestamp that
  must still be rebuilt.
- **Conformance against the official runner.** A test corpus (at least 240 behaviour cases, 65 fault
  cases and 30 refusal cases, plus real velnor-new jobs) runs through both executors on both
  platforms. They must agree 100%. About 30 planted bugs ("mutants") test the comparison itself; at
  least 95% must be caught. It runs on every executor change, every night, and within 14 days of
  each new official runner release.
- **Zero tolerance for false greens; a budget for false reds.** A *false red* is a failure that
  passes on a rerun (a flake). Today 8.1% of full runs had one (measured). The budget is 3% at
  first and 1% at the goal. Flaky tests are counted and fixed, never hidden by retries.

## 5. The honest limits

### 5.1 What no runner can remove

- **GitHub's merge queue.** It took 19 s from enqueue to the group run and 26 s to merge (measured
  medians under load; not a proven floor). That is about 45 s per merge. Owner decision 7 keeps
  GitHub's queue, so these seconds stay.
- **The longest single test.** A test cannot be split across machines. Today one test takes 924.8 s,
  one suite 679 s, and behind two looped tests the floor is still 125–138 s (measured). Caching and
  spreading work across machines cannot shorten one test; only fixing the test can (a faster-clock
  machine might give 1.3–1.5×, estimate). The goal is no test over 60 s (stretch 15 s).
  Until then, a PR that touches those tests is as slow as they are.
- **Evidence that needs real traffic.** Some steps need real PRs to happen before they count: 3,000
  clean comparisons per kind of reuse, 300 PR heads and 100 merge groups of parity with today's CI,
  300 rerun pairs for a flake rate. At today's volume, 100 merge groups take about 44 hours
  (estimate). More agents cannot make these arrive faster.

### 5.2 The floor for this repository

At the goal, the typical PR's push-to-merge comes to about 85 s (estimate): about 35 s to the
verdict (start, checkout, a 15 s compile, 10 s of tests), about 25 s for the merge queue group, and
26 s for GitHub's merge. About half of that is GitHub's queue (measured), which Velnor cannot touch.
A PR that touches the slowest tests is bound by its longest test.

### 5.3 Why the targets are what they are

- The goal (verdict ≤ 90 s P50, ≤ 4 min P90; push-to-merge ≤ 3 min P50, ≤ 8 min P90) sits above the
  ≈ 35 s / ≈ 85 s estimate on purpose. P90 must absorb cache misses, cold builds, merge-queue
  ejections and a busy cluster.
- It needs three things first: keys per folder (after the file-access audit), compile reuse proven
  (3,000 warm-against-cold comparisons per Cargo root) and the slow tests fixed.
- Before that, the second run must build cold, so the first full version (day 5.2) is slower:
  about 3 minutes of cold build plus about 2.5 minutes for the longest test binary (estimate).
- The stretch (30 s / 100 s verdict) also needs keys per package and stamping the commit id after
  linking, so the product does not rebuild for every commit.
- No target counts as met until it is measured under real load.

## 6. What gets built, by whom, by when

Agents do all the building: 12 builders, 4 reviewers and 1 orchestrator, around the clock. The work
is about 820 agent-hours of building and 120 of review, about 2.8 days of fleet time (estimate). So
coding does not set the dates. CI and queue throughput, reviews, owner decisions and real-traffic
evidence do. "Day N" counts from the start; P90 dates are in brackets. Figures are estimates, except
the goal and stretch rows, which are targets.

| Day | Milestone (and what it deletes) | Verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR |
|---|---|---|---|---|
| Today | Baseline (measured, proxy) | 26.7 / 92.3 min | 85 / 457 min | 285–347 |
| 0 | Phase 0: safer ruleset, gate jobs off hosted machines, auto-merge. Deletes the `edited` trigger, admin bypass, fork job, `-j2` caps, macOS legs | ≤ 12 / ≤ 20 min | ≤ 35 / ≤ 60 min | ≤ 170 |
| 1.2 | First conformance runs on Docker, both executors | ≤ 11 / ≤ 18 min | ≤ 30 / ≤ 55 min | ≤ 160 |
| 2.1 (3) | Cohort 1 (gate jobs) on Velnor Native on Kubernetes | ≤ 9 / ≤ 16 min | ≤ 25 / ≤ 45 min | ≤ 150 |
| 3.2 (4.5) | Cohort 2 (the 26 crate jobs) | ≤ 6 / ≤ 10 min | ≤ 15 / ≤ 28 min | ≤ 120 |
| 3.8 (5.3) | Cohort 3 (Docker tests on private daemons) | ≤ 5 / ≤ 9 min | ≤ 13 / ≤ 25 min | ≤ 110 |
| 4.2 (6.5) | The App gate is required; the label flow is on | ≤ 5 / ≤ 9 min | ≤ 12 / ≤ 22 min | ≤ 110 |
| 4.8 (7) | Cohort 4 (releases) on Velnor | — | — | — |
| 5.2 (7.5) | PRs and groups verified by Velnor obligations; audit A1 replaces the full push run | ≈ 6 / 18 min | ≈ 9 / 30 min | ≈ 110 |
| 6.5 (9) | Final deletion of ARC and the old runner code (the official runner image is kept) | ≈ 4 / 12 min | ≈ 7 / 22 min | ≈ 90 |
| 7.5 (10) | Compile reuse proven on the helper and generator | ≈ 2.5 / 8 min | ≈ 5 / 15 min | ≈ 80 |
| 10 (13.5) | **Goal**: per-folder keys, slow tests fixed | **≤ 90 s / ≤ 4 min** | **≤ 3 / ≤ 8 min** | **≤ 50** |
| 16–18 | **Stretch**: per-package keys, `velnor verify`, longest test ≤ 15 s | 30 s / 100 s | 100 s / 4 min | ≤ 20 |

The verdict at day 5.2 is briefly slower than at day 4.2, because the second run builds cold until
compile reuse is proven. A milestone that misses its numbers holds the next one until the miss is
explained.

## 7. What is still open

The plan runs on each default until you answer.

- **D1** (hour 0): release the prepared engine version that moves gate jobs off hosted machines, and
  pre-approve CI-only engine releases (D9)? Recommended: yes.
- **D2** (hour 0): should agents turn on auto-merge and stop merging `main` into their branches?
  Recommended: yes.
- **D3** (hour 2): apply the first ruleset update now (both checks bound to GitHub Actions, empty
  bypass list, ALLGREEN, depth 2, 90-minute timeout)? Recommended: yes.
- **D4** (day 3): which expression evaluator ships: A, the old one (3 hours), or B, a port of
  GitHub's new one (about 28 hours)? Recommended: B. Default: A.
- **D7** (day 2): create the `velnor-verify` App and an agent identity that cannot change the
  ruleset? Recommended: yes.
- **D8** (hour 6): a deploy identity for `velnor-system`, and the node changes (XFS disks,
  reservations, taints, a storage drive per worker)? Recommended: yes.
- **D11** (day 1): accept the per-job-type cutover counts as written? Recommended: yes.
- **D14** (day 8): delete the engine crates once velnor-gen's workflow replaces the engine's CI?
  Recommended: only after checking for other users.
- **D15** (day 1): approve the recovery path for a bad Velnor release (redeploy the previous image;
  if none works, a pre-approved break-glass verifier), with no admin merge? Recommended: yes.
- **D16** (day 2): should a label added while the PR was a draft start CI when it becomes ready?
  Recommended: yes. Default: no.
- **D17** (day 3): who may approve removing a check or test from the list of required checks?
  Recommended: only you, through a signed entry. Default: nobody.

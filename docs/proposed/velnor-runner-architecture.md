# Velnor runner architecture

**Status:** Proposal. It replaces nothing yet. Building it needs the owner decisions in section 13,
because parts of it contradict `docs/goals.md` and `docs/hard-decisions.md`.

**Scope:** how Velnor verifies velnor-new's changes on the project's Hetzner k3s cluster so that
waiting for CI stops being the slowest part of delivering a change. The generator (`velnor-gen`) and
helper (`velnor-ci`) are in scope where the runner needs something from them.

**Sources.** Facts are cited from the [evidence pack](../research/velnor-runner-2026-11/evidence.md)
as §A T5 and similar, from the eight panel proposals under
`../research/velnor-runner-2026-11/proposals/`, and from the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea the panel raised with
its decision (ids `I001`–`I120`, `N01`–`N17`) and every hole (`F-H`) and alternative (`F-B`) the
design review found. Every number is tagged: **[M]** measured (with the source), **[E]** estimate
(with the arithmetic or basis), **[T]** a target. A target is **committed** when the evidence makes
us confident, **stretch** when an experiment must prove it first.

## 1. Goal and success metric

### 1.1 The metric

The single metric is **push-to-merge**: the time from the final commit's accepted push to a pull
request's head until the pull request is merged, measured from the pull request timeline. We report
P50 and P90 per week over every merged pull request, next to PR verdict time (push to the `Required`
check), the admission span (run created → first job), runner-minutes per merged pull request, the
compile-cache hit rate and the false-failure rate.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M] 98 merged PRs, `proposals/opus-5.5.md` §1.1 (clock from the first CI run on the final head) |
| Push-to-merge on the worst measured day, P50 | 400 min, of which 278–282 min between queue entry and merge | [M] `proposals/opus-5.5.md`, `proposals/fable-5.2.md` |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M] §A T2 |
| PR run with no hosted queue | about 11 min | [M] 10.9 min over 12 runs (`proposals/fable-5.2.md`); replays 11.4 and 12.0 min |
| Admission span, PR runs: run created → first job | 2 s p50, 226 s p90, 2,203 s max; a run that supersedes a running one waits 60 s p50, 626 s p90 | [M] 200 sampled runs; every delay over 10 s had an unfinished earlier run on the branch (design review F-H06) |
| PR runs started on an unchanged head SHA | 18.3%, most of them cancelling an identical run in flight | [M] 469 of 2,558, the `edited` trigger (F-H07) |
| Runner-minutes per run (PR / merge group / push) | 52.9 / 64.6 / 62.0 | [M] §A T2 |
| Runner-minutes per merged PR | 285–347 | [E] about 3–4.8 PR runs plus a merge group and a push per merge |
| Full runs per merge | 3 (PR, merge group, push on the same SHA) | [M] §A T7: 120 of 159 green merge groups had a push run on their SHA |
| False failures per full run | 8.1% | [M] 10 of 124 push runs failed on a SHA whose merge group had passed, all in test steps (decision log, (h)) |
| Cross-job compile reuse on the cluster | 0 | [M] sources cache 0 of 48, toolchain 0 of 58, state 0 of 3,636 (§A T9a, T9b) |

### 1.2 Same confidence: the contract

"Same confidence" is a contract with seven clauses. A change to this design that breaks a clause is
rejected, however much time it saves.

1. **C1. Every tip of `main` is verified.** For every commit that becomes the tip of `main` and
   every obligation the plan lists for it, there is a passing result whose key equals the
   obligation's key at that commit, produced by Velnor's trusted execution in the sandbox class the
   obligation requires. An *obligation* is one unit of verification: format, lint, docs, a per-crate
   check, one test binary under one configuration, or a named check. Under the merge queue's
   HEADGREEN grouping a group's earlier commits land as ancestors of a verified tip without a
   passing result of their own; a failed entry has landed that way inside a passing group. Those
   commits are marked for bisection and release tooling, and owner question 3 offers ALLGREEN
   instead.
2. **C2. Keys capture every input, by construction.** A key is the hash of what the obligation's
   sandbox contains: the Git tree hashes of its inputs, and its definition (workflow, helper
   version, toolchain, runner image, sandbox class). The sandbox is a sparse checkout of exactly
   those paths, so an input that is not in the key is not on disk, and a lookup of it fails (section
   4.1).
3. **C3. Selection is sound or wide.** The plan comes from a released helper, never from the change
   under test (`docs/goals.md` rule 12). A file the plan cannot place selects everything. Nothing is
   skipped relative to a merge group's predecessor in the queue; an obligation is skipped only when
   a receipt with its exact key exists.
4. **C4. Reusable results are observed by Velnor.** A result that a run of a *different* tree may
   reuse is recorded by the node agent from test processes it started itself. A job's own report may
   back an exact-tree receipt only, because only runs of that identical tree reuse it, and that code
   can already report its own result today. A job never signs a receipt.
5. **C5. Uncomparable work always runs, and the sandbox hides what a key does not hold.**
   Obligations that read the network, the event payload, the clock in an assertion, randomness
   without a fixed seed, Docker images not pinned by digest, or the commit's identity, or that
   publish anything, run fresh every time. Build and test sandboxes have no `.git` directory and a
   scrubbed environment (no `GITHUB_*` variables, a fixed `HOME`, `PATH` and locale), and the CPU
   count of each sandbox class is fixed and part of the definition digest. An obligation that reads
   Git objects, as `velnor-runner-host`'s build script does with `.velnor/runner.lock`
   (`build.rs:69-80`), stays in this clause until it reads a declared file instead.
6. **C6. Reuse is audited, compile reuse included.** A random sample of reused results is executed
   again (2% of reuse by key in its first month). The nightly and every-20th-merge full runs build
   from an empty target in a fresh pod and compare their results and test-binary digests with the
   warm path. A cache lineage restarts from empty on every lockfile or toolchain change and at least
   daily. One disagreement disables that kind of reuse, or retires that lineage, until it is
   explained.
7. **C7. Flakes are budgeted** (section 1.3).

Where a repeat run carries information, and where it does not: for an obligation whose inputs the
change did not touch, a second execution says something only about the test's own nondeterminism,
which C6 and C7 collect deliberately. For an obligation whose inputs the change touched, a repeat
does say something about the change: a regression that fails with probability p per execution is
caught by three executions with 1 − (1 − p)³ (49% at p = 0.2, 14% at p = 0.05) and by one with p.
Today's three full runs give that by accident. Velnor gives it on purpose: **every obligation whose
inputs the change touched executes three times in parallel, and any failure blocks.** Those are the
only tests that run, so the cost is small [E: `velnor-ci`'s step is 10 s median, §A T10].

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes. It costs a
rerun or a merge-queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** at most 1% false failures per full required run from phase 3 (committed), at most 3%
  from phase 1. Today 8.1% [M].
- **Measure:** a paired-rerun sampler re-executes 5% of green merge-group SHAs on idle capacity and
  counts disagreements. It replaces the push run as the denominator once the push run stops
  re-verifying.
- **Report:** nextest marks a test that passed only on retry as flaky; retries stay at 0 in the
  required profile, so a flaky test fails the run and is counted.
- **Act:** a test with two false failures in seven days gets an owner-labelled issue. It is fixed,
  or quarantined with the owner's sign-off. A quarantined test still runs in a visible non-blocking
  lane, so no verification disappears silently.
- **Known causes to remove first:** CPU starvation of tests with tight timeouts, such as the 2 s
  kill in `ci-helper/.config/nextest.toml`, which runs under pods that request 2 CPU and burst to 84
  cores (§B.2.2); and the `event.json` rewrite race between steps (decision log I023).

### 1.4 Targets

Committed figures with the stretch in brackets, P50 / P90, following the design review's consensus.
A phase's figures hold only if its conditions do. CPU per merged PR charges everything: completed,
cancelled and superseded runs, and batch work.

| Phase | PR verdict | Push-to-merge | CPU per merged PR | Conditions |
|---|---|---|---|---|
| 0 | ≤ 12 / ≤ 20 min | ≤ 25 / ≤ 60 min (18 / 40) | baseline re-measured | the regenerated workflow is live; admission span p90 ≤ 5 s |
| 1 | ≤ 5 / ≤ 10 min (3.5 / 8) | ≤ 12 / ≤ 30 min (10 / 22) | ≤ 150 runner-min | selection fixed in the engine (about 212 without it, 139 with it [E]); `Journal::open` on a template ≤ 1 s |
| 2 | ≤ 2 / ≤ 6 min (80 s / 4 min) | ≤ 5 / ≤ 15 min (3.5 / 9) | ≤ 60 | per-root keys, with ≥ 65% of merge groups executing nothing; the cache engine chosen by its experiment; no test over 60 s measured alone |
| 3 | ≤ 90 s / ≤ 4 min (60 s / 2.5 min) | ≤ 3 / ≤ 8 min (1.75 / 5) | ≤ 35 (15) | the week-1 experiments passed: webhook arrival under 8 s, closure-only checkout for ≥ 80% of packages, queue-tip match ≥ 70%; ≥ 85% of merge groups execute nothing; ≤ 0.5% of merges ejected by a false failure; demand-to-start p90 ≤ 2 s for merge and gate work |
| 4 | (30 s / 100 s); ≤ 20 s after a preflighted push | (100 s / 4 min); about 50–75 s if Velnor lands the merge | (≤ 15) | stretch only |

If phase 3's experiments fail, its own design still supports about 90 s / 4 min for the verdict and
4 / 10 min push-to-merge. Further committed figures: first cargo command ≤ 15 s after the job starts
(phase 2) and ≤ 5 s after the push (phase 3, stretch ≤ 4 s); `Journal::open` ≤ 1 s (phase 1), no
test over 60 s (phase 2, stretch 15 s); false failures ≤ 3% per run (phase 1) and ≤ 1% (phase 3).

## 2. Where the time goes, and what changes the problem

Four facts set the design [M unless marked]:

1. **Waiting, not working.** Without hosted queues a PR run takes about 11 min, but the median PR
   waits 85 min from push to merge. On the worst day the three serial gate jobs on GitHub-hosted
   runners waited 57.0 of a 71.8 min run (§A T5), on an organisation with the Free plan's 20
   concurrent hosted jobs shared with two busy non-CI workflows (§A T11). A push that supersedes a
   running PR run waits until the cancelled run's `Required` job, which runs on `always()`, has been
   scheduled and finished: 60 s p50, 626 s p90 and up to 2,203 s. The merge queue builds 2 entries
   at a time and no agent enables auto-merge (0 of 40 recent merged PRs [M, decision log]).
2. **The same verification three times.** Each merge runs the full job set on the PR, on the merge
   group, and again on the push of the same SHA (§A T7). The third run found nothing and failed
   falsely 8.1% of the time.
3. **Every job is cold.** No compiled output, toolchain or crate source survives a job on the
   cluster (§A T9a, T9b). The cache restore that exists costs more than the build it saves: a 1.92
   GB entry took 44–61 s from GitHub's cache (decision log I036).
4. **Code sets the floor.** Two tests loop over 40 and 6 cases inside one test function and take
   258–274 s and 203–271 s; `maintenance-helpers` compiles with `build.jobs=2`; selection forces
   every crate with a build script (decision log I059, I061, I068).

The design attacks each fact by removing work, not by speeding it up. Gates move off the hosted
pool. Verification leaves GitHub jobs altogether: the push webhook starts it on the cluster, and one
witness job reports it. A verification ledger with input-addressed keys makes "verify identical
inputs once" structural, so a merge group stays a full verification and is mostly a lookup. Caches
live on the node that runs the work. The slow tests are rewritten. Rust components are built where
owning the mechanism removes work: the node agent and its executor, the ledger, the stores and the
webhook ingress. Owning job acquisition is kept as an experiment, because an idle runner already
starts the next job in 2 s (section 3.5).

## 3. Architecture

### 3.1 What stays compatible and what is redesigned

GitHub remains the platform. From the outside Velnor is a GitHub App plus GitHub Actions runners:

- **Unchanged:** workflow YAML and its semantics, the required checks, rulesets and the merge queue,
  `GITHUB_TOKEN` and OIDC, and the runners that execute workflow jobs (ARC scale sets running the
  official runner, within GitHub's 63-day version window, §F.6.1).
- **Redesigned inside:** when verification starts (at the push, not when GitHub schedules a job),
  where it runs (Velnor sandboxes, not job containers), what is already on that disk, what work is
  skipped because an identical verified result exists, and how the verdict reaches the job GitHub
  waits for.

### 3.2 Components

All new code is Rust in the root workspace. Each crate has a line budget; crossing it stops work for
a review (decision log I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| Webhook ingress | `velnor-hooks` | one service per site | Receives the App's push, pull-request and merge-group webhooks, verifies their signature, calls `ensure`, replays missed deliveries | 3k lines |
| Ledger | `velnor-ledger` | library plus one service per site | `ensure` requests, obligation keys, signed receipts, scopes, promotion, audit sampling | 6k lines |
| Node agent | `velnor-node` | privileged DaemonSet on workers | The obligation executor behind `ensure`: pre-created sandboxes, result observation, node stores, the `main` builder, harvest, cleanup by UID | 15k lines |
| Store | `velnor-store` | library plus one service per site | Content-addressed blobs (test binaries, archives, reports, snapshot seeds), replication within a site, eviction | 6k lines |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, obligation commands, the witness step `velnor-ci witness`, the workflow shape | existing |
| Experiment-gated | `velnor-proto`, `velnor-dispatch`, `velnor-sandbox`, `velnor-exec` | not built | Owned job acquisition, a hot Worker, podless job completion and a Worker replacement; built only if the witness leaves more than 5 s per run (section 3.5) | 35.5k lines |

### 3.3 The verification path: `ensure` and the witness

1. **Push.** A push to a pull request reaches `velnor-hooks` as a webhook 1–2 s later [M: GitHub
   creates push-event runs within 1–2 s, `fable-5.2` design review F1]. A merge group's creation
   arrives the same way.
2. **Plan.** `velnor-hooks` calls `ensure(tree, plan)` on the site's ledger. The ledger fetches the
   commit into the node mirror, computes the tree to verify with `git merge-tree` (the pull
   request's head merged into its current base, or the merge group's own commit), and runs the
   released planner, which lists the obligations and their keys.
3. **Lookup.** For each obligation the ledger finds a passing receipt (done), an execution in flight
   (join it) or nothing (schedule it).
4. **Execute.** The node agent that placement picks (section 3.4) runs each scheduled obligation in
   a sandbox it creates: a pre-created single-use pod of the obligation's class, with no `.git`, a
   scrubbed environment, no GitHub token and the target view from the cache engine (section 5). The
   agent starts the processes, observes their results itself, and writes a receipt per obligation as
   it finishes.
5. **Witness.** The workflow has one job, named `Required`, which GitHub schedules as usual on a
   small gate-class runner. Its single step, `velnor-ci witness`, calls the same `ensure`, joins the
   work already in flight or starts it if the webhook was lost, streams the obligations' logs into
   the job log, and exits with the verdict: success only when every obligation of its plan holds.
6. **Who executes what.** The official `Runner.Worker` executes the witness job, so
   `docs/goals.md:16` holds literally. Obligations never run inside a GitHub job.
7. **Failure.** A late or lost webhook only delays: the witness's own `ensure` starts the work. A
   merge tree that differs from GitHub's is a key miss, never a wrong verdict. A superseded push
   cancels its tree's scheduled work; work that another request needs keeps running.

The same `ensure` has two more callers: the merge-group webhook, and, as a stretch, the agent's
`velnor verify` before it pushes (moonshot M8). For the median pull request the path costs [E,
`fable-5.2` design review T1]: webhook 2 s + merge tree and plan 2 s + delta build 15 s + lanes 10 s
= 29 s of verification, while GitHub creates and starts the witness in about 15 s; the verdict lands
about 35 s after the push.

### 3.4 Placement

Velnor places obligations, not jobs, so GitHub's choice of runner decides only where the witness
prints its logs:

- **Every node is warm for `main`** (phase 2). A builder publishes `main`'s outputs after every
  merge (section 5), so an obligation on any node starts from `main`. Placement no longer decides
  whether work is warm; it decides only the smaller things below.
- **Per-obligation placement** (phase 3) minimises predicted finish time: sandbox availability +
  view set-up + predicted rebuild on that node + predicted tests + a site penalty when the request's
  other obligations and artifacts are in the other site. An idle cold node beats a busy warm one.
- **Request stickiness.** All obligations of one request stay in one site; DE to FI is 35 ms over
  WireGuard (§B.5).
- **Class routing** for workflow jobs works through labels: `velnor-gate`, `velnor-build`,
  `velnor-docker`, each its own ARC scale set with warm pods.

### 3.5 Executors

- **Obligations run on Velnor's own executor.** Generated obligations are plain commands (`cargo`,
  `cargo clippy`, `cargo doc`, `cargo nextest`), so the agent runs them directly in its sandboxes
  through `velnor-ci`; no step semantics are reimplemented. This is the native executor the panel
  asked for, limited to the work Velnor generates.
- **GitHub's `Runner.Worker` runs every workflow job**, unmodified: the witness, release jobs, and
  every job of other repositories and organisations. It keeps every workflow construct working and
  is the oracle the executor is compared against.
- **Experiment-gated behind the witness.** An idle ARC runner already starts the next job 2 s after
  the previous one ends, 3 s at p90 [M, `fable-5.2` design review F4], so owning acquisition buys at
  most a few seconds per run. The dispatcher that holds runner identities, the hot Worker, podless
  completion of jobs from receipts, and a Worker replacement for arbitrary jobs are each built only
  if the witness design leaves more than 5 s per run that they remove (decision log I003, N05, N06,
  I005). If built, the dispatcher keeps the official Listener's first-renewal rule and force-fail
  path (`Runner.Listener/JobDispatcher.cs:393-405,1208-1219`), and the Worker reads only `.runner`
  and the job message (`Runner.Worker/JobRunner.cs:47`).

### 3.6 Isolation and trust

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | the witness and other small jobs | ARC pod, 1 CPU | GitHub API | read-only tools |
| build | same-repository PR, merge group and `main` obligations | agent-created pod: no Docker, seccomp `RuntimeDefault`, all capabilities dropped, no service-account token, no GitHub token, no `.git`, scrubbed environment | allow-list: crates mirror, site store | read-only trusted; per-obligation views; never a shared writable path |
| docker | only the test binaries that need a Docker daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), run as their own obligation beside the build that produced them: placement puts that build, in the build class, on a docker node | pod with a privileged dind sidecar, on two tainted nodes (one per site) | as build, plus a registry mirror | the build's test binaries, cloned in about 1 s |
| builder | `main` builds, no GitHub job | unprivileged pod | crates mirror | writes the trusted scope through the agent only |
| fork | fork pull requests | GitHub-hosted runners until a microVM class exists | — | none |

Rules:

- An obligation never writes a store another obligation reads. The node agent takes what it produced
  after the build phase and decides where it may go (section 5.6).
- Pull-request code now runs outside any GitHub job, so it holds no GitHub token at all.
- `ensure` acts only on same-repository heads; a fork's tree never reaches a Velnor sandbox.
- The required checks are bound to one integration in the ruleset. Today the ruleset names `Required
  verdict` and `Required` with no integration, so any app, or a workflow token granted `statuses:
  write`, can post them (N12).

### 3.7 Failure handling and recovery

- **Journal discipline:** persist the intent, make the call, record the outcome. Unknown is never
  treated as absent. This is velnor-new's rule; the code is new and small.
- **`ensure` is idempotent.** A repeated request returns the same receipts or joins the same
  executions. A request that finds the ledger unreachable runs its obligations fresh.
- **Node agent restart.** It reconciles sandboxes by UID; an execution whose sandbox is gone is
  scheduled again and never reported twice.
- **Webhook gaps.** `velnor-hooks` replays missed deliveries from the App's delivery list every
  minute; the witness's own `ensure` covers whatever remains.
- **Break-glass.** The hosted workflow stays as a permanent fallback, exercised weekly, and the
  cluster runs only released agent versions. Moving velnor-new back is a label change in the
  generator config.

### 3.8 What is taken, and what is not copied

| Source | Taken | Not copied |
|---|---|---|
| velnor-new runner | The journal discipline, UID-based absence proofs, the job-authority policy, velnor-local as the conformance harness; for the gated dispatcher, the wire types (`velnor-runner-github`) and lifecycle types (`velnor-runner-core`) | The 157k-line `velnor-runner-host` and its Docker launch path, which keeps serving the Mac host. Putting runner credentials inside a sandbox |
| velnor-legacy | As reference code with tests: the trust-scope model, the per-slot lesson of its mbx store, and its broker and run-service client for the gated dispatcher; its incidents as regression tests (rerun log ownership, artifact scoping, silent supervisors, idle CPU, a poisoned persistent store); its expression engine as a conformance corpus | The 37k-line executor and runner files, the Docker socket proxy, BuildKit handling, the slot supervisor fleet, Firecracker until phase 5 |
| actions/runner | The protocol as ground truth; `Runner.Worker` for every workflow job; the action archive cache (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, `ActionManager.cs:1232`) | One pod per obligation run inside a job; self-update inside sandboxes |
| ARC | The scale sets that run the witness and every other job, with warm pods per class | Cold pods with `emptyDir` state, a dind sidecar in every pod, cache-blind placement of verification work |

## 4. Verification ledger

The ledger is the mechanism that removes duplicate work while keeping C1–C7.

### 4.1 Obligations and input-addressed keys

- The plan (`velnor-ci plan`, from a released version) lists every obligation of the tree, on every
  event. Each has a **definition digest**: what runs and how (command, configuration and features,
  profile, nextest profile and overrides, helper version, toolchain, runner image, sandbox class and
  its CPU count).
- **The key is the hash of what the sandbox contains.** Per Cargo root in phase 2: the Git tree
  hashes of the root, of the roots it reaches (only `ci-helper` reaches outside its own root, into
  five `generator` crates), of the shared files (toolchain pin, `mise.*`, `.github/`, `.velnor/`),
  and any declared extra inputs, plus the definition digest. Per package in phase 3: the same over
  the package's dependency closure from cargo's package graph. The hashes come from the commit
  itself, in under 5 ms, with no build.
- **The sandbox is a sparse checkout of exactly the key's paths.** An input the key does not hold is
  not on disk, so reading it fails instead of passing on stale data, negative lookups included.
  Every failure in a closure-only checkout names an undeclared input; it is declared or the key
  widens.
- **The exact tree is the degenerate key**, used when no narrower key is proven. Test-binary digests
  are recorded with every receipt and audit the key; they are not part of it, so no bit-identical
  build is needed.
- **Package keys exist before the root workspaces merge.** The planned move to one root workspace
  removes the root boundaries that phase-2 keys use.

### 4.2 Receipts, joins and scopes

- **One receipt per obligation, written as it finishes.** A receipt binds a key to a result, the
  request that produced it, the sandbox class and node, the expected test inventory it ran, and the
  observing agent's signature. A receipt expires after 7 days, which bounds how long a
  time-dependent failure can hide.
- **In-flight join.** A request whose key is already executing waits for that execution instead of
  starting a second one. A cancelled request drops its claim; the execution continues while another
  request needs it. A partial result satisfies nothing.
- **Why reuse across trees is safe here.** Everything that could forge or change a root's result is
  inside its key: the root's files, `.github/`, the pinned helper and the toolchain. The ledger
  accepts a receipt only for an obligation the released planner assigned to that execution.

| Scope | Written by | Reusable by |
|---|---|---|
| key receipts | the agent, for an execution built from a `trusted` generation plus its own delta | any request whose obligation has the same key, except forks |
| `trusted` | receipts of commits on `main`, and the `main` builder's outputs | every request of the organisation except forks |
| fork | nothing | nothing |

A build that can mint a reusable receipt reads the `trusted` cache plus its own outputs only; a
stale build-script output left by an earlier push could otherwise let a tree pass that no clean
build accepts. That costs the request's delta build, 10–20 s for the generator and 45–80 s for
`velnor-runner-host` [E], and it retires the separate PR-scope cache.

### 4.3 What a request executes

- **Every event lists every obligation.** A merge group stays a full verification, as hard decision
  3 requires; most of its obligations are satisfied by a receipt with the same key. An obligation
  with no such receipt runs. A missing report or a test step that ran zero tests fails.
- **"Executes nothing" holds only when every obligation of the consuming plan has a receipt.** A PR
  run's receipts cover what its tree's keys cover; for a generator-only PR, the other roots' keys
  equal `main`'s, so their receipts come from `main`.

| Situation | Phase | What runs | Status |
|---|---|---|---|
| Push to `main` whose SHA passed its merge group | 1 | release and publication duties only | committed |
| A root whose key equals a receipted key (any event) | 2 | nothing for that root | committed |
| A package whose closure key equals a receipted key | 3 | nothing for that package | committed after its experiment |
| Re-run of a failed job | 2 | only the obligations without a passing receipt | committed |
| Test-binary digest closures finer than packages | 4 | nothing for that test binary | stretch, moonshot M4 |
| Any obligation in C5 | all | always runs | — |

Replayed over 100 real merges, the root-jobs merge groups would execute fall from 500 (full) to 425
with exact-tree receipts and 125 with per-root keys [E, `opus-5.5` design review T3].

**Enforcement for finer closures.** Below the package level a test binary is eligible only when it
passes inside a mount namespace that contains its binary, its declared inputs, the toolchain runtime
and a private `/tmp`, with no network. Landlock is not used, because it cannot restrict `stat` or
`access`. The agent's executor starts each test inside that namespace, and C4 wants the agent to
observe the result anyway.

### 4.4 Trust root and audit

- **Keys.** Each node's signing key lives outside every pod, in a signer the agent calls; no sandbox
  can read it. Receipts go to an append-only log per site. The witness verifies each receipt's
  signature and its log entry before it passes.
- **Degraded mode.** If the ledger cannot be reached, nothing is satisfied by lookup: obligations
  run fresh, or, if the executor is down too, the per-job workflow takes over by a configuration
  switch. A missing receipt is slow, never wrong.
- **Audit.** The audit sampler (C6), the cold differential builds of the nightly and
  every-20th-merge full runs, the test-binary digests recorded with each receipt, and the file-read
  audit (phase 3, as telemetry) each produce a non-blocking check. A disagreement turns that reuse
  class off for the organisation, or retires that cache lineage, and opens an issue; it never turns
  a red result green.

## 5. Cache system

### 5.1 The engine is decided by experiment

Three candidates compete for the compile cache. **mbx stays unless it measurably loses.**

| Candidate | How it works | What it promises | What threatens it |
|---|---|---|---|
| (a) mbx | A node-persistent mbx store on each worker plus an `mbx-cache` server (jdx/mr-boxington-cache) per site. OIDC rules let pull requests read and only `main` pushes write; the anonymous read-only namespace is the fallback where build jobs have no `id-token` permission | Per-invocation reuse across checkouts, paths and nodes; hard-linked outputs where no reflink exists; the site server shares `main`'s outputs natively | Units it does not consult; growth of never-reused entries |
| (b) cargo + reflinks | Plain cargo over a frozen target snapshot cloned per job with reflinks on XFS, plus `velnor-ci inputs restore` | Cargo's own fingerprints decide freshness; no wrapper | Per-file clone time on 6–9 GB targets; the page cache is not shared between clones |
| (c) cargo + overlayfs | Plain cargo over an overlayfs mount whose read-only lower layers are frozen generations | Constant-time set-up; the lower layers' page cache is shared; the upper layer is the job's delta | Overlay copy-up of large files; kernel and container-runtime limits |

Every candidate keeps the same rules: one fixed checkout path for every job (the Mac measurement
below rebuilt the workspace crates whenever the path changed); pull requests never write the shared
scope; C6's cold differential build audits it; a lineage restarts on every lockfile or toolchain
change and daily; and its disk has a budget with eviction. `CARGO_INCREMENTAL` stays `0` in required
builds until its own experiment shows a safe gain.

### 5.2 What the Mac measurement showed

[M] The owner's machine, `ci-helper` with its `generator` path dependencies, six fresh checkouts at
different paths, mbx 1.29.2 against plain cargo with a cloned or copied target:

| Measure | mbx | Plain cargo |
|---|---|---|
| Disk per extra checkout, APFS (clones available) | 473 MiB | 457 MiB with a clone |
| Disk per extra checkout, HFS+ (no clones, like ext4) | 835 MiB | 1,411 MiB with a full copy: mbx 41% smaller, through hard links |
| Crates rebuilt per extra checkout at a new path | the same 6 workspace crates | the same 6 workspace crates |
| A second node pulling from the `mbx-cache` server | 164 hits, 6 misses, 503.7 MiB downloaded | — |
| Never-reused workspace outputs stored per checkout | about 230 MiB | — |

Three lessons hold whichever candidate wins: build at one fixed path; give the store a disk budget
(the owner's own mbx store has grown to about 485 GiB); and on the XFS data volume, where reflinks
exist, (a) and (b) cost about the same disk, while on ext4 (a) costs 41% less. One workspace and one
commit do not settle wall time, and a lockfile change was not measured.

### 5.3 Cluster measurement

**Pending.** The same comparison runs in a throwaway namespace on velnor-190 and velnor-5, with
fixed checkout paths, a lockfile change and a page-cache check. It decides:

- wall time and CPU-seconds of candidates (a), (b) and (c) on an unchanged tree, a one-file edit, a
  lockfile change and a second node;
- disk per checkout and page cache with four concurrent jobs;
- set-up and teardown time per job, by file count;
- whether a second node warms from its site in time to matter.

The winner is the candidate that loses on none of these by more than its measurement noise; with no
clear loser, mbx stays. This subsection records the numbers and the decision.

### 5.4 Tiers

| Tier | Where | Holds | Access |
|---|---|---|---|
| L0 | the sandbox's directory on the node's XFS volume | the obligation's own view and outputs | read-write by the sandbox |
| L1 | node store on the same XFS volume | toolchains, tools, crate sources, Git mirror, action archives, the compile cache | read-only mounts and views made by the agent |
| L2 | one store, ledger and (for candidate (a)) `mbx-cache` service per site | receipts, test binaries and archives, reports, compile outputs of `main` | through the agent |
| L3 | GitHub's cache | only for workflows running on GitHub-hosted runners | as today, within the 10 GB budget |

Nothing on the verification path reads across sites synchronously. Sites replicate receipts and
seeds in the background.

### 5.5 What is cached where

| Item | Tier | Key | Writer | Expected hit rate | Basis |
|---|---|---|---|---|---|
| Rust toolchain and components | L1, read-only | toolchain file digest | node provisioner | ≈ 100% | [M] 0 of 58 restored today because it installs per job into `runner.temp` (`velnor-runners/state/RESULT.md:115`) |
| mise tools | L1, read-only | tool pins | node provisioner | ≈ 100% | [M] tool setup 10 s median, 18 s p90 per job today (§A T9b) |
| Crate sources | L1, read-only; crates mirror on miss | `Cargo.lock` checksums | builder; mirror | ≥ 99% of crates | [M] 10.5% of PRs touch a lockfile (`proposals/fable-5.2.md`); mirror fetch 2.7–3.3 s (§B.4.2) |
| Git objects | L1 mirror | — | agent on push webhooks | ≈ 100% except objects pushed seconds earlier | the ledger fetches the pushed commit before planning |
| Action archives | L1, read-only | `owner_repo/<sha>.tar.gz` | provisioner | 100% for pinned actions | [M] about 4.4 s per job today (`proposals/opus-5.5.md`) |
| Compiled outputs of `main` | L1 per node, L2 per site for candidate (a) | the engine's own key | the `main` builder | ≥ 95% of units fresh on PRs without a lockfile or toolchain change | [E] decided by section 5.3 |
| Test binaries and archives | L2 | content digest | agent | — | [M] velnor-new's nextest archive is 1.48 GB; GitHub upload took 36–43 s (I045) |
| Receipts | L2 ledger | obligation key | agents | exact tree: same-SHA push 100%; merge groups 0–18% today [M] and higher as verdicts shorten [E] | [M] decision log, (e) |

### 5.6 Trust and promotion

- **Only the `main` builder writes the trusted scope.** Pull-request builds write no shared scope;
  with candidate (a), pull requests read the site server and only `main` pushes write it.
- **Snapshots and generations are immutable.** Requests get views; nothing writes a frozen
  generation.
- **Post-build harvest.** The agent takes a sandbox's outputs when the build phase ends and before
  any test runs, so a test that writes into `target/` cannot poison a later request.
- **Promotion.** A merge group never writes the trusted scope, because a merge group may never land.
  Its receipts become trusted when its exact SHA is on `main`.
- **Cold differential.** The nightly and every-20th-merge runs build from empty in a fresh pod and
  compare with the warm path (C6); cargo cannot see a file it did not read in the last build, so
  only a cold build finds that class of staleness.
- **Forks** read nothing and write nothing on the cluster.
- **The GitHub cache path** stays correct for hosted runners: cache paths become fixed absolute
  paths, because today `${{ runner.temp }}` differs between hosted and ARC runners and the cache
  lookup filters on a version derived from the path, which is why sources missed 48 of 48 times
  (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`, `.github/workflows/ci.yml:337`).

### 5.7 Eviction and budgets

- Per node and organisation: the last three generations of `main` outputs per root, then
  least-recently-used.
- Budget: 40% of the node's data volume across all organisations, alert at 70% of the volume. The FI
  workers have one 3.5 TB data disk each (§B.1.4), the DE workers 32–50 TB.
- Sizes: a runner-workspace target is 5.9–7.1 GB and the root one 9.0 GB [M,
  `proposals/fable-5.2.md`]; a velnor-new job grows the disk by 14.7 GB [M, decision log I036].
- **Bumps.** An image, toolchain or helper bump changes every definition digest. The builder builds
  the next generation and seeds its receipts before the bump lands, so the hour after it is not cold
  for everyone.

## 6. Workflow design

### 6.1 Shape

- Phase 2: `velnor-gen` writes the workflow (decision log I056): one plan job, one job per Cargo
  root, and one per configuration where it differs (`velnor-runner-host` with `local-mode`). Inside
  a root job: build test binaries once per configuration, then run format, lint, documentation, the
  "check alone" steps and tests as parallel lanes. The generated workflow runs these steps one after
  another today; the Worker already supports background steps.
- Phase 3: the workflow is one witness job named `Required` plus the non-blocking `Cache` check;
  every obligation runs through `ensure` (section 3.3).
- Until the owner changes the ruleset, the workflow emits both required names.

### 6.2 Change-aware selection that keeps confidence

- **Phase 1, in the current engine:** declare the inputs of the three crate-root build scripts and
  the test reads the plan already names. Today `undeclared_reads` is true for every crate with a
  build script (`crates/velnor-actions-rust/src/tasks.rs:254`), which is why the heavy jobs ran in
  97.4% of PR runs. Merge groups stay full under hard decision 3, so a missed input shows up as a
  merge-group failure, never as a false green.
- From phase 2, a pull request's plan follows `docs/goals.md` design rule 2: the Cargo package graph
  plus declared inputs, with unknown files selecting everything. Skipped obligations are not lost:
  every merge group lists all of them.
- **No selection relative to the queue.** A merge group diffed against the entry ahead of it would
  never select a failed predecessor's change, and under HEADGREEN that change lands with the group
  (`ci-helper/crates/velnor-ci/src/event.rs:42-45`). Merge groups therefore stay full and are
  satisfied by key lookup (section 4.3), which keeps hard decision 3 literally.
- A scheduled full run (nightly) and every 20th merge execute everything, built cold (C6).

### 6.3 Tests

- **The unit of cost is `Journal::open`,** not two loops: it has 93 call sites in
  `velnor-runner-host`, and every journal-opening test pays it. The phase-1 gate is that
  `Journal::open` on a pre-migrated template takes at most 1 s, measured, and that no test exceeds
  10 CPU-seconds measured alone outside a recorded, shrinking list. Splitting the 40-case and 6-case
  loops into one test per case comes with it.
- **A test-time budget** check enforces that list: non-blocking in phase 1, blocking for new tests
  from phase 2.
- **The compile floor.** `velnor-runner-host` splits into five or six crates along its module
  boundaries, with one test binary per package instead of one per file, so a leaf edit recompiles
  one part instead of 157k lines; a one-file edit cost 61 s and 159 CPU-s on a Mac [M, `opus-5.5`
  design review]. Keeping incremental state in the cache is an experiment: 10.9 s instead of 61 s on
  a Mac [M, `fable-5.2` design review], at the price of non-identical binaries, which input keys
  tolerate.
- **Changed tests run three times** in parallel (section 1.2).
- **Shard only by arithmetic.** A suite is split only when its summed test time divided by the
  slot's threads exceeds its longest test plus the cost of moving binaries (I067).
- **Test granularity** (phase 4, stretch): the agent's executor runs a request's test cases across
  sandboxes on several nodes of the site, longest first, and checks that every expected case
  reported exactly once.

### 6.4 No double or triple runs

| Event | Today | Phase 1 | Phase 2 | Phase 3 |
|---|---|---|---|---|
| PR push | full selected set | same | selected set; roots with a receipted key skipped | `ensure` on the push; packages with a receipted key skipped |
| Merge group | everything | everything | every obligation listed; roots with a receipted key satisfied by lookup | `ensure` on the group; packages satisfied by lookup; queue-tip candidates pre-verified (M5) |
| Push to `main` | everything again | release duties only when the SHA passed its merge group | same | same |
| Direct push to `main` (no queue) | everything | everything | everything | everything |

### 6.5 Merge queue

| Setting | Today | Phase 0 | Phase 3 |
|---|---|---|---|
| `max_entries_to_build` | 2 | 4 | 8, once false failures are ≤ 2% per run |
| Grouping, `max_entries_to_merge`, `min_entries_to_merge` | HEADGREEN, 10, 1 | same | owner question 3 (ALLGREEN) |
| `min_entries_to_merge_wait_minutes` | 5 | unchanged: it never applies when the minimum is 1 [M: enqueue to merge-group run 19 s] | same |
| `check_response_timeout_minutes` | 90 | 90 | 20 |
| Auto-merge | 0 of 40 PRs | agents enable it when they open a PR | same |
| Catch-up merges of `main` into PR branches | common | stopped | stopped |

HEADGREEN lands every entry of a group on the strength of the group tip's checks. In the 15 days of
evidence one failed queue entry landed that way: its own merge-group run failed and the next entry's
passed. C1 is stated for tips; the earlier commits of a group are recorded in a list that bisection
and release tooling read, and a release is never cut from one of them.

### 6.6 Minimum honest time to verdict

The median pull request touches only `generator/` or `ci-helper/` (47%); 25% touch only the runner
workspace; 8.5% only the root engine crates [M, 200 merged PRs, `proposals/fable-5.2.md`].

**Phase 3, the witness design** (median PR):

| Segment | Time | Basis |
|---|---|---|
| Push → webhook at `velnor-hooks` | 2 s | [M] push-event runs appear in 1–2 s (`fable-5.2` design review F1) |
| Fetch, merge tree, plan | 2 s | [E] |
| Delta build | 15 s | [E] 8.8 s mbx-warm on an Apple M5, scaled 1.7× for an EPYC core; measured on a worker in work item 1 |
| Lanes: tests, lint, docs in parallel | 10 s | [M] `velnor-ci`'s test step is 10 s median, 62 s p90 (§A T10) |
| Witness completes after the last receipt | 5 s | [E] the job is created and started about 15 s after the push, in parallel |
| **Total** | **about 35 s**; committed **≤ 90 s** | |

Runner-workspace PRs: 1.5–3 min until the crate split and the test-floor programme land [E: a
one-file edit to `velnor-runner-host` costs 35–99 s as a whole-crate compile on an M5 and 10.9 s
with incremental compilation (`fable-5.2` design review H10), plus tests]. Engine PRs stay at 5–6
min until the image-publication fixture (89 s, a cold example build) and its suite are fixed.

**Push-to-merge** adds: auto-merge enqueue (seconds), enqueue → merge-group run 19 s [M], the
merge-group request, and merge-group finish → merged 26 s [M]. A merge group finds every receipt it
needs when its tree's keys equal those the PR verified. With per-root keys that holds unless another
pull request touching the same roots landed in between, so the question is how long that window is.
Today it is hours, and S4's 0–18% exact-tree matches describe it. In the design it is the verdict
time plus about a minute. Two replays of real arrivals give the share of merge groups whose tree
equals the PR-tested tree [E, `opus-5.5` and `fable-5.2` design reviews, 98 and 142 merges]:

| Window between the PR's verdict and its merge group | Same tree |
|---|---|
| 1–2 min | 89% |
| 2.5–5 min | 78–83% |
| 10 min | 70% |
| 11–20 min | 52–53% |
| twice today's demand, at 2.5 and 11 min | 80% and 69% |

Per-root and per-package keys add 3–10 points at every window. For those merges push-to-merge is the
verdict plus about 1 min: about 2 min for the median PR in phase 3 [E]. The rest re-run the
obligations whose keys changed.

## 7. Kubernetes

- **Role.** k3s stays as the fleet layer: it deploys the node agents, site services, mirrors and the
  ARC scale sets, accounts for every pod's resources, and enforces quotas, priorities and network
  policy. Velnor's scheduler places obligations on nodes; kube-scheduler places the pods.
- **Webhook ingress.** `velnor-hooks` runs in each site behind one public HTTPS endpoint. Today the
  nodes expose only SSH, peers and the API to one address (§B.4.3), so this endpoint is new: it
  accepts only requests signed with the App's webhook secret, and `velnor-hooks` replays missed
  deliveries from the App's delivery list every minute.
- **Sandbox lifecycle.** Each node agent keeps single-use pods of each class created and idle on its
  node. A pod runs one obligation request and is deleted with a UID precondition. Creating a pod
  takes about 6 s [M, 436 pod starts, `proposals/opus-5.5.md`], and it happens before the work
  arrives. A pod is never reused: work can leave processes, images and configuration behind, and a
  persistent store was poisoned this way in velnor-legacy (§E.6).
- **Warm slots.** Per worker: 2 idle build sandboxes with at most 4 running, 1 idle docker sandbox
  on each tainted docker node; ARC keeps warm gate runners for the witness. The agent resizes pools
  from the last hour's arrivals. An idle pod costs about 45 MiB (§B.2.2).
- **Sizes.** Build: request 16 CPU with no CPU limit, memory 32 GiB request equal to limit (the
  heaviest observed job peaked at 15.7–16.8 GiB, §B.2.2 and decision log I036). Docker: 8 CPU plus 4
  for dind, 24 GiB, for Docker-needing test binaries only; the runner root itself builds in the
  build class, although it is touched by 55 of 100 merges and its Docker tests would otherwise pull
  it into the smallest class. Gate: 1 CPU, 2 GiB. Cargo and nextest parallelism follow the sandbox's
  CPUs, not a constant.
- **Node settings.** Kube and system reservations (3 CPU, 6 GiB), a `memory.available` eviction
  threshold of 2 GiB, `podPidsLimit` 16384, the kubelet root directory on the XFS data volume (today
  `emptyDir` lands on the ext4 OS disk, `velnor-runners/PLAN.md:45`), topology labels for site.
- **Priorities.** `velnor-merge` > `velnor-gate` > `velnor-pr` = other organisations' CI >
  `velnor-batch` (the builder, speculation, scheduled full runs). Only `velnor-batch` is preempted.
- **Untrusted pull requests.** Fork pull requests run on GitHub-hosted runners, and `ensure` never
  touches a fork's tree. Build sandboxes get a default-deny NetworkPolicy with an allow-list, no
  service-account token and no host paths except what the agent mounts read-only.
- **Both sites.** Each site has its own `velnor-hooks`, ledger and store service; a request stays in
  one site. The tainted docker pool has one node per site.
- **Other organisations.** ChainArgos and jackin keep their ARC scale sets. They get a guaranteed
  CPU quota, separate namespaces and separate stores; no store, receipt or snapshot crosses an
  organisation.

## 8. Servers and cluster tuning

Ranked by gain per effort. "Measure first" items get no credit until a measurement on a worker shows
the stall they fix.

| Rank | Change | Expected gain | Evidence | Risk | Rollback |
|---|---|---|---|---|---|
| 1 | Gate jobs on the cluster in a warm gate class | −5.8 to −57 min per run | [M] §A T5 | Gate jobs compete with builds | Labels back to hosted |
| 2 | Honest requests, kubelet reservations, eviction threshold, PID limit, priority classes, per-org quotas | Removes the 1.2–2.4× stretch under load | [M] runner-host 3.7 → 8.8 min (§A T6); pods bursting to 84 cores on 2-CPU requests (§B.2.2) | Fewer concurrent pods | Revert values |
| 3 | Remove `build.jobs=2` and `--jobs 2`; parallelism from the pod | `maintenance-helpers` 8.5 → about 5 min | [M] `mise.toml:90-193`; [E] `proposals/opus-5.5.md` | None | Revert lines |
| 4 | Toolchain, tools and action archives as read-only node stores | 14–18 s per job | [M] 10 s tools (§A T9b) + 4.4 s actions | Stale tool (keys are versions) | Remove the mounts |
| 5 | Docker only in the docker class | 3–4 s per job, fewer privileged pods | [M] 57 dind probe failures per hour (§B.6) | A misrouted Docker job fails fast | Label back |
| 6 | Kubelet root directory on the XFS volume, `reflink=1` confirmed | Enables every L1 store | [M] root is ext4 on the OS disk; data LV is XFS (`velnor-runners/PLAN.md:45,72-80`) | Node rebuild mistakes | Revert the kubelet argument |
| 7 | Node-persistent compile cache, engine per section 5.3 | From 46–278 compiled crates per job to the PR's own delta | [M] §A T9a; Mac measurement (section 5.2) | A stale view means a slow build, not a wrong one (C6) | Disable the mount |
| 8 | Crates mirror through environment variables | 1–3 s per job, no internet dependency | [M] §B.4.2 | Mirror outage | Unset the variables |
| 9 | Site labels and request stickiness | Avoids 35 ms round trips for artifacts | [M] §B.5 | Unbalanced sites | Remove the preference |
| 10 | Debuginfo: line tables for workspace crates, none for dependencies | −10 to −24% cache size; −20 to −27% CPU on a runner-host rebuild | [M] decision log I108; `proposals/opus-5.5.md` | Poorer backtraces | Profile setting |
| 11 | CPU governor or EPP to performance | 2–10% | measure first: not readable without a shell (§B.1.6) | Power | Revert |
| 12 | Same-site network at 10 Gbit/s | Needed for moonshot M6 and site replication | measure first: NIC speed unknown | Cost | — |
| 13 | Static CPU manager on one node | 0–15% | measure first | Loses the burst cold builds need | Policy back to `none` |
| 14 | THP and dirty-writeback settings | credit 0 | measure first | Compaction stalls | Revert |
| 15 | mold | small | measure first: Rust ≥ 1.90 already links with rust-lld | Link bugs | Remove |
| — | Striping the data volume; tmpfs work directories | rejected | FI workers have one data disk; reflinks cannot cross into tmpfs and the slow tests are CPU-bound (decision log I099, I100) | | |

SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 9. Moonshots

Each moonshot states what must be true, the design element that attacks it, and the cheapest
experiment that proves or kills it. None is dropped for being hard; one is dropped only when its
experiment shows it cannot keep C1–C7. Every experiment runs in week 1, beside phase 0, on
velnor-lab or a drained worker, and none touches the required path.

| Id | Moonshot | Phase | Status |
|---|---|---|---|
| M1 | PR verdict under 60 s for the median PR | 3 | stretch (committed ≤ 90 s) |
| M2 | Push-to-merge under 5 min for the median PR | 2 (≤ 5 min), 3 (≤ 3 min), 4 (100 s) | committed; stretch in phase 4 |
| M3 | Zero cold start: the first cargo command 3–4 s after the push | 3 | stretch |
| M4 | Zero duplicate verification across PR, merge queue and `main` | 1, 2, 3 | committed for the SHA, root and package keys; stretch below packages |
| M5 | Speculative merge queue on the queue tip | 3 | stretch, shadow from phase 0 |
| M6 | Test-granularity scheduling over all 480 threads | 4 | stretch |
| M7 | Break the single-test floor | 1–2 | committed: `Journal::open` ≤ 1 s; stretch ≤ 15 s per test |
| M8 | Verification that begins before the push | 3 | stretch |
| M9 | Zero queue wait | 0–3 | committed ≤ 5 s p90; stretch ≤ 1 s |

**M1. PR verdict under 60 s.** Must be true: the push webhook reaches Velnor in ≤ 8 s, the time
GitHub needs to create the first job; the median PR's delta build takes ≤ 15 s and its lanes ≤ 10 s;
the witness adds ≤ 5 s after the last receipt. Attacked by: `ensure` on the push webhook and the
witness job (section 3.3). Cheapest experiment, 2–3 days on velnor-lab: a webhook receiver, one warm
pod and a one-job workflow that waits for the result; 50 pushes, recording webhook arrival, first
cargo command and verdict. Killed if webhook arrival P50 is worse than 8 s; the target then becomes
90 s.

**M2. Push-to-merge under 5 min.** Must be true: a short verdict, so that the window in which
another pull request can land before the merge group stays short; auto-merge always on; and a merge
group that finds its receipts by key. Attacked by: `ensure` on the push (a short window), per-root
and per-package keys, queue-tip speculation (M5), merge-group-first admission and depth 8. In the
window model of section 6.6, 78–89% of merge groups find every receipt when the window is 1–5 min,
and those merges take the verdict plus about 1 min. Cheapest experiment: work item 4 records the
tested tree and keys of every run; the replay of each week's merges at the measured verdict time
gives the hit rate. Killed if that rate falls below 70%. Committed ≤ 5 min in phase 2 and ≤ 3 min in
phase 3; stretch ≤ 100 s in phase 4.

**M3. Zero cold start.** Must be true: the merge tree and plan take ≤ 2 s and a pre-created sandbox
has its view ready in ≤ 1 s. Attacked by: `ensure` on the push webhook, pre-created sandboxes,
read-only stores and the cache engine's views; later, microVMs restored from a snapshot for the fork
and docker classes. Cheapest experiment: M1's run, recording push → first cargo command. Killed if
that exceeds 10 s at P50.

**M4. Zero duplicate verification.** Must be true: an obligation's inputs can be named, so its key
is the hash of what its sandbox contains. Attacked by: input-addressed keys, per root in phase 2 and
per package in phase 3 (section 4.1), and finer test-binary closures as a phase-4 refinement.
Cheapest experiment, 1–2 days on velnor-lab: run each root, then each package, from a sparse
checkout of its key's paths; every failure names an undeclared input; then replay 200 merges with
the final declarations. Killed for package keys if more than 20% of packages cannot pass in a
closure-only checkout once their inputs are declared; the root keys still stand.

**M5. Speculative merge queue on the queue tip.** Must be true: Velnor predicts the tree GitHub will
build, which is the queue tip plus the PR, not `main` plus the PR: 122 of 193 merge-group runs were
built on another group's commit that had not landed [M, design-review check]. Attacked by: for
queued and auto-merge PRs, Velnor keeps the ordered prefixes `T_i = T_{i−1} ⊕ PR_i` and the
single-ejection bypass trees (7 trees for 4 PRs), computed with `git merge-tree` on the mirror, and
verifies them at batch priority through `ensure`. A wrong guess wastes preemptible CPU and never
changes a verdict. Cheapest experiment: a read-only shadow from phase 0 that logs predictions on
every enqueue and every `main` advance and compares them with the real merge-group trees over 50
merges or two weeks. Killed below a 70% match.

**M6. Test-granularity scheduling.** Must be true: some suite's summed test time divided by one
sandbox's threads exceeds its longest test after M7, and binaries move within a site in ≤ 5 s.
Attacked by: the agent's executor distributing test cases (section 6.3). Cheapest experiment: one
week of per-test CPU-seconds from nextest's JSON output, plus `iperf3` between same-site workers.
Killed if no suite's summed time exceeds its longest test by more than the transfer cost.

**M7. Break the single-test floor.** Must be true: journal set-up dominates the slow tests, and
cases can share a pre-built valid prefix. Attacked by: the pre-migrated journal template, one test
per case, the per-test CPU budget, and optionally optimised dependencies in tests (owner question
8). Cheapest experiment, 1–2 days on one worker: CPU-seconds of the 20 slowest tests under four
profile variants, and a branch with the template. The 15 s stretch is killed if the cases share
state that cannot be separated.

**M8. Verification that begins before the push.** Must be true: agents spend time between "code
done" and "pushed" that verification can overlap. Attacked by: `velnor verify`, the agents' test
command, which sends the working tree to the node mirror and calls the same `ensure`; the later push
finds receipts. Only results that Velnor executed on linux/amd64 count: a test run on a developer's
Mac is another platform, and its key belongs to the agent. Cheapest experiment: a week-long hook
that records commit time and tree, plus the duration of the agents' local test runs. Killed if
agents rarely test before pushing.

**M9. Zero queue wait.** Must be true: capacity covers peak demand for merge and gate work. Attacked
by: merge-group-first admission and preempting batch work. Cheapest experiment: replay the busiest
measured hour's arrivals against the capacities in section 7.

## 10. Phases

Every moonshot experiment and the cache head-to-head run in week 1, beside phase 0; their results,
not the calendar, choose what phases 2 and 3 build. The targets are those of section 1.4.

| Phase | Scope | Exit gate (measured on velnor-new over at least 50 merges) | Expected saving |
|---|---|---|---|
| 0, week 1 | Gate jobs on the cluster: the generator release that moves them is merged, but the workflow was never regenerated, so they still run hosted; regenerating it is the first measured gain. Narrow the image-candidate trigger; Source qualification on the cluster or only in the queue; auto-merge; no catch-up merges; depth 4; fixed cache paths; `event.json` race fix; push-to-merge, admission and flake instrumentation; the queue-tip shadow; the week-1 experiments | PR verdict P50 ≤ 12 min, P90 ≤ 20; push-to-merge P50 ≤ 25 min, P90 ≤ 60; hosted gate queue on the critical path p90 ≤ 1 min; admission span p90 ≤ 5 s | 5.8–57 min of gate queue per run; the cancelled-run stall shrinks from up to 2,203 s to one warm job; 7.5 min median enqueue delay; twice the queue throughput |
| 1, weeks 2–3 | Selection fixed in the engine; the test-floor programme; `-j` caps removed; gate, build and docker classes under ARC with warm pools and honest sizes; node tools and action archives; crates mirror; push run reduced to release duties; flake budget machinery; tested-tree recording | PR verdict P50 ≤ 5 min, P90 ≤ 10; push-to-merge P50 ≤ 12, P90 ≤ 30; runner-min per merged PR ≤ 150; false failures ≤ 3%; `Journal::open` ≤ 1 s | About 62 runner-min per merge from the push run; a generator or helper PR falls from 10.9 to about 3.5 min and from 52.9 to about 8 runner-min [E] |
| 2, weeks 3–6 | `velnor-gen` cutover with parity; the root jobs call `ensure`, so the agent executes and observes every obligation; the ledger with per-root keys; the compile-cache engine the head-to-head chose; kubelet on XFS; the `main` builder; the crate split; state seal retired; test-time budget | PR verdict P50 ≤ 2 min, P90 ≤ 6; push-to-merge P50 ≤ 5, P90 ≤ 15; ≥ 65% of merge groups execute nothing; CPU per merged PR ≤ 60; no test over 60 s | Cold compiles of 196–497 crates per job become deltas; a merge group re-runs only roots whose key changed: 125 root-jobs per 100 merges instead of 500 [E] |
| 3, weeks 6–10 | `ensure` on the push and merge-group webhooks; the witness job; per-package keys; queue-tip speculation; `velnor verify` as a stretch; depth 8 | PR verdict P50 ≤ 90 s, P90 ≤ 4 min; push-to-merge P50 ≤ 3, P90 ≤ 8; ≥ 85% of merge groups execute nothing; first cargo ≤ 5 s after the push; false failures ≤ 1%; CPU per merged PR ≤ 35 | Two of three job hops and the plan job disappear; most merges take the verdict plus about 1 min |
| 4, weeks 10–18 | Moonshot M6, finer test-binary closures, the Velnor merge train if approved; the gated dispatcher, hot Worker or podless completion if the witness left a residual over 5 s | stretch: PR verdict P50 ≤ 30 s, P90 ≤ 100 s; push-to-merge P50 ≤ 100 s, P90 ≤ 4 min | GitHub's 45 s of queue latency per merge, if Velnor lands merges |
| 5, after week 18 | Agent workspaces on the cluster; microVM fork and docker classes; other organisations on Velnor | stretch: push-to-merge P50 ≤ 75 s | Verification overlaps the agent's own work |

A phase that misses its gate does not start the next one until the miss is explained. Each phase
keeps a working fallback: hosted labels in phase 0, the per-job workflow until the witness has
passed a four-week soak, and the hosted break-glass workflow always.

## 11. The first ten work items

1. **Cache head-to-head.** Scope: candidates (a), (b) and (c) of section 5.1 on the runner
   workspace, at fixed checkout paths, on velnor-190 and velnor-5 in a throwaway namespace: an
   unchanged tree, a one-file edit, a lockfile change, a second node, four concurrent jobs; count
   rustc processes, wall and CPU time, disk, page cache, set-up and teardown, with
   `CARGO_INCREMENTAL` off and on. Acceptance: section 5.3 filled with raw data and a decision by
   its rule. Depends on: owner approval of the namespace (running).
2. **Gates on the cluster.** Scope: regenerate and land the workflow from the generator release that
   moves Plan, Ledger CLI, the lints and both required jobs to a `velnor-gate` scale set with warm
   pods and no dind; the image-candidate workflow triggered only where it is needed; Source
   qualification on the cluster or only in the merge queue. Acceptance: over 50 PR runs, hosted
   queue on the critical path p90 ≤ 1 min and no CI job queued on a hosted label. Depends on:
   nothing.
3. **Queue and agent procedure.** Scope: agents enable auto-merge when they open a pull request and
   stop catch-up merges; `max_entries_to_build` 4; required checks bound to the GitHub Actions
   integration; the list of a HEADGREEN group's intermediate commits for release tooling.
   Acceptance: ≥ 90% of merged PRs have auto-merge; green → enqueue P50 ≤ 30 s. Depends on: owner
   questions 3, 5 and 6.
4. **Measurement and shadows.** Scope: a push-to-merge collector from PR timelines, clocked from the
   accepted push of the final commit; the admission span (run created → first job) and the
   cancelled-run stall as their own spans; queue residence and merges per hour; the tested tree
   hash, root keys and obligation digests in every plan's output; the paired-rerun flake sampler;
   the queue-tip prediction shadow (M5); webhook arrival times (M1); the weekly window-model replay
   (M2). Acceptance: a daily report with P50, P90 and denominators that reproduces the 85-min
   baseline from past data. Depends on: nothing.
5. **The test floor.** Scope: a pre-migrated journal template for `Journal::open`; one test per case
   for the two looped tests; the per-test CPU budget and its list; `ci-helper` tests in parallel
   with timeouts sized to the pod; the `event.json` snapshot before parallel steps. Acceptance:
   `Journal::open` on the template ≤ 1 s measured on a worker; the list shrinks; no new false
   failures in 100 paired reruns. Depends on: nothing.
6. **Selection in the engine.** Scope: declare the inputs of the crate-root build scripts and the
   test reads the plan names; replay the last 200 PRs through the fixed planner in shadow.
   Acceptance: the replay selects every job that failed in those PRs; a generator or helper PR runs
   ≤ 4 min. Depends on: nothing.
7. **Honest classes under ARC.** Scope: gate, build and docker scale sets; the sizes in section 7;
   kubelet reservations, eviction and PID limits; priority classes and per-org quotas; the `-j` caps
   removed; toolchain, tools and action archives as read-only node mounts. Acceptance: job-duration
   p90/p50 ≤ 1.3 at peak; tool setup ≤ 2 s; no OOM kills over a week. Depends on: 2.
8. **No third run.** Scope: the push run checks for a green merge-group run on the exact SHA and
   then does only release and publication duties; otherwise it runs everything. Acceptance: ≤ 2
   runner-min per matching push; the release gate on `main` still passes. Depends on: 4.
9. **`velnor-gen` cutover with parity.** Scope: one job per root with parallel lanes, whose steps
   call `ensure` on the node's agent; the local-mode configuration, the maintenance checks, both
   required names, the Velnor classes in `runs-on`; no `edited` trigger once the ledger re-plans a
   base retarget without restarting identical work; one week of shadow runs beside the engine
   comparing obligation inventories. Acceptance: empty inventory difference over 100 runs; PR
   verdict P50 ≤ 2 min. Depends on: 1, 5, 10.
10. **The ledger and `ensure` on velnor-lab.** Scope: `velnor-hooks`, the ledger with per-root keys,
    per-obligation receipts and in-flight join, the signer and the append-only log, the agent's
    executor in pre-created sandboxes, and a one-job workflow whose witness calls `ensure`; the
    sparse-checkout experiment for root and package keys; webhook replay; the M1 measurements.
    Acceptance: 50 pushes with webhook arrival P50 ≤ 8 s and verdict P50 ≤ 60 s for a
    generator-sized change; a killed webhook still yields a correct verdict through the witness; at
    most 20% of packages fail closure-only; 0 disagreements in 500 sampled re-executions. Depends
    on: owner question 1 for the phase-3 cutover, nothing for the lab.

## 12. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A receipt or a selection gives a false green | Audit sampler disagreement; cold differential builds; nightly and every-20th-merge full runs; undeclared reads in the file-read audit | That reuse class turns off and work runs again; receipts expire after 7 days; the bad merge is reverted by a normal PR |
| A stale compile view hides a failing build | Cold differential build disagrees with the warm path | Retire the lineage; rebuild from empty |
| GitHub changes webhooks, the merge queue or the runner protocol | Canary request every 10 min; a version tracker against GitHub's deprecation dates | The witness's own `ensure`; the per-job workflow; the hosted break-glass workflow |
| The ingress or the ledger is down | Webhook replay backlog; ledger health | Witnesses run their obligations fresh; slower, never wrong |
| False failures rise as work packs denser | Paired-rerun sampler; nextest flaky counts per test | Lower the per-node build cap; queue depth back to 4; quarantine with sign-off |
| A PR poisons a cache another request trusts | Receipts record producer and scope; promotion only by the builder | Delete the generation; requests build cold, slowly and correctly |
| A signing key leaks, or a forged receipt reaches a verdict | The witness verifies each receipt's signature and its entry in the append-only log; audits re-execute a sample | Revoke that node's key and every receipt it signed; affected obligations run fresh |
| FI workers run out of disk | Store budgets and alerts at 70% | Least-recently-used eviction, per-org caps |
| Other organisations starve, or starve velnor-new | Wait per organisation | Quotas and priorities; preempt only batch work |
| The new code grows like velnor-legacy | Line budgets checked in CI; phase gates | Stop and review at a budget; the per-job workflow keeps working at every phase |
| The `velnor-gen` cutover drops a configuration or check | Shadow inventory comparison | Keep the engine workflow until the inventories match |
| Speculation or the builder starves real work | Batch-class CPU share | Lowest priority, preempted first |

## 13. Owner questions

1. **`docs/goals.md:16` says jobs are always executed by GitHub's official runner.** The witness
   design keeps that literally: the official runner executes the only job, `Required`. What changes
   is that the work the job reports on runs in Velnor sandboxes before and beside the job, started
   by a webhook. Example: a one-file change to `ci-helper` is built and tested 2 s after the push,
   and the `Required` job prints those logs and the verdict about 35 s after the push. The gated
   dispatcher, podless completion and a Worker replacement would contradict the line; each is
   decided separately if its experiment passes. Recommendation: amend line 16 to "every job runs in
   GitHub's official runner; verification may run in Velnor sandboxes that the job reports".
2. **Merge groups keep running in full, and receipts satisfy most of it.** This replaces the earlier
   question about selecting merge groups by change, which the design review showed unsound: diffing
   a group against the entry ahead never selects a failed predecessor's change, and HEADGREEN lands
   it. Hard decision 3 now holds literally, and the question is whether a receipt with the same
   input key counts as "ran and passed" (`docs/goals.md`, "Green means verified"). Example: a
   generator-only pull request's merge group lists every root's obligations; four roots have the
   same key as on `main`, so their receipts satisfy them, and only the generator root executes. The
   group takes about 1 min instead of a 3–5 min warm full run. Recommendation: yes, with C1–C7 and
   the audit as written.
3. **HEADGREEN or ALLGREEN?** HEADGREEN lands a whole group when its tip passes, so a failed entry
   can land inside a passing group; it happened once in the evidence's 15 days. Example: entry A
   fails its own merge-group run, entry B (main + A + B) passes, and both land. With ALLGREEN, A is
   ejected and B is rebuilt without it. Recommendation: keep HEADGREEN while merge groups are slow,
   with C1 stated for group tips and intermediate commits listed for bisection and release tooling;
   switch to ALLGREEN once queue-tip speculation keeps the bypass trees verified, which makes an
   ejection nearly free.
4. **Stay on the Free plan?** The org's plan is `free` (20 concurrent hosted jobs). After phase 0 no
   job on the merge path runs on hosted runners; forks and macOS release builds still do.
   `docs/goals.md:45` keeps the generator inside free limits, and the cluster tiers do not change
   what hosted runners need. Recommendation: stay on Free; buy a paid plan only if fork volume or
   release builds queue, measured.
5. **Agent procedure.** Example: 0 of 40 recent merged PRs enabled auto-merge and the median PR
   waited 7.5 min between green and enqueue. Recommendation: agents enable auto-merge when they open
   a pull request, stop merging `main` into their branches, and later run `velnor verify` before
   pushing.
6. **Ruleset changes.** Bind `Required verdict` and `Required` to one integration now (anything can
   post them today); move `max_entries_to_build` to 4 now and 8 in phase 3; accept a single
   `Required` name at the `velnor-gen` cutover; the grouping as question 3 decides. Recommendation:
   yes to the first three.
7. **The Kubernetes runtime inside `velnor-runner-host`.** The proposal under review puts the
   cluster runtime behind a trait in `velnor-runner-host`. This design builds the cluster's executor
   as new crates instead: that crate is 157k lines with about 60 open correctness issues and sets
   the CI critical path, and its unit places the runner's JIT credentials inside the job container.
   Recommendation: keep `velnor-runner-host` for the Mac and local mode, and build the cluster
   executor separately.
8. **Test profile.** Building the runner workspace's dependencies at `opt-level = 3` in tests cut
   the slowest test class's CPU by 66% [M, `proposals/fable-5.2.md`], measured with debug assertions
   off in dependencies. With them on, nobody has measured it. Recommendation: measure the four
   combinations of `opt-level` and debug assertions with overflow checks fixed, under design rule
   14, and decide on both numbers.
9. **Other organisations.** Should ChainArgos and jackin share velnor-new's priority for PR jobs,
   with a guaranteed quota, and when do they move to Velnor's executor? Recommendation: equal PR
   priority with quotas now; offer Velnor in phase 5.
10. **Hardware.** Rent one high-clock node for a month (section 14)? Recommendation: yes, after
    phase 2, routed only runner-workspace work.
11. **A Velnor merge train instead of GitHub's merge queue?** GitHub's queue adds 19 s to start a
    merge group and 26 s to merge it [M], about 45 s per merge, which becomes the largest term once
    a verdict takes under a minute. Velnor could land merges itself: it fast-forwards `main` to the
    exact commit whose obligations all hold, a compare-and-swap that verifies every tip by
    construction. Example: a green pull request lands 5 s after its verdict instead of about 50 s.
    The cost: squashed pull requests must still show as merged, the queue's interface goes away, and
    a Velnor outage stops merges until the queue rule is re-enabled. Recommendation: run a two-week
    shadow train beside the queue in phase 4, kill it below a 95% tree match, then decide.
12. **One counted re-execution for tests the change could not have caused to fail?** Retries are 0
    today, so every flake ejects a queue entry and rebuilds the entries behind it. A test whose
    package closure the change did not touch has the same inputs as on `main`, where it passed.
    Example: a generator-only change hits a timing flake in a `velnor-runner-host` test; one
    immediate re-execution passes, it is recorded in a non-blocking `Flaky` check and charged to the
    flake budget, and the merge proceeds. Tests whose closure changed never get a retry; they run
    three times instead (section 1.2). Recommendation: yes, after replaying the 10 false failures of
    the evidence against their pull requests' closures.

## 14. What more hardware would buy

| Purchase | Buys | When |
|---|---|---|
| One high-clock node (desktop-class cores at 5.5 GHz or more) | 1.3–1.5× on the serial path: rebuilding `velnor-runner-host` and the longest tests [E] | After phase 2, rented for a month first |
| 10 Gbit/s between same-site workers | Moonshot M6 and site replication; 1.48 GB of test binaries in about 1.2 s instead of 12 s [E] | When M6's experiment shows a suite that needs it |
| Two more k3s server nodes | A control plane that survives one node; no speed | Phase 4 |
| More NVMe on the FI workers | Cache headroom on the 3.5 TB data disks | When eviction starts dropping live generations |
| More EPYC workers | +96 threads each, about +20% capacity | Only if the wait for merge and gate work exceeds 15 s p90 after phase 3 |

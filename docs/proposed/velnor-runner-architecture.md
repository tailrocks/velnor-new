# Velnor runner architecture

**Status:** Proposal. It replaces nothing yet. Building it needs the owner decisions in section 13,
because parts of it contradict `docs/goals.md` and `docs/hard-decisions.md`.

**Scope:** how Velnor runs velnor-new's GitHub Actions jobs on the project's Hetzner k3s cluster so
that waiting for CI stops being the slowest part of delivering a change. The generator
(`velnor-gen`) and helper (`velnor-ci`) are in scope where the runner needs something from them.

**Sources.** Facts are cited from the [evidence pack](../research/velnor-runner-2026-11/evidence.md)
as §A T5 and similar, from the eight panel proposals under
`../research/velnor-runner-2026-11/proposals/`, and from the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea the panel raised with
its decision (ids `I001`–`I120`, `N01`–`N17`). Every number is tagged: **[M]** measured (with the
source), **[E]** estimate (with the arithmetic or basis), **[T]** a target. A target is
**committed** when the evidence makes us confident, **stretch** when an experiment must prove it
first.

## 1. Goal and success metric

### 1.1 The metric

The single metric is **push-to-merge**: the time from the last push to a pull request's head until
the pull request is merged, measured from the pull request timeline. We report P50 and P90 per week
over every merged pull request, next to PR verdict time (push to the `Required` check),
runner-minutes per merged pull request, the compile-cache hit rate and the false-failure rate.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M] 98 merged PRs, `proposals/opus-5.5.md` §1.1 |
| Push-to-merge on the worst measured day, P50 | 400 min, of which 278–282 min between queue entry and merge | [M] `proposals/opus-5.5.md`, `proposals/fable-5.2.md` |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M] §A T2 |
| PR run with no hosted queue | about 11 min | [M] 10.9 min over 12 runs (`proposals/fable-5.2.md`); replays 11.4 and 12.0 min |
| Runner-minutes per run (PR / merge group / push) | 52.9 / 64.6 / 62.0 | [M] §A T2 |
| Runner-minutes per merged PR | 285–347 | [E] about 3–4.8 PR runs plus a merge group and a push per merge |
| Full runs per merge | 3 (PR, merge group, push on the same SHA) | [M] §A T7: 120 of 159 green merge groups had a push run on their SHA |
| False failures per full run | 8.1% | [M] 10 of 124 push runs failed on a SHA whose merge group had passed, all in test steps (decision log, (h)) |
| Cross-job compile reuse on the cluster | 0 | [M] sources cache 0 of 48, toolchain 0 of 58, state 0 of 3,636 (§A T9a, T9b) |

### 1.2 Same confidence: the contract

"Same confidence" is a contract with seven clauses. A change to this design that breaks a clause is
rejected, however much time it saves.

1. **C1. Every landed tree is verified.** For every commit that lands on `main` and every obligation
   the plan selects for it, there is a passing result whose key equals the obligation's key at that
   commit, produced by Velnor's trusted execution in the sandbox class the obligation requires. An
   *obligation* is one unit of verification: format, lint, docs, a per-crate check, one test binary
   under one configuration, or a named check.
2. **C2. Keys capture every input.** In phases 0–3 the key is the exact Git tree plus the
   obligation's definition (workflow, helper version, toolchain, runner image, sandbox class). A
   wider key, the *closure* of the files and settings one obligation can observe, is allowed only
   for obligations whose inputs the sandbox enforces (section 4.3).
3. **C3. Selection is sound or wide.** The plan comes from a released helper, never from the change
   under test (`docs/goals.md` rule 12). A file the plan cannot place selects everything. A file
   read that no declaration covers widens selection and is reported.
4. **C4. Reusable results are observed by Velnor.** A result that a run of a *different* tree may
   reuse is recorded by the node agent from test processes it started itself. A job's own report may
   back an exact-tree receipt only, because only runs of that identical tree reuse it, and that code
   can already report its own result today. A job never signs a receipt.
5. **C5. Uncomparable work always runs.** Obligations that read the network, the event payload, the
   clock in an assertion, Docker images not pinned by digest, or that publish anything, run fresh
   every time.
6. **C6. Reuse is audited.** A random sample of reused results is executed again (2% of exact-tree
   reuse, 10% of closure reuse in its first month), and a scheduled full run executes everything
   nightly and on every 20th merge. One disagreement disables that kind of reuse until it is
   explained.
7. **C7. Flakes are budgeted** (section 1.3).

Why removing a duplicate run does not lower confidence: for an obligation whose inputs are
identical, a second execution adds information only about the test's own nondeterminism, not about
the change. That information is collected deliberately by C6 and C7 instead of by running every
merge three times.

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

| Target | Phase | Committed | Stretch |
|---|---|---|---|
| Push-to-merge P50 / P90 | 0 | ≤ 25 / ≤ 60 min | |
| Push-to-merge P50 / P90 | 1 | ≤ 15 / ≤ 35 min | |
| Push-to-merge P50 / P90 | 2 | ≤ 9 / ≤ 20 min | |
| Push-to-merge P50 / P90 | 3 | ≤ 6 / ≤ 15 min (P50 ≤ 8 without merge-group selection) | |
| Push-to-merge P50 / P90 | 4 | | ≤ 4 / ≤ 10 min |
| PR verdict P50 for the median PR | 3 / 4 / 5 | ≤ 2.5 min (phase 3) | ≤ 60 s (phase 4); ≤ 20 s after push when preflighted (phase 5) |
| Velnor-side wait (offer to job start), p90, merge and gate jobs | 3 | ≤ 5 s | ≤ 1 s |
| Job assignment to first cargo command, P50 | 2 / 3 | ≤ 15 s / ≤ 5 s | ≤ 2 s |
| Longest single test | 1 / 2 | ≤ 60 s | ≤ 15 s |
| Obligations executed twice on identical inputs | 1 / 3 / 4 | none on the same SHA (1); none on the same tree (3) | none on the same enforced closure (4) |
| Runner-minutes per merged PR | 1 / 2 / 3 | ≤ 150 / ≤ 60 / ≤ 40 | ≤ 15 |
| False failures per full run | 1 / 3 | ≤ 3% / ≤ 1% | ≤ 0.3% |

## 2. Where the time goes, and what changes the problem

Four facts set the design [M unless marked]:

1. **Waiting, not working.** Without hosted queues a PR run takes about 11 min, but the median PR
   waits 85 min from push to merge. On the worst day the three serial gate jobs on GitHub-hosted
   runners waited 57.0 of a 71.8 min run (§A T5), on an organisation with the Free plan's 20
   concurrent hosted jobs shared with two busy non-CI workflows (§A T11). The merge queue builds 2
   entries at a time and no agent enables auto-merge (0 of 40 recent merged PRs [M, decision log]).
2. **The same verification three times.** Each merge runs the full job set on the PR, on the merge
   group, and again on the push of the same SHA (§A T7). The third run found nothing and failed
   falsely 8.1% of the time.
3. **Every job is cold.** No compiled output, toolchain or crate source survives a job on the
   cluster (§A T9a, T9b). The cache restore that exists costs more than the build it saves: a 1.92
   GB entry took 44–61 s from GitHub's cache (decision log I036).
4. **Code sets the floor.** Two tests loop over 40 and 6 cases inside one test function and take
   258–274 s and 203–271 s; `maintenance-helpers` compiles with `build.jobs=2`; selection forces
   every crate with a build script (decision log I059, I061, I068).

The design attacks each fact by removing work, not by speeding it up: gates move off the hosted
pool; a verification ledger makes "verify identical inputs once" structural; caches live on the node
that runs the job; and the slow tests are rewritten. Rust components are built where owning the
mechanism removes work: the dispatcher, the node agent, the ledger and the stores. The step executor
stays GitHub's own, because the measured interpreter cost is 87–147 ms per action
(`proposals/fable-5.2.md`) and nothing in the evidence shows a native executor saving more than
seconds (section 3.5).

## 3. Architecture

### 3.1 What stays compatible and what is redesigned

GitHub remains the platform. From the outside Velnor is a GitHub Actions runner:

- **Unchanged:** workflow YAML and its semantics, the Runner Scale Set protocol (sessions, long
  poll, `acquirejobs`, `generatejitconfig`), broker sessions, the run service (`acquirejob`,
  `renewjob`, `completejob`), Results for logs and step status, check runs, rulesets and the merge
  queue, `GITHUB_TOKEN` and OIDC, and a runner version within GitHub's 63-day window (§F.6.1).
- **Redesigned inside:** who holds the runner identities, where and when a job runs, what is already
  on that disk, what a job may write, which work is skipped because an identical verified result
  exists, and how verdicts reach GitHub.

### 3.2 Components

All new code is Rust in the root workspace. Each crate has a line budget; crossing it stops work for
a review (decision log I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| Protocol client | `velnor-proto` | library | Wire types (grown from `velnor-runner-github`) and HTTP clients for the scale-set admin API, broker sessions, run service and the minimal Results calls | 10k lines |
| Dispatcher | `velnor-dispatch` | Deployment, leader plus standby in the other site | GitHub App auth, scale-set sessions, a pool of idle JIT identities with their broker sessions, admission order, `acquirejob`, job classification, placement, refusal, cancel relay, capacity advertisement, an acquisition journal | 12k lines |
| Node agent | `velnor-node` | privileged DaemonSet on workers | Pre-created single-use job pods, job hand-off, lease records and lock renewal for running jobs, node stores, main warmer, post-build harvest, result observation, file-read audit, cleanup by UID | 15k lines |
| Sandbox init | `velnor-sandbox` | PID 1 in each job pod | Keeps a hot `Runner.Worker` waiting on its pipe, writes `.runner`, delivers the job message, relays cancel, reports the exit | 1.5k lines |
| Ledger | `velnor-ledger` | library plus one service per site | Obligation keys, signed receipts, scopes, promotion, audit sampling | 6k lines |
| Store | `velnor-store` | library plus one service per site | Content-addressed blobs (test binaries, archives, reports, snapshot seeds), replication within a site, eviction | 6k lines |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, gate, cache steps, ledger lookups, test orchestration, the workflow shape | existing |
| Native executor | `velnor-exec` | not built | Only if the gate in section 3.5 passes | 12k lines |

### 3.3 The job path, from offer to completion

1. GitHub queues a job for a Velnor scale set. The dispatcher's scale-set session receives
   `JobAvailable` with the repository, event name, workflow reference, job name and labels.
2. **Admission.** The dispatcher acquires offers in priority order: merge-group jobs, then PR gate
   jobs, then PR jobs, then scheduled and speculative work. It advertises capacity
   (`X-ScaleSetMaxCapacity`) equal to free slots, so excess demand waits at GitHub, not inside
   Velnor.
3. **Assignment.** GitHub assigns the job to one of the dispatcher's idle JIT identities; the
   dispatcher keeps as many idle identities as the capacity it advertises. It holds every identity's
   broker session, receives the `RunnerJobRequest`, calls `acquirejob` with that identity's
   credentials and now holds the full job message. It journals the acquisition (job, plan, run
   service URL, identity) before doing anything else.
4. **Classification.** From the job message, which is authoritative, the dispatcher reads the
   repository, event, head repository, run and attempt, labels and steps. A fork or unauthorised job
   is failed with an annotation at this point: no pod, no code (decision log N11). Otherwise it
   picks the class (gate, build, docker), the scopes (section 4.2) and the site.
5. **Lease.** The first `renewjob` must succeed before any code runs, as in the official Listener
   (`Runner.Listener/JobDispatcher.cs:393-405`). Renewal uses the job's own token from the message,
   not the runner's key (`JobDispatcher.cs:734-746,1244-1249`).
6. **Placement** (section 3.4) picks a node and an idle pre-created pod of the class.
7. **Hand-off.** The dispatcher sends the message to that node's agent over mTLS. The agent prepares
   the pod's job directory (reflink clones, section 5), writes a sealed lease record and takes over
   renewal every 60 s.
8. **Start.** The agent passes the message to `velnor-sandbox` in the pod, which writes `.runner`
   (no credentials) and sends the message to the already running `Runner.Worker` over its pipe: a
   4-byte message type, then a length-prefixed UTF-16LE body
   (`Runner.Common/ProcessChannel.cs:70-81`, `StreamString.cs:18`). The Worker never sees the
   runner's RSA key; only `Runner.Listener` uses it.
9. **Steps** run in the unmodified Worker. Logs and step status go to Results with the job token.
   `velnor-ci` steps reach the ledger and node stores through a socket the agent mounts into the
   pod.
10. **Completion.** The Worker calls `completejob` itself. The agent harvests what the job produced
    (section 5.3), records observed results as receipts, deletes the pod with a UID precondition,
    records absence, and frees the slot. The dispatcher registers a fresh JIT identity in place of
    the consumed one.
11. **Failure.** If the Worker dies, the agent fails the job with `completejob` and an annotation,
    as the Listener does (`JobDispatcher.cs:1208-1219`). `JobCancel` arrives on the broker session;
    the dispatcher relays it and the Worker receives `CancelRequest` on its pipe.

### 3.4 Placement

GitHub, not Velnor, picks which idle runner of a scale set gets a job, and neither the offer nor
`generatejitconfig` carries a binding (§F.1.3). Velnor therefore places **after** assignment, inside
its own runner identity, and makes most placement unnecessary in the first place:

- **Every node is warm for `main`** (phase 2). A warmer on each node rebuilds the latest `main` for
  each Cargo root and freezes it as a snapshot generation, so a job on any node starts from `main`.
  Placement no longer decides whether a job is warm; it decides only the smaller things below.
- **Per-job placement** (phase 3) minimises predicted finish time: pod availability + clone time +
  predicted rebuild on that node (smaller where the pull request's own scope lives) + predicted
  tests + a site penalty when the run's other jobs and artifacts are in the other site. An idle cold
  node beats a busy warm one.
- **Run stickiness.** All jobs of one run stay in one site; DE to FI is 35 ms over WireGuard (§B.5).
- **Class routing** works before phase 3 through labels: `velnor-gate`, `velnor-build`,
  `velnor-docker`, each its own ARC scale set with warm pods.

### 3.5 Executor strategy and fallback

- **The executor is GitHub's `Runner.Worker`**, unmodified, in every phase this document commits to.
  It keeps every workflow construct working, including new ones, and it is the oracle any other
  executor is compared against.
- **Hot Worker.** The Worker waits 30 s for its job message (`Runner.Worker/Worker.cs:23`) and reads
  `.runner` only after the message arrives (`JobRunner.cs:47`). `velnor-sandbox` keeps one Worker
  process waiting in every idle pod and replaces it every 25 s, so the .NET start-up is off the
  critical path (experiment-gated, N05).
- **Receipt short-circuit** (phase 4, owner decision). When every obligation of a job already holds
  by receipt, the dispatcher completes the job itself through `completejob` with the receipt in the
  log and no pod at all. This is the executor change with the largest payoff, because it removes
  whole jobs rather than milliseconds (N06).
- **Native step executor** (phase 5, experiment-gated). Built only if the measured time from message
  delivery to first step with a hot Worker and pre-seeded actions exceeds 15 s per run or 10% of the
  warm critical path, over 100 paired jobs per root with zero semantic differences (I005, I007). Its
  fallback is decided by the dispatcher before the first step, in the same runner identity, so no
  lease ever moves between runners; a lease cannot be transferred after `acquirejob`
  (`RunServiceHttpClient.cs:106-107`). There is no mid-job fallback.

### 3.6 Isolation and trust

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | plan, verdict | unprivileged pod, 1 CPU | GitHub API | read-only tools |
| build | same-repository PR, merge group, main | unprivileged pod, no Docker, seccomp `RuntimeDefault`, all capabilities dropped, no service-account token | allow-list: GitHub, crates mirror, site store | read-only trusted; per-job clones; never a shared writable path |
| docker | jobs that declare Docker | pod with a privileged dind sidecar, on two tainted nodes (one per site) | as build, plus a registry mirror | as build |
| warmer | `main` builds, no GitHub job | unprivileged pod | crates mirror | writes the trusted scope through the agent only |
| fork | fork pull requests | GitHub-hosted runners until a microVM class exists | — | none |

Rules:

- A job never writes a store another job reads. The node agent takes what a job produced after the
  job's build phase and decides where it may go (section 5.3).
- The runner's credentials stay in the dispatcher. A job sees only its own token, which GitHub
  already gives it.
- The dispatcher's refusal, not a hook inside an already-running privileged pod, is the trust
  boundary from phase 3. The existing job-start hook stays as a second check.
- The required checks are bound to one integration in the ruleset. Today the ruleset names `Required
  verdict` and `Required` with no integration, so any app, or a workflow token granted `statuses:
  write`, can post them (N12).

### 3.7 Failure handling and recovery

- **Journal discipline:** persist the intent, make the call, record the outcome. Unknown is never
  treated as absent. This is velnor-new's rule; the code is new and small.
- **Dispatcher restart.** Running jobs are not affected, because node agents hold their leases.
  Journaled acquisitions not yet handed off are handed off or failed. Sessions reopen.
- **Node agent restart.** It reloads sealed lease records, resumes renewal before `lockedUntil`,
  reconciles pods by UID and fails jobs whose pods are gone.
- **Node loss.** Its jobs' leases expire and GitHub fails them; the dispatcher stops placing on the
  node. No job is re-run silently: "already acquired" and "unavailable" never trigger repository
  code a second time.
- **Fallback:** ARC stays installed with its own label for four weeks after the cutover. Moving
  velnor-new back is a label change in the generator config.

### 3.8 What is taken, and what is not copied

| Source | Taken | Not copied |
|---|---|---|
| velnor-new runner | Wire types without an HTTP dependency (`velnor-runner-github`), lifecycle and ownership types (`velnor-runner-core`), the journal discipline, UID-based absence proofs, the job-authority policy and job-start hook, velnor-local as the conformance harness | The 157k-line `velnor-runner-host` and its Docker launch path, which keeps serving the Mac host. Putting the runner's JIT credentials inside the job container |
| velnor-legacy | As reference code with tests: broker and run-service client and its error taxonomy, the scale-set client, the trust-scope model, the per-slot lesson of its mbx store; its incidents as regression tests (rerun log ownership, artifact scoping, silent supervisors, idle CPU, a poisoned persistent store); its expression engine as a conformance corpus | The 37k-line executor and runner files, the Docker socket proxy, BuildKit handling, the slot supervisor fleet, Firecracker until phase 5 |
| actions/runner | The protocol as ground truth; `Runner.Worker` as the executor; the action archive cache (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, `ActionManager.cs:1232`); the first-renewal rule and force-fail path | One Listener process and one pod per job; self-update inside sandboxes (the image carries the version the dispatcher reports) |
| ARC | Acknowledge a message only after the scale decision; demand from assigned jobs; the capacity header; JIT config never in env or argv | Cold pods with `emptyDir` state, cache-blind placement, a dind sidecar in every pod, `minRunners` as the only warm pool |

## 4. Verification ledger

The ledger is the mechanism that removes duplicate work while keeping C1–C7.

### 4.1 Obligations and keys

- The plan (`velnor-ci plan`, from a released version) lists the run's obligations. Each has a
  **definition digest**: what runs and how (command, configuration and features, profile, nextest
  profile and overrides, helper version, toolchain, runner image, sandbox class).
- **Exact-tree key** (phase 3): Git tree hash + definition digest. Two runs with the same key
  execute the same thing on the same bytes.
- **Closure key** (phase 4, stretch): for a test obligation, the digest of the test binary
  (output-addressed: whatever the build produced) + the digests of its declared runtime inputs + the
  definition digest. For compile obligations cargo's own fingerprints already play this role inside
  a target snapshot.

### 4.2 Receipts and scopes

A receipt binds a key to a result, the run and attempt that produced it, the sandbox class and node,
and the observing agent's signature. A receipt expires after 7 days, which bounds how long a
time-dependent failure can hide. Scopes decide who may reuse it:

| Scope | Written by | Reusable by |
|---|---|---|
| `tree/<hash>` | the agent of any run that executed exactly that tree | any run of the same tree; the code that produced it is exactly the code being verified |
| `pr/<number>` | the agent of that pull request's runs | that pull request's later pushes and its merge-group candidates |
| `trusted` | the warmer for landed `main` commits; a merge group's receipts once its SHA is on `main` | every run of the organisation except forks |
| fork | nothing | nothing |

### 4.3 Where reuse is allowed

| Situation | Phase | What runs | Status |
|---|---|---|---|
| Push to `main` whose SHA passed its merge group | 1 | release and publication duties only | committed |
| Merge-group candidate tree equals a tree the PR run verified | 3 | plan and verdict; obligations hold by `tree/` receipts | committed |
| Re-run of a failed job on the same tree | 3 | only the obligations without a passing receipt | committed |
| Merge-group candidate differs, test obligation's enforced closure unchanged | 4 | nothing for that obligation | stretch, gated by moonshot M4 |
| Second push to a pull request, closure unchanged | 4 | nothing for that obligation | stretch, M4 |
| Any obligation in C5 | all | always runs | — |

**Enforcement for closure reuse.** A test binary is eligible only when it passes inside a mount
namespace that contains its binary, its declared inputs, the toolchain runtime and a private `/tmp`,
with no network. An undeclared read finds no file and fails the test, so the binary becomes
ineligible until its inputs are declared. Landlock is not used, because it cannot restrict `stat` or
`access`. nextest's run-wrapper hook, which velnor-new already uses for Docker tests, hands each
test to the agent's test executor, which starts it inside that namespace: an unprivileged pod under
the default seccomp profile cannot create the namespace itself, and C4 wants the agent to observe
the result anyway.

### 4.4 Audit

The audit sampler (C6), the nightly and every-20th-merge full runs, and the file-read audit (phase
3: the agent records each build job's file reads with fanotify and reports reads no declaration
covers) each produce a non-blocking check. A disagreement turns that reuse class off for the
organisation and opens an issue; it never turns a red result green. If the ledger cannot be reached,
every obligation executes: a missing receipt is slow, never wrong.

## 5. Cache system

### 5.1 Tiers

| Tier | Where | Holds | Access |
|---|---|---|---|
| L0 | the job pod's directory on the node's XFS volume | the job's own clones and outputs | read-write by the job |
| L1 | node store on the same XFS volume | toolchains, tools, crate sources, Git mirror, action archives, target snapshot generations, PR scopes | read-only mounts and reflink clones made by the agent |
| L2 | one store and ledger service per site | receipts, test binaries and archives, reports, snapshot seeds for new nodes | `velnor-ci` through the agent socket |
| L3 | GitHub's cache | only for workflows running on GitHub-hosted runners | as today, within the 10 GB budget |

Nothing on the job path reads across sites synchronously. Sites replicate receipts and seeds in the
background.

### 5.2 What is cached where

| Item | Tier | Key | Writer | Expected hit rate | Basis |
|---|---|---|---|---|---|
| Rust toolchain and components | L1, read-only | toolchain file digest | node provisioner | ≈ 100% | [M] 0 of 58 restored today because it installs per job into `runner.temp` (`velnor-runners/state/RESULT.md:115`); bumps are provisioned when `main` changes the pin |
| mise tools | L1, read-only | tool pins | node provisioner | ≈ 100% | [M] tool setup 10 s median, 18 s p90 per job today (§A T9b) |
| Crate sources | L1, read-only; crates mirror on miss | `Cargo.lock` checksums | warmer; mirror | ≥ 99% of crates | [M] 10.5% of PRs touch a lockfile (`proposals/fable-5.2.md`); mirror fetch 2.7–3.3 s (§B.4.2) |
| Git objects | L1 mirror | — | agent on push webhooks | ≈ 100% except objects pushed seconds earlier | unmodified `actions/checkout` borrows objects through alternates |
| Action archives | L1, read-only | `owner_repo/<sha>.tar.gz` | provisioner | 100% for pinned actions | [M] about 4.4 s per job today (`proposals/opus-5.5.md`) |
| Target snapshots of `main` | L1, one generation per `main` commit per root | root, toolchain, profile, features, lock digest, image | warmer | ≥ 95% of units fresh on PRs without a lockfile or toolchain change | [M] plain cargo compiled 0 crates on unchanged trees in 3 of 3 runs, mbx 10–68 (decision log I036) |
| PR scope | L1 on the node of the PR's last run | pull request, root | agent, from the post-build snapshot | hit when the next push lands on that node | [E] placement prefers that node; a miss costs only the PR's own delta |
| Test binaries and archives | L2 | content digest | agent | — | [M] velnor-new's nextest archive is 1.48 GB; GitHub upload took 36–43 s (I045) |
| Receipts | L2 ledger | obligation key | agents | exact tree: same-SHA push 100%; merge groups 0–18% today [M] and higher as PR runs shorten [E]; closure: 50–88% of merge-group test obligations [E] | [M] decision log, (e); [E] 88% from a package-level replay of 97 merge groups (`proposals/opus-5.5.md`) |

### 5.3 Trust and promotion

- **Compile cache.** The project's own experiment chose plain cargo over mbx: on unchanged trees
  cargo compiled nothing and every mbx setting recompiled 10–68 crates (decision log I036, I110).
  Velnor keeps that choice and removes its one weakness at velnor-new's size, the 44–61 s download
  of a 1.92 GB entry: a reflink clone on the node replaces the download. `velnor-ci inputs restore`
  keeps restoring source mtimes only for files whose Git blob is unchanged, so cargo's own
  fingerprints stay the authority on freshness.
- **Snapshots are immutable.** The warmer builds a new generation from the previous one on every
  `main` advance, for the roots the merge selected, and freezes it; that is an incremental build at
  batch priority on each node. Jobs get clones; nothing writes a frozen generation.
- **Post-build harvest.** The agent snapshots a job's target directory when the build phase ends and
  before any test runs, so a test that writes into `target/` cannot poison a later run. That
  snapshot becomes the PR scope.
- **Promotion.** Only the warmer writes the trusted scope. A merge group never writes it, because a
  merge group may never land. A merge group's receipts become trusted when its exact SHA is on
  `main`.
- **Forks** read nothing and write nothing on the cluster.
- **The GitHub cache path** stays correct for hosted runners: cache paths become fixed absolute
  paths, because today `${{ runner.temp }}` differs between hosted and ARC runners and the cache
  lookup filters on a version derived from the path, which is why sources missed 48 of 48 times
  (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`, `.github/workflows/ci.yml:337`).

### 5.4 Eviction and budgets

- Per node and organisation: the last three `main` generations per root, PR scopes until the pull
  request closes or three idle days, then least-recently-used. Clones share extents with their
  generation, so a PR scope costs only its changed blocks.
- Budget: 40% of the node's data volume across all organisations, alert at 70% of the volume. The FI
  workers have one 3.5 TB data disk each (§B.1.4), the DE workers 32–50 TB.
- Sizes: a runner-workspace target is 5.9–7.1 GB and the root one 9.0 GB [M,
  `proposals/fable-5.2.md`]; a velnor-new job grows the disk by 14.7 GB [M, decision log I036].

## 6. Workflow design

### 6.1 Shape

- `velnor-gen` writes the workflow (decision log I056): one plan job (gate class), one job per Cargo
  root, and one per configuration where it differs (`velnor-runner-host` with `local-mode`); after
  the move to one root workspace, one job with package-level selection inside it.
- Inside a root job: build test binaries once per configuration, then run format, lint,
  documentation, the "check alone" steps and tests as parallel lanes. The generated workflow runs
  these steps one after another today; the Worker already supports background steps.
- `Required` reports the verdict; the non-blocking `Cache` and `Selection` checks report cache
  failures and undeclared reads. Until the owner changes the ruleset, the workflow emits both
  required names.

### 6.2 Change-aware selection that keeps confidence

- Selection follows `docs/goals.md` design rule 2: the Cargo package graph plus declared inputs,
  with unknown files selecting everything.
- Build scripts declare their inputs (`rerun-if-changed`); today every crate with a build script is
  forced, which is why the heavy jobs ran in 97.4% of PR runs (decision log I068).
- Merge groups select by the change against the queue base only after the owner decides (section 13,
  question 2), the file-read audit runs, and two weeks of shadow plans show zero selection misses
  against full runs.
- A scheduled full run (nightly) and every 20th merge select everything.

### 6.3 Tests

- **Break the floor first.** The 40-case and 6-case loops become one test per case; journal tests
  clone a pre-migrated template database instead of replaying migrations; the schema validation runs
  once per open. Committed: longest test ≤ 60 s (phase 1). Stretch: ≤ 15 s (phase 2).
- **A test-time budget** check is non-blocking at first and blocks new tests over 30 s from phase 2;
  the limit falls with the floor.
- **Shard only by arithmetic.** A suite is split only when its summed test time divided by the
  slot's threads exceeds its longest test plus the cost of moving binaries (I067).
- **Test granularity** (phase 4, stretch): `velnor-ci test` hands the run's test binaries and case
  list to the agent's test executor, which runs cases across sandboxes on other nodes of the same
  site, longest first, and checks that every expected case reported exactly once. GitHub still sees
  one job; there is no extra job start per shard.

### 6.4 No double or triple runs

| Event | Today | Phase 1 | Phase 3 | Phase 4 |
|---|---|---|---|---|
| PR push | full selected set | same | same; work held by receipts is skipped | closure receipts too |
| Merge group | everything | everything | everything not held by `tree/` receipts | closure receipts; candidates pre-verified (M5) |
| Push to `main` | everything again | release duties only when the SHA passed its merge group | same | same |
| Direct push to `main` (no queue) | everything | everything | everything | everything |

### 6.5 Merge queue

| Setting | Today | Phase 0 | Phase 3 |
|---|---|---|---|
| `max_entries_to_build` | 2 | 4 | 8, once false failures are ≤ 2% per run |
| Grouping, `max_entries_to_merge`, `min_entries_to_merge` | HEADGREEN, 10, 1 | same | same |
| `min_entries_to_merge_wait_minutes` | 5 | unchanged: it never applies when the minimum is 1 [M: enqueue to merge-group run 19 s] | same |
| `check_response_timeout_minutes` | 90 | 90 | 20 |
| Auto-merge | 0 of 40 PRs | agents enable it when they open a PR | same |
| Catch-up merges of `main` into PR branches | common | stopped | stopped |

### 6.6 Minimum honest time to verdict

The median pull request touches only `generator/` or `ci-helper/` (47%); 25% touch only the runner
workspace; 8.5% only the root engine crates [M, 200 merged PRs, `proposals/fable-5.2.md`].

**Phase 3, committed** (median PR):

| Segment | Time | Basis |
|---|---|---|
| Push → offer at the dispatcher | 5–10 s | [E] not measured; the dispatcher measures it from push webhooks |
| Plan job: start, checkout from the mirror, plan | 10 s | [E] 3 s start, 7 s work |
| Root job start, clone, build | 15 s | [E] 3 s start, 2 s clone (measured in work item 6); [M] warm rebuild 8.1–8.8 s |
| Lanes: tests, lint, docs in parallel | 55 s | [M] `velnor-ci`'s 781 tests take 55 s on one thread, which its nextest config forces today |
| `Required` job | 10 s | [E] |
| **Total** | **about 1.6 min**; committed **≤ 2.5 min** | |

Runner-workspace PRs: about 4 min [E: 35–99 s to rebuild `velnor-runner-host` after a one-file edit
(`proposals/fable-5.2.md`) plus about 2 min of tests once the loop is split]. Engine PRs stay at 5–6
min until the image-publication fixture (89 s) and its suite are fixed.

**Phase 4, stretch** (median PR): push → offer 8 s, plan 5 s, hot start 2 s, clone 2 s, build 10 s,
lanes 12 s (the 781 tests on 8 threads), verdict posted by the Velnor App 3 s: **about 42 s**.

**Push-to-merge** adds: auto-merge enqueue (seconds), enqueue → merge-group run 19 s [M], the
merge-group run, and merge-group finish → merged 26 s [M]. With an unchanged base the merge-group
run executes nothing (receipts) and takes about 30 s [E]. With a moved base, phase 3 re-runs the
PR's selected obligations on the new tree, which takes about as long as the PR verdict. A moved base
is the common case: none of 40 merge groups on the worst measured day and 11 of 60 (18%) on two
quieter days had the base the PR's last green run had tested (decision log, (e)). Median PR in phase
3: about 3.3 min with an unchanged base and 4.2 min with a moved one [E], which is why the committed
P50 is 6 min. That target assumes merge groups select by change (section 13, question 2); with full
merge-group runs it is 8 min. Phase 4 removes most of the second verification through closure
receipts (M4) and speculative candidates (M5): about 2 min [E].

## 7. Kubernetes

- **Role.** k3s stays as the fleet layer: it deploys the dispatcher, node agents, site services and
  mirrors, accounts for every pod's resources, and enforces quotas, priorities and network policy.
  It is not the job scheduler of record: kube-scheduler cannot see a job before GitHub assigns it.
- **Pod lifecycle.** Each node agent keeps single-use pods of each class created and idle on its
  node. A pod runs one job and is deleted with a UID precondition. Creating a pod takes about 6 s
  [M, 436 pod starts, `proposals/opus-5.5.md`], and it happens before the job arrives. A pod is
  never reused: a job can leave processes, images and configuration behind, and a persistent store
  was poisoned this way in velnor-legacy (§E.6).
- **Warm slots.** Per worker: 4 idle gate pods, 2 idle build pods with at most 4 running, 1 idle
  docker pod on each tainted docker node; the agent resizes pools from the last hour's arrivals. An
  idle pod costs about 45 MiB (§B.2.2).
- **Sizes.** Build: request 16 CPU with no CPU limit, memory 32 GiB request equal to limit (the
  heaviest observed job peaked at 15.7–16.8 GiB, §B.2.2 and decision log I036). Docker: 8 CPU plus 4
  for dind, 24 GiB. Gate: 1 CPU, 2 GiB. Cargo and nextest parallelism follow the pod's CPUs, not a
  constant.
- **Node settings.** Kube and system reservations (3 CPU, 6 GiB), a `memory.available` eviction
  threshold of 2 GiB, `podPidsLimit` 16384, the kubelet root directory on the XFS data volume (today
  `emptyDir` lands on the ext4 OS disk, `velnor-runners/PLAN.md:45`), topology labels for site.
- **Priorities.** `velnor-merge` > `velnor-gate` > `velnor-pr` = other organisations' CI >
  `velnor-batch` (warmer, speculation, scheduled full runs). Only `velnor-batch` is preempted.
- **Untrusted pull requests.** Fork pull requests run on GitHub-hosted runners; the dispatcher fails
  any fork job that reaches a Velnor scale set before a pod exists. Build pods get a default-deny
  NetworkPolicy with an allow-list, no service-account token and no host paths except what the agent
  mounts read-only.
- **Both sites.** One dispatcher leads, a standby in the other site takes over by Kubernetes Lease.
  Each site has its own store and ledger service. A run stays in one site. The tainted docker pool
  has one node per site.
- **Other organisations.** ChainArgos and jackin keep their ARC scale sets until they choose to move
  (phase 5). They get a guaranteed CPU quota, separate namespaces and separate stores; no store,
  receipt or snapshot crosses an organisation.

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
| 7 | Node stores, warmer and reflink target snapshots | From 46–278 compiled crates to about 0 on unchanged trees; restore 44–61 s → ≤ 2 s | [M] decision log I036; [E] clone time | Stale snapshot means a slow build, not a wrong one | Disable the mount |
| 8 | Crates mirror through environment variables | 1–3 s per job, no internet dependency | [M] §B.4.2 | Mirror outage | Unset the variables |
| 9 | Site labels and run stickiness | Avoids 35 ms round trips for artifacts | [M] §B.5 | Unbalanced sites | Remove the preference |
| 10 | Debuginfo: line tables for workspace crates, none for dependencies | −10 to −24% cache size; −20 to −27% CPU on a runner-host rebuild | [M] decision log I108; `proposals/opus-5.5.md` | Poorer backtraces | Profile setting |
| 11 | CPU governor or EPP to performance | 2–10% | measure first: not readable without a shell (§B.1.6) | Power | Revert |
| 12 | Same-site network at 10 Gbit/s | Needed for moonshot M6 | measure first: NIC speed unknown | Cost | — |
| 13 | Static CPU manager on one node | 0–15% | measure first | Loses the burst cold builds need | Policy back to `none` |
| 14 | THP and dirty-writeback settings | credit 0 | measure first | Compaction stalls | Revert |
| 15 | mold | small | measure first: Rust ≥ 1.90 already links with rust-lld | Link bugs | Remove |
| — | Striping the data volume; tmpfs work directories | rejected | FI workers have one data disk; reflinks cannot cross into tmpfs and the slow tests are CPU-bound (decision log I099, I100) | | |

SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 9. Moonshots

Each moonshot states what must be true, the design element that attacks it, and the cheapest
experiment that proves or kills it. None is dropped for being hard; one is dropped only when its
experiment shows it cannot keep C1–C7.

| Id | Moonshot | Phase | Status |
|---|---|---|---|
| M1 | PR verdict under 60 s for the median PR | 4 | stretch |
| M2 | Push-to-merge under 5 min for the median PR | 3 (≤ 6 min), 4 (≤ 4 min) | committed / stretch |
| M3 | Zero cold start: assignment to first step ≤ 1 s | 3–5 | stretch |
| M4 | Zero duplicate verification across PR, merge queue and `main` | 1, 3, 4 | committed for SHA and tree; stretch for closures |
| M5 | Speculative merge queue | 4 | stretch |
| M6 | Test-granularity scheduling over all 480 threads | 4 | stretch |
| M7 | Break the 200–274 s single-test floor | 1–2 | committed ≤ 60 s; stretch ≤ 15 s |
| M8 | Verification that begins before the push | 5 | stretch |
| M9 | Zero queue wait | 0–3 | committed ≤ 5 s p90; stretch ≤ 1 s |

**M1. PR verdict under 60 s.** Must be true: GitHub turns a push into an offer in ≤ 10 s; a warm job
starts in ≤ 2 s; the median PR rebuilds in ≤ 10 s; its tests finish in ≤ 15 s; the verdict is posted
without another job. Attacked by: the dispatcher with hot pods, node snapshots, parallel lanes,
parallel `ci-helper` tests, and the Velnor App posting `Required`. Cheapest experiment: from phase
2, run one generator-only PR end to end on a hand-prepared warm pod and time every segment; read one
day of push-to-offer latency from the dispatcher's webhook log. If GitHub's own latency is the
larger term, the dispatcher starts the plan and build on the push webhook, computing the merge tree
itself, and the jobs find `tree/` receipts when they arrive (N09). Killed if push-to-offer P50
exceeds 20 s even then; the target becomes 90 s.

**M2. Push-to-merge under 5 min.** Must be true: M1 or close to it, auto-merge always on, and a
merge group that re-executes almost nothing. Exact trees alone do not get there: 0 of 40 merge
groups on the worst measured day and 11 of 60 on two quieter days had the base the PR run had tested
(decision log, (e)), because `main` moves while a PR waits. Attacked by: shorter PR runs (a smaller
window for `main` to move), merge-group selection, closure receipts (M4) and speculative candidates
(M5), plus merge-group-first admission and depth 8. If the second verification is still the largest
term after that, the next experiment is "PR-light, queue-full" (N15): PR pushes run the fast
obligations and the merge group, still the gate, runs the rest. Cheapest experiment: work item 3
records the tested tree of every run; from phase 1 on, measure how the share of merge groups with an
already-verified tree grows as PR runs shorten. Committed ≤ 6 min in phase 3 (with merge-group
selection), stretch ≤ 4 min in phase 4.

**M3. Zero cold start.** Must be true: a hot Worker accepts a message and starts the first step in
under 1 s; clones are metadata-only. Attacked by: pre-created pods, the hot Worker, read-only
stores, reflink clones; later, microVMs restored from a snapshot for the fork and docker classes.
Cheapest experiment: in a velnor-lab pod, start `Runner.Worker spawnclient` early and time message
to first step against a cold start, 50 runs each. Killed if the hot Worker saves under 1 s;
pre-created pods stay either way.

**M4. Zero duplicate verification.** Must be true for the closure stage: test binaries are
bit-identical across nodes for identical inputs at fixed paths; at least 80% of test binaries pass
under mount-namespace enforcement with declared inputs; the audit finds no disagreement in 1,000
reuses. Attacked by: the ledger with output-addressed keys and enforced inputs. Cheapest experiment:
build one commit twice on two nodes and compare test-binary digests; run each root's suite under the
enforcement wrapper and count failures; replay 200 merges to count how many merge-group test
obligations had the same closure as the PR run. Killed if digests differ for reasons that cannot be
pinned, or enforcement fails most suites.

**M5. Speculative merge queue.** Must be true: Velnor computes the same candidate tree GitHub will
build, early enough to matter. Attacked by: when `main` moves while a pull request is open with
auto-merge, Velnor verifies `main ⊕ PR` at once, at batch priority, and stores `tree/` receipts;
GitHub's later merge group finds them. A wrong guess wastes idle capacity and never changes a
verdict. Cheapest experiment: two weeks of shadow prediction with no execution, comparing `git
merge-tree` output on the mirror with every merge group's tree. Killed if fewer than half of the
predicted trees match.

**M6. Test-granularity scheduling.** Must be true: some suite's summed test time divided by one
pod's threads exceeds its longest test after M7, and binaries move within a site in ≤ 5 s. Attacked
by: the cluster test executor (section 6.3). Cheapest experiment: one week of per-test CPU-seconds
from nextest's JSON output, plus `iperf3` between same-site workers. Killed if no suite's summed
time exceeds its longest test by more than the transfer cost; tests then stay inside their job's
pod.

**M7. Break the single-test floor.** Must be true: the slow cases are independent, and journal
set-up dominates their time. Attacked by: one test per case, a pre-migrated template database,
linear schema validation, the test-time budget; optionally dependencies at `opt-level = 3` in tests,
measured at −66% CPU on the slowest class (owner decision). Cheapest experiment: the split and the
template on a branch, timed per test on a worker. The 15 s stretch is killed if the cases share
state that cannot be separated; the floor is then the slowest case.

**M8. Verification that begins before the push.** Must be true: agents spend time between "code
done" and "pushed" that verification can overlap, and Velnor can verify the tree they will push.
Attacked by: a preflight call that sends the agent's commit to the Git mirror and verifies `main ⊕
commit` on the cluster, producing `tree/` receipts the PR run reuses; later, agent workspaces on the
cluster sharing node stores. A test run on a developer's Mac is not reusable: it is a different
platform (arm64 macOS against linux/amd64), and its key belongs to the agent. Cheapest experiment:
count how often agents run tests before pushing and for how long, then route those runs through
preflight on velnor-lab and measure the PR run's receipt hit rate. Killed if agents rarely test
before pushing; the PR run is then already the earliest point.

**M9. Zero queue wait.** Must be true: advertised capacity covers peak demand for merge and gate
jobs. Attacked by: the capacity header, merge-group-first acquisition, and preempting batch work.
Cheapest experiment: before phase 3, replay the busiest measured hour's job arrivals against the
class capacities in section 7. From phase 3 the dispatcher measures offer to job start directly.

## 10. Phases

| Phase | Scope | Exit gate (measured on velnor-new over at least 50 merges) | Expected saving |
|---|---|---|---|
| 0, week 1 | Gate jobs on the cluster; narrow the image-candidate trigger; Source qualification on the cluster or only in the queue; auto-merge; no catch-up merges; depth 4; fixed cache paths; `event.json` race fix; push-to-merge, queue and flake instrumentation; host facts read by the owner | push-to-merge P50 ≤ 25 min, P90 ≤ 60; hosted gate queue on the critical path p90 ≤ 1 min | 5.8–57 min of gate queue per run; 7.5 min median enqueue delay; twice the queue throughput |
| 1, weeks 2–3 | Loop splits and journal fixes; `-j` caps removed; gate, build and docker classes under ARC with warm pools and honest sizes; node tools and action archives; crates mirror; push run reduced to release duties; flake budget machinery; tested-tree recording | PR verdict P50 ≤ 7 min, P90 ≤ 14; push-to-merge P50 ≤ 15, P90 ≤ 35; runner-min per merged PR ≤ 150; false failures ≤ 3%; longest test ≤ 60 s | About 62 runner-min per merge from the push run; 14–18 s per job of setup; 3–4 min off the zero-queue floor |
| 2, weeks 3–6 | `velnor-gen` cutover with parity; kubelet on XFS; node stores, warmer, reflink snapshots; state seal and mbx retired; test-time budget; journal template | PR verdict P50 ≤ 3.5 min, P90 ≤ 8; push-to-merge P50 ≤ 9, P90 ≤ 20; ≥ 95% of units fresh on PRs without lockfile changes; first cargo ≤ 15 s after assignment | Cold compiles of 196–497 crates per job become deltas; 24 of 29 per-job overheads disappear with one job per root |
| 3, weeks 6–12 | Dispatcher and node agent replace ARC for tailrocks; pre-created pods; per-job placement; PR scopes; ledger with `tree/` receipts; node-held leases; fork refusal; file-read audit; merge-group selection if approved; depth 8 | PR verdict P50 ≤ 2.5 min, P90 ≤ 8; push-to-merge P50 ≤ 6 (8 without merge-group selection), P90 ≤ 15; Velnor-side wait p90 ≤ 5 s; first cargo command ≤ 5 s after assignment; false failures ≤ 1%; runner-min per merged PR ≤ 40 | Merge groups re-run only the PR's selected obligations, or nothing on a verified tree |
| 4, weeks 12–20 | Moonshots M1, M4–M6: closure receipts, speculative candidates, cluster test executor, `Required` from the Velnor App, receipt short-circuit, push-webhook start | stretch: PR verdict P50 ≤ 60 s; push-to-merge P50 ≤ 4 min, P90 ≤ 10 | The second verification of unchanged obligations disappears |
| 5, after week 20 | Preflight and agent workspaces; native executor if its gate passes; microVM fork and docker classes; other organisations on Velnor | stretch: verdict ≤ 20 s after a preflighted push; push-to-merge P50 ≤ 2.5 min | Verification overlaps the agent's own work |

A phase that misses its gate does not start the next one until the miss is explained. Each phase
keeps a working fallback: hosted labels in phase 0, ARC until four weeks after phase 3.

## 11. The first ten work items

1. **Gates on the cluster.** Scope: the released generator version that moves Plan, Ledger CLI, the
   lints and both required jobs to a `velnor-gate` scale set with warm pods and no dind; the
   image-candidate workflow triggered only where it is needed; Source qualification on the cluster
   or only in the merge queue. Acceptance: over 50 PR runs, hosted queue on the critical path p90 ≤
   1 min and no CI job queued on a hosted label. Depends on: nothing.
2. **Queue and agent procedure.** Scope: agents enable auto-merge when they open a pull request and
   stop catch-up merges; `max_entries_to_build` 4; required checks bound to the GitHub Actions
   integration. Acceptance: ≥ 90% of merged PRs have auto-merge; green → enqueue P50 ≤ 30 s. Depends
   on: owner questions 5 and 6.
3. **Measurement.** Scope: a push-to-merge collector from PR timelines; queue residence and merges
   per hour; per-job spans; the tested tree hash and obligation digests in every plan's output; the
   paired-rerun flake sampler. Acceptance: a daily report with P50, P90 and denominators that
   reproduces the 85-min baseline from past data. Depends on: nothing.
4. **The single-test floor.** Scope: one test per case for the two looped tests; the journal schema
   validation and template fixes, each measured; `ci-helper` tests in parallel with timeouts sized
   to the pod; the `event.json` snapshot before parallel steps. Acceptance: longest test ≤ 60 s on a
   worker; runner-host and actions-cli test steps ≤ 90 s P50; no new false failures in 100 paired
   reruns. Depends on: nothing.
5. **Honest classes under ARC.** Scope: gate, build and docker scale sets; the sizes in section 7;
   kubelet reservations, eviction and PID limits; priority classes and per-org quotas; the `-j` caps
   removed; toolchain, tools and action archives as read-only node mounts. Acceptance: job-duration
   p90/p50 ≤ 1.3 at peak; tool setup ≤ 2 s; no OOM kills over a week. Depends on: 1.
6. **Storage spike.** Scope: on one drained worker, move the kubelet root to the XFS volume, confirm
   `reflink=1`, time reflink clones of 6–9 GB targets, and compare cold, warm and one-file-edit
   cargo runs for each velnor-new root. Acceptance: clone ≤ 2 s P50; an unchanged tree compiles 0
   crates; numbers recorded with raw data. Depends on: 5.
7. **No third run.** Scope: the push run checks for a green merge-group run on the exact SHA and
   then does only release and publication duties; otherwise it runs everything. Acceptance: ≤ 2
   runner-min per matching push; the release gate on `main` still passes. Depends on: 3.
8. **`velnor-gen` cutover with parity.** Scope: one job per root with parallel lanes, the local-mode
   configuration, the maintenance checks, both required names, the Velnor classes in `runs-on`,
   node-store cache steps; one week of shadow runs beside the engine comparing obligation
   inventories. Acceptance: empty inventory difference over 100 runs; PR verdict P50 ≤ 3.5 min.
   Depends on: 4, 6.
9. **Dispatcher on velnor-lab.** Scope: `velnor-proto`, `velnor-dispatch`, `velnor-node` and
   `velnor-sandbox` running the official Worker in pre-created pods, with node-held leases, cancel
   and force-fail; conformance through the velnor-local scenarios and a canary every 10 min.
   Acceptance: 1,000 lab jobs with 0 stranded and 0 executed twice; assignment to first step ≤ 3 s
   P50; killing the dispatcher mid-job loses nothing. Depends on: owner question 1.
10. **Ledger with exact-tree receipts.** Scope: agents write `tree/` receipts; `velnor-ci` plan and
    gate read them; the audit sampler. Acceptance: on merge groups whose tree equals a verified
    tree, the run executes no obligation; 0 disagreements in 500 sampled re-executions. Depends on:
    3, 8.

## 12. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A receipt or a selection gives a false green | Audit sampler disagreement; nightly and every-20th-merge full runs; undeclared reads in the file-read audit | That reuse class turns off and work runs again; receipts expire after 7 days; the bad merge is reverted by a normal PR |
| GitHub changes the broker or run service, or deprecates the runner version | Canary job every 10 min per path; a version tracker against GitHub's deprecation dates | Switch labels to the ARC scale set; the dispatcher reports the version of the Worker in the image |
| Dispatcher or agent bugs strand jobs | Jobs running on GitHub with no lease record; renewal failures | Node-held leases; force-fail; ARC label fallback |
| False failures rise as jobs pack denser | Paired-rerun sampler; nextest flaky counts per test | Lower the per-node build cap; queue depth back to 4; quarantine with sign-off |
| A PR poisons a cache another run trusts | Receipts record producer and scope; promotion only by the warmer | Delete the generation; jobs build cold, slowly and correctly |
| FI workers run out of disk | Store budgets and alerts at 70% | Least-recently-used eviction, per-org caps |
| Other organisations starve, or starve velnor-new | Velnor-side wait per organisation | Quotas and priorities; preempt only batch work |
| The new runner grows like velnor-legacy | Line budgets checked in CI; phase gates | Stop and review at a budget; ARC and the official runner keep working at every phase |
| The `velnor-gen` cutover drops a configuration or check | Shadow inventory comparison | Keep the engine workflow until the inventories match |
| Speculation or warming starves real work | Batch-class CPU share | Lowest priority, preempted first |

## 13. Owner questions

1. **`docs/goals.md:16` says jobs are always executed by GitHub's official runner.** Phase 3 keeps
   the official `Runner.Worker` for every step but replaces `Runner.Listener` with the dispatcher.
   Phase 4's receipt short-circuit completes a job with no runner process at all, for example a
   merge-group job whose obligations all passed on the same tree a minute earlier finishes in 1 s
   without a pod. A native executor in phase 5 would run steps without the official runner.
   Recommendation: amend line 16 to "every step that runs, runs in GitHub's official
   `Runner.Worker`; a job may instead be completed from a signed receipt of an identical verified
   execution"; decide the native executor separately when its gate passes.
2. **Hard decision 3 runs merge groups and pushes in full.** `docs/goals.md:86` makes "what the
   change selects" the generator's default. Example: a docs-only pull request's merge group runs
   every job today (64.6 runner-min median for a merge group, §A T2); with selection it runs the
   plan and the verdict. Recommendation: choose selection for merge groups once the file-read audit
   exists and two weeks of shadow plans show no miss, keeping the nightly and every-20th-merge full
   runs; a push to `main` on a verified SHA does release duties only, and a direct push still runs
   everything.
3. **Receipts change what "ran and passed" means** (`docs/goals.md` "Green means verified").
   Example: a merge group whose tree equals the PR's verified tree finishes in about 30 s and
   executes nothing. Recommendation: approve exact-tree receipts now; decide closure receipts after
   moonshot M4's experiment.
4. **Stay on the Free plan?** The org's plan is `free` (20 concurrent hosted jobs). After phase 0 no
   job on the merge path runs on hosted runners; forks and macOS release builds still do.
   `docs/goals.md:45` keeps the generator inside free limits, and the cluster tiers do not change
   what hosted runners need. Recommendation: stay on Free; buy a paid plan only if fork volume or
   release builds queue, measured.
5. **Agent procedure.** Example: 0 of 40 recent merged PRs enabled auto-merge and the median PR
   waited 7.5 min between green and enqueue. Recommendation: agents enable auto-merge when they open
   a pull request, stop merging `main` into their branches, and later call preflight before pushing.
6. **Ruleset changes.** Bind `Required verdict` and `Required` to one integration now (anything can
   post them today); move `max_entries_to_build` to 4 now and 8 in phase 3; accept a single
   `Required` name at the `velnor-gen` cutover; let the Velnor App post `Required` in phase 4.
   Recommendation: yes to all four.
7. **The Kubernetes runtime inside `velnor-runner-host`.** The proposal under review puts the
   cluster runtime behind a trait in `velnor-runner-host`. This design builds it as new crates
   instead: that crate is 157k lines with about 60 open correctness issues and sets the CI critical
   path, and its unit places the runner's JIT credentials inside the job container. Recommendation:
   keep `velnor-runner-host` for the Mac and local mode, and build the cluster runtime separately on
   the shared wire types.
8. **Test profile.** Building the runner workspace's dependencies at `opt-level = 3` in tests cut
   the slowest test class's CPU by 66% [M, `proposals/fable-5.2.md`]. It changes how dependencies
   execute in tests; debug assertions and overflow checks stay on. Recommendation: run it as an
   experiment under design rule 14.
9. **Other organisations.** Should ChainArgos and jackin share velnor-new's priority for PR jobs,
   with a guaranteed quota, and when do they move off ARC? Recommendation: equal PR priority with
   quotas now; offer Velnor in phase 5.
10. **Hardware.** Rent one high-clock node for a month (section 14)? Recommendation: yes, after
    phase 2, routed only runner-workspace jobs.

## 14. What more hardware would buy

| Purchase | Buys | When |
|---|---|---|
| One high-clock node (desktop-class cores at 5.5 GHz or more) | 1.3–1.5× on the serial path: rebuilding `velnor-runner-host` and the longest tests [E] | After phase 2, rented for a month first |
| 10 Gbit/s between same-site workers | Moonshot M6 and site-store replication; 1.48 GB of test binaries in about 1.2 s instead of 12 s [E] | When M6's experiment shows a suite that needs it |
| Two more k3s server nodes | A control plane that survives one node; no speed | Phase 4 |
| More NVMe on the FI workers | Cache headroom on the 3.5 TB data disks | When eviction starts dropping live generations |
| More EPYC workers | +96 threads each, about +20% capacity | Only if Velnor-side wait p90 exceeds 15 s after phase 3 |

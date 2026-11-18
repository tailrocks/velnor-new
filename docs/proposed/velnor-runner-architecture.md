# Velnor runner architecture

**Status:** Proposal. It replaces nothing yet. Building it needs the owner decisions in section 13,
because parts of it contradict `docs/goals.md` and `docs/hard-decisions.md`.

**Scope:** how Velnor verifies velnor-new's changes on the project's Hetzner k3s cluster so that
waiting for CI stops being the slowest part of delivering a change. The generator (`velnor-gen`) and
helper (`velnor-ci`) are in scope where the runner needs something from them.

**Sources.** Facts are cited from the [evidence pack](../research/velnor-runner-2026-11/evidence.md)
as §A T5 and similar, from the eight panel proposals under
`../research/velnor-runner-2026-11/proposals/`, and from the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea the panel raised
(`I001`–`I120`, `N01` onward) and every hole (`F-H`) and alternative (`F-B`) the design review
found. Every number is tagged **[M]** measured (with its source), **[E]** estimate (with its basis)
or **[T]** target. A target is **committed** when the evidence makes us confident, **stretch** when
an experiment must prove it first.

## 1. Goal and success metric

### 1.1 The metric

The single metric is **push-to-merge**: from the final commit's accepted push to a pull request's
head until the pull request is merged, from the pull request timeline. We report P50 and P90 per
week over every merged pull request, with intervals, for every PR class (generator and helper,
runner, engine, local, no Rust, mixed), and keep superseded pushes, time-outs and open pull requests
visible beside the merged cohort. Beside it: PR verdict time (push to the `Required` check), the
admission span (run created → first job), runner-minutes per merged pull request, the compile-cache
hit rate and the false-failure rate. Three guardrails keep a faster last push from hiding slower
feedback: time to the first actionable failure, first-push-to-merge, and failed attempts per merged
change.

Runner-minutes charge foreground jobs, remote execution slots, cancelled attempts, discarded
predictions, the builder and audits to the repository that consumed them, count each occupied slot
once (one job fanning out to ten one-minute workers spends ten worker-minutes), are split into
completed, cancelled and superseded work, and are published beside idle reserved core-hours,
CPU-seconds and GiB-minutes.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M] 98 merged PRs, `proposals/opus-5.5.md` §1.1 (clock from the first CI run on the final head) |
| Push-to-merge on the worst measured day, P50 | 400 min, of which 278–282 min between queue entry and merge | [M] `proposals/opus-5.5.md`, `proposals/fable-5.2.md` |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M] §A T2 |
| PR run with no hosted queue | about 11 min | [M] 10.9 min over 12 runs (`proposals/fable-5.2.md`); replays 11.4 and 12.0 min |
| Admission span, PR runs | 2 s p50, 226 s p90, 2,203 s max; a run that supersedes a running one waits 60 s p50, 626 s p90 | [M] 200 sampled runs; every delay over 10 s had an unfinished earlier run on the branch (design review F-H06) |
| PR runs started on an unchanged head SHA | 18.3%, most cancelling an identical run in flight | [M] 469 of 2,558, the `edited` trigger (F-H07) |
| Runner-minutes per run (PR / merge group / push) | 52.9 / 64.6 / 62.0 | [M] §A T2 |
| Runner-minutes per merged PR | 285–347 | [E] about 3–4.8 PR runs plus a merge group and a push per merge |
| Full runs per merge | 3 (PR, merge group, push on the same SHA) | [M] §A T7: 120 of 159 green merge groups had a push run on their SHA |
| False failures per full run | 8.1%, one direction | [M] 10 of 124 push runs failed on a SHA whose merge group had passed, all in test steps (decision log, (h)) |
| Cross-job compile reuse on the cluster | 0 | [M] sources cache 0 of 48, toolchain 0 of 58, state 0 of 3,636 (§A T9a, T9b) |

### 1.2 Same confidence: the contract

"Same confidence" is a contract with seven clauses. A change that breaks a clause is rejected,
however much time it saves.

1. **C1. Every tip of `main` is verified.** For every commit that becomes the tip of `main` and
   every obligation the plan lists for it, there is a passing result whose key equals the
   obligation's key at that commit, produced by Velnor's trusted execution in the sandbox class the
   obligation needs. An *obligation* is one unit of verification: format, lint, docs, a per-crate
   check, one test binary under one configuration, or a named check. Under the merge queue's
   HEADGREEN grouping a group's earlier commits land as ancestors of a verified tip without a
   passing result of their own, and a failed entry has landed that way; those commits are listed for
   bisection and release tooling, and owner question 3 offers ALLGREEN instead.
2. **C2. Keys capture every input, by construction.** A key is the hash of what the obligation's
   sandbox contains: the Git tree hashes of its inputs, its definition and its execution manifest.
   The sandbox is a sparse checkout of exactly those paths, so an input outside the key is not on
   disk and a lookup of it fails (section 4.1).
3. **C3. Selection is sound or wide.** The plan comes from a released helper, never from the change
   under test (`docs/goals.md` rule 12). A file the plan cannot place selects everything. Nothing is
   skipped relative to a merge group's predecessor in the queue; an obligation is skipped only when
   a receipt with its exact key exists.
4. **C4. Only what Velnor observed is evidence for another run.** A receipt is minted only from
   processes the node agent started in a sandbox it created, running the released plan's command
   graph on the inputs it mounted, with every exit and the expected test inventory observed. A
   missing, skipped, cancelled, masked or zero-test result fails its obligation. A job's own report
   is never evidence for another run; a lab test checks that a forged passing report yields zero
   receipts.
5. **C5. Uncomparable work always runs, and the sandbox hides what a key does not hold.**
   Obligations that read the network, the event payload, the clock in an assertion, randomness
   without a fixed seed, Docker images not pinned by digest or the commit's identity, or that
   publish anything, run fresh every time. For tests this is enforced, not trusted: a loopback-only
   network namespace and no event file. Build and test sandboxes have no `.git`, a scrubbed
   environment (no `GITHUB_*` variables, fixed `HOME`, `PATH` and locale) and a fixed CPU count per
   class, part of the definition digest. An obligation that reads Git objects, as
   `velnor-runner-host`'s build script does with `.velnor/runner.lock` (`build.rs:69-80`), stays in
   this clause until it reads a declared file.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow with every
   reuse re-executed, and graduates per obligation kind after about 3,000 clean reuses; after that,
   2% of reuses are re-executed. Every landed tree is executed in full once within 24 h on batch
   capacity. The nightly and every-20th-merge runs build from an empty target in a fresh pod and
   compare results and test-binary digests with the warm path, and a cache lineage restarts from
   empty on every lockfile or toolchain change and at least daily. A disagreement is re-run five
   times to separate a flake from a wrong key; a confirmed one retires that key version or lineage
   until it is explained.
7. **C7. Flakes are budgeted** (section 1.3).

A repeat run says nothing about the change for an obligation whose inputs the change did not touch;
C6 and C7 collect what it says about the test. For an obligation whose inputs the change touched, it
does: a regression that fails with probability p per execution is caught by three executions with 1
− (1 − p)³ (49% at p = 0.2, 14% at p = 0.05) and by one with p. Today's three full runs give that by
accident. Velnor gives it on purpose: **every obligation whose inputs the change touched executes
three times in parallel, and any failure blocks.** Those are the only tests that run, so the cost is
small [E: `velnor-ci`'s step is 10 s median, §A T10].

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes. It costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** at most 3% false failures per full required run from phase 1 and 1% from phase 3, each
  claimed only once enough pairs exist. Today's 8.1% [M] is a one-direction discordance rate.
- **Measure:** a paired-rerun sampler re-runs green and red results in both directions under
  representative load, classified by obligation and environment. Nothing is claimed before about 300
  pairs: zero disagreements in 299 give a one-sided 95% bound just under 1%, while 50 merges at a 5%
  sample give 2.5 pairs. The sampler replaces the push run as the denominator.
- **Report and act:** retries stay at 0 in the required profile, so a flaky test fails the run and
  is counted. A test with two false failures in seven days gets an owner-labelled issue; it is
  fixed, or quarantined only when a blocking check covers the same property or under an owner
  exception that expires, and it keeps running visibly.
- **Known causes first:** CPU starvation of tight timeouts, such as the 2 s kill in
  `ci-helper/.config/nextest.toml`'s default profile, which the generated workflow would inherit
  because its nextest commands pass no profile (so it names one), under pods that request 2 CPU and
  burst to 84 cores (§B.2.2); and the `event.json` rewrite race between steps (decision log I023).

### 1.4 Targets

Committed figures with the stretch in brackets, P50 / P90, from the design review's consensus. A
phase's figures hold only if its conditions do. CPU per merged PR charges completed, cancelled and
superseded work and the batch work it caused: the builder, speculation and audits.

| Phase | PR verdict | Push-to-merge | CPU per merged PR | Conditions |
|---|---|---|---|---|
| 0 | ≤ 12 / ≤ 20 min | ≤ 25 / ≤ 60 min (18 / 40) | baseline re-measured | the regenerated workflow is live; admission span p90 ≤ 5 s |
| 1 | ≤ 5 / ≤ 10 min (3.5 / 8) | ≤ 12 / ≤ 30 min (10 / 22) | ≤ 150 runner-min | selection fixed in the engine (about 212 without it, 139 with it [E]); `Journal::open` on a template ≤ 1 s |
| 2 | ≤ 2 / ≤ 6 min (80 s / 4 min) | ≤ 5 / ≤ 15 min (3.5 / 9) | ≤ 60 | one job per run; per-root keys, ≥ 65% of merge groups executing nothing; the cache engine chosen by its experiment; no test over 60 s measured alone |
| 3 | ≤ 90 s / ≤ 4 min (60 s / 2.5 min) | ≤ 3 / ≤ 8 min (1.75 / 5) | ≤ 35 (15) | week-1 experiments passed (webhook under 8 s, closure-only checkout for ≥ 80% of packages, queue-tip match ≥ 70%); ≥ 85% of merge groups execute nothing; ≤ 0.5% of merges ejected by a false failure; eligible-to-start p90 ≤ 2 s for merge and gate work |
| 4 | (30 s / 100 s); ≤ 20 s after a preflighted push | (100 s / 4 min); about 50–75 s if Velnor lands the merge | (≤ 15) | stretch only |

If phase 3's experiments fail, its design still supports about 90 s / 4 min for the verdict and 4 /
10 min push-to-merge. Further committed figures: first cargo command ≤ 15 s after the job starts
(phase 2) and ≤ 5 s after the push (phase 3, stretch 4 s); no test over 60 s (phase 2, stretch 15
s).

## 2. Where the time goes, and what changes the problem

1. **Waiting, not working** [M]. A PR run with no hosted queue takes about 11 min, but the median PR
   waits 85 min from push to merge. On the worst day the three serial gate jobs on hosted runners
   waited 57.0 of a 71.8 min run (§A T5), on the Free plan's 20 concurrent hosted jobs shared with
   two busy non-CI workflows (§A T11). A superseding push waits until the cancelled run's `always()`
   `Required` job has run: up to 2,203 s. The queue builds 2 entries at a time, and no agent enables
   auto-merge (0 of 40).
2. **The same verification three times** [M]. Each merge runs the full job set on the PR, on the
   merge group and on the push of the same SHA (§A T7); the third run found nothing and failed
   falsely 8.1% of the time.
3. **Every job is cold** [M]. Nothing compiled survives a job on the cluster (§A T9a, T9b), and the
   one restore that exists costs more than the build it saves: 44–61 s for a 1.92 GB entry (I036).
4. **Code sets the floor** [M]. Two tests loop over 40 and 6 cases (258–274 s, 203–271 s), and the
   floor behind them is still 125–138 s; `maintenance-helpers` compiles with `build.jobs=2`;
   selection forces every crate with a build script (I059, I061, I068).

The design removes work instead of speeding it up. Gates leave the hosted pool. Verification leaves
GitHub jobs: the push webhook starts it on the cluster, and one witness job reports it. A ledger
with input-addressed keys makes "verify identical inputs once" structural, so a merge group stays a
full verification that is mostly a lookup. Caches live on the node that runs the work, and the slow
tests are rewritten. The Rust built is the part that removes work: the node agent and its executor,
the ledger, the stores and the webhook ingress. Owning job acquisition stays an experiment, because
an idle runner already starts the next job in 2 s.

## 3. Architecture

### 3.1 What stays compatible and what is redesigned

From the outside Velnor is a GitHub App plus GitHub Actions runners. **Unchanged:** workflow YAML
and its semantics, the required checks, rulesets and the merge queue, `GITHUB_TOKEN` and OIDC, and
the runners that execute workflow jobs (ARC scale sets running the official runner within GitHub's
63-day version window, §F.6.1). **Redesigned:** when verification starts (at the push), where it
runs (Velnor sandboxes, not job containers), what is already on that disk, what is skipped because
an identical verified result exists, and how the verdict reaches the job GitHub waits for.

### 3.2 Components

All new code is Rust in the root workspace. Crossing a crate's line budget stops work for a review
(decision log I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| Webhook ingress | `velnor-hooks` | one service per site | Receives the App's push, pull-request and merge-group webhooks, verifies their signature, calls `ensure`, replays missed deliveries | 3k lines |
| Ledger | `velnor-ledger` | library plus one service per site | `ensure` requests, obligation keys, signed receipts, scopes, promotion, audit | 6k lines |
| Node agent | `velnor-node` | privileged DaemonSet on workers | The obligation executor behind `ensure`: pre-created sandboxes, result observation, node stores, the `main` builder, harvest, cleanup by UID | 15k lines |
| Store | `velnor-store` | library plus one service per site | Content-addressed blobs (test binaries, archives, reports, snapshot seeds), replication within a site, eviction | 6k lines |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, obligation commands, the witness step `velnor-ci witness`, the workflow shape | existing |
| Experiment-gated | `velnor-proto`, `velnor-dispatch`, `velnor-sandbox`, `velnor-exec` | not built | Owned job acquisition, a hot Worker, podless job completion, a Worker replacement; built only if the witness leaves more than 5 s per run (section 3.5) | 35.5k lines |

### 3.3 The verification path: `ensure` and the witness

1. **Push.** A push to a pull request reaches `velnor-hooks` as a webhook 1–2 s later [M: GitHub
   creates push-event runs within 1–2 s, `fable-5.2` design review F1]; a merge group's creation
   arrives the same way.
2. **Plan.** `velnor-hooks` calls `ensure(tree, plan)` on the site's ledger, which fetches the
   commit into the node mirror, computes the tree to verify with `git merge-tree` (the head merged
   into its current base, or the merge group's own commit), and runs the released planner to list
   the obligations and their keys.
3. **Lookup.** Each obligation has a passing receipt (done), an execution in flight (join it) or
   nothing (schedule it).
4. **Execute.** The node agent that placement picks runs each scheduled obligation in a sandbox it
   creates: a pre-created single-use pod of the obligation's class, with no `.git`, a scrubbed
   environment, no GitHub token, and the target view from the cache engine (section 5), made after
   the agent writes the sparse checkout so that nothing a checkout cleans can wipe it. The agent
   starts the processes, observes their results, and writes a receipt per obligation as it finishes.
5. **Witness.** The workflow's one job, `Required`, runs on a small gate-class runner. Its step,
   `velnor-ci witness`, calls the same `ensure`, joins the work in flight or starts it if the
   webhook was lost, streams the logs and exits with the verdict: success only when every obligation
   of its plan holds. It is the only writer of `Required` for the repository.
6. **Who executes what.** The official `Runner.Worker` executes the witness, so `docs/goals.md:16`
   holds literally; obligations never run inside a GitHub job.
7. **Failure.** A late or lost webhook only delays: the witness's own `ensure` starts the work. A
   merge tree that differs from GitHub's is a key miss, never a wrong verdict. A superseded push
   cancels its tree's scheduled work; work another request needs keeps running.

From phase 2 the one-job workflow calls `ensure` when GitHub starts it; phase 3 adds the webhook
callers, so the work starts at the push. A third caller, `velnor verify`, runs before the agent
pushes (M8). For the median pull request [E, `fable-5.2` design review T1]: webhook 2 s + merge tree
and plan 2 s + delta build 15 s + lanes 10 s = 29 s of verification, while GitHub creates and starts
the witness in about 15 s; the verdict lands about 35 s after the push.

### 3.4 Placement

Velnor places obligations, not jobs, so GitHub's choice of runner decides only where the witness
prints its logs.

- **Every node is warm for `main`** (phase 2). The builder builds each `main` generation once per
  site and replicates it to the site's nodes, so an obligation on any node starts from `main`.
- **Per-obligation placement** (phase 3) minimises predicted finish time: sandbox availability, view
  set-up, predicted rebuild on that node, predicted tests, and a penalty when the request's other
  obligations and artifacts sit in the other site. An idle cold node beats a busy warm one.
- **Request stickiness.** A request's obligations stay in one site; DE to FI is 35 ms over
  WireGuard.
- **Class routing** for workflow jobs from phase 0: one ARC scale set per event class and
  capability, `velnor-merge` (chosen in `runs-on` from `github.event_name`), `velnor-gate`,
  `velnor-build` and `velnor-docker`, each with warm pods. A pod's priority is fixed before GitHub
  assigns it a job, so merge-first admission needs its own scale set, not a priority class.

### 3.5 Executors

- **Obligations run on Velnor's own executor.** Generated obligations are plain commands (`cargo`,
  `cargo clippy`, `cargo doc`, `cargo nextest`), so the agent runs them in its sandboxes through
  `velnor-ci` and reimplements no step semantics. This is the native executor, limited to the work
  Velnor generates.
- **GitHub's `Runner.Worker` runs every workflow job**, unmodified: the witness, release jobs, and
  every job of other repositories and organisations. It is the oracle the executor is compared with.
- **Synthetic completion** is limited to a versioned generated job whose whole contract (outputs,
  step results, logs, post steps, dependents) is proven in the lab; it uploads and seals a log
  through Results and sends one step result, because `completejob` carries no log body. It needs the
  required checks bound to the Actions integration first (N12): until then a synthetic green and a
  forged one look the same.
- **Experiment-gated behind the witness.** An idle ARC runner already starts the next job 2 s after
  the previous one, 3 s at p90 [M, `fable-5.2` design review F4]. The dispatcher, the hot Worker,
  podless completion and a Worker replacement for arbitrary jobs are built only if the witness
  leaves more than 5 s per run that they remove. Their preconditions are recorded with their
  decisions (I003, I005, N05, N06, N10): a dispatcher persists the request identity and acquisition
  intent before `acquirejob` and the sealed response before any hand-off, keeps the Listener's
  first-renewal rule and force-fail path (`Runner.Listener/JobDispatcher.cs:393-405,1208-1219`),
  renews from `lockedUntil` with a loop written fresh (the upstream overload dereferences a null
  request when renewal fails, `JobDispatcher.cs:748-852`) and advertises the scale set's total
  capacity; a hot Worker gets `.runner` and a public-only `.credentials` at pod creation
  (`Runner.Worker/Program.cs:14`), is frozen rather than recycled, and is tested with a delivery
  handshake and kill-at-the-boundary fuzz, after a Worker started on arrival is timed; podless
  completion starts with a one-day test that a downstream job reads a completed job's `outputs:`.

### 3.6 Isolation and trust

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | the witness and other small jobs | ARC pod, 1 CPU | GitHub API | read-only tools |
| build | same-repository PR, merge group and `main` obligations | agent-created pod: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account or GitHub token, no `.git`, scrubbed environment | allow-list: crates mirror, site store | read-only trusted; per-obligation views; never a shared writable path |
| docker | only test binaries that need a daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), as their own obligation beside the build that made them | pod with a privileged dind sidecar on two tainted nodes, one per site; placement puts that build there | as build, plus a registry mirror | the build's test binaries, cloned in about 1 s |
| builder | `main` builds, no GitHub job | unprivileged pod | crates mirror | writes the trusted scope through the agent only |
| fork | fork pull requests | GitHub-hosted runners until a microVM class exists | — | none |

- An obligation never writes a store another reads; the agent harvests outputs after the build phase
  and decides where they go (section 5.6).
- Pull-request code runs outside any GitHub job, so it holds no GitHub token at all.
- `ensure` acts only on same-repository heads; a fork's tree never reaches a Velnor sandbox.
- Docker without a privileged sidecar is an experiment: image builds go to a rootless BuildKit
  service per site, and tests start containers as sibling sandboxes through the agent; if more than
  5% of the Docker suites need a real daemon, a microVM docker class replaces privileged pods.
- The required checks are bound to one integration in the ruleset. Today `Required verdict` and
  `Required` have none, so any app, or a workflow token with `statuses: write`, can post them (N12).

### 3.7 Failure handling and recovery

- **Journal discipline:** persist the intent, make the call, record the outcome; unknown is never
  absent. The ledger persists an execution's intent (key, node, epoch) before a sandbox starts, and
  the agent records the result by compare-and-swap on that epoch, so a node that lost its epoch to a
  restart or partition can report nothing into it; its sandbox is killed and its process tree's exit
  proven.
- **Fault injection** in the lab before each cutover: a crash before and after every remote call and
  journal write, a response dropped after success, duplicate and reordered webhooks, a ledger
  partition, a node lost mid-execution, and the loss of the control plane and of a whole site.
- **`ensure` is idempotent:** a repeated request returns the same receipts or joins the same
  executions. A restarted agent reconciles sandboxes by UID; an execution whose sandbox is gone is
  scheduled again and never reported twice.
- **Webhook gaps:** `velnor-hooks` replays missed deliveries from the App's delivery list every
  minute; the witness's own `ensure` covers the rest.
- **Break-glass:** the hosted workflow stays as a permanent fallback, and the cluster runs only
  released agent versions. The label switch back and the per-job route are drilled weekly.
- **Control plane:** k3s has one server today (§B.1.5), and losing it stalls pod creation and
  cleanup in both sites. The phase-3 cutover waits until control-plane and site loss are qualified:
  three servers across failure domains, or independent admission per site with durable fencing.

### 3.8 What is taken, and what is not copied

| Source | Taken | Not copied |
|---|---|---|
| velnor-new runner | The journal discipline, UID-based absence proofs, the job-authority policy, velnor-local as the conformance harness; for the gated dispatcher, `velnor-runner-github` wire types and `velnor-runner-core` lifecycle types | The 157k-line `velnor-runner-host` and its Docker launch path, which keeps serving the Mac host; runner credentials inside a sandbox |
| velnor-legacy | Reference code with tests: the trust-scope model, the per-slot lesson of its mbx store, its broker and run-service client for the gated dispatcher; its incidents as regression tests (rerun log ownership, artifact scoping, silent supervisors, idle CPU, a poisoned persistent store); its expression engine as a corpus | The 37k-line executor and runner files, the Docker socket proxy, BuildKit handling, the slot supervisor fleet, Firecracker until phase 5 |
| actions/runner | The protocol as ground truth; `Runner.Worker` for every workflow job; the action archive cache (`ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, `ActionManager.cs:1232`) | One pod per obligation run inside a job; self-update inside sandboxes |
| ARC | The scale sets that run the witness and every other job, with warm pods per class | Cold `emptyDir` pods, a dind sidecar in every pod, cache-blind placement of verification work |

## 4. Verification ledger

### 4.1 Obligations and input-addressed keys

- The plan (`velnor-ci plan`, from a released version) lists every obligation of the tree on every
  event. Each has a **definition digest**: command, configuration and features, profile, nextest
  profile and overrides, helper version, toolchain, runner image, sandbox class and CPU count.
- **The key is the hash of what the sandbox contains.** Per Cargo root in phase 2: the Git tree
  hashes of the root, of the roots it reaches (only `ci-helper` reaches outside its own, into five
  `generator` crates), of the shared files (toolchain pin, `mise.*`, `.github/`, `.velnor/`) and of
  declared extra inputs, plus the definition digest. Per package in phase 3: the same over the
  package's dependency closure from cargo's package graph. The hashes come from the commit in under
  5 ms, with no build. Package keys exist before the root workspaces merge, which removes the root
  boundaries.
- **The sandbox is a sparse checkout of exactly the key's paths.** An input the key does not hold is
  absent, and tree hashes include directory membership, so enumeration and negative lookups are
  covered too. The receipt-minting run logs every lookup outside the declared set that returns
  `ENOENT`. A class enrols only after its restricted runs pass 100% and match an unrestricted run's
  test inventory and outcomes; a failure names an undeclared input to declare.
- **The execution manifest joins every key:** the event class (none for a pure build or test; an
  obligation that reads event, ref or commit values is in C5), the allowed environment and its
  values, the toolchain and image digests, the sandbox class and CPU count, the path prefix, and the
  modes and symlink targets of the materialised files. The receipt names the bytes the agent
  mounted. An obligation counts as event-sensitive until an enforced run shows otherwise.
- **The exact tree is the degenerate key.** Test-binary digests are recorded with every receipt and
  audit the key without being part of it, so no bit-identical build is needed. Where a digest ever
  joins a key (phase 4), it covers every executable the test can spawn, from `cargo test --no-run
  --message-format=json`: 102 test files use `CARGO_BIN_EXE_*` 171 times, and `velnor-actions-cli`
  has no library, so an edit can change a spawned binary while the test binary stays identical.

### 4.2 Receipts, joins and scopes

- **One receipt per obligation, written as it finishes:** key, result, the request that produced it,
  the sandbox class and node, the expected test inventory, and the agent's signature. It expires
  after 7 days, which bounds how long a time-dependent failure can hide.
- **In-flight join.** A request whose key is executing waits for that execution; a cancelled request
  drops its claim, and the execution continues while another request needs it. A partial result
  satisfies nothing.
- **Why reuse across trees is safe here.** Everything that could forge or change a root's result is
  inside its key: the root's files, `.github/`, the pinned helper and the toolchain. The ledger
  accepts a receipt only for an obligation the released planner assigned to that execution.

| Scope | Written by | Reusable by |
|---|---|---|
| key receipts | the agent, for an execution built from a `trusted` generation plus its own delta | any request whose obligation has the same key, except forks |
| `trusted` | receipts of commits on `main`, and the builder's outputs | every request of the organisation except forks |
| fork | nothing | nothing |

A build that can mint a reusable receipt reads only the `trusted` cache and its own outputs, because
a stale build-script output from an earlier push could let a tree pass that no clean build accepts.
That costs the delta build, 10–20 s for the generator and 45–80 s for `velnor-runner-host` [E], and
retires the separate PR-scope cache.

### 4.3 What a request executes

- **Every event lists every obligation.** A merge group stays a full verification, as hard decision
  3 requires, and most of its obligations are satisfied by a receipt with the same key. The
  consuming plan's inventory is recomputed from the protected policy (the released planner and the
  ruleset) and the actual event, never copied from the run that produced the receipts, and evidence
  is matched per obligation, configuration and expected test inventory. An obligation with no
  receipt runs; a missing report or a test step that ran zero tests fails.
- **"Executes nothing" holds only when every obligation of the consuming plan has a receipt.** For a
  generator-only PR the other roots' keys equal `main`'s, so their receipts come from `main`.

| Situation | Phase | What runs | Status |
|---|---|---|---|
| Push to `main` whose SHA passed its merge group | 1 | release and publication duties only | committed |
| A root whose key equals a receipted key (any event) | 2 | nothing for that root | committed |
| A package whose closure key equals a receipted key | 3 | nothing for that package | committed after its experiment |
| Re-run of a failed job | 2 | only obligations without a passing receipt | committed |
| Test-binary closures finer than packages | 4 | nothing for that binary | stretch, M4 |
| Any obligation in C5 | all | always runs | — |

Replayed over 100 real merges, the root-jobs merge groups execute fall from 500 (full) to 425 with
exact-tree receipts and 125 with per-root keys [E, `opus-5.5` design review T3]. Below the package
level a test binary is eligible only inside a mount namespace the trusted helper creates, holding
its binary, declared inputs, toolchain runtime and a private `/tmp`, with no network (Landlock
cannot restrict `stat`).

### 4.4 Trust root and audit

- **Keys.** Each node has one signing key per sandbox class in a signer outside every pod and
  outside the agent's general service. A docker node's key signs docker-class obligations only,
  never a `trusted` receipt. Receipts go to an append-only log per site; the witness verifies each
  receipt's signature, class and log entry before it passes. The agent socket in a sandbox exposes
  only that sandbox's own execution.
- **Revocation.** Every verdict carries the policy epoch it was computed under. Revoking a signer or
  a reuse class raises the epoch: older verdicts are re-checked, queue entries carrying them pause
  until re-verified, and landed commits that relied on them are listed. The 7-day expiry is only a
  backstop.
- **Changing the verifier.** The released verifier stays authoritative while a new planner, ledger
  or executor is qualified on the same graphs, including controlled missing-test,
  wrong-configuration and wrong-candidate cases (`docs/goals.md` rules 12 and 13).
- **Degraded mode.** If the ledger is unreachable nothing is satisfied by lookup: obligations run
  fresh or, if the executor is down too, the per-job workflow takes over. Slow, never wrong.
- **Audit** (C6). New reuse classes run in shadow; every landed tree runs in full once within 24 h;
  audits re-execute on a different node; cold differential builds, recorded digests and the
  file-read audit (telemetry only) each feed a non-blocking check. A confirmed disagreement retires
  the key version or lineage and opens an issue; it never turns a red result green.

## 5. Cache system

### 5.1 The engine is decided by experiment

Three candidates compete for the compile cache. **mbx stays unless it measurably loses.**

| Candidate | How it works | What it promises | What threatens it |
|---|---|---|---|
| (a) mbx | A node-persistent mbx store on each worker plus an `mbx-cache` server (jdx/mr-boxington-cache) per site; OIDC rules let pull requests read and only `main` pushes write, with the anonymous read-only namespace as fallback | Per-invocation reuse across checkouts, paths and nodes; hard-linked outputs where no reflink exists; the site server shares `main`'s outputs natively | Units it does not consult; growth of never-reused entries |
| (b) cargo + reflinks | Plain cargo over a frozen target snapshot cloned per job with reflinks on XFS, plus `velnor-ci inputs restore` | Cargo's own fingerprints decide freshness; no wrapper | Per-file clone time on 6–9 GB targets; clones do not share page cache |
| (c) cargo + overlayfs | Plain cargo over an overlayfs mount whose read-only lower layers are frozen generations | Constant-time set-up; shared lower-layer page cache; the upper layer is the delta. Set-up and teardown 6–15 s → under 20 ms, page cache for four jobs 36 → 9 GiB [E, `gemini-4-carbon` design review] | Copy-up of large files; kernel and runtime limits |

Every candidate keeps the same rules: one fixed checkout path for every job, since a new path
rebuilt the workspace crates on the Mac; pull requests never write the shared scope; C6's cold
differential build audits it; a lineage restarts on every lockfile or toolchain change and daily;
its disk has a budget with eviction. `CARGO_INCREMENTAL` stays `0` in required builds until its own
experiment shows a safe gain.

### 5.2 What the Mac measurement showed

[M] The owner's machine: `ci-helper` with its `generator` path dependencies, six fresh checkouts at
different paths, mbx 1.29.2 against plain cargo with a cloned or copied target.

| Measure | mbx | Plain cargo |
|---|---|---|
| Disk per extra checkout, APFS (clones) | 473 MiB | 457 MiB with a clone |
| Disk per extra checkout, HFS+ (no clones, like ext4) | 835 MiB | 1,411 MiB with a full copy: mbx 41% smaller, through hard links |
| Crates rebuilt per extra checkout at a new path | the same 6 workspace crates | the same 6 workspace crates |
| A second node pulling from `mbx-cache` | 164 hits, 6 misses, 503.7 MiB downloaded | — |
| Never-reused workspace outputs stored per checkout | about 230 MiB | — |

Three lessons hold whichever candidate wins: build at one fixed path; budget the store (the owner's
own mbx store is about 485 GiB); and on XFS, where reflinks exist, (a) and (b) cost about the same
disk, while on ext4 (a) costs 41% less. One workspace and one commit do not settle wall time, and a
lockfile change was not measured.

### 5.3 Cluster measurement

**Pending.** The same comparison runs in a throwaway namespace on velnor-190 and velnor-5, at fixed
checkout paths, with a lockfile change and a page-cache check. It decides wall time and CPU-seconds
of (a), (b) and (c) on an unchanged tree, a one-file edit, a lockfile change and a second node; disk
per checkout and page cache with four concurrent jobs; set-up and teardown per job by file count;
and whether a second node warms from its site in time. The winner loses on none of these by more
than its noise; with no clear loser, mbx stays. This subsection records the numbers and the
decision.

### 5.4 Tiers and contents

L0 is the sandbox's own directory on the node's XFS volume. L1 is the node store on the same volume:
toolchains, tools, crate sources, the Git mirror, action archives and the compile cache, read-only
or as views the agent makes. L2 is one store, ledger and (for candidate (a)) `mbx-cache` service per
site: receipts, test binaries, reports and `main`'s compiled outputs. L3, GitHub's cache, serves
only workflows on GitHub-hosted runners, within the 10 GB budget. Nothing on the verification path
reads across sites synchronously; sites replicate receipts and seeds in the background.

| Item | Tier | Key | Writer | Expected hit rate | Basis |
|---|---|---|---|---|---|
| Rust toolchain and components | L1, read-only | toolchain file digest | node provisioner | ≈ 100% | [M] 0 of 58 restored today: it installs per job into `runner.temp` (`velnor-runners/state/RESULT.md:115`) |
| mise tools | L1, read-only | tool pins | node provisioner | ≈ 100% | [M] tool setup 10 s median, 18 s p90 per job (§A T9b) |
| Crate sources | L1, read-only; crates mirror on miss | `Cargo.lock` checksums | builder; mirror | ≥ 99% of crates | [M] 10.5% of PRs touch a lockfile (`proposals/fable-5.2.md`); mirror fetch 2.7–3.3 s (§B.4.2) |
| Git objects | L1 mirror | — | agent on push webhooks | ≈ 100% except objects pushed seconds earlier | the ledger fetches the pushed commit before planning |
| Action archives | L1, read-only | `owner_repo/<sha>.tar.gz` | provisioner | 100% for pinned actions | [M] about 4.4 s per job today (`proposals/opus-5.5.md`) |
| Compiled outputs of `main` | L1 per node; L2 per site for candidate (a) | the engine's own key | the builder | ≥ 95% of units fresh on PRs without a lockfile or toolchain change | [E] decided by section 5.3 |
| Test binaries and archives | L2 | content digest | agent | — | [M] velnor-new's nextest archive is 1.48 GB; GitHub upload took 36–43 s (I045) |
| Receipts | L2 ledger | obligation key | agents | same-SHA push 100%; merge groups 0–18% by exact tree today [M], 78–89% at a 1–5 min window [E] | decision log, (e); section 6.6 |

### 5.5 Trust, promotion and budgets

- **Only the builder writes the trusted scope.** It builds each `main` generation once per site and
  replicates it; with candidate (a), pull requests read the site server and only `main` pushes write
  it. Pull-request builds write no shared scope.
- **Generations are immutable**, and requests get views. The agent harvests a sandbox's outputs when
  the build phase ends and before any test runs, so a test that writes into `target/` cannot poison
  a later request.
- **Promotion.** A merge group never writes the trusted scope, because it may never land; its
  receipts become trusted when its exact SHA is on `main`. Forks read and write nothing on the
  cluster.
- **Cold differential** (C6): cargo cannot see a file it did not read in the last build, so only a
  cold build finds that staleness.
- **The GitHub cache path** stays correct for hosted runners: cache paths become fixed absolute
  paths, because `${{ runner.temp }}` differs between hosted and ARC runners and the lookup filters
  on a version derived from the path, which is why sources missed 48 of 48 times
  (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`, `.github/workflows/ci.yml:337`).
- **Budgets.** The last three `main` generations per root, then least-recently-used; 40% of a node's
  data volume across organisations, alert at 70%. FI workers have one 3.5 TB data disk each, DE
  workers 32–50 TB (§B.1.4). A runner-workspace target is 5.9–7.1 GB and the root one 9.0 GB [M,
  `proposals/fable-5.2.md`]; a velnor-new job grows the disk by 14.7 GB [M, I036].
- **Bumps.** An image, toolchain or helper bump changes every definition digest, so the builder
  builds the next generation and seeds its receipts before the bump lands.

## 6. Workflow design

### 6.1 Shape

- **Phase 2:** `velnor-gen` writes one job, named `Required`, whose single step runs the released
  verifier: it calls `ensure` with the whole obligation graph (plan, build once per configuration,
  lanes, join), streams the logs and exits with the verdict. Separate jobs remain only where
  permissions or publication need them. One job instead of 25 executed jobs removes about 33 s of
  overhead each and their start-up tails [E].
- **Lanes.** Inside a root's request the agent builds test binaries once per configuration, then
  runs format, lint, documentation, one combined "check alone" step and tests in parallel. Lanes
  need one target lane per check kind, since cargo locks a target directory exclusively
  (`docs/hard-decisions.md:56-62`); clippy and doc metadata pre-built by the builder; and one CPU
  budget across lanes: a jobserver shared by the sandbox, or mbx's scheduler if candidate (a) wins.
- **Phase 3:** the same job becomes a pure witness, because the webhooks have already called
  `ensure`. The non-blocking `Cache` check stays.
- **One verdict writer** per repository and phase, behind a flag; a new writer shadows the old one
  first. Until the owner changes the ruleset, the workflow emits both required names.
- **Forks:** `runs-on` is fork-conditional, for example `${{
  github.event.pull_request.head.repo.fork && 'ubuntu-26.04' || 'velnor-gate' }}`; today
  `velnor-gen` writes a fixed string (`generator/crates/velnor-render/src/workflow.rs:65`), which
  would send a fork to Velnor, where it is refused.
- **`cargo-deny`** reads the RustSec database pinned by commit in the repository and advanced by an
  automated pull request, so it is an ordinary keyed obligation instead of a network read that
  ejects every queued entry when an advisory appears.

### 6.2 Change-aware selection that keeps confidence

- **Phase 1, in the current engine:** declare the inputs of the three crate-root build scripts and
  the test reads the plan names. Today `undeclared_reads` is true for every crate with a build
  script (`crates/velnor-actions-rust/src/tasks.rs:254`), so the heavy jobs ran in 97.4% of PR runs.
  Merge groups stay full, so a missed input shows up as a merge-group failure, never as a false
  green.
- **From phase 2,** a pull request's plan follows `docs/goals.md` design rule 2: the package graph
  plus declared inputs, unknown files selecting everything. Skipped obligations are not lost: every
  merge group lists all of them.
- **No selection relative to the queue.** Diffing a merge group against the entry ahead never
  selects a failed predecessor's change, and HEADGREEN lands it
  (`ci-helper/crates/velnor-ci/src/event.rs:42-45`). Merge groups stay full and are satisfied by key
  lookup, which keeps hard decision 3 literally.
- A nightly run and every 20th merge execute everything, built cold (C6).

### 6.3 Tests

- **The unit of cost is `Journal::open`** (93 call sites in `velnor-runner-host`). The phase-1 gate:
  `Journal::open` on a pre-migrated template takes at most 1 s, measured, and no test exceeds 10
  CPU-seconds measured alone outside a recorded, shrinking list, which a non-blocking check enforces
  in phase 1 and a blocking one for new tests from phase 2. The 40-case and 6-case loops become one
  test per case.
- **The floor behind the loops is measured.** Excluding them, the longest test is still 132 s median
  in actions-cli, 125 s in runner-host and 138 s in local-mode; a median of 12, 41 and 51 other
  tests exceed 60 s; a receipt-integrity test took 924.8 s, building the same state four times [M,
  28 job logs, F-H33]. The method: build the expensive valid prefix once with the real code, clone
  the closed journal or publication artifacts into a private fixture per case, and keep every case's
  reopen, corruption-rejection, readback and preserved-file assertions. Migrations, historical
  upgrades and real deadlines stay their own obligations. 15 s needs a threefold cut per journal
  case.
- **The compile floor.** `velnor-runner-host` splits into five or six crates along module
  boundaries, with one test binary per package and `velnor-runner-local` extracted, so both
  configurations run without compiling the large crate twice; a one-file edit cost 61 s and 159
  CPU-s on a Mac [M, `opus-5.5` design review]. Incremental state in the cache is an experiment:
  10.9 s on a Mac [M, `fable-5.2` design review], with non-identical binaries that input keys
  tolerate.
- **Changed tests run three times** (section 1.2). **Shard only by arithmetic:** a suite splits only
  when its summed time over the slot's threads exceeds its longest test plus the cost of moving
  binaries (I067). **Test granularity** (phase 4): the executor spreads a request's cases over the
  site's nodes, longest first, and checks that every expected case reported exactly once.

### 6.4 No double or triple runs

| Event | Today | Phase 1 | Phase 2 | Phase 3 |
|---|---|---|---|---|
| PR push | full selected set | same | selected set; roots with a receipted key skipped | `ensure` on the push; packages with a receipted key skipped |
| Merge group | everything | everything | every obligation listed; roots satisfied by lookup | `ensure` on the group; packages satisfied by lookup; queue-tip candidates pre-verified (M5) |
| Push to `main` | everything again | one real `Required` job: it skips each obligation the merge group verified on the same tree, never on the group run's conclusion alone, then does release duties | the same, by key lookup | the same |
| Direct push to `main` | everything | everything | everything | everything |

The release gate fixes the push run's shape. `velnor-ci release-gate` keeps only `push` runs on the
exact SHA and accepts a required check only from GitHub Actions, integration 15368
(`ci-helper/crates/velnor-ci/src/gate.rs:15,127-159,277-288`), and `product-release.yml:86` asserts
exactly one job named `Required` in the push run of `.github/workflows/ci.yml`, a pinned file name.
The push run keeps that job; a verdict posted by an App would change `gate.rs`, the release
workflows and the publisher in one cutover.

### 6.5 Merge queue

| Setting | Today | Phase 0 | Phase 3 |
|---|---|---|---|
| `max_entries_to_build` | 2 | 2, until the under-load sampler has data | from a replay of real arrivals with all failures counted: about 4 at ≤ 3% false failures, 8 at ≤ 2% with bypass trees |
| Grouping, `max_entries_to_merge`, `min_entries_to_merge` | HEADGREEN, 10, 1 | same | owner question 3 (ALLGREEN) |
| `min_entries_to_merge_wait_minutes` | 5 | unchanged: it never applies with a minimum of 1 [M: enqueue to merge-group run 19 s] | same |
| `check_response_timeout_minutes` | 90 | 90 | 20 |
| Auto-merge | 0 of 40 PRs | agents enable it when they open a PR | same |
| Catch-up merges of `main` into PR branches | common | stopped | stopped |

At depth 8 one failure at the first slot invalidates up to seven candidates and GitHub starts their
replacements at once, so depth 8 waits for queue-tip speculation to keep the bypass trees verified.
HEADGREEN lands a group on its tip's checks; in the 15 days of evidence one failed entry landed that
way. Earlier commits of a group are listed for bisection and release tooling, and no release is cut
from one.

### 6.6 Minimum honest time to verdict

The median pull request touches only `generator/` or `ci-helper/` (47%); 25% only the runner
workspace; 8.5% only the root engine crates [M, 200 merged PRs, `proposals/fable-5.2.md`].

**Phase 3, the witness design** (median PR):

| Segment | Time | Basis |
|---|---|---|
| Push → webhook at `velnor-hooks` | 2 s | [M] push-event runs appear in 1–2 s (`fable-5.2` design review F1) |
| Fetch, merge tree, plan | 2 s | [E] |
| Delta build | 15 s | [E] 8.8 s mbx-warm on an Apple M5, scaled 1.7× for an EPYC core; measured in work item 1 |
| Lanes: tests, lint, docs in parallel | 10 s | [M] `velnor-ci`'s test step is 10 s median, 62 s p90 (§A T10) |
| Witness completes after the last receipt | 5 s | [E] the job is created and started about 15 s after the push, in parallel |
| **Total** | **about 35 s**; committed **≤ 90 s** | |

Runner-workspace PRs take 1.5–3 min until the crate split and the test floor land [E: a one-file
edit to `velnor-runner-host` is 35–99 s as a whole-crate compile on an M5, 10.9 s incremental].
Engine PRs stay at 5–6 min until the image-publication fixture (89 s, a cold example build) and its
suite are fixed.

**Push-to-merge** adds auto-merge enqueue (seconds), enqueue → merge-group run 19 s [M], the
merge-group request, and finish → merged 26 s [M]. A merge group finds every receipt when its keys
equal those the PR verified, which holds unless a pull request touching the same roots landed in the
window between them. Today that window is hours (S4's 0–18% exact-tree matches); in the design it is
the verdict time plus about a minute. Two replays of real arrivals [E, `opus-5.5` and `fable-5.2`
design reviews, 98 and 142 merges]:

| Window between the PR's verdict and its merge group | Same tree |
|---|---|
| 1–2 min | 89% |
| 2.5–5 min | 78–83% |
| 10 min | 70% |
| 11–20 min | 52–53% |
| twice today's demand, at 2.5 and 11 min | 80% and 69% |

Per-root and per-package keys add 3–10 points. For those merges push-to-merge is the verdict plus
about 1 min, about 2 min for the median PR in phase 3 [E]; the rest re-run what their keys changed.

## 7. Kubernetes

- **Role.** k3s stays the fleet layer: it deploys the agents, site services, mirrors and ARC scale
  sets, accounts for every pod, and enforces quotas, priorities and network policy. Velnor's
  scheduler places obligations; kube-scheduler places pods.
- **Webhook ingress.** `velnor-hooks` runs in each site behind one new public HTTPS endpoint (today
  the nodes expose only SSH, peers and the API to one address, §B.4.3); it accepts only requests
  signed with the App's webhook secret and replays missed deliveries every minute.
- **Sandbox lifecycle.** Each agent keeps single-use pods of each class created and idle on its
  node. A pod runs one obligation request and is deleted with a UID precondition. Creating one takes
  about 6 s [M, 436 pod starts, `proposals/opus-5.5.md`], before the work arrives. Pods are never
  reused: work can leave processes, images and configuration behind, as a poisoned store showed in
  velnor-legacy (§E.6). An idle pod costs about 45 MiB (§B.2.2).
- **Sizes.** Idle and running sandboxes share one memory budget per worker: 125.5 GiB less 6 GiB for
  the system, 2 GiB of eviction margin and 8 GiB for four gate pods leaves 109.5 GiB. Build
  sandboxes request 16 CPU with no CPU limit and 24 GiB of memory, request equal to limit (the
  heaviest job peaked at 15.7–16.8 GiB, §B.2.2), so four fit per worker, idle ones included: up to
  two free places hold pre-created idle sandboxes, resized from the last hour's arrivals; 32 GiB
  would fit only three. Docker sandboxes: 8 CPU plus 4 for dind, 24 GiB, for Docker-needing binaries
  only, while the runner root, touched by 55 of 100 merges, builds in the build class. Gate: 1 CPU,
  2 GiB. `CARGO_BUILD_JOBS` and nextest's threads are set to the sandbox's CPUs, because a pod
  without a CPU limit sees all 96. Elastic cgroup slots carved by the agent (5–6 build slots per
  worker instead of 4 [E]) are an experiment beside cache candidate (c).
- **Node settings.** Kube and system reservations (3 CPU, 6 GiB), a 2 GiB `memory.available`
  eviction threshold, `podPidsLimit` 16384, and the kubelet root on the XFS data volume after its
  real mount is inspected (today `emptyDir` lands on the ext4 OS disk, `velnor-runners/PLAN.md:45`),
  with fixed-size eviction thresholds (`nodefs.available: 100Gi`; 5% of a 50 TB volume locks out 2.5
  TB), XFS project quotas per sandbox, and site topology labels. The toolchain is mounted read-only
  only with the workflow that expects it: today's fails on a read-only `RUSTUP_HOME`
  (`rust_installed_manifest_unwritable`, `ci.yml:427,438`).
- **Reserved merge path.** Each site keeps one 16-CPU build slot and 2 gate CPUs for merge-group and
  gate work, about 7.7% of allocatable CPU, instead of relying on preemption: at 95% utilisation
  waiting grows to about 19 times the service time. The wait is measured from eligibility, not from
  GitHub's offer. A three-day experiment cordons one worker as a merge and gate lane under ARC
  labels and compares queue waits with today's.
- **Priorities.** `velnor-merge` > `velnor-gate` > `velnor-pr` = other organisations' CI >
  `velnor-batch` (builder, speculation, scheduled runs); only `velnor-batch` is preempted.
- **Untrusted pull requests** run on hosted runners, and `ensure` never touches a fork's tree. Build
  sandboxes get a default-deny NetworkPolicy with an allow-list, no service-account token and no
  host paths except the agent's read-only mounts.
- **Both sites** have their own `velnor-hooks`, ledger and store; a request stays in one site; the
  tainted docker pool has one node per site.
- **Other organisations.** ChainArgos and jackin keep their ARC scale sets, with a guaranteed CPU
  quota and separate namespaces and stores; nothing crosses an organisation.

## 8. Servers and cluster tuning

Ranked by gain per effort. "Measure first" items get no credit until a worker shows the stall.

| Rank | Change | Expected gain | Evidence | Risk | Rollback |
|---|---|---|---|---|---|
| 1 | Gate jobs on the cluster in a warm gate class | −5.8 to −57 min per run | [M] §A T5 | Gates compete with builds | Labels back to hosted |
| 2 | Honest requests, reservations, eviction threshold, PID limit, per-org quotas | Removes the 1.2–2.4× stretch under load | [M] runner-host 3.7 → 8.8 min (§A T6); 84-core bursts on 2-CPU requests (§B.2.2) | Fewer concurrent pods | Revert values |
| 3 | Remove `build.jobs=2` and `--jobs 2`; parallelism from the sandbox | `maintenance-helpers` 8.5 → about 5 min | [M] `mise.toml:90-193`; [E] `proposals/opus-5.5.md` | None | Revert lines |
| 4 | Toolchain, tools and action archives as read-only node stores | 14–18 s per job | [M] 10 s tools (§A T9b) + 4.4 s actions | Stale tool (keys are versions) | Remove the mounts |
| 5 | Docker only in the docker class | 3–4 s per job, fewer privileged pods | [M] 57 dind probe failures per hour (§B.6) | A misrouted job fails fast | Label back |
| 6 | Kubelet root on the XFS volume with `reflink=1`, fixed eviction thresholds, project quotas | Enables every L1 store | [M] root is ext4 on the OS disk; the data LV is XFS (`velnor-runners/PLAN.md:45,72-80`) | Node rebuild mistakes | Revert the kubelet argument |
| 7 | Node-persistent compile cache, engine per section 5.3 | From 46–278 compiled crates per job to the PR's own delta | [M] §A T9a; section 5.2 | A stale view is slow, not wrong (C6) | Disable the mount |
| 8 | Crates mirror through environment variables | 1–3 s per job, no internet dependency | [M] §B.4.2 | Mirror outage | Unset the variables |
| 9 | Site labels and request stickiness | No 35 ms round trips for artifacts | [M] §B.5 | Unbalanced sites | Remove the preference |
| 10 | Debuginfo: line tables for workspace crates, none for dependencies | −10 to −24% cache size; −20 to −27% CPU on a runner-host rebuild | [M] I108; `proposals/opus-5.5.md` | Poorer backtraces | Profile setting |
| 11–15 | Governor or EPP to performance (2–10%); same-site 10 Gbit/s (needed for M6); static CPU manager on one node (0–15%); THP and writeback (credit 0); mold (small: Rust ≥ 1.90 links with rust-lld) | measure first | not readable read-only (§B.1.6) or unmeasured | Power; lost burst; stalls; link bugs | Revert each |
| — | Striping the data volume; tmpfs work directories | rejected | FI workers have one data disk; reflinks cannot cross into tmpfs; the slow tests are CPU-bound (I099, I100) | | |

SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 9. Moonshots

Each moonshot states what must be true, the design element that attacks it, and the cheapest
experiment that proves or kills it. None is dropped for being hard, only when its experiment shows
it cannot keep C1–C7. Every experiment runs in week 1 beside phase 0, on velnor-lab or a drained
worker, off the required path.

| Id | Moonshot (phase; status) | Must be true | Attacked by | Cheapest experiment; killed if |
|---|---|---|---|---|
| M1 | PR verdict under 60 s for the median PR (3; stretch, committed ≤ 90 s) | The push webhook arrives within the 8 s GitHub needs to create the first job; the delta build takes ≤ 15 s, the lanes ≤ 10 s, the witness ≤ 5 s more | `ensure` on the push webhook, and the witness (section 3.3) | 2–3 days on velnor-lab: a webhook receiver, one warm pod and a one-job workflow; 50 pushes recording webhook arrival, first cargo command and verdict. Killed if webhook arrival P50 exceeds 8 s; the target then becomes 90 s |
| M2 | Push-to-merge under 5 min for the median PR (committed ≤ 5 min in 2 and ≤ 3 min in 3; stretch 100 s in 4) | A short verdict keeps the window short; auto-merge is always on; merge groups find their receipts by key | `ensure` on the push, root and package keys, queue-tip speculation, the reserved merge path: at a 1–5 min window 78–89% of merge groups find every receipt and take the verdict plus about 1 min | Work item 4's weekly window replay at the measured verdict time. Killed below a 70% hit rate |
| M3 | Zero cold start: the first cargo command 3–4 s after the push (3; stretch) | Merge tree and plan take ≤ 2 s; a pre-created sandbox has its view in ≤ 1 s | `ensure` on the push, pre-created sandboxes, read-only stores, reflinked views; later, microVMs restored from a snapshot for the fork and docker classes | M1's run, recording push → first cargo command. Killed if that exceeds 10 s at P50 |
| M4 | Zero duplicate verification across PR, merge queue and `main` (1–3; committed for the SHA, root and package keys; stretch below packages) | An obligation's inputs can be named | Input-addressed keys: per root in phase 2, per package in phase 3, test-binary closures in phase 4 (section 4.1) | 1–2 days on velnor-lab: run each root, then each package, from a sparse checkout of its key; a package enrols only at a 100% enforced pass that matches an unrestricted run; replay 200 merges. Package keys are killed if more than 20% of packages cannot enrol; root keys stand |
| M5 | Speculative merge queue on the queue tip (3; stretch, shadow from 0) | Velnor predicts the tree GitHub builds, the queue tip plus the PR: 122 of 193 merge-group runs were built on an unlanded group commit [M] | Ordered prefixes `T_i = T_{i−1} ⊕ PR_i` and single-ejection bypass trees (7 trees for 4 PRs) from `git merge-tree`, verified at batch priority through `ensure`; a wrong guess wastes preemptible CPU, never a verdict | A read-only shadow from phase 0 comparing predictions with the real merge-group trees over 50 merges or two weeks. Killed below a 70% match |
| M6 | Test-granularity scheduling over all 480 threads (4; stretch) | A suite's summed test time over one sandbox's threads exceeds its longest test, and binaries move within a site in ≤ 5 s | The executor spreads cases and fetches each changed test executable and its fixtures by digest; the 1.48 GB archive to four workers in 5 s would need 9.5 Gbit/s | A week of per-test CPU-seconds from nextest's JSON output, and `iperf3` between same-site workers. Killed if no suite's summed time exceeds its longest test by more than the transfer cost |
| M7 | Break the single-test floor (1–2; committed `Journal::open` ≤ 1 s and no test over 60 s; stretch 15 s) | Journal set-up dominates the slow tests, and cases can share a pre-built valid prefix | The template, prefixes built once and cloned per case, one test per case, the CPU budget, optionally optimised dependencies (question 8); the two loop splits alone leave 125–138 s and a 924.8 s test | 1–2 days on one worker: CPU-seconds of the 20 slowest tests under four profile variants, and a branch with the template. The 15 s stretch is killed if cases share state that cannot be separated |
| M8 | Verification that begins before the push (3; stretch) | Agents spend time between "code done" and "pushed" that verification can overlap | `velnor verify` sends the tree to the node mirror and calls `ensure`; the push finds receipts. Only results Velnor executed on linux/amd64 count, never a run on a developer's Mac | A week-long hook recording commit time, tree and the agents' local test time. Killed if agents rarely test before pushing |
| M9 | Zero queue wait, from eligibility (3; committed p90 ≤ 2 s and p99 ≤ 10 s; stretch 1 s) | The reserved merge path covers peak merge and gate work | The reservation per site and preemption of batch work, clocked from eligibility | Replay the busiest hour plus a site loss against the reservation, and the three-day cordon (section 7) |

## 10. Phases

The moonshot experiments and the cache head-to-head run in week 1, beside phase 0; their results,
not the calendar, choose what phases 2 and 3 build. Targets are section 1.4's.

| Phase | Scope | Exit gate (at least 300 merges or four weeks, with intervals beside each P50 and P90; at least 200 matched heads over peak and quiet periods for any P90; every PR class) | Expected saving |
|---|---|---|---|
| 0, week 1 | Gate jobs on the cluster: the generator release that moves them is merged, but the workflow was never regenerated, so regenerating it is the first measured gain. Narrow the image-candidate trigger; Source qualification on the cluster or only in the queue; auto-merge; no catch-up merges; queue depth from the replay; fixed cache paths; `event.json` race fix; push-to-merge, admission and flake instrumentation; the queue-tip shadow; the week-1 experiments | PR verdict P50 ≤ 12 min, P90 ≤ 20; push-to-merge P50 ≤ 25 min, P90 ≤ 60; hosted gate queue on the critical path p90 ≤ 1 min; admission span p90 ≤ 5 s | 5.8–57 min of gate queue per run; the cancelled-run stall shrinks from up to 2,203 s to one warm job; 7.5 min median enqueue delay |
| 1, weeks 2–3 | Selection fixed in the engine; the test-floor programme; `-j` caps removed; event-class and capability scale sets with warm pools and honest sizes; node tools and action archives; crates mirror; push run reduced to release duties; flake machinery; tested-tree recording | PR verdict P50 ≤ 5 min, P90 ≤ 10; push-to-merge P50 ≤ 12, P90 ≤ 30; runner-min per merged PR ≤ 150; false failures ≤ 3%; `Journal::open` ≤ 1 s | About 62 runner-min per merge from the push run; a generator or helper PR falls from 10.9 to about 3.5 min and from 52.9 to about 8 runner-min [E] |
| 2, weeks 3–6 | `velnor-gen` cutover to one job named `Required` that calls `ensure`, so the agent executes and observes every obligation; the ledger with per-root keys; the cache engine the head-to-head chose; kubelet on XFS; the builder; the crate split; the state seal and `cache-prune.yml` retired; test-time budget | PR verdict P50 ≤ 2 min, P90 ≤ 6; push-to-merge P50 ≤ 5, P90 ≤ 15; ≥ 65% of merge groups execute nothing; CPU per merged PR ≤ 60; no test over 60 s; view set-up time on `velnor-runner-host` published by file count | Cold compiles of 196–497 crates per job become deltas; merge groups re-run only roots whose key changed: 125 root-jobs per 100 merges instead of 500 [E] |
| 3, weeks 6–10 | Before the cutover: control-plane and site loss qualified, the rollback drilled. Then `ensure` on the push and merge-group webhooks; the witness; per-package keys; queue-tip speculation; `velnor verify` as a stretch; depth from the replay | PR verdict P50 ≤ 90 s, P90 ≤ 4 min; push-to-merge P50 ≤ 3, P90 ≤ 8; ≥ 85% of merge groups execute nothing; first cargo ≤ 5 s after the push; false failures ≤ 1%; CPU per merged PR ≤ 35 | Two of three job hops and the plan job disappear; most merges take the verdict plus about 1 min |
| 4, weeks 10–18 | Moonshot M6, finer closures, the merge train if approved; the gated dispatcher, hot Worker or podless completion if the witness left more than 5 s | stretch: PR verdict P50 ≤ 30 s, P90 ≤ 100 s; push-to-merge P50 ≤ 100 s, P90 ≤ 4 min | GitHub's 45 s of queue latency per merge, if Velnor lands merges |
| 5, after week 18 | Agent workspaces on the cluster; microVM fork and docker classes; other organisations on Velnor | stretch: push-to-merge P50 ≤ 75 s | Verification overlaps the agent's own work |

A phase that misses its gate does not start the next until the miss is explained. Every phase keeps
a working fallback: hosted labels in phase 0, the per-job workflow until the witness passes a
four-week soak, and the hosted break-glass workflow always.

## 11. The first ten work items

1. **Cache head-to-head.** Scope: candidates (a), (b) and (c) on the runner workspace at fixed
   checkout paths, on velnor-190 and velnor-5 in a throwaway namespace: unchanged tree, one-file
   edit, lockfile change, second node, four concurrent jobs; rustc processes, wall and CPU time,
   disk, page cache, set-up and teardown, `CARGO_INCREMENTAL` off and on; and the reproducibility
   gate C6's digest comparison needs (one commit on two nodes at two paths, every test binary
   compared). Acceptance: section 5.3 filled with raw data and a decision by its rule. Depends on:
   owner approval of the namespace (running).
2. **Gates on the cluster.** Scope: regenerate and land the workflow from the generator release that
   moves Plan, Ledger CLI, the lints and both required jobs to a `velnor-gate` scale set with warm
   pods and no dind; narrow the image-candidate trigger; Source qualification on the cluster or only
   in the queue. Acceptance: over 50 PR runs, hosted queue on the critical path p90 ≤ 1 min and no
   CI job on a hosted label. Depends on: nothing.
3. **Queue and agent procedure.** Scope: agents enable auto-merge when they open a pull request and
   stop catch-up merges; queue depth only as section 6.5 allows; required checks bound to the
   Actions integration; the list of a HEADGREEN group's intermediate commits for release tooling.
   Acceptance: ≥ 90% of merged PRs with auto-merge; green → enqueue P50 ≤ 30 s. Depends on: owner
   questions 3, 5 and 6.
4. **Measurement and shadows.** Scope: a push-to-merge collector clocked from the final commit's
   accepted push; admission span and cancelled-run stall as spans; queue residence and merges per
   hour; tested tree, root keys and obligation digests in every plan's output; the paired-rerun
   sampler; the queue-tip shadow (M5); webhook arrival times (M1); the weekly window replay (M2);
   the post-merge watch (section 12). Acceptance: a daily report with P50, P90, intervals and
   denominators that reproduces the 85-min baseline. Depends on: nothing.
5. **The test floor.** Scope: the journal template; one test per case for the two loops; prefixes
   built once and cloned per case for the receipt-integrity and publication-recheck tests; the 20
   slowest tests ranked by CPU-seconds alone after each change; the 12 serialised archive-guard
   goldens and the `crates_mirror` tests with private temp directories and ports, so
   `threads-required` goes; the 12-case `blob_faults` loop split; the 89 s fixture folded into the
   warm `--all-targets` build; the per-test budget and list; `ci-helper` tests in parallel with
   sized timeouts; the `event.json` snapshot. Acceptance: `Journal::open` ≤ 1 s on a worker; the
   list shrinks; no new false failures in 100 paired reruns. Depends on: nothing.
6. **Selection in the engine.** Scope: declare the crate-root build scripts' inputs and the test
   reads the plan names; replay the last 200 PRs through the fixed planner in shadow. Acceptance:
   the replay selects every job that failed in those PRs; a generator or helper PR runs ≤ 4 min.
   Depends on: nothing.
7. **Honest classes under ARC.** Scope: the event-class and capability scale sets; section 7's
   sizes; reservations, eviction and PID limits; per-org quotas; the `-j` caps removed; toolchain,
   tools and action archives as read-only mounts. Acceptance: job-duration p90/p50 ≤ 1.3 at peak;
   tool setup ≤ 2 s; no OOM kills in a week. Depends on: 2.
8. **No third run.** Scope: the push run keeps one real Actions job named `Required`, which lists
   every obligation, skips each one the merge group verified on the same tree (never on the group
   run's conclusion alone) and then does release duties; without that evidence it runs everything.
   Acceptance: ≤ 2 runner-min per matching push; `velnor-ci release-gate` and `product-release.yml`
   pass on `main`, finding exactly one Actions `Required` job in `ci.yml`'s push run. Depends on: 4.
9. **`velnor-gen` cutover with parity.** Scope: one job named `Required` whose step runs the
   obligation graph through `ensure` with parallel lanes; the local-mode configuration, the
   maintenance checks, both required names, the Velnor classes in `runs-on`; the three
   example-binary fixtures as test set-up in `.velnor/rust-ci.toml`, with examples in the warm
   cache; the mise pin the suites expect (2026.11.1; the golden carries 2026.11.9); no `edited`
   trigger once the ledger re-plans a retarget without restarting identical work; a week of shadow
   runs comparing obligation inventories. Acceptance: empty inventory difference over 100 runs; PR
   verdict P50 ≤ 2 min. Depends on: 1, 5, 10.
10. **The ledger and `ensure` on velnor-lab.** Scope: `velnor-hooks`, the ledger with per-root keys,
    per-obligation receipts and in-flight join, the signer and append-only log, the agent's executor
    in pre-created sandboxes, a one-job workflow whose witness calls `ensure`; the sparse-checkout
    experiment; webhook replay; M1's measurements. Acceptance: webhook arrival P50 ≤ 8 s and verdict
    P50 ≤ 60 s for a generator-sized change over 50 pushes; a killed webhook still yields a correct
    verdict; at most 20% of packages fail to enrol; a merge group's recomputed inventory matches per
    obligation, configuration and test inventory; 0 disagreements in 500 sampled re-executions,
    reported with its bound (about 0.6% at 95%). Depends on: owner question 1 for the phase-3
    cutover, nothing for the lab.

## 12. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A receipt or a selection gives a false green | Shadow re-execution of new classes; the audit sample; cold differential builds; every landed tree in 24 h; undeclared-read telemetry | Retire the key version; work runs again; revert the bad merge by a normal PR |
| A bad change lands anyway | The post-merge watch: revert rate, red-`main` rate, failures on receipt-passed trees, cost per merged PR | Raise the policy epoch; pause reuse for the affected classes |
| A stale compile view hides a failing build | The cold differential build disagrees with the warm path | Retire the lineage; rebuild from empty |
| GitHub changes webhooks, the merge queue or the runner protocol | A canary request every 10 min; a version tracker against GitHub's deprecation dates | The witness's own `ensure`; the per-job workflow; the hosted break-glass |
| The ingress or the ledger is down | Replay backlog; ledger health | Obligations run fresh; slower, never wrong |
| False failures rise as work packs denser | The paired-rerun sampler; flaky counts per test | Lower the per-node build cap; keep the queue shallow; quarantine only as section 1.3 allows |
| A PR poisons a cache another request trusts | Receipts record producer and scope; only the builder promotes | Delete the generation; requests build cold, slowly and correctly |
| Reuse degrades unseen | Before phase 3: hit rate by mechanism, builder lag, the audit's false-green bound, placement regret | Drop placement to site stickiness if a site fetch at P90 costs no more than a delta rebuild at P50, or regret stays under 5 s P50 |
| A revoked signer's or policy's greens are already queued | Every verdict carries its policy epoch; the witness re-checks it | Raise the epoch; pause and re-verify entries; list landed commits that relied on them |
| A signing key leaks, or a forged receipt reaches a verdict | The witness checks signature, class and log entry; audits on another node | Revoke the key and every receipt it signed; affected obligations run fresh |
| A new planner, sandbox or verifier passes a change ordinary CI would fail | A corpus of known-bad changes with green controls (a new failing golden, an omitted `local-mode` case, a stale publication attempt, a broken queue prefix repaired by the next PR), replayed after every such change, warm caches included | A candidate that passes one bad change loses authority for that class |
| FI workers run out of disk | Store budgets, alerts at 70% | Least-recently-used eviction, per-org caps |
| Other organisations starve, or starve velnor-new | Wait per organisation | Quotas and the reserved merge path; preempt only batch work |
| The new code grows like velnor-legacy | Line budgets checked in CI; phase gates | Stop and review at a budget; the per-job workflow keeps working |
| The cutover drops a configuration or check | Shadow inventory comparison | Keep the engine workflow until the inventories match |
| Speculation or the builder starves real work | Batch-class CPU share | Lowest priority, preempted first |

## 13. Owner questions

1. **`docs/goals.md:16` says the official runner always executes jobs.** The witness design keeps
   that literally: the official runner executes the only job, `Required`, and the work it reports
   runs in Velnor sandboxes started by a webhook. Example: a one-file `ci-helper` change is built
   and tested from 2 s after the push, and `Required` prints the logs and the verdict about 35 s
   after it. The gated dispatcher, podless completion and a Worker replacement would break the line;
   each comes back only if its experiment passes. Recommendation: amend line 16 to "every job runs
   in GitHub's official runner; verification may run in Velnor sandboxes that the job reports".
2. **Does a receipt with the same input key count as "ran and passed"?** Merge groups keep running
   in full, as hard decision 3 says, and receipts satisfy most of it. This replaces the earlier
   question about selecting merge groups by change, which the review showed unsound: a failed
   predecessor's change is never selected, and HEADGREEN lands it. Example: a generator-only pull
   request's merge group lists every root; four roots have `main`'s keys, so receipts satisfy them
   and only the generator root runs, about 1 min instead of a 3–5 min warm full run. Recommendation:
   yes, under C1–C7 and the audit (`docs/goals.md`, "Green means verified").
3. **HEADGREEN or ALLGREEN?** HEADGREEN lands a group when its tip passes, so a failed entry can
   land inside a passing group; it happened once in 15 days. Example: entry A fails, entry B (main +
   A + B) passes, and both land; with ALLGREEN, A is ejected and B rebuilt without it.
   Recommendation: keep HEADGREEN while merge groups are slow, with C1 stated for tips and
   intermediate commits listed; switch to ALLGREEN once queue-tip speculation keeps the bypass trees
   verified, which makes an ejection nearly free.
4. **Stay on the Free plan?** Example: on the worst day the three gate jobs waited 57 of a 71.8 min
   run for the plan's 20 hosted slots; after phase 0 they run on the cluster, and only forks and
   macOS release builds stay hosted. `docs/goals.md:45` keeps the generator inside free limits.
   Recommendation: stay on Free; pay only if forks or release builds queue, measured.
5. **Agent procedure.** Example: 0 of 40 recent merged PRs enabled auto-merge, and the median PR
   waited 7.5 min between green and enqueue. Recommendation: agents enable auto-merge when they open
   a pull request, stop merging `main` into their branches, and later run `velnor verify` before
   pushing.
6. **Ruleset changes.** Example: today any app, or a workflow token granted `statuses: write`, can
   post a green `Required`. Recommendation: bind `Required verdict` and `Required` to one
   integration now; raise `max_entries_to_build` only when the replay shows more merges per hour at
   the under-load false-failure rate; at the cutover accept a single `Required` name, keeping the
   push run's one Actions `Required` job that the release gate reads; set the grouping by question
   3.
7. **The Kubernetes runtime inside `velnor-runner-host`?** The proposal under review puts the
   cluster runtime behind a trait in that crate. Example: the crate is 157k lines with about 60 open
   correctness issues, sets the CI critical path, and its unit puts the runner's JIT credentials
   inside the job container. Recommendation: keep it for the Mac and local mode, and build the
   cluster executor as new crates.
8. **Test profile.** Dependencies at `opt-level = 3` in tests cut the slowest test class's CPU by
   66% [M, `proposals/fable-5.2.md`], measured with debug assertions off in dependencies; with them
   on, nobody has measured it. Recommendation: measure the four combinations of `opt-level` and
   debug assertions, with overflow checks fixed, under design rule 14, and decide on both numbers.
9. **Other organisations.** Should ChainArgos and jackin share velnor-new's PR priority with a
   guaranteed quota, and when do they move to Velnor's executor? Example: a ChainArgos PR job and a
   velnor-new PR job queued together start in arrival order, while velnor-new's merge-group work
   goes first. Recommendation: equal PR priority with quotas now; offer Velnor in phase 5.
10. **Hardware.** Example: the longest test took 924.8 s on an EPYC core; desktop-class cores at 5.5
    GHz or more would run the serial path 1.3–1.5× faster [E]. Recommendation: rent one such node in
    phase 1 for the test-floor experiments; buy a worker when CPU stays above 70% at peak after
    dedupe, or the eligible wait for merge work exceeds a p90 of 10 s for two weeks.
11. **A Velnor merge train instead of GitHub's merge queue?** The queue adds 19 s to start a merge
    group and 26 s to merge it [M], the largest term once a verdict takes under a minute. Velnor
    could fast-forward `main` to the exact commit whose obligations all hold, a compare-and-swap
    that verifies every tip by construction. Example: a green pull request lands 5 s after its
    verdict instead of about 50 s; but squashed pull requests must still show as merged, and a
    Velnor outage stops merges until the queue rule returns. Recommendation: a two-week shadow train
    in phase 4, killed below a 95% tree match, then decide.
12. **One counted re-execution for tests the change could not have broken?** Retries are 0, so every
    flake ejects a queue entry and rebuilds the entries behind it, although a test whose package
    closure the change did not touch has `main`'s inputs, where it passed. Example: a generator-only
    change hits a timing flake in a `velnor-runner-host` test; one immediate re-execution passes, is
    recorded in a non-blocking `Flaky` check and charged to the budget, and the merge proceeds.
    Tests whose closure changed never get a retry; they run three times. Recommendation: yes, after
    replaying the evidence's 10 false failures against their pull requests' closures.

## 14. What more hardware would buy

| Purchase | Buys | When |
|---|---|---|
| One high-clock node (desktop-class cores at 5.5 GHz or more) | 1.3–1.5× on the serial path: rebuilding `velnor-runner-host` and the longest tests [E] | Rented in phase 1 for the test-floor experiments |
| 10 Gbit/s between same-site workers | M6 and site replication; 1.48 GB of test binaries in about 1.2 s instead of 12 s [E] | When M6 shows a suite that needs it |
| Two more k3s servers, across failure domains | A control plane that survives one node or site; no speed | Before phase 3's cutover |
| More NVMe on the FI workers | Cache headroom on the 3.5 TB data disks | When eviction drops live generations |
| More EPYC workers | +96 threads each, about +20% capacity | Trigger set now: CPU above 70% at peak after dedupe, or an eligible-wait p90 above 10 s for two weeks |

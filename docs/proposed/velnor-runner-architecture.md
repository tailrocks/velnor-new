# Velnor runner architecture

**Status:** Proposal; it replaces nothing yet. It follows the owner's binding decisions in section
3.1; the questions still open are in section 13, and parts of the design contradict `docs/goals.md`
and `docs/hard-decisions.md` until the owner changes them. **Scope:** how Velnor verifies
velnor-new's changes on the project's Hetzner k3s cluster so that waiting for CI stops being the
slowest part of delivering a change, with the generator (`velnor-gen`) and helper (`velnor-ci`)
where the runner needs something from them.

**Sources.** Facts are cited from the [evidence pack](../research/velnor-runner-2026-11/evidence.md)
as §A T5 and similar, from the eight panel proposals under
`../research/velnor-runner-2026-11/proposals/`, from the cache experiments under
`../research/velnor-runner-2026-11/experiments/`, and from the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea the panel raised
(`I001`–`I120`, `N01` onward) and every hole (`F-H`) and alternative (`F-B`) the design review
found. Every number is tagged **[M]** measured (with its source), **[E]** estimate (with its basis)
or **[T]** target. A target is **committed** when the evidence makes us confident, **stretch** when
an experiment must prove it first.

## 1. Goal and success metric

### 1.1 The metric

The single metric is **push-to-merge**: from the final commit's accepted push to a pull request's
head until it is merged, from the pull request timeline, reported weekly as P50 and P90 with
intervals over every merged pull request, for every PR class (generator and helper, runner, engine,
local, no Rust, mixed), with superseded pushes, time-outs and open pull requests visible beside the
merged cohort. Beside it: PR verdict time (push to the `Required` check), the admission span (run
created → first job), runner-minutes per merged pull request, the compile-cache hit rate and the
false-failure rate. Three guardrails keep a faster last push from hiding slower feedback: time to
the first actionable failure, first-push-to-merge, and failed attempts per merged change.
Runner-minutes (decision I117) count each occupied slot once, split completed, cancelled and
superseded work, and charge the builder, speculation and audits to the consuming repository.

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
   every obligation the plan lists for it, a passing result exists whose key equals the obligation's
   key at that commit, produced by Velnor's trusted execution in the sandbox class the obligation
   needs. An *obligation* is one unit of verification: format, lint, docs, a per-crate check, one
   test binary under one configuration, or a named check. Under HEADGREEN a merge group's earlier
   commits land as ancestors of a verified tip without results of their own, and a failed entry has
   landed that way; such commits are listed for bisection and release tooling (question 3:
   ALLGREEN).
2. **C2. Keys capture every input, by construction.** A key is the hash of what the obligation's
   sandbox contains: the Git tree hashes of its inputs, its definition and its execution manifest.
   The sandbox is a sparse checkout of exactly those paths, so an input outside the key is not on
   disk and a lookup of it fails (section 4.1).
3. **C3. Selection is sound or wide.** The plan comes from a released helper, never from the change
   under test (`docs/goals.md` rule 12). A file the plan cannot place selects everything. Nothing is
   skipped relative to a merge group's predecessor; an obligation is skipped only when a receipt
   with its exact key exists.
4. **C4. Only what Velnor observed is evidence for another run.** A receipt is minted only from
   processes the node agent started in a sandbox it created, running the released plan's command
   graph on the inputs it mounted, with every exit and the expected test inventory observed. A
   missing, skipped, cancelled, masked or zero-test result fails. A job's own report is never
   evidence; a lab test checks that a forged passing report yields zero receipts.
5. **C5. Uncomparable work always runs, and the sandbox hides what a key does not hold.**
   Obligations that read the network, the event payload, the clock in an assertion, unseeded
   randomness, Docker images not pinned by digest or the commit's identity, or that publish
   anything, run fresh every time. For tests this is enforced: a loopback-only network namespace and
   no event file. Build and test sandboxes have no `.git`, a scrubbed environment (no `GITHUB_*`
   variables; fixed `HOME`, `PATH` and locale) and a fixed CPU count per class, part of the
   definition digest. An obligation that reads Git objects, as `velnor-runner-host`'s build script
   reads `.velnor/runner.lock` (`build.rs:69-80`), stays here until it reads a declared file.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow with every
   reuse re-executed and graduates per obligation kind after about 3,000 clean reuses; then 2% are
   re-executed. Every landed tree is executed in full once within 24 h on batch capacity. The
   nightly and every-20th-merge runs build from an empty target in a fresh pod and compare results
   and test-binary digests with the warm path, and a cache lineage restarts from empty on every
   lockfile or toolchain change and at least daily. A disagreement is re-run five times to separate
   a flake from a wrong key; a confirmed one retires that key version or lineage until explained.
7. **C7. Flakes are budgeted** (section 1.3).

For an obligation whose inputs the change did not touch, a repeat run says something only about the
test, which C6 and C7 collect. For one whose inputs it touched, a regression that fails with
probability p per execution is caught by three executions with 1−(1−p)³ (49% at p = 0.2, 14% at p =
0.05) and by one with p. Today's three full runs give that by accident; Velnor gives it on purpose:
**every obligation whose inputs the change touched executes three times in parallel, and any failure
blocks**, at small cost, since only those tests run.

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes. It costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** at most 3% false failures per full required run from phase 1 and 1% from phase 3, each
  claimed only once enough pairs exist. Today's 8.1% [M] is a one-direction discordance rate.
- **Measure:** a paired-rerun sampler re-runs green and red results in both directions under
  representative load, by obligation and environment, as the denominator. Nothing is claimed before
  about 300 pairs: zero disagreements in 299 give a one-sided 95% bound just under 1%; 50 merges at
  a 5% sample give 2.5 pairs.
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
| 1 | ≤ 5 / ≤ 10 min (3.5 / 8) | ≤ 12 / ≤ 30 min (10 / 22) | ≤ 150 runner-min | selection fixed in the engine (about 212 without it, 139 with it [E]); `Journal::open` on a template ≤ 1 s; work directories on XFS with warm snapshots |
| 2 | ≤ 2 / ≤ 6 min (80 s / 4 min) | ≤ 5 / ≤ 15 min (3.5 / 9) | ≤ 60 | one job per run; per-root keys, ≥ 65% of merge groups executing nothing; the `mbx-cache` threshold evaluated; no test over 60 s measured alone |
| 3 | ≤ 90 s / ≤ 4 min (60 s / 2.5 min) | ≤ 3 / ≤ 8 min (1.75 / 5) | ≤ 35 (15) | week-1 experiments passed (webhook under 8 s, closure-only checkout for ≥ 80% of packages, queue-tip match ≥ 70%); ≥ 85% of merge groups execute nothing; ≤ 0.5% of merges ejected by a false failure; eligible-to-start p90 ≤ 2 s for merge and gate work |
| 4 | (30 s / 100 s); ≤ 20 s after a preflighted push | (100 s / 4 min); about 50–75 s if Velnor lands the merge | (≤ 15) | stretch only |

If phase 3's experiments fail, its design still supports about 90 s / 4 min for the verdict and 4 /
10 min push-to-merge. Further committed figures: first cargo command ≤ 15 s after the job starts
(phase 2) and ≤ 5 s after the push (phase 3, stretch 4 s); no test over 60 s (phase 2, stretch 15
s). After the executor cutover, job start is ≤ 1 s at P50 and ≤ 2 s at P90 from the broker message.

## 2. Where the time goes, and what changes the problem

1. **Waiting, not working** [M]. A PR run with no hosted queue takes about 11 min, but the median PR
   waits 85 min from push to merge. On the worst day the three serial gate jobs waited 57.0 of a
   71.8 min run for the Free plan's 20 hosted slots, shared with two busy non-CI workflows (§A T5,
   T11). A superseding push waits up to 2,203 s for the cancelled run's `always()` `Required` job;
   the queue builds 2 entries at a time; no agent enables auto-merge.
2. **The same verification three times** [M]: on the PR, the merge group and the push of the same
   SHA (§A T7); the third run found nothing and failed falsely 8.1% of the time.
3. **Every job is cold** [M]. Nothing compiled survives a job on the cluster (§A T9a, T9b); the one
   restore that exists took 44–61 s for a 1.92 GB entry, longer than the build it saved (I036).
4. **Code sets the floor** [M]. Two tests loop over 40 and 6 cases (258–274 s, 203–271 s) and the
   floor behind them is still 125–138 s; `maintenance-helpers` compiles with `build.jobs=2`;
   selection forces every crate with a build script (I059, I061, I068).

The design removes work instead of speeding it up. Gates leave the hosted pool. Velnor executes jobs
itself, natively, and verification leaves GitHub jobs: the push webhook starts it on the cluster,
and the job reports it. Input-addressed keys make "verify identical inputs once" structural, so a
merge group stays a full verification that is mostly a lookup. Every node keeps `main` warm, and the
slow tests are rewritten. Everything Velnor runs is Rust, by the owner's decision; the speed comes
from locality and removed round trips, not from the language (section 3.1).

## 3. Architecture

### 3.1 The owner's decisions

These are binding; the rest of the design is built on them. The table after the list says which APIs
are in and which are out, and why.

- **Rust only.** Velnor's native Rust executor runs every velnor-new job. GitHub's official runner
  (`Runner.Listener` and `Runner.Worker`) is not used in production, neither as the executor nor as
  a fallback; its latest release is the lab's conformance oracle (section 3.6). Gemini 4 Carbon
  proposed the general native worker; Claude Fable 5.2, Muse Wonder and Muse Wonder Full the native
  lane.
- **No Node and no JavaScript actions.** Velnor implements the closed set of actions velnor-new's
  workflows use natively in Rust and refuses any other action before the job starts (section 3.5).
- **Code from velnor-legacy is copied, never revived.** A component that targets the latest APIs and
  passes the oracle is copied into velnor-new, reviewed, adapted and owned there from then on.
  velnor-legacy stays archived: no new commits, no git or path dependency on it, no shared crate
  published from it. velnor-new's `crates/velnor-runner`, which runs the official runner in Docker,
  is superseded as an executor; its journal, recovery and ownership proofs are reused where they
  fit.
- **`docs/goals.md:16` is overruled.** It says jobs are always executed by GitHub's official runner;
  the vision paragraph (line 20) and design rule 11 (line 75) assume the same. The owner changes
  those lines. `docs/hard-decisions.md` binds only the existing engine and names no runner.
- **Speed, stated honestly (H1).** Starting Node costs 87–147 ms per action [M, `fable-5.2`], so the
  language is not where the time goes. The gain is locality and fewer round trips: checkout from the
  node's Git mirror, caches and tools from node stores, no action downloads, placement before
  acquisition, no official-runner start per job. The owner chose a fully new stack for control over
  the whole path; both reasons stand.

| API | Status | Why |
|---|---|---|
| Scale-set sessions, broker long-poll, JIT runner configuration; run-service `acquirejob`, `renewjob`, `completejob` | in | how GitHub delivers jobs to self-hosted runners today |
| Results twirp for logs, timeline and step summaries; cache service v2 (twirp); artifacts v4; the OIDC token endpoint | in | the services current actions and the current runner use |
| node24-era runner semantics: contexts, env files, defaults, workflow commands | in | what the oracle runs |
| The legacy session with AES message encryption | out | replaced by JIT runners and the broker; it needs a private key on disk for no benefit |
| Legacy pipelines and distributed-task APIs (plans, timelines and `distributedtask` job requests) | out | superseded by the run service and Results |
| The older `DTExpressions` evaluator | out | one evaluator only: `Expressions2`, as transcribed from actions/runner v2.337.0 |
| Cache v1 and artifacts v3 | out | GitHub deprecated both for cache service v2 and artifacts v4 |

### 3.2 What stays compatible and what is redesigned

From the outside Velnor is a GitHub App plus self-hosted runner scale sets. **Unchanged:** workflow
YAML and its semantics, required checks, rulesets and the merge queue, `GITHUB_TOKEN`, OIDC, and the
Actions services jobs report to, so a job Velnor executes still reports its check as GitHub Actions
(integration 15368), as the release gate requires. **Redesigned:** who executes jobs, when
verification starts, where it runs, what is on disk, what is skipped by receipt, how actions run.

### 3.3 Components

New code is Rust in the root workspace; crossing a crate's line budget stops work for review (I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| Dispatcher | `velnor-dispatch` | one service per site | Scale-set sessions, broker long-poll, the JIT runner identities and their credentials; placement before acquisition; the lease journal | 6k lines |
| Executor | `velnor-exec` | inside the node agent | Job messages, contexts and expressions, steps and composites, workflow commands and env files, masking, post steps, cancellation and timeouts; the run service and Results | 20k lines |
| Native actions | `velnor-native-actions` | inside the executor | The closed action set (section 3.5) and the refusal of everything else | 10k lines |
| Conformance harness | `velnor-conformance` | velnor-lab | Differential runs against the official runner (section 3.6) | 5k lines |
| Webhook ingress | `velnor-hooks` | one service per site | Receives the App's push, pull-request and merge-group webhooks, verifies their signature, calls `ensure`, replays missed deliveries | 3k lines |
| Ledger | `velnor-ledger` | library plus one service per site | `ensure` requests, obligation keys, signed receipts, scopes, promotion, audit | 6k lines |
| Node agent | `velnor-node` | privileged DaemonSet on workers | Hosts the executor; pre-created sandboxes, result observation, node stores, the `main` builder, harvest, cleanup by UID | 15k lines |
| Store | `velnor-store` | library plus one service per site | Content-addressed blobs (test binaries, archives, reports), snapshot copies within a site, eviction | 6k lines |
| Cross-node compile cache | `mbx`, `mbx-cache` (existing) | one server per site | Fills cold nodes and mismatched snapshots by per-crate reuse; kept while it passes its threshold (section 5.1) | existing |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, obligation commands, the workflow shape; `velnor-gen` emits only the closed action set at qualified SHAs | existing |

### 3.4 The job path

1. **Push.** From phase 3 a push to a pull request, or a merge group's creation, reaches
   `velnor-hooks` as a webhook 1–2 s later [M: GitHub creates push-event runs within 1–2 s,
   `fable-5.2` design review F1], which calls `ensure(tree, plan)` on the site's ledger. The ledger
   fetches the commit into the node mirror, computes the tree with `git merge-tree` (the head merged
   into its current base, or the merge group's own commit) and runs the released planner for the
   obligations and their keys.
2. **Lookup and execute.** Each obligation has a passing receipt (done), an execution in flight
   (join it) or nothing (schedule it). Obligations are plain commands (`cargo`, `cargo clippy`,
   `cargo doc`, `cargo nextest`) run through `velnor-ci`, so they need no step semantics. The agent
   that placement picks runs each scheduled one in a pre-created single-use pod of its class
   (section 3.8), with a reflink clone of the node's warm snapshot made after the sparse checkout is
   written, observes the results, and writes a receipt per obligation as it finishes.
3. **Acquire.** GitHub offers the job to the scale set. `velnor-dispatch` holds every JIT runner
   identity itself, so GitHub's choice of identity does not decide the node: the dispatcher reads
   the request's labels, event and repository, picks the node (warm snapshot, site, class, reserved
   merge path), and only then acquires the job and hands its message and token to that node's
   executor, merge-group work first.
4. **Execute the job.** The executor refuses the job before any step if it names an action outside
   the closed set; otherwise it renews the lease every 60 s, runs the steps, streams logs, timeline
   and step summaries to Results, and finishes with `completejob`. Generated obligation steps run
   through the same `ensure`, so they join the work the webhook started or are satisfied by
   receipts; a lost webhook only delays. Other steps run in the job's own sandbox. The job's
   conclusion is the required check; there is one writer of `Required` per repository.
5. **Failure.** A merge tree that differs from GitHub's is a key miss, never a wrong verdict; a
   superseded push cancels its tree's scheduled work, and work another request needs keeps running.

For the median pull request [E, `fable-5.2` design review T1]: webhook 2 s, merge tree and plan 2 s,
delta build 15 s and lanes 10 s make 29 s of verification, while GitHub creates the job in about 15
s and Velnor starts it within 1 s; the verdict lands about 35 s after the push. Job start, from the
broker message to the first step, is ≤ 1 s at P50 and ≤ 2 s at P90 [T], against 2–3 s for an idle
ARC runner and 12–14 s on demand [M].

### 3.5 Native actions

velnor-new's workflows reference this closed set [M: `uses:` lines in `.github/workflows/`]; Velnor
implements each in Rust against node-local state.

| Action | Uses | Native implementation | Node-local state |
|---|---|---|---|
| `actions/checkout` | 61 | Git checkout with the action's inputs and outputs | objects from the node's Git mirror, fetched on the push webhook |
| `actions/upload-artifact`, `actions/download-artifact` | 77, 60 | an artifacts v4 client | downloads by digest from the site store |
| `actions/cache`, `/restore`, `/save` | 2, 17, 5 | cache service v2 with the action's key and restore-key rules | restores by digest from the node store |
| `jdx/mise-action` | 48 | the pinned tools on `PATH`, with mise's environment | the node's prepopulated, read-only tool store |
| `jdx/mr-boxington-action` | 12 | mbx installed and configured as the action does, until `velnor-gen` drops it (jobs run plain cargo, section 5.1) | the node store |
| `actions/attest-build-provenance`, `actions/attest` | 8, 1 | Sigstore signing with the job's OIDC token, then the attestation upload | — |
| `asamarts/alint` | 1 | the pinned alint release, run with the action's inputs | the tool store |
| Local composite actions (`./.github/actions/*`, `actions/setup-velnor`) | — | composite semantics | — |
| `actions/github-script` | 3 | none: it runs JavaScript. `velnor-gen` replaces its three uses (a ruleset-drift check and two qualification steps) with `velnor-ci` commands | — |
| The Docker and node24 test actions of `qualification.yml` | 4 | refused; those cases become refusal checks | — |

An action, or an action SHA, outside the qualified set fails the job before its first step, with an
error that names it and lists the supported set. `velnor-gen` emits only qualified actions and SHAs,
enforced by a generator test, so a new SHA (from Renovate, for example) passes the harness first.
The implementations use node-local state, but their effects (cache entries, artifacts, outputs,
annotations) must equal the upstream action's, and the harness checks that.

### 3.6 The conformance oracle

`velnor-conformance` runs each corpus case on Velnor and on the latest official runner release in
the lab, and compares them. velnor-new's `qualification.yml`, which already compares a hosted job
with a Velnor scale-set job, is its seed.

- **Corpus.** (1) velnor-new's full job set: every job of its 16 workflow files under each event
  that triggers it, at pinned commits, with recorded failures, cancellations and re-runs; release
  workflows publish to a lab repository and lab registries. (2) A semantics corpus: velnor-legacy's
  and actions/runner's expression tests; every context (github, runner, job, steps, matrix, needs,
  inputs, vars, secrets masking); `if:` with `success()`, `failure()`, `always()` and `cancelled()`;
  `continue-on-error`; outputs through `GITHUB_OUTPUT`, step and job outputs and `needs`;
  `GITHUB_ENV`, `GITHUB_PATH` and `GITHUB_STEP_SUMMARY`; workflow commands and annotations; post
  steps; cancellation at every step boundary; job and step timeouts; cache and artifact round trips;
  OIDC tokens. (3) Refusals: actions outside the set, Node and Docker actions, legacy features.
- **Pairing.** The lab repository dispatches each case twice from the same commit and event, once
  per runner label; the harness pairs jobs by workflow, job and matrix, and collects runs, jobs,
  steps, logs, annotations, summaries, caches, artifacts and attestations through the GitHub API.
- **Normalisation.** Ids, timestamps and durations; runner names, labels, versions and machine
  names; workspace, temp and tool-cache paths; tokens; the official runner's own lines (action
  downloads, banners, Node set-up); ANSI codes; the interleaving of concurrent output. Contexts are
  compared as JSON with those fields removed.
- **Pass criteria.** After normalisation: exact equality of step outcomes and conclusions, outputs,
  contexts and env, expression results, annotations, step summaries, cache and artifact effects
  (keys, hits, file lists, digests), post-step order and results, and cancellation and timeout
  outcomes; the same timeline structure and user-visible log lines. Any other difference fails the
  case unless an owner-signed allow-list entry with its reason covers it. The gate is 100% of cases.
- **When.** On every change to the executor, the native actions or the protocol code, as a blocking
  check; nightly; and within 24 h of each official runner release, found through the releases API.
  Velnor reports the runner version it last passed against, so GitHub's 63-day version window
  (§F.6.1) becomes the deadline for passing each new release, and protocol drift shows up as a
  failing case instead of a production incident.

### 3.7 Placement

- **Every node is warm for `main`** (phase 1): the builder builds each `main` generation once per
  site and every node's warmer takes it into its snapshot (section 5.1).
- **Per-obligation placement** (phase 3) minimises predicted finish time: sandbox availability, view
  set-up, predicted rebuild and tests on that node, and a penalty when the request's other
  obligations and artifacts sit in the other site. An idle cold node beats a busy warm one. A
  request stays in one site; DE to FI is 35 ms over WireGuard.
- **Class routing.** Until the cutover, ARC serves one scale set per event class and capability
  (`velnor-merge`, chosen in `runs-on` from `github.event_name`, `velnor-gate`, `velnor-build`,
  `velnor-docker`), because a pod's priority is fixed before GitHub assigns it a job; after it, the
  dispatcher orders work by event itself.

### 3.8 Isolation and trust

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | small jobs (lints, the plan) | single-use pod, 1 CPU | GitHub API | read-only tools |
| build | same-repository PR, merge-group and `main` jobs and their obligations | pod: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account token; obligations also without `.git`, a GitHub token or the job's environment | allow-list: crates mirror, site store | read-only trusted; per-obligation clones; never a shared writable path |
| docker | only test binaries that need a daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), as their own obligation beside the build that made them | pod with a privileged dind sidecar on two tainted nodes, one per site; placement puts that build there | as build, plus a registry mirror | the build's test binaries, cloned in about 1 s |
| builder | `main` builds, no GitHub job | unprivileged pod | crates mirror, `mbx-cache` | writes the trusted scope and `mbx-cache` |
| fork | fork pull requests | GitHub-hosted runners after a maintainer approves, until a microVM class passes conformance (owner question 7) | — | none |

- An obligation never writes a store another reads; the agent harvests outputs after the build phase
  and decides where they go (section 5.5). Runner credentials stay in the dispatcher, never in a
  sandbox, and `ensure` acts only on same-repository heads, so no fork's tree reaches a sandbox.
- Docker without a privileged sidecar is an experiment: image builds go to a rootless BuildKit
  service per site, and tests start containers as sibling sandboxes through the agent; if more than
  5% of the Docker suites need a real daemon, a microVM docker class replaces privileged pods.
- The required checks are bound to one integration in the ruleset. Today `Required verdict` and
  `Required` have none, so any app, or a workflow token with `statuses: write`, can post them (N12).

### 3.9 Failure handling, releases and rollback

- **Journal discipline,** copied from velnor-new's runner: persist the intent, make the call, record
  the outcome; unknown is never absent. The executor journals the plan id, job id, run-service URL,
  job token and pod UID before acting; after a crash it resumes or fails the job with its own token,
  never twice. The dispatcher persists the request identity and acquisition intent before
  `acquirejob` and the sealed response before any hand-off, keeps the official Listener's
  first-renewal rule and force-fail path (`Runner.Listener/JobDispatcher.cs:393-405,1208-1219`), and
  renews from `lockedUntil` with a loop written fresh, because the upstream overload dereferences a
  null request when renewal fails (`JobDispatcher.cs:748-852`; N10). The ledger persists an
  execution's intent (key, node, epoch) before a sandbox starts, and the agent records the result by
  compare-and-swap on that epoch, so a node that lost its epoch can report nothing into it; its
  sandbox is killed and its process tree's exit proven.
- **Fault injection** in the lab before the cutover: crashes around every remote call, journal write
  and step boundary, a response dropped after success, a lease lost mid-job, duplicate and reordered
  webhooks, a ledger partition, a lost node, the control plane and a whole site.
- **`ensure` is idempotent:** a repeated request returns the same receipts or joins the same
  executions. A restarted agent reconciles sandboxes by UID; an execution whose sandbox is gone is
  scheduled again and never reported twice. `velnor-hooks` replays missed deliveries every minute,
  and a job's own `ensure` covers the rest.
- **Releases and rollback.** The cluster runs only released executor versions, never the change
  under test, so a broken release cannot block its own fix. A release serves 10% of jobs for a day
  before all of them; the previous release stays installed, and switching back is drilled weekly.
  Until the cutover (section 10), ARC with the official runner serves velnor-new; after it, rollback
  is the previous Velnor release, never the official runner.
- **Control plane:** k3s has one server today (§B.1.5), and losing it stalls pod creation and
  cleanup in both sites. The executor cutover waits until control-plane and site loss are qualified:
  three servers across failure domains, or independent admission per site with durable fencing.

### 3.10 What is taken, and what is not

| Source | Taken | Not taken |
|---|---|---|
| velnor-legacy (archived) | Copied into velnor-new, reviewed, adapted to the latest APIs and owned there, each piece with its tests and only after it passes the oracle: the expression engine (3,565 lines, transcribed from actions/runner v2.337.0); the broker, run-service and Results clients and the scale-set session; the native checkout (`checkout.rs`, `git_mirror.rs`) and cache adapters; the trust-scope model; its incidents as regression tests | The legacy pipelines and `distributedtask` paths, the v1 cache service, JavaScript sidecars and Docker actions, the 37k-line `executor.rs` and `runner.rs` as wholes, BuildKit handling, Firecracker until phase 5. velnor-legacy itself gets no commits, no dependency and no published shared crates |
| velnor-new's runner (`crates/velnor-runner`) | Journal discipline, crash recovery, UID-based absence and ownership proofs, with their code, and its wire types (`velnor-runner-github`) and lifecycle types (`velnor-runner-core`) where they match the latest APIs; velnor-local as a harness | Its executor, which runs the official runner in Docker |
| actions/runner | The protocol as ground truth; its latest release as the lab oracle | Any production use |
| ARC | Its scale sets serve velnor-new until the cutover | Everything after it: its listener and controller exist to run the official runner |

## 4. Verification ledger

### 4.1 Obligations and input-addressed keys

- The plan (`velnor-ci plan`, from a released version) lists every obligation of the tree on every
  event. Each has a **definition digest**: command, configuration and features, profile, nextest
  profile and overrides, helper version, toolchain, runner image, sandbox class and CPU count.
- **The key is the hash of what the sandbox contains.** Per Cargo root in phase 2: the Git tree
  hashes of the root, of the roots it reaches (only `ci-helper` reaches outside its own, into five
  `generator` crates), of the shared files (toolchain pin, `mise.*`, `.github/`, `.velnor/`) and of
  declared extra inputs, plus the definition digest. Per package in phase 3: the same over the
  package's dependency closure from cargo's package graph, in place before the root workspaces
  merge. The hashes come from the commit in under 5 ms, with no build.
- **The sandbox is a sparse checkout of exactly the key's paths.** An input the key does not hold is
  absent, and tree hashes include directory membership, so enumeration and negative lookups are
  covered. The receipt-minting run logs every lookup outside the declared set that returns `ENOENT`.
  A class enrols only when its restricted runs pass 100% and match an unrestricted run's test
  inventory and outcomes; a failure names an undeclared input to declare.
- **The execution manifest joins every key:** the event class (none for a pure build or test; an
  obligation that reads event, ref or commit values is in C5), the allowed environment and its
  values, the toolchain and image digests, the sandbox class and CPU count, the path prefix, and the
  modes and symlink targets of the materialised files, as the bytes the agent mounted. An obligation
  counts as event-sensitive until an enforced run shows otherwise.
- **The exact tree is the degenerate key.** Test-binary digests are recorded with every receipt and
  audit the key without joining it, so no bit-identical build is needed. A digest that ever joins a
  key (phase 4) covers every executable the test can spawn (N02: 171 `CARGO_BIN_EXE_*` uses).

### 4.2 Receipts, joins and scopes

- **One receipt per obligation, written as it finishes:** key, result, the request that produced it,
  the sandbox class and node, the expected test inventory, and the agent's signature. It expires
  after 7 days, which bounds how long a time-dependent failure can hide.
- **In-flight join.** A request whose key is executing waits for it; a cancelled request drops its
  claim, and the execution goes on while another needs it. A partial result satisfies nothing.
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
level a test binary is eligible only inside a mount namespace the trusted helper creates, with its
declared inputs and no network (N02).

### 4.4 Trust root and audit

- **Keys.** Each node has one signing key per sandbox class, in a signer outside every pod and the
  agent's general service; a docker node's key signs docker-class obligations only, never a
  `trusted` receipt. Receipts go to an append-only log per site, and the executor verifies each
  receipt's signature, class and log entry before it passes an obligation. The agent socket in a
  sandbox exposes only that sandbox's execution.
- **Revocation.** Every verdict carries its policy epoch. Revoking a signer or a reuse class raises
  the epoch: older verdicts are re-checked, queue entries carrying them pause until re-verified, and
  landed commits that relied on them are listed. The 7-day expiry is only a backstop.
- **Changing the verifier.** The released verifier stays authoritative while a new planner, ledger
  or executor is qualified on the same graphs, with controlled missing-test, wrong-configuration and
  wrong-candidate cases (rules 12 and 13).
- **Degraded mode.** With the ledger unreachable nothing is satisfied by lookup: obligations run
  fresh or, if the executor is down too, the per-job workflow takes over. Slow, never wrong.
- **Audit** (C6) re-executes on a different node; cold differential builds, recorded digests and the
  file-read audit (telemetry only) feed non-blocking checks. A confirmed disagreement retires the
  key version or lineage and opens an issue; it never turns a red result green.

## 5. Cache system

### 5.1 The decision

Measured on the cluster (section 5.3) and the owner's Mac (5.2); the owner approved these roles.

- **The hot path is plain cargo over a reflinked warm snapshot.** A build job, and from phase 2 a
  sandbox, gets a reflink clone of its node's warm snapshot of the trusted `main` generation, on the
  XFS volume at the fixed checkout path `/w/velnor-new`, and runs plain cargo: the clone takes 0.25
  s and 2.5 MB, and an unchanged tree compiles 0 crates in 0.31 s [M]. Two rules keep it sound.
  Unchanged files keep the snapshot's recorded modification times and every other file is newer than
  every output, so cargo's time-based fingerprints rebuild exactly what changed (`velnor-ci inputs
  restore` does this today; the agent does it for sandboxes, which have no `.git`); the measurement
  took its times from one archive, so work item 1 tests a changed file carrying an older time. And
  the path never changes: on the Mac a new path rebuilt the workspace crates under both engines.
- **mbx plus a per-site `mbx-cache` is the cross-node layer.** It fills a cold node in about 40 s
  instead of 92 s and, once any node has built a lockfile change, serves it to a node without a
  matching snapshot in about 43 s, because it reuses content per crate. It is not the per-job hot
  path: a warm snapshot is 0.3 s, 0 crates and 2.5 MB; plain cargo beat mbx on a same-node lockfile
  change, 67 s against 88 s; and mbx's own 0.32 s no-op relies on one managed target directory per
  path, which concurrent jobs at the fixed path would share. The builder builds each `main`
  generation once per site and publishes it through `mbx-cache`; each node's warmer advances its
  snapshot from there instead of compiling. Jobs never run mbx: one that lands on a node without a
  matching snapshot takes the warmer's refill or plain cargo over the older snapshot, whichever the
  agent predicts is faster.
- **Its value depends on one number:** the share of build jobs that land on a node without a warm,
  matching snapshot. Phase 1 measures it by cause: a new node, an eviction, a `main` lockfile or
  toolchain change the node has not taken yet, or the PR's own new lockfile, which the server cannot
  serve. **Keep `mbx-cache` while at least 10% of build jobs would otherwise start cold or
  mismatched and are served from it, or while it saves at least 20 runner-minutes a day of
  foreground builds; retire it after four weeks below both** [T, owner-approved]. The limbs agree at
  today's volume: 10% of about 300 build executions a day at 24–52 s saved each is 12–26
  runner-minutes [E]. Frequent Renovate lockfile updates, new nodes and evictions keep it; near-zero
  misses retire it in favour of incremental warmers that take the builder's changed files.
- **The honest alternative.** Incremental warmers make nodes rarely cold, and copying a snapshot
  from another node moves about the same 8.7 GiB as mbx. mbx's unique strength is partial, per-crate
  reuse when dependencies differ.
- **Overlayfs** offered constant-time set-up, which the clone gives, and shared page cache, which
  clones lack (36 → 9 GiB for four jobs [E, `gemini-4-carbon` design review]); it is the remedy if
  the page-cache budget (section 7) is exceeded. `CARGO_INCREMENTAL` stays off until measured.

### 5.2 What the Mac measurement showed

[M] On the owner's machine
([report](../research/velnor-runner-2026-11/experiments/mbx-cache-mac.md); `ci-helper` with its
`generator` path dependencies, six fresh checkouts at different paths, mbx 1.29.2), an extra
checkout cost 473 MiB with mbx and 457 MiB with a cloned target on APFS, and 835 MiB against 1,411
MiB for a full copy on HFS+, which like ext4 has no clones (mbx 41% smaller, through hard links).
Both engines rebuilt the same 6 workspace crates at every new path, since a path crate's identity
includes its absolute path. A second machine got 164 hits and 6 misses (503.7 MiB) from `mbx-cache`;
mbx kept about 230 MiB of never-reused outputs per checkout; the owner's own store is about 485 GiB.

### 5.3 Cluster measurement

[M] velnor-190 and velnor-5 (96 vCPU, 125 GiB, kernel 6.12), the runner workspace (357 crates, 8 in
the workspace), `cargo build --locked --workspace --all-targets` at `/w/velnor-new` with 32 jobs and
`CARGO_INCREMENTAL=0`, rust 1.99.1, mbx 1.29.2, `mbx-cache` built from source, nodes nearly idle
([report](../research/velnor-runner-2026-11/experiments/mbx-cache-cluster.md)). The local-path
volumes are XFS with reflink; `emptyDir` is ext4 on the OS disk, without.

| Scenario | Wall | Compiled | Disk and network |
|---|---|---|---|
| Plain cargo, cold | 92 s | 357 crates | 10.6 GiB target |
| Plain cargo over a reflink clone of the warm target | 0.25 s clone + 0.31 s | 0 | +2.5 MB |
| Plain cargo over a full copy on ext4 `emptyDir`, today's layout | 8.6 s copy (source cached) + 0.23 s | 0 | +10.75 GiB on the OS disk |
| mbx, cold, empty server | 93 s | 357 | uploads 8.7 GiB |
| mbx, same node and path, warm store | 0.32 s | 0 | its path-keyed managed target |
| mbx, warm store, managed target gone | 7.2 s | 0 (496 hits) | +1.94 GiB |
| mbx, second node, empty store, warm server | 39.9 s | 0 (496 hits, 0 misses) | downloads 8.7 GiB at about 215 MB/s |
| Lockfile change (one leaf crate; 11 rebuilt, 3 in the workspace): plain cargo over the clone | 67.4 s | 11 | +8.7 GiB; stale outputs stay |
| The same, mbx on the same node | 88.4 s | 11 | +15.5 GiB; uploads 6.8 GiB |
| The same, mbx on the second node after the first built it | 43.4 s | 0 (29 hits) | downloads 6.8 GiB |

Clones share disk blocks, not page cache: two clones each held 14.29 GB of cache once read, and the
snapshot's own pages were evicted; a no-op build reads only metadata. mbx needed a loopback
forwarder and `MBX_REMOTE_TRUST_WRITES=true` (section 5.5); its store grew from 8.8 to 15.6 GiB
after one lockfile change, with garbage collection off. Still to measure (work item 1): a one-file
edit, four concurrent jobs, `CARGO_INCREMENTAL` on, a second commit.

### 5.4 Tiers and contents

L0 is the sandbox's directory on the node's XFS volume; L1 the node store on the same volume
(toolchains, tools, crate sources, the Git mirror, action archives, the warm snapshots), read-only
or as clones the agent makes; L2 one store, ledger and `mbx-cache` per site; L3, GitHub's cache,
only for workflows on hosted runners, within its 10 GB budget. Nothing on the verification path
reads across sites synchronously.

| Item | Tier | Key | Writer | Expected hit rate | Basis |
|---|---|---|---|---|---|
| Rust toolchain and components | L1, read-only | toolchain file digest | node provisioner | ≈ 100% | [M] 0 of 58 restored today: it installs per job into `runner.temp` (`velnor-runners/state/RESULT.md:115`) |
| mise tools | L1, read-only | tool pins | node provisioner | ≈ 100% | [M] tool setup 10 s median, 18 s p90 per job (§A T9b) |
| Crate sources | L1, read-only; crates mirror on miss | `Cargo.lock` checksums | builder; mirror | ≥ 99% of crates | [M] 10.5% of PRs touch a lockfile (`proposals/fable-5.2.md`); mirror fetch 2.7–3.3 s (§B.4.2) |
| Git objects | L1 mirror | — | agent on push webhooks | ≈ 100% except objects pushed seconds earlier | the ledger fetches the pushed commit before planning |
| Action archives | L1, read-only | `owner_repo/<sha>.tar.gz` | provisioner | 100% for pinned actions | [M] about 4.4 s per job today (`proposals/opus-5.5.md`) |
| Compiled outputs of `main` | L1 snapshot per node; L2 `mbx-cache` per site | `main` generation; mbx's key per crate | the builder | every unit the change did not touch, when its lockfile and toolchain match the snapshot | [M] section 5.3; the share of matching jobs is measured in phase 1 |
| Test binaries and archives | L2 | content digest | agent | — | [M] velnor-new's nextest archive is 1.48 GB; GitHub upload took 36–43 s (I045) |
| Receipts | L2 ledger | obligation key | agents | same-SHA push 100%; merge groups 0–18% by exact tree today [M], 78–89% at a 1–5 min window [E] | decision log, (e); section 6.6 |

### 5.5 Trust, promotion and budgets

- **Only trusted builds of `main` write** the snapshot generations and `mbx-cache`: a `main` push
  job through the server's OIDC rule, or the builder outside GitHub with a credential scoped to its
  namespace and `MBX_REMOTE_TRUST_WRITES=true`, since mbx otherwise makes the remote read-only
  outside a GitHub or GitLab push to a protected branch. Warmers only read, over TLS, which
  production needs because mbx refuses a plain-HTTP remote with a token unless the host is loopback;
  build jobs never contact the server. Pull-request builds write no shared scope; forks get nothing.
- **Generations are immutable.** The agent harvests a sandbox's outputs when the build phase ends,
  before any test runs, so a test that writes into `target/` cannot poison a later request. A merge
  group never writes the trusted scope, since it may never land; its receipts become trusted when
  its exact SHA is on `main`. A lineage restart (C6) builds cold, because only a cold build finds a
  file cargo did not read last time, and publishes into a fresh `mbx-cache` namespace, so it reads
  nothing an older lineage wrote.
- **The GitHub cache path** stays correct for hosted runners: cache paths become fixed absolute
  paths, because `${{ runner.temp }}` differs between hosted and ARC runners and the lookup filters
  on a version derived from the path, which is why sources missed 48 of 48 times
  (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`, `.github/workflows/ci.yml:337`).
- **Budgets.** The last three `main` generations per root, then least-recently-used; 40% of a node's
  data volume across organisations, alert at 70%; FI workers have one 3.5 TB data disk each, DE
  workers 32–50 TB (§B.1.4). The runner workspace's target is 10.6 GiB [M] and the root one 9.0 GB
  [M, `proposals/fable-5.2.md`]. `mbx-cache` collects garbage and starts at 300 GiB per site, about
  35 full uploads, resized from four weeks of upload volume [T].
- **Bumps.** An image, toolchain or helper bump changes every definition digest, so the builder
  builds the next generation and seeds its receipts before the bump lands.

## 6. Workflow design

### 6.1 Shape

- **Phase 2:** `velnor-gen` writes one job, named `Required`, whose steps are the obligation graph;
  the executor runs them through `ensure` (plan, build once per configuration, lanes, join), streams
  the logs and finishes the job with the verdict. Until the executor cutover, a `velnor-ci witness`
  step on ARC calls the same `ensure`. Separate jobs remain only where permissions or publication
  need them. One job instead of 25 executed jobs removes about 33 s of overhead each and their
  start-up tails [E]. **Phase 3:** the push and merge-group webhooks start the same work before
  GitHub creates the job, which then joins it; the non-blocking `Cache` check stays.
- **Lanes.** Inside a root's request the agent builds test binaries once per configuration, then
  runs format, lint, documentation, one combined "check alone" step and tests in parallel: one
  target lane per check kind, since cargo locks a target directory exclusively
  (`docs/hard-decisions.md:56-62`); clippy and doc metadata pre-built by the builder; and one
  jobserver shared by the sandbox's lanes as their CPU budget.
- **One verdict writer** per repository and phase, behind a flag; a new writer shadows the old one
  first. Until the owner changes the ruleset, the workflow emits both required names.
- **Forks:** `runs-on` is fork-conditional, for example `${{
  github.event.pull_request.head.repo.fork && 'ubuntu-26.04' || 'velnor-gate' }}`; today
  `velnor-gen` writes a fixed string (`generator/crates/velnor-render/src/workflow.rs:65`), which
  would send a fork to Velnor, where it is refused.
- **`cargo-deny`** reads the RustSec database pinned by commit and advanced by an automated pull
  request, so it is an ordinary keyed obligation instead of a network read that ejects every queued
  entry when an advisory appears.

### 6.2 Change-aware selection that keeps confidence

- **Phase 1, in the current engine:** declare the inputs of the three crate-root build scripts and
  the test reads the plan names. Today `undeclared_reads` is true for every crate with a build
  script (`crates/velnor-actions-rust/src/tasks.rs:254`), so heavy jobs ran in 97.4% of PR runs.
  Merge groups stay full, so a missed input shows up as a merge-group failure, never a false green.
- **From phase 2,** a pull request's plan follows `docs/goals.md` design rule 2: the package graph
  plus declared inputs, unknown files selecting everything. Every merge group still lists every
  obligation, and a nightly run and every 20th merge execute everything, built cold (C6).
- **No selection relative to the queue.** Diffing a merge group against the entry ahead never
  selects a failed predecessor's change, and HEADGREEN lands it
  (`ci-helper/crates/velnor-ci/src/event.rs:42-45`). Merge groups stay full and are satisfied by key
  lookup, which keeps hard decision 3 literally.

### 6.3 Tests

- **The unit of cost is `Journal::open`** (93 call sites in `velnor-runner-host`). The phase-1 gate:
  `Journal::open` on a pre-migrated template takes at most 1 s, and no test exceeds 10 CPU-seconds
  measured alone outside a recorded, shrinking list, enforced by a non-blocking check in phase 1 and
  a blocking one for new tests from phase 2. The 40-case and 6-case loops become one test per case.
- **The floor behind the loops is measured.** Excluding them, the longest test is still 132 s median
  in actions-cli, 125 s in runner-host and 138 s in local-mode; a median of 12, 41 and 51 other
  tests exceed 60 s; a receipt-integrity test took 924.8 s, building the same state four times [M,
  28 job logs, F-H33]. The method: build the expensive valid prefix once with the real code, clone
  the closed journal or publication artifacts into a private fixture per case, and keep every case's
  reopen, corruption-rejection, readback and preserved-file assertions. Migrations, historical
  upgrades and real deadlines stay separate obligations. 15 s needs a threefold cut per case.
- **The compile floor.** `velnor-runner-host` splits into five or six crates along module
  boundaries, with one test binary per package and `velnor-runner-local` extracted, so both
  configurations run without compiling the large crate twice; a one-file edit cost 61 s and 159
  CPU-s on a Mac [M, `opus-5.5` design review]. Incremental state in the cache is an experiment:
  10.9 s on a Mac [M, `fable-5.2` design review], and input keys tolerate its binaries.
- **Changed tests run three times** (section 1.2). A suite is sharded only when its summed time over
  the slot's threads exceeds its longest test plus the cost of moving binaries (I067). In phase 4
  the executor spreads a request's cases over the site's nodes, longest first, and checks that every
  expected case reported once.

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
Commits landing inside a HEADGREEN group are listed (C1), and no release is cut from one.

### 6.6 Minimum honest time to verdict

The median pull request touches only `generator/` or `ci-helper/` (47%); 25% only the runner
workspace; 8.5% only the root engine crates [M, 200 merged PRs, `proposals/fable-5.2.md`].

**Phase 3, median PR: about 35 s, committed ≤ 90 s** (section 3.4). Push → webhook 2 s [M:
push-event runs appear in 1–2 s]; fetch, merge tree and plan 2 s [E]; delta build 15 s [E: 8.8 s
warm on an Apple M5, scaled 1.7× for an EPYC core; on a worker an unchanged tree took 0.31 s and an
11-crate lockfile change 67 s [M]; a one-file edit is measured in work item 1]; tests, lint and docs
in parallel lanes 10 s [M: `velnor-ci`'s test step is 10 s median, 62 s p90, §A T10]; the job
completes 5 s after the last receipt [E: GitHub creates it about 15 s after the push, in parallel,
and Velnor starts it within 1 s].

Runner-workspace PRs take 1.5–3 min until the crate split and the test floor land [E: a one-file
edit to `velnor-runner-host` is 35–99 s compiled whole on an M5, 10.9 s incremental]; engine PRs
stay at 5–6 min until the image-publication fixture (89 s, a cold example build) is fixed.

**Push-to-merge** adds auto-merge enqueue (seconds), enqueue → merge-group run 19 s [M], the
merge-group request, and finish → merged 26 s [M]. A merge group finds every receipt when its keys
equal those the PR verified, unless a pull request touching the same roots landed in between: today
that window is hours (S4: 0–18% exact-tree matches), in the design the verdict time plus about a
minute. Two replays of real arrivals [E, `opus-5.5` and `fable-5.2` design reviews, 98 and 142
merges] give the share of merge groups whose tree equals the PR-tested tree, by the window between
the PR's verdict and its merge group: 89% at 1–2 min, 78–83% at 2.5–5 min, 70% at 10 min and 52–53%
at 11–20 min; at twice today's demand, 80% at 2.5 min and 69% at 11 min. Per-root and per-package
keys add 3–10 points. For those merges push-to-merge is the verdict plus about 1 min, about 2 min
for the median PR in phase 3 [E]; the rest re-run what their keys changed.

## 7. Kubernetes

- **Role.** k3s stays the fleet layer: it deploys the dispatchers, the node agents that host the
  executor, site services, mirrors and, until the cutover, ARC; it accounts for every pod and
  enforces quotas, priorities and network policy. Velnor places work; kube-scheduler places pods.
- **Webhook ingress.** `velnor-hooks` runs in each site behind one new public HTTPS endpoint (today
  the nodes expose only SSH, peers and the API to one address, §B.4.3) and accepts only requests
  signed with the App's webhook secret.
- **Sandbox lifecycle.** Each agent keeps single-use pods of each class created and idle on its
  node; a pod runs one request and is deleted with a UID precondition. Creating one takes about 6 s
  [M, 436 pod starts, `proposals/opus-5.5.md`], before the work arrives, and an idle pod costs about
  45 MiB (§B.2.2). Pods are never reused: work can leave processes, images and configuration behind,
  as a poisoned store showed in velnor-legacy (§E.6).
- **Sizes.** Idle and running sandboxes share one memory budget per worker: 125.5 GiB less 6 GiB for
  the system, 2 GiB of eviction margin and 8 GiB for four gate pods leaves 109.5 GiB. Build
  sandboxes request 16 CPU with no CPU limit and 24 GiB of memory, request equal to limit (the
  heaviest job peaked at 15.7–16.8 GiB, §B.2.2), so four fit per worker, idle ones included, two of
  them pre-created and resized from the last hour's arrivals; 32 GiB would fit only three. Docker
  sandboxes take 8 CPU plus 4 for dind and 24 GiB, for Docker-needing binaries only; the runner
  root, touched by 55 of 100 merges, builds in the build class. Gate: 1 CPU, 2 GiB.
  `CARGO_BUILD_JOBS` and nextest's threads equal the sandbox's CPUs, because a pod without a CPU
  limit sees all 96. Elastic cgroup slots (5–6 build slots per worker instead of 4 [E]) are an
  experiment beside overlayfs.
- **Page-cache budget.** Clones do not share page cache, so a build sandbox's cache counts inside
  its own 24 GiB limit: about 7 GiB above the heaviest job's peak before the kernel reclaims clean
  pages, against 14.3 GB for a fully read runner-workspace target [M, section 5.3]. The agent
  records memory pressure per sandbox; overlayfs is the remedy if reclaim slows builds.
- **Node settings.** Kube and system reservations (3 CPU, 6 GiB), a 2 GiB `memory.available`
  eviction threshold, `podPidsLimit` 16384, site topology labels. Job work directories move in phase
  1 to local-path volumes, which are XFS with reflink, while `emptyDir` is ext4 on the OS disk and
  cannot clone [M, section 5.3]; each gets an XFS project quota, since local-path enforces no size;
  if the kubelet root moves too, eviction thresholds become fixed sizes (`nodefs.available: 100Gi`;
  5% of a 50 TB volume locks out 2.5 TB). The toolchain is read-only only with a workflow that
  expects it: today's fails on a read-only `RUSTUP_HOME` (`rust_installed_manifest_unwritable`,
  `ci.yml:427,438`).
- **Reserved merge path.** Each site keeps one 16-CPU build slot and 2 gate CPUs for merge-group and
  gate work, about 7.7% of allocatable CPU, instead of relying on preemption: at 95% utilisation
  waiting grows to about 19 times the service time. The wait is measured from eligibility. A
  three-day experiment cordons one worker as a merge and gate lane and compares queue waits.
- **Priorities.** `velnor-merge` > `velnor-gate` > `velnor-pr` = other organisations' CI >
  `velnor-batch` (builder, speculation, scheduled runs); only `velnor-batch` is preempted.
- **Isolation and sites.** Build sandboxes get a default-deny NetworkPolicy with an allow-list, no
  service-account token and no host paths except the agent's read-only mounts. Each site has its own
  dispatcher, `velnor-hooks`, ledger, store and `mbx-cache`; a request stays in one site; the
  tainted docker pool has one node per site. ChainArgos and jackin keep their ARC scale sets until
  they move to Velnor (owner question 9), with a guaranteed CPU quota and separate namespaces and
  stores; nothing crosses an organisation.

## 8. Servers and cluster tuning

Ranked by gain per effort. "Measure first" items get no credit until a worker shows the stall.

| Rank | Change | Expected gain | Evidence | Risk | Rollback |
|---|---|---|---|---|---|
| 1 | Gate jobs on the cluster in a warm gate class | −5.8 to −57 min per run | [M] §A T5 | Gates compete with builds | Labels back to hosted |
| 2 | Honest requests, reservations, eviction threshold, PID limit, per-org quotas | Removes the 1.2–2.4× stretch under load | [M] runner-host 3.7 → 8.8 min (§A T6); 84-core bursts on 2-CPU requests (§B.2.2) | Fewer concurrent pods | Revert values |
| 3 | Remove `build.jobs=2` and `--jobs 2`; parallelism from the sandbox | `maintenance-helpers` 8.5 → about 5 min | [M] `mise.toml:90-193`; [E] `proposals/opus-5.5.md` | None | Revert lines |
| 4 | Toolchain, tools and action archives as read-only node stores | 14–18 s per job | [M] 10 s tools (§A T9b) + 4.4 s actions | Stale tool (keys are versions) | Remove the mounts |
| 5 | Docker only in the docker class | 3–4 s per job, fewer privileged pods | [M] 57 dind probe failures per hour (§B.6) | A misrouted job fails fast | Label back |
| 6 | Job work directories on the XFS volume (local-path, reflink) with project quotas | A warm target in 0.25 s and 2.5 MB instead of 8.6 s and 10.75 GiB; enables every L1 store | [M] section 5.3; `emptyDir` is on the ext4 OS disk (`velnor-runners/PLAN.md:45,72-80`) | A volume without a quota fills the disk | Back to `emptyDir` |
| 7 | Warm node snapshots with plain cargo; `mbx-cache` across nodes (section 5.1) | From 46–278 compiled crates per job to the PR's own delta; 0 for an unchanged tree | [M] §A T9a; section 5.3 | A stale view is slow, not wrong (C6) | Clone nothing |
| 8 | Crates mirror through environment variables | 1–3 s per job, no internet dependency | [M] §B.4.2 | Mirror outage | Unset the variables |
| 9 | Site labels and request stickiness | No 35 ms round trips for artifacts | [M] §B.5 | Unbalanced sites | Remove the preference |
| 10 | Debuginfo: line tables for workspace crates, none for dependencies | −10 to −24% cache size; −20 to −27% CPU on a runner-host rebuild | [M] I108; `proposals/opus-5.5.md` | Poorer backtraces | Profile setting |
| 11–15 | Governor or EPP to performance (2–10%); same-site 10 Gbit/s (needed for M6); static CPU manager on one node (0–15%); THP and writeback (credit 0); mold (small: Rust ≥ 1.90 links with rust-lld) | measure first | not readable read-only (§B.1.6) or unmeasured | Power; lost burst; stalls; link bugs | Revert each |
| — | Striping the data volume; tmpfs work directories | rejected | FI workers have one data disk; reflinks cannot cross into tmpfs; the slow tests are CPU-bound (I099, I100) | | |

SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 9. Moonshots

Each moonshot states what must be true, the design element that attacks it, and the cheapest
experiment that proves or kills it; none is dropped for being hard, only when its experiment shows
it cannot keep C1–C7. Every experiment runs in week 1 beside phase 0, on velnor-lab or a drained
worker, off the required path.

| Id | Moonshot (phase; status) | Must be true | Attacked by | Cheapest experiment; killed if |
|---|---|---|---|---|
| M1 | PR verdict under 60 s for the median PR (3; stretch, committed ≤ 90 s) | The push webhook arrives within the 8 s GitHub needs to create the first job; the delta build takes ≤ 15 s, the lanes ≤ 10 s, the job ≤ 5 s more | `ensure` on the push webhook, and the native executor's job start within 1 s (section 3.4) | 2–3 days on velnor-lab: a webhook receiver, one warm pod and a one-job workflow; 50 pushes recording webhook arrival, first cargo command and verdict. Killed if webhook arrival P50 exceeds 8 s; the target then becomes 90 s |
| M2 | Push-to-merge under 5 min for the median PR (committed ≤ 5 min in 2 and ≤ 3 min in 3; stretch 100 s in 4) | A short verdict keeps the window short; auto-merge is always on; merge groups find their receipts by key | `ensure` on the push, root and package keys, queue-tip speculation, the reserved merge path: at a 1–5 min window 78–89% of merge groups find every receipt and take the verdict plus about 1 min | Work item 4's weekly window replay at the measured verdict time. Killed below a 70% hit rate |
| M3 | Zero cold start: the first cargo command 3–4 s after the push (3; stretch) | Merge tree and plan take ≤ 2 s; a pre-created sandbox has its view in ≤ 1 s | `ensure` on the push, pre-created sandboxes, read-only stores, reflinked views (0.25 s [M]); later, microVMs restored from a snapshot for the fork and docker classes | M1's run, recording push → first cargo command. Killed if that exceeds 10 s at P50 |
| M4 | Zero duplicate verification across PR, merge queue and `main` (1–3; committed for the SHA, root and package keys; stretch below packages) | An obligation's inputs can be named | Input-addressed keys: per root in phase 2, per package in phase 3, test-binary closures in phase 4 (section 4.1) | 1–2 days on velnor-lab: run each root, then each package, from a sparse checkout of its key; a package enrols only at a 100% enforced pass that matches an unrestricted run; replay 200 merges. Package keys are killed if more than 20% of packages cannot enrol; root keys stand |
| M5 | Speculative merge queue on the queue tip (3; stretch, shadow from 0) | Velnor predicts the tree GitHub builds, the queue tip plus the PR: 122 of 193 merge-group runs were built on an unlanded group commit [M] | Ordered prefixes `T_i = T_{i−1} ⊕ PR_i` and single-ejection bypass trees (7 trees for 4 PRs) from `git merge-tree`, verified at batch priority through `ensure`; a wrong guess wastes preemptible CPU, never a verdict | A read-only shadow from phase 0 comparing predictions with the real merge-group trees over 50 merges or two weeks. Killed below a 70% match |
| M6 | Test-granularity scheduling over all 480 threads (4; stretch) | A suite's summed test time over one sandbox's threads exceeds its longest test, and binaries move within a site in ≤ 5 s | The executor spreads cases and fetches each changed test executable and its fixtures by digest; the 1.48 GB archive to four workers in 5 s would need 9.5 Gbit/s | A week of per-test CPU-seconds from nextest's JSON output, and `iperf3` between same-site workers. Killed if no suite's summed time exceeds its longest test by more than the transfer cost |
| M7 | Break the single-test floor (1–2; committed `Journal::open` ≤ 1 s and no test over 60 s; stretch 15 s) | Journal set-up dominates the slow tests, and cases can share a pre-built valid prefix | The template, prefixes built once and cloned per case, one test per case, the CPU budget, optionally optimised dependencies (question 8); the two loop splits alone leave 125–138 s and a 924.8 s test | 1–2 days on one worker: CPU-seconds of the 20 slowest tests under four profile variants, and a branch with the template. The 15 s stretch is killed if cases share state that cannot be separated |
| M8 | Verification that begins before the push (3; stretch) | Agents spend time between "code done" and "pushed" that verification can overlap | `velnor verify` sends the tree to the node mirror and calls `ensure`; the push finds receipts. Only results Velnor executed on linux/amd64 count, never a run on a developer's Mac | A week-long hook recording commit time, tree and the agents' local test time. Killed if agents rarely test before pushing |
| M9 | Zero queue wait, from eligibility (3; committed p90 ≤ 2 s and p99 ≤ 10 s; stretch 1 s) | The reserved merge path covers peak merge and gate work | The reservation per site and preemption of batch work, clocked from eligibility | Replay the busiest hour plus a site loss against the reservation, and the three-day cordon (section 7) |

## 10. Phases

The moonshot experiments run in week 1, beside phase 0 (the cache head-to-head has run); their
results, not the calendar, choose what phases 2 and 3 build. Targets are section 1.4's; none depends
on the executor cutover's date: until it, the same jobs run on ARC and call `ensure` alike.

| Phase | Scope | Exit gate (at least 300 merges or four weeks, with intervals beside each P50 and P90; at least 200 matched heads over peak and quiet periods for any P90; every PR class) | Expected saving |
|---|---|---|---|
| 0, week 1 | Configuration only, on today's runners. Gate jobs on the cluster: the generator release that moves them is merged, but the workflow was never regenerated, so regenerating it is the first measured gain. Narrow the image-candidate trigger; Source qualification on the cluster or only in the queue; auto-merge; no catch-up merges; queue depth from the replay; fixed cache paths; `event.json` race fix; push-to-merge, admission and flake instrumentation; the queue-tip shadow; the week-1 experiments | PR verdict P50 ≤ 12 min, P90 ≤ 20; push-to-merge P50 ≤ 25 min, P90 ≤ 60; hosted gate queue on the critical path p90 ≤ 1 min; admission span p90 ≤ 5 s | 5.8–57 min of gate queue per run; the cancelled-run stall shrinks from up to 2,203 s to one warm job; 7.5 min median enqueue delay |
| 1, weeks 2–3 | Job work directories on XFS; warm node snapshots for build jobs, with the builder and `mbx-cache` per site and the snapshot-miss rate recorded; selection fixed in the engine; the test-floor programme; `-j` caps removed; event-class and capability scale sets with warm pools and honest sizes; node tools and action archives; crates mirror; the push run reduced to release duties; flake machinery; tested-tree recording; on velnor-lab, the native executor, the closed action set and the conformance harness | PR verdict P50 ≤ 5 min, P90 ≤ 10; push-to-merge P50 ≤ 12, P90 ≤ 30; runner-min per merged PR ≤ 150; false failures ≤ 3%; `Journal::open` ≤ 1 s; warm view set-up ≤ 1 s at P90; the miss rate published by cause; the lab corpus's pass rate published daily | About 62 runner-min per merge from the push run; cold compiles of 196–497 crates per job become deltas; a generator or helper PR falls from 10.9 to about 3.5 min and from 52.9 to about 8 runner-min [E] |
| 2, weeks 3–6 | The executor cutover once its gate passes; `velnor-gen` cutover to one job named `Required` whose obligation steps run through `ensure`; the ledger with per-root keys; agent-made views of the snapshots; `mbx-cache` kept or retired by its threshold; the crate split; the state seal and `cache-prune.yml` retired; test-time budget | PR verdict P50 ≤ 2 min, P90 ≤ 6; push-to-merge P50 ≤ 5, P90 ≤ 15; ≥ 65% of merge groups execute nothing; CPU per merged PR ≤ 60; no test over 60 s; view set-up time on `velnor-runner-host` published by file count; after the cutover, job start ≤ 2 s at P90 | Merge groups re-run only roots whose key changed: 125 root-jobs per 100 merges instead of 500 [E]; no official-runner start or action download per job |
| 3, weeks 6–10 | `ensure` on the push and merge-group webhooks; per-package keys; queue-tip speculation; `velnor verify` as a stretch; depth from the replay | PR verdict P50 ≤ 90 s, P90 ≤ 4 min; push-to-merge P50 ≤ 3, P90 ≤ 8; ≥ 85% of merge groups execute nothing; first cargo ≤ 5 s after the push; false failures ≤ 1%; CPU per merged PR ≤ 35 | Two of three job hops and the plan job disappear; most merges take the verdict plus about 1 min |
| 4, weeks 10–18 | Moonshot M6, finer closures, the merge train if approved; image builds without a privileged daemon | stretch: PR verdict P50 ≤ 30 s, P90 ≤ 100 s; push-to-merge P50 ≤ 100 s, P90 ≤ 4 min | GitHub's 45 s of queue latency per merge, if Velnor lands merges |
| 5, after week 18 | Agent workspaces on the cluster; microVM fork and docker classes; the native executor for velnor-local and the Mac host; other organisations on Velnor | stretch: push-to-merge P50 ≤ 75 s | Verification overlaps the agent's own work |

**The executor cutover.** ARC with the official runner serves velnor-new until all of these hold:
(1) the lab corpus passes 100% against the latest official release on two consecutive nightly runs,
with every allow-list entry signed off; (2) for two weeks and at least 300 merges, a non-blocking
twin of every velnor-new job runs on Velnor, with publication off and outside the required checks,
and every verdict difference is either fixed and replayed or shown by paired reruns to be a flake on
both sides; (3) per job class, Velnor's twin is no slower than the official job at P90, and its job
start is ≤ 2 s at P90; (4) fault injection, the switch to the previous release, and control-plane
and site loss are qualified. Then `velnor-gen` routes velnor-new to Velnor's scale sets and the same
change deletes ARC's velnor-new scale sets; from then on rollback is the previous Velnor release,
never the official runner.

A phase that misses its gate does not start the next until the miss is explained. Until the cutover,
today's runners keep every phase working; after it, the previous Velnor release does.

## 11. The first ten work items

| # | Item | Scope | Acceptance | Depends on |
|---|---|---|---|---|
| 1 | **Warm node snapshots** | Puts the measured decision (section 5.3) into service: job work directories on local-path XFS volumes with project quotas; per-node warm snapshots of `main` cloned into each build job at `/w/velnor-new`, with `velnor-ci inputs restore`'s time rule; the builder and an `mbx-cache` per site with TLS, garbage collection, a disk budget and write rules; the snapshot-miss rate per job by cause; the open measurements (a one-file edit, four concurrent jobs with memory pressure, `CARGO_INCREMENTAL` on) and the reproducibility gate C6 needs (one commit built cold on two nodes and warm, every test binary compared) | An unchanged tree compiles 0 crates with set-up ≤ 1 s; a changed file carrying an older time is still rebuilt; a one-file edit compiles only its crate and dependents; the daily report shows the miss rate by cause; the `mbx-cache` threshold is evaluated after four weeks | Nothing |
| 2 | **Cluster classes on today's runners** | Regenerate and land the workflow from the generator release that moves Plan, Ledger CLI, the lints and both required jobs to a `velnor-gate` scale set with warm pods and no dind; narrow the image-candidate trigger; Source qualification on the cluster or only in the queue; the event-class and capability scale sets with section 7's sizes; reservations, eviction and PID limits; per-org quotas; the `-j` caps removed; toolchain, tools and action archives as read-only mounts | Over 50 PR runs, hosted queue on the critical path p90 ≤ 1 min and no CI job on a hosted label; job-duration p90/p50 ≤ 1.3 at peak; tool setup ≤ 2 s; no OOM kills in a week | Nothing |
| 3 | **Queue, procedure and the push run** | Auto-merge enabled when agents open a pull request; no catch-up merges; queue depth only as section 6.5 allows; required checks bound to the Actions integration; the list of a HEADGREEN group's intermediate commits for release tooling; the push run keeps one real `Required` job, which skips each obligation the merge group verified on the same tree (never on the group run's conclusion alone) and then does release duties | ≥ 90% of merged PRs with auto-merge; green → enqueue P50 ≤ 30 s; ≤ 2 runner-min per matching push; `velnor-ci release-gate` and `product-release.yml` pass on `main`, finding exactly one Actions `Required` job in `ci.yml`'s push run | Owner questions 3, 5 and 6; work item 4 for the push run |
| 4 | **Measurement and shadows** | A push-to-merge collector clocked from the final commit's accepted push; admission span and cancelled-run stall; queue residence and merges per hour; tested tree, root keys and obligation digests in every plan's output; the paired-rerun sampler; the queue-tip shadow (M5); webhook arrival (M1); the weekly window replay (M2); the post-merge watch (section 12) | A daily report with P50, P90, intervals and denominators that reproduces the 85-min baseline | Nothing |
| 5 | **The test floor** | The journal template; one test per case for the two loops; prefixes built once and cloned per case for the receipt-integrity and publication-recheck tests; the 20 slowest tests ranked by CPU-seconds alone after each change; private temp directories and ports for the 12 serialised archive-guard goldens and the `crates_mirror` tests, so `threads-required` goes; the 12-case `blob_faults` loop split; the 89 s fixture folded into the warm `--all-targets` build; the per-test budget; `ci-helper` tests in parallel with sized timeouts; the `event.json` snapshot | `Journal::open` ≤ 1 s on a worker; the list shrinks; no new false failures in 100 paired reruns | Nothing |
| 6 | **Selection in the engine** | Declare the crate-root build scripts' inputs and the test reads the plan names; replay the last 200 PRs through the fixed planner in shadow | The replay selects every job that failed in those PRs; a generator or helper PR runs ≤ 4 min | Nothing |
| 7 | **The native executor (MVP)** | `velnor-dispatch` and `velnor-exec` on velnor-lab: scale-set sessions, broker long-poll and JIT identities held by the dispatcher; placement before acquisition; `acquirejob`, `renewjob` every 60 s and `completejob`; logs, timeline and step summaries through Results; the expression engine and protocol clients copied from velnor-legacy with their tests; contexts, steps and composites, workflow commands and env files, masking, post steps, cancellation and timeouts; the journal and recovery from velnor-new's runner; refusal of anything outside the closed set before a step runs | Every job of `ci.yml` runs green on velnor-lab; job start ≤ 1 s at P50 and ≤ 2 s at P90 from the broker message; a crash at every step boundary leaves no job reported twice; a check on endpoint strings finds no legacy API | Nothing |
| 8 | **The conformance harness** | `velnor-conformance` with section 3.6's corpus, pairing, normalisation and pass criteria; the latest official runner on velnor-lab as the oracle; a blocking check on executor changes, a nightly run, and a run within 24 h of each official release; the twin-job shadow and the cutover gate's report (section 10) | It catches each of ten planted differences (a wrong output, a secret left unmasked, a reordered or missing post step, a wrong `cancelled()`, a lost annotation, among others); the corpus's pass rate is published daily per part | 7 |
| 9 | **Native actions** | Section 3.5's closed set in Rust against node-local state (the Git mirror, node and site stores, the tool store); refusal of anything else with a clear error; `velnor-gen`'s closed-set test and qualified SHAs; `velnor-ci` commands for the three `actions/github-script` uses; the qualification workflow's Docker and node24 cases as refusal checks | Every action passes the harness at the SHAs `velnor-gen` emits; a checkout of a warm tree takes ≤ 1 s; no job downloads an action | 7, 8 |
| 10 | **The ledger, `ensure` and the generator cutover** | `velnor-hooks`, the ledger with per-root keys, per-obligation receipts and in-flight join, the signer and append-only log, the agent's executor in pre-created sandboxes; `velnor-gen` writing one job named `Required` whose obligation steps run through `ensure`, with parity: the local-mode configuration, the maintenance checks, both required names, the three example-binary fixtures as test set-up in `.velnor/rust-ci.toml`, the mise pin the suites expect (2026.11.1; the golden carries 2026.11.9), and no `edited` trigger once the ledger re-plans a retarget without restarting identical work; a week of shadow runs comparing obligation inventories; on velnor-lab, the sparse-checkout experiment and M1's measurements | Empty inventory difference over 100 runs; PR verdict P50 ≤ 2 min; over 50 lab pushes, webhook arrival P50 ≤ 8 s and verdict P50 ≤ 60 s for a generator-sized change; a killed webhook still yields a correct verdict; at most 20% of packages fail to enrol; a merge group's recomputed inventory matches per obligation, configuration and test inventory; 0 disagreements in 500 sampled re-executions, reported with its bound (about 0.6% at 95%) | 1, 5 |

## 12. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A receipt or a selection gives a false green | Shadow re-execution of new classes; the audit sample; cold differential builds; every landed tree in 24 h; undeclared-read telemetry | Retire the key version; work runs again; revert the bad merge by a normal PR |
| A bad change lands anyway | The post-merge watch: revert rate, red-`main` rate, failures on receipt-passed trees, cost per merged PR | Raise the policy epoch; pause reuse for the affected classes |
| Velnor's executor departs from GitHub's semantics, or a cutover drops a configuration or check | The harness on every change, nightly and for each official release; the twin-job shadow; the shadow inventory comparison | Block the release; switch to the previous release; before the cutover, today's runners keep serving |
| GitHub changes webhooks, the merge queue or the runner protocol | The harness within 24 h of each official release; a canary job every 10 min; a tracker of GitHub's deprecation dates | Fix and release inside the 63-day version window; a job's own `ensure` covers webhook gaps |
| A stale compile view or a poisoned cache hides a failing build | The cold differential build disagrees with the warm path; work item 1's changed file with an older time; receipts record producer and scope, and only the builder promotes | Retire the lineage or delete the generation; requests build cold, slowly and correctly |
| The ingress, the ledger or the dispatcher is down | Replay backlog; ledger and dispatcher health; jobs waiting at GitHub | Obligations run fresh, slower but never wrong; the dispatcher's standby reopens sessions from its journal |
| False failures rise as work packs denser | The paired-rerun sampler; flaky counts per test | Lower the per-node build cap; keep the queue shallow; quarantine only as section 1.3 allows |

| Reuse degrades unseen | Before phase 3: hit rate by mechanism, builder lag, the audit's false-green bound, placement regret | Drop placement to site stickiness if a site fetch at P90 costs no more than a delta rebuild at P50, or regret stays under 5 s P50 |
| A signer or policy is revoked, a signing key leaks, or a forged receipt reaches a verdict | Every verdict carries its policy epoch; signature, class and log entry are checked; audits run on another node | Raise the epoch; revoke the key and its receipts; pause and re-verify queued entries; list landed commits that relied on them |
| A new planner, sandbox or verifier passes a change ordinary CI would fail | A corpus of known-bad changes with green controls (a new failing golden, an omitted `local-mode` case, a stale publication attempt, a broken queue prefix repaired by the next PR), replayed after every such change, warm caches included | A candidate that passes one bad change loses authority for that class |
| FI workers or `mbx-cache` run out of disk | Store budgets, alerts at 70% | Least-recently-used eviction, per-org caps, `mbx-cache` garbage collection |
| Other organisations, speculation or the builder starve real work, or the reverse | Wait per organisation; batch-class CPU share | Quotas and the reserved merge path; batch work at the lowest priority, preempted first |
| The new code grows like velnor-legacy | Line budgets checked in CI; phase gates | Stop and review at a budget; nothing is copied from velnor-legacy without its tests and an oracle pass |

## 13. Owner questions

The design assumes each recommendation until the owner decides otherwise; question 1 is decided.

| # | Question | Example | Recommendation |
|---|---|---|---|
| 1 | **Decision taken: Rust only.** `docs/goals.md:16` says jobs are always executed by GitHub's official runner; the vision paragraph (line 20) and design rule 11 (line 75) assume it too. Velnor now runs no official runner in production. | Line 16 describes the official runner inside "the containers that `velnor-runner` starts"; that executor is superseded. | The owner rewrites those three lines, for example: "Jobs are executed by Velnor's native executor; GitHub's official runner is the conformance oracle". `docs/hard-decisions.md` names no runner and needs no change. |
| 2 | Does a receipt with the same input key count as "ran and passed" (`docs/goals.md`, "Green means verified")? Merge groups keep running in full, as hard decision 3 says, and receipts satisfy most of it. This replaces the earlier question about selecting merge groups by change, which the review showed unsound. | A generator-only pull request's merge group lists every root; four roots have `main`'s keys, so receipts satisfy them and only the generator root runs: about 1 min instead of a 3–5 min warm full run. | Yes, under C1–C7 and the audit. |
| 3 | HEADGREEN or ALLGREEN? HEADGREEN lands a group when its tip passes, so a failed entry can land inside a passing group; it happened once in 15 days. | Entry A fails, entry B (main + A + B) passes, and both land; with ALLGREEN, A is ejected and B rebuilt without it. | Keep HEADGREEN while merge groups are slow, with C1 stated for tips and intermediate commits listed; switch to ALLGREEN once queue-tip speculation keeps the bypass trees verified, which makes an ejection nearly free. |
| 4 | Stay on the Free plan? `docs/goals.md:45` keeps the generator inside free limits. | On the worst day the three gate jobs waited 57 of a 71.8 min run for the plan's 20 hosted slots; after phase 0 they run on the cluster, and only the hosted cases of question 7 remain. | Stay on Free; pay only if forks or release builds queue, measured. |
| 5 | Should agents change how they merge? | 0 of 40 recent merged PRs enabled auto-merge, and the median PR waited 7.5 min between green and enqueue. | Agents enable auto-merge when they open a pull request, stop merging `main` into their branches, and later run `velnor verify` before pushing. |
| 6 | Which ruleset changes? | Today any app, or a workflow token granted `statuses: write`, can post a green `Required`. | Bind `Required verdict` and `Required` to one integration now; raise `max_entries_to_build` only when the replay shows more merges per hour at the under-load false-failure rate; at the `velnor-gen` cutover accept a single `Required` name, keeping the push run's one Actions `Required` job that the release gate reads; set the grouping by question 3. |
| 7 | Do fork pull requests and macOS release builds count as "production" under the Rust-only decision? Both run on GitHub-hosted runners, GitHub's service, which contains GitHub's own runner; neither is a fallback for Velnor. | A fork's pull request needs isolation Velnor does not have yet, and `generator-release-build-macos` needs a Mac. | Keep both hosted for now: forks only after a maintainer approves, until the microVM fork class passes conformance (phase 5); macOS builds until Velnor runs on a Mac host. |
| 8 | Build dependencies at `opt-level = 3` in tests? | It cut the slowest test class's CPU by 66% [M, `proposals/fable-5.2.md`], measured with debug assertions off in dependencies; with them on, nobody has measured it. | Measure the four combinations of `opt-level` and debug assertions, with overflow checks fixed, under design rule 14, and decide on both numbers. |
| 9 | Should ChainArgos and jackin share velnor-new's PR priority with a guaranteed quota, and when do they leave ARC for Velnor? | A ChainArgos PR job and a velnor-new PR job queued together start in arrival order, while velnor-new's merge-group work goes first. | Equal PR priority with quotas now; they keep their ARC scale sets until velnor-new's cutover has soaked for four weeks, then move (phase 5). |
| 10 | Rent a high-clock node, and set the trigger for more workers now? | The longest test took 924.8 s on an EPYC core; desktop-class cores at 5.5 GHz or more would run the serial path 1.3–1.5× faster [E]. | Rent one in phase 1 for the test-floor experiments; buy a worker when CPU stays above 70% at peak after dedupe, or the eligible wait for merge work exceeds a p90 of 10 s for two weeks. |
| 11 | A Velnor merge train instead of GitHub's merge queue? The queue adds 19 s to start a merge group and 26 s to merge it [M], the largest term once a verdict takes under a minute. Velnor could fast-forward `main` to the exact commit whose obligations all hold, a compare-and-swap that verifies every tip by construction. | A green pull request lands 5 s after its verdict instead of about 50 s; the costs are that squashed pull requests must still show as merged and a Velnor outage stops merges until the queue rule returns. | A two-week shadow train in phase 4, killed below a 95% tree match, then decide. |
| 12 | One counted re-execution for tests the change could not have broken? Retries are 0, so every flake ejects a queue entry and rebuilds the entries behind it, although a test whose package closure the change did not touch has `main`'s inputs, where it passed. Tests whose closure changed never get a retry; they run three times. | A generator-only change hits a timing flake in a `velnor-runner-host` test; one immediate re-execution passes, is recorded in a non-blocking `Flaky` check and charged to the budget, and the merge proceeds. | Yes, after replaying the evidence's 10 false failures against their pull requests' closures. |

## 14. What more hardware would buy

| Purchase | Buys | When |
|---|---|---|
| One high-clock node (desktop-class cores at 5.5 GHz or more) | 1.3–1.5× on the serial path: rebuilding `velnor-runner-host` and the longest tests [E] | Rented in phase 1 for the test-floor experiments |
| 10 Gbit/s between same-site workers | M6 and site replication; 1.48 GB of test binaries in about 1.2 s instead of 12 s [E] | When M6 shows a suite that needs it |
| Two more k3s servers, across failure domains | A control plane that survives one node or site; no speed | Before the executor cutover |
| More NVMe on the FI workers | Cache headroom on the 3.5 TB data disks | When eviction drops live generations |
| More EPYC workers | +96 threads each, about +20% capacity | Trigger set now: CPU above 70% at peak after dedupe, or an eligible-wait p90 above 10 s for two weeks |

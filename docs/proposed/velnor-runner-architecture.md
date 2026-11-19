# Velnor runner architecture

**Status:** Proposal (v3); it replaces nothing yet. It follows the owner's binding decisions 1–24
(section 5.1). Facts the panel still contests are marked **pending ruling** and run on the safe
default meanwhile; questions only the owner can answer are in section 14. **Scope:** how Velnor
verifies, merges and releases velnor-new's changes on the project's own machines, so that CI stops
being the slowest part of delivering a change, with the generator (`velnor-gen`) and helper
(`velnor-ci`) where the runner needs something from them.

**Sources.** The [evidence pack](../research/velnor-runner-2026-11/evidence.md) (cited as §A T5 and
similar); the cache and blaze experiments under `../research/velnor-runner-2026-11/experiments/`;
the panel's nine Rust-only redesigns and their comparison, and the [delivery
plan](../research/velnor-runner-2026-11/delivery-plan.md), under
`../research/velnor-runner-2026-11/`; and the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea with its decision and
who raised it. Numbers are tagged **[M]** measured, **[E]** estimated (with the arithmetic), **[C]**
read from code at the cited path, or **[T]** target. A target is **committed** when the evidence
makes us confident and **stretch** when an experiment must prove it first.

## 1. Goal, metric and the contract

### 1.1 The metric

The single metric is **push-to-merge**: from the final labelled push to a pull request's head until
the pull request is merged. **PR verdict** runs from the label, or from a labelled push, to a green
`Required` check. Both are reported weekly as P50 and P90 with intervals, over every merged pull
request and for every PR class (generator and helper, runner, engine, local, no Rust, mixed).
Because CI starts on demand (decision 12), the wait between a push and its label is reported beside
them, and so are three guardrails: time to the first actionable failure, first-push-to-merge, and
failed attempts per merged change. Runner-minutes (decision log I117) count each occupied slot once,
split completed, cancelled and superseded work, and charge the builder, speculation and audits to
the consuming repository.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M] 98 merged PRs, `proposals/opus-5.5.md` §1.1 |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M] §A T2 |
| PR runs cancelled; runs on an unchanged head | 1,156 of 2,558 (45%); 469 (18.3%), the `edited` trigger | [M] §A T1; F-H07 |
| Admission span, PR runs | 2 s p50, 226 s p90, 2,203 s max; a superseding run waits 60 s p50, 626 s p90 | [M] 200 sampled runs (F-H06) |
| Runner-minutes per run (PR / merge group / push) | 52.9 / 64.6 / 62.0 | [M] §A T2 |
| Runner-minutes per merged PR | 285–347 for CI; about 460–515 with every workflow | [E] decision log; the panel's audits |
| Full runs per merge | 3 (PR, merge group, push on the same SHA in 120 of 159) | [M] §A T7 |
| False failures per full run | 8.1%, one direction (10 of 124 push runs) | [M] decision log S1 |
| Cross-job compile reuse on the cluster | 0 (sources 0 of 48, toolchain 0 of 58, state 0 of 3,636) | [M] §A T9a, T9b |

### 1.2 Same confidence: the contract

Correctness is the hard constraint and speed is optimised only inside it (decision 11): anything
that could turn a real failure green is rejected, however fast. False reds have a budget; false
greens have zero tolerance. "Same confidence" is this contract, and a change that breaks a clause is
rejected.

1. **C1. Every commit that lands on `main` is verified.** For every commit that becomes part of
   `main` and every obligation the plan lists for it, a passing result exists whose key equals the
   obligation's key at that commit, produced by an executor run in production role (section 5.3). An
   *obligation* is one unit of verification: format, lint, docs, a per-crate check, one test binary
   under one configuration, or a named check. The merge queue's ALLGREEN grouping keeps this true
   for every landed commit, not only for a group's tip (pending ruling Q10; ALLGREEN is the safe
   default).
2. **C2. Keys capture every input, by construction.** A key covers the sources, every workspace
   manifest, the toolchain, dependency versions, environment, flags, features, the binaries a test
   executes (`CARGO_BIN_EXE_*`), data files, and the executor's and environment's identity (section
   6.1). The sandbox holds the full tree: cargo refuses `-p` builds when another member is missing
   [M, Opus 5.5], so a sparse sandbox cannot even load the workspace. Reads outside a key's paths
   are observed (section 6.1) and demote the result to exact-tree scope.
3. **C3. Selection is sound or wide.** The plan comes from a released helper, never from the change
   under test (`docs/goals.md` rule 12). A file the plan cannot place selects everything. Nothing is
   skipped relative to a merge group's predecessor; an obligation is skipped only when a receipt
   with its exact key exists. The expected test inventory, planned arguments and feature policy come
   from the trusted planner, never from the candidate, and a head whose plan drops obligations or
   tests that `main`'s plan has lists them as a reduction in the `Required` summary, so a change
   cannot quietly weaken its own tests.
4. **C4. Only what Velnor observed is evidence.** A receipt is minted only from processes the node
   agent started in a sandbox it created, running the released plan's command graph on the inputs it
   mounted, with every exit and the expected test inventory observed. A missing, skipped, cancelled,
   masked or zero-test result fails. Results are never reused across executors, and a reference run
   of the official runner (section 5.3) never produces a verdict, receipt, cache entry or release.
5. **C5. Uncomparable work always runs, and the sandbox hides what a key does not hold.**
   Obligations that read the network, the event payload, the clock in an assertion, unseeded
   randomness, Docker images not pinned by digest or the commit's identity, or that publish
   anything, run fresh every time. Tests get a loopback-only network and no event file; sandboxes
   have no `.git`, a scrubbed environment (no `GITHUB_*`; fixed `HOME`, `PATH` and locale) and a
   fixed CPU count per class.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow with every
   reuse re-executed until about 3,000 clean comparisons per obligation kind; then 2% are
   re-executed. Every landed commit is rebuilt cold and its changed obligations re-run at batch
   priority (audit A1, section 6.5). A disagreement is re-run five times to separate a flake from a
   wrong key; a confirmed one revokes the key version by policy epoch.
7. **C7. Flakes are budgeted** (section 1.3).

Every obligation whose inputs the change touched executes **twice, in parallel on two nodes**,
before merge, matching today's two pre-merge runs (the PR run and the merge group); any failure
blocks. A third execution is pending ruling Q12. A regression that fails with probability p per
execution is then caught with 1−(1−p)², as today.

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes; it costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** at most 3% false failures per full required run once verification moves off today's
  full runs, and 1% at the committed targets, each claimed only after about 300 rerun pairs. Today's
  8.1% [M] is a one-direction rate from full runs; running only changed closures removes most of it,
  because a flaky test elsewhere can no longer eject anyone.
- **Measure:** a paired-rerun sampler re-runs green and red results in both directions under
  representative load. Zero disagreements in 299 pairs give a one-sided 95% bound just under 1%.
- **Report and act:** retries stay at 0, so a flaky test fails the run and is counted. A test with
  two false failures in seven days gets an owner-labelled issue; it is fixed, or quarantined only
  when a blocking check covers the same property or under an expiring owner exception.
- **Known causes first:** CPU starvation of tight timeouts, such as the 2 s kill in
  `ci-helper/.config/nextest.toml`'s default profile under pods that request 2 CPU and burst to 84
  cores (§B.2.2); the `event.json` rewrite race between steps (I023).

### 1.4 Targets

From the delivery plan (section 11), measured on the same clocks:

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Conformance |
|---|---|---|---|---|
| Today [M] | 26.7 / 92.3 min | 85 / 457 min | 285–347 (CI) | — |
| Committed [T] | ≤ 90 s / ≤ 4 min | ≤ 3 / ≤ 8 min | ≤ 35 | 100% on both backends |
| Stretch [T] | 30 s / 100 s | 100 s / 4 min | ≤ 15 | 100% |

Further committed figures: job start ≤ 1 s at P50 and ≤ 2 s at P90 from the broker message;
`Journal::open` ≤ 1 s and no test over 60 s alone (stretch 15 s); per release, promotion only: ≤ 1
runner-minute and zero compiles.

## 2. Where the time goes, and what changes the problem

1. **Waiting, not working** [M]. A PR run with no hosted queue takes about 11 min, but the median PR
   waits 85 min from push to merge. On the worst day three serial gate jobs waited 57.0 of a 71.8
   min run for the Free plan's 20 hosted slots (§A T5, T11); a superseding push waits up to 2,203 s
   for the cancelled run's `always()` job; the queue builds 2 entries; no agent enables auto-merge.
2. **The same work many times** [M]. 45% of PR runs are cancelled; each merge runs the full job set
   on the PR, the merge group and the push of the same SHA; third-party crates compile 4.4 times per
   run; releases rebuild what `main` already built (section 4).
3. **Every job is cold** [M]. Nothing compiled survives a job (§A T9a, T9b); the one restore that
   exists took 44–61 s for 1.92 GB, longer than the build it saved (I036).
4. **Code sets the floor** [M]. Two tests loop over 40 and 6 cases (258–274 s, 203–271 s) and the
   floor behind them is still 125–138 s; `maintenance-helpers` compiles with `build.jobs=2`;
   selection forces every crate with a build script (I059, I061, I068).

The design removes work rather than speeding it up. A push starts nothing until the label asks for
it. Each unit of work runs once per unique input and every later stage reuses it by content address;
a release promotes the bytes `main` verified. Every node keeps `main` warm and mirrors every
upstream, and Velnor executes jobs itself, natively, next to that state. Velnor Native wins on
locality and on removed round trips, not on the language: starting Node costs 87–147 ms per action
[M, `fable-5.2`].

## 3. The latency budget

One typical PR (the 47% class that touches only `generator/` or `ci-helper/`), labelled, from push
to merged. Today's figures are measured; v3's are committed targets, with the estimate in brackets.

| Stage | Today [M] | v3 [T] (estimate) | Bottleneck removed |
|---|---|---|---|
| 1. Event, webhook, job offer | push → run 1–2 s; run → first job 2 s p50, 226 s p90; hosted Plan queue 5.5 min median | webhook 1–2 s | Verification starts from the label or labelled push; the job offer is off the verdict path (pending ruling Q2, Q14) |
| 2. Acquire | 12 s p50, 18 s p90 from job created to started (scale set) | ≤ 0.3 s | Pre-registered JIT identities; placement before acquisition |
| 3. Sandbox start | ARC pod about 6 s (436 starts) plus a dind probe | ≤ 0.3 s | Warm pool; reflink clone 0.25 s [M] |
| 4. Checkout | 6 s per job × 31–39 jobs; Plan 12 s | ≤ 1 s | Node mirror; a reflink clone of the `main` tree plus the changed files (section 7.4) |
| 5. Toolchain and tools | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | Read-only verified tool store; native actions |
| 6. Dependency fetch | 3.5–6.1 s per job; sources cache 0 of 48 | 0 s (≈ 3 s on a lockfile change) | Site crates mirror and node crate store |
| 7. Compile | 196–497 crates per job; 81 s runner-host, 14 s velnor-ci; 8.5 min critical path | ≤ 25 s (15 s) | Warm snapshot: only the change's closure compiles; 0 crates and 0.31 s for an unchanged tree [M] |
| 8. Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s; floor 125–138 s | ≤ 60 s (10 s) | Only changed closures run, twice in parallel; the test floor programme |
| 9. Result reporting | 1.48 GB archive upload 36–43 s; `Required verdict` 49 s | ≤ 1 s | Results streaming; binaries by digest in the site store |
| 10. Required check | hosted verdict queue 1.6 / 24.3 min, then `Required` 1.9 / 21.4 min | ≤ 1 s after the last receipt | One App-written check (pending ruling Q2) |
| 11. Merge-queue entry and run | green → enqueue 7.5 min (0 of 40 auto-merge); enqueue → group 19 s; group 19.9 / 84.1 min | ≤ 30 s (≈ 25 s) | Auto-merge; exact-tree or per-obligation lookup |
| 12. Merge | 26 s | 26 s | GitHub's queue; irreducible while GitHub lands merges (decision 7) |
| **Verdict / push-to-merge** | **26.7 / 92.3 min; 85 / 457 min** | **≤ 90 s / ≤ 4 min; ≤ 3 / ≤ 8 min (≈ 35 s; ≈ 85 s)** | |

The floor while GitHub owns the merge is about 50 s: delivery 1–2 s, the merge queue's 19 s and 26
s, a few API round trips, plus the longest changed test and one compile of the change. With an
Actions job as the required check it is about 58 s, because GitHub takes about 8 s to offer a job
[E; pending ruling Q14]; with the App check, about 47 s.

## 4. The duplicate-work audit

Every repeated operation across PR, merge group, push to `main`, release, scheduled runs, warmers
and conformance [M from the evidence pack and the panel's audits, which read `.github/workflows/*`,
`ci-helper/` and `generator/`]. Decision: **E** eliminate, **R** reuse or promote by content, **K**
keep deliberately.

| # | Operation | Runs in | Same input | Today | v3 | Mechanism |
|---|---|---|---|---|---|---|
| 1 | Runs for unrequested pushes (draft, unlabelled, ready for review) | PR | — | 45% of PR runs cancelled; 4.77 PR runs per merged PR | E | Label flow (section 8.2) |
| 2 | Re-runs on an unchanged head (`edited`, reopen) | PR | yes | 18.3% of PR runs, 52.9 runner-min each | E | No trigger; exact-tree verdict |
| 3 | Rebase-only push with the same tree | PR | yes | a full run | R in ≤ 2 s | Exact-tree receipt |
| 4 | Push N, then N+1 | PR | partly | heavy jobs in 97.4% of runs; 60 s / 626 s stall | R the unchanged closures | Closure keys; in-flight join; cancel without waiting |
| 5 | Concurrent, stacked and identical PRs | PR | partly | no cross-PR reuse | R | Content keys; candidate receipts (section 6.2) |
| 6 | Agent pre-push test, then CI | pre-push, PR | yes | unmeasured | R only through `velnor verify` in a sealed Linux sandbox | Receipts |
| 7 | Plan job, Ledger CLI build and 31 downloads, `Required` aggregation | every run | yes | Plan 0.9 min plus 5.5 min hosted queue; Ledger CLI 22 s + 31 × 3.2 s; `Required verdict` 49 s | E | Released planner and helper in the tool store; App check |
| 8 | Checkout, toolchain, mise, action downloads | every job (31–39 per run) | yes | 14–18 s per job; 7.5–8 min per run | E | Node mirror, read-only stores, native actions |
| 9 | Crate fetch | every Rust job | yes | 4–5 s per job | R once per lockfile | Offline crate store |
| 10 | Third-party crate compile | every Rust job | yes | 2,106 builds of 475 crates per run (4.4×; another count gives 2,260 of 463, 4.9×); 6.3–21.7 GB written per job and discarded | R once per lockfile, toolchain and profile | Warm snapshot |
| 11 | Workspace crates in several configurations | runner-host default and `local-mode` | partly | 404–407 crates twice | R once per closure key | Crate split; `velnor-runner-local` extracted |
| 12 | Clippy, doc, check-alone | every run | different outputs | clippy 36–75 s, doc 30–65 s per job | K as parallel lanes; R per key | One target lane per check kind |
| 13 | `-j2` caps, the 89 s image-publication fixture, the CLI built ≥ 9 times per event | several jobs | yes | caps cost 4.5 of 8.5 min | E / R once | Caps removed; fixture in the warm build |
| 14 | Tests across PR, merge group and push | three runs | yes | about 25 min per run | R by receipt; K changed obligations twice in parallel | Receipts |
| 15 | Full CI on the merge group | merge group | when the tree equals the PR's | 64.6 runner-min; equal trees 7 of 45 measured | R exact tree, else per obligation | Receipts; speculation |
| 16 | Full CI on the push to `main` | push | yes (same SHA in 120 of 159) | 62.0 runner-min; 8.1% false failures | E | Lookup and promotion; audit A1 |
| 17 | State seal producers and cache prune | `main` push | yes | 683 s per push; 3.49 GB of GitHub cache, 0 of 3,636 restores | E | Snapshots and storage tiers |
| 18 | Source qualification on every PR event | PR | yes | 1,007 s per event, about 80 runner-min per merged PR | R once per key, in the queue | Obligations |
| 19 | Job image candidate on every push | every push | yes unless images changed | 200 s; about 20 min per PR | R once per image-input key | Keyed action |
| 20 | Source preview build, tag rebuilds, product release | `main` push, tag | yes | preview 410–534 s per push; tools 190–434 s; product 4,105–6,238 s | R promote by digest | Release promotion (section 8.4) |
| 21 | macOS and Darwin legs | `main` push, release | — | 1,063–1,406 s per push | E (paused, decision 9) | — |
| 22 | `VELNOR_COMMIT` forces both tools to rebuild per commit | every merge | no, by construction | two tool builds per merge | E | Commit stamped after link (pending ruling Q6) |
| 23 | Per-node rebuilds of `main` by warmers | after every merge | yes | about 120 incremental builds an hour | R: the landed merge group's build output is the next generation, trusted when A1's cold rebuild gives equal digests | Generations by content |
| 24 | GitHub cache save and restore | many | — | 1.92 GB restore 44–61 s | E from the verification path | Storage tiers (section 7) |
| 25 | A Docker daemon in every pod | every job | — | 3–4 s per job; 57 probe failures an hour | E | Docker only for the three binaries that need it |
| 26 | Conformance pairs, cold audit A1, speculation | lab, batch | yes | — | K | Measured to pay: they buy confidence or wall time at batch priority |

**Totals.** Per merged PR: 285–347 CI runner-minutes today (about 460–515 with every workflow) → ≤
35 committed, ≤ 15 stretch. Per release: 68–104 runner-minutes for a product release or 3.2–7.2 for
the tools → ≤ 1, with zero compiles. GitHub cache: 3.49 GB with no restores → 0 on the verification
path.

## 5. Architecture

### 5.1 The owner's decisions

| # | Decision | Where |
|---|---|---|
| 1, 20 | Velnor executes jobs itself. Velnor Native (Rust) is the default and the recommendation; GitHub Official is a supported, deliberately configured alternative behind the same interface. Verdicts from either count; results never cross executors | 5.3, 6.1 |
| 2, 19 | The latest official runner is also Velnor's permanent reference executor: the conformance gate, sampled shadow runs, and "run both and diff". Reference runs never produce verdicts, receipts, caches or releases, and nothing falls back to them | 5.3, 5.7 |
| 3 | Latest GitHub APIs only | 5.3 |
| 4 | No Node in Velnor Native; a closed native action set; unknown actions refused before the job starts. The official executor runs Node by nature | 5.6 |
| 5, 14, 17 | Nothing is reused by default: velnor-legacy (archived, very old) and velnor-new's own code are reviewed per component, and only what is well designed is ported | 5.10 |
| 6 | Experimental dependencies are judged on evidence | 7.3 |
| 7 | GitHub stays the platform: code, PRs, the merge queue, the YAML format | 8.3 |
| 8 | No pull requests from forks; isolation for trusted authors whose code may be buggy | 5.8 |
| 9 | macOS release builds paused; every velnor-new job runs on Velnor | 8.4, 11 |
| 10 | Build once, verify once, reuse everywhere; releases promote | 4, 6, 8.4 |
| 11 | Correctness first; a four-part soundness case for every reuse | 1.2, 6.4 |
| 12 | CI on demand through the label flow; only a green latest commit merges | 8.2 |
| 13, 21 | One platform-agnostic core; Kubernetes and Docker backends now, more later; the core branches only on capabilities | 5.4 |
| 15 | Agents do the work; schedules follow the real constraint | 11 |
| 16, 18 | Per-job-type migration with no moving back; every move deletes its old route | 11 |
| 22 | Native, fast `actions/cache` with GitHub's exact semantics | 7.2 |
| 23 | Configurable storage: one abstraction, selectable backends and tiers | 7.1 |
| 24 | Node-local mirrors of the repository and of every upstream (crates, toolchains, tools, images), configurable per platform | 7.4 |

### 5.2 Components

All new code is Rust in velnor-new's root workspace; crossing a crate's line budget stops work for
review (I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| App and label flow | `velnor-control` | one service per site | Webhooks, the label state machine, the `Required` check, the reconciler, `merge_group` handling | 5k lines |
| Dispatcher | `velnor-dispatch` | one service per site | Scale-set sessions, broker long-poll, JIT identities and their credentials; placement before acquisition; lease journal | 6k lines |
| Executor interface | `velnor-executor` | library | The `Executor` trait, job model, identity | 1k lines |
| Velnor Native | `velnor-exec` | in the node agent | Contexts and expressions, steps, composites, background and post steps, commands and env files, masking, cancellation, timeouts; run service and Results | 20k lines |
| GitHub Official | `velnor-official` | in the node agent | Drives the latest official runner through the same interface (section 5.3) | 3k lines |
| Native actions | `velnor-native-actions` | in Velnor Native | The closed action set (section 5.6) | 10k lines |
| Platform | `velnor-platform`, `-k8s`, `-docker` | library | The platform trait, capabilities and the two backends (section 5.4) | 2k + 6k + 4k lines |
| Storage and caches | `velnor-storage`, `velnor-cache` | library and site service | Blob stores, tiers and routes; the `actions/cache` service layer (section 7) | 4k + 5k lines |
| Ledger | `velnor-ledger` | library and site service | Plans, keys, receipts, `ensure`, joins, promotion, audits | 6k lines |
| Node agent | `velnor-node` | DaemonSet, or a host daemon under Docker | Warm pools, observation and receipt signing, node stores, the builder, cleanup by proof | 12k lines |
| Conformance | `velnor-conformance` | velnor-lab | The differential harness (section 5.7) | 5k lines |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, obligation commands, workflow shape; `velnor-gen` emits only qualified actions | existing |

### 5.3 Two executors behind one interface

```rust
pub trait Executor: Send + Sync {
    /// Kind (Native | Official), version and image digest. Part of every result key.
    fn identity(&self) -> ExecutorId;
    async fn run(&self, job: AcquiredJob, env: Env, role: Role, io: JobIo) -> Result<JobOutcome>;
}
pub enum Role { Production, Reference } // Reference: conformance, shadow, diff; never a verdict
```

- **Velnor Native** is the default and the recommendation. It runs steps natively, implements the
  closed action set without Node, and reads the node's mirror, stores and warm snapshots directly.
- **GitHub Official** runs the latest official runner (`Runner.Worker` with a job message Velnor
  hands it) inside the same sandboxes. Both executors get the same platform backends, warm pools,
  scheduling, label flow, `Required` check, logs and timeline, ledger and caches. It runs JavaScript
  actions on Node by nature. Velnor's dispatcher drives both; ARC drives neither.
- **Choice.** `.velnor/` names the executor per job type or globally. Velnor never switches on
  failure: a different executor is a configuration change, not a fallback.
- **Identity in every key.** Results never cross executors, so switching costs one re-verification
  and can never produce a false green.
- **Reference role** (decision 19): the conformance gate, sampled shadow runs of real jobs, and
  on-demand "run this job both ways and diff". Reference runs write nothing anyone can consume.
- **APIs** (decision 3), as the latest release implements them, including its broker-session
  ownership on deletion and its connectivity probe: scale-set sessions, broker long-poll, JIT
  configuration, run-service `acquirejob`, `renewjob`, `completejob`, Results twirp for logs,
  timeline and summaries, cache service v2, artifacts v4, OIDC. Out: the AES-encrypted legacy
  session, legacy pipelines and distributed task, the legacy expression evaluator, cache v1 and
  artifacts v3. The evaluator the latest runner returns is itself pending ruling Q4; the harness
  decides it on evidence (section 5.7).

**Speed per job type** [E, from the per-job set-up in §A T9b; the paired runs of section 5.7 replace
these numbers with measured ones]. Both executors share the platform, so the difference is what the
official runner does inside the job: set-up 4–5 s, actions fetched and started under Node,
`actions/checkout` over the network, tools installed per job, and `actions/cache` through GitHub's
service (section 7.2).

| Job type | Velnor Native | GitHub Official on the same platform | Difference |
|---|---|---|---|
| Gate jobs (`Required`, Plan, lints) | ≤ 1 s set-up | 15–25 s set-up | 15–25 s per job, on the critical path |
| 26 crate-template jobs | ≤ 1 s | about 30 s each | about 13 runner-min per run |
| Runner-host and local-mode jobs | ≤ 1 s, warm target clone | about 30 s plus a 44–61 s cache restore | 1–1.5 min per job |
| Release promotion | about 15 s | 1–2 min | about 1 min per release |

### 5.4 The platform abstraction

The core describes environments generically and never names pods or containers; a backend is the
only code that knows where jobs run (decisions 13, 21).

```rust
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> &Capabilities;
    async fn provision(&self, spec: &EnvSpec) -> Result<Env>;           // from the warm pool when it can
    async fn attach(&self, env: &Env, dirs: &[DirSpec]) -> Result<()>;  // entry kind chosen by capability
    async fn spawn(&self, env: &Env, proc: &ProcSpec) -> Result<Proc>;  // stdout and stderr streamed
    async fn signal(&self, proc: &Proc, sig: Signal) -> Result<()>;
    async fn wait(&self, proc: &Proc) -> Result<Exit>;
    async fn teardown(&self, env: Env) -> Result<AbsenceProof>;
    async fn observe(&self, id: &EnvId) -> Result<Observation>;          // Present | AbsentProven | Unknown
    async fn recover(&self) -> Result<Vec<Recovered>>;                   // after a crash: adopt or kill, never guess
}
pub struct EnvSpec {
    pub cpus: u32, pub memory: Bytes, pub dirs: Vec<DirSpec>,
    pub network: NetPolicy, pub lifetime: Lifetime, pub arch: Arch, pub os: Os,
}
pub struct Capabilities {
    pub kernel: Kernel,             // Shared | Separate
    pub dir_entry: Vec<DirEntry>,   // BindOrReflink | VirtioFs | DiskImage
    pub shared_page_cache: bool, pub snapshot_restore: bool, pub warm_pool: bool,
    pub docker: Docker,             // None | PrivilegedDind | RootlessNested
    pub network_policy: bool, pub cpu_pinning: bool, pub cgroup_kill: bool,
    pub arch: Vec<Arch>, pub os: Os,
}
```

| Capability | Kubernetes (Hetzner cluster) | Docker (one host: Docker Desktop on macOS, or Linux) |
|---|---|---|
| Kernel | shared | shared (a Linux VM's kernel on macOS) |
| Directories enter by | reflink clone on XFS, bind mount | bind mount; copy where the host has no reflink |
| Shared page cache across clones | no (measured) | no |
| Warm pool | pre-created single-use pods | pre-created containers |
| Docker for tests | privileged dind on tainted nodes | the host daemon's sibling containers, or refused |
| Network policy, cgroup kill, CPU pinning | yes, yes, optional | partial, container kill, no |

- **The core branches on capabilities, never on backend names.** A missing capability selects a
  slower but correct path: copy instead of reflink, refuse a Docker-class job instead of running it
  unsafely, kill the container instead of `cgroup.kill`.
- **Lifecycle, crash recovery, ownership proofs and teardown are part of the contract.** `observe`
  is tri-state, and `Unknown` is never treated as absent.
- **Conformance runs per backend**, and a new backend is accepted when it passes the same corpus. A
  microVM backend (Firecracker, Cloud Hypervisor, Kata) or Apple's Containerization framework
  declares `kernel: Separate`, `dir_entry: [VirtioFs]` or `[DiskImage]`, `snapshot_restore`, and
  possibly `RootlessNested` Docker; adding one is a new implementation of this trait and nothing
  else.
- **Result keys include the environment's class**: kernel kind, architecture, OS and CPU count, so a
  result never crosses an isolation difference that could change it.

### 5.5 The job path

1. **Trigger.** The label flow (section 8.2) decides whether a head is verified at all. A labelled
   head's webhook reaches `velnor-control` in 1–2 s; it calls `ensure(tree, plan)` on the site's
   ledger, which fetches the commit into the node mirror, computes the tree to verify with `git
   merge-tree` (the head merged into its current base, or the merge group's own commit), and runs
   the released planner for the obligations and their keys.
2. **Lookup and execute.** Each obligation has a passing receipt (done), an execution in flight
   (join it) or nothing (schedule it). Obligations are plain commands (`cargo`, `cargo clippy`,
   `cargo doc`, `cargo nextest`) run through `velnor-ci`; the agent that placement picks runs each
   in a warm sandbox of its class with a reflink clone of the node's warm snapshot, observes the
   results, and writes a receipt per obligation as it finishes.
3. **Acquire Actions jobs.** For the jobs GitHub creates, the dispatcher holds every JIT identity
   itself, so GitHub's choice of identity does not decide the node: it reads the request's labels,
   event and repository, picks the node (warm state, site, class, the reserved merge path), and only
   then acquires the job and hands it to the configured executor. Merge-group work goes first.
4. **Report.** `velnor-control` writes `Required` when every obligation of the head's plan holds
   (pending ruling Q2; section 8.2).

### 5.6 Native actions

The closed set comes from every `uses:` in `.github/` and in velnor-gen's samples [C]. Velnor Native
refuses a job before its first step when an action, an action SHA, or an action input is outside the
qualified set, naming it and the supported set; `velnor-gen` emits only qualified actions and SHAs,
enforced by a generator test.

| Action | Uses | Native implementation | Node-local state |
|---|---|---|---|
| `actions/checkout` | 61 | The action's inputs and outputs, including `sparse-checkout` (44 uses) and `fetch-depth`; the exact commit and tree verified; a reflink clone of the pre-materialised `main` tree with only the changed files rewritten | The node's bare mirror (section 7.4) |
| `actions/upload-artifact`, `download-artifact` | 77, 60 | Artifacts v4 | Downloads by digest from the storage tiers |
| `actions/cache`, `/restore`, `/save` | 2, 17, 5 | Section 7.2 | L0 and L1 tiers |
| `jdx/mise-action` | 48 | Pinned tools on `PATH`, with mise's environment | The prepopulated, read-only tool store |
| `jdx/mr-boxington-action` | 12 | Until `velnor-gen` drops it: jobs run plain cargo (section 7.3) | The node store |
| `actions/attest-build-provenance`, `actions/attest` | 8, 1 | OIDC token, ephemeral key, Fulcio certificate, Rekor entry, attestation upload | — |
| `actions/create-github-app-token` | 6, in release goldens | A scoped installation token, masked, revoked in a post step | — |
| `asamarts/alint` | 1 | The pinned release, run with the action's inputs | The tool store |
| Composite actions (`./.github/actions/*`, `actions/setup-velnor`) | 134 calls | Composite semantics, including `background: true` steps (105 uses), `wait`, `wait-all` and `cancel` | — |
| `actions/github-script` | 3 | None: JavaScript. `velnor-gen` replaces its uses with `velnor-ci` commands | — |

`hashFiles` is native. The qualification workflow's Docker and node24 test actions become refusal
cases.

### 5.7 The conformance oracle

`velnor-conformance` runs each corpus case through both executors, on each platform backend, and
compares; velnor-new's `qualification.yml` is its seed.

- **Corpus.** At least 240 semantics cases, 65 fault cases and 30 refusal cases beside the replayed
  jobs: velnor-new's full job set under every triggering event, replayed at pinned commits with
  recorded failures, cancellations and re-runs; a semantics corpus (every expression function and
  coercion pair, every context including secrets masking, status functions × job states,
  `continue-on-error`, outputs and `needs`, env files, workflow commands, post steps, background
  steps, cancellation at every step boundary, timeouts); each native action across its inputs,
  including a cache or artifact saved by one executor and restored by the other; refusal cases; and
  the label flow's states.
- **Normalisation.** Ids, times and durations; runner names, versions and paths; tokens; the
  official runner's own set-up and download lines; ANSI codes; the interleaving of concurrent
  output, compared per stream. Contexts are compared as JSON with those fields removed.
- **Pass criteria.** After normalisation, exact equality of step outcomes and conclusions, outputs,
  contexts and env, expression results, annotations and summaries, cache and artifact effects,
  post-step order and results, and cancellation and timeout outcomes. Any other difference fails the
  case unless an owner-signed allow-list entry covers it. The gate is 100% per backend, with zero
  secret leaks. The harness is itself tested: it must kill at least 95% of about 30 executor mutants
  (a swapped post-step order, a dropped masker encoder, a changed shell flag) before its green
  counts.
- **When.** On every change to an executor, a native action or the protocol code, as a blocking
  check; nightly; within hours of each official release (Velnor reports as its runner version the
  newest official release it has passed, so GitHub's 63-day version window becomes a conformance
  deadline); and as sampled shadow runs of real velnor-new jobs. The broker's own
  `registration_deprecates_at` and `runtime_deprecates_at` fields are read and alerted on, and every
  protocol message is parsed strictly: an unknown field is logged and alerted rather than dropped,
  so new server behaviour shows on its first message. Each new official release also gets a protocol
  surface diff, the source diff between two tags restricted to about 40 files (the broker,
  run-service and Results clients, the job dispatcher, the step runner, the execution context,
  command handling), and a canary runs every 10 min in each site through the production path:
  checkout, a cache and an artifact round trip, an OIDC token, an attestation of a scratch subject,
  a step summary, a background step and a cancel, each read back through the API; three failures in
  a row page the owner. Every release binary is also scanned for the banned legacy endpoint paths,
  so a legacy API cannot ship by accident.
- **Per-job-type gate** (decision 16): a job type moves only after its cases pass at 100% on the
  Kubernetes backend, its replayed real-commit pairs show no unexplained difference, and every step
  made to fail is red exactly as on the oracle (section 11).

### 5.8 Isolation and trust

No pull request comes from a fork (decision 8): isolation is designed for trusted authors whose code
may be buggy, not for hostile outsiders. The rule that PR results never become `main`'s trusted
caches or receipts without verification is a correctness rule and holds on every tier.

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | small jobs (lints, plan) | warm sandbox, 1 CPU | GitHub API | read-only tools |
| build | PR, merge-group and `main` jobs and their obligations | warm sandbox: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account token; obligations also without `.git`, a GitHub token or the job's environment | allow-list: crates mirror, site storage | read-only trusted; per-obligation clones; never a shared writable path |
| docker | only the three test binaries that need a daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), as their own obligations | by capability: privileged dind on two tainted nodes today (pending ruling Q19) | as build, plus a registry mirror | the build's test binaries, cloned |
| builder | `main` generations, no GitHub job | unprivileged | crates mirror, storage | writes the trusted scope |

Runner credentials stay in the dispatcher and never enter a sandbox. The required check is bound to
one integration in the ruleset (N12).

### 5.9 Failure handling, releases and rollback

- **Journal discipline:** persist the intent, make the call, record the outcome; unknown is never
  absent. The executor journals the plan id, job id, run-service URL, job token and environment id
  before acting, runs no credential-bearing step before its first renewal succeeds, renews the lease
  every 60 s from `lockedUntil` (converted to a conservative monotonic deadline) with a loop written
  fresh (the upstream overload dereferences a null request when renewal fails,
  `JobDispatcher.cs:748-852`), and after a crash resumes or fails the job with its own token, never
  twice. The ledger persists an execution's intent (key, node, epoch) before a sandbox starts, and
  results land by compare-and-swap on that epoch.
- **Fault injection** before each job type moves: crashes around every remote call, journal write
  and step boundary, a lost lease, duplicate and reordered webhooks, a ledger partition, a lost
  node, the control plane and a whole site.
- **No fallback** (decision 18). A divergence after a move is fixed forward. A bad Velnor release is
  recovered by redeploying the previous immutable image digest, a deploy rather than a code path
  (D15, section 14). The cluster runs only released versions, never the change under test.
- **Control plane:** k3s has one server today (§B.1.5); a whole-cluster dependency waits until
  control-plane and site loss are qualified: three servers across failure domains, and a third-site
  witness for any leader the sites share, such as the ledger's.

### 5.10 What is reused, after review

Nothing is reused by default (decisions 14, 17): each component is reviewed, and kept only when it
is well designed for this architecture.

| Component | Verdict | Why |
|---|---|---|
| velnor-legacy's broker, JIT and run-service client, Results client | Port and refactor | The wire behaviour is right and tested; legacy-protocol paths are stripped and the god files (`executor.rs` 37,028 lines, `runner.rs` 36,759) are split |
| velnor-legacy's expression engine (3,565 lines, from actions/runner v2.337.0) | Port as-is or replace, pending ruling Q4 | It equals what upstream runs today; whether that counts as the banned legacy evaluator is the open question |
| velnor-legacy's checkout, artifacts, attest, App-token and mise adapters | Port and refactor | Behaviour and tests are sound; they move onto the node stores |
| velnor-legacy's `gha_cache.rs` (17,140 lines, v1 and v2 server), Docker lease, JS sidecars | Redesign or drop | Cache v1 is out; the cache layer is new (section 7.2); there is no Node |
| velnor-new's `velnor-runner-core` (lifecycle, `CleanupProof`, `OwnedIds`) | Port and refactor | The tri-state "unknown is never absent" rule is right; its two open slot defects (a short id accepted as covering, a live slot replaced) become tests |
| velnor-new's `velnor-runner-github` (scale-set wire types) | Port and refactor, pending review | Pure types; it carries official-runner lifecycle states |
| velnor-new's `velnor-host` journal (Turso) and launch state machine | Redesign | Bound to the official-runner pair; `Journal::open` drives the test floor. The discipline is kept |
| velnor-new's Docker client and spec audit | Port into the Docker backend, pending review | The audit rules (no host mounts, no outer socket) are valuable |
| The `Runtime` trait proposed for velnor-new's runner | Port its observation model into the platform trait | `Present`/`AbsentProven`/`Unknown` and Pod-UID proofs are right; the official-runner pair methods are dropped |
| `velnor-runner-host`'s official-runner wrapper | Redesign into `velnor-official` only if review fails it | Decision 19 keeps the official runner as an executor; the wrapper is reused only if it passes review |
| `velnor-ci`, `velnor-gen` | Port and refactor | Generators of record; `gate.rs` must read the App check, and velnor-gen hard-codes hosted runners for release (`release.rs:157,172`) |
| The engine's 39-job `ci.yml` | Redesign | Replaced by velnor-gen's workflow, which deletes the engine-generated CI |

## 6. The ledger and reuse

### 6.1 Keys

- **Definition digest:** command, configuration and features, profile, nextest profile and
  overrides, helper version, toolchain, image and tool-store digests, the **executor identity**
  (decision 20), and the environment class (kernel kind, architecture, OS, CPU count).
- **Inputs, per Cargo root:** the Git tree hashes of the root, of the roots it reaches (only
  `ci-helper` reaches outside its own, into five `generator` crates), of the shared files (toolchain
  pin, `mise.*`, `.github/`, `.velnor/`), of every workspace manifest, and of declared extra inputs
  (fixtures, `include_str!` targets, `.velnor/runner.lock` for `velnor-runner-host`'s build script).
  Per package, later: the same over the package's dependency closure, plus the digests of every
  executable the test spawns (`CARGO_BIN_EXE_*`: 102 test files use it 171 times).
- **Execution manifest:** the allowed environment and its values, the path prefix, modes and symlink
  targets of the materialised files. An obligation that reads event, ref or commit values is
  uncomparable (C5).
- **Full tree, observed reads.** Every sandbox holds the full tree. A read by a build script or test
  outside the key's paths, including an `ENOENT`, demotes the receipt to exact-tree scope and opens
  a planner fix.
- **Time and randomness.** No test manifest declares proptest, quickcheck or rand [M]; tests that
  read the wall clock get receipts that expire in 24 h, others in 7 days.
- **The exact tree is the degenerate key**, used whenever a narrower key is not proven.

### 6.2 Receipts, joins and scopes

- **One receipt per obligation, written as it finishes:** key, result, producing request, sandbox
  class and node, expected test inventory, and the agent's signature. **In-flight join:** a request
  whose key is executing waits for it; a cancelled request drops its claim, and the execution
  continues while another request needs it.
- **Scopes.** A PR's execution writes `candidate` receipts, reusable by any request whose obligation
  has the same key: the same PR's later pushes, other PRs, stacked PRs and merge groups. A receipt
  becomes `trusted` only when its tree is on `main` and audit A1 has passed for that commit. A build
  that can mint a reusable receipt reads only the `trusted` cache and its own outputs.

### 6.3 What a request executes

| Situation | What runs |
|---|---|
| Rebase-only push, same tree; a re-run with no code change | nothing: the exact-tree verdict is reported within seconds |
| Push N+1 while N runs | N is cancelled at once (no stall); N+1 joins N's work its keys still need and runs the rest |
| Stacked or concurrent PRs | each obligation runs once per key; others join or reuse |
| Merge group | every obligation listed; the PR's whole verdict is reused only when the group's tree equals the PR's; otherwise per-obligation receipts with equal keys are reused and the rest runs |
| Push to `main` | nothing new: promotion of the group's receipts, release obligations, audit A1 |
| Release tag | promotion of verified bytes; zero compiles (section 8.4) |
| Any obligation in C5 | always runs |

### 6.4 Soundness cases

Every reuse or skip, with its four parts (decision 11): why the key is complete, what fails if it is
not, how that is detected, and the automatic fallback.

| Reuse | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| Exact-tree verdict (rebase-only push, re-run, identical merge-group tree, same-SHA push) | The tree is every input; definition and executor identity equal | A non-tree input changed: clock, network, a tool | C5 obligations always run; audit A1 | Revoke by epoch; full run |
| Per-root receipt | Root tree, reached roots, shared files, all manifests, declared inputs, definition | An undeclared input outside the root | Read audit (incl. `ENOENT`); A1; 2% sampling | Demote to exact tree; planner fix |
| Per-package receipt | Closure trees, all manifests, spawned-executable digests | Feature unification or a spawned binary outside the closure | Shadow at 100% until about 3,000 clean; read audit; A1 | The kind drops to per-root keys |
| Candidate receipt reused across pushes and PRs | Same key, Velnor-observed, signed | A PR sandbox leaked state into a result | Sampled re-execution; trusted only after A1 | Candidate reuse off for that class |
| `velnor verify` before the push | Same sealed Linux sandbox and key as CI | A developer machine's result | Only Velnor-executed results count; Mac runs never do | Runs in CI |
| Warm compile snapshot | Cargo fingerprints plus the time rule: unchanged files keep the snapshot's times, others are newer than every output | A changed file carrying an older time; a stale `OUT_DIR` or proc-macro output | A1 cold rebuild with digest comparison; sampled compile units rebuilt from empty and compared; lineage restarts daily | Retire the generation; build cold |
| `mbx-cache` units for a cold or mismatched node | mbx's per-crate keys | A missed input in mbx's key | A1; the builder's cold rebuilds | Server retired; plain cargo |
| `actions/cache` entries | The user's key, with GitHub's scope rules, enforced by Velnor | A user key that omits an input (as on GitHub) | Conformance against the official runner; content hash on every read | The entry is treated as a miss |
| Artifacts and test binaries by digest | Content address | Corruption | Hash verified on read | Refetch or rebuild |
| Tool and crate stores | Pinned versions and lockfile checksums | A store edited in place | Read-only mounts (`EROFS`); digests | Rebuild the store |
| Mirror checkout | The exact commit and tree ids are checked before the first step; every rewritten blob is hash-verified | A stale or corrupt mirror yields a different tree | Tree identity by construction, a full recompute on a sample | Fetch the commit from GitHub; rebuild the mirror from the site bundle |
| Release promotion | Closure, release profile, target, tool image; the digest is in the receipt and the attestation | Wrong bytes published | Re-hash and `gh attestation verify` at publish; A1 rebuilds cold and compares | The release stops; nothing is rebuilt silently |

### 6.5 Trust root and audit

- **Keys.** One Ed25519 signing key per node and sandbox class, held by a signer daemon outside
  every sandbox and outside the agent's general process; receipts go to an append-only log per site;
  the ledger verifies signature, class and log entry before it counts a receipt.
- **Revocation.** Every verdict carries its policy epoch. Revoking a signer or a reuse class raises
  the epoch: older verdicts are re-checked, queue entries pause, and landed commits that relied on
  them are listed. The first reused pass whose cold run fails revokes its class at once and blocks
  every open head, queue entry and release that relied on it, before any statistics: sampling finds
  defects, but never licenses a suspected false green.
- **Audit A1.** Every landed commit is rebuilt cold at batch priority and its changed obligations
  re-run, comparing results and test-binary digests with the warm path, within 2 h of landing at P90
  [T]; a stale generation can therefore serve only the merges inside that window. Then 2% sampled
  re-execution once a class has about 3,000 clean comparisons.
- **Audit A2.** Nightly, and with the clock shifted 8 and 30 days ahead through a shim, so a test
  that fails only with time (an expiring fixture, a date boundary) is found before the date arrives.
- **Degraded mode.** With the ledger unreachable nothing is satisfied by lookup: obligations run
  fresh. Slow, never wrong.

## 7. Storage and caches

### 7.1 One storage abstraction, configurable tiers

```rust
pub trait BlobStore: Send + Sync {
    fn capabilities(&self) -> &StorageCaps; // reflink, server_side_copy, multipart, conditional_writes, replication
    async fn get(&self, key: &Key, range: Option<ByteRange>) -> Result<Body>;
    async fn put_if_absent(&self, key: &Key, body: Body) -> Result<PutOutcome>; // keys are immutable
    async fn copy(&self, from: &Key, to: &Key) -> Result<()>;                   // server-side when the backend can
    async fn list(&self, prefix: &Prefix) -> Result<Vec<Key>>;
    async fn delete(&self, key: &Key) -> Result<()>;
}
```

- **Backends:** the local filesystem (node NVMe, or a directory on a Mac); RustFS single-node;
  RustFS multi-node, an erasure-coded cluster per site over the workers' disks, optionally
  replicated between sites; any S3-compatible store (RustFS, MinIO, AWS S3, R2) through the same S3
  backend.
- **Tiers** are composed in Velnor's configuration, for example `local`, `local → site` or `local →
  site → cross-site`, each tier naming a backend. **Each data class is routed separately:**
  `actions/cache`, artifacts v4, promoted release artifacts, the mbx-cache store, and warm-snapshot
  shipping. One RustFS cluster per site can back all of them, so there are fewer systems to run.
- **Defaults per platform:** Docker or local macOS: local only, no extra services. Kubernetes: node
  NVMe plus a RustFS cluster per site.
- **Invariants for every configuration:** the user-facing `actions/cache` semantics never change;
  correctness is enforced by Velnor's cache layer, not by the backend (scoping, PR writes never
  reaching `main` scope, immutable keys, content verification on read); a misconfiguration can only
  make things slower, never wrong. Backends declare capabilities and the layer picks the fastest
  correct path, as with platforms.

### 7.2 Native `actions/cache`

Users write plain `actions/cache` (`key`, `restore-keys`, `path`, `enableCrossOsArchive`, the
`cache-hit` and `cache-matched-key` outputs) with GitHub's exact semantics, branch scoping included:
a run reads its own ref, its base and the default branch, writes its own ref, and keys are
immutable. There is no new syntax, and conformance proves it against the official runner.

- **Tiers:** L0, node NVMe, unpacked once per node and then reflink-cloned per job; L1, the site's
  RustFS cluster, so a node miss is a LAN fetch; L2, RustFS site replication DE↔FI in the
  background. GitHub's cache service is not on the hot path.
- **Prefetch:** the dispatcher evaluates the job's cache keys before the job starts (for example
  `hashFiles('Cargo.lock')`) and warms L0 before the first step.
- **Content-addressed chunks** make saves deduplicated and incremental.
- **Open, to verify in the runner's source:** whether GitHub Official can be pointed at a Velnor
  endpoint compatible with cache service v2, by handing `Runner.Worker` a job message with rewritten
  endpoints, so both executors get the fast cache. Until it is verified, GitHub Official uses
  GitHub's own cache service: correct, slower.

### 7.3 Compile reuse

**Measured on two workers** [M, `experiments/mbx-cache-cluster.md`; the runner workspace, 357
crates, `/w/velnor-new`]: plain cargo cold 92 s; over a reflink clone of the warm target 0.25 s and
2.5 MB for the clone and 0.31 s with 0 crates for the build; today's full copy onto the ext4
`emptyDir` 8.6 s and 10.75 GiB per job; mbx on a second node from a warm `mbx-cache` 39.9 s (496
hits, 0 misses, 8.7 GiB); a lockfile change 67.4 s with plain cargo, 88.4 s with mbx on the same
node and 43.4 s with mbx on another node once one has built it; clones hold their own page cache
(14.29 GB each when fully read). On the owner's Mac [M, `experiments/mbx-cache-mac.md`] both engines
rebuilt the same 6 workspace crates at every new path, and without clones mbx used 41% less disk.

- **The hot path is plain cargo over a reflink clone of the node's warm snapshot**, at a fixed
  checkout path, with the time rule of `velnor-ci inputs restore` (section 6.4). Job work
  directories live on the XFS volume (local-path volumes; `emptyDir` is ext4 and cannot clone).
- **mbx with an `mbx-cache` per site is the cross-node layer**, not the hot path: it fills a cold
  node in about 40 s instead of 92 s and serves a lockfile change to a node without a matching
  snapshot in about 43 s, through per-crate reuse. Its store moves to the site's RustFS in S3 mode.
  It is kept while at least 10% of build jobs would otherwise start cold or mismatched and are
  served from it, or while it saves at least 20 runner-minutes a day; otherwise it is retired
  (owner-approved, N54). The honest alternative: incremental warmers make nodes rarely cold, and
  copying a snapshot moves about the same 8.7 GiB.
- **Race a fill against a build.** When a node lacks the generation a job needs, the agent starts
  both a local delta build and a fill from `mbx-cache` or a peer's snapshot, picks the predicted
  winner's result and cancels the other: a duplicate kept because it is faster (a fill took 39.9 s
  against a 92 s cold build, and 43.4 s against 67.4 s on a lockfile change [M]), at batch cost.
- **Reflink against overlayfs, a hard-linked immutable `deps/` base, or other shared inodes** is
  pending ruling Q16: clones do not share page cache, so each build sandbox's cache counts inside
  its own memory limit; the head-to-head decides.
- **Incremental compilation** is off in required builds until a worker measurement and a
  non-incremental audit show equal results (pending ruling Q17; cargo incremental edits took 1.5–2.8
  s against 9.2–12.2 s non-incremental [M, `experiments/blaze.md`]).
- **blaze is rejected** on evidence: it built none of velnor-new's five workspaces, ignores the
  1.99.1 pin, and missed a clippy `deny` lint; cold builds were 12× and edits 5–15× slower than
  cargo incremental [M, `experiments/blaze.md`].

### 7.4 Node-local mirrors

Each node keeps a bare mirror of every configured repository (decision 24), covering PR head refs,
merge-group refs and tags. Push webhooks fetch only the new objects, and a periodic catch-up repairs
anything a webhook missed. New or wiped nodes seed from a git bundle in the site store, not from
GitHub. Mirrors are read-only to jobs.

**Native checkout from the mirror,** with a target well under 1 s:

1. Check that the exact requested commit is in the mirror, fetching only the missing objects in one
   round trip if not; the mirror is never assumed fresh.
2. Reflink-clone the node's pre-materialised tree of its latest `main`.
3. Rewrite only the files whose blob ids differ from the requested tree, including deletions, modes
   and symlinks, each written from a hash-verified blob.
4. Verify before the first step that the commit and its tree hash are exactly the ones requested:
   the base tree's hash was recorded when it was materialised, and every rewritten blob is verified,
   so the result matches by construction; a full recompute of the tree hash runs on a sample and
   whenever the base's record is missing.
5. Set times by the warm-snapshot rule (section 6.4): unchanged files keep the base tree's recorded
   times, which are the warm snapshot's, and every rewritten file is newer than every output, so
   cargo's fingerprints stay valid. A job that needs `.git` gets one whose objects come from the
   read-only mirror through alternates; obligations get no `.git` at all (C5).

**The same pattern for every upstream,** so each new version crosses the internet roughly once per
site: the crates.io sparse index and crates (the cluster's existing crates-mirror DaemonSet, unused
today, §B.4.2), Rust toolchains, mise tools, and container images through a registry mirror per
site. Which repositories and upstreams, the refresh policy, the storage tier, and on or off per
platform are configuration; on a developer's Mac the Docker backend may reuse the developer's own
clone, read-only.

**What it saves** [E: about 10,000 job executions a day today, from about 50 merges a day (159 green
merge groups over three days [M]) × about 6.5 runs per merge (4.77 PR runs, one merge group, about
0.75 push runs [M]) × about 30 jobs per run [M: 31–39]]:

| Upstream | Today, per job [M] | With node mirrors | Saved per job | Saved per day at today's volume [E] |
|---|---|---|---|---|
| Repository checkout | 6 s median, 8 s p90; a 110 MiB working tree fetched from GitHub | ≤ 1 s from the mirror | about 5 s | about 14 runner-hours; about 10,000 checkouts from GitHub become one incremental fetch per node per push (about 250 pushes a day) |
| Crates (index and crates) | 3.5–6.1 s; 435–522 crate downloads; the sources cache hit 0 of 48 | 0 s while the lockfile is unchanged; a new version once per site | about 4.5 s | about 9 runner-hours over about 7,500 Rust jobs; about 3.5 million crate downloads become a few dozen new versions |
| Rust toolchain and mise tools | 14–15 s; the toolchain restored 0 of 58 times, so it installs per job | 0 s: a read-only store, refreshed once per site per version | about 14 s | about 39 runner-hours; every per-job install becomes one download per site per version |
| Container images (job images; images pulled inside the Docker tests' fresh daemons) | pulled per node, and per daemon inside dind | a registry mirror per site | not yet measured | measured when the Docker-class jobs move |

Together about 62 runner-hours a day at today's volume, roughly a fifth of today's 250–290 [E:
285–347 runner-min per merged PR × about 50 merges]. Under v3 far fewer jobs run, so the absolute
saving shrinks, but each remaining job still saves about 20 s.

### 7.5 Budgets

The last three `main` generations per root, then least-recently-used; 40% of a node's data volume,
alert at 70%. FI workers have one 3.5 TB data disk each, DE workers 32–50 TB (§B.1.4). The runner
workspace's target is 10.6 GiB [M]. Every store has garbage collection and a budget; `mbx-cache`
starts at 300 GiB per site.

## 8. Label flow, workflow, merge queue and release

### 8.1 Shape

velnor-gen writes one job per run for the work that must be an Actions job; verification itself is
the obligation graph run through `ensure`. Inside a root's request, test binaries build once per
configuration, and format, lint, documentation, "check alone" and tests run as parallel lanes, with
one target directory per check kind (cargo locks a target directory exclusively,
`docs/hard-decisions.md:56-62`), clippy and doc metadata pre-built, and one jobserver per sandbox.
`cargo-deny` reads the RustSec database pinned by commit.

### 8.2 The label flow and the required check

**Pending ruling Q1, Q2 and Q8; the safe default is in force: an App-written required check.** A job
skipped by `if:` reports success and does not block a required check [docs: using conditions to
control job execution], so no Actions job carries the gate.

| Event | Label | Velnor's action | `Required` on that head |
|---|---|---|---|
| opened, reopened, ready for review, push | absent | Nothing runs; creates `Required` as `queued`, titled "Not requested: add `velnor:verify`" | pending, which blocks |
| labeled `velnor:verify` | — | Plans the head merged with its base and runs the missing obligations | in progress, then success or failure |
| push | present | The new head gets a new check; old-head work the new head does not need is cancelled at once; shared work is joined | the old SHA's check: cancelled |
| unlabeled | — | Cancels in-flight work; nothing further runs; a completed verdict stays true for its head | in progress → queued, "Not requested" |
| edited | any | Nothing; a base change re-plans | unchanged |
| `check_run` re-requested | present | Re-runs only the failed obligations, charged to the flake budget | in progress |
| `merge_group` checks requested / destroyed | — | Verifies the group's tree / cancels it | on the group's head SHA |

- **The ruleset** requires `Required` bound to the App's `integration_id` on PRs and in the merge
  queue, with no bypass actors: break-glass means the owner edits the ruleset, which is
  audit-logged. Any push makes the head's check pending again, so only a green latest commit can
  merge or enter the queue, and only the App can satisfy a queued check.
- **Label events made with `GITHUB_TOKEN`** may not trigger workflows; whether App webhooks still
  arrive is pending ruling Q3. The safe default: agents label with the App's token or the UI, and a
  reconciler lists open PRs every 30 s, so a missed delivery delays and never verifies the wrong
  head.
- **Two PRs at the same head SHA** share one check (pending ruling Q9). The safe default keeps it
  pending while any open PR at that SHA lacks the label.
- The release gate (`ci-helper/crates/velnor-ci/src/gate.rs:15,127-159,277-288`) and
  `product-release.yml:86` read an Actions `Required` job today; they migrate together to the App
  check in the same change (decision 18).

### 8.3 Merge queue

GitHub's queue stays (decision 7); Velnor runs no merge train. ALLGREEN is the safe default (pending
ruling Q10), so every landed commit has its own verdict. Depth starts at 2 and rises only when a
replay with real arrivals and every failure counted shows more merges per hour. Speculation verifies
the queue tip's ordered prefixes and single-ejection bypass trees at batch priority through
`ensure`; a wrong guess wastes only batch CPU. Whether merge-group trees equal PR-tested trees often
enough to matter is pending ruling Q11: 7 of 45 were equal today [M], and a read-only shadow over 50
merges decides, with a kill below 70%. Agents enable auto-merge and stop catch-up merges.

### 8.4 Release: build once, promote by digest

A release never rebuilds (decision 10). Which profile tests use was measured [M, `generator` and
`ci-helper`, cargo 1.99.1, CPU-seconds]: a release-profile test build shares only 15 of 90 crates
with the release binary, because the binary uses `panic = "abort"` and tests need unwinding.

| Option | CPU per cold build, `generator` / `ci-helper` | What is tested |
|---|---|---|
| Test the release build | 320.8 / 365.7 CPU-s | optimised code without debug assertions; still not the shipped panic strategy or musl |
| Debug tests, release bytes built once | 191.9 / 282.8 CPU-s | today's coverage, and the shipped bytes get their own checks |

Where release bytes are built is pending ruling Q6; the safe default: tests stay in the debug
profile, and release-profile bytes are built once per closure key in the merge group, qualified once
on the exact bytes (`--version`, `check-tag`, the black-box suites that today run only after the
merge), packed deterministically with `SHA256SUMS`, and attested once. A tag runs one job: gate,
fetch by digest, re-hash, `gh attestation verify`, publish (idempotent, with a read-back of what was
published, and crates.io receiving the exact packaged `.crate` bytes): about 15 s instead of 190–434
s for the tools and 4,105–6,238 s for a product release [M]. `option_env!("VELNOR_COMMIT")` makes
every commit a new compile input; with the commit patched after link into a fixed-size section,
release bytes recompile only when their closure changes, about half of all merges (also pending Q6).
The macOS legs are paused (decision 9); `aarch64-unknown-linux-musl` is cross-compiled and smoke-run
under user-mode qemu.

### 8.5 Tests and the floor

- **The unit of cost is `Journal::open`** (93 call sites): a pre-migrated template opens in ≤ 1 s,
  and no test exceeds 10 CPU-seconds alone outside a shrinking list. The 40-, 12-, 10-, 6- and
  4-case loops become one test per case, and the serial `test-threads=1` setting goes once tests use
  private temp directories and ports; expensive valid prefixes are built once and cloned per case.
- **The floor behind the loops** [M, 28 job logs]: the longest test is still 132 s in actions-cli,
  125 s in runner-host and 138 s in local-mode; one receipt-integrity test took 924.8 s.
- **The compile floor:** `velnor-runner-host` (156,822 lines) splits into five or six crates, with
  `velnor-runner-local` extracted, so both configurations run without compiling it twice. The 356
  separate integration-test binaries (177 in the workflow renderer, 109 in the orchestrator, 44 in
  actions-cli, 26 in runner-host) become one `tests/it` binary per package, saving about 340 link
  passes per cold build.
- **A test that spawns about 45,000 `git` processes** (`non_inputs_differential.rs:198-264`, 6,425
  globs × 7 subprocesses, a 679 s suite) matches globs in process with `gix` against one prebuilt
  fixture repository, keeping a 32-case differential against real `git` [E: about 135 s → 2 s].
- **Dependency `opt-level`** cut the slowest test class's CPU by 66% with assertions off in
  dependencies; with them on it is unmeasured (pending ruling Q7), so assertions stay on until a
  four-way matrix decides.
- **Fan-out and hedging.** A suite's cases spread longest-first across the site's free threads when
  its summed time over one sandbox's threads exceeds its longest test plus the cost of moving
  binaries by digest. A shard still running past its P95 is raced by a restart on another node, and
  the first result wins: a duplicate kept because it is faster, at batch cost.

## 9. The cluster

- **Role.** k3s is the Kubernetes backend's fleet layer: it deploys the dispatchers, node agents,
  site services, RustFS and mirrors, accounts for every pod and enforces quotas, priorities and
  network policy. Velnor places work; kube-scheduler places pods.
- **Sizes.** Idle and running sandboxes share one memory budget per worker: 125.5 GiB less 6 GiB for
  the system, 2 GiB of eviction margin and 8 GiB for gate sandboxes leaves 109.5 GiB. Build
  sandboxes request 16 CPU with no CPU limit and 24 GiB (the heaviest job peaked at 15.7–16.8 GiB,
  §B.2.2), so four fit per worker, idle ones included. Nextest's threads equal the sandbox class's
  fixed CPU count, which is part of the key, because a pod without a CPU limit sees all 96. Only the
  compile jobserver's width follows the node's free CPUs at sandbox start, from 16 up to 64, so a
  lone PR on an idle node builds faster; compile parallelism changes no output.
- **Page cache.** Clones do not share page cache, so each sandbox's cache counts inside its 24 GiB
  limit: about 7 GiB above the heaviest job's peak, against 14.3 GB for a fully read target. The
  agent records memory pressure per sandbox.
- **Node settings.** Reservations (3 CPU, 6 GiB), a 2 GiB `memory.available` eviction threshold,
  `podPidsLimit` 16384, site topology labels; job work directories on local-path XFS volumes with
  project quotas.
- **Reserved merge path.** Each site keeps one 16-CPU build slot and 2 gate CPUs for merge and gate
  work, about 7.7% of allocatable CPU: at 95% utilisation waiting grows to about 19 times the
  service time. Priorities: merge > gate > PR = other organisations' CI > batch (builder,
  speculation, audits); only batch is preempted. Batch work runs in `cpu.idle` cgroups and is frozen
  with `cgroup.freeze` under memory pressure instead of being killed.
- **Sites.** Each site has its own control, dispatcher, ledger and RustFS cluster; a request stays
  in one site; DE to FI is 35 ms over WireGuard.
- **Other organisations.** ChainArgos and jackin keep their own ARC scale sets, which are not
  velnor-new's path, with guaranteed quotas and separate namespaces and stores, until they choose
  Velnor.

## 10. Servers and cluster tuning

| Rank | Change | Expected gain | Evidence | Rollback |
|---|---|---|---|---|
| 1 | Gate jobs on the cluster in a warm gate class | −5.8 to −57 min per run | [M] §A T5 | Labels back |
| 2 | Honest requests, reservations, eviction threshold, PID limit, per-org quotas | Removes the 1.2–2.4× stretch under load | [M] runner-host 3.7 → 8.8 min (§A T6) | Revert values |
| 3 | Remove `build.jobs=2` and `--jobs 2` | `maintenance-helpers` 8.5 → about 5 min | [M] `mise.toml:90-193` | Revert lines |
| 4 | Work directories on XFS; warm snapshots | 0.25 s and 2.5 MB instead of 8.6 s and 10.75 GiB per job | [M] cluster measurement | — |
| 5 | Docker only in the docker class | 3–4 s per job; 57 fewer probe failures an hour | [M] §B.6 | Label back |
| 6 | Debuginfo: line tables for workspace crates, none for dependencies | −10 to −24% cache; −20 to −27% CPU on a runner-host rebuild | [M] I108 | Profile setting |
| 7–11 | Governor, same-site 10 Gbit/s, CPU manager, THP, mold | measure first | unmeasured | Revert |

SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 11. Migration and deletion

A job type moves to the Velnor executor as soon as it passes conformance for that type (decision
16); its old route is deleted in the same change, and nothing moves back (decision 18). Until then
it stays on today's system, which gets no new investment. The order relieves the critical path
first.

| Step | Moves | Deleted in the same change |
|---|---|---|
| 0 | Configuration only: gates on the cluster, auto-merge, no catch-up merges, no `edited`, the interim label gate, the cancel stall removed, macOS legs paused, per-push image and qualification triggers narrowed, `-j2` caps | `edited`; the fork twin; the catch-up rule; `-j2` caps; Darwin release legs; per-push triggers |
| 1 | The gate chain: `Required`, `Required verdict`, Plan, Ledger CLI, the lints | Their ARC routes and hosted twins; Plan's tool-seed steps; the tool composites they used |
| 2 | The Rust crate template (26 jobs), `check-maintenance-helpers`, `check-generator-samples` | 26 routes; six tool composites |
| 3 | `velnor-runner-host`, its `local-mode`, `velnor-local` (the Docker-class binaries) | Their routes; the dind sidecar for velnor-new |
| 4 | The App check becomes required; state seal; image candidate; source qualification; release workflows (Linux) | The Actions `Required` jobs; the interim gate; hosted release labels; the state seal and `cache-prune.yml`; macOS release workflows |
| Last | — | ARC's velnor-new scale set and the ARC path that ran production jobs on the official runner; `velnor-runner-host`'s official-runner wrapper unless it passed review for `velnor-official`; the engine-generated CI |

The official runner stays, as GitHub Official and as the reference executor (decisions 19, 20).

## 12. The first ten work items

1. Phase-0 configuration (step 0), landed with its deletions.
2. Review verdicts for `velnor-host`, the proposed `Runtime` trait and the legacy executor modules
   (section 5.10).
3. The platform trait, capabilities and the Docker backend.
4. The Kubernetes backend with warm pools and proofs.
5. The executor interface; Velnor Native's protocol client (broker, JIT, run service, Results).
6. GitHub Official behind the same interface, and the conformance harness with its mutants.
7. Native actions, with `actions/cache` on the storage layer.
8. `velnor-control`: the label flow, the App check and the reconciler (pending ruling Q2).
9. Node-local mirrors, warm node snapshots, the storage tiers and the builder.
10. The ledger with per-root keys, candidate and trusted scopes, and audit A1.

## 13. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A reuse gives a false green | Shadow re-execution; 2% sampling; audit A1; read audit | Revoke by epoch; work runs again |
| Velnor Native departs from GitHub's semantics | The harness on every change, nightly and per official release; sampled shadow runs | Block the release; fix forward |
| GitHub changes the protocol | The harness within hours of each release; a canary every 10 min | Fix inside the 63-day version window |
| The skipped-job trap or a mis-set ruleset lets an unverified head merge | A canary PR that must never be mergeable | App-written check (pending ruling Q2); no bypass actors |
| A move leaves no way back and diverges | Sampled shadow runs; per-type failure rates | Fix forward on a 2-hour target; redeploy the previous image digest |
| A backend behaves differently (exec, signals, mounts) | Per-backend conformance | Capability fallbacks, never backend-specific branches in the core |
| A store is misconfigured | Content verification on read | Slower, never wrong (section 7.1) |
| Stale compile state | Audit A1 digest comparison | Retire the generation; build cold |
| The cluster is saturated | Per-class CPU | Batch work preempted first; reserved merge path |

## 14. Owner questions

| # | Question | Example | Recommendation |
|---|---|---|---|
| 1 | Does a receipt with the same input key count as "ran and passed" (`docs/goals.md`, "Green means verified")? | A generator-only PR's merge group lists every root; four roots have `main`'s keys, so receipts satisfy them and only the generator root runs | Yes, under C1–C7 and audit A1 |
| 2 | How is a bad Velnor release recovered? | An executor release that fails every job also blocks the PR that fixes it | Redeploy the previous immutable image digest; if that is not enough, an admin merge of the fix with lab conformance evidence |
| 3 | Should ChainArgos and jackin move to Velnor, and when? | Their ARC scale sets share the nodes with velnor-new | After velnor-new's last move has soaked |

## 15. What more hardware would buy

| Purchase | Buys | When |
|---|---|---|
| Two more k3s servers across failure domains | A control plane that survives one node or site | Before a whole-cluster dependency lands |
| One high-clock node | 1.3–1.5× on the serial path [E] | Only after the test-floor fixes (set-up and transfer cost first), then rented for a measured trial |
| 10 Gbit/s between same-site workers | Faster cross-node fills and test fan-out | When a measurement shows the need |
| More EPYC workers | +96 threads each | CPU above 70% at peak after dedupe, or eligible merge wait p90 above 10 s for two weeks |

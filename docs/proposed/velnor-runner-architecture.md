# Velnor runner architecture

**Status:** Proposal (v3); it replaces nothing yet. It follows the owner's binding decisions 1–24
(section 5.1). The panel's contested facts are ruled on evidence (decision log); where a ruling
names a lab test, the design runs on that ruling's safe default until the lab reports (section
11.3); questions only the owner can answer are in section 14. **Scope:** how Velnor verifies, merges
and releases velnor-new's changes on the project's own machines, so that CI stops being the slowest
part of delivering a change, with the generator (`velnor-gen`) and helper (`velnor-ci`) where the
runner needs something from them.

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
   for every landed commit, not only for a group's tip: under HEADGREEN three commits whose own
   `Required` failed already sit on `main` [M].
2. **C2. Keys capture every input, by construction.** A key covers the sources, every workspace
   manifest, the toolchain, dependency versions, environment, flags, features, the binaries a test
   executes (`CARGO_BIN_EXE_*`), data files, and the executor's and environment's identity (section
   6.1). Every obligation runs on the full tree: cargo refuses a workspace with a missing member,
   and a sandbox trimmed until it loads turns a real failure green, because a test that lists or
   reads paths outside its closure sees fewer files and passes [M, reproduced]. A closure narrows
   only the key, and only where audited reads prove it (section 6.1); observed reads may propose
   declarations but never prove a key complete.
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

Every obligation whose key is new executes **twice, in parallel, on two nodes with different test
orders**, before the PR's status goes green; any failure blocks, with no retry. The merge group runs
what the PR's receipts do not cover, and the cold audit of every landed commit is the third
execution, as today's push run is (section 6.5). A regression failing with probability p is caught
before merge with 1−(1−p)², as today; if lab C2 finds the twins correlated, the merge group re-runs
the PR's changed obligations even on an identical tree.

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes; it costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget and measure:** at most 3% false failures per required run once verification leaves
  today's full runs, and 1% at the committed targets, each claimed only after about 300 rerun pairs
  from a sampler that re-runs green and red results both ways under load (zero disagreements in 299
  bound the rate just under 1% at 95%). "Not requested" never counts. Today's 8.1% [M] comes from
  full runs; running only changed closures removes most of it.
- **Report and act:** retries stay at 0, so a flaky test fails the run and is counted. A test with
  two false failures in seven days gets an owner-labelled issue; it is fixed, or quarantined only
  when a blocking check covers the same property or under an expiring owner exception.
- **Known causes first:** CPU starvation of tight timeouts, such as the 2 s kill in
  `ci-helper/.config/nextest.toml`'s default profile under pods that request 2 CPU and burst to 84
  cores (§B.2.2); the `event.json` rewrite race between steps (I023).

### 1.4 Targets

From the delivery plan (section 11.2), measured on the same clocks:

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

The design removes work rather than speeding it up: a push starts nothing until the label asks; each
unit of work runs once per unique input and is reused by content address; a release promotes the
bytes `main` verified; every node keeps `main` warm and mirrors every upstream, and Velnor executes
jobs natively next to that state. The gain is locality and removed round trips, not the language:
starting Node costs 87–147 ms per action [M, `fable-5.2`].

## 3. The latency budget

One typical PR (the 47% class that touches only `generator/` or `ci-helper/`), labelled, from push
to merged. Today's figures are measured; v3's are committed targets, with the estimate in brackets.

| Stage | Today [M] | v3 [T] (estimate) | Bottleneck removed |
|---|---|---|---|
| 1. Event, webhook, job offer | push → run 1–2 s; run → first job 2 s p50, 226 s p90; hosted Plan queue 5.5 min median | webhook 1–2 s; the reconciler within 10 s | Verification starts from the label or labelled push; an Actions job, queued about 5 s and running about 8 s after the event [M], is not on the verdict path |
| 2. Acquire | 12 s p50, 18 s p90 from job created to started (scale set) | ≤ 0.3 s | Pre-registered JIT identities; placement before acquisition |
| 3. Sandbox start | ARC pod about 6 s (436 starts) plus a dind probe | ≤ 0.3 s | Warm pool; reflink clone 0.25 s [M] |
| 4. Checkout | 6 s per job × 31–39 jobs; Plan 12 s | ≤ 1 s | Node mirror; a reflink clone of the `main` tree plus the changed files (section 7.4) |
| 5. Toolchain and tools | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | Read-only verified tool store; native actions |
| 6. Dependency fetch | 3.5–6.1 s per job; sources cache 0 of 48 | 0 s (≈ 3 s on a lockfile change) | Site crates mirror and node crate store |
| 7. Compile | 196–497 crates per job; 81 s runner-host, 14 s velnor-ci; 8.5 min critical path | ≤ 25 s (15 s) | Warm snapshot: only the change's closure compiles; 0 crates and 0.31 s for an unchanged tree [M] |
| 8. Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s; floor 125–138 s | ≤ 60 s (10 s) | Only changed closures run, twice in parallel; the test floor programme |
| 9. Result reporting | 1.48 GB archive upload 36–43 s; `Required verdict` 49 s | ≤ 1 s | Results streaming; binaries by digest in the site store |
| 10. Required check | hosted verdict queue 1.6 / 24.3 min, then `Required` 1.9 / 21.4 min | ≤ 1 s after the last receipt | One App-written status, `Velnor / Verified` |
| 11. Merge-queue entry and run | green → enqueue 7.5 min (0 of 40 auto-merge); enqueue → group 19 s; group 19.9 / 84.1 min | ≤ 30 s (≈ 25 s) | Auto-merge; an exact-tree lookup for about 75% of groups with auto-enqueue (16% today), else per obligation |
| 12. Merge | 26 s | 26 s | GitHub's queue; irreducible while GitHub lands merges (decision 7) |
| **Verdict / push-to-merge** | **26.7 / 92.3 min; 85 / 457 min** | **≤ 90 s / ≤ 4 min; ≤ 3 / ≤ 8 min (≈ 35 s; ≈ 85 s)** | |

The floor while GitHub owns the merge is about 50 s: delivery 1–2 s, the merge queue's 19 s and 26
s, a few API round trips, plus the longest changed test and one compile of the change. With an
Actions job as the required check it would be about 58 s, because GitHub creates a job about 5 s and
runs it about 8 s after the event [M]; with the App status, about 47 s.

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
| 15 | Full CI on the merge group | merge group | when the tree equals the PR's | 64.6 runner-min; equal trees 16% today | R exact tree (about 75% with auto-enqueue against the queue tip), else per obligation | Receipts; speculation |
| 16 | Full CI on the push to `main` | push | yes (same SHA in 120 of 159) | 62.0 runner-min; 8.1% false failures | E | Lookup and promotion; audit A1 |
| 17 | State seal producers and cache prune | `main` push | yes | 683 s per push; 3.49 GB of GitHub cache, 0 of 3,636 restores | E | Snapshots and storage tiers |
| 18 | Source qualification on every PR event | PR | yes | 1,007 s per event, about 80 runner-min per merged PR | R once per key, in the queue | Obligations |
| 19 | Job image candidate on every push | every push | yes unless images changed | 200 s; about 20 min per PR | R once per image-input key | Keyed action |
| 20 | Source preview build, tag rebuilds, product release | `main` push, tag | yes | preview 410–534 s per push; tools 190–434 s; product 4,105–6,238 s | R promote by digest | Release promotion (section 8.4) |
| 21 | macOS and Darwin legs | `main` push, release | — | 1,063–1,406 s per push | E (paused, decision 9) | — |
| 22 | `VELNOR_COMMIT` forces both tools to rebuild per commit | every merge | no, by construction | two tool builds per merge | E once the stamp is proven | The group SHA in the key today; stamping after link is a lab item |
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
| 9 | macOS release builds paused; every velnor-new job runs on Velnor | 8.4, 11.1 |
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
  hands it, JavaScript actions on Node) in the same sandboxes, with the same platforms, warm pools,
  scheduling, label flow, required status, logs, ledger and caches. Velnor's dispatcher drives both;
  ARC drives neither.
- **Choice and identity.** `.velnor/` names the executor per job type or globally; Velnor never
  switches on failure. The identity is in every key, so switching costs one re-verification and can
  never produce a false green.
- **Reference role** (decision 19): the conformance gate, sampled shadow runs and on-demand "run
  both ways and diff"; reference runs write nothing anyone can consume.
- **APIs** (decision 3), as the latest release implements them, including its broker-session
  ownership on deletion and its connectivity probe: scale-set sessions, broker long-poll, JIT
  configuration, run-service `acquirejob`, `renewjob`, `completejob`, Results twirp for logs,
  timeline and summaries, cache service v2, artifacts v4, OIDC. Out: the AES-encrypted legacy
  session, legacy pipelines and distributed task, the legacy expression evaluator, cache v1 and
  artifacts v3. **Expressions:** Velnor matches the evaluator whose results the latest official
  runner returns, verified by conformance. At v2.338.2 that is the legacy `Expressions2` evaluator
  on every path, even when the new one runs in compare mode, so Velnor ships that behaviour
  (velnor-legacy's port) and ports the new evaluator (`src/Sdk/Expressions`,
  `src/Sdk/WorkflowParser`) when GitHub switches. It conflicts with decision 3's "nothing legacy"
  and is owner question D4; correctness comes first.

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

The core branches on capabilities, never on backend names; a missing capability selects a slower but
correct path (copy instead of reflink, refusing a Docker-class job, killing the container instead of
`cgroup.kill`). Lifecycle, crash recovery, tri-state ownership proofs (`Unknown` is never absent)
and teardown are part of the contract, conformance runs per backend, and result keys include the
environment's class (kernel kind, architecture, OS, CPU count). A microVM backend (Firecracker,
Cloud Hypervisor, Kata) or Apple's Containerization framework declares `kernel: Separate`,
`dir_entry: [VirtioFs]` or `[DiskImage]` and `snapshot_restore`: a new implementation of this trait
and nothing else.

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
4. **Report.** `velnor-control` writes the commit status `Velnor / Verified` when every obligation
   of the head's plan holds (section 8.2).

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
- **When.** Blocking on every change to an executor, a native action or the protocol code; nightly;
  within 14 days of each official release, against its tag (v2.338.2, `d1eb12d4`, today; the panel's
  source clone is unreleased `main`); and as sampled shadow runs of real jobs.
- **Drift.** Deadlines come per version from the deprecations endpoint (2.337.0 stops on 2026-12-25,
  2.338.0 on 2027-01-04): an alarm fires 21 days before the version in use reaches
  `runtime_deprecates_at`, a critical update pages at once, and Velnor reports the newest version it
  has certified. Unknown protocol fields are logged and alerted, never dropped; each release gets a
  protocol surface diff of about 40 files; release binaries are scanned for banned legacy endpoints.
- **Evaluators.** The oracle runs with `ACTIONS_RUNNER_COMPARE_WORKFLOW_PARSER=true`: a job on which
  GitHub's two evaluators differ is refused until understood, and a fuzzer compares expressions
  against both.
- **Canaries.** A production-path canary runs every 10 min per site (checkout, cache and artifact
  round trips, OIDC, an attestation, a summary, a background step, a cancel). Permanent canary PRs
  that must stay unmergeable (no status; the context from another source; a required Actions job
  with `if: false`) are probed every 10 min: any `mergeable_state` but `blocked` pages the owner and
  stops the App writing `success`.
- **Per-job-type gate** (decision 16): a job type moves only after its cases pass at 100% on the
  Kubernetes backend, its replayed real-commit pairs show no unexplained difference, and every step
  made to fail is red exactly as on the oracle (section 11.1).

### 5.8 Isolation and trust

No pull request comes from a fork (decision 8): isolation is designed for trusted authors whose code
may be buggy, not for hostile outsiders. The rule that PR results never become `main`'s trusted
caches or receipts without verification is a correctness rule and holds on every tier.

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | small jobs (lints, plan) | warm sandbox, 1 CPU | GitHub API | read-only tools |
| build | PR, merge-group and `main` jobs and their obligations | warm sandbox: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account token; obligations also without `.git`, a GitHub token or the job's environment | allow-list: crates mirror, site storage | read-only trusted; per-obligation clones; never a shared writable path |
| docker | only the three test binaries that need a daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), as their own obligations | a private privileged daemon per request, in a sidecar, on tainted nodes, never shared between requests; rootless and microVM daemons are lab D experiments; a skipped or missing Docker test fails the obligation | as build, plus a registry mirror | the build's test binaries, cloned |
| builder | `main` generations, no GitHub job | unprivileged | crates mirror, storage | writes the trusted scope |

Runner credentials stay in the dispatcher and never enter a sandbox; the App's private key exists
only in the control plane, and agents act under an identity without repository administration.

### 5.9 Failure handling, releases and rollback

- **Journal discipline:** persist the intent, make the call, record the outcome; unknown is never
  absent. The executor journals the plan id, job id, run-service URL, job token and environment id
  before acting, runs no credential-bearing step before its first renewal succeeds, renews the lease
  every 60 s from `lockedUntil` (converted to a conservative monotonic deadline) with a loop written
  fresh (the upstream overload dereferences a null request when renewal fails,
  `JobDispatcher.cs:748-852`), and after a crash resumes or fails the job with its own token, never
  twice. The ledger persists an execution's intent (key, node, epoch) before a sandbox starts, and
  results land by compare-and-swap on that epoch.
- **Fault injection** before each move: crashes around every remote call, journal write and step
  boundary, a lost lease, duplicate and reordered webhooks, a ledger partition, a lost node, the
  control plane and a site. **No fallback** (decision 18): a divergence is fixed forward, and a bad
  release is recovered by redeploying the previous immutable image digest (D15); the cluster runs
  only released versions, never the change under test.
- **Control plane:** k3s has one server today (§B.1.5); a whole-cluster dependency waits until
  control-plane and site loss are qualified: three servers across failure domains, and a third-site
  witness for any leader the sites share, such as the ledger's.

### 5.10 What is reused, after review

Nothing is reused by default (decisions 14, 17): each component is reviewed, and kept only when it
is well designed for this architecture.

| Component | Verdict | Why |
|---|---|---|
| velnor-legacy's broker, JIT and run-service client, Results client | Port and refactor | The wire behaviour is right and tested; legacy-protocol paths are stripped and the god files (`executor.rs` 37,028 lines, `runner.rs` 36,759) are split |
| velnor-legacy's expression engine (3,565 lines, from actions/runner v2.337.0) | Port as-is for production; also the harness's reference | Its results are what v2.338.2 returns on every path; the new evaluator is ported when GitHub switches (owner question D4) |
| velnor-legacy's checkout, artifacts, attest, App-token and mise adapters | Port and refactor | Behaviour and tests are sound; they move onto the node stores |
| velnor-legacy's `gha_cache.rs` (17,140 lines, v1 and v2 server), Docker lease, JS sidecars | Redesign or drop | Cache v1 is out; the cache layer is new (section 7.2); there is no Node |
| velnor-new's `velnor-runner-core` (lifecycle, `CleanupProof`, `OwnedIds`) | Port and refactor | The tri-state "unknown is never absent" rule is right; its two open slot defects (a short id accepted as covering, a live slot replaced) become tests |
| velnor-new's `velnor-runner-github` (scale-set wire types) | Port and refactor, pending review | Pure types; it carries official-runner lifecycle states |
| velnor-new's `velnor-host` journal (Turso) and launch state machine | Redesign | Bound to the official-runner pair; `Journal::open` drives the test floor. The discipline is kept |
| velnor-new's Docker client and spec audit | Port into the Docker backend, pending review | The audit rules (no host mounts, no outer socket) are valuable |
| The `Runtime` trait proposed for velnor-new's runner | Port its observation model into the platform trait | `Present`/`AbsentProven`/`Unknown` and Pod-UID proofs are right; the official-runner pair methods are dropped |
| `velnor-runner-host`'s official-runner wrapper | Redesign into `velnor-official` only if review fails it | Decision 19 keeps the official runner as an executor; the wrapper is reused only if it passes review |
| `velnor-ci`, `velnor-gen` | Port and refactor | Generators of record; `gate.rs` gains `--app-check` and `--push-check` (section 8.2), and velnor-gen's hard-coded hosted release runners (`release.rs:29`, `release_binaries.rs:44-46`) go in phase 0 |
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
- **Full tree; keys narrowed only by audited reads.** Every obligation executes on the whole tree
  (today's checkout is `/*` without `/docs/**`, `ci.yml:97`). A key may be narrower than the tree
  only for an obligation whose audited reads (opens, stats, directory listings and failed lookups)
  all fall inside it; one read outside widens that obligation's key to the whole tree and opens a
  planner fix. In velnor-new 79 of 86 files using `CARGO_MANIFEST_DIR` climb out of their package
  and 70 test files list directories, so most test keys stay wide. The key includes the exact
  package selection and features: feature unification follows the selected packages, and a test
  passed under `-p a` and failed under `--workspace` [M].
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
| Merge group | every obligation listed; the PR's whole verdict is reused only when the group's tree equals the PR's (about 75% with auto-enqueue against the queue tip; 16% today); otherwise per-obligation receipts with equal keys are reused and the rest runs |
| Push to `main` | promotion of the group's receipts and bytes; audit A1, the third execution, reported by the push run's Actions `Required` job, which the release gate reads |
| Release tag | promotion of verified bytes; zero compiles (section 8.4) |
| Any obligation in C5 | always runs |

### 6.4 Soundness cases

Every reuse or skip, with its four parts (decision 11): why the key is complete, what fails if it is
not, how that is detected, and the automatic fallback.

| Reuse | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| Exact-tree verdict (rebase-only push, re-run, identical merge-group tree, same-SHA push) | The tree is every input; definition and executor identity equal | A non-tree input changed: clock, network, a tool | C5 obligations always run; audit A1 | Revoke by epoch; full run |
| Per-root receipt | Root tree, reached roots, shared files, all manifests, declared inputs, the exact package selection and features, definition | An undeclared input outside the root | Read audit (incl. `ENOENT`); A1; 2% sampling | Demote to exact tree; planner fix |
| Per-package receipt | Closure trees, all manifests, the package selection and features, spawned-executable digests; only where every audited read falls inside | A read outside the closure, or a spawned binary outside it | Shadow at 100% until about 3,000 clean; read audit; A1 | The key widens to the whole tree |
| Candidate receipt reused across pushes and PRs | Same key, Velnor-observed, signed | A PR sandbox leaked state into a result | Sampled re-execution; trusted only after A1 | Candidate reuse off for that class |
| `velnor verify` before the push | Same sealed Linux sandbox and key as CI | A developer machine's result | Only Velnor-executed results count; Mac runs never do | Runs in CI |
| Warm compile snapshot | The materialiser sets every time: unchanged files keep the generation's recorded times, every changed file gets one later than its newest output; cargo's fingerprints then rebuild what changed | A changed file with an older time passes on stale code (reproduced: `cargo test` exits 0 and compiles nothing); a stale `OUT_DIR` or proc-macro output | A planted older-time edit as a standing canary; A1's cold digest comparison, because the time rule alone never authorises a hit; sampled units rebuilt from empty | Retire the generation; build cold |
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
  [T], so a stale generation serves only the merges inside that window. A1 is a changed test's third
  execution, after the PR's twin, and the push run's Actions `Required` job reports it for the
  release gate. A class with about 3,000 clean comparisons then drops to 2% sampled re-execution.
- **Audit A2** runs nightly with the clock shifted 8 and 30 days ahead through a shim, so a
  time-dependent failure is found before its date. **Degraded mode:** with the ledger unreachable
  nothing is satisfied by lookup; obligations run fresh, slow but never wrong.

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

- **Backends:** the local filesystem (node NVMe, or a directory on a Mac); RustFS single-node, or
  multi-node as an erasure-coded cluster per site over the workers' disks with optional site
  replication; any S3-compatible store (MinIO, AWS S3, R2) through the same S3 backend.
- **Tiers and routes.** Tiers are composed in configuration (`local`, `local → site`, `local → site
  → cross-site`), each naming a backend, and each data class is routed separately: `actions/cache`,
  artifacts v4, promoted release artifacts, the mbx-cache store, warm-snapshot shipping; one RustFS
  cluster per site can back all of them. Defaults: local only on Docker and macOS; node NVMe plus a
  site RustFS cluster on Kubernetes.
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

**Measured** [M, `experiments/mbx-cache-cluster.md`, the runner workspace of 357 crates at
`/w/velnor-new`]: plain cargo cold 92 s; over a reflink clone of the warm target, 0.25 s and 2.5 MB
for the clone and 0.31 s with 0 crates for the build; today's ext4 copy 8.6 s and 10.75 GiB per job;
mbx on a second node from a warm `mbx-cache` 39.9 s; a lockfile change 67.4 s with plain cargo, 88.4
s with mbx on the same node, 43.4 s on another node once one has built it. On the owner's Mac both
engines rebuilt the same 6 crates at every new path [M, `experiments/mbx-cache-mac.md`].

- **The hot path is plain cargo over a reflink clone of the node's warm snapshot**, at a fixed
  checkout path, with the time rule of `velnor-ci inputs restore` (section 6.4). Job work
  directories live on the XFS volume (local-path volumes; `emptyDir` is ext4 and cannot clone).
- **No transport on the verdict path.** A request goes to a node that holds its generation, or
  builds cold. mbx with an `mbx-cache` per site (its store on the site's RustFS) only fills nodes in
  the background, through per-crate reuse: a cold node in about 40 s instead of 92 s, a lockfile
  change in about 43 s instead of 67 s once any node has built it. It is kept while at least 10% of
  build jobs would otherwise start cold or mismatched, or while it saves at least 20 runner-minutes
  a day (owner-approved, N54), and lab B3 keeps any transport only if it beats a cold build on more
  than 10% of requests. The honest alternative: incremental warmers make nodes rarely cold, and a
  snapshot copy moves about the same 8.7 GiB.
- **A reflink clone of the site builder's generation is the default**, the only variant measured on
  the cluster. An overlayfs lower layer (shared page cache behind a kernel-enforced read-only layer)
  and compressed layers are lab B2 and B3 candidates with no default role; a hard-linked `deps/`
  base is not adopted, because one ownership mistake would put one request's bytes into another's
  verdict. `-Z checksum-freshness` would retire the time rule, but it is nightly-only on 1.99.1.
- **`CARGO_INCREMENTAL=0` on every build that mints a receipt or ships bytes**, as today;
  incremental state only in lanes that mint nothing, because the cold digest audit does not cover
  incremental outputs (edits took 1.5–2.8 s against 9.2–12.2 s [M, `experiments/blaze.md`]). Lab B4
  enrols it only at zero differences.
- **blaze is rejected** on evidence: it built none of velnor-new's five workspaces, ignores the
  1.99.1 pin, and missed a clippy `deny` lint; cold builds were 12× and edits 5–15× slower than
  cargo incremental [M, `experiments/blaze.md`].

### 7.4 Node-local mirrors

Each node keeps a bare mirror of every configured repository (decision 24), covering PR head refs,
merge-group refs and tags. Push webhooks fetch only the new objects, and a periodic catch-up repairs
anything a webhook missed. New or wiped nodes seed from a git bundle in the site store, not from
GitHub. Mirrors are read-only to jobs.

**Native checkout from the mirror,** with a target well under 1 s:

1. Make sure the exact commit is in the mirror, fetching only missing objects in one round trip; the
   mirror is never assumed fresh.
2. Reflink-clone the node's pre-materialised tree of its latest `main`, and rewrite only the files
   whose blob ids differ (deletions, modes and symlinks included), each from a hash-verified blob.
3. Verify before the first step that the commit and tree are exactly the ones requested: by
   construction from the base's recorded tree hash, with a full recompute on a sample and whenever
   that record is missing.
4. Set times by the warm-snapshot rule (section 6.4), so cargo's fingerprints stay valid. A job that
   needs `.git` gets the read-only mirror through alternates; obligations get no `.git` (C5).

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

Once the transition in section 8.2 completes, a PR and a merge group have no Actions run:
verification is the obligation graph run through `ensure`, and the App's status judges it. `ci.yml`
keeps one always-run job named `Required` on push to `main`; it reports the cold audit of the landed
commit (section 6.5) and is what the release gate reads. Release, image and scheduled workflows stay
Actions jobs, executed on Velnor. Inside a root's request, test binaries build once per
configuration, and format, lint, documentation, "check alone" and tests run as parallel lanes, with
one target directory per check kind (cargo locks a target directory exclusively,
`docs/hard-decisions.md:56-62`), clippy and doc metadata pre-built, and one jobserver per sandbox.
`cargo-deny` reads the RustSec database pinned by commit.

### 8.2 The label flow and the required status

**The gate is one commit status, `Velnor / Verified`,** written by a dedicated GitHub App (working
name `velnor-verify`) and bound to that App's `integration_id`. GitHub applies a branch's required
list to the PR head, to merge or enter the queue, and to each merge group's head, to land; the same
status gates both, on two SHAs. No Actions job is in the list: a skipped job passes a required
check, also when it is skipped through `needs` [docs], and integration 15368 is every workflow in
the repository, including one a PR adds or edits, so a dispatched or PR-edited workflow could mint
an Actions-bound check [M: a dispatched run's `Required` landed on the head commit under 15368]. A
status has one passing state, where a check run has three that pass (`success`, `skipped`,
`neutral`).

| Event | Velnor's action | `Velnor / Verified` |
|---|---|---|
| `labeled velnor:verify` | Verifies the current head merged with the queue tip | `pending`, then `success` or `failure` |
| push to a labelled PR | Cancels the running request at once and starts one on the new head; shared work is joined | none on the new SHA until written |
| push to an unlabelled or draft PR; `ready_for_review` | Nothing | none, which blocks |
| `unlabeled`, `closed` | Cancels; writes nothing; a verdict already written stays true for its commit | unchanged |
| `merge_group.checks_requested` / `.destroyed` | Verifies the group head, reusing the PR's verdict only on an identical tree / cancels | on the group's head SHA |

- **Why an unverified head is never green.** Only the App satisfies the context; one function writes
  `success`, from a signed ledger verdict naming the subject SHA, tree and plan epoch, to that SHA
  only, never the test-merge commit; a new push has no status, and absence blocks. Writes are
  serialised per SHA. The detail view is a differently named check run, where "not requested" is its
  own state and never a false failure.
- **Liveness from state, not events.** Every 10 s a reconciler reads the labelled open PRs with
  their heads (a conditional request; a `304` costs no rate limit) and the queue's branches, starts
  what is missing, cancels what is unwanted, and corrects any status the ledger does not hold; a
  webhook only cuts the delay to about 1 s. Labels are applied by people or with an App token, never
  `GITHUB_TOKEN`, whose label events may reach no App.
- **PRs at one SHA share a verdict:** it belongs to the commit, and the merge group verifies the
  merged tree again.
- **No bypass.** `bypass_actors` is empty (an admin bypass carried 7 of 136 merges in three days
  [M]); break-glass is an audit-logged ruleset edit, and agents act under an identity that cannot
  edit the ruleset.
- **The release gate.** `product-release.yml:86` is unchanged. `velnor-ci release-gate` gains
  `--app-check 'Velnor / Verified'=velnor-verify` (the App's newest status on the release SHA must
  be `success`, inside an active ruleset with empty `bypass_actors`) and `--push-check Required`
  (the push-job test for a name the ruleset no longer lists) (`gate.rs:84-94,277-288,354-376`); the
  helper is released and pinned before the ruleset changes.
- **Transition.** Step 0, now: today's two contexts bound to integration 15368, `bypass_actors: []`,
  ALLGREEN. Step 1: the changed helper released. Step 2: the App in shadow. Step 3, after lab A and
  300 heads with no App `success` beside an Actions failure: all three contexts required and the
  label flow on. Step 4: the Actions contexts dropped. Before step 3, the only safe label gate on
  Actions is an always-run job with no `if:` and no `needs:` that fails without the label.

### 8.3 Merge queue

GitHub's queue stays (decision 7); Velnor runs no merge train. **ALLGREEN** applies now: under
HEADGREEN three commits whose own `Required` failed already sit on `main` [M]. Depth is a cost
setting: `max_entries_to_build` stays 2 until a merge-group verdict takes at most 5 min at P90 and
paired false failures are at most 3%, then 4 (at 8.1% false failures depth 4 disturbs 29% of entries
and depth 8 49%); the timeout falls from 90 to 15 min with it. Speculation verifies the queue tip's
ordered prefixes and single-ejection bypass trees at batch priority through `ensure`; a wrong guess
wastes only batch CPU. **How often a group's tree equals the PR-tested tree** [M, a replay of 172
queue entries]: today 16%; with the PR verdict computed on `merge-tree(queue tip, head)` and the PR
auto-enqueued inside the window, 83% at 1 min, 78% at 2, 74% at 2.5, 67% at 5, 56% at 10 and 37% at
20 min; about 60% without auto-merge. Plan on 75% at best. A group always derives its own keys and
executes every miss, so no verdict depends on the rate, and latency targets carry the group's delta
for one merge in four. Agents enable auto-merge and stop catch-up merges.

### 8.4 Release: build once, promote by digest

A release never rebuilds (decision 10). Which profile tests use was measured [M, `generator` and
`ci-helper`, cargo 1.99.1, CPU-seconds]: a release-profile test build shares only 15 of 90 crates
with the release binary, because the binary uses `panic = "abort"` and tests need unwinding.

| Option | CPU per cold build, `generator` / `ci-helper` | What is tested |
|---|---|---|
| Test the release build | 320.8 / 365.7 CPU-s | optimised code without debug assertions; still not the shipped panic strategy or musl |
| Debug tests, release bytes built once | 191.9 / 282.8 CPU-s | today's coverage, and the shipped bytes get their own checks |

Tests stay in the dev profile; one `verified` profile for both is not adopted, because Cargo ignores
`panic` for test targets and the tools ship as musl binaries with `panic = "abort"`, thin LTO and
`strip`. Release bytes are one obligation keyed by closure, lockfile, toolchain, profile, target
and, while the commit id is compiled in, the commit SHA. That key is first computable in the merge
group, whose SHA becomes `main`'s [M], so the bytes are built there, in parallel with verification:
for the tools, the release PR's group builds them and smoke-tests the shipped bytes as part of its
verdict; for the product, released by dispatch on the `main` tip, each group builds them outside the
verdict, replacing today's per-push preview build. They are qualified once on the exact bytes
(`--version`, `check-tag`, the black-box suites that today run only after the merge), packed
deterministically with `SHA256SUMS`, and attested once. A tag runs one job: gate, fetch by digest,
re-hash, `gh attestation verify`, publish (idempotent, with a read-back of what was published, and
crates.io receiving the exact packaged `.crate` bytes): about 15 s instead of 190–434 s for the
tools and 4,105–6,238 s for a product release [M]. Stamping the commit after link instead of
`option_env!("VELNOR_COMMIT")` would build the product once per closure instead of once per tip; it
is a lab item, not a default. The macOS legs are paused (decision 9); `aarch64-unknown-linux-musl`
is cross-compiled and smoke-run under user-mode qemu.

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
- **Dependency `opt-level = 3` holds with assertions on** [M, one test, three configurations]: with
  `debug-assertions` and `overflow-checks` both on, the measured test used 67% less CPU and 74%
  fewer instructions than today's dev profile, and the two checks cost 5% of instructions. The
  override is adopted with both checks written out and `turso_core`'s own line raised (a named
  override beats `"*"`); it removes no assertion and changes every compile key once. No target
  counts the gain until lab B1 measures the whole suite on a worker.
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
  §B.2.2), so four fit per worker, idle ones included. Nextest's threads equal the class's fixed CPU
  count, which is in the key; only the compile jobserver's width follows the node's free CPUs, from
  16 up to 64, so a lone PR builds faster without changing any output.
- **Page cache.** Clones do not share page cache, so each sandbox's cache counts inside its 24 GiB
  limit, about 7 GiB above the heaviest job's peak; a fully read target is 14.29 GB, which caps a
  node at six build sandboxes until lab B2 (memory already caps it at four).
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
- **Other organisations.** ChainArgos and jackin keep their own ARC scale sets, outside velnor-new's
  path, with guaranteed quotas and separate namespaces and stores, until they choose Velnor.
- **Hardware.** Two more k3s servers across failure domains come before any whole-cluster
  dependency. A high-clock node (1.3–1.5× on the serial path [E]) is rented for a measured trial
  only after the test-floor fixes; 10 Gbit/s between same-site workers only when a measurement shows
  the need. A worker is bought when CPU stays above 70% at peak after dedupe, or the eligible merge
  wait p90 exceeds 10 s for two weeks.

## 10. Servers and cluster tuning

The changes that pay are elsewhere in this design: gate jobs on the cluster (−5.8 to −57 min per
run, §A T5), honest requests and quotas (they remove the 1.2–2.4× stretch under load, §A T6), no
`-j2` caps, work directories on XFS, and Docker only in its class. Debuginfo drops to line tables
for workspace crates and none for dependencies (−10 to −24% cache, −20 to −27% CPU on a runner-host
rebuild, I108). The governor, same-site 10 Gbit/s, the CPU manager, THP and mold get no credit until
a worker shows the stall. SMT stays on, the kernel stays at 6.12, and flannel over WireGuard stays.

## 11. Delivery: per-job-type migration, milestones and deletions

Agents do all the work (decision 15): about 560 agent-hours of building and 100 of review, roughly
2.3 days for 12 builder agents, 4 reviewers and an orchestrator. Coding therefore does not set the
dates; velnor-new's own CI and merge queue, review gates and owner decisions, conformance evidence,
and free capacity, API limits and file conflicts do. The configuration comes first because it speeds
up every later PR: an agent PR's cycle falls from 94 min at P50 to about 34 after day 0 and about 6
after day 8 [E], saving about 17 h of CI waiting at P50 (60 h at P90) on a critical path of about 14
dependent PRs.

### 11.1 Per-job-type migration

A job type moves when, counted before the move because nothing moves back (decisions 16, 18), its
corpus cases pass at 100% on the Kubernetes backend, every planted mutant is caught, N replayed
real-commit pairs show no unexplained difference (a bound below 3/N at 95%), every step made to fail
is red exactly as on the oracle (the deterministic zero-false-green check), and its start latency is
no worse than ARC's at P90. Afterwards sampled shadow runs continue (decision 19); a divergence is
fixed forward on a 2-hour target, and a bad release redeploys the previous image digest (D15).

| Cohort | Job types | Pairs (N) | Saves when it moves | Deleted in the same change |
|---|---|---|---|---|
| 1 | `Required` gate, `Required verdict`, Plan, Ledger CLI, the four lints, freshness | 100–150 per type | about 15 s of acquisition and 25–40 s of set-up per gate job, on the critical path | Their ARC routes and hosted twins; Plan's tool-seed steps; two tool composites |
| 2 | The Rust crate template (26 jobs), `check-maintenance-helpers`, `check-generator-samples` | 300 across crates; 150 | about 30 s of set-up × 26 = 13 runner-min per run; the verdict's maximum 8.5 → ≤ 4 min | 26 routes; six tool composites |
| 3 | `velnor-runner-host`, its `local-mode`, `velnor-local` (the three Docker-class binaries) | 150 each | verdict P90 10.1–10.4 → ≤ 6 min | Their routes; the dind sidecar for velnor-new |
| 4 | State seal (then deleted), job image candidate, source qualification, `release-rust-ci-tools` and product release (Linux) | 30 each; three release dry runs per workflow | hosted queue 5.9–9.1 min per run; a release about 104 → about 1 min with promotion | Hosted release labels in velnor-gen and the engine; the state seal and `cache-prune.yml` |
| — | `qualification.yml`, homebrew and mac-proof | — | — | Retired |

Merge-group and push runs move with their job types. The order relieves the critical path first:
cohort 1 is the gate chain with the least semantics and the cheapest evidence, cohort 2 is one
template for 26 jobs, cohort 3 sets the verdict's maximum, and release types come last because they
run rarely, hold secrets and need attestation and the App token.

### 11.2 Milestones by day

"Day N" means hours 24N to 24(N+1) from the start; the clocks are those of section 1.1.

| Day | Milestone, and what it deletes | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Constraint that sets the date |
|---|---|---|---|---|---|
| Today | Baseline [M] | 26.7 / 92.3 min | 85 / 457 min | 285–347 | — |
| 0 | Configuration landed: today's two contexts bound to integration 15368, empty `bypass_actors`, ALLGREEN, depth 2; the crates mirror re-enabled. **Deletes** velnor-gen's hard-coded hosted release runners, `edited`, the fork twin, the catch-up rule, `-j2` caps, Darwin release legs, per-push image and qualification triggers, duplicate push-run obligations | ≤ 12 / ≤ 20 min | ≤ 35 / ≤ 60 | ≤ 170 | Owner decisions D1–D3, D9; the engine release's wall time |
| 0.75 | First conformance runs on the Docker backend: ≥ 80% of 300 cases, the first 12 mutants caught | ≤ 11 / ≤ 18 | ≤ 30 / ≤ 55 | ≤ 160 | Critical-path coding: reviews, then the platform trait, then the Docker backend |
| 1.7 | Cohort 1 on Velnor Native on Kubernetes; native checkout from node mirrors. **Deletes** the gate routes and seed steps | ≤ 9 / ≤ 16 | ≤ 25 / ≤ 45 | ≤ 150 | D8 and the Kubernetes backend; 150 pairs per type |
| 2.8 | Cohort 2. **Deletes** 26 crate routes and six tool composites | ≤ 5 / ≤ 9 | ≤ 13 / ≤ 25 | ≤ 110 | Evidence and replay capacity |
| 3.4 | Cohort 3; the journal template, loop splits and parallel helper tests landed | ≤ 4 / ≤ 7 | ≤ 11 / ≤ 20 | ≤ 100 | Evidence; runner-host test files have a single writer |
| 5.5 (P90 8.5) | Every CI job on Velnor; after lab A and 300 heads in shadow, `Velnor / Verified` required, then the Actions contexts dropped. **Deletes** the interim fail-closed step, the Actions `Required` jobs, hosted release labels, the state seal and prune | ≤ 3.5 / ≤ 6 | ≤ 9 / ≤ 16 | ≤ 90 | D7 and lab A; release dry-run wall time |
| 7 (P90 10) | Final deletion. **Deletes** ARC's velnor-new scale set and execution profiles, `velnor-runner-host`'s official-runner wrapper unless review kept it for GitHub Official, velnor-local's pair mode, the Turso journal and the rest of `crates/velnor-runner` that review did not keep. **Keeps** the official runner's image and launch code as GitHub Official and the reference executor (decisions 19, 20) | ≤ 2.5 / ≤ 5 | ≤ 6 / ≤ 12 | ≤ 60 | Local runs on the Docker backend; review of a large deletion |
| 7 | Warm snapshots, the site's RustFS tiers and native `actions/cache`; exact-tree reuse | ≤ 2 / ≤ 4 | ≤ 5 / ≤ 10 | ≤ 55 | D8's node changes |
| 10.5 (P90 13.5) | **Committed targets.** Receipts after 3,000 clean comparisons; the flake rate from 300 pairs; speculation from 50 merges; velnor-gen's workflow deletes the engine-generated CI | **≤ 90 s / ≤ 4 min** | **≤ 3 / ≤ 8 min** | **≤ 35** | Statistical evidence |
| 16–18 | **Stretch measured:** per-package keys, `velnor verify`, longest test ≤ 15 s, deeper queue | 30 s / 100 s | 100 s / 4 min | ≤ 15 | Package enrolment; labs B and C |

A milestone that misses its numbers does not start the next cohort until the miss is explained;
stretch work continues. Work items take 2–8 agent-hours each, one PR of under 400 refactored lines;
a port lands in two commits, first a verbatim move proven by a function-hash checker and then the
refactor; every PR that replaces something deletes it; the conformance harness is the acceptance
oracle for behaviour, and reviewers judge structure, safety and deletions.

### 11.3 Lab experiments and their safe defaults

Four rulings depend on facts only a lab can settle; each default stays correct whichever way the lab
goes.

| Lab | What it runs | Pass | Default until then |
|---|---|---|---|
| A. GitHub semantics | A lab repository with a ruleset, a merge queue and a test App: no status; App `pending`; App `success`, then one more push; the context posted by a user token and by `GITHUB_TOKEN`; a status only on the test-merge SHA; a required Actions job with `if: false`; a skipped caller whose inner job is required; a dispatched run; two PRs at one SHA; queue entries with no report, `failure` and `success`; labels changed with `GITHUB_TOKEN`, an App token and a user token | The no-status, pending, foreign-source and skipped-caller cases block; App `success` merges until the next push; a queue entry lands only on `success` | Today's gate, hardened and bound to 15368; the App only observes |
| B. Build matrix | One idle worker, three repetitions: B1 dependency `opt-level` 0 and 3 against assertions on and off, whole suite; B2 reflink against overlayfs with 1, 4 and 6 sandboxes, including the planted older-time edit and digests against a cold build; B3 seeding and lockfile changes by `mbx-cache`, file-delta sync and compressed layers; B4 incremental on and off over 300 replayed diffs and 20 planted failing edits | B2: the planted edit rebuilds and digests match; B3: a transport beats a cold build on more than 10% of requests; B4: zero differences | Reflink; `CARGO_INCREMENTAL=0` for receipts; the `opt-level` override with checks on, its gain uncounted |
| C. Shadow ledger | Beside production for 300 merges: C1 whole-tree and per-obligation hits of each merge group against the PR's receipts; C2 twins on two nodes against two runs 30 min apart, on 20 tests with seeded failure probabilities of 0.05–0.2 | C2: joint pass rates match (1−p)² | Groups derive their own keys and execute every miss |
| D. Docker class | The three Docker-needing binaries, 200 runs each, under a private privileged daemon, a rootless daemon and a microVM | Equal pass, fail and skip sets; nothing one request leaves is visible to the next | A private privileged daemon per request on tainted nodes |

## 12. The first ten work items

| # | Item | Scope | Acceptance | Depends on |
|---|---|---|---|---|
| 1 | **Owner decision pack** | Section 14's questions due in the first 48 h, each with its default, answered in one sitting | Recorded answers | — |
| 2 | **Release the engine version that moves the gates** | Dispatch the prepared release; bump the lock | Asset sha256 equal to the lock | D1 |
| 3 | **Gate jobs off hosted runners** | Overrides for plan, alint, cargo-deny, cargo-machete, jactionlint, ledger-cli, `required` and `required-gate` (still `ubuntu-26.04` at HEAD); regenerate | No CI job on a hosted label in 50 runs; hosted queue on the critical path p90 ≤ 1 min | 2 |
| 4 | **Merge procedure and ruleset** | Auto-merge at creation; no catch-up merges; today's two contexts bound to integration 15368, empty `bypass_actors`, ALLGREEN, depth 2, minimum wait 0 | ≥ 90% auto-merge; green → enqueue P50 ≤ 30 s; no merge outside the queue; ruleset read back | D2, D3 |
| 5 | **The day-0 engine batch** | No `edited`; the interim label flow as an always-run job with no `if:` and no `needs:` that fails without the label; the cancel stall; the push run reduced; macOS paused; per-push triggers narrowed; `-j2` caps out; per-job profiles | A body edit creates no run; an unlabelled head is red and cannot merge; supersede wait p90 ≤ 30 s; ≤ 2 runner-min per push | D9 |
| 6 | **Reviews before reuse** | `velnor-host` (Docker client and audit, proofs, journal) and the proposed `Runtime` trait; a verdict per part with reasons | A verdict table in section 5.10 | — |
| 7 | **The executor root** | New Cargo root `executor/`, line budgets, and a function-hash checker that proves ported bodies equal to their source | The checker fails on one changed token | — |
| 8 | **Capability model and platform trait** | `Capabilities` with the fallback table; `Platform` with warm pool, mounts, exec and log stream, limits, cancel, teardown, recovery and tri-state proofs; a conformance kit for backends | Each fallback has a test that switches its capability off; the kit runs | 6, 7 |
| 9 | **Broker and JIT client** | Ported from velnor-legacy with the legacy paths stripped; session, poll, acknowledge | Lab transcripts replay; the legacy-endpoint scan is clean | 7 |
| 10 | **The lab and the reference executor** | The upstream official runner on the Docker backend first; the paired-job transform and replay driver | Paired runs of one job type produce a comparison report | 8, D8 for the cluster lab |

## 13. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A reuse gives a false green | Shadow re-execution; 2% sampling; audit A1; read audit | Revoke by epoch; work runs again |
| Velnor Native departs from GitHub's semantics | The harness on every change, nightly and per official release; sampled shadow runs | Block the release; fix forward |
| GitHub changes the protocol | The harness within 14 days of each release; per-version deadlines from the API with an alarm 21 days ahead; a canary every 10 min | Fix before the deadline; a critical update pages at once |
| The skipped-job trap or a mis-set ruleset lets an unverified head merge | Permanent canary PRs probed every 10 min | `Velnor / Verified` is the only gate; no bypass actors |
| A move leaves no way back and diverges | Sampled shadow runs; per-type failure rates | Fix forward on a 2-hour target; redeploy the previous image digest |
| A backend behaves differently (exec, signals, mounts) | Per-backend conformance | Capability fallbacks, never backend-specific branches in the core |
| A store is misconfigured | Content verification on read | Slower, never wrong (section 7.1) |
| Stale compile state | Audit A1 digest comparison | Retire the generation; build cold |
| The cluster is saturated | Per-class CPU | Batch work preempted first; reserved merge path; lab replay at night and on Docker hosts outside the cluster |
| Owner decisions wait overnight | The decision pack's due hours | One pack with defaults (work item 1); the Docker backend lets conformance start without D8 |
| Agents collide on shared files | Claims before branches; conflict counts | One owner per crate and per shared file; no catch-up merges |

## 14. Owner questions

Only what is still open; each comes from the delivery plan's decision pack, and the plan runs on the
default until the answer arrives. Decided already: parking the old runner's open PRs (decision 16)
and the lab's reference executor (decision 19). Ruled on evidence, so no longer questions: two
pre-merge executions and ALLGREEN (D10), and where release bytes are built (D12).

| # | Question | Example | Recommendation | Needed by |
|---|---|---|---|---|
| D1 | Release the prepared engine version that moves the gate jobs off hosted runners? | Eight gate jobs still run on `ubuntu-26.04` and waited 57 of a 71.8 min run on the worst day | Yes, now | hour 0 |
| D2 | Should agents enable auto-merge and stop merging `main` into their branches? | 0 of 40 recent PRs used auto-merge; the median waited 7.5 min between green and enqueue | Yes | hour 0 |
| D3 | Apply the ruled ruleset changes now: today's two contexts bound to integration 15368, empty `bypass_actors`, ALLGREEN? | An admin bypass carried 7 of 136 merges in three days, and three commits with a failed `Required` sit on `main` | Yes; depth 2, raised to 4 and the timeout to 15 min once a group verdict takes ≤ 5 min at P90 and paired false failures are ≤ 3% | hour 2 |
| D4 | Ship the evaluator whose results the latest official runner returns, though decision 3 bans the legacy one? | v2.338.2 computes the new evaluator in compare mode but returns the legacy `Expressions2` result on every path | Yes: match current behaviour, verified by conformance, because correctness comes first; port the new evaluator when GitHub switches | day 3 |
| D7 | Create the `velnor-verify` App (`statuses: write`, `checks: write`, a public webhook endpoint) and an agent identity without repository administration? | Today agents label and merge under the owner's identity, which can edit the ruleset | Yes; the App's status becomes the gate after lab A (section 8.2) | day 2 |
| D8 | A deploy identity for `velnor-system`, and the node changes (XFS work directories, reservations, taints)? | Job directories sit on the ext4 OS disk, where a warm target copy costs 8.6 s and 10.75 GiB | Yes | hour 6 |
| D9 | Pre-approve engine releases that change only CI? | Each day-0 change needs one release | Yes, one approval for the batch | day 0 |

| D11 | The per-type cutover counts of section 11.1? | Cohort 1 needs 150 replayed pairs per type | Yes, as written | day 1 |

| D14 | Delete the engine crates from velnor-new once velnor-gen's workflow replaces the engine's CI? | The engine's orchestrator and renderer are about 275k lines | Only after checking for other consumers | day 8 |
| D15 | How is a bad Velnor release recovered? | A release that fails every job also blocks the PR that fixes it | Redeploy the previous immutable image digest; then an admin merge of the fix with lab conformance evidence | day 1 |

# Velnor runner architecture

**Status:** Proposal (v3); it replaces nothing yet. It follows the owner's binding decisions 1–24
(section 5.1). The panel's contested facts are ruled on evidence (decision log); where a ruling
names a lab test, the design runs on that ruling's safe default until the lab reports (section
10.3); questions only the owner can answer are in section 12. **Scope:** how Velnor verifies, merges
and releases velnor-new's changes on the project's own machines, so that CI stops being the slowest
part of delivering a change, with the generator (`velnor-gen`) and helper (`velnor-ci`) where the
runner needs something from them.

**Sources.** The [evidence pack](../research/velnor-runner-2026-11/evidence.md) (cited as §A T5 and
similar); the experiments under `../research/velnor-runner-2026-11/experiments/`; the panel's
Rust-only redesigns, their comparison and the [rulings](../research/velnor-runner-2026-11/rust-only-rulings.md);
the [delivery plan](../research/velnor-runner-2026-11/delivery-plan.md); and the [decision
log](../research/velnor-runner-2026-11/decisions.md), which records every idea with its decision and
who raised it. Numbers are tagged **[M]** measured, **[M, proxy]** measured on a stand-in clock or
cohort that the row names, **[E]** estimated (with the arithmetic), **[C]** read from code at the
cited path, or **[T]** target. A target is a **goal**: phase gates measure it, and no goal counts as
met until joint traces under load (misses, fresh work, queue ejections, competing load) support it.

## 1. Goal, metric and the contract

### 1.1 The metric

The single metric is **push-to-merge**, from the final labelled push to the merge. **PR verdict**
runs from the label or a labelled push to a green `Velnor / Verified`. Beside them, each on its own
clock: push-to-green, time to the first actionable red, enqueue time, queue residence, post-green
landing, label wait and failed attempts per merged change; weekly P50 and P90 with intervals, per PR
class, compared only on identical cohorts through an event-level replay. Runner-minutes (I117) count
each occupied slot once, split completed, cancelled and superseded work, and charge twins, group
misses, builders, speculation, audits, release preparation and mirror fills; bytes from GitHub and
from the LAN are reported apart.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M, proxy] 98 merged PRs, from the first observable CI run at the final SHA, not a push timestamp (`proposals/opus-5.5.md` §1.1) |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M, proxy] run creation to completion, 77 completed PR runs, 27 of them failed (§A T2) |
| PR runs cancelled; runs on an unchanged head | 1,156 of 2,558 (45%); 469 (18.3%), the `edited` trigger | [M] §A T1; F-H07 |
| Admission span, PR runs | 2 s p50, 226 s p90, 2,203 s max; a superseding run waits 60 s p50, 626 s p90 | [M] 200 sampled runs (F-H06) |
| Runner-minutes per run (PR / merge group / push) | 52.9 / 64.6 / 62.0 | [M] §A T2 |
| Runner-minutes per merged PR | 285–347 for CI; about 460–515 with every workflow | [E] decision log; the panel's audits |
| Full runs per merge | 3 (PR, merge group, push on the same SHA in 120 of 159) | [M] §A T7 |
| False failures per full run | 8.1%, one direction (10 of 124 push runs) | [M] decision log S1 |

### 1.2 Same confidence: the contract

Correctness is the hard constraint and speed is optimised only inside it (decision 11): anything
that could turn a real failure green is rejected, however fast. False reds have a budget; false
greens have zero tolerance. "Same confidence" is this contract.

1. **C1. Every commit that lands on `main` is verified.** For every landed commit and every
   obligation the released inventory lists for it, a passing verification record exists whose key
   equals the obligation's key at that commit, with every required witness passing (section 6.2).
   An *obligation* is one unit of verification: format, lint, docs, a per-crate check, one test
   binary under one configuration, release bytes, or a named check. ALLGREEN keeps this true for
   every landed commit, not only a group's tip: under HEADGREEN three commits whose own `Required`
   failed already sit on `main` [M].
2. **C2. Keys capture every input, by construction:** sources, every workspace manifest, toolchain,
   dependency versions, environment, flags, features, the binaries a test executes
   (`CARGO_BIN_EXE_*`), data files, the execution fingerprint and the executor's identity (section
   6.1). Every obligation runs on the full tree: cargo refuses a missing member, and a trimmed
   sandbox turns a real failure green [M, reproduced]. **Keys are the whole tree** until section
   6.1's enrolment narrows one; a read trace never proves a key complete, because it cannot see a
   branch the run did not take.
3. **C3. Selection is sound or wide.** The plan and the **obligation inventory** come from a
   released helper and the base commit, never from the change under test (`docs/goals.md` rule 12).
   The inventory is versioned and maps every check that gates today (section 8.1); a file the plan
   cannot place selects everything. Each obligation carries its exact expected test IDs, each with
   one outcome; a missing, duplicate or extra ID fails. A head whose plan drops an obligation or test
   that `main`'s plan has fails, unless an owner-signed reduction entry names it.
4. **C4. Only what Velnor observed is evidence.** A receipt is minted only from processes the node
   agent started in a sandbox it created, running the released command graph on inputs it mounted,
   with every exit and the expected inventory observed. **A receipt binds the digest of every binary
   it executed** (each test binary and every `CARGO_BIN_EXE_*` it spawned), the generation it
   compiled over (or "cold"), the node, the witness slot and the epoch. A missing, skipped,
   cancelled, timed-out, masked or zero-test result fails. Results never cross executors, and a
   reference run never produces a verdict, receipt, cache entry or release.
5. **C5. Uncomparable work runs on every request.** Obligations that read the network, the event
   payload, the wall clock, unseeded randomness, Docker images not pinned by digest or the commit's
   identity, or that publish, run fresh on every verification request, an exact-tree rebase and a
   merge group included, unless the varying input is fixed and recorded in the key (a fixed clock or
   seed, pinned fixtures). Tests get a loopback-only network and no event file; sandboxes have no
   `.git`, a scrubbed environment (no `GITHUB_*`; fixed `HOME`, `PATH` and locale) and a fixed CPU
   count.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow with every
   reuse re-executed until about 3,000 clean comparisons per obligation kind; then 2% are
   re-executed. Until compile reuse graduates, the second witness builds cold. Every landed commit is
   rebuilt cold and its changed obligations re-run (audit A1, section 6.5). A disagreement is re-run
   five times to separate a flake from a wrong key; a confirmed one revokes the key version by epoch.
7. **C7. Flakes are budgeted** (section 1.3).

**Two independent witnesses.** Every obligation whose key is new needs two passing witnesses before
it counts, in a PR or a merge group: on two nodes, in two attempts, with different test orders, and
over two different verified generations or with one built cold. A witness counts once, for one slot
of one record; any failure blocks, with no retry. The cold audit of every landed commit is the third
execution, as today's push run is. A regression failing with probability p is caught before merge
with 1−(1−p)², as today; if lab C2 finds the twins correlated, the merge group re-runs the PR's
changed obligations even on an identical tree.

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes; it costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** at most 3% false failures per required run once verification leaves today's full
  runs, and 1% at the goals, each claimed only after about 300 rerun pairs sampled both ways under
  load (zero disagreements in 299 bound the rate just under 1% at 95%). "Not requested" never
  counts. Today's 8.1% [M] comes from full runs.
- **Act:** retries stay at 0, so a flaky test fails and is counted. Two false failures in seven days
  open an owner-labelled issue; a test is fixed, or quarantined only when a blocking check covers the
  same property or under an expiring owner exception.
- **Known causes first:** CPU starvation of tight timeouts, such as the 2 s kill in
  `ci-helper/.config/nextest.toml`'s default profile under pods that request 2 CPU and burst to 84
  cores (§B.2.2); the `event.json` rewrite race between steps (I023).

### 1.4 Targets

Measured on the clocks of section 1.1; the arithmetic is in sections 3, 4 and 10.2.

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Conformance |
|---|---|---|---|---|
| Today | 26.7 / 92.3 min [M, proxy] | 85 / 457 min [M, proxy] | 285–347 (CI) [E] | — |
| App gate, whole-tree keys, cold second witness | ≈ 6 / 18 min [E] | ≈ 9 / 30 min [E] | ≈ 75–90 [E] | 100% on both backends |
| Goal | ≤ 90 s / ≤ 4 min [T] | ≤ 3 / ≤ 8 min [T] | ≤ 50 [T] | 100% |
| Stretch | 30 s / 100 s [T] | 100 s / 4 min [T] | ≤ 20 [T] | 100% |

The goal needs per-root keys enrolled, compile reuse graduated and the test floor fixed; the
stretch needs per-package keys and the commit stamped after link. Further goals [T]: job start ≤ 1 s
P50, ≤ 2 s P90 from the broker message; `Journal::open` ≤ 1 s; no test over 60 s alone (stretch 15
s); per release, zero compiles and about 15 s of publication [E].

## 2. Where the time goes, and what changes the problem

1. **Waiting, not working** [M]. A PR run with no hosted queue takes about 11 min, yet the median PR
   waits 85 min [M, proxy]: gate jobs queue for the Free plan's 20 hosted slots (57.0 of a 71.8 min
   run on the worst day, §A T5), a superseding push waits up to 2,203 s for the cancelled run, the
   queue builds 2 entries, and no agent enables auto-merge.
2. **The same work many times** [M]: 45% of PR runs cancelled; three full runs per merge; crates
   compiled 4.4 times per run; releases rebuild. **Every job is cold** [M]: nothing compiled
   survives a job, and the one restore took 44–61 s, longer than the build it saved (I036).
3. **Code sets the floor** [M]: loops of 40 and 6 cases (258–274 s, 203–271 s), a 125–138 s floor
   behind them, and `build.jobs=2` caps (I059, I061).

The design removes work: nothing runs until the label asks; each unit runs once per unique input and
is reused only where its key is proven; releases promote verified bytes; verified state stays warm
next to the executor. The gain is locality, not the language: Node starts in 87–147 ms [M].

## 3. The latency budget

One typical PR (the 47% class touching only `generator/` or `ci-helper/`), labelled, at the goal
[T], with the estimate in brackets.

| Stage | Today [M] | Goal [T] (estimate) | Bottleneck removed |
|---|---|---|---|
| 1. Event, webhook, job offer | push → run 1–2 s; run → first job 2 s p50, 226 s p90; hosted Plan queue 5.5 min median | webhook 1–2 s; the reconciler within 10 s | Verification starts from the label or labelled push; no Actions job is on the verdict path |
| 2. Acquire | 12 s p50, 18 s p90 from job created to started (scale set) | not on the verdict path | Obligations are not Actions jobs; Actions jobs (release, image, scheduled) acquire on pre-registered JIT identities |
| 3. Sandbox start | ARC pod about 6 s (436 starts) plus a dind probe | ≤ 0.3 s | Warm pool; reflink clone 0.25 s [M] |
| 4. Checkout | 6 s per job × 31–39 jobs; Plan 12 s | ≤ 1 s | Node mirror; a reflink clone of a sealed tree plus the changed files, then a full tree check (section 7.4) |
| 5. Toolchain and tools | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | Read-only verified tool store; native actions |
| 6. Dependency fetch | 3.5–6.1 s per job; sources cache 0 of 48 | 0 s (≈ 3 s on a lockfile change) | Site crates mirror and node crate store |
| 7. Compile | 196–497 crates per job; 81 s runner-host, 14 s velnor-ci; 8.5 min critical path | ≤ 25 s (15 s) | Warm generation: only the change's closure compiles; 0 crates and 0.31 s for an unchanged tree [M] |
| 8. Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s; floor 125–138 s | ≤ 60 s (10 s) | Per-root keys, two witnesses in parallel; the test floor programme |
| 9. Result reporting | 1.48 GB archive upload 36–43 s; `Required verdict` 49 s | ≤ 1 s | Results streaming; binaries by digest in the site store |
| 10. Required check | hosted verdict queue 1.6 / 24.3 min, then `Required` 1.9 / 21.4 min | ≤ 1 s after the last receipt | One App-written status, `Velnor / Verified` |
| 11. Merge-queue entry and run | green → enqueue 7.5 min (0 of 40 auto-merge); enqueue → group 19 s; group 19.9 / 84.1 min | ≤ 30 s (≈ 25 s) | Auto-merge; an exact-tree lookup for about 60–75% of groups (16% today), else per obligation |
| 12. Merge | 26 s | 26 s | GitHub's queue (decision 7) |
| **Verdict / push-to-merge** | **26.7 / 92.3 min; 85 / 457 min [M, proxy]** | **≤ 90 s / ≤ 4 min; ≤ 3 / ≤ 8 min (≈ 35 s; ≈ 85 s)** | |

**What GitHub still costs:** delivery 1–2 s, and the measured medians of 19 s to a group and 26 s
to merge [M], which include the queue's own load and are not a proven floor. The rest is the longest
changed test and one compile of the change.

## 4. The duplicate-work audit

Every repeated operation across all stages [M, the evidence pack and the panel's audits of
`.github/workflows/*`, `ci-helper/` and `generator/`]: **E** eliminate, **R** reuse or promote by
content, **K** keep deliberately.

| # | Operation | Runs in | Same input | Today | v3 | Mechanism |
|---|---|---|---|---|---|---|
| 1 | Runs for unrequested pushes (draft, unlabelled, ready for review) | PR | — | 45% of PR runs cancelled; 4.77 PR runs per merged PR | E | Label flow (section 8.2) |
| 2 | Re-runs on an unchanged head (`edited`, reopen) | PR | yes | 18.3% of PR runs, 52.9 runner-min each | E | No trigger; exact-tree verdict |
| 3 | Rebase-only push with the same tree | PR | yes | a full run | R; C5 obligations still run | Exact-tree record |
| 4 | Push N, then N+1 | PR | partly | heavy jobs in 97.4% of runs; 60 s / 626 s stall | R unchanged keys | In-flight join; cancel without waiting |
| 5 | Concurrent, stacked and identical PRs | PR | partly | no cross-PR reuse | R | Content keys; candidate records (section 6.2) |
| 6 | Agent pre-push test, then CI | pre-push, PR | yes | unmeasured | R only through `velnor verify` in a sealed Linux sandbox | Records |
| 7 | Plan job, Ledger CLI build and 31 downloads, `Required` aggregation | every run | yes | Plan 0.9 min plus 5.5 min hosted queue; Ledger CLI 22 s + 31 × 3.2 s; `Required verdict` 49 s | E | Released planner and helper in the tool store; App status |
| 8 | Checkout, toolchain, mise, action downloads | every job (31–39 per run) | yes | 14–18 s per job; 7.5–8 min per run | E | Node mirror, read-only stores, native actions |
| 9 | Crate fetch | every Rust job | yes | 4–5 s per job | R once per lockfile | Offline crate store |
| 10 | Third-party crate compile | every Rust job | yes | 2,106 builds of 475 crates per run (4.4×; another count gives 2,260 of 463, 4.9×); 6.3–21.7 GB written per job and discarded | R once per lockfile, toolchain and profile | Warm generation |
| 11 | Workspace crates in several configurations | runner-host default and `local-mode` | partly | 404–407 crates twice | R once per closure key | Crate split; `velnor-runner-local` extracted |
| 12 | Clippy, doc, check-alone | every run | different outputs | clippy 36–75 s, doc 30–65 s per job | K as parallel lanes; R per key | One target lane per check kind |
| 13 | `-j2` caps, the 89 s image-publication fixture, the CLI built ≥ 9 times per event | several jobs | yes | caps cost 4.5 of 8.5 min | E / R once | Caps removed; fixture in the warm build |
| 14 | Tests across PR, merge group and push | three runs | yes | about 25 min per run | R by record; K new keys twice, in parallel | Two witnesses |
| 15 | Full CI on the merge group | merge group | when the tree equals the PR's | 64.6 runner-min; equal trees 16% today | R exact tree (60–75% with auto-enqueue against the queue tip), else per obligation | Records; speculation |
| 16 | Full CI on the push to `main` | push | yes (same SHA in 120 of 159) | 62.0 runner-min; 8.1% false failures | E, after A1 replaces it | Audit A1 |
| 17 | State seal producers and cache prune | `main` push | yes | 683 s per push; 3.49 GB of GitHub cache, 0 of 3,636 restores | E | Generations and storage tiers |
| 18 | Source qualification on every PR event | PR | yes | 1,007 s per event, about 80 runner-min per merged PR | R once per key, in the queue | Obligations |
| 19 | Job image candidate on every push | every push | yes unless images changed | 200 s; about 20 min per PR | R once per image-input key | Keyed action |
| 20 | Source preview build, tag rebuilds, product release | `main` push, tag | yes | preview 410–534 s per push; tools 190–434 s; product 4,105–6,238 s | R promote by digest | Release promotion (section 8.4) |
| 21 | macOS and Darwin legs | `main` push, release | — | 1,063–1,406 s per push | E (paused, decision 9) | — |
| 22 | `VELNOR_COMMIT` forces both tools to rebuild per commit | every merge | no, by construction | two tool builds per merge | E once the stamp is proven | The group SHA in the key today; stamping after link is a lab item |
| 23 | Per-node rebuilds of `main` by warmers | after every merge | yes | about 120 incremental builds an hour | R: the landed group's build output is the next generation, trusted when A1 gives equal digests | Generations by content |
| 24 | GitHub cache save and restore | many | — | 1.92 GB restore 44–61 s | E from the verification path | Storage tiers (section 7) |
| 25 | A Docker daemon in every pod | every job | — | 3–4 s per job; 57 probe failures an hour | E | Docker only for the three binaries that need it |
| 26 | Second witness, cold audit A1, conformance pairs, speculation | PR, group, batch | yes | — | K | They buy confidence or wall time; charged below |

**Totals per merged PR** [E; occupied slots from executed steps, for the generator and helper class,
about 2 labelled heads per merged PR, 3 runner-min of changed obligations per witness, 25–40% group
misses]:

| Item | Whole-tree keys, cold second witness | Goal: per-root keys, graduated reuse |
|---|---|---|
| Two witnesses per labelled head | 2 × (16 warm + 22 cold) = 76 | 2 × 2 × 3 = 12 |
| Group misses (twins) | 0.3 × 38 = 11 | 0.3 × 6 = 2 |
| A1: cold build and changed obligations | ≈ 10 | ≈ 10 |
| Product bytes per landed tip (410–534 s preview build today [M]) | ≈ 8 | ≈ 8 |
| C5 fresh work, 2% sampling, speculation, cancelled claims | ≈ 5 | ≈ 5 |
| **Total** | **≈ 110** (warm 16 and cold 22 per full tree are [E]) | **≈ 37; goal ≤ 50 [T]** |

The runner class costs more; the stretch's ≤ 20 needs product bytes once per closure (stamp after
link). Per release: 68–104 runner-min (product) or 3.2–7.2 (tools) → promotion, zero compiles.

## 5. Architecture

### 5.1 The owner's decisions

Decisions 1–24 bind this design. Where each lands: one interface for Velnor Native (the default)
and GitHub Official, with the official runner also the permanent reference executor (1, 2, 19, 20:
sections 5.3, 5.7); latest APIs only (3: 5.3); no Node in Velnor Native and a closed action set (4:
5.6); review before any reuse (5, 14, 17: 5.10); experimental dependencies judged on evidence (6:
7.3); GitHub stays the platform (7: 8.3); no forks (8: 5.8); macOS paused (9: 8.4); build once,
promote (10: 4, 6, 8.4); correctness first with a four-part soundness case (11: 1.2, 6.4); the label
flow, and a green latest commit to merge (12: 8.2); one core with capability-declaring platforms
(13, 21: 5.4); agent delivery sized by the real constraint, per-job-type migration with deletion
(15, 16, 18: 11); storage, `actions/cache` and mirrors (22–24: 7).

### 5.2 Components

All new code is Rust in a new Cargo root, `executor/`, with its own lockfile, so the engine crates
can be deleted without touching it; crossing a crate's line budget stops work for review (I012).

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| App and label flow | `velnor-control` | one fenced writer per repository | Webhooks, the label state machine, the status writer, the reconciler, `merge_group` handling | 6k lines |
| Dispatcher | `velnor-dispatch` | one service per site | Scale-set sessions, broker long-poll, JIT identities and their credentials; placement before acquisition; lease journal | 6k lines |
| Executor interface | `velnor-executor` | library | The `Executor` trait, the work model, identity | 1k lines |
| Velnor Native | `velnor-exec` | in the node agent | Contexts and expressions, steps, composites, background and post steps, commands and env files, masking, cancellation, timeouts; run service and Results | 20k lines |
| GitHub Official | `velnor-official` | in the node agent | Drives the latest official runner for acquired jobs and for obligations (section 5.3) | 4k lines |
| Native actions | `velnor-native-actions` | in Velnor Native | The closed action set (section 5.6) | 10k lines |
| Platform | `velnor-platform`, `-k8s`, `-docker` | library | The platform trait, capabilities and the two backends (section 5.4) | 2k + 6k + 4k lines |
| Storage and caches | `velnor-storage`, `velnor-cache` | library and site service | Blob stores, tiers and routes; the authoritative cache index (section 7) | 4k + 6k lines |
| Ledger | `velnor-ledger` | library and site service | Plans, keys, verification records, `ensure`, joins, promotion, audits, epochs | 7k lines |
| Node agent | `velnor-node` | DaemonSet, or a host daemon under Docker | Warm pools, observation, the read audit, receipt signing, node stores, the builder, cleanup by proof | 13k lines |
| Conformance | `velnor-conformance` | velnor-lab | The differential harness (section 5.7) | 5k lines |
| Helper and generator | `velnor-ci`, `velnor-gen` | existing | Plan, obligation inventory, obligation commands, release gate, workflow shape | existing |

### 5.3 Two executors behind one interface

```rust
pub trait Executor: Send + Sync {
    /// Kind (Native | Official), version and image digest. Part of every result key.
    fn identity(&self) -> ExecutorId;
    async fn run(&self, work: Work, env: Env, role: Role, io: JobIo) -> Result<Outcome>;
}
pub enum Work { Job(AcquiredJob), Obligation(ObligationSpec) } // GitHub's job, or the planner's
pub enum Role { Production, Reference } // Reference: conformance, shadow, diff; never a verdict
```

- **Velnor Native** is the default and the recommendation: steps run natively, the closed action set
  without Node, next to the node's mirror, stores and generations.
- **GitHub Official** runs the latest official runner (`Runner.Worker`, JavaScript actions on Node)
  in the same sandboxes, with the same platforms, pools, label flow, status, ledger and caches;
  Velnor's dispatcher drives both, ARC neither. For an *acquired job* (release, image, scheduled)
  Velnor hands the Worker GitHub's job message; the Worker completes against GitHub's run service,
  as v2.338.2 does (`JobRunner.cs` `CompleteJobAsync`), and the dispatcher owns the lease. An
  *obligation* has no GitHub job: Velnor synthesises a job message whose steps are the released
  command and serves the run-service, Results and cache endpoints it names from the node agent, so
  the Worker never reaches GitHub. The command runs under `velnor-ci obligation exec`, which reports
  exits and the test inventory to the agent over a channel the sandbox cannot write, so the receipt
  comes from observation, not the Worker's completion. This mode passes its own conformance cases
  before `.velnor/` may select Official for an obligation kind; until then the plan refuses that
  selection.
- **Choice and identity.** `.velnor/` names the executor per job type or obligation kind, or
  globally; Velnor never switches on failure. The identity is in every key, so a deliberate switch
  re-runs verification and mints separate records, and can never produce a false green.
- **Reference role** (decision 19): the conformance gate, sampled shadow runs and "run both ways and
  diff", writing only to an isolated reference namespace.
- **APIs** (decision 3), as the latest release implements them, including broker-session ownership
  on deletion and the connectivity probe: scale-set sessions, broker long-poll, JIT configuration,
  run-service `acquirejob`, `renewjob`, `completejob`, Results twirp, cache service v2, artifacts v4,
  OIDC. Out: the AES legacy session, legacy pipelines and distributed task, cache v1, artifacts v3.
- **Expressions (owner question D4).** At v2.338.2 the Worker returns the legacy `Expressions2`
  result on every path, even with the new evaluator in compare mode. The oracle runs in compare mode
  and Velnor refuses any job on which GitHub's two evaluators differ, so on every accepted job both
  give the runner's result. Option A ships velnor-legacy's port of `Expressions2` (3 agent-hours;
  conflicts with decision 3); option B ports `src/Sdk/Expressions` and `src/Sdk/WorkflowParser`
  (about 28; ruling Q4). Until the owner answers, the default is A, matching what the latest runner
  returns; B is built in parallel, and only the chosen one ships (decision 18).

**Speed per job type** [the difference is unmeasured; paired runs on the same backend, tool store,
generation and warm or cold mix measure it, section 5.7]. Official adds Worker start and set-up (4–5
s per job [M, ARC era]), Node start for each JavaScript action (87–147 ms [M]), and GitHub's cache
until the cache endpoint is qualified (section 7.2). Per-job tool installs and the 44–61 s cache
restore measured under ARC are not charged to it: both executors share the tool store and
generations. The paired runs itemise Worker and Node costs apart from cache routing and set-up, with
Results, artifacts and OIDC behaviour kept equal.

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
    pub cpus: u32, pub memory: Bytes, pub dirs: Vec<DirSpec>, pub network: NetPolicy, pub lifetime: Lifetime, pub arch: Arch, pub os: Os,
}
pub struct Capabilities {
    pub kernel: Kernel,             // Shared | Separate
    pub dir_entry: Vec<DirEntry>,   // BindOrReflink | VirtioFs | DiskImage
    pub shared_page_cache: bool, pub snapshot_restore: bool, pub warm_pool: bool,
    pub docker: Docker,             // None | PrivateDaemonPrivileged | PrivateDaemonRootless
    pub read_audit: bool,           // file-access tracing for key enrolment (section 6.1)
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
| Docker for tests | a private privileged daemon per request, in a sidecar on tainted nodes | a private daemon container per request, or refused; never the host daemon's sibling containers |
| Read audit | yes, from the node agent | no: keys stay whole-tree |
| Network policy, cgroup kill, CPU pinning | yes, yes, optional | partial, container kill, no |

The core branches on capabilities, never backend names; a missing one selects a slower, correct
path (copy for reflink, refusal of the Docker class, whole-tree keys without a read audit, container
kill for `cgroup.kill`). Each Docker-class request gets its own daemon, and a backend passes the
private-daemon cases (nothing of one request visible to the next; process, mount and network
proofs) or refuses the class. Lifecycle, recovery, tri-state proofs (`Unknown` is never absent) and
teardown are in the contract; conformance runs per backend. A microVM or Apple Containerization
backend declares `kernel: Separate`, `dir_entry: [VirtioFs]` or `[DiskImage]` and
`snapshot_restore`: a new implementation of this trait and nothing else.

### 5.5 The job path

1. **Trigger.** The label flow (section 8.2) decides whether a head is verified. A labelled head's
   webhook reaches `velnor-control` in 1–2 s; `ensure(tree, plan)` fetches the commit into the node
   mirror, computes the tree with `git merge-tree` (the head merged into the queue tip, or the group's
   own commit) and runs the released planner for the obligation vector and its keys.
2. **Resolve and execute.** Each obligation's verification record is satisfied (every witness slot
   holds a passing, current, unrevoked receipt), in flight (join it) or open (schedule its missing
   witnesses); C5 obligations always schedule. Obligations are plain commands (`cargo`, `cargo
   clippy`, `cargo doc`, `cargo nextest`) run through `velnor-ci` in a warm sandbox over a reflink
   clone of a verified generation; the agent observes them and signs a receipt per witness.
3. **Actions jobs** (release, image, scheduled): the dispatcher holds every JIT identity, picks the
   node (warm state, site, class, the reserved merge path), then acquires and hands the job to the
   configured executor.
4. **Report.** The repository's fenced writer posts `Velnor / Verified` when every record of the
   head's vector is satisfied under the active epoch (section 8.2).

### 5.6 Native actions

The closed set comes from every `uses:` in `.github/` and velnor-gen's samples [C]. Velnor Native
refuses a job before its first step when an action, SHA or input is outside the qualified set;
`velnor-gen` emits only qualified actions and SHAs, enforced by a generator test.

| Action | Uses | Native implementation | Node-local state |
|---|---|---|---|
| `actions/checkout` | 61 | The action's inputs and outputs, including `sparse-checkout` (44 uses) and `fetch-depth`; a reflink clone of a sealed tree with only the changed files rewritten, then the full tree checked (section 7.4) | The node's bare mirror |
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
compares; velnor-new's `qualification.yml` is its seed. The oracle side is `velnor-official` in the
reference role, the same maintained adapter production may select.

- **Corpus.** At least 240 semantics, 65 fault and 30 refusal cases beside replayed jobs:
  velnor-new's full job set under every event at pinned commits, with recorded failures,
  cancellations and re-runs; every expression function and coercion pair, every context including
  masking, status functions × job states, `continue-on-error`, outputs and `needs`, env files,
  workflow commands, post and background steps, cancellation at every step boundary, timeouts; each
  native action across its inputs, including a cache or artifact saved by one executor and restored
  by the other; both executors' obligation mode; the label flow's states.
- **Normalisation.** Ids, times, runner names, versions and paths, tokens, the official runner's own
  set-up lines, ANSI codes, and output interleaving (compared per stream); contexts as JSON.
- **Pass criteria.** Exact equality, after normalisation, of step outcomes, outputs, contexts and
  env, expression results, annotations and summaries, cache and artifact effects, post-step order,
  and cancellation and timeout outcomes; other differences need an owner-signed allow-list entry.
  The gate is 100% per backend with zero secret leaks, and the harness must kill at least 95% of
  about 30 executor mutants before its green counts.
- **When.** On every change to an executor, a native action or the protocol code; nightly; within 14
  days of each official release against its tag (v2.338.2, `d1eb12d4`, today); as sampled shadow
  runs of real jobs.
- **Drift.** Deadlines per version from the deprecations endpoint (2.337.0 stops on 2026-12-25,
  2.338.0 on 2027-01-04), with an alarm 21 days ahead and a page on a critical update; unknown
  protocol fields alerted, never dropped; a protocol surface diff per release; release binaries
  scanned for banned legacy endpoints.
- **Evaluators.** The oracle runs with `ACTIONS_RUNNER_COMPARE_WORKFLOW_PARSER=true`; a job on which
  GitHub's two evaluators differ is refused until understood, and a fuzzer compares both.
- **Canaries.** A production-path canary every 10 min per site (checkout, cache and artifact round
  trips, OIDC, an attestation, a summary, a background step, a cancel). Permanent canary PRs that
  must stay unmergeable (no status; the context from another source; a required Actions job with
  `if: false`) are probed every 10 min; any `mergeable_state` but `blocked` pages the owner and
  stops `success` writes.
- **Per-job-type gate** (decision 16): section 10.1.

### 5.8 Isolation and trust

No PR comes from a fork (decision 8): isolation is for trusted authors whose code may be buggy. PR
results never become `main`'s trusted caches or receipts without verification, on every tier.

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | small jobs (lints, plan) | warm sandbox, 1 CPU | GitHub API | read-only tools |
| build | PR, merge-group and `main` jobs and their obligations | warm sandbox: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account token; obligations also without `.git`, a GitHub token or the job's environment | allow-list: crates mirror, site storage | read-only trusted; per-obligation clones; never a shared writable path |
| docker | only the three test binaries that need a daemon (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests), as their own obligations | a private daemon per request, never shared and never the host's (section 5.4); rootless and microVM daemons are lab D experiments; a skipped or missing Docker test fails the obligation | as build, plus a registry mirror | the build's test binaries, cloned |
| builder | `main` generations, no GitHub job | unprivileged | crates mirror, storage | writes the trusted scope |

Runner credentials stay in the dispatcher and never enter a sandbox; the App's private key exists
only in the control plane, and agents act under an identity without repository administration.

### 5.9 Failure handling, releases and rollback

- **Journal discipline:** persist the intent, make the call, record the outcome; unknown is never
  absent. The executor journals plan id, job id, run-service URL, job token and environment id
  before acting, runs no credential-bearing step before its first renewal succeeds, renews every 60 s
  from `lockedUntil` (a conservative monotonic deadline) with a loop written fresh (the upstream
  overload dereferences a null request on failure, `JobDispatcher.cs:748-852`), and after a crash
  resumes or fails the job with its own token, never twice. The ledger persists an execution's
  intent (key, node, witness slot, epoch) before a sandbox starts; results land by compare-and-swap.
- **Fault injection** before each move: crashes around every remote call, journal write and step
  boundary, a lost lease, duplicate and reordered webhooks, a ledger partition, a lost node, the
  control plane and a site. **No fallback** (decision 18): a divergence is fixed forward; the
  cluster runs only released versions.
- **Recovery from a bad Velnor release** (D15), with no merge outside the gate: (1) redeploy the
  previous immutable image digest (a deploy, not a code path); the fix PR is verified by that
  release like any other. (2) If no earlier image can verify, the owner-pre-approved **break-glass
  verifier** runs: a minimal released binary that takes the status writer's fence, executes the full
  inventory cold on one node with the reference-qualified executor, and writes `Velnor / Verified`
  only if every obligation passes, with an audit record and a page. It is a real green on the exact
  tree; `bypass_actors` stays empty, and no procedure contains an admin merge.
- **Control plane:** k3s has one server today (§B.1.5); a whole-cluster dependency waits for three
  servers across failure domains and a third-site witness for any shared leader, such as the
  status writer's fence.

### 5.10 What is reused, after review

Nothing is reused by default (decisions 14, 17): each component is reviewed, and kept only when it
is well designed for this architecture.

| Component | Verdict | Why |
|---|---|---|
| velnor-legacy's broker, JIT and run-service client, Results client | Port and refactor | The wire behaviour is right and tested; legacy-protocol paths are stripped and the god files (`executor.rs` 37,028 lines, `runner.rs` 36,759) are split |
| velnor-legacy's expression engine (3,565 lines, from actions/runner v2.337.0) | Option A of D4 (production until the owner answers); the harness's reference either way | Its results are what v2.338.2 returns on every path; option B ports the new evaluator (section 5.3) |
| velnor-legacy's checkout, artifacts, attest, App-token and mise adapters | Port and refactor | Behaviour and tests are sound; they move onto the node stores |
| velnor-legacy's `gha_cache.rs` (17,140 lines, v1 and v2 server) | Redesign; its v2 message types reviewed for the Official cache endpoint | Cache v1 is out; the cache index is new (section 7.2) |
| velnor-legacy's Docker lease and JS sidecars | Drop | The socket proxy caused hangs; there is no Node in Velnor Native |
| velnor-new's `velnor-runner-core` (lifecycle, `CleanupProof`, `OwnedIds`) | Port and refactor | The tri-state "unknown is never absent" rule is right; its two open slot defects (a short id accepted as covering, a live slot replaced) become tests |
| velnor-new's `velnor-runner-github` (scale-set wire types) | Port and refactor, pending review | Pure types; it carries official-runner lifecycle states |
| velnor-new's `velnor-host` journal (Turso) and launch state machine | Redesign | Bound to the official-runner pair; `Journal::open` drives the test floor. The discipline is kept |
| velnor-new's Docker client and spec audit | Port into the Docker backend, pending review | The audit rules (no host mounts, no outer socket) are valuable |
| The `Runtime` trait proposed for velnor-new's runner | Port its observation model into the platform trait | `Present`/`AbsentProven`/`Unknown` and Pod-UID proofs are right; the official-runner pair methods are dropped |
| `velnor-runner-host`'s official-runner wrapper | Reviewed for `velnor-official`; redesigned if it fails | Decisions 19 and 20 make the official runner a maintained executor with production and reference roles |
| `velnor-ci`, `velnor-gen` | Port and refactor | Generators of record; `velnor-ci release-gate` becomes the one release authority (section 8.4), and velnor-gen's hard-coded hosted release runners (`release.rs:29`, `release_binaries.rs:44-46`) go in phase 0 |
| The engine's 39-job `ci.yml` and its embedded product release gate | Redesign | Replaced by velnor-gen's workflow and `velnor-ci release-gate` |

## 6. The ledger and reuse

### 6.1 Keys

- **Definition digest:** command, configuration and features, profile, nextest profile and
  overrides, helper version, toolchain, image and tool-store digests, the **executor identity**
  (decision 20), and the **execution fingerprint**: kernel release, architecture, OS, the CPU
  features the toolchain may target, CPU count, sandbox profile digest and tool versions.
- **Inputs:** the Git tree of the whole repository (today's checkout is `/*` without `/docs/**`,
  `ci.yml:97`); the exact package selection and features (a test passed under `-p a` and failed
  under `--workspace` [M]); the execution manifest (allowed environment and values, path prefix,
  modes and symlink targets).
- **Narrower keys need an enrolled contract.** A per-root or per-package key is used only after: (1)
  the read audit has run in shadow on that obligation; (2) a reviewed dependency contract in the
  trusted inventory names every path the obligation may observe, directory contents and absent paths
  included, with deterministic inputs enforced; (3) every execution, hit or sample, runs on the
  intact tree with the audit enforcing the contract, so one observation outside it fails the witness
  and revokes the enrolment; (4) a shadow case where a new sibling file outside the contract changes
  the expected verdict is caught; (5) about 3,000 clean comparisons for its kind. The trace proposes
  candidates and revokes enrolment; it never licenses a hit. In velnor-new 79 of 86 files using
  `CARGO_MANIFEST_DIR` climb out of their package and 70 test files list directories, so most test
  keys stay whole-tree.
- **Time and randomness.** No test manifest declares proptest, quickcheck or rand [M]. The trusted
  planner classes an obligation as C5 when its sources or workspace closure read the wall clock,
  randomness, the network or the event (11 test files read the clock today); nothing is reused on a
  time-to-live.
- **The obligation vector.** For every subject SHA the planner derives every obligation and key; the
  ledger resolves the vector against current, unrevoked records. An "instant" verdict (rebase-only
  push, identical group tree) is reported only for a vector with zero C5 obligations and zero
  misses; otherwise the C5 set and the misses run.

### 6.2 Verification records, witnesses, joins and scopes

- **A verification record per key**, with mandatory witness slots: two for a new key, in a PR or a
  group alike. Each slot binds one receipt from a distinct node, attempt and order seed, over a
  different verified generation or cold, bound to the immutable inputs and the active epoch. The
  record passes only when every slot holds a passing receipt, and a receipt fills one slot of one
  record. Audit re-execution (sampling, A1, shadow) writes to an audit stream, never to a slot.
- **In-flight join:** a request joins the record its key needs, not one execution; a cancelled
  request drops its claim, and execution continues while another claimant needs it. Shared work
  takes the highest live claimant's priority and moves to reserved capacity if it cannot resume
  promptly (section 9).
- **Hedging** is for preparation only (fetching, cloning, compiling a closure whose output digest
  must then equal). A raced copy of a verification attempt is an extra witness: the obligation
  passes only when every launched copy passes, and a cancelled, timed-out or lost copy fails it.
  Only an infrastructure abort the platform proves (the node `AbsentProven`, a preemption by policy)
  lets a fresh attempt replace a lost one, and that one must pass. Hedging can shorten the time to a
  failure, never to a green.
- **Scopes.** A PR's execution writes `candidate` records, reusable by any request with the same
  key: later pushes, other and stacked PRs, merge groups. A record becomes `trusted` only when its
  tree is on `main` and A1 has passed for that commit. A build that can mint a reusable receipt reads
  only the `trusted` scope and its own outputs.

### 6.3 What a request executes

| Situation | What runs |
|---|---|
| Rebase-only push, same tree; a re-run with no code change | the C5 obligations; the rest resolves from records within seconds |
| Push N+1 while N runs | N is cancelled at once (no stall); N+1 joins N's records its keys still need and runs the rest |
| Stacked or concurrent PRs | each key's record fills once; others join or reuse |
| Merge group | the whole vector: records with equal keys are reused (about 60–75% of groups match the PR's whole tree with auto-enqueue against the queue tip; 16% today), C5 and misses run with two witnesses |
| Push to `main` | promotion of the group's records and bytes; audit A1, the third execution, reported by the push run's Actions `Required` job |
| Release tag | promotion of verified bytes; zero compiles (section 8.4) |

### 6.4 Soundness cases

Every reuse or skip, with its four parts (decision 11): why the key is complete, what fails if it is
not, how that is detected, and the automatic fallback.

| Reuse | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| Whole-tree record (rebase-only push, re-run, identical group tree, same-SHA push) | The tree is every repository input; definition, fingerprint and executor equal; C5 runs anyway | A non-tree input changed: a tool, the kernel, an undeclared clock read | Execution fingerprint in the key; C5 classification; A1; A2 | Revoke by epoch; full run |
| Per-root or per-package record | An enrolled, reviewed contract covering every possible observation, enforced by the read audit on every execution | A read the contract missed, on a branch no audited run took | The enforced audit on every execution and sample; the sibling-file canary; A1 | Enrolment revoked; whole-tree key |
| Candidate record reused across pushes and PRs | Same key, Velnor-observed, two independent witnesses | A PR sandbox leaked state into a result | Sampled re-execution; trusted only after A1 | Candidate reuse off for that class |
| `velnor verify` before the push | Same sealed Linux sandbox and key as CI | A developer machine's result | Only Velnor-executed results count; Mac runs never do | Runs in CI |
| Warm compile over a generation | Section 7.3: inputs proven by content against the generation's manifest; every changed, added or removed path either declared by a unit's dep-info or the root builds cold; every unit cargo did not rebuild has dep-info inputs with unchanged blob ids | A stale `OUT_DIR`, build-script or proc-macro output, or an older-time file cargo skips | Until graduation the second witness builds cold and the binary digests must equal; A1's cold digests; the standing canaries of section 7.3 | Retire the generation; build cold |
| `mbx-cache` units for a cold or mismatched node | mbx's per-crate keys | A missed input in mbx's key | A1; the builder's cold rebuilds; digests equal before a unit enters a generation | Server retired; plain cargo |
| `actions/cache` entries | The user's key and version, resolved by the authoritative index with GitHub's scope and order | A user key that omits an input (as on GitHub) | Conformance against the official runner; content hash on every read | Treated as a miss; never used by a receipt-minting obligation |
| Artifacts and test binaries by digest | Content address | Corruption | Hash verified on read | Refetch or rebuild |
| Tool and crate stores | Pinned versions and lockfile checksums | A store edited in place | Read-only mounts (`EROFS`); digests | Rebuild the store |
| Mirror checkout | The staged tree is hashed in full and must equal the requested tree before the first step | A stale, corrupt or concurrently refreshed base | The full check, every time | Fetch the commit from GitHub; rebuild the mirror from the site bundle |
| Release promotion | Closure, release profile, target, tool image; the digest is in the receipt and the attestation | Wrong bytes published | Re-hash and `gh attestation verify` at publish; A1 rebuilds cold and compares | The release stops; nothing is rebuilt silently |

**The read audit** is node-agent tracing of the sandbox's cgroup: opens, stats, `readlink`,
directory listings and failed lookups (`ENOENT`), with paths resolved against the materialised
tree. A backend without it keeps whole-tree keys.

### 6.5 Trust root, publication and audit

- **Keys.** One Ed25519 signing key per node and sandbox class, held by a signer daemon outside every
  sandbox and the agent's general process; receipts go to an append-only log per site; the ledger
  verifies signature, class and log entry before a receipt fills a slot.
- **Publication.** One fenced status writer per repository holds an epoch-numbered lease; until a
  third-site witness exists it runs only in the repository's home site, and the other site executes
  and reports to it. It keeps a durable index of every `success` it published (subject SHA, PR or
  group, epoch), including heads that later lost the label. Before a write it rechecks the request
  generation and the active epoch; after it, it reads the status back and reconciles an ambiguous
  write. The fence refuses a late write from a stale lease.
- **Revocation.** Every record carries its policy epoch. A *planned* change (a reuse class enabled, a
  key version raised) freezes new `success` writes, lets in-flight groups finish under the old
  epoch, activates the new one and re-resolves open heads. An *emergency* revocation (a signer, a
  reuse class, a reused pass whose cold run fails) stops writes at once, writes `failure` on every
  indexed success that is still an open head or queued group under the revoked epoch (GitHub ejects a
  group whose required status fails), re-verifies them, and lists every commit that landed in the
  propagation window for an immediate cold audit and, on failure, a revert.
- **Audit A1.** For each confirmed update of `main`, the ledger journals an audit request for every
  new first-parent commit (a group of two lands two) and reconciles gaps from Git history, so an
  intermediate commit with no push run is still audited. Each is rebuilt cold and its changed
  obligations re-run, comparing results and binary digests with the records it landed on; the result
  is bound to the commit and that proof set. One SLO: within 2 h of landing at P90 [T]. A commit
  unaudited at 6 h pages; the trusted scope does not advance past it, its release waits, and
  warm-compile reuse suspends until it clears. The push run's `Required` job on the tip reports A1
  for the release gate. A class with about 3,000 clean comparisons drops to 2% sampling.
- **Audit A2** runs nightly with the clock shifted 8 and 30 days ahead through a shim. **Degraded
  mode:** with the ledger unreachable nothing is satisfied by lookup; obligations run fresh.

## 7. Storage and caches

### 7.1 One storage abstraction, configurable tiers

```rust
pub trait BlobStore: Send + Sync {
    fn capabilities(&self) -> &StorageCaps; // reflink, server_side_copy, multipart, conditional_writes, replication
    async fn get(&self, key: &Key, range: Option<ByteRange>) -> Result<Body>;
    async fn put_if_absent(&self, key: &Key, body: Body) -> Result<PutOutcome>; // content-addressed keys
    async fn copy(&self, from: &Key, to: &Key) -> Result<()>;                   // server-side when the backend can
    async fn list(&self, prefix: &Prefix) -> Result<Vec<Key>>;
    async fn delete(&self, key: &Key) -> Result<()>;
}
```

- **Backends:** the local filesystem (node NVMe, or a directory on a Mac); RustFS single-node, or an
  erasure-coded cluster per site with optional site replication; any S3-compatible store (MinIO, AWS
  S3, R2) through the same S3 backend.
- **Tiers and routes.** Tiers compose in configuration (`local`, `local → site`, `local → site →
  cross-site`), each naming a backend; each data class routes separately: `actions/cache`,
  artifacts v4, promoted release artifacts, the mbx-cache store, generation shipping.
- **Invariants for every configuration:** `actions/cache` semantics never change; correctness is
  enforced by Velnor's cache layer, not the backend (scoping, PR writes never reaching `main` scope,
  immutable keys, content verification on read); a misconfiguration can only make things slower.
  Blob stores hold only content-addressed bytes; every name that maps to them lives in Velnor's
  authoritative index (section 7.2). Backends declare capabilities and the layer picks the fastest
  correct path.

### 7.2 Native `actions/cache`

Users write plain `actions/cache` (`key`, `restore-keys`, `path`, `enableCrossOsArchive`, the
`cache-hit` and `cache-matched-key` outputs) with GitHub's exact semantics: a run reads its own ref,
its base and the default branch, writes its own ref, and keys are immutable. Conformance proves it
against the official runner.

- **One authority for names.** Each entry is `repository × scope × key × version` (GitHub's version
  hashes paths, compression and `enableCrossOsArchive`), committed once, insert-if-absent, by the
  repository's cache index, a serialised writer in its home site; it names a content-addressed
  manifest of chunks. The other site resolves names through the home index; if it is unreachable, a
  lookup is a miss and a save is dropped.
- **Scope and order.** The scope comes from the authenticated run context (the job token's ref, base
  ref and event), the PR merge ref included. Lookup follows GitHub's order (exact key, a prefix of
  the key, then each restore key, in the run's ref; then the same in the base and default branch)
  and resolves a manifest before any blob tier is read, so an L0 prefix hit never masks an exact
  entry.
- **Tiers:** L0, node NVMe, holds the bytes of resolved manifests, unpacked once per node and
  reflink-cloned per job; L1, the site's RustFS cluster; L2, replication DE↔FI in the background. A
  lagging replica, a missing blob or a hash mismatch is a miss. GitHub's cache is off the hot path.
- **Prefetch:** the dispatcher resolves keys whose inputs are known before the job (for example
  `hashFiles('Cargo.lock')` at the subject commit) and warms L0; the step resolves again and uses the
  prefetched bytes only if it names the same manifest.
- **Receipt-minting obligations never restore `actions/cache`**; it serves Actions jobs. Chunks
  make saves deduplicated and incremental.
- **Tests:** concurrent saves of one key, an L0 stale prefix beside an exact L1 entry, PR-to-`main`
  isolation, a disconnected site.
- **Open:** whether GitHub Official can use a Velnor endpoint compatible with cache service v2
  (v2.338.2 `NodeScriptActionHandler.cs:54-83` and the pinned toolkit name the route, the runtime
  permissions and the shared Results URL). Until verified, Official uses GitHub's cache for acquired
  jobs: correct, slower.

### 7.3 Compile reuse

**Measured** [M, `experiments/mbx-cache-cluster.md`, the runner workspace of 357 crates at
`/w/velnor-new`, idle nodes]: plain cargo cold 92 s; a reflink clone of the warm target 0.25 s and
2.5 MB, then 0.31 s and 0 crates to build; today's ext4 copy 8.6 s and 10.75 GiB; mbx on a second
node from a warm `mbx-cache` 39.9 s; a lockfile change 67.4 s plain, 88.4 s with mbx on the same
node, 43.4 s on another node once one has built it.

**The hot path is plain cargo over a reflink clone of a verified generation** at a fixed checkout
path, with inputs proven by content, independently of cargo's freshness:

1. A generation G is the target directory of a trusted build, sealed with its **input manifest**
   M_G (every path's blob id, mode and materialised time) and every unit's dep-info.
2. For a request tree T, the changed set Δ is every path whose blob or mode differs from M_G, plus
   every path added or removed. The base is G's manifest, never the checkout's base tree.
3. If a path in Δ is outside every unit's declared inputs (dep-info, `rerun-if-changed`, the package
   directory of a script without directives), every workspace unit of that Cargo root builds cold;
   third-party units, whose inputs are the checksummed crate, features, profile, toolchain and
   declared environment, are reused.
4. The materialiser stamps times: unchanged paths keep M_G's times; every path in Δ gets a time
   later than G's newest output.
5. After cargo runs, each dep-info input of every unit it did not rebuild is checked against M_G by
   blob id (`velnor-ci` `io/audit/dep_info.rs`); a mismatch fails the build and retires G.
6. Toolchain, profile, environment and fingerprint are in G's key; another one means another
   generation or a cold build.

- **Witnesses.** Until compile reuse graduates (about 3,000 clean warm-against-cold digest
  comparisons per Cargo root, from production's cold witnesses and lab B2 replay), the second
  witness of every obligation builds cold and both must report equal binary digests. Afterwards the
  second witness runs over a different verified generation on another node, and 2% build cold. A
  unit that is not reproducible at the fixed path (lab C06) keeps its dependants' obligations cold
  until it is fixed.
- **Generations.** The landed group's output becomes the next generation only after A1's cold
  rebuild gives equal digests; nothing unqualified is ever a base.
- **No transport on the verdict path by default:** a node holding the generation, or cold. mbx with
  a per-site `mbx-cache` fills nodes in the background while it saves ≥ 20 runner-min a day or ≥ 10%
  of builds would start cold (N54). Lab B3 measures waiting for a warm slot against fetching
  verified state and starting cold, under load; a bounded transfer joins the verdict path only where
  measured faster at the same provenance bar.
- **Variants.** Overlayfs and compressed layers are lab candidates; no hard-linked `deps/` base;
  `-Z checksum-freshness` is nightly-only; `CARGO_INCREMENTAL=0` on every receipt or shipped build
  (lab B4); blaze rejected (it built none of the five workspaces and missed a clippy `deny` [M]).
- **Standing canaries** (lab B2, then hourly): the Q16 reproduction (a source file changed to a
  failing value and stamped older than the outputs must rebuild and fail); a build script reading an
  undeclared file that then changes; a changed, a removed and a newly present input; a proc macro
  reading a file.

### 7.4 Node-local mirrors

Each node keeps a bare mirror of every configured repository (decision 24), covering PR head refs,
merge-group refs and tags, fetched on push webhooks with a periodic catch-up. New or wiped nodes
seed from a git bundle in the site store, not from GitHub. Mirrors are read-only to jobs.

**Native checkout from the mirror,** with a target well under 1 s:

1. Make sure the exact commit is in the mirror, fetching only missing objects in one round trip.
2. Reflink-clone a **sealed base**: a pre-materialised tree recorded with its tree id, read-only and
   never refreshed in place (a refresh makes a new base). Rewrite the paths whose blob or mode
   differs (deletions and symlinks included), each from a hash-verified blob.
3. Before the first step, hash the whole staged tree (contents, modes, symlink targets) and require
   it to equal the requested tree: about 0.1–0.3 s for the 110 MiB tree [E: SHA-1 at about 1 GB/s
   per core, in parallel]. A sparse checkout verifies the selected paths against the tree filtered
   by the same patterns; submodules are verified as gitlinks and fetched only when the job asks,
   from that repository's mirror, verified the same way.
4. Set times by the generation rule (section 7.3). A job that needs `.git` gets the read-only mirror
   through alternates; obligations get no `.git` (C5).

Tested: a stale unchanged file in the base, a mode-only change, a deletion, and a base refresh
concurrent with a clone.

**Every upstream the same way,** so each new version crosses the internet about once per site, and
every mirrored object is pinned and verified before use: the crates.io index and crates by lockfile
checksum (the cluster's unused crates-mirror DaemonSet, §B.4.2), toolchains and mise tools by pinned
digest, container images by digest through a registry mirror per site. Repositories, upstreams,
refresh policy, tier and on or off per platform are configuration; on a Mac the Docker backend may
reuse the developer's clone, read-only.

**What it saves** [E, an upper bound: about 10,000 job records a day from about 50 merges a day (159
green groups in three days [M]) × about 6.5 runs per merge × about 30 job records per run (31–39
[M], skipped jobs included)]:

| Upstream | Today, per job [M] | With node mirrors | Saved per job | Saved per day [E, upper bound] |
|---|---|---|---|---|
| Repository checkout | 6 s median, 8 s p90; 110 MiB from GitHub | ≤ 1 s from the mirror | about 5 s | about 14 runner-hours; about 10,000 GitHub checkouts become one fetch per node per push |
| Crates (index and crates) | 3.5–6.1 s; 435–522 downloads; sources cache 0 of 48 | 0 s while the lockfile is unchanged | about 4.5 s | about 9 runner-hours; about 3.5 million downloads become a few dozen new versions |
| Rust toolchain and mise tools | 14–15 s; toolchain restored 0 of 58 times | 0 s: a read-only store, once per site per version | about 14 s | about 39 runner-hours |
| Container images | pulled per node, and per daemon inside dind | a registry mirror per site | not yet measured | measured when the Docker-class jobs move |

At most about 62 runner-hours a day at today's volume, about a fifth of today's 250–290 [E]; under
v3 far fewer jobs run, but each still saves about 20 s.

### 7.5 Budgets

The last three verified generations per root, then least-recently-used; 40% of a node's data
volume, alert at 70%. FI workers have one 3.5 TB data disk each, DE workers 32–50 TB (§B.1.4). The
runner workspace's target is 10.6 GiB [M]. Every store has garbage collection and a budget;
`mbx-cache` starts at 300 GiB per site.

## 8. Label flow, workflow, merge queue and release

### 8.1 Shape and gate parity

Once the transition of section 8.2 completes, a PR and a merge group have no Actions run: the
obligation graph runs through `ensure` and the App's status judges it. `ci.yml` keeps one always-run
`Required` job on push to `main`, which reports A1 for the pushed tip to the release gate; release,
image and scheduled workflows stay Actions jobs on Velnor. Inside a request, format, lint, docs,
"check alone" and tests run as parallel lanes, one target directory per check kind (cargo locks a
target directory, `docs/hard-decisions.md:56-62`); `cargo-deny` reads a pinned RustSec database.

**The obligation inventory** is the parity contract, not summary text. It is versioned, released
with `velnor-ci`, and maps every check that blocks today (Plan, the lints, every Rust variant, the
named and configuration checks, release prerequisites and the aggregated verdicts; evidence pack
§C.1) to obligations with exact package selection, features and expected test IDs. No PR or group
Actions route is removed until shadow parity holds: the App verifies every PR head and every exact
merge group beside today's CI, failure cases included, with zero unexplained lost obligations or
false greens by job type and event. The parity evidence is its own report, apart from the cutover
PR.

### 8.2 The label flow and the required status

**The gate is one commit status, `Velnor / Verified`, and only `success` passes.** A dedicated GitHub
App (working name `velnor-verify`) writes it, bound by `integration_id`. GitHub applies the branch's
required list to the PR head, to merge or enter the queue, and to each merge group's head, to land.
No Actions job is in the list: a skipped job passes a required check, also through `needs` [docs],
and integration 15368 is every workflow, including one a PR adds or edits [M]. A check run also
passes on `neutral` or `skipped`, so the detail view is a differently named check run, `Velnor /
Details`, that no ruleset lists.

**The App.** Permissions `statuses: write`, `checks: write`, `pull_requests: read`, `contents:
read`, `metadata: read`. Webhooks: `pull_request` (`opened`, `reopened`, `synchronize`, `labeled`,
`unlabeled`, `ready_for_review`, `converted_to_draft`, `closed`), `merge_group` (`checks_requested`,
`destroyed`), `push`, and `check_run` (`rerequested` on `Velnor / Details`).

**The one state machine.** A PR is *armed* when `velnor:verify` was applied while it was ready for
review, has not been removed since, and the PR has not returned to draft. Under owner question D16's
default, a label carried over from draft does not arm the PR at `ready_for_review`; re-applying it
does.

| Event | Velnor's action | `Velnor / Verified` |
|---|---|---|
| `labeled velnor:verify` on a ready PR | Arms; verifies the head merged with the queue tip (with `main` if that conflicts) | `pending`, then `success` or `failure` on the head SHA |
| `labeled` on a draft PR | Records the label; nothing runs | none, which blocks |
| push to an armed PR | Cancels the running request at once and verifies the new head, joining shared records | none on the new SHA until written |
| push to an unarmed or draft PR; `ready_for_review` | Nothing (D16 default) | none, which blocks |
| `unlabeled`, `converted_to_draft`, `closed` | Disarms and cancels; writes nothing; a written verdict stays true for its commit | unchanged |
| `check_run.rerequested` on `Velnor / Details` | Re-runs that subject's failed obligations | `pending`, then the result |
| `merge_group.checks_requested` / `.destroyed` | Verifies the group head, reusing records with equal keys / cancels and drops claims only the group held | on the group's head SHA / unchanged |

- **Why an unverified head is never green.** Only the App satisfies the context; only the fenced
  writer (section 6.5) writes `success`, from a signed ledger verdict naming the subject SHA, tree,
  inventory version and epoch, to that SHA only, never the test-merge commit. A new push has no
  status; absence, `pending`, `failure` and `error` all block.
- **Liveness from state.** Every 10 s the reconciler reads the labelled open PRs with heads and
  draft state (a conditional request; a `304` costs no rate limit), the queue's branches, and, for a
  PR whose arming it does not hold, the issue's label and draft events; it starts what is missing,
  cancels what is unwanted and corrects any status the ledger does not hold. A webhook only cuts the
  delay to about 1 s; lost, duplicate or reordered deliveries converge. Labels are applied by people
  or an App token, never `GITHUB_TOKEN`.
- **PRs at one SHA share a verdict** (Q9); the merge group verifies the merged tree again.
- **No bypass.** `bypass_actors` is empty (an admin bypass carried 7 of 136 merges in three days
  [M]); agents act under an identity that cannot edit the ruleset; no procedure uses an admin merge.
- **Lab A proves it before it gates:** draft → ready with and without a carried label; label removal
  racing a completion; a superseding push; group destruction; duplicate and lost events; a two-site
  partition with a delayed stale write; a read-back showing the App context required on both
  subjects.
- **Transition.** Step 0, now: today's two contexts bound to 15368, `bypass_actors: []`, ALLGREEN,
  depth 2, timeout 90 min, minimum wait 0. Step 1: `velnor-ci release-gate` released and both release
  entry points migrated (section 8.4). Step 2: the App in shadow on every PR head and group. Step 3,
  after lab A and shadow parity: all three contexts required, label flow on. Step 4: the Actions
  contexts dropped and the PR and group Actions gates deleted; the push `Required` stays. Before step
  3 the only label gate is an always-run Actions job with no `if:` or `needs:` that fails without the
  label; it comes from the candidate's workflow, the same authority as today's gate, so it adds no
  hole, and independent authority arrives only with the App.

### 8.3 Merge queue

GitHub's queue stays (decision 7). **ALLGREEN** applies from the first ruleset update: under
HEADGREEN three commits whose own `Required` failed already sit on `main` [M]. Depth is a cost
setting: `max_entries_to_build` stays 2 and the timeout 90 min until a group verdict takes ≤ 5 min at
P90 and paired false failures are ≤ 3%, then 4 and 15 min (at 8.1% false failures depth 4 disturbs
29% of entries). Depth 2 at about 11-min groups lands about 8 merges an hour [E: two groups per 11
min, less 15.5% disturbed], above today's 2.44 entries an hour [M]. Speculation verifies the queue
tip's ordered prefixes and single-ejection bypass trees at batch priority; a real group claiming a
speculative record lifts it to the group's priority. **A group's tree equals the PR-tested tree**
[M, replay of 172 entries] in 16% of groups today; with the PR verdict on `merge-tree(queue tip,
head)` and auto-enqueue, 83% at 1 min, 74% at 2.5, 67% at 5 and 37% at 20; about 60% without
auto-merge. Plan on 60–75%; no verdict depends on the rate. Agents enable auto-merge and stop
catch-up merges.

### 8.4 Release: build once, promote by digest

A release never rebuilds (decision 10). Tests stay in the dev profile [M, `generator` / `ci-helper`,
cargo 1.99.1]: testing the release build costs 320.8 / 365.7 CPU-s per cold build against 191.9 /
282.8 for debug tests plus release bytes built once, shares only 15 of 90 crates with the shipped
binary, and still is not the shipped code, because Cargo ignores `panic` for test targets and the
tools ship as musl binaries with `panic = "abort"`, thin LTO and `strip`. Release bytes are one
obligation keyed by closure, lockfile, toolchain, profile, target and, while the commit id is
compiled in, the commit SHA, first computable in the merge group, whose SHA becomes `main`'s [M]:

- **The tools:** the release PR's group builds them and smoke-tests the shipped bytes in its
  verdict.
- **The product**, released by dispatch on the `main` tip: each group starts the build at batch
  priority under a durable claim keyed by the group SHA. Landing retains the claim; a destroyed
  group drops it unless its SHA landed. A release stays pending until A1 has passed for the SHA and
  qualified bytes exist.

Bytes are qualified once (`--version`, `check-tag`, the post-merge black-box suites), packed with
`SHA256SUMS` and attested once, bound to the approved source, identity, target and digest.

**One release authority.** Today the product workflow embeds its own shell gate
(`schema2_release_eligibility.rs:223-247`, run again by `schema2_release_publish.sh:45-60`), which
checks the current tip and the exact push run and attempt but never the App's status or the ruleset
[C]. Before the ruleset changes, both product entry points (eligibility and the publisher's
recheck) and the tools release call one released `velnor-ci release-gate`. It keeps the source SHA,
workflow authority, run and attempt and current-tip checks, and adds the App's `success` on the SHA
under the active epoch (`--app-check 'Velnor / Verified'=velnor-verify`), an active ruleset with
that context required and empty `bypass_actors`, and exactly one successful push `Required` job
reporting A1 (`--push-check Required`; `gate.rs:84-94,277-288,354-376`). Missing or stale proof, a
wrong source, a skipped audit and a changed ruleset are exercised at both entry points.

A tag runs one job: gate, fetch by digest, re-hash, `gh attestation verify`, publish (idempotent,
read back, crates.io receiving the exact packaged `.crate` bytes): about 15 s [E; timed in the dry
runs] instead of 190–434 s for the tools and 4,105–6,238 s for a product release [M]. Stamping the
commit after link would build the product once per closure instead of once per tip; it is a lab
item. The macOS legs are paused (decision 9); `aarch64-unknown-linux-musl` is cross-compiled and
smoke-run under user-mode qemu.

### 8.5 Tests and the floor

- **The unit of cost is `Journal::open`** (93 call sites): a pre-migrated template opens in ≤ 1 s;
  no test exceeds 10 CPU-seconds alone outside a shrinking list; the 40-, 12-, 10-, 6- and 4-case
  loops become one test per case; `test-threads=1` goes once tests use private temp directories and
  ports; expensive valid prefixes are built once and cloned per case. Behind the loops the longest
  test is still 132 s in actions-cli, 125 s in runner-host and 138 s in local-mode, and one
  receipt-integrity test took 924.8 s [M, 28 job logs].
- **The compile floor:** `velnor-runner-host` (156,822 lines) splits into five or six crates with
  `velnor-runner-local` extracted; the 356 integration-test binaries become one `tests/it` binary
  per package, saving about 340 link passes per cold build.
- **The Git differential stays whole.** `non_inputs_differential.rs` compares the planner, Git's
  sparse checkout and the archive guard for every accepted generated glob (about 45,000 `git`
  processes, a 679 s suite). It stays on the merge gate until those paths are deleted or an
  in-process matcher is qualified against the complete corpus; its time falls first through fixture
  reuse, batching and sharding with the original assertions, measured against the full suite.
- **Dependency `opt-level = 3` holds with assertions on** [M, one test]: 67% less CPU and 74% fewer
  instructions than today's dev profile, with `debug-assertions` and `overflow-checks` costing 5% of
  instructions. Adopted with both checks written out and `turso_core`'s line raised; it changes
  every compile key once, and no target counts the gain until lab B1.
- **Fan-out.** A suite's cases spread longest-first across free threads when its summed time exceeds
  its longest test plus the cost of moving binaries by digest. A copy started to race a slow shard
  is an extra witness under section 6.2's hedging rule.

## 9. The cluster

- **Role.** k3s is the Kubernetes backend's fleet layer: it deploys the dispatchers, node agents,
  site services, RustFS and mirrors, and enforces quotas, priorities and network policy. Velnor
  places work; kube-scheduler places pods.
- **Sizes.** Per worker, 125.5 GiB less 6 GiB for the system, 2 GiB of eviction margin and 8 GiB for
  gate sandboxes leaves 109.5 GiB for idle and running sandboxes together. A build sandbox reserves
  its measured peak RSS plus measured page-cache use, 24 GiB today (the heaviest job peaked at
  15.7–16.8 GiB, §B.2.2), and 16 CPU with no CPU limit, so four fit. Nextest's threads equal the
  class's fixed CPU count, which is in the key; only the compile jobserver widens with free CPUs
  (16–64), which changes no output. A fully read target holds 14.29 GB of page cache per clone,
  which caps a node at six build sandboxes until lab B2.
- **Node settings.** Reservations (3 CPU, 6 GiB), a 2 GiB `memory.available` eviction threshold,
  `podPidsLimit` 16384, site topology labels; work directories on local-path XFS with quotas.
- **Reserved merge path.** Each site keeps one 16-CPU build slot and 2 gate CPUs for merge and gate
  work (about 7.7% of allocatable CPU; at 95% utilisation waiting grows to about 19 times the
  service time). Priorities: merge > gate > PR = other organisations' CI > batch; shared work runs
  at its highest live claimant's priority. Batch runs in `cpu.idle` cgroups and is the only class
  preempted. Freezing pauses CPU and frees no memory, so at the memory watermark restartable batch
  work is terminated and requeued; freezing only pauses CPU work whose reservation stays.
- **Sites.** Each site has its own dispatcher, ledger replica, node agents and RustFS cluster; a
  request executes in one site; DE to FI is 35 ms over WireGuard. A repository's status writer and
  cache index live in its home site (section 6.5).
- **Other organisations.** ChainArgos and jackin keep their ARC scale sets, quotas and stores.
- **Hardware and tuning.** Two more k3s servers precede any whole-cluster dependency; a high-clock
  node (1.3–1.5× serial [E]) is trialled only after the floor fixes; a worker is bought when peak CPU
  stays above 70% after dedupe or the eligible merge wait p90 exceeds 10 s for two weeks. What pays:
  gate jobs on the cluster (−5.8 to −57 min per run, §A T5), honest requests (§A T6), no `-j2` caps,
  XFS, line-table debuginfo (−20 to −27% CPU, I108); other tuning waits for a measured stall.

## 10. Delivery: per-job-type migration, milestones and deletions

Agents do all the work (decision 15). The [delivery plan](../research/velnor-runner-2026-11/delivery-plan.md)
(v2, decisions 1–24 and the rulings) sizes about 800 agent-hours of building and 120 of review,
about 2.8 days for 12 builders, 4 reviewers and an orchestrator, so coding does not set the dates:
velnor-new's CI and queue, reviews, owner decisions, conformance and parity evidence, capacity, API
limits and file conflicts do. Phase 0 comes first: an agent PR's cycle falls from 94 to about 34 min
at P50 [E]. Its first ten items are listed there.

### 10.1 Per-job-type migration

A job type moves (nothing moves back, decisions 16, 18) when its corpus cases pass at 100% on
Kubernetes, every mutant is caught, N replayed pairs show no unexplained difference (bound 3/N at
95%), every step made to fail is red as on the oracle, and P90 start latency is no worse than ARC's.
Shadow sampling continues (decision 19); a divergence is fixed forward on a 2-hour target.

| Cohort | Job types | Pairs (N) | Saves when it moves | Deleted in the same change |
|---|---|---|---|---|
| 1 | `Required` gate, `Required verdict`, Plan, Ledger CLI, the four lints, freshness | 100–150 per type | about 15 s of acquisition and 25–40 s of set-up per gate job, on the critical path | Their ARC routes and hosted twins; Plan's tool-seed steps; two tool composites |
| 2 | The Rust crate template (26 jobs), `check-maintenance-helpers`, `check-generator-samples` | 300 across crates; 150 | about 30 s of set-up × 26 = 13 runner-min per run; the verdict's maximum 8.5 → ≤ 4 min | 26 routes; six tool composites |
| 3 | `velnor-runner-host`, its `local-mode`, `velnor-local` (the three Docker-class binaries) | 150 each | verdict P90 10.1–10.4 → ≤ 6 min | Their routes; the shared dind sidecar for velnor-new |
| 4 | State seal (then deleted), job image candidate, source qualification, `release-rust-ci-tools` and product release (Linux) | 30 each; three release dry runs per workflow | hosted queue 5.9–9.1 min per run; a release about 104 → about 1 min with promotion | Hosted release labels in velnor-gen and the engine; the state seal and `cache-prune.yml` |
| — | `qualification.yml`, homebrew and mac-proof | — | — | Retired |

Merge-group and push runs move with their job types. Cohort 1 is the cheapest gate chain, cohort 2
one template for 26 jobs, cohort 3 sets the verdict's maximum, and releases come last: rare, holding
secrets, needing attestation and the App token.

### 10.2 Milestones by day

"Day N" means hours 24N to 24(N+1) from the start; P90 dates in brackets; every figure is [E] from
the delivery plan's critical path, except the goal row, which is [T].

| Day | Milestone, and what it deletes | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Constraint that sets the date |
|---|---|---|---|---|---|
| Today | Baseline [M, proxy] | 26.7 / 92.3 min | 85 / 457 min | 285–347 | — |
| 0 | Configuration landed: two contexts bound to 15368, empty `bypass_actors`, ALLGREEN, depth 2; crates mirror on. **Deletes** hosted release runners in velnor-gen, `edited`, the fork twin, the catch-up rule, `-j2` caps, Darwin legs, per-push image and qualification triggers | ≤ 12 / ≤ 20 min | ≤ 35 / ≤ 60 | ≤ 170 | Owner decisions D1–D3, D9; the engine release's wall time |
| 1.2 | First conformance runs on the Docker backend, both executors | ≤ 11 / ≤ 18 | ≤ 30 / ≤ 55 | ≤ 160 | Reviews, the platform trait, the Docker backend, `velnor-official` |
| 2.1 (3) | Cohort 1 on Velnor Native on Kubernetes; native checkout. **Deletes** the gate routes and seed steps | ≤ 9 / ≤ 16 | ≤ 25 / ≤ 45 | ≤ 150 | D8; the Kubernetes backend and dispatcher; 150 pairs per type |
| 3.2 (4.5) | Cohort 2. **Deletes** 26 crate routes and six tool composites | ≤ 6 / ≤ 10 | ≤ 15 / ≤ 28 | ≤ 120 | Evidence and replay capacity |
| 3.8 (5.3) | Cohort 3 with private Docker daemons. **Deletes** the shared dind sidecar | ≤ 5 / ≤ 9 | ≤ 13 / ≤ 25 | ≤ 110 | Lab D; Docker-class evidence |
| 4.2 (6.5) | App required on both subjects after lab A and shadow parity; release authority migrated; label flow on | ≤ 5 / ≤ 9 | ≤ 12 / ≤ 22 | ≤ 110 | D7; 300 heads and 100 groups of parity |
| 4.8 (7) | Cohort 4 (releases on Velnor). **Deletes** hosted release labels, the state seal and prune | — | — | — | Release dry runs |
| 5.2 (7.5) | Ruleset step 4; obligations verify PRs and groups with whole-tree keys and a cold second witness; A1 replaces the full push run. **Deletes** the PR and group Actions gates and the interim label step | ≈ 6 / 18 | ≈ 9 / 30 | ≈ 75–90 | Parity report; A1 live |
| 6.5 (9) | Final deletion. **Deletes** ARC's velnor-new scale set and profiles, the Turso journal and the parts of `crates/velnor-runner` review did not keep. **Keeps** the official runner's image, `.velnor/runner.lock` and `velnor-official` (decisions 19, 20) | ≈ 4 / 12 | ≈ 7 / 22 | ≈ 70 | Local runs on Docker; review of a large deletion |
| 7.5 (10) | Compile reuse graduated on the helper and generator roots: second witness over another generation | ≈ 2.5 / 8 | ≈ 5 / 15 | ≈ 55 | 3,000 comparisons per root |
| 10 (13.5) | **Goal.** Per-root keys enrolled after the read audit's shadow; the test floor fixed | **≤ 90 s / ≤ 4 min** [T] | **≤ 3 / ≤ 8 min** [T] | **≤ 50** [T] | Contract review; statistical evidence |
| 16–18 | **Stretch measured:** per-package keys, `velnor verify`, longest test ≤ 15 s, the commit stamped after link | 30 s / 100 s [T] | 100 s / 4 min [T] | ≤ 20 [T] | Package enrolment; labs B and C |

A milestone that misses its numbers holds the next cohort until the miss is explained. Items are
2–8 agent-hours, one PR under 400 refactored lines; ports land as a checker-proven verbatim move,
then the refactor; every replacing PR deletes what it replaces.

### 10.3 Lab experiments and their safe defaults

Each default stays correct whichever way its lab goes.

| Lab | What it runs | Pass | Default until then |
|---|---|---|---|
| A. GitHub semantics | A lab repository with a ruleset, a merge queue and a test App: no status; App `pending`; App `success`, then one more push; the context from a user token and from `GITHUB_TOKEN`; a status only on the test-merge SHA; a required Actions job with `if: false`; a skipped caller whose inner job is required; a dispatched run; two PRs at one SHA; queue entries with no report, `failure` and `success`; labels changed with `GITHUB_TOKEN`, an App token and a user token; the state-machine cases of section 8.2 | The no-status, pending, foreign-source and skipped-caller cases block; App `success` merges until the next push; a queue entry lands only on `success`; the read-back shows the App context required on both subjects | Today's gate, hardened and bound to 15368; the App only observes |
| B. Build matrix | One idle worker, three repetitions: B1 dependency `opt-level` 0 and 3 against assertions on and off, whole suite; B2 reflink against overlayfs with 1, 4 and 6 sandboxes, the canaries of section 7.3, and digests against a cold build; B3 seeding, lockfile changes and the wait-or-fetch-or-cold crossover under load, by `mbx-cache`, file-delta sync and compressed layers; B4 incremental on and off over 300 replayed diffs and 20 planted failing edits | B2: every canary rebuilds and digests match; B3: a transport beats a cold build on more than 10% of requests; B4: zero differences | Reflink; cold second witness; `CARGO_INCREMENTAL=0` for receipts; the `opt-level` override with checks on, its gain uncounted |
| C. Shadow ledger | Beside production for 300 merges: C1 whole-tree and per-obligation hits of each merge group against the PR's records; C2 twins on two nodes against two runs 30 min apart, on 20 tests with seeded failure probabilities of 0.05–0.2 | C2: joint pass rates match (1−p)² | Groups derive their own vectors and execute every miss |
| D. Docker class | The three Docker-needing binaries, 200 runs each, under a private privileged daemon, a rootless daemon and a microVM, on both backends | Equal pass, fail and skip sets; nothing one request leaves is visible to the next | A private privileged daemon per request, or refusal |

## 11. Top risks

| Risk | Detection | Containment |
|---|---|---|
| A reuse gives a false green | Two independent witnesses; shadow re-execution; 2% sampling; audit A1; the enforced read audit | Revoke by epoch; work runs again; landed commits in the window audited cold |
| Velnor Native departs from GitHub's semantics | The harness on every change, nightly and per official release; sampled shadow runs | Block the release; fix forward |
| GitHub changes the protocol | The harness within 14 days of each release; per-version deadlines from the API with an alarm 21 days ahead; a canary every 10 min | Fix before the deadline; a critical update pages at once |
| The skipped-job trap or a mis-set ruleset lets an unverified head merge | Permanent canary PRs probed every 10 min; ruleset read-back | `Velnor / Verified` is the only gate; only `success` passes; no bypass actors |
| A stale status write survives a policy change | The success index; read-back; lab A's partition case | One fenced writer; emergency revocation writes `failure` |
| The release path trusts the old gate | Both release entry points exercised with missing, stale and wrong proofs | One release authority before the ruleset changes |
| A move leaves no way back and diverges | Sampled shadow runs; per-type failure rates | Fix forward on a 2-hour target; redeploy the previous image digest |
| A backend or store behaves differently | Per-backend conformance; content verification on read | Capability fallbacks; slower, never wrong |
| Stale compile state | Content-proven inputs; the cold second witness; A1 digests; canaries | Retire the generation; build cold |
| The cluster is saturated | Per-class CPU and memory watermarks | Batch work preempted or requeued first; reserved merge path; lab replay at night and on Docker hosts outside the cluster |
| Owner decisions wait; agents collide | Due hours; conflict counts | One decision pack with defaults; one owner per crate and shared file (delivery plan §8) |

## 12. Owner questions

Only what is open; the plan runs on each default until the answer arrives. Decided or ruled: the
old runner's PRs parked (decision 16), the reference executor (19), two witnesses and ALLGREEN (D10),
where release bytes are built (D12).

| # | Question | Example | Recommendation and default | Needed by |
|---|---|---|---|---|
| D1 | Release the prepared engine version that moves the gate jobs off hosted runners? | Eight gate jobs still run on `ubuntu-26.04` and waited 57 of a 71.8 min run on the worst day | Yes, now | hour 0 |
| D2 | Should agents enable auto-merge and stop merging `main` into their branches? | 0 of 40 recent PRs used auto-merge; the median waited 7.5 min between green and enqueue | Yes | hour 0 |
| D3 | Apply the first ruleset update now: two contexts bound to 15368, empty `bypass_actors`, ALLGREEN, depth 2, timeout 90, minimum wait 0? | An admin bypass carried 7 of 136 merges in three days, and three commits with a failed `Required` sit on `main` | Yes; depth 4 and timeout 15 min only once a group verdict takes ≤ 5 min at P90 and paired false failures are ≤ 3% | hour 2 |
| D4 | Which evaluator ships: A, velnor-legacy's port of the legacy `Expressions2`, or B, a port of the new `Sdk/Expressions` and `WorkflowParser`? | v2.338.2 computes the new evaluator in compare mode but returns the legacy result; Velnor refuses every job where the two differ, so on accepted jobs A and B give the same answer | Recommend B: it complies with decision 3 and ruling Q4, and correctness is equal because differing jobs are refused. Default A until you answer, because it matches the latest runner and costs 3 h against 28; only one ships | day 3 |
| D7 | Create the `velnor-verify` App (section 8.2's permissions and webhooks) and an agent identity without repository administration? | Today agents label and merge under the owner's identity, which can edit the ruleset | Yes; its status becomes the gate after lab A and parity | day 2 |
| D8 | A deploy identity for `velnor-system`, and the node changes (XFS work directories, reservations, taints)? | Job directories sit on the ext4 OS disk, where a warm target copy costs 8.6 s and 10.75 GiB | Yes | hour 6 |
| D9 | Pre-approve engine releases that change only CI? | Each day-0 change needs one release | Yes, one approval for the batch | day 0 |
| D11 | The per-type cutover counts of section 10.1? | Cohort 1 needs 150 replayed pairs per type | Yes, as written | day 1 |
| D14 | Delete the engine crates from velnor-new once velnor-gen's workflow replaces the engine's CI? | The engine's orchestrator and renderer are about 275k lines | Only after checking for other consumers | day 8 |
| D15 | Approve the recovery path for a bad Velnor release, with no admin merge? | A release that fails every job also blocks the PR that fixes it | Yes: redeploy the previous image digest; if none can verify, the pre-approved break-glass verifier writes a green only after a full cold pass (section 5.9) | day 1 |
| D16 | Does a `velnor:verify` label added while the PR was a draft start verification when it becomes ready? | An agent opens a draft, labels it early, then marks it ready; today nothing would run until it re-labels or pushes | Recommend yes: the label states intent and the reconciler can read it from state. Default no, the literal reading of decision 12's step 2 | day 2 |
| D17 | Who may approve a reduction (an obligation or test removed from the inventory)? | A PR deletes a slow test file; the head plan lists fewer tests than `main`'s | Recommend: only the owner, through a signed inventory entry. Default: no reduction is accepted | day 3 |

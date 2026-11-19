# Velnor runner architecture

**Status:** Proposal (v3); it replaces nothing yet. It follows the owner's binding decisions 1–24
(section 5.1). The panel's contested facts are ruled on evidence (decision log); where a ruling
names a lab test, the design runs on that ruling's safe default until the lab reports (section
10.3); questions only the owner can answer are in section 13. **Scope:** how Velnor verifies, merges
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
from the LAN are reported apart. Push-to-merge is reported gross and net of GitHub's own queue
share (enqueue to group, group to merge), which no runner controls.

**The claim.** Until section 11's suite passes, the claim is only: *designed to cut velnor-new's
push-to-merge; very likely 10–40× faster than its historical hosted and ARC CI* [E, against
handicapped baselines]. After it passes: *the fastest measured push-to-merge on a specified
velnor-new workload, at a stated resource budget, with the same verification contract*. The speed
belongs to the verification system (ledger, label flow, mirrors, warm generations): PR and group
verdicts run no Actions job, so the executor is off the verdict path; an executor claim needs its
own race (section 11). Every `ensure` is one OpenTelemetry trace (D11): plan, record resolution with
its reason, provision, checkout, compile, each test binary, signing and the status write, exported
over OTLP to a self-hosted collector; nothing in the gate reads it.

| Measure | Today | Basis |
|---|---|---|
| Push-to-merge, P50 / P90 | 85 min / 457 min | [M, proxy] 98 merged PRs, from the first observable CI run at the final SHA, not a push timestamp (`proposals/opus-5.5.md` §1.1) |
| PR run wall time, P50 / P90 (cluster era) | 26.7 min / 92.3 min | [M, proxy] run creation to completion, 77 completed PR runs, 27 of them failed (§A T2) |
| PR runs cancelled; runs on an unchanged head | 1,156 of 2,558 (45%); 469 (18.3%), the `edited` trigger | [M] §A T1; F-H07 |
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
   6.1's enrolment narrows one; a read trace never proves a key complete: it cannot see a branch the
   run did not take, clock reads answered in the vDSO, or the run-to-run variation of what is read
   (a `HashMap`'s seed chose a different fixture on one tree, so a trace-keyed lookup passed a tree
   where 12 of 40 fresh runs fail [M, landscape ruling X01]).
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
   `.git` (a census first finds obligations that need it, such as `root.rs:22`'s `git rev-parse
   --show-toplevel`; they get a read-only repository at the subject commit and are keyed by it), a
   scrubbed environment (no `GITHUB_*`; fixed `HOME`, `PATH` and locale) and a fixed CPU count.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow with every
   reuse re-executed until about 3,000 clean comparisons per obligation kind (a bound near 0.1% at
   95%, never a proof of zero, so A1, sampling and planted failures stay); then 2% are re-executed. Until compile reuse graduates, the second witness builds cold. Every landed commit is
   rebuilt cold and its changed obligations re-run (audit A1, section 6.5). The first disagreement
   suspends the class and blocks its dependants at once; five re-runs then decide between a flake
   (the class resumes) and a wrong key (the epoch is revoked).
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

- **Budget:** per verification record, which has two witnesses, at most 3% false failures once
  verification leaves today's full runs and 1% at the goals, so each witness may fail falsely about
  half as often (two runs at today's 8.1% fail together-or-either 15.5% of the time); each claimed only after about 300 rerun pairs sampled both ways under
  load (zero disagreements in 299 bound the rate just under 1% at 95%). "Not requested" never
  counts. Today's 8.1% [M] comes from full runs.
- **Act:** retries stay at 0, so a flaky test fails and is counted. Per-test discordance is scored
  from witness pairs, A1 and the 2% sample; two false failures in seven days open an issue
  automatically. A test is fixed, or quarantined only when an equivalent blocking check is shown to
  catch the same planted fault; a quarantined test keeps running in a non-blocking lane and returns
  to blocking automatically when its quarantine expires. An owner exception is an inventory
  reduction (owner question D17), unavailable while D17's default holds.
- **Known causes first:** CPU starvation of tight timeouts under pods that request 2 CPU and burst
  to 84 cores (§B.2.2); the `event.json` rewrite race between steps (I023). Obligations run the `ci`
  nextest profile, so the 2 s kill of the default profile (S8, F-H36) is not among them.

### 1.4 Targets

Measured on the clocks of section 1.1; the arithmetic is in sections 3, 4 and 10.2.

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Conformance |
|---|---|---|---|---|
| Today | 26.7 / 92.3 min [M, proxy] | 85 / 457 min [M, proxy] | 285–347 (CI) [E] | — |
| App gate, whole-tree keys, cold second witness | ≈ 6 / 18 min [E] | ≈ 9 / 30 min [E] | ≈ 110 [E] | 100% on both backends |
| Goal | ≤ 90 s / ≤ 4 min [T] | ≤ 3 / ≤ 8 min [T] | ≤ 50 [T] | 100% |
| Stretch | 30 s / 100 s [T] | 100 s / 4 min [T] | ≤ 20 [T] | 100% |

Every row after "Today" is conditional on section 11's measurements. The goal needs per-root keys
enrolled, compile reuse graduated and the test floor fixed; the
stretch needs per-package keys and the commit stamped after link. Further goals [T]: job start ≤ 1 s
P50, ≤ 2 s P90 from the broker message; no test over 60 s alone (stretch 15 s); per release, zero
compiles and about 15 s of publication [E].

## 2. Where the time goes, and what changes the problem

Four causes [M]: **waiting** (gate jobs queued for the Free plan's 20 hosted slots, 57.0 of a 71.8
min run on the worst day; a superseding push waits up to 2,203 s; no auto-merge); **repetition**
(45% of PR runs cancelled, three full runs per merge, crates compiled 4.4 times per run, releases
rebuild); **cold jobs** (the one restore took 44–61 s, longer than the build it saved); and **the
code's floor** (looped tests of 258–274 s, `build.jobs=2` caps). The design removes work rather than
speeding it up.

## 3. The latency budget

One typical PR (the 47% class touching only `generator/` or `ci-helper/`), labelled, at the goal
[T], with the estimate in brackets.

| Stage | Today [M] | Goal [T] (estimate) | Bottleneck removed |
|---|---|---|---|
| 1. Event, webhook, job offer | push → run 1–2 s; run → first job 2 s p50, 226 s p90; hosted Plan queue 5.5 min median | webhook 1–2 s; the reconciler within 10 s | Verification starts from the label or labelled push; no Actions job is on the verdict path |
| 2. Acquire | 13 s p50, 27 s p90 from job created to started, n = 1,124 (scale set, ruling V5) | not on the verdict path | Obligations are not Actions jobs; Actions jobs (release, image, scheduled) acquire on pre-registered JIT identities |
| 3. Sandbox start | ARC pod about 6 s (436 starts) plus a dind probe | build class on a pool hit: ≤ 1 s P90 for claim, attach and spawn (only the 0.25 s clone is [M]) | Warm pool sized by demand; the clone lands in the warm pod's pre-mounted volume, since a running pod takes no new mount |
| 3d. Docker-class start | 3–4 s dind probe [M] | seconds: a private daemon per request | Only the three Docker-class binaries pay it |
| 4. Checkout | 6 s per job × 31–39 jobs; Plan 12 s | ≤ 1 s | Node mirror; a reflink clone of a sealed tree plus the changed files, then a full tree check (section 7.4) |
| 5. Toolchain and tools | 10 s median, 18 s p90, plus mise 2 s and action downloads 4.4 s per job | ≈ 0 s | Read-only verified tool store; native actions |
| 6. Dependency fetch | 3.5–6.1 s per job; sources cache 0 of 48 | 0 s (≈ 3 s on a lockfile change) | Site crates mirror and node crate store |
| 7. Compile | 196–497 crates per job; 81 s runner-host, 14 s velnor-ci; 8.5 min critical path | ≤ 25 s (15 s) on heads that add no source file; 64% of landed commits add one and build their root cold until lab B5 | Warm generation: only the change's closure compiles; 0 crates and 0.31 s for an unchanged tree [M] |
| 8. Tests | velnor-ci 10 s / 62 s; runner-host 192 / 417 s; floor 125–138 s | ≤ 60 s (10 s) | Per-root keys, two witnesses in parallel; the test floor programme |
| 9. Result reporting | 1.48 GB archive upload 36–43 s; `Required verdict` 49 s | ≤ 1 s | Results streaming; binaries by digest in the site store |
| 10. Required check | hosted verdict queue 1.6 / 24.3 min, then `Required` 1.9 / 21.4 min | ≤ 1 s after the last receipt | One App-written status, `Velnor / Verified` |
| 11. Merge-queue entry and run | green → enqueue 7.5 min (0 of 40 auto-merge); enqueue → group 19 s; group 19.9 / 84.1 min | ≤ 30 s (≈ 25 s) | Auto-merge; an exact-tree lookup for about 60–75% of groups (16% today), else per obligation |
| 12. Merge | 26 s | 26 s | GitHub's queue (decision 7) |
| **Verdict / push-to-merge** | **26.7 / 92.3 min; 85 / 457 min [M, proxy]** | **≤ 90 s / ≤ 4 min; ≤ 3 / ≤ 8 min (≈ 35 s; ≈ 85 s)** | |

**Floors** [E]. GitHub's 19 s to a group and 26 s to merge are measured medians under load, not a
proven floor. For the helper and generator class the runner's own floor is about 20–35 s of verdict
(webhook and `ensure` about 2 s, one whole-crate compile 8–20 s, tests 2–10 s), so push-to-merge
floors near 65–80 s with GitHub's share. The P90 has no such support: about 20% of first-parent
commits since 2026-11-01 touched a whole-tree path (`.github/`, `.velnor/`, root manifests and
configuration: 42 of 214 [M, `git log`]), and their slowest obligation sets the verdict.

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
| 8 | Checkout, toolchain, mise, action downloads; Crate fetch | every job (31–39 per run); every Rust job | yes | 14–18 s per job; 7.5–8 min per run; 4–5 s per job | E; R once per lockfile | Node mirror, read-only stores, native actions; Offline crate store |
| 9 | Third-party crate compile | every Rust job | yes | 2,106 builds of 475 crates per run (4.4×; another count gives 2,260 of 463, 4.9×); 6.3–21.7 GB written per job and discarded | R once per lockfile, toolchain and profile | Warm generation |
| 10 | Clippy, doc, check-alone | every run | different outputs | clippy 36–75 s, doc 30–65 s per job | K as parallel lanes; R per key | One target lane per check kind |
| 11 | `-j2` caps, the 89 s image-publication fixture, the CLI built ≥ 9 times per event | several jobs | yes | caps cost 4.5 of 8.5 min | E / R once | Caps removed; fixture in the warm build |
| 12 | Tests across PR, merge group and push | three runs | yes | about 25 min per run | R by record; K new keys twice, in parallel | Two witnesses |
| 13 | Full CI on the merge group | merge group | when the tree equals the PR's | 64.6 runner-min; equal trees 16% today | R exact tree (60–75% with auto-enqueue against the queue tip), else per obligation | Records; speculation |
| 14 | Full CI on the push to `main` | push | yes (same SHA in 120 of 159) | 62.0 runner-min; 8.1% false failures | E, after A1 replaces it | Audit A1 |
| 15 | State seal producers and cache prune; GitHub cache save and restore | `main` push; many | yes; — | 683 s per push; 3.49 GB of GitHub cache, 0 of 3,636 restores; 1.92 GB restore 44–61 s | E; E from the verification path | Generations and storage tiers; Storage tiers (section 7) |
| 16 | Source qualification on every PR event; Job image candidate on every push | PR; every push | yes; yes unless images changed | 1,007 s per event, about 80 runner-min per merged PR; 200 s; about 20 min per PR | R once per key, in the queue; R once per image-input key | Obligations; Keyed action |
| 17 | Source preview build, tag rebuilds, product release | `main` push, tag | yes | preview 410–534 s per push; tools 190–434 s; product 4,105–6,238 s | R promote by digest | Release promotion (section 8.4) |
| 18 | macOS and Darwin legs; `VELNOR_COMMIT` rebuilding both tools per commit | `main` push, release; every merge | —; no, by construction | 1,063–1,406 s per push; two tool builds per merge | E (paused, decision 9); E once the stamp after link is proven | — ; the group SHA in the key today |
| 19 | Per-node rebuilds of `main` by warmers | after every merge | yes | about 120 incremental builds an hour | R: the landed group's build output is the next generation, trusted when A1 gives equal digests | Generations by content |
| 20 | A Docker daemon in every pod | every job | — | 3–4 s per job; 57 probe failures an hour | E | Docker only for the three binaries that need it |
| 21 | Second witness, cold audit A1, conformance pairs, speculation | PR, group, batch | yes | — | K | They buy confidence or wall time; charged below |

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
- **GitHub Official** runs the latest official runner in the same sandboxes, pools, ledger and
  caches; Velnor's dispatcher drives both, ARC neither. An *acquired job* (release, image, scheduled)
  completes against GitHub's run service as v2.338.2 does (`JobRunner.cs`); the dispatcher owns the
  lease. An *obligation* gets a job message Velnor synthesises, with run-service, Results and cache
  endpoints served by the node agent; the command runs under `velnor-ci obligation exec`, which
  reports exits and the test inventory over a channel the sandbox cannot write, so the receipt comes
  from observation. This mode passes its own conformance cases before it may be selected. It runs
  with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (`Constants.cs:327`) on a read-only store filled by
  pinned SHA, in both roles, removing 4.4 s of action downloads per job [M].
- **Targets** (D05). Clients name where work runs with `--target` or `VELNOR_TARGET` (`local`,
  `k8s://de`, `k8s://fi`, `docker://host`), authenticated over mTLS with the agent identity; a URL
  scheme maps to a declared capability set, and a result's trust tier comes from the signer class of
  the node that executed it, never from the URL. On a cluster node, an agent shell reaches its own
  node agent through a request-only socket (D06), which accepts requests, never results.
- **Choice and identity.** `.velnor/` names the executor per job type or obligation kind, or
  globally, read from the base commit and limited to certified identities and digests, so a change
  takes effect only after it lands; Velnor never switches on failure. The identity is in every key,
  so a deliberate switch re-runs verification and can never produce a false green.
- **Reference role** (decision 19): the conformance gate, sampled shadow runs and "run both ways and
  diff", writing only to an isolated reference namespace.
- **APIs** (decision 3), as v2.338.2 implements them: scale-set sessions, broker long-poll, JIT
  configuration, run-service acquire, renew and complete, Results, cache v2, artifacts v4, OIDC.
- **Expressions (owner question D4).** At v2.338.2 the Worker returns the legacy `Expressions2`
  result on every path, even with the new evaluator in compare mode. The oracle runs in compare mode
  and Velnor refuses any job on which GitHub's two evaluators differ, so on every accepted job both
  give the runner's result. Option A ships velnor-legacy's port of `Expressions2` (3 agent-hours;
  conflicts with decision 3); option B ports `src/Sdk/Expressions` and `src/Sdk/WorkflowParser`
  (about 28; ruling Q4). Until the owner answers the default is A, matching what the latest runner
  returns; B is built in parallel, and only the chosen one ships (decision 18).

**Speed per job type** [unmeasured as a difference]: Official adds Worker set-up (4–5 s per job [M,
ARC era]), Node start per JavaScript action (87–147 ms [M]) and GitHub's cache until ST05; both share
tool store and generations. Native saves seconds per Actions job, not on verdicts; S2 measures it,
and under 5 s per job at P50 earns no executor claim.

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
    pub network_policy: bool, pub cpu_pinning: bool, pub cgroup_kill: bool, pub arch: Vec<Arch>, pub os: Os,
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
`snapshot_restore`: a new implementation of this trait and nothing else. Direct namespace and cgroup
slots in the node agent, instead of a pod per sandbox, are a Kubernetes-backend candidate measured
against warm pods (provision to first step under load, section 11). **Pool:** per class, a floor by
hour plus the sandboxes `ensure`'s vector asks for plus one group's worth per auto-merge enqueue;
idle members hold small requests and are resized in place at claim (stable on the cluster's
Kubernetes 1.37); a deferred resize counts as a miss; delivery, queue, claim, attach and first
process are timed. **Snapshots** (contract only, nothing built): a snapshot's digest joins the
execution fingerprint; only the builder class takes one, from a pristine environment before any
assignment; a restore supplies fresh identity, entropy, credentials and private writable storage,
and is chosen only where measured faster than provisioning.

### 5.5 The job path

1. **Trigger.** A labelled head's webhook reaches `velnor-control` in 1–2 s (section 8.2);
   `ensure(tree, plan)` fetches the commit into the node mirror, computes the tree with `git
   merge-tree` (head into queue tip, or the group's commit) and runs the released planner.
2. **Resolve and execute.** Each record is satisfied (every slot holds a passing, current, unrevoked
   receipt), in flight (join it) or open (schedule the missing witnesses); C5 always schedules.
   Obligations are plain commands (`cargo`, `cargo clippy`, `cargo doc`, `cargo nextest`) run through
   `velnor-ci` in a sandbox pre-cloned from a verified generation, planned by a resident planner;
   the agent observes them and signs one receipt per witness.
3. **Placement** prefers, softly, the node whose agent advertises the generation, L0 manifests and
   mirror head the work needs, ranked by predicted completion with queue wait; witnesses still need
   distinct nodes and generations.
4. **Actions jobs** (release, image, scheduled): the dispatcher holds every JIT identity, picks the
   node (warm state, site, class, reserved merge path), then acquires and hands over the job.
5. **Report.** The fenced writer posts `Velnor / Verified` when every record of the head's vector is
   satisfied under the active epoch.

### 5.6 Native actions

The closed set comes from every `uses:` in `.github/` and velnor-gen's samples [C]. Velnor Native
refuses a job before its first step when an action, SHA or input is outside the qualified set;
`velnor-gen` emits only qualified actions and SHAs, enforced by a generator test.

| Action | Uses | Native implementation | Node-local state |
|---|---|---|---|
| `actions/checkout` | 61 | The action's inputs and outputs, including `sparse-checkout` (44 uses) and `fetch-depth`; a reflink clone of a sealed tree with only the changed files rewritten, then the full tree checked (section 7.4) | The node's bare mirror |
| `actions/upload-artifact`, `download-artifact` | 77, 60 | Artifacts v4 on GitHub's service, so `gh run download` and the API see them | The site tier as a read-through cache by digest |
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

`velnor-conformance` runs each corpus case through both executors on each backend and compares;
`qualification.yml` is its seed, and the oracle is `velnor-official` in the reference role.

- **Corpus.** At least 240 semantics, 65 fault and 30 refusal cases beside replays of velnor-new's
  job set under every event (failures, cancellations, re-runs): expression functions and coercions,
  contexts and masking, status functions × job states, outputs and `needs`, env files, commands,
  post and background steps, cancellation at every step boundary, timeouts; each native action
  across its inputs, an artifact (and, once the Official cache endpoint is qualified, a cache) saved
  by one executor and restored by the other; both obligation modes; the label flow's states.
- **Pass.** After normalising ids, times, runner names and paths, tokens, set-up lines, ANSI codes
  and interleaving, outcomes, outputs, contexts, env, expression results, annotations, summaries,
  cache and artifact effects, post-step order, cancellation and timeouts are equal, unless an
  owner-signed allow-list entry covers a difference: 100% per backend, zero secret leaks, at least
  95% of about 30 executor mutants killed.
- **When.** As a timed obligation of every PR that changes an executor, a native action or the
  protocol code; nightly; within 14 days of each official release against its tag (v2.338.2,
  `d1eb12d4`, today), with deadlines from the deprecations endpoint (2.337.0 stops 2026-12-25,
  2.338.0 on 2027-01-04; an alarm 21 days ahead, a page on a critical update); unknown protocol
  fields alerted; a surface diff per release; a scan for banned legacy endpoints.
- **Evaluators.** `ACTIONS_RUNNER_COMPARE_WORKFLOW_PARSER=true`; a job on which GitHub's two
  evaluators differ is refused until understood; a fuzzer compares both.
- **Canaries.** A production-path canary every 10 min per site; permanent canary PRs that must stay
  unmergeable (no status; the context from another source; a required `if: false` job), probed every
  10 min: any `mergeable_state` but `blocked` pages the owner and stops `success`.
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
  before acting, runs no credential-bearing step before its first renewal, renews every 60 s from
  `lockedUntil` with a fresh loop (upstream dereferences a null request, `JobDispatcher.cs:748-852`),
  and after a crash resumes or fails the job with its own token, never twice. The ledger persists an execution's
  intent (key, node, witness slot, epoch) before a sandbox starts; results land by compare-and-swap.
- **Fault injection** before each move: crashes around every remote call, journal write and step
  boundary, a lost lease, duplicate and reordered webhooks, a ledger partition, a lost node, the
  control plane and a site. **No fallback** (decision 18): a divergence is fixed forward; the
  cluster runs only released versions.
- **Recovery from a bad Velnor release** (D15), with no merge outside the gate: (1) redeploy the
  previous immutable image digest (a deploy, not a code path); the fix PR is verified by that
  release like any other. (2) If no earlier image can verify, the owner-pre-approved **break-glass
  verifier** runs: a minimal released binary that takes the status writer's fence, executes the full
  inventory cold twice, on two nodes, with the reference-qualified executor (the same two-witness
  proof as section 6.2), and writes `Velnor / Verified` only if every obligation passes in both, with
  an audit record and a page. It is a real green on the exact
  tree; `bypass_actors` stays empty, and no procedure contains an admin merge.
- **Control plane:** one k3s server today (§B.1.5); a whole-cluster dependency waits for three servers
  and a third-site witness for shared fences.

### 5.10 What is reused, after review

Nothing is reused by default (decisions 14, 17): each component is reviewed, and kept only when it
is well designed for this architecture.

| Component | Verdict | Why |
|---|---|---|
| velnor-legacy's broker, JIT, run-service and Results clients; its checkout, artifacts, attest, App-token and mise adapters | Port and refactor | Wire behaviour and tests are sound; legacy-protocol paths are stripped, the god files (`executor.rs` 37,028 lines, `runner.rs` 36,759) split, and the adapters moved onto the node stores |
| velnor-legacy's expression engine (3,565 lines, from actions/runner v2.337.0) | Option A of D4 (production until the owner answers); the harness's reference either way | Its results are what v2.338.2 returns on every path; option B ports the new evaluator (section 5.3) |
| velnor-legacy's `gha_cache.rs` (17,140 lines, v1 and v2 server); its Docker lease and JS sidecars | Redesign, reviewing the v2 message types for the Official cache endpoint; drop the lease and sidecars | Cache v1 is out and the index is new (section 7.2); the socket proxy caused hangs; no Node in Velnor Native |
| velnor-new's `velnor-runner-core` (lifecycle, `CleanupProof`, `OwnedIds`) | Port and refactor | The tri-state "unknown is never absent" rule is right; its two open slot defects (a short id accepted as covering, a live slot replaced) become tests |
| velnor-new's `velnor-runner-github` (scale-set wire types) | Port and refactor, pending review | Pure types; it carries official-runner lifecycle states |
| velnor-new's `velnor-host` journal (Turso) and launch state machine | Delete at the final milestone; no new investment (decision 16) | Bound to the official-runner pair; its discipline (persist the intent, call, record) is kept in the executor journal |
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
  under `--workspace` [M]); the execution manifest (allowed environment, path prefix, modes,
  symlink targets).
- **Enrolled keys** are computed from Git subtree ids plus the contract's single files and absent
  paths; records also carry the digests of their log and JUnit output.
- **Narrower keys need an enrolled contract.** A per-root or per-package key is used only after the
  read audit (section 6.4) has run in shadow on that obligation; a reviewed dependency contract in
  the trusted inventory names every path it may observe, directory contents and absent paths
  included, with deterministic inputs enforced; every execution, hit or sample, runs on the intact
  tree with the audit enforcing the contract, so one observation outside it fails the witness and
  revokes the enrolment; a shadow case where a new sibling file outside the contract changes the
  verdict is caught; and about 3,000 comparisons for its kind are clean. The trace proposes and
  revokes; it never licenses a hit. 79 of 86 files using `CARGO_MANIFEST_DIR` climb out of their
  package and 70 test files list directories, so most test keys stay whole-tree.
- **Time and randomness.** No test manifest declares proptest, quickcheck or rand [M]. The trusted
  planner classes an obligation as C5 when its code path reads the realtime clock, randomness, the
  network or the event: today 5 test files of `velnor-ci` and `velnor-gen` read `SystemTime`, and
  `velnor-ci`'s `now_seconds()` (`io/mod.rs:210`) feeds cache and recording decisions [C]. Ten more
  test files use only `Instant` deadlines: a load-dependent flake risk inside the flake budget, not a
  C5 input. Nothing is reused on a time-to-live; the share of obligations and obligation-seconds in
  C5 is measured before any target counts it (section 11). Repository work shrinks C5: realtime reads
  go through one injectable clock fixed per obligation and recorded in its key (A2 still shifts it
  to catch date bombs), and `CARGO_MANIFEST_DIR` climbs become declared fixture lists. A process-wide
  clock shim is not used: it would silently change what every test checks.
- **The obligation vector.** For every subject SHA the planner derives every obligation and key;
  the ledger resolves the vector against current, unrevoked records. An "instant" verdict
  (rebase-only push, identical group tree) needs zero C5 obligations and zero misses; otherwise the
  C5 set and the misses run.
- **Key shape and reuse class** (Dagger ideas D01, D04, D10). A key is a canonical struct with named
  fields (definition, inputs, fingerprint, executor, reuse class), digested over its encoding, so
  `velnor explain` names the field that changed; a new field raises the key version (a miss, never
  a hit). Each inventory obligation declares `reuse = content | per-request | never`; the planner
  takes the stricter of that and the C5 classifier, and a TTL is used only for advice, never
  evidence. Obligations resolve network inputs `frozen`, only from a committed lock through the
  digest-checked mirrors; an unlocked input fails the obligation or makes it C5, and `velnor lock
  update` is the only `live` path.

### 6.2 Verification records, witnesses, joins and scopes

- **A verification record per key** has mandatory witness slots: two for a new key, in a PR or a
  group alike. Each slot binds one receipt from a distinct node, attempt and order seed, over a
  different verified generation or cold, bound to the immutable inputs and the active epoch; the
  record passes only when every slot passes, and a receipt fills one slot of one record. Audit
  re-execution writes to an audit stream, never a slot. A failed record is shown, never reused.
- **Splits** follow recorded durations from signed receipts (layout only): the union of shard ids
  must equal the inventory, an unknown id gets a conservative estimate and runs, and a nextest test
  group stays in one shard.
- **Order.** nextest 0.9.151 is pinned (`mise.toml:24`) with no shuffled order configured, so the
  agent either lists a binary's tests and runs them in seeded batches through filter expressions, or
  runs a digest-pinned nextest patched with a seed option, whichever costs ≤ 1.5 s per binary
  (measured); until then no order difference is claimed. A fanned-out witness records its shard
  layout in the execution manifest; fan-out stays off for receipt runs until lab C and the batching
  measurement of section 11 show equal results, then splits long binaries longest-first across 2–4
  nodes by test ID.
- **Twin generations.** The builder makes each landed tip's generation twice, cold, on two nodes,
  and seals both only when their digests are equal; the second witness then runs over the twin, so
  its delta equals the first's and no older generation lags it (lab B before it counts).
- **Fast red.** A non-minting lane builds incrementally, runs `cargo check` and the changed or
  historically failing tests first, and posts a failure to `Velnor / Details` at once, cancelling the
  twins; it never writes `success` and never fills a slot, so it shortens time to red only.
- **In-flight join:** a request joins the record its key needs; a cancelled request drops its claim
  and execution continues for the others. Shared work takes its highest live claimant's priority
  and moves to reserved capacity if it cannot resume promptly (section 9).
- **Hedging** is for preparation only (fetch, clone, a compile whose output digest must then equal).
  A raced verification copy is an extra witness that must pass; a cancelled, timed-out or lost copy
  fails the obligation, and only a platform-proven infrastructure abort (`AbsentProven`, policy
  preemption) lets a fresh attempt replace it. Hedging shortens the time to a failure, never to a
  green.
- **Scopes.** A PR's execution writes `candidate` records, reusable by any request with the same key.
  A record becomes `trusted` only when its tree is on `main` and A1 has passed; a build that can mint
  a reusable receipt reads only the `trusted` scope and its own outputs.

### 6.3 What a request executes

| Situation | What runs |
|---|---|
| Rebase-only push, same tree; a re-run with no code change | the C5 obligations; the rest resolves from records within seconds |
| Push N+1 while N runs; stacked or concurrent PRs | N is cancelled at once (no stall); each key's record fills once, and others join or reuse it |
| Merge group | the whole vector: records with equal keys are reused (about 60–75% of groups match the PR's whole tree with auto-enqueue against the queue tip; 16% today), C5 and misses run with two witnesses |
| Push to `main` | promotion of the group's records and bytes; audit A1, the third execution, reported by the push run's Actions `Required` job |
| Release tag | promotion of verified bytes; zero compiles (section 8.4) |

### 6.4 Soundness cases

Every reuse or skip, with its four parts (decision 11): why the key is complete, what fails if it is
not, how that is detected, and the automatic fallback.

| Reuse | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| Whole-tree record (rebase-only push, re-run, identical group tree, same-SHA push) | The tree is every repository input; definition, fingerprint and executor equal; C5 runs anyway | A non-tree input changed: a tool, the kernel, an undeclared clock read | Execution fingerprint in the key; C5 classification; A1; A2 | Revoke by epoch; full run |
| Per-root or per-package record | An enrolled, reviewed contract covering every possible observation, enforced by the read audit on every execution | A read the contract missed, on a branch no audited run took | Only executions chosen independently of the key: the 2% sample, A1's key-independent re-runs and the nightly reuse-free run; the sibling-file canary | Enrolment revoked; whole-tree key |
| Candidate record reused across pushes and PRs | Same key, Velnor-observed, two independent witnesses | A PR sandbox leaked state into a result | Sampled re-execution; trusted only after A1 | Candidate reuse off for that class |
| `velnor verify`, `velnor ship` and counting agent commands (section 8.6) | The same sealed sandbox, released command graph and key as the gate, on cluster nodes, over the node-hashed tree | A Mac or dirty-worktree result treated as evidence | Only T2 node-signed receipts fill slots; T0 and T1 never; arm64 and amd64 keys differ | The label runs what is missing |
| Warm compile over a generation | Section 7.3: inputs proven by content against the generation's manifest; every changed, added or removed path either declared by a unit's dep-info or the root builds cold; every unit cargo did not rebuild has dep-info inputs with unchanged blob ids | A stale `OUT_DIR`, build-script or proc-macro output, or an older-time file cargo skips | Until graduation the second witness builds cold and the binary digests must equal; A1's cold digests; the standing canaries of section 7.3 | Retire the generation; build cold |
| `mbx-cache` units for a cold or mismatched node | mbx's per-crate keys | A missed input in mbx's key | A1; the builder's cold rebuilds; digests equal before a unit enters a generation | Server retired; plain cargo |
| `actions/cache` entries | The user's key and version, resolved by the authoritative index with GitHub's scope and order | A user key that omits an input (as on GitHub) | Conformance against the official runner; content hash on every read | Treated as a miss; never used by a receipt-minting obligation |
| Artifacts, binaries, tool and crate stores | Content address; pinned versions and lockfile checksums | Corruption; a store edited in place | Hash on read; read-only mounts (`EROFS`) | Refetch, rebuild, or rebuild the store |
| Test run keyed by binary digests (D03; only after lab R06 removes the commit from the binary, twin builds give equal digests, and about 3,000 clean D02 comparisons per kind) | The run step's inputs are the bytes of every binary it executes, the enrolled runtime contract, the definition and the fingerprint; `include_bytes!` data is in the bytes | A stale warm build reproducing the old binary; a run-time read outside the contract | Lookup digests come only from twin builds with equal digests, one cold until compile reuse graduates; 2% re-execution; A1; S1 plants a data-file change that leaves the binary unchanged | The class is suspended; keys fall back to per-root or whole-tree |
| Hedged copy of a slow shard | Every launched copy is a witness of the same key and must pass | A hang discarded as the losing copy | A cancelled, timed-out or lost copy fails the obligation (C4) | None needed: hedging cannot produce a green |
| Mirror checkout | The staged tree is hashed in full and must equal the requested tree before the first step | A stale, corrupt or concurrently refreshed base | The full check, every time | Fetch the commit from GitHub; rebuild the mirror from the site bundle |
| Release promotion | Closure, release profile, target, tool image; the digest is in the receipt and the attestation | Wrong bytes published | Re-hash and `gh attestation verify` at publish; A1 rebuilds cold and compares | The release stops; nothing is rebuilt silently |

**The read audit** resolves paths in the kernel through LSM hooks (opens, attribute and directory
reads, failed lookups), filtered by the sandbox's cgroup, and a seccomp filter makes every path call
it cannot see fail the witness: `access`, `faccessat2`, `execve` lookups, the legacy `open`, `stat`,
`lstat`, `readlink` and `getdents`, and io_uring opens, which complete without a syscall exit. Paths
resolve against the materialised tree; its overhead, with the tree hash, must
stay within 3% of suite wall and 1.5 s per obligation at P50, measured in shadow. A
backend without it keeps whole-tree keys. **Every per-root contract includes** each tracked file
directly in the repository root and in every ancestor of the root (today `rustfmt.toml`,
`clippy.toml`, `deny.toml`, `rust-toolchain.toml`, both `Cargo.*` and `mise.*`) and `.cargo/` and
`.config/`: rustfmt, clippy, cargo and nextest search parent directories, and `ci-helper/` and
`generator/` have no configuration of their own. `.github/` and `.velnor/`, which 20% of commits
touch, enter a contract only where review or the audit finds a reader (generator goldens, the
planner's own inputs), so most heads that touch them keep narrow keys.

### 6.5 Trust root, publication and audit

- **Keys.** One Ed25519 signing key per node and sandbox class, held by a signer daemon outside every
  sandbox and the agent's general process; receipts go to an append-only log per site; the ledger
  verifies signature, class and log entry before a receipt fills a slot.
- **Publication.** One fenced status writer per repository holds an epoch-numbered lease, in the
  repository's home site until a third-site witness exists; the other site executes and reports to
  it. It keeps a durable index of every `success` it published (subject SHA, PR or group, epoch),
  labelled or not, rechecks the request generation and active epoch before a write, reads the
  status back after it, and reconciles an ambiguous write. The fence refuses a stale lease's write.
- **Revocation.** Every record carries its epoch. A *planned* change (a reuse class enabled, a key
  version raised) freezes new `success` writes, lets in-flight groups finish, activates the new epoch
  and re-resolves open heads. An *emergency* revocation (a signer, a reuse class, a reused pass whose
  cold run fails) stops writes, writes `failure` on every indexed success still an open head or
  queued group (GitHub ejects a group whose required status fails), re-verifies them, and audits
  every commit landed in the window cold, reverting on failure.
- **Audit A1.** For each confirmed update of `main` the ledger journals an audit request per new
  first-parent commit (a group of two lands two) and reconciles gaps from Git history, so an
  intermediate commit with no push run is audited too. Each is rebuilt cold at the fixed path, its
  changed obligations re-run, and results and binary digests compared with its records and bound to
  the commit. A1 picks no work by a narrowed key: narrow-key obligations re-run at the hit-sampling
  rate whether or not their key changed, and one reuse-free run of the full inventory covers the tip
  every night. A1 has a reserved slot per site; its SLO, 2 h at P90 [T], holds at 95% utilisation; a
  2 h lag alarms, and at 6 h it pages, the trusted scope stops advancing, the release waits and
  warm-compile reuse suspends. The push `Required` job reports A1 (section 8.1).
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

- **Backends and routes:** the local filesystem; RustFS single-node or an erasure-coded cluster per
  site; any S3-compatible store (MinIO, AWS S3, R2). Tiers compose in configuration (`local`, `local
  → site`, `local → site → cross-site`); each data class routes separately (`actions/cache`,
  artifacts, release artifacts, the mbx-cache store, generation shipping). Defaults: `local` only on
  Docker and macOS (single-node RustFS optional on Linux, but it can never grow); on Kubernetes, node
  NVMe (L0) → the site's RustFS (L1) → the other site by replication (L2), `strict` durability on
  every bucket until Linux fsync is measured, release buckets always `strict`.
- **What goes where** [M, `rustfs-exp/results.md`, one loaded Mac, indicative]: a 6,053-file
  registry took about 1.2 s to GET as a 44.5 MB tarball and about 17 s to extract, against about 5 s
  to reflink-copy it; small PUTs ran at 120–385 objects a second (16 KiB). Restore cost is
  extraction, so **extracted trees stay in L0** and **RustFS holds only blobs**.
- **Topology.** One cluster per site (quorum writes in one erasure set, no WAN-aware placement);
  between sites, asynchronous replication of content-addressed buckets, as warmth only. Keys are
  single-writer: blobs are named by hash, names live only in Velnor's index. Each worker gives
  RustFS its own device or LV, never the L0 filesystem; parity is explicit; layouts are immutable.
  **The sites are uneven** [M, evidence §B.1.4]: each of FI's three workers has one 3.5 TB data disk
  wholly in VG `data`, LV `rancher` (XFS cannot shrink), so each is drained and its VG rebuilt with a
  second LV, one node at a time (L0 is cache); EC:1 over three drives then survives a node loss. DE's
  two workers survive a drive loss, not a node loss, so DE is cache-only. FI is the home site of the
  status writer, the cache index and the release buckets.
- **Invariants:** `actions/cache` semantics never change; Velnor's cache layer, not the backend,
  enforces scoping, PR writes never reaching `main` scope, immutable keys and content verification;
  a misconfigured, lagging, stalled or degraded store only makes things slower (a missing or corrupt
  blob is a miss). The node agent holds credentials; jobs get presigned URLs. `BlobStore` gains a
  batched `find_missing` (answered from Velnor's index where a backend lacks it); a manifest or
  record commits only after every blob it names is confirmed present and its last use renewed, so
  eviction never leaves a record without bytes; L1 reads split into parallel ranges, hash-checked per
  chunk, and lookups are batched across the 35 ms site link.
- **Phase-1 cluster test** (plan ST06) before L1 carries any route: node loss and return with issue
  #9480's sequence; replication over WireGuard at 35 ms with existing-object backfill (#8768) and an
  `rc` resync; Linux fsync cost per durability mode; the 8.7 GiB snapshot against a 92 s cold build.

### 7.2 Native `actions/cache`

Users write plain `actions/cache` (`key`, `restore-keys`, `path`, `enableCrossOsArchive`,
`cache-hit`, `cache-matched-key`) with GitHub's exact semantics: a run reads its own ref, its base
and the default branch, writes its own ref, and keys are immutable; conformance proves it.

- **One authority for names.** Each entry is `repository × scope × key × version` (GitHub's version
  hashes paths, compression and `enableCrossOsArchive`), committed once, insert-if-absent, by the
  repository's cache index in its home site; it names a content-addressed manifest of chunks, and
  user key strings never become object names (RustFS rejects `.`, `..`, empty segments). The other
  site resolves through the home index; unreachable means a miss and a dropped save.
- **Scope and order.** The scope comes from the job token's ref, base ref and event, the PR merge
  ref included. Lookup follows GitHub's order (exact key, a prefix of it, then each restore key, in
  the run's ref, then the base and default branch) and resolves a manifest before any blob tier is
  read, so an L0 prefix hit never masks an exact entry.
- **Tiers and chunks.** L0 holds the extracted tree of each resolved manifest, reflink-cloned per
  job; L1 holds compressed chunks of about 1 MiB cut by content over the uncompressed archive (a
  one-crate change uploaded about 1% of the bytes, 4 of 419 chunks [M, one pair]); L2 replicates
  them. A lagging replica, missing chunk or hash mismatch is a miss; GitHub's cache is off the path.
- **Eviction** (7 days unused, a size cap) is least-recently-used order in the index; S3 lifecycle
  only backstops orphans. **Prefetch** warms L0 for keys known before the job (`hashFiles` at the
  subject commit); the step resolves again and uses it only for the same manifest.
- **Receipt-minting obligations never restore `actions/cache`.** Tests: concurrent saves, an L0
  stale prefix beside an exact L1 entry, PR-to-`main` isolation, a disconnected site.
- **GitHub Official's cache** (verified at v2.338.2): Velnor writes the job message, so it points
  `ResultsServiceUrl` at a node-agent endpoint; only actions' traffic follows it (the Worker's logs
  use `system.github.results_endpoint`). The endpoint passes artifacts v4 calls through to GitHub
  with the job's token, and its upload URL implements Azure Blob's Put Blob, Put Block and Put Block
  List, which the stock save uses; a restore is one plain GET. It is re-qualified when the pinned
  action moves; the reference role keeps GitHub's endpoints; until plan ST05 passes, acquired
  Official jobs use GitHub's cache.

### 7.3 Compile reuse

**Measured** [M, `experiments/mbx-cache-cluster.md`, the runner workspace of 357 crates at
`/w/velnor-new`, idle nodes]: plain cargo cold 92 s; a reflink clone of the warm target 0.25 s and
2.5 MB, then 0.31 s and 0 crates to build; today's ext4 copy 8.6 s and 10.75 GiB; mbx on a second
node from a warm `mbx-cache` 39.9 s; a lockfile change 67.4 s plain, 43.4 s with mbx on another node
once one has built it.

**The hot path is plain cargo over a reflink clone of a verified generation** at a fixed checkout
path, with inputs proven by content, independently of cargo's freshness:

1. A generation G is the target directory of a trusted build whose cold A1 rebuild gave equal
   digests, sealed with its **input manifest** M_G (every path's blob id, mode and materialised
   time) and every unit's dep-info. Nothing unqualified is ever a base.
2. For a request tree T, the changed set Δ is every path whose blob or mode differs from M_G, plus
   every path added or removed. The base is G's manifest, never the checkout's base tree: a verbatim
   port of `inputs restore`, which stamps only files unchanged against the checkout
   (`inputs.rs:102-151`), reproduces Q16 when `main` has moved past G.
3. If a path in Δ is outside every unit's declared inputs (dep-info, `rerun-if-changed`, the package
   directory of a script without directives), every workspace unit of that Cargo root builds cold;
   third-party units, whose inputs are the checksummed crate, features, profile, toolchain and
   declared environment, are reused. A new `.rs` file is always outside them, and 64% of landed
   commits add one [M, 137 of 213]. Behind lab B5 this narrows (landscape ruling X01b, as selection,
   never proof): an added or removed path that meets no unit's traced read set (step 5) rebuilds
   nothing by itself, one that meets a set rebuilds that unit and its dependants, and cargo still
   plans on the real tree. It needs a complete trace of every unit of the root, or the root builds
   cold; it is its own reuse class, so the second witness builds cold with equal digests until about
   3,000 clean comparisons on such deltas per root, and the first digest disagreement restores
   root-cold.
4. Unchanged paths keep M_G's times; every path in Δ is stamped later than G's newest output.
5. Each unit's read set is its dep-info plus every path its build script, proc macro or `rustc`
   opened, listed or failed to find while G was built, traced by the node agent. Before cargo runs,
   any unit whose read set meets Δ has its fingerprint removed, so it and its dependants rebuild,
   even when another unit declared the path. After cargo runs, each dep-info input of every unit it
   did not rebuild is checked against M_G by blob id (`io/audit/dep_info.rs`); a mismatch fails the
   build and retires G.
6. Toolchain, profile, environment and fingerprint are in G's key; another one means another
   generation or a cold build.

- **Witnesses.** Until compile reuse graduates (about 3,000 clean warm-against-cold digest
  comparisons per Cargo root), the second witness builds cold and both must report equal binary
  digests; afterwards it runs over a different verified generation on another node, and 2% build
  cold. A unit not reproducible at the fixed path (lab C06) keeps its dependants cold until fixed.
- **No transport on the verdict path by default:** a node holding the generation, or cold. mbx with
  a per-site `mbx-cache` fills nodes in the background while it saves ≥ 20 runner-min a day or ≥ 10%
  of builds would start cold (N54). Lab B3 measures waiting for a warm slot against fetching
  verified state and starting cold, under load, and the wait threshold comes from those queue
  observations; a bounded transfer joins the verdict path only where
  measured faster at the same provenance bar.
- **Variants.** Incremental compilation stays off for every minting witness (ruling Q17) until lab
  B4 shows zero differences; it serves only the fast-red lane. Overlayfs and compressed layers are
  lab candidates; no hard-linked `deps/` base;
  `-Z checksum-freshness` is nightly-only; `CARGO_INCREMENTAL=0` on every receipt or shipped build
  (lab B4); blaze rejected (it built none of the five workspaces and missed a clippy `deny` [M]).
- **Standing canaries** (lab B2, then hourly): the Q16 reproduction (a source file changed to a
  failing value and stamped older than the outputs must rebuild and fail), also with the checkout
  base a commit ahead of G and the file changed between them; an undeclared build-script input that
  changes; a changed, a removed and a new input; a proc macro reading a file.

### 7.4 Node-local mirrors

Each node keeps a bare mirror of every configured repository (decision 24), covering PR head refs,
merge-group refs and tags, fetched on push webhooks with a periodic catch-up. One writer per mirror
per node (`flock`); at most one fetch runs and one waits, absorbing later requests and rechecking
for the commit after the lock; explicit refspecs with `--atomic --no-auto-maintenance`, `gc.auto=0`
and no pruning in place; a refresh is a new mirror swapped in, the old one deleted only after its
readers are proven absent, which protects jobs that read `.git` through alternates. New or wiped nodes
seed from a git bundle in the site store, not from GitHub. Mirrors are read-only to jobs.

**Native checkout from the mirror,** with a target well under 1 s:

1. Make sure the exact commit is in the mirror, fetching only missing objects in one round trip.
2. Reflink-clone a **sealed base**: a pre-materialised tree recorded with its tree id, read-only and
   never refreshed in place (a refresh makes a new base). Rewrite the paths whose blob or mode
   differs (deletions and symlinks included), each from a hash-verified blob.
3. Before the first step, hash the whole staged tree (contents, modes, symlink targets) and require
   it to equal the requested tree. The cost is per file, not per byte: 5,164 files and 37.3 MiB, read
   cold because a reflink clone has no resident pages; 0.43 s on eight threads on a Mac [M, proxy],
   unmeasured on a worker (BM13, a fresh clone, idle and loaded); stage 4's ≤ 1 s is the budget. A
   delta-only hash needs a kernel-verified lower layer, which XFS lacks (no fs-verity). A sparse checkout verifies the selected paths against the tree filtered
   by the same patterns; submodules are verified as gitlinks and fetched only when the job asks,
   from that repository's mirror, verified the same way.
4. Set times by the generation rule (section 7.3). A job that needs `.git` gets the read-only mirror
   through alternates; obligations get no `.git` (C5).

A kernel-verified read-only lower layer that lets only the delta be hashed is a lab candidate; the
full hash stays until its measured cost is known. Tested: a stale unchanged file in the base, a mode-only change, a deletion, and a base refresh
concurrent with a clone.

**Every upstream the same way,** each new version crossing the internet about once per site and
pinned and verified before use: crates by lockfile checksum (the unused crates-mirror DaemonSet,
§B.4.2), toolchains and mise tools by digest, images by digest through a registry mirror per site.
All of it is configuration per platform; on a Mac the Docker backend may reuse the developer's clone.

**What it saves** [E, an upper bound: about 10,000 job records a day, skipped jobs included]: per
job about 5 s of checkout (110 MiB from GitHub today), 4.5 s of crates (sources cache 0 of 48) and
14 s of toolchain and tools (restored 0 of 58 times) [M per job], about 62 runner-hours a day, a
fifth of today's 238–289 [E]; images are measured when the Docker class moves. v3 runs far fewer
jobs, each still saving about 20 s.

### 7.5 Budgets

The last three verified generations per root, then least-recently-used; a generation from a warm
build is replaced by a cold one once its size passes a set multiple of the cold target; 40% of a node's data
volume, alert at 70%. FI workers have one 3.5 TB data disk each, DE workers 32–50 TB (§B.1.4); a FI
worker's RustFS drive is a second LV after the rebuild of section 7.1. The runner workspace's target is 10.6 GiB [M].
Every store has garbage collection and a budget kept by Velnor; `mbx-cache` starts at 300 GiB per
site, and its metadata (PostgreSQL) is per site, so replicating its blobs does not make it
cross-site.

## 8. Label flow, workflow, merge queue and release

### 8.1 Shape and gate parity

After the transition (section 8.2) a PR and a merge group have no Actions run: obligations run
through `ensure` and the App's status judges them; release, image and scheduled workflows stay
Actions jobs on Velnor. Format, lint, docs, "check alone" and tests run as parallel lanes, one target
directory per check kind (`docs/hard-decisions.md:56-62`); `cargo-deny` reads a pinned RustSec copy.

**The push `Required` job** runs `velnor-ci audit wait` with a read-only ledger token: it polls for
the A1 result bound to `GITHUB_SHA` and passes only on a pass, failing after 2.5 h (the SLO plus
margin); re-running the attempt waits again. Its concurrency group is per SHA, so one tip's wait
never queues behind another's (today push runs are not cancelled, `ci.yml:57-59`). While it waits
it holds a gate slot, which the runner-minute accounting charges; when the tip's audit is still
unclaimed, the job takes the audit's exclusive lease and runs it through the node agent's cold path
instead of waiting, with the same signed evidence.

**Checks by name** (D07, D08). `velnor check -l` lists the inventory's obligations by name
(`clippy:generator`, `test:velnor-ci`), and `velnor check <name>` runs exactly that obligation's
released command graph from the base commit; an unknown name is refused, and agents and hooks call
names, not raw `cargo` arguments. Every generator in the inventory (velnor-gen's workflows and
goldens, schema dumps) gets a derived freshness obligation (regenerate in the sandbox; any diff
fails), a generator registered without one is an inventory error, and the hand-written freshness
jobs are deleted when cohort 1 moves.

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
- **Lab A proves it first:** draft → ready with a carried label, removal racing a completion, a
  superseding push, group destruction, lost events, a partitioned stale write, the ruleset read-back,
  and a relabel that republishes nothing (a verdict belongs to its commit, ruling Q9).
- **Transition.** Step 0, now: today's two contexts bound to 15368, `bypass_actors: []`, ALLGREEN,
  depth 2, timeout 90 min, minimum wait 0. Step 1: `velnor-ci release-gate` released and both release
  entry points migrated (section 8.4). Step 2: the App in shadow on every PR head and group. Step 3,
  after lab A and shadow parity: all three contexts required, label flow on. Step 4: the Actions
  contexts and the PR and group Actions gates deleted; the push `Required` stays. Before step 3 the
  label gate is an always-run Actions job with no `if:` or `needs:` that fails without the label: the
  candidate's workflow, the same authority as today's gate, so no new hole; independence arrives with
  the App. Accepted interim costs: any label change re-runs the workflow (`ci.yml:59`), and removing
  the label turns a verified head red; agents and Renovate label only at PR creation until step 3.

### 8.3 Merge queue

GitHub's queue stays (decision 7). **ALLGREEN** applies from the first ruleset update: under
HEADGREEN three commits whose own `Required` failed already sit on `main` [M]. Depth is a cost
setting: `max_entries_to_build` stays 2 and the timeout 90 min until a group verdict takes ≤ 5 min at
P90 and paired false failures are ≤ 3%, then 4 and 15 min (at 8.1% false failures depth 4 disturbs
29% of entries). Depth 2 at about 11-min groups lands about 8 merges an hour [E: two groups per 11
min, less 15.5% disturbed], above today's 2.44 entries an hour [M]. Speculation verifies the queue
tip's ordered prefixes, up to four deep whatever the ruleset's depth, and single-ejection bypass
trees at batch priority; a real group claiming a
speculative record lifts it to the group's priority. Under ALLGREEN each prefix gets its own
status, so the first failing prefix names the culprit and no bisection runs. **A group's tree equals the PR-tested tree**
[M, replay of 172 entries] in 16% of groups today; with the PR verdict on `merge-tree(queue tip,
head)` and auto-enqueue, 83% at 1 min, 74% at 2.5, 67% at 5 and 37% at 20; about 60% without
auto-merge. Plan on 60–75%; no verdict depends on the rate. Agents enable auto-merge and stop
catch-up merges.

### 8.4 Release: build once, promote by digest

A release never rebuilds (decision 10). Tests stay in the dev profile [M]: testing the release build
costs 320.8 / 365.7 CPU-s per cold build of `generator` / `ci-helper` against 191.9 / 282.8 for
debug tests plus release bytes once, shares 15 of 90 crates with the shipped binary, and is still
not the shipped code (Cargo ignores `panic` for tests; the tools ship as musl with `panic = "abort"`,
thin LTO, `strip`). Release bytes are one obligation keyed by closure, lockfile, toolchain, profile,
target and, while the commit id is compiled in, the commit SHA, first computable in the merge group,
whose SHA becomes `main`'s [M]. The tools' release PR group builds and smoke-tests them in its
verdict; for the product, each group starts the build at batch priority under a durable claim keyed
by the group SHA, kept on landing and dropped with a destroyed group, and a release waits for A1 and
qualified bytes. Bytes are qualified once (`--version`, `check-tag`, the post-merge black-box
suites), packed with `SHA256SUMS` and attested once, bound to source, identity, target and digest.

**One release authority.** Today the product workflow embeds its own shell gate
(`schema2_release_eligibility.rs:223-247`, rerun by `schema2_release_publish.sh:45-60`) that never
checks the App or the ruleset [C]. Before the ruleset changes, both product entry points and the
tools release call one released `velnor-ci release-gate`: the source SHA, workflow authority, run,
attempt and current-tip checks, plus the App's `success` under the active epoch (`--app-check`), an
active ruleset requiring it with empty `bypass_actors`, and one successful push `Required` reporting
A1 (`--push-check`; `gate.rs:84-94,277-288,354-376`), exercised at both entry points with missing or
stale proof, a wrong source, a skipped audit and a changed ruleset.

A tag runs one job: gate, fetch by digest, re-hash, `gh attestation verify`, publish (idempotent,
read back, exact `.crate` bytes): about 15 s [E; timed by plan R05] instead of 190–434 s (tools) and
4,105–6,238 s (product) [M]. Stamping the commit after link (lab R06) would build the product once
per closure; macOS legs are paused; `aarch64-unknown-linux-musl` is cross-compiled, smoke-run under
qemu.

### 8.5 Tests and the floor

- **The floor that survives.** `velnor-runner-host`, its Turso journal, `Journal::open` and the
  924.8 s `receipt_integrity` test (it lives in `velnor-runner-host`'s host controller [C]) are
  deleted at the final milestone (decision 16), so floor work aims at what stays: the 679 s
  `non_inputs_differential` suite, actions-cli's 132 s test and 258–274 s looped tests, and every
  other test over 60 s [M, 28 job logs]; a per-binary inventory on a worker (plan BM08) replaces
  these job-log figures. Looped cases become one test each, `test-threads=1`
  goes once tests use private temp directories and ports, expensive valid prefixes are built once
  and cloned, and 356 integration-test binaries become one `tests/it` per package (about 340 fewer
  link passes). Runner-host is 25% of today's PRs, so the PR-class mix is recomputed after F01.
- **The Git differential stays whole.** `non_inputs_differential.rs` compares the planner, Git's
  sparse checkout and the archive guard for every accepted glob: only 69 of the 6,077 generated
  candidates are accepted, so at most about 490 direct `git` calls [C, GPT-6.2-Astra's count]; the
  679 s is the whole suite's. It stays on the merge gate until those paths are deleted or a matcher
  is qualified on the complete corpus; a sealed no-checkout fixture cloned per glob, batching and
  sharding with the original assertions cut its time first.
- **Dependency `opt-level = 3` with assertions on** [M, one test]: 67% less CPU, the checks costing
  5% of instructions; adopted with both checks written out and `turso_core`'s line raised; no target
  counts the gain until lab B1.
- **Pipelined link and test.** Each test binary starts as soon as it links, bound by digest to its
  witness; a later build or inventory failure still fails the obligation.
- **Fan-out** spreads a suite longest-first across free threads when that beats its longest test
  plus moving binaries; a copy racing a slow shard is an extra witness (section 6.2).

### 8.6 Agents verify through Velnor

An agent's verification runs through Velnor, on warm cluster nodes in the gate's own sealed
sandbox, so it counts; a Mac result is advice only; agents ask the ledger before running anything;
and the label then mostly finds the work done. The gate's meaning and decisions 1–24 do not change.

- **Front end.** `velnor cargo` takes the `mise.toml` cargo wrapper slot from `mbx` (which missed 10
  of 10 workspace units locally [M, Carbon]), and a per-user daemon,
  `velnord`, keeps state (≤ 50 ms added per call at P90 [T]). Verbs: `velnor verify`, `velnor ship`,
  `velnor check <name>`, `velnor resolve`, `velnor wait`. A command the planner maps to an inventory
  obligation (its argv shape, package selection and features) is *counting*; anything else is
  scratch and runs locally as advice. The verbs are the contract; the wrapper only maps exact argv
  shapes (a wrong mapping is a miss, never a wrong hit) and stays only if count-only data show agents
  use it. Every verb has `--json`, and `velnor resolve --json` prints the planned vector without
  running anything. The first release only counts and journals (no cluster part).
- **Trees, not pushes.** The daemon snapshots the worktree as a Git tree through a private index
  (tracked and untracked, never ignored files) and sends a thin pack against the node mirror under
  `refs/velnor/wip/` (seven-day expiry, secret scan on the delta); the node hashes the whole staged
  tree before step 1. A later commit, push or PR of the same tree has the same key.
- **Where it runs.** Counting commands go to a warm node of the agent's site in the `interactive`
  class; per command class, a replay of 200 recorded commands sets the route (remote by default where
  its P50 is within 1 s of the Mac's: today's Mac nextest P50 is 7.2 s [M, agent transcripts]). The
  Mac keeps a non-minting fast-red lane (incremental, changed tests first) that races the remote
  run: the first red is reported at once, and a local green is shown as provisional. The `hk`
  pre-commit hooks call `velnor check <name>`, so their keys are the gate's.
- **Trust tiers, shown to users:** T0 Mac native and T1 sealed Linux on a Mac, advice only (never a
  slot; a Mac key signs only in an advisory namespace); T2 a cluster candidate, node-signed, which may
  fill one witness slot; T3 trusted, on `main` after A1. The gate is unchanged: two T2-or-better
  witnesses per new key on distinct nodes, seeds and generations, then A1; an arm64 key never equals
  an amd64 key, so a Mac pass proves nothing to the gate and a Mac fail is a useful red. The planner
  keeps host, execution and target triples apart: a cross-built arm64 binary satisfies only its own
  released obligation. Work run early is charged once, to the claimant, in the runner-minutes.
- **Verify and ship.** `velnor verify` fills both slots on two nodes against `merge-tree(queue tip,
  tree)` and shows both SHAs; `velnor ship` runs it, pushes, labels with the agent's App identity and
  enables auto-merge, an explicit act under decision 12. The control plane derives the PR's subject
  tree itself, never from the client. `velnor wait` long-polls the verdict and returns the failing
  test id, its signature and the last log lines, and streams queue-tip moves so a stale merge base
  is visible. Any two qualified runs fill a record: a builder agent's run and its verifier
  subagent's run on another node, with distinct seeds, are its two witnesses.
- **Ledger first.** `velnor resolve` (and the wrapper, automatically) asks the ledger before running:
  a hit returns its record (44 of 107 agent sessions ran cargo on a tree before editing it [M, Opus
  transcripts]), in-flight work is joined, and the gate re-resolves everything under the current
  epoch; a stale or unreachable ledger means run fresh. A failed record on an unchanged key comes
  back as advice (`velnor why-red`: also red on `main`, a known flake, or new on this tree) unless
  the agent passes `--rerun-failed`; it never satisfies or blocks the gate. `verify` soaks new or changed test
  ids 20 times under seeds and CPU pressure at batch priority; soak runs never fill a slot.
- **The `interactive` class** sits between gate and PR work, with a per-identity budget (30 CPU-min
  an hour, then demoted to PR class), never touching the merge, gate or A1 reservations.
- **The PR verdict becomes a lookup:** the label resolves the head's own vector; hits read, misses
  and C5 run; its floor is the C5 critical path plus about 3 s [E], which falls to about 3 s on the
  helper and generator class once plan T08 injects the clock. Continuous verification of
  unpushed commits would start work no one asked for, so it is owner question D23, default off.

## 9. The cluster

- **Role.** k3s deploys the dispatchers, node agents, site services, RustFS and mirrors, and enforces
  quotas, priorities and network policy; Velnor places work, kube-scheduler places pods.
- **Sizes.** Per worker, 125.5 GiB less 6 GiB system, 2 GiB eviction margin and 8 GiB gate sandboxes
  leaves 109.5 GiB for idle and running sandboxes. A build sandbox reserves its measured peak RSS
  plus page-cache use, 24 GiB today (the heaviest job peaked at 15.7–16.8 GiB, §B.2.2), and 16 CPU
  with no CPU limit, so four fit. Nextest's threads equal the class's fixed CPU count, which is in
  the key; only the compile jobserver widens with free CPUs (16–64). A fully read target holds 14.29
  GB of page cache per clone, capping a node at six build sandboxes until lab B2. Node settings:
  reservations (3 CPU, 6 GiB), a 2 GiB eviction threshold, `podPidsLimit` 16384, XFS work volumes.
- **Reserved merge path.** One 16-CPU build slot and 2 gate CPUs per site for merge and gate work
  (about 7.7% of CPU; at 95% utilisation waiting grows about 19-fold), plus A1's slot. A group
  whose records miss is admitted with a pair of distinct warm slots, so its second witness never
  waits behind the first; restartable work may borrow the second slot. Priorities:
  merge > gate > PR = other organisations' CI > batch; shared work runs at its highest live
  claimant's priority; only batch (`cpu.idle`) is preempted. Freezing frees no memory, so at the
  memory watermark restartable batch work is terminated and requeued.
- **Sites.** Each site has its own dispatcher, ledger replica, node agents and RustFS cluster; a
  request executes in one site (DE to FI is 35 ms over WireGuard); a repository's status writer and
  cache index live in its home site. ChainArgos and jackin keep their ARC scale sets and quotas.
- **Hardware and tuning.** Two more k3s servers precede any whole-cluster dependency; a high-clock
  node (1.3–1.5× serial [E]) is trialled after the floor fixes; a worker is bought when peak CPU
  stays above 70% after dedupe or the eligible merge wait p90 exceeds 10 s for two weeks. What pays:
  gate jobs on the cluster (−5.8 to −57 min per run, §A T5), honest requests, no `-j2` caps, XFS,
  line-table debuginfo (−20 to −27% CPU, I108).

## 10. Delivery: per-job-type migration, milestones and deletions

Agents do all the work (decision 15). The [delivery plan](../research/velnor-runner-2026-11/delivery-plan.md)
(v2, decisions 1–24 and the rulings) sizes about 1272 agent-hours of building and 120 of review,
about 4.4 days for 12 builders, 4 reviewers and an orchestrator, so coding does not set the dates:
velnor-new's CI and queue, reviews, owner decisions, conformance and parity evidence, capacity, API
limits, file conflicts and, for the speed claim, the benchmark arms (run one at a time) do. Phase 0
comes first: an agent PR's cycle falls from 94 to about 34 min at P50 [E].

### 10.1 Per-job-type migration

A job type moves (nothing moves back, decisions 16, 18) when its corpus cases pass at 100% on
Kubernetes, every mutant is caught, N replayed pairs show no unexplained difference (bound 3/N at
95%), every step made to fail is red as on the oracle, and P90 start latency is no worse than ARC's.
Shadow sampling continues (decision 19); a divergence is fixed forward on a 2-hour target.

| Cohort | Job types | Pairs (N) | Saves when it moves | Deleted in the same change |
|---|---|---|---|---|
| 1 | `Required` gate, `Required verdict`, Plan, Ledger CLI, the four lints, freshness | 100–150 per type | about 15 s of acquisition and 25–40 s of set-up per gate job, on the critical path | Their ARC routes and hosted twins; Plan's tool-seed steps; two tool composites |
| 2 | The Rust crate template (26 jobs), `check-maintenance-helpers`, `check-generator-samples` | 300 across crates; 150 | about 30 s of set-up × 26 = 13 runner-min per run; the verdict's maximum 8.5 → ≤ 4 min | 26 routes; six tool composites |
| 3 | `velnor-runner-host`, its `local-mode`, `velnor-local` (the three Docker-class binaries), only what review keeps; the rest is deleted with its code at the final milestone instead of moved | 150 each | verdict P90 10.1–10.4 → ≤ 6 min | Their routes; the shared dind sidecar for velnor-new |
| 4 | State seal (then deleted), job image candidate, source qualification, `release-rust-ci-tools` and product release (Linux) | 30 each; three release dry runs per workflow | hosted queue 5.9–9.1 min per run; a release about 104 → about 1 min with promotion | Hosted release labels; the state seal and `cache-prune.yml`; `qualification.yml`, homebrew and mac-proof retired |

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
| 5.2 (7.5) | Ruleset step 4; obligations verify PRs and groups with whole-tree keys and a cold second witness; A1 replaces the full push run. **Deletes** the PR and group Actions gates and the interim label step | ≈ 6 / 18 | ≈ 9 / 30 | ≈ 110 | Parity report; A1 live |
| 6.5 (9) | Final deletion. **Deletes** ARC's velnor-new scale set and profiles, the Turso journal and the parts of `crates/velnor-runner` review did not keep. **Keeps** the official runner's image, `.velnor/runner.lock` and `velnor-official` (decisions 19, 20) | ≈ 4 / 12 | ≈ 7 / 22 | ≈ 90 | Local runs on Docker; review of a large deletion |
| 7.5 (10) | Compile reuse graduated on the helper and generator roots: second witness over another generation | ≈ 2.5 / 8 | ≈ 5 / 15 | ≈ 80 | 3,000 comparisons per root |
| 10 (13.5) | **Goal.** Per-root keys enrolled after the read audit's shadow; the test floor fixed | **≤ 90 s / ≤ 4 min** [T] | **≤ 3 / ≤ 8 min** [T] | **≤ 50** [T] | Contract review; statistical evidence |
| 16–18 | **Stretch measured:** per-package keys, `velnor verify`, longest test ≤ 15 s, the commit stamped after link | 30 s / 100 s [T] | 100 s / 4 min [T] | ≤ 20 [T] | Package enrolment; labs B and C |

A missed milestone holds the next cohort until explained. Items are 2–8 agent-hours, one PR under
400 refactored lines; every replacing PR deletes what it replaces.

### 10.3 Lab experiments and their safe defaults (each correct whichever way its lab goes)

| Lab | What it runs | Pass | Default until then |
|---|---|---|---|
| A. GitHub semantics | A lab repository with a ruleset, a merge queue and a test App: no status; App `pending`; App `success`, then one more push; the context from a user token and from `GITHUB_TOKEN`; a status only on the test-merge SHA; a required Actions job with `if: false`; a skipped caller whose inner job is required; a dispatched run; two PRs at one SHA; queue entries with no report, `failure` and `success`; labels changed with `GITHUB_TOKEN`, an App token and a user token; the state-machine cases of section 8.2 | The no-status, pending, foreign-source and skipped-caller cases block; App `success` merges until the next push; a queue entry lands only on `success`; the read-back shows the App context required on both subjects | Today's gate, hardened and bound to 15368; the App only observes |
| B. Build matrix | One idle worker, three repetitions: B1 dependency `opt-level` 0 and 3 against assertions on and off, whole suite; B2 reflink against a read-only overlayfs lower layer with 1, 4 and 6 sandboxes, page cache per additional sandbox, bytes copied up per build (cargo's hard links copy up), the canaries of section 7.3, and digests against a cold build (a lower layer never replaces the full tree hash); B5 the narrowed step 3 over 300 diffs that add or remove a path plus planted cases (`foo/mod.rs` beside `foo.rs`, a new `tests/x.rs`, a file a build script globs or a proc macro lists, a new `clippy.toml`, `rustfmt.toml` or `.cargo/config.toml` below the root); B3 seeding, lockfile changes and the wait-or-fetch-or-cold crossover under load, by `mbx-cache`, file-delta sync, compressed layers and a signed chunk manifest of chunks ≥ 4 MiB; B4 incremental on and off over 300 replayed diffs and 20 planted failing edits | B2: every canary rebuilds and digests match; B5: every planted case rebuilds and every digest equals cold; B3: a transport beats a cold build on more than 10% of requests; B4: zero differences | Reflink; cold second witness; `CARGO_INCREMENTAL=0` for receipts; the `opt-level` override with checks on, its gain uncounted |
| C. Shadow ledger | Beside production for 300 merges: C1 whole-tree and per-obligation hits of each merge group against the PR's records; C2 twins on two nodes against two runs 30 min apart, and an agent's run at t0 against the label's at t1, on 20 tests with seeded failure probabilities of 0.05–0.2 | C2: joint pass rates match (1−p)² | Groups derive their own vectors and execute every miss |
| D. Docker class | The three Docker-needing binaries, 200 runs each, under a private privileged daemon, a rootless daemon and a microVM, on both backends | Equal pass, fail and skip sets; nothing one request leaves is visible to the next | A private privileged daemon per request, or refusal |

## 11. Proving the speed

Nothing has run end to end, and no comparator has raced this repository, so the speed claim waits
on this suite (decision log, fastest-runner critiques K01–K27).

**The contract is frozen first** (K20): workload revisions, expected test IDs, features, Git and
toolchain versions, timeouts, the released planner and both exact subjects are pinned; every arm
must agree with an independent cold oracle on every contracted case, and unknown inputs force fresh
work.

**Workload.** 300 historical velnor-new PR heads replayed at their SHAs and arrival times, every
class unweighted (the class mix from the daily report), plus the 2026-11-18 arrival trace; the same
released inventory, expected test IDs, features, toolchain and contract for every arm.
**Resource budget.** Each self-hosted arm gets the same two FI workers (EPYC 9454P, 125.5 GiB, XFS
NVMe) exclusively, with ChainArgos and jackin load replayed beside them in the loaded runs; rented
arms get at most the same vCPU concurrency, and every arm reports CPU-seconds, GiB-minutes, bytes
and cost.

| # | Benchmark (K ids) | Setup | Pass |
|---|---|---|---|
| S1 | Planted failures (K07) | 100 failing edits: the Q16 older-time edit, an undeclared build-script input, a new sibling file, a clock-shifted test, a stale generation, flaky tests at p = 0.05–0.2, on every fast path | 100% red, or detected at the measured (1−p)² rate; it gates every other run |
| S2 | **Control race** (K01, K26) | Velnor Native against GitHub Official on the same warm workers, mirrors, tool store and generation, separate executor keys; ≥ 30 interleaved pairs per job type at 15% and 80% CPU; the same race on today's ARC with phase 0 applied | Per-job-type deltas with intervals; an executor claim only at ≥ 5 s per job at P50 on ≥ 3 job types |
| S3 | **Head-to-head** (K02, K03, K25) | Arms: today's CI; **a strong conventional baseline** on the same workers (the official runner on persistent agents with a reflink-cloned warm target, crates mirror, baked tool image, gates on the cluster, auto-merge, ALLGREEN); Velnor with Official; Velnor Native; rented arms per owner question D22 | The publication rule below, on push-to-merge gross and net and on the verdict; every stage timed, `ensure` included |
| S4 | Loaded replay (K04, K21) | The 2026-11-18 trace with other organisations' load, A1 and release preparation at production priority; then the reversal cases: a root or manifest edit, a cold toolchain, a lost worker, an ejected predecessor | Verdict P90 ≤ 1.5× quiet; merge-path wait P90 ≤ 10 s |
| S5 | Census (K06, D02) | C5 classifier and read audit on `main` and the last 300 heads; a content-key would-hit counter (equal binary digests under a different source key), telemetry only | Shares of C5 obligation-seconds and of heads with whole-tree keys published; goals restated from them |
| S6 | Warm compile (K05) | 300 replayed diffs per root over a clone, cold page cache, idle and 70–90% load, with a cold control; heads that add a source file reported separately | Slower witness P90 ≤ 25 s for helper and generator, ≤ 40 s loaded; digests equal cold |

**Publication rule** (K22, pre-registered). Velnor is "fastest" against an arm only if paired
arrival-block bootstrap intervals for the P50 and the P90 difference, at simultaneous 95%, lie
strictly below zero, per class; each self-hosted pair is interleaved at least five times and
reported as median and range. A tie, an unrun arm, a lost class or a win only in the light class
narrows the sentence, and any false green or skipped work disqualifies the arm or configuration.
No claim is made against an arm S3 has not run. The strong baseline (BM07) stands in for
persistent agents such as Buildkite's and keeps the Actions hop, so it bounds them on stages 3–9
only; "Velnor with Official" is the same-ledger, other-agent case. A rented arm runs with the
vendor's recommended caches and phase 0 applied, or it is another handicapped baseline. Against
Bazel and Buck2 remote execution the sentence excludes cold and wide builds unless BM10 ran; BM10
runs only if one `cargo build --timings` of the cold runner root shows its longest dependency chain
under 70% of the wall, and reports a subgraph result.

Component labs carry the rest: twin independence (K08), group reuse and GitHub's share (K09, K10),
hardware (K11), floor inventory (K12), page cache (K13), overhead (K14), graduation (K15), storage
(K16, K27), sandbox lifecycle (K17), toolchain matrix (K18), batching (K19), release timing (K23)
and cost (K24); the plan's BM items own them.

## 12. Top risks

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
| RustFS is young (1.0.0 two months old, a patch about every 10 days) and has open issues on node return (#9480), replication backfill (#8768) and small PUTs (#9161) | The phase-1 cluster test; per-tier hit and error rates; replication lag | Routes stay `local` until the test passes; a pinned version upgraded through the storage kit; any S3 store can replace it; a stalled or degraded tier is a miss |
| Stale compile state | Content-proven inputs; the cold second witness; A1 digests; canaries | Retire the generation; build cold |
| The cluster is saturated | Per-class CPU and memory watermarks | Batch work preempted or requeued first; reserved merge path; lab replay at night and on Docker hosts outside the cluster |
| The speed claim is published before it is measured | Section 11's pre-registered rule | The claim stays "designed to cut push-to-merge" until S1–S6 pass |
| Owner decisions wait; agents collide | Due hours; conflict counts | One decision pack with defaults; one owner per crate and shared file (delivery plan §8) |

## 13. Owner questions

Only what is open; the plan runs on each default until the answer arrives. Decided or ruled: the
old runner's PRs parked (decision 16), the reference executor (19), two witnesses and ALLGREEN (D10),
where release bytes are built (D12).

| # | Question | Example | Speed gained | Recommendation and default | Needed by |
|---|---|---|---|---|---|
| D1 | Release the prepared engine version that moves the gate jobs off hosted runners, and pre-approve CI-only engine releases (D9)? | Eight gate jobs still run on `ubuntu-26.04` and waited 57 of a 71.8 min run on the worst day | — | Yes, now, one approval for the batch | hour 0 |
| D2 | Should agents enable auto-merge and stop merging `main` into their branches? | 0 of 40 recent PRs used auto-merge; the median waited 7.5 min between green and enqueue | — | Yes | hour 0 |
| D3 | Apply the first ruleset update now: two contexts bound to 15368, empty `bypass_actors`, ALLGREEN, depth 2, timeout 90, minimum wait 0? | An admin bypass carried 7 of 136 merges in three days, and three commits with a failed `Required` sit on `main` | — | Yes; depth 4 and timeout 15 min only once a group verdict takes ≤ 5 min at P90 and paired false failures are ≤ 3% | hour 2 |
| D4 | Which evaluator ships: A, velnor-legacy's port of the legacy `Expressions2`, or B, a port of the new `Sdk/Expressions` and `WorkflowParser`? | v2.338.2 computes the new evaluator in compare mode but returns the legacy result; Velnor refuses every job where the two differ, so on accepted jobs A and B give the same answer | — | Recommend B: it complies with decision 3 and ruling Q4, and correctness is equal because differing jobs are refused. Default A until you answer, because it matches the latest runner and costs 3 h against 28; only one ships | day 3 |
| D7 | Create the `velnor-verify` App (section 8.2's permissions and webhooks) and an agent identity without repository administration? | Today agents label and merge under the owner's identity, which can edit the ruleset | — | Yes; its status becomes the gate after lab A and parity | day 2 |
| D8 | A deploy identity for `velnor-system`, and the node changes (XFS work directories, reservations, taints, and a rebuild of each FI worker's data volume to give RustFS its own LV)? | Job directories sit on the ext4 OS disk, where a warm target copy costs 8.6 s and 10.75 GiB | — | Yes | hour 6 |
| D11 | The per-type cutover counts of section 10.1? | Cohort 1 needs 150 replayed pairs per type | — | Yes, as written | day 1 |
| D14 | Delete the engine crates from velnor-new once velnor-gen's workflow replaces the engine's CI? | The engine's orchestrator and renderer are about 275k lines | — | Only after checking for other consumers | day 8 |
| D15 | Approve the recovery path for a bad Velnor release, with no admin merge? | A release that fails every job also blocks the PR that fixes it | — | Yes: redeploy the previous image digest; if none can verify, the pre-approved break-glass verifier writes a green only after two full cold passes on two nodes (section 5.9) | day 1 |
| D16 | Does a `velnor:verify` label added while the PR was a draft start verification when it becomes ready? | An agent opens a draft, labels it early, then marks it ready; today nothing would run until it re-labels or pushes | — | Recommend yes: the label states intent and the reconciler can read it from state. Default no, the literal reading of decision 12's step 2 | day 2 |
| D23 | May `velnord` verify an agent's committed, unpushed work automatically (no explicit `verify`), at batch priority, opt-in per worktree? Decision 12 starts verification only by the label, and an agent's explicit request | An agent commits, keeps editing for two minutes, then ships; today nothing runs until it asks | First red at commit plus 1–2 min; label about 35 s → 3–5 s [E]; at least 70% of commits are superseded, about 21 runner-hours a day at the goal, 265 with whole-tree keys [E, Fable] | Not yet: explicit `verify` and `ship` first; ask again with a month's commit rate, cost per head and cluster slack. Default off | day 10 |
| D24 | May the agents' `interactive` class outrank PR-class work, including other organisations' CI? Section 9 ranks other organisations' CI equal to PR work | Ten agents each run a 10 s test while ChainArgos' PRs fill the cluster | Keeps an agent's loop near the Mac's 7.2 s P50 under load [M, E] | Yes, within a per-identity budget (30 CPU-min an hour) and never above merge, gate or A1. Default: equal to PR work | day 6 |
| D25 | May agents work inside cluster sandboxes (a sticky, non-minting shell beside the mirror and generations), or on a dedicated agent worker? | An agent's build waits on a 61 s Mac edit-build when a warm node has the generation | The whole local-to-remote gap; a dedicated worker about $200 a month [E, Carbon] | A two-agent pilot after `verify` ships; the shell never mints. Default no | day 10 |
| D26 | Should a native arm64 lane exist for the shipped aarch64 binaries (Hetzner Ampere nodes, or a `linux/arm64` worker), additive only? Decision 9 pauses macOS; aarch64 musl bytes get only a qemu smoke test | A memory-ordering bug that shows only on arm64 hardware | No verdict time; arm64 coverage natively | Not now; an advisory lane when aarch64 releases matter. Default nothing built | day 10 |
| D17 | Who may approve a reduction (an obligation or test removed from the inventory, or quarantined by owner exception)? | A PR deletes a slow test file; the head plan lists fewer tests than `main`'s | — | Recommend: only the owner, through a signed inventory entry. Default: no reduction is accepted | day 3 |
| D18 | May Velnor land merges itself (a merge train that fast-forwards `main` by compare-and-swap after verifying the exact resulting tree), instead of GitHub's merge queue? Decision 7 keeps GitHub's queue | A green PR waits 19 s for GitHub to form its group and 26 s to merge it, even when the group's tree was verified seconds earlier | About 45 s per merge [M: medians of 19 s and 26 s], about half the 85 s goal estimate, more in bursts | Not now. Measure GitHub's share on 50–200 auto-merged PRs and shadow a compare-and-swap train for tree-match and latency; bring the numbers back before changing decision 7 | day 8 |
| D19 | May Velnor compile and test a ready PR's pushes before the label, at idle priority, so the label finds the work done? Decision 12 says an unlabelled push starts nothing | An agent pushes, then labels 2 min later; today the label starts a 15–25 s compile and the tests | About 15–25 s off label-to-verdict [E], for about 5–10 runner-min per speculated head, 45% of which are superseded | No: keep decision 12. Use pre-push `velnor verify` (decision 10c) instead, which gives the same lookup when the agent asks for it | day 3 |
| D20 | Rent high-clock nodes (Ryzen 9950X or EPYC 4564P class) now? A green waits for the slower of two witnesses on distinct nodes, so one node per site shortens only reds and A1 | The workers are EPYC 9454P throughput parts; one rustc front end and the longest test run on one core | 1.3–1.5× on the compile and longest-test chain [E, I104]: helper class about 25 → 18 s, only with two such nodes in the site | Rent one now for BM15's ratio, which also says whether both CPU models share one fingerprint; two per site only at a ratio ≥ 1.15. Default: no rental | day 4 |
| D21 | After compile reuse graduates, may one compile feed both test witnesses (the second runs the first's digest-bound binary on another node)? | Today each witness compiles its own binary, so a stale build on one node is cross-checked by the other | No wall time; about 3 runner-min per merged PR | No: the second compile is the cross-check the two-witness rule and ruling Q12 rely on | day 7 |
| D22 | May the benchmark rent comparator arms (one of Namespace, Blacksmith or Depot, and GitHub larger runners) and run velnor-new's replayed heads on them? | Without a rented arm, "fastest" can only mean faster than our own baselines | None directly; it decides how wide the claim can be | Yes, one rented arm and GitHub larger runners, measurement only, for about a week; nothing they produce counts as a verdict. Default: no rented arm, and the claim names only the arms run | day 5 |

# Velnor runner architecture

**Status:** Proposal. It follows the owner's binding decisions 1–24 (section 6.1) and the
[rulings](../research/velnor-runner-2026-11/velnor-gen-rebase/rulings.md) R1–R13, which put the
design on top of velnor-gen. Phase 0 (section 3) changes only the generator, the helper and their
configuration. The runner side (sections 6 and 8) conflicts with [`docs/goals.md`](../goals.md)
(section 6.2); none of it starts before the owner answers question Q1. Owner questions use the ids
of the [owner sheet](../research/velnor-runner-2026-11/owner-questions.md).

**Scope.** How Velnor verifies, merges and releases velnor-new's changes, so that CI stops being the
slowest part of delivering a change. velnor-gen is the verification compiler: it decides what must
pass. The helper (`velnor-ci`) proves it at run time. The runner executes what the generator wrote.

**Sources.** The as-built reports for the
[generator](../research/velnor-runner-2026-11/velnor-gen-rebase/as-built/generator-as-built.md) and
the [runner](../research/velnor-runner-2026-11/velnor-gen-rebase/as-built/runner-as-built.md), the
[plan audit](../research/velnor-runner-2026-11/velnor-gen-rebase/as-built/plan-audit.md), the
[measurements](../research/velnor-runner-2026-11/velnor-gen-rebase/as-built/measurements.md) and
the [comparison](../research/velnor-runner-2026-11/velnor-gen-rebase/comparison.md); the
[evidence pack](../research/velnor-runner-2026-11/evidence.md) for cluster and protocol facts; the
experiments under `../research/velnor-runner-2026-11/experiments/`; and the
[decision log](../research/velnor-runner-2026-11/decisions.md). Numbers are tagged **[M]** measured,
**[M, snapshot]** measured once on the cluster and not verifiable from the repository, **[M, proxy]**
measured on a stand-in that the row names, **[E]** estimated (with the arithmetic), **[C]** read from
code, or **[T]** target. A target is a `docs/goals.md` target, a proposed target or a budget (section
1.4). No target counts as met until traces under load support it.

## 1. Goal, metric and the contract

### 1.1 The metric

The single metric is **push-to-merge**, from the final labelled push to the merge. **PR verdict**
runs from the label or a labelled push to a green `Required` (later `Velnor / Verified`, section
9.3). Beside them, each on its own clock: push-to-green, time to the first actionable red, enqueue
time, queue residence, post-green landing, label wait and failed attempts per merged change; weekly
P50 and P90 with intervals, per PR class, compared only on identical cohorts. Runner-minutes count
each occupied slot once and split completed, cancelled and superseded work. Push-to-merge is
reported gross and net of GitHub's own queue share, which no runner controls.

**The claim.** Until section 12's suite passes, the claim is only: *designed to cut velnor-new's
push-to-merge*. After it passes: *the fastest measured push-to-merge on a specified velnor-new
workload, at a stated resource budget, with the same verification contract*. The speed belongs to
the verification system (the contract, the reuse ladder, the label flow, warm state). An executor
claim needs its own race (S2). Every request is one OpenTelemetry trace; nothing in the gate reads
it.

**Where things stand.** The measured state of the generated workflow on `main` (what is red, what
merged without tests, what a push job costs) is dated evidence. It is kept, each number with its run,
in the [delivery plan](../research/velnor-runner-2026-11/delivery-plan.md) (section 3) and the
measurements report. This document holds the contract. The baseline is phase-0 item 8 (section 3).

### 1.2 Same confidence: the contract

Correctness is the hard constraint, and speed is optimised only inside it (decision 11). Anything
that could turn a real failure green is rejected, however fast. False reds have a budget; false
greens have zero tolerance. "Same confidence" is this contract.

1. **C1. Every landed commit is verified.** For every commit on `main` and every obligation the
   verification contract lists for it, a passing result exists whose key equals the obligation's
   key at that commit. An *obligation* is one unit: format, lint, docs, a per-crate check, one test
   binary under one configuration, release bytes, or a named check. An obligation due before landing
   has its result before the commit lands. One the owner defers until after landing (Q13) has it
   before the next release, and its failure turns `main` red. The queue uses ALLGREEN.
   HEADGREEN is unsound with diff selection: each queue entry is tested only on its own diff, so a
   later group's pass does not cover an earlier entry (section 9.4).
2. **C2. Keys capture every input, by construction:** sources, manifests, toolchain, dependency
   versions, environment, flags, features, the binaries a test executes (`CARGO_BIN_EXE_*`), data
   files, the execution fingerprint and the executor (section 7.2). Every obligation runs on the full
   tree: a trimmed sandbox turns a real failure green [M, reproduced]. **Keys are the whole tree**
   until an enrolled contract narrows one. A read trace never proves a key complete: it cannot see an
   untaken branch, clock reads answered in the vDSO, or run-to-run variation (a `HashMap` seed chose
   another fixture, so a trace-keyed lookup passed a tree where 12 of 40 fresh runs fail [M]).
3. **C3. Selection is sound or wide.** The plan comes from a released helper: the pin in
   `.velnor/rust-ci.lock`, installed by commit SHA through `actions/setup-velnor`, with the binary
   digest checked on every call [C]. What the gate demands is checked against the base branch by a
   producer the candidate cannot change (section 9.3).
   A file the plan cannot place selects everything (`AllTrigger::UnknownPath`). Each obligation
   carries its exact expected test IDs; a missing, duplicate or extra ID fails. A head that drops an
   obligation or a test that `main` has fails, unless the owner signed that reduction.
4. **C4. Only what Velnor observed is evidence.** In phase 0 the evidence is the generated job's own
   outputs (`tests_run`, `tests_listed`, `tests_by_package`), checked by `velnor_rust::decide`; a test
   step that ran no tests fails. Once the node agent exists, a receipt comes only from processes it
   started in a sandbox it created, running the generated command on inputs it mounted. **It binds
   the digest of every binary executed**, the generation compiled over (or "cold"), the node, the
   slot and the epoch. A missing, skipped, cancelled, timed-out, masked or zero-test result fails.
   Results never cross executors; a reference run never yields a verdict, cache or release.
5. **C5. Uncomparable work runs on every request.** Obligations that read the network, the event,
   the wall clock, unseeded randomness, unpinned images or the commit's identity, or that publish,
   run fresh on every request, unless the varying input is fixed and keyed. Tests get a
   loopback-only network and no event file; sandboxes get no `.git` unless a census shows a test
   needs it (it then gets a read-only repository keyed by the commit), a scrubbed environment and a
   fixed CPU count.
6. **C6. Reuse is audited, compile reuse included.** A new reuse class runs in shadow, every reuse
   re-executed, until about 3,000 clean comparisons per obligation kind (a bound near 0.1% at 95%,
   never zero), then 2%. Until compile reuse graduates, the second witness builds cold. Audit A1
   re-checks every landed commit (section 7.6). The first disagreement suspends the class and its
   dependants; five re-runs decide between a flake and a wrong key.
7. **C7. Flakes are budgeted** (section 1.3).

**Two independent witnesses.** Every new key needs two passing witnesses, in a PR or a group: two
nodes, two attempts, different test orders, and two verified generations or one cold build. A
witness fills one slot of one record. Any failure blocks, with no retry. A1 is the third execution.
A regression that fails with probability p is caught before merge with probability 1−(1−p)². If lab C
finds the twins correlated, the group re-runs the PR's changed obligations even on an equal tree.
Witnesses need the node agent (after Q1). Until then, reuse stays on rungs 1–3 of the ladder (section
7.1), which reuse whole results of the same commit or tree and need no witness pairing.

### 1.3 Flake budget

A *false failure* is a required run that fails on inputs where an immediate rerun passes. It costs a
rerun or a queue ejection, and every queued entry behind an ejection is rebuilt.

- **Budget:** per verification record, at most 3% false failures after phase 0 and 1% at the proposed targets,
  so each witness may fail falsely about half as often. Each is claimed only after about 300 rerun
  pairs sampled under load (zero disagreements in 299 bound the rate just under 1% at 95%). "Not
  requested" never counts. The rate on the generated workflow is unmeasured; phase-0 item 8 measures
  it.
- **Act:** retries stay at 0, so a flaky test fails and is counted. Two false failures of one test in
  seven days open an issue automatically. A test is fixed, or quarantined only when an equivalent
  blocking check is shown to catch the same planted fault. A quarantined test keeps running in a
  non-blocking lane and returns to blocking when its quarantine expires. An owner exception is an
  owner-signed reduction.
- **Known causes first:** CPU starvation of tight timeouts [M, snapshot]; the `event.json` rewrite
  race between steps. Obligations run the `ci` nextest profile, so its 2 s kill does not apply.

### 1.4 Targets: goals and budgets

Targets are budgets until measured (R9).

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 |
|---|---|---|
| Proposed target [T] (Q18) | ≤ 90 s / ≤ 4 min | ≤ 3 / ≤ 8 min |
| Trial budget after phase 0 [T] | about 4 / 9 min | about 9 / 20 min |
| Baseline | Phase-0 item 8 | Phase-0 item 8 |

- **`docs/goals.md` sets the binding targets:** a run that selects no crate finishes within 1 min; at
  most 30 s from job start to the first cargo command; the generator's tests under 1 min; an ordinary
  PR's wait and runner-minutes do not get worse, except for a check correctness needs, with its cost
  measured and accepted by the owner. The baseline measures these. The proposed targets in the table
  come from an earlier design round and are not in that file (Q18).
- No dated target is published before a green, test-running baseline exists. The baseline replaces
  the trial budgets.
- The "fastest runner" claim waits for the S1–S6 suite (section 12), which needs frozen contracts
  and two executors.
- **A helper-pin regression check** is separate and earlier: a pin bump replays recent commits on the
  current pin and on the candidate, and is blocked when the candidate is slower by more than a
  threshold fixed in advance from the baseline's run-to-run spread.

- Further proposed targets [T], after Q1: job start ≤ 1 s P50 and ≤ 2 s P90 from the broker message; no test over
  60 s alone; per release, zero compiles and about 15 s of publication.

## 2. Built on velnor-gen

velnor-gen is the verification compiler (R1). From `.velnor/rust-ci.toml`, `.velnor/workflows.toml`
and the recorded metadata, it writes every generated workflow and, in this design, the
**verification contract** beside it. The helper resolves the contract at run time. The runner
executes what the generator wrote; it never plans.

### 2.1 What the design builds on

All of this exists on `main` [C]:

- **One root workspace.** The crates sit flat under `crates/velnor-*`. New code goes there too. There
  is no second Cargo root. `generator/` and `ci-helper/` hold only documentation and samples.
- **Generated workflows.** velnor-gen writes `rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml`,
  `tag-ruleset-drift.yml` and `release-rust-ci-tools.yml`. Each carries a digest header
  (`velnor-render` `header.rs`, `seal`), which no CI step checks until phase-0 item 3. `ci.yml` (four
  lint jobs) is the last hand-written workflow; phase 0 deletes it.
- **The dialect.** Every generated `run:` step is one command, usually `velnor-ci …`. Long shell
  lives in launchers carried by environment variables. A workflow uses only the 16 entries of
  `ALLOWED_ACTIONS` (`velnor-render/src/rules.rs`), each pinned to a commit SHA
  (`pinned_actions.rs`).
- **A released helper.** `.velnor/rust-ci.lock` pins `velnor-ci` and `velnor-gen` by version, commit
  and digest. The lock is part of the candidate tree, so a pull request can pin another release; the
  trusted verifier treats an older pin as a reduction (section 9.3).
- **Selection.** `velnor_rust::select` and `select_event` read `.velnor/rust-metadata.json`, run no
  cargo at plan time, and fail closed to "everything" (`RustSelection`, `AllTrigger`,
  `SelectionReason` in `velnor-model`).
- **The gate.** The `Required` job runs `velnor-ci gate`, which calls `velnor_rust::decide` over the
  outputs of the jobs it needs, including the expected test count (`required_job.rs`,
  `tests_expected`).
- **Reuse below the verdict.** Cargo fingerprints with restored file times, the per-PR delta layer,
  the mbx store (`mbx_shared_store = true`), the crates mirror (`[crates_mirror]`) and `tools-local`.
  Only trusted events write trusted stores (`mbx_store::trusted_event`); a PR writes only its own
  delta layer.
- **One prior-run lookup.** `velnor-ci merge-group-passed`, switched by the config key
  `ci.skip_push_after_merge_group` and planned through `Step.skip_after_merge_group:
  Option<PlanFrom>`. It answers yes or no for a whole queue run and is off in this repository;
  phase-0 item 7 replaces it with evidence per obligation.

### 2.2 Where each design component lives

"Proposed" marks a name that does not exist yet.

| Component | Generator time | Run time | Runner side | When |
|---|---|---|---|---|
| Verification contract: one obligation per thing that must pass, with id, key recipe, reuse class and when it is due | Proposed `velnor-model` types `VerificationContract`, `ObligationSpec`, `ObligationId`, `ReuseClass`, `KeyRecipe`, beside `CacheKey` and `VerdictSpec`; built by `velnor_rust::plan`; written by velnor-render as the sealed `.velnor/verify.lock` (proposed) with the same digest header as `rust-ci.yml` | `velnor-ci plan` writes the resolved keys as outputs | The node agent reads the same lock | Phase 0, item 10 |
| Key, definition half | Fixed in the lock: workflow digest, helper digest, toolchain, profile | — | — | Phase 0, item 10 |
| Key, input half | — | `velnor-ci plan` resolves tree, packages and image. A test-run key that depends on built bytes waits until the build has produced them | The node agent binds the digests it observes | Phase 0, item 10 |
| Selection and disposition | `RustSelection`, `AllTrigger`, `SelectionReason`; the `[[inputs]]` config key | Each obligation gets a disposition, proposed `Disposition::{Execute, Reused(receipt), Excluded(reason)}`. A reuse hit never turns a package into "not selected", so the gate can always say why something was required and whether it was proven | — | Selection exists; disposition after phase 0 |
| Label flow | `pr_ci = "label"` (`PrCi::Label`), `PrRun`, `PullRequestType` (gains proposed `Unlabeled`), `StepKind::FailOnDraftRun` and `DraftNotice`; `ClearRunLabel`, `CI_RUN_LABEL` and `CI_AUTO_LABEL` deleted | `Required` fails "CI not requested" on a draft, on an unlabelled PR and after the label is removed | The `velnor-verify` App, later | Phase 0, items 0 and 2 |
| Gate | `VerdictSpec`, `GateNeed`, `required_job.rs` | `velnor-ci gate` → `velnor_rust::decide`, extended to accept a reuse receipt only when its key matches exactly | `Velnor / Verified`, later (section 9.3) | Exists; extension after phase 0 |
| Reduction check | The contract, emitted on both trees | In the workflow, the gate compares the head contract with the base contract through `velnor_rust::weaken` and fails on an unsigned reduction: an accident guard. From `main`, the trusted `workflow_run` verifier repeats the comparison and writes `Velnor / Verified` | The App, later | Phase 0, item 10 and the verifier |
| Generator freshness | `StepKind::GeneratorCheck` (defined, never planned) planned for every managed workflow; proposed `GateNeed::Generated` | Runs `velnor-gen check` from the pinned release, as a `velnor-ci` subcommand | — | Phase 0, item 3 |
| Helper contract | `velnor-gen` `helper_gate.rs` checks feature minima; proposed: it also reads the pinned helper's command schema and refuses to render a command or flag the pin cannot parse. End-to-end tests render, plan and preflight with the pinned helper | — | — | Phase 0, item 1 |
| Lint lane | `[[lint]]` (`config_lint.rs`, `lint_jobs.rs`) takes over the `ci.yml` lint jobs | Lint jobs join `Required`'s needs | — | Phase 0, item 4 |
| Reuse points | `Step.skip_after_merge_group: Option<PlanFrom>`, generalised to proposed `Step.reuse: Option<ReusePoint>` with `SameSha`, `QueuePassed` and `SameTree` | `velnor-ci merge-group-passed`; proposed verdict lookup and record over GitHub cache entries | A ledger service only on measured need | Rung 2 in phase 0; rungs 1 and 3 after. Every rung after Q8 |
| Release gate | The tools release workflow, generated from `.velnor/workflows.toml` | `velnor-ci release-gate`, fail closed | — | Phase 0, item 5 |
| Branch ruleset | Proposed: a generated branch ruleset and drift check, as `velnor-gen release ruleset` and `release ruleset-drift` already do for the tag rulesets | — | — | Phase 0, item 6 |
| Closed action set | `ALLOWED_ACTIONS`, `pinned_actions.rs`, the one-command rule | — | Velnor Native implements exactly this dialect (section 6.7) | Exists; Native after Q1 |
| Metrics and budgets | The `[budget]` key, `shape = auto`, `.velnor/rust-ci-timings.json` (absent; phase-0 item 8 writes it) | `velnor-ci report`, `bench`, `timings` | Traces from the node agent | Partly exists |
| Executors, platform, storage, mirrors, node agent, App | — | — | Proposed crates under `crates/velnor-*` (section 6.3) | After Q1 (R6, R7) |

The contract is the obligation inventory of this design. Gate parity is therefore a generator
property: the contract and `Required`'s needs come from the same plan, so a check cannot be in one
and missing from the other.

## 3. Phase 0: correctness first

Correctness is the gap, not speed (R2). Nothing reaches the ruleset until `Required` has been green
on `main` **with tests running**. A red `main` has had several causes; what let each of them land is
one condition, a change merged without a green, test-running `Required`. Phase 0 removes it first.

Phase 0 starts when the owner freezes the admin bypass to repairs of `main` (Q3). The item numbers
are the delivery plan's (item 6 is W6); the trusted verifier is N26. The order of work is 0, 1, 2, 4,
3, 5, 6 (step 1), 8, 9, 10, 7, the verifier, 6 (step 2); the delivery plan has the dependency graph.
Item 7 follows item 10 because it reads the contract's keys. Item 3 follows item 4 because deleting
`.velnor/config.toml` makes the generator check report `ci.yml` as an unmarked file. Each item becomes
a GitHub issue whose "done when" checks state on `main`, so work that another session lands first
closes the item instead of being planned again. The owner confirms the plan before any issue is
created (R12).

| # | Item | Done when |
|---|---|---|
| 0 | **No green without tests.** A draft and an unlabelled ready PR get a red `Required` at once. The existing run labels stay until item 2 | A draft push and an unlabelled ready PR are red in the lab |
| 1 | **Green main, and tests that keep it green.** velnor-gen reads the pinned helper's command schema and refuses to generate a step the pin cannot parse. End-to-end tests render, plan and preflight with the pinned helper and nextest, and a manifest edit without regeneration fails a test | Three consecutive push runs are green with tests run and save a target cache; each test was seen red on the fault and green on the fix |
| 2 | **An honest `Required`** (section 9.2; Q2) | The lab cases of section 9.2 pass; the old run labels, the quick set and the `ci-run-label` job are gone |
| 3 | **Generator freshness in the gate.** Plan `StepKind::GeneratorCheck`, run it from the pinned release, make `Required` depend on it. Delete `.velnor/config.toml` and the helper commands only the deleted engine used (`plan --mode`, `write-request`, `resolve-qualification`, the `image` group) | The check passes on `main`; a hand edit of a generated workflow, a manifest edit without regeneration, or an unmarked workflow fails `Required` |
| 4 | **One lint lane.** Alint and Jactionlint become `[[lint]]` entries, `cargo deny` and `cargo machete` stay only in `policy`, and `ci.yml` is deleted. The advisory scan runs on every push to `main` and every night, whatever changed | Every former `ci.yml` failure has a failing counterpart; `ci.yml` is gone |
| 5 | **Release gate.** The generated tools release requires the green push run of the tagged commit: the workflow file, the exact commit, the latest attempt (`velnor-ci release-gate`, fail closed) | On a lab tag: a red push blocks; a green push passes; a skipped run, another workflow's job of the same name or an older attempt blocks |
| 6 | **Ruleset (the owner applies it; Q3, Q4, D7).** Step 1: bind `Required` to the GitHub Actions app, remove the bypass, switch the queue to ALLGREEN, keep every other rule; auto-merge is a separate setting. Step 2, after the verifier: also require `Velnor / Verified`. A generated drift check fails if any of it is undone | The read-back matches field by field; a red canary cannot merge; three PRs land through `merge_group`; the lab shows two overlapping entries under ALLGREEN |
| 7 | **Decide what blocks landing** (Q13), then let the push after a queue merge reuse the queue's result per obligation (rung 2; Q8) | The reuse tests of section 7.5 pass; a shadow run over twenty queue merges shows at least one valid would-hit and no mismatch |
| 8 | **The first green baseline**, measured with push reuse off, before any new speed target | Cohorts by event, cache state and selection; a percentile is reported only with enough runs; timings and `[budget]` committed; every [E] figure in this document replaced or removed |
| 9 | **Speed quick wins and selection hygiene** (section 4) | A replay of 400 commits reports the unplaced-path count and the select-everything count apart; an edit to a path goals rule 2 names still selects everything; the wall time saved against item 8 is posted |
| 10 | **The contract in shadow, and the reduction check** (section 9.3) | For 200 runs, no two results with equal keys differ; a PR that drops a test or an obligation through configuration is red unless the owner signed that reduction |
| N26 | **The trusted verifier** (section 9.3) | In the lab, a PR that deletes the check step, adds a second `Required` job or pins an older helper cannot merge |

Generator and helper changes reach CI only through a tools release and a re-pin (goals rule 12), so
the delivery plan groups the items into release trains and names the owner's part in each (D9).

**No runner, platform or storage crates in phase 0** (R7). The `velnor-runner-*` crates get
correctness fixes only. New runner features wait for Q1 and Q9.

## 4. Where the time goes

The measurements, each with its run, are in the delivery plan (section 3). No labelled PR with tests
has passed on the generated workflow, so test time is unknown until phase-0 item 8. The causes:

- **A red main makes runs cold.** A push saves the target cache only when it is green.
- **The same tree is verified up to three times:** on the PR, in the queue and on the push.
- **The post-merge tail runs in series in one job:** 18 alone-checks, docs and the lane guard.
- **Unplaced paths select everything.** Many commits touch one, mostly under `generator/samples/**`
  and `local/**`.
- **Small waste:** rows 8, 9 and 14 of section 5, and `policy` waiting for `rust`.

**Levers, in order** (R8):

1. A green main (phase-0 items 0 and 1).
2. Stop verifying the same tree three times (the reuse ladder, section 7.1). Rung 2 saves lint and
   test time; builds and cache saves still run.
3. Decide where the post-merge tail runs (Q13). Steps of one job run in series, so moving the tail
   into the queue delays every group by its run time unless it goes into a second job, which needs a
   measured gain (Q7). Alone-checks run only for crates that are released.
4. Map the paths the plan cannot place with `[[inputs]]` entries, only where a path has a proven
   owner. The paths goals rule 2 names (the workflow, the toolchain file, cargo configuration) and
   the `.velnor/` configuration and locks always select everything; a mapping that covers one is a
   configuration error.
5. Cheap measured wins: the small waste above, and fetching the base commit on demand for a
   multi-commit push instead of selecting everything.
6. Shape from measured timings.

### 4.1 The latency budget

One labelled PR at the proposed target [T]. "Today" is the generated `rust-ci.yml` on the ARC scale set.

| Stage | Today | Goal path | Bottleneck removed |
|---|---|---|---|
| 1. Event to job | Actions run, then scale-set acquisition: 13 s P50, 27 s P90 from job created to started [M, snapshot] | Webhook 1–2 s; Native start from a warm pool (after Q1) | Warm pool; dispatcher |
| 2. Sandbox start | ARC pod about 6 s, plus a Docker-in-Docker probe [M, snapshot] | ≤ 1 s P90 | Warm pods; Docker only for the obligations that need it |
| 3. Checkout | Full tree at depth 2 from GitHub | ≤ 1 s | Node mirror with a full tree check (section 8.4) |
| 4. Helper and tools | `setup-velnor`, mise, `tools-local` | ≈ 0 s | Read-only tool store |
| 5. Plan | `velnor-ci plan` about 16 s [M] | ≤ 2 s | Resident planner over the same metadata |
| 6. Dependency fetch | Crates mirror | 0 s | Node crate store |
| 7. Compile | Cold while `main` is red; delta layer and mbx otherwise | Only the change's closure | Green main; warm generation (section 8.3) |
| 8. Tests | Unknown | ≤ 60 s | Per-package keys; the test floor (section 9.6) |
| 9. Post-merge tail | 18 alone-checks, docs and the lane guard, in series after the merge | Where Q13 puts it; in parallel only in a second job | Lever 3 |
| 10. Required | `velnor-ci gate` | ≤ 1 s after the last result | — |
| 11. Queue entry and run | Never used; auto-merge off | ≤ 30 s on a same-tree lookup | Auto-merge; rung 3; the speculative queue (section 11.3) |
| 12. Merge | GitHub: 19 s enqueue to group and 26 s to merge, medians on this repository's queue [M] | The same | GitHub's queue (decision 7) |

**Floors** [E]. GitHub's 19 s and 26 s are measured medians, not a proven floor. The runner's own
floor for a one-crate change is the C5 critical path plus a warm compile and its tests; it is
measured in S6. The P90 has no such support: changes to whole-tree paths (`.github/`, `.velnor/`,
root manifests and configuration) select everything, and their slowest obligation sets the verdict.

## 5. The duplicate-work audit

Every repeated operation in the generated workflow [M, C]: **E** eliminate, **R** reuse or promote by
content, **K** keep deliberately.

| # | Operation | Today | Action | Mechanism |
|---|---|---|---|---|
| 1 | Runs for unrequested pushes (draft, ready without the label) | A ready PR runs the quick set on every push | E | Label flow (section 9.2) |
| 2 | Runs on an unchanged head (a title edit, an unrelated label) | Each starts a workflow run whose jobs skip | K, cheap | The ignored-event path: a separately named job in its own concurrency group, so it publishes no `Required` and cancels nothing. The App removes the run |
| 3 | A rerun of a commit already verified | A full run | R | Rung 1 |
| 4 | Push N, then N+1 | The older run is cancelled | R unchanged keys | In-flight join, once records exist |
| 5 | The queue group after the PR | The full suite (when the queue runs) | R | Rung 3 when the tree is equal; per obligation after; the speculative queue |
| 6 | The push after a queue merge | The full suite plus the post-merge tail | R | Rung 2, per obligation (section 7.1) |
| 7 | 18 alone-checks and docs after merge | Most of a full push job | K, placed by Q13 | In the queue if the owner decides so, in a second job only on measured gain; alone-checks only for released crates |
| 8 | The lane guard lists tests three times | Three `nextest list` calls | E | Read the test step's listing |
| 9 | Feature-lane lint and docs when the lane's package is not selected | They run on the whole workspace | E | Run only when the lane's package is selected |
| 10 | Unplaced paths select everything | Many commits | E | `[[inputs]]` entries for paths with a proven owner (lever 4) |
| 11 | A shallow checkout cannot reach the base of a multi-commit push or group | The plan selects everything (`AllTrigger::BaseUnavailable`) [C] | E | Fetch the base commit on demand (phase-0 item 9); later, selection by "not yet proven" (section 11.3) |
| 12 | A red main saves no target cache | Every run compiles cold | E | Green main |
| 13 | Compiles across PR, queue and push | Each run compiles its own | R | Records; warm generations (after Q1) |
| 14 | `velnor-ci` tests one at a time | One test group with `max-threads = 1` | E | Isolate the tests that change the environment |
| 15 | The tools release rebuilds from source on hosted runners | About 4 min per release [M] | R | Build once on `main`, promote at tag (section 9.5) |
| 16 | A Docker daemon in every ARC pod | A privileged dind sidecar per job [M, snapshot] | E | Docker only for the obligations that need it (after Q1) |
| 17 | Second witness, audit A1, conformance pairs, speculation | — | K | They buy confidence or wall time, and are charged |

Runner-minutes per merged PR are measured in phase-0 item 8. No estimate is published before then.

## 6. Runner architecture

Everything in this section and in section 8 waits for Q1. Phase 0 and the reuse ladder do not need
it.

### 6.1 The owner's decisions

Decisions 1–24 bind this design. Where each lands: one interface for Velnor Native (the default) and
GitHub Official, with the official runner also the permanent reference executor (1, 2, 19, 20:
sections 6.4, 6.8); latest APIs only (3: 6.4); no Node in Velnor Native and a closed action set (4:
6.7); review before any reuse (5, 14, 17: 6.11); experimental dependencies judged on evidence (6:
8.3); GitHub stays the platform (7: 9.4); no forks (8: 6.9); macOS paused (9: 9.5); build once,
promote (10: 9.5); correctness first with a four-part soundness case (11: 1.2, 7.5); the label flow,
and a green latest commit to merge (12: 9.2); one core with capability-declaring platforms (13, 21:
6.5); agents do the work, per-job-type migration with deletion (15, 16, 18: 11); storage,
`actions/cache` and mirrors (22–24: 8).

### 6.2 Conflicts with `docs/goals.md`

`docs/goals.md` changes only with the owner's explicit approval. The design conflicts with it here:

| `docs/goals.md` | The design | Owner question |
|---|---|---|
| Line 16: "Jobs are always executed by GitHub's official runner" | Velnor Native executes the workflows velnor-gen writes, by default | Q1 |
| Lines 11–12: the layout table names `generator/`, `ci-helper/`, `crates/velnor-runner` and "the existing engine in `crates/`" | The code sits in flat `crates/velnor-*` in one root workspace, and the old engine is gone | Q1 |
| Line 75, rule 11: a workflow is verified locally "with GitHub's official runner, started by the Velnor runner" | Local runs use the executor the project uses | Q1 |
| Line 43: a fork PR gets no token and only reads caches | No PR comes from a fork (decision 8) | Q6 |
| Lines 45 and 59: GitHub's free limits, at most 8 GB of GitHub's cache | Native `actions/cache` on node disks and RustFS | Q19 |
| Lines 40–41: a check is skipped only when the change cannot affect it; green means everything selected ran and passed | The reuse ladder satisfies a selected check with a receipt of an earlier run | Q8 |
| Line 60: an ordinary PR's wait does not get worse, unless correctness needs a check and the owner accepts its measured cost | Phase 0 adds checks to every PR | Q18 |

Two active specifications say the same as line 16: `docs/proposed/macos-scaleset-runner.md` (no
Actions interpreter; no second workflow interpreter in scope) and `docs/deferred/self-hosted-runner.md`.
Q1 covers them. `docs/hard-decisions.md` binds only the old engine, so it does not decide the reuse
ladder; the goals do (Q8). Lanes stay steps in one job per workspace, as goals rules 4 and 5 require,
unless a measured gain earns a split (Q7).

### 6.3 Components

New code is Rust under `crates/velnor-*` in the root workspace. Crossing a crate's line budget stops
work for review.

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| App and label flow | `velnor-control` | One fenced writer per repository | Webhooks, the label state machine, the status writer, the reconciler, `merge_group` handling | 6k lines |
| Dispatcher | `velnor-dispatch` | One service per site | Scale-set sessions, JIT identities and credentials; placement before acquisition; lease journal | 6k lines |
| Executor interface | `velnor-executor` | Library | The `Executor` trait, the work model, identity | 1k lines |
| Velnor Native | `velnor-exec` | In the node agent | The velnor-render dialect: steps, expressions, env files, masking, cancellation, timeouts | Measured after the dialect corpus |
| GitHub Official | `velnor-official` | In the node agent | Drives the pinned official runner for acquired jobs and as the reference | 4k lines |
| Native actions | `velnor-native-actions` | In Velnor Native | The closed action set (section 6.7) | Per action |
| Platform | `velnor-platform`, `-k8s`, `-docker` | Library | The platform trait, capabilities and two backends (section 6.5) | 2k + 6k + 4k lines |
| Storage and caches | `velnor-storage`, `velnor-cache` | Library and site service | Blob stores, tiers and routes; the cache index (section 8) | 4k + 6k lines |
| Ledger | `velnor-ledger` | Library, and a site service only on measured need | Records, joins, promotion, audits, epochs | 7k lines |
| Node agent | `velnor-node` | DaemonSet, or a host daemon under Docker | Warm pools, observation, the read audit, receipt signing, node stores, the builder, cleanup by proof | 13k lines |
| Conformance | `velnor-conformance` | velnor-lab | The differential harness (section 6.8) | 5k lines |

Names to settle in one decision record before the first crate lands: audit A1 against `velnor-ci
inputs audit`; the verification ledger against velnor-local's dispatch ledger; `velnor trace`
against `velnor-ci report`; `velnor check` against `velnor-ci gate` and `release-gate`; the
proposed `velnor` binary against `velnor-host` and `velnor-local`.

### 6.4 Two executors behind one interface

```rust
pub trait Executor: Send + Sync {
    /// Kind (Native | Official), version and image digest. Part of every result key.
    fn identity(&self) -> ExecutorId;
    async fn run(&self, work: Work, env: Env, role: Role, io: JobIo) -> Result<Outcome>;
}
pub enum Work { Job(AcquiredJob), Obligation(ObligationSpec) } // GitHub's job, or the contract's
pub enum Role { Production, Reference } // Reference: conformance, shadow, diff; never a verdict
```

All names in this sketch are proposed.

- **Velnor Native executes what velnor-gen writes** (R6). For velnor-new it runs the dialect
  velnor-render emits: the 16 pinned actions, one-command `run:` steps with their launchers, and the
  fixed set of expressions the renderer produces. It refuses anything else before the first step.
  This removes most of the general Actions protocol (the expression engine, composites in general,
  JavaScript actions) from velnor-new's path. How much protocol Native still needs to acquire and
  report a job (broker, run service, Results) is Q14.
- **Full GitHub Actions compatibility** stays the product goal for other repositories. It is a later
  stream, gated by the owner.
- **GitHub Official** runs the pinned official runner (`.velnor/runner.lock`) in the same sandboxes,
  pools and caches. It stays the switchable executor and the permanent reference. The existing
  launch code in `velnor-runner-host` is reviewed for this role and redesigned if it fails review.
- **Identity.** `.velnor/` names the executor per job type or globally, read from the base commit and
  limited to certified identities, so a change takes effect only after it lands. Velnor never
  switches on failure. The identity is in every key, so nothing is reused across executors and a
  switch re-verifies once.
- **Targets.** Clients name where work runs with `--target` (`local`, `k8s://de`, `docker://host`).
  A result's trust tier comes from the node that executed it, never from the target name.
- **Expressions.** For the dialect the expression set is enumerable from velnor-render and tested
  against the official runner in the corpus. The choice of a general evaluator matters only for the
  full-compatibility stream (D4).

**Speed per job type** [unmeasured as a difference]: Official adds the Worker's set-up per job and a
Node start per JavaScript action. Native saves seconds per job. S2 measures it; under 5 s per job at
P50 earns no executor claim.

### 6.5 The platform abstraction

The core describes environments generically and never names pods or containers. A backend is the
only code that knows where jobs run (decisions 13, 21). All names are proposed.

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
pub struct Capabilities {
    pub kernel: Kernel,             // Shared | Separate
    pub dir_entry: Vec<DirEntry>,   // BindOrReflink | VirtioFs | DiskImage
    pub shared_page_cache: bool, pub snapshot_restore: bool, pub warm_pool: bool,
    pub docker: Docker,             // None | PrivateDaemonPrivileged | PrivateDaemonRootless
    pub read_audit: bool,           // file-access tracing for key enrolment (section 7.2)
    pub network_policy: bool, pub cpu_pinning: bool, pub cgroup_kill: bool, pub arch: Vec<Arch>, pub os: Os,
}
```

| Capability | Kubernetes (Hetzner cluster) | Docker (one host: Docker Desktop on macOS, or Linux) |
|---|---|---|
| Kernel | Shared | Shared (a Linux VM's kernel on macOS) |
| Directories enter by | Reflink clone on XFS, bind mount | Bind mount; copy where the host has no reflink |
| Docker for tests | A private privileged daemon per request, on tainted nodes | A private daemon container per request, or refused; never the host daemon |
| Read audit | Yes, from the node agent | No: keys stay whole-tree |
| Network policy, cgroup kill, CPU pinning | Yes, yes, optional | Partial, container kill, no |

The core branches on capabilities, never on backend names. A missing capability selects a slower,
correct path. Lifecycle, recovery, tri-state proofs (`Unknown` is never absent) and teardown are in
the contract. Conformance runs per backend. A microVM or Apple Containerization backend is a new
implementation of this trait and nothing else. `docs/proposed/kubernetes-runtime.md` proposes a
different `Runtime` trait that keeps the host journal; one decision record merges the two before
either is built.

### 6.6 The job path

- **Phase 0 and phase 1.** GitHub Actions runs the generated workflows on the ARC scale set with the
  official runner. The gate is the `Required` job and, once the trusted verifier is required,
  `Velnor / Verified` (section 9.3). Reuse happens inside `velnor-ci` (section 7.1).
- **After Q1.** Velnor's dispatcher acquires the same jobs and places them on a node that advertises
  the generation, mirror head and tool store the job needs. Velnor Native runs the job as written.
  Witnesses still need distinct nodes and generations.
- **After the App.** A labelled head's webhook reaches `velnor-control` in 1–2 s. `ensure(tree,
  contract)` resolves each obligation as satisfied, in flight (join it) or open (schedule the missing
  witnesses); C5 obligations always run. The fenced writer posts `Velnor / Verified` when every
  obligation of the head is satisfied under the active epoch.

### 6.7 The Native dialect

Velnor Native implements the actions in `ALLOWED_ACTIONS` that velnor-new's generated workflows use,
and refuses a job before its first step when an action, SHA or input is outside the qualified set.
One generator test keeps the two lists equal: every allowed action either has a Native
implementation or a recorded refusal.

| Action | Native implementation | Node-local state |
|---|---|---|
| `actions/checkout` | The inputs the generator uses (`fetch-depth`, sparse checkout); a reflink clone of a sealed tree with only the changed files rewritten, then the full tree checked (section 8.4) | The node's bare mirror |
| `actions/cache`, `/restore`, `/save` | Section 8.2 | L0 and L1 tiers |
| `actions/upload-artifact`, `download-artifact` | Artifacts v4 on GitHub's service, so `gh run download` and the API see them | The site tier as a read-through cache by digest |
| `jdx/mise-action` | Pinned tools on `PATH`, with mise's environment | The read-only tool store |
| `tailrocks/velnor-new/actions/setup-velnor` | Installs the pinned helper by digest | The tool store |
| `jdx/mr-boxington-action` | Until the generator drops mbx steps: plain cargo over a generation (section 8.3) | The node store |
| `actions/create-github-app-token` | A scoped installation token, masked, revoked after the job | — |
| `actions/attest-build-provenance` | Only if Q5 moves releases off hosted runners | — |
| `docker/login-action`, `setup-buildx-action`, `build-push-action` | Only when a velnor-new job type needs them; refused until then | — |
| `actions/github-script` | None: it is JavaScript. velnor-gen replaces its uses with `velnor-ci` commands before a job type that uses it moves | — |

`hashFiles` is native.

### 6.8 The conformance oracle

`velnor-conformance` runs each corpus case through both executors on each backend and compares. The
oracle is `velnor-official` in the reference role.

- **Corpus.** The generator's golden samples (`generator/samples/*/expected`), every generated
  workflow of this repository under every event, `qualification.yml` (generated from
  `.velnor/workflows.toml`, with hosted and scale-set job pairs) and the `local/conformance` probes;
  plus fault and refusal cases: cancellation at every step boundary, timeouts, masking, post steps,
  an artifact saved by one executor and restored by the other, and the label flow's states.
- **Pass.** After normalising ids, times, runner names, paths, tokens, set-up lines and
  interleaving, outcomes, outputs, env, annotations, summaries, cache and artifact effects,
  cancellation and timeouts are equal, unless an owner-signed allow-list entry covers a difference:
  100% per backend, zero secret leaks, at least 95% of about 30 executor mutants killed.
- **When.** On every PR that changes an executor, a native action or the renderer's dialect; nightly;
  and within 14 days of each official release, with deadlines from the deprecations endpoint and an
  alarm 21 days ahead.
- **Canaries.** A production-path canary every 10 min per site, and permanent canary PRs that must
  stay unmergeable, probed every 10 min: any `mergeable_state` but `blocked` pages the owner.

### 6.9 Isolation and trust

Decision 8 says no PR comes from a fork: isolation is for trusted authors whose code may be buggy.
The generator today renders a fork fence (`fork_runner`, a hosted runner for fork PRs) and goals.md
line 43 promises fork PRs read-only access. Which one holds is Q6. PR results never become `main`'s
trusted caches or receipts without verification, on every tier.

| Class | Runs | Sandbox | Network | Stores |
|---|---|---|---|---|
| gate | Small jobs (lint, policy, plan) | Warm sandbox, 1 CPU | GitHub API | Read-only tools |
| build | PR, group and `main` jobs and their obligations | Warm sandbox: no Docker, seccomp `RuntimeDefault`, no capabilities, no service-account token; obligations also without `.git`, a GitHub token or the job's environment | Allow-list: crates mirror, site storage | Read-only trusted; per-obligation clones |
| docker | Only the test binaries that need a daemon (the `local-mode` lane of `velnor-runner-host`, velnor-local's Docker tests), as their own obligations | A private daemon per request, never shared and never the host's; a skipped or missing Docker test fails the obligation | As build, plus a registry mirror | The build's test binaries, cloned |
| builder | `main` generations, no GitHub job | Unprivileged | Crates mirror, storage | Writes the trusted scope |

Runner credentials stay in the dispatcher and never enter a sandbox. The App's private key exists
only in the control plane. Agents act under an identity without repository administration.

### 6.10 Failure handling and recovery

- **Journal discipline:** persist the intent, make the call, record the outcome; unknown is never
  absent. The executor journals plan id, job id, run-service URL, job token and environment id before
  acting, renews every 60 s with a fresh loop, and after a crash resumes or fails the job with its own
  token, never twice. Results land by compare-and-swap.
- **Fault injection** before each move: crashes around every remote call, journal write and step
  boundary, a lost lease, duplicate and reordered webhooks, a lost node, the control plane and a site.
  **No fallback** (decision 18): a divergence is fixed forward. The cluster runs only released
  versions.
- **A bad helper release.** The fix is a PR that pins the previous release. Its own run uses the
  helper it pins. The verifier treats an older pin as a reduction, so the owner signs it. No
  procedure contains an admin merge.
- **A bad Velnor release,** once Velnor executes jobs: redeploy the previous image digest (a deploy,
  not a code path). Recovery when no earlier image can verify is an open question in the owner
  sheet; an emergency stop on the queue and tags is Q16.

### 6.11 What is reused, after review

Nothing is reused by default (decisions 14, 17). Each component is reviewed and kept only when it is
well designed for this architecture.

| Component | Verdict | Why |
|---|---|---|
| `velnor-gen`, `velnor-ci` and the `velnor-model`, `-rust`, `-render`, `-workflows`, `-release` libraries | The base of the design | They already compile and enforce the contract's parts (section 2) |
| `velnor-runner-core` (lifecycle, `CleanupProof`, `OwnedIds`) | Port and refactor | The tri-state "unknown is never absent" rule is right |
| `velnor-runner-github` (scale-set wire types and client) | Port and refactor, pending review | The scale-set side exists in Rust; do not write it twice |
| `velnor-runner-host` journal and launch state machine | Correctness fixes only until Q9 | Its discipline (persist the intent, call, record) moves into the executor journal |
| `velnor-runner-host` Docker client and spec audit | Port into the Docker backend, pending review | The audit rules (no host mounts, no outer socket) are valuable |
| `velnor-runner-host` official-runner launch code | Reviewed for `velnor-official`; redesigned if it fails | Decisions 19 and 20 keep the official runner as a maintained executor |
| The `Runtime` trait in `docs/proposed/kubernetes-runtime.md` | Port its observation model into the platform trait | `Present`, `AbsentProven`, `Unknown` and Pod-UID proofs are right |
| velnor-legacy's protocol clients, `gha_cache.rs`, Docker lease and JavaScript sidecars | Protocol clients only for the full-compatibility stream; the rest is redesigned or dropped | velnor-new's dialect needs little protocol (Q14); the cache index is new; no Node in Velnor Native |

## 7. Verification records and reuse

### 7.1 The reuse ladder

Reuse climbs a ladder, cheapest proof first (R5).

| Rung | Subject | Mechanism | When |
|---|---|---|---|
| 1. Same commit | A rerun of a commit already verified | A lookup of its records | After phase 0 |
| 2. Queue to push | The push after a queue merge | Records of the queue run, per obligation. It replaces the run-level `velnor-ci merge-group-passed` lookup behind `skip_push_after_merge_group` | Phase-0 item 7 |
| 3. Same tree | A queue group whose tree equals an already verified tree | A lookup (`ReusePoint::SameTree`, proposed, generalising `Step.skip_after_merge_group`) | After phase 0 |
| 4. Same obligation key | Per-package and per-test-binary keys | Only after a read audit proves the key covers every input | After Q1 (the node agent) |

**Rules for every rung.**

- **Evidence is per obligation.** An obligation is reused only when a record shows that this
  obligation ran and passed, on the source run's latest attempt, and the record's full resolved key
  equals the request's (section 7.2). A green run is no evidence for a step it did not execute, and
  the queue and the push can select differently.
- **Unclassified work runs fresh.** Until the C5 classifier and the Git-metadata census have
  classified an obligation, it runs on every request.
- **Shadow first.** A rung turns on after shadow comparisons counted as real would-hits, each
  re-executed, with no mismatch and at least one hit (C6). The hit rate is published.
- **Every rung waits for Q8,** because reuse satisfies a selected check with a receipt.

**Phase-1 records need no new service.** They are GitHub cache entries keyed by the full key. A cache
scope says who may read an entry, not who observed the result, and a queue run cannot read a PR's
scope. So only two publishers write records others may trust: push runs on `main`, and the trusted
verifier (section 9.3), which is the path from a PR's result to the queue. Until the lab proves that
path, rung 3 stays in shadow. A separate `velnor-ledger` service comes only when a measured need
exists.

### 7.2 Keys

- **A key is a canonical struct** with named fields, digested over its encoding, so `velnor explain`
  names the field that changed. A new field raises the key version (a miss). The **definition half**
  is fixed at generation: command, configuration and features, profile, nextest profile, workflow
  digest, helper digest, toolchain and tool-store digests. The **input half** is resolved by
  `velnor-ci plan`: the whole repository's tree, the exact package selection and features (a test
  passed under `-p a` and failed under `--workspace` [M]), and the execution manifest. Keys also
  carry the **executor identity** and, once Velnor executes jobs, the **execution fingerprint**
  (kernel release, architecture, OS, CPU features, CPU count, sandbox profile digest). A test-run key
  that depends on built bytes waits until the build has produced them (R1).
- **Equal tree is not equal release bytes.** The tools embed `VELNOR_COMMIT`, and a squash changes the
  commit, so release keys include the commit until section 9.5's signed manifest exists.
- **Narrower keys need an enrolled contract.** A per-package key, computed from Git subtree ids plus
  the contract's single files and absent paths, is used only after the read audit (section 7.5) has
  run in shadow on that obligation. A reviewed contract names every path it may observe, directory
  contents and absent paths included. Every execution runs on the intact tree with the audit
  enforcing the contract, so one observation outside it fails the witness and revokes the
  enrolment. The trace proposes and revokes; it never licenses a hit. Many test files climb out of
  their package through `CARGO_MANIFEST_DIR` or list directories, so most test keys stay whole-tree
  until S5 counts them.
- **Time, randomness and the network.** An obligation is C5 when its code path reads the realtime
  clock, randomness, the network or the event. The helper reads the clock in `now_seconds()`
  (`crates/velnor-ci/src/io/mod.rs`, `io/budget.rs`) [C]. Each obligation also declares `reuse =
  content | per-request | never`, and the stricter of that and the classifier wins. Nothing is reused
  on a time-to-live. Realtime reads go through one injectable clock fixed per obligation and recorded
  in its key. Network inputs resolve only from committed locks through digest-checked mirrors.
- **An "instant" verdict** (rebase-only push, identical group tree) needs zero C5 obligations and
  zero misses.

### 7.3 Records, witnesses, joins and scopes

These need the node agent (after Q1), except scopes, which phase 1 maps onto GitHub's cache scopes.

- **A verification record per key** has mandatory witness slots: two for a new key, in a PR or a group
  alike. Each slot binds one receipt from a distinct node, attempt and order seed, over a different
  verified generation or cold, bound to the inputs and the active epoch. The record passes only when
  every slot passes. Audit re-execution writes to an audit stream, never a slot. A failed record is
  shown, never reused.
- **Order.** nextest 0.9.152 is pinned (`mise.toml`) with no shuffled order configured, so the agent
  either runs a binary's tests in seeded batches through filter expressions, or runs a digest-pinned
  nextest patched with a seed option, whichever costs ≤ 1.5 s per binary. Until then no order
  difference is claimed.
- **Fast red.** A non-minting lane builds incrementally, runs `cargo check` and the changed or
  historically failing tests first, and posts a failure at once. It never writes `success` and never
  fills a slot.
- **In-flight join.** A request joins the record its key needs. A cancelled request drops its claim;
  execution continues for the others.
- **Scopes.** A PR's execution yields `candidate` records, reusable by any request with the same key.
  A record becomes `trusted` only when its tree is on `main` and A1 has passed. In phase 1 a PR run
  writes nothing that another event can read; the trusted verifier publishes the candidate record of
  a PR run it verified, and push runs on `main` publish trusted records.

### 7.4 What a request executes

| Situation | What runs |
|---|---|
| A rerun of a verified commit | Rung 1: obligations with an equal full key resolve by record; C5 and unclassified obligations run |
| An unrelated label, or a title or body edit | Nothing: the event publishes no check and cancels nothing |
| Rebase-only push with the same tree | The C5 obligations; the rest resolves by rung 3 |
| Push N+1 while N runs; stacked or concurrent PRs | N is cancelled at once; each key's record fills once, and others join it |
| Merge group | Rung 3 when the tree equals a verified tree; otherwise every miss and every C5 obligation |
| Push to `main` after a queue merge | Rung 2: per obligation, what the queue ran and passed on an equal key; everything else runs; later, audit A1 |
| Release tag | The release gate, then promotion of verified bytes (section 9.5) |

### 7.5 Soundness cases

Every reuse or skip, with its four parts (decision 11): why the key is complete, what fails if it is
not, how that is detected, and the automatic fallback.

| Reuse | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| Same commit (rung 1) | The commit fixes the tree; the full resolved key is equal (selection, workflow, helper, toolchain, runner image, executor); C5 and unclassified obligations run anyway | A non-tree input changed: a tool, the runner image, an undeclared clock read | Definition half and fingerprint in the key; C5 classification; A1 | Revoke by epoch; full run |
| Queue to push (rung 2) | The record is per obligation, from the queue run of the same commit on its latest attempt, and its full key equals the push's | A step skipped on push that the queue never ran, or ran under a narrower selection | Tests for an unequal selection, a skipped or cancelled step, an older attempt and a different commit; shadow would-hits re-executed | No record on the key: the step runs |
| Same tree (rung 3) | The tree is every repository input; selection, definition, fingerprint and executor are equal; the record comes from the trusted verifier or from `main`; C5 and unclassified obligations run anyway | A non-tree input changed | As rung 1; A2 | Revoke by epoch; full run |
| Per-package record (rung 4) | An enrolled, reviewed contract covering every possible observation, enforced by the read audit on every execution | A read the contract missed, on a branch no audited run took | Executions chosen independently of the key: the 2% sample, A1's re-runs and the nightly reuse-free run; the sibling-file canary | Enrolment revoked; whole-tree key |
| Candidate record reused across pushes and PRs | Same key, Velnor-observed, two independent witnesses | A PR sandbox leaked state into a result | Sampled re-execution; trusted only after A1 | Candidate reuse off for that class |
| `velnor verify`, `velnor ship` and counting agent commands (section 9.7) | The same sealed sandbox, command and key as the gate, on cluster nodes, over the node-hashed tree | A Mac or dirty-worktree result treated as evidence | Only T2 node-signed receipts fill slots; T0 and T1 never; arm64 and amd64 keys differ | The label runs what is missing |
| Warm compile over a generation | Section 8.3: inputs proven by content against the generation's manifest | A stale `OUT_DIR`, build-script or proc-macro output, or an older-time file cargo skips | Until graduation the second witness builds cold and the binary digests must be equal; A1's cold digests; lab B's canaries | Retire the generation; build cold |
| mbx units for a cold or mismatched node | mbx's per-crate keys | A missed input in mbx's key | A1; the builder's cold rebuilds; digests equal before a unit enters a generation | Store retired; plain cargo |
| `actions/cache` entries | The user's key and version, resolved with GitHub's scope and order | A user key that omits an input (as on GitHub) | Conformance against the official runner; content hash on every read | Treated as a miss; never used by a receipt-minting obligation |
| Artifacts, binaries, tool and crate stores | Content address; pinned versions and lockfile checksums | Corruption; a store edited in place | Hash on read; read-only mounts | Refetch, rebuild, or rebuild the store |
| Test run keyed by binary digests | The run's inputs are the bytes of every binary it executes, the enrolled runtime contract, the definition and the fingerprint | A stale warm build reproducing the old binary; a run-time read outside the contract | Digests only from twin builds; 2% re-execution; A1; S1 plants a data-file change that leaves the binary unchanged | The class is suspended; keys fall back to whole-tree |
| Hedged copy of a slow shard | Every launched copy is a witness of the same key and must pass | A hang discarded as the losing copy | A cancelled, timed-out or lost copy fails the obligation (C4) | None needed: hedging cannot produce a green |
| Mirror checkout | The staged tree is hashed in full and must equal the requested tree before the first step | A stale, corrupt or concurrently refreshed base | The full check, every time | Fetch the commit from GitHub; rebuild the mirror |
| Release promotion | Closure, release profile, target, tool image and the signed manifest's commit | Wrong bytes published | Re-hash and `gh attestation verify` at publish | The release stops; nothing is rebuilt silently |

**The read audit** resolves paths in the kernel through LSM hooks (opens, attribute and directory
reads, failed lookups) per sandbox cgroup, and a seccomp filter fails the witness on every path call
it cannot see. With the tree hash it must stay within 3% of suite wall and 1.5 s per obligation at
P50; a backend without it keeps whole-tree keys. **Every per-package contract includes** each tracked
file in the repository root (`rustfmt.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`,
`Cargo.*`, `mise.*`) and `.cargo/` and `.config/`, because the tools search parent directories.

### 7.6 Trust root, publication and audit

- **Keys.** One Ed25519 signing key per node and sandbox class, held by a signer daemon outside every
  sandbox; receipts go to an append-only log per site; a receipt fills a slot only after its
  signature, class and log entry are checked.
- **Publication** (with the App). One fenced status writer per repository holds an epoch-numbered
  lease. It keeps a durable index of every `success` it published, rechecks the request and epoch
  before a write, reads the status back after it, and reconciles an ambiguous write.
- **Revocation.** Every record carries its epoch. A *planned* change (a reuse class enabled, a key
  version raised) freezes new `success` writes, lets in-flight groups finish, activates the new
  epoch and re-resolves open heads. An *emergency* revocation stops writes, writes `failure` on every
  indexed success that is still an open head or a queued group, re-verifies them, and audits every
  commit landed in the window cold.
- **Audit A1.** For each update of `main`, every new first-parent commit is rebuilt cold, its changed
  obligations re-run, and results and binary digests compared with its records. A1 keeps a persisted
  frontier on the first-parent history, so a commit that landed between two audits, or whose push run
  reused results, is still audited. One reuse-free run of the full contract covers the tip every
  night. A1's SLO is 2 h at P90 [T]; a 2 h lag alarms; at 6 h
  it pages, the trusted scope stops advancing, the release waits and warm-compile reuse suspends.
  Once A1 exists, the push `Required` job reports it (section 9.1).
- **Audit A2** runs nightly with the clock shifted 8 and 30 days ahead. **Degraded mode:** with the
  records unreachable nothing is satisfied by lookup; obligations run fresh.

## 8. Storage and caches

This section is runner-side and waits for Q1. Native `actions/cache` off GitHub's service also
touches goals.md lines 45 and 59, which is Q19.

### 8.1 One storage abstraction, configurable tiers

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
  site; any S3-compatible store. Tiers compose in configuration (`local`, `local → site`, `local →
  site → cross-site`), and each data class routes separately. Defaults: `local` only on Docker and
  macOS; on Kubernetes, node NVMe (L0) → the site's RustFS (L1) → the other site by replication (L2).
- **What goes where** [M, `experiments/rustfs.md`, one loaded Mac, indicative]: a 6,053-file registry
  took about 1.2 s to GET as a 44.5 MB tarball and about 17 s to extract, against about 5 s to
  reflink-copy it. Restore cost is extraction, so **extracted trees stay in L0** and **RustFS holds
  only blobs**.
- **Topology.** One cluster per site; between sites, asynchronous replication of content-addressed
  buckets, as warmth only. Blobs are named by hash; names live only in Velnor's index. Each worker
  gives RustFS its own device or volume, never the L0 filesystem. The sites are uneven [M, snapshot]:
  FI's workers need their data volume rebuilt to give RustFS its own volume; DE's two workers survive
  a drive loss, not a node loss, so DE is cache-only.
- **Invariants:** `actions/cache` semantics never change; Velnor's cache layer, not the backend,
  enforces scoping, PR writes never reaching `main` scope, immutable keys and content verification; a
  misconfigured, lagging or degraded store only makes things slower (a missing or corrupt blob is a
  miss). A manifest or record commits only after every blob it names is confirmed present. Every
  store has garbage collection and a budget: three generations per workspace, 40% of a node's data
  volume, an alert at 70%.
- **Cluster test** before L1 carries any route: node loss and return, replication over WireGuard at
  35 ms with existing-object backfill, Linux fsync cost per durability mode.

### 8.2 Native `actions/cache`

Users write plain `actions/cache` with GitHub's exact semantics: a run reads its own ref, its base
and the default branch, writes its own ref, and keys are immutable. Conformance proves it.
`velnor-cache-rules` already emulates these rules for velnor-local and is the starting point after
review.

- **One authority for names.** Each entry is `repository × scope × key × version`, committed once,
  insert-if-absent, by the repository's cache index in its home site. It names a content-addressed
  manifest of chunks. User key strings never become object names.
- **Scope and order** follow GitHub's lookup order, and a manifest is resolved before any blob tier is
  read, so an L0 prefix hit never masks an exact entry.
- **Tiers and chunks.** L0 holds the extracted tree of each resolved manifest, reflink-cloned per
  job; L1 holds compressed chunks of about 1 MiB cut by content (a one-crate change uploaded about 1%
  of the bytes [M, one pair]).
- **Eviction** (7 days unused, a size cap) is least-recently-used order in the index. **Prefetch**
  warms L0 for keys known before the job.
- **Receipt-minting obligations never restore `actions/cache`.**
- For the generated workflows the helper's own steps (`tools-local`, the mbx store) may serve the
  same purpose without an `actions/cache` server; the cheaper path is chosen by measurement.

### 8.3 Compile reuse

**Measured** [M, `experiments/mbx-cache-cluster.md`, 357 crates, idle nodes]: cold 92 s; a reflink
clone of the warm target 0.25 s and 2.5 MB, then 0.31 s and 0 crates; an ext4 copy 8.6 s and 10.75
GiB; mbx on a second node 39.9 s; a lockfile change 67.4 s plain, 43.4 s with mbx on another node
once one has built it. The CI store is now shared between nodes (`mbx_shared_store = true`), so the
mbx figures are re-measured before they count.

**The hot path is plain cargo over a reflink clone of a verified generation** at a fixed path, with
inputs proven by content, independently of cargo's freshness:

1. A generation G is the target of a trusted build whose cold A1 rebuild gave equal digests, sealed
   with its **input manifest** M_G (each path's blob id, mode and time) and every unit's dep-info.
2. For a request tree T, the changed set Δ is every path whose blob or mode differs from M_G, plus
   every path added or removed. The base is G's manifest, never the checkout's base. The existing
   `inputs restore` (`crates/velnor-ci/src/io/inputs.rs`) is not ported verbatim, because it diffs
   against the checkout base.
3. If a path in Δ is outside every unit's declared inputs, every workspace unit builds cold;
   third-party units are reused. A new `.rs` file is always outside them. Lab B5 may narrow this, as
   selection and never proof.
4. Unchanged paths keep M_G's times; every path in Δ is stamped later than G's newest output.
5. Any unit whose read set meets Δ loses its fingerprint before cargo runs. Afterwards each dep-info
   input of every unit not rebuilt is checked against M_G by blob id
   (`crates/velnor-ci/src/io/audit/dep_info.rs`), and a mismatch fails the build and retires G.
6. Toolchain, profile, environment and fingerprint are in G's key.

- **Witnesses.** Until compile reuse graduates (about 3,000 clean warm-against-cold digest
  comparisons), the second witness builds cold with equal digests.
- **Variants.** `CARGO_INCREMENTAL=0` on every minting or shipped build until lab B4 shows zero
  differences; blaze was rejected (it built none of the five workspaces and missed a clippy `deny`
  [M]).

### 8.4 Node-local mirrors

Each node keeps a bare, job-read-only mirror of every configured repository (decision 24): PR heads,
merge-group refs and tags, fetched on push webhooks with a periodic catch-up, seeded from a site
bundle. One writer per mirror; a refresh is a new mirror swapped in. The crates mirror
(`[crates_mirror]`, `velnor-ci crates-mirror`) is the precedent: the generator gains a mirror option
on `Checkout`, rendered as one `velnor-ci` command that verifies `HEAD^{tree}` (proposed).

**Native checkout,** well under 1 s:

1. Fetch only the missing objects of the exact commit, in one round trip.
2. Reflink-clone a **sealed base** (a pre-materialised tree recorded with its tree id, read-only) and
   rewrite each path whose blob or mode differs.
3. Before the first step, hash the whole staged tree and require it to equal the requested tree
   (0.43 s on eight threads on a Mac [M, proxy]; unmeasured on a worker).
4. Set times by the generation rule (section 8.3). Obligations get no `.git` (C5).

**Every upstream the same way,** pinned and verified before use: crates by lockfile checksum,
toolchains and mise tools by digest, images by digest through a registry mirror per site.

## 9. Gate, label flow, merge queue and release

### 9.1 Shape and gate parity

- **One job per workspace.** `rust-ci.yml` runs one `rust` job, then `policy`, then `Required`; the
  lint jobs join after phase-0 item 4. Lanes are steps. A job is split only when it shortens the wait
  by more than its own start-up time, from measured timings (goals rules 4 and 5; Q7).
- **Parity** is a generator property (section 2.2): the contract and `Required`'s needs come from one
  plan.
- **The push `Required` job** runs `velnor-ci gate` today. Once A1 exists it also reports A1 for
  `GITHUB_SHA`, so the release gate keeps reading one push-run check.
- **Checks by name.** `velnor check -l` lists the contract's obligations by name, following velnor-gen
  step and package names (`fmt`, `clippy:velnor-ci`, `test:velnor-gen`, `fresh:rust-ci.yml`), and
  `velnor check <name>` runs exactly that obligation's generated command. An unknown name is
  refused. Every generator in the contract gets a freshness obligation (regenerate; any diff fails).

### 9.2 The label flow

This is exactly the flow the owner asked for (decision 12, R3).

- **One label, `velnor:verify`.** The older run labels, the `ci-run-label` job and the quick set on
  unlabelled PRs are deleted by phase-0 item 2. `pr_ci = "label"` is rewritten in place; there is no
  third mode.
- **`Required` is never skipped** on a pull-request event that counts, because a skipped required job
  passes. It is **red** ("CI not requested") on any commit of a PR that is a draft or has no label.
- **Labels come from people or App tokens,** never `GITHUB_TOKEN`, whose label events start no
  workflow.

| Event | What runs | `Required` on that commit |
|---|---|---|
| Push to a draft | Nothing | Red: "CI not requested" |
| Ready for review, no label | Nothing | Red: "CI not requested" |
| `velnor:verify` added to a ready PR | The latest commit runs | Pending, then green or red |
| A draft that carries the label becomes ready | The PR is armed; the latest commit runs | Pending, then green or red |
| Push while labelled | The new commit runs; the previous run is cancelled | Pending, then green or red |
| `velnor:verify` removed | The run in flight is cancelled | Red, also when it was green: an unlabelled PR never has a green required check |
| The base branch changes | The latest commit runs against the new base, if labelled | Pending, then green or red |
| Another label added or removed; a title or body edit | Nothing | Unchanged: the event publishes no `Required` |

**Generator changes.** `PullRequestType` gains `Unlabeled` (proposed). `Edited` stays for a change of
the base branch only. An ignored event (another label, a title or body edit) runs a job with a
different name in a concurrency group of its own, so it publishes nothing named `Required` and
cancels nothing. No earlier verdict is looked up by commit SHA in phase 0.

**What the check is attached to.** A check on GitHub belongs to a commit. Two PRs with the same head
commit therefore share the newest `Required`, and a pull-request run reports on the head commit while
it tests the merge with the base. Lab A records both. State per PR (label, head, tested tree, base,
run generation) comes with the App.

**The cost.** An unlabelled PR shows one short red job per push until the App exists. It is accepted,
because a green check that verified nothing is worse.

**Lab A proves it** on velnor-lab before the ruleset changes: draft red; unarmed red; label → full
run; push while labelled cancels; green, then unlabel, is red; another label added or removed during
a run does not cancel it; a base change reruns; two PRs on one commit; a run that completes after an
unlabel or a push cannot make the head green.

### 9.3 A PR must not weaken its own gate

On `pull_request` and `merge_group` events GitHub runs the candidate's own copy of the workflow, with
the helper the candidate pins. A check that the workflow runs on itself can be removed by the same
PR, and a required check bound to the GitHub Actions app accepts a job of that name from any workflow
file. Agents are trusted but fallible, so this is the realistic false-green path once the bypass is
gone (R4, Q12).

- **Accident guards (phase-0 items 3 and 10).** Inside the workflow, the generator check fails on a
  stale or hand-edited generated file, and the gate compares the PR's contract with the **base
  branch's** contract through `velnor_rust::weaken`, failing on a reduction the owner has not signed.
  They catch mistakes made through configuration. They do not stop a PR that deletes the check step,
  adds a second workflow with a job named `Required`, or pins an older helper.
- **The trusted verifier (phase 0).** A generated `workflow_run` workflow, which GitHub always takes
  from the default branch. It checks out nothing and reads the candidate tree as data. With `main`'s
  pinned helper it checks generator freshness, the reduction check, the helper pin, and that the
  `rust-ci.yml` run for that exact commit passed on its latest attempt with its `Required` job. It
  writes `Velnor / Verified` on PR heads and group heads through an App whose key is a secret of an
  environment only `main` can use. The ruleset requires that status from that App, so a status a PR's
  own workflow posts does not count. From then on a PR cannot weaken its own gate.
- **A signed reduction.** The owner approves a reduction with an SSH signature over the base contract
  digest, the head contract digest, the sorted list of reductions and an expiry. The verifier checks
  it against `.velnor/allowed_signers` as it is on the base branch. A rebase or any contract change
  voids it.
- **Later:** the `velnor-verify` App service takes over from the verifier workflow.

**The App.** It writes `Velnor / Verified`, bound by `integration_id`, on the PR head and on each
group's head; only `success` passes. Permissions: `statuses: write`, `checks: write`, `pull_requests:
read`, `contents: read`, `metadata: read`. Webhooks: `pull_request` (`opened`, `reopened`,
`synchronize`, `labeled`, `unlabeled`, `ready_for_review`, `converted_to_draft`, `closed`),
`merge_group`, `push` and `check_run` (`rerequested` on `Velnor / Details`). It runs the state machine
of section 9.2. Every 10 s a reconciler reads the labelled open PRs and the queue, starts what is
missing, cancels what is unwanted and corrects any status the records do not hold; webhooks only cut
the delay. `Velnor / Details` is a check run with the full report and is never required. The App
keeps admission per PR, so two PRs at one commit share results but not an armed state; the merge group
verifies its own tree.

### 9.4 Merge queue

GitHub's queue stays (decision 7). The live ruleset binds `Required` to no app, lets admins bypass,
uses HEADGREEN with two entries built, a 5-minute minimum wait and a 90-minute timeout, and auto-merge
is off [M]. Phase-0 item 6 changes it.

- **ALLGREEN.** HEADGREEN is unsound with diff selection: a group entry is tested only on its own diff
  (`merge_group.base_sha..head_sha` [C]), so a later group's pass does not cover an earlier entry.
  Lab A observes the ranges with two overlapping entries before the ruleset changes.
- **The minimum wait** applies only until the minimum group size is met. With a minimum of one entry
  it adds no wait.
- **Depth.** Two entries built and a 90-minute timeout until a group verdict takes ≤ 5 min at P90 and
  paired false failures are ≤ 3%.
- **Auto-merge.** A repository setting, separate from the ruleset. Agents enable it when they open the
  PR and stop merging `main` into their branches.
- **No bypass.** `bypass_actors` is empty; agents act under an identity that cannot edit the ruleset.
- **Tree-equal groups.** How often a group's tree equals the PR-tested tree decides what rung 3 and
  the speculative queue are worth. It is measured in shadow first (section 11.3). No verdict depends
  on the rate.

### 9.5 Release: built once, promoted

A release never rebuilds (decision 10, R10).

- **Now (phase-0 item 5):** the generated tools release is gated on the green push run of the tagged
  commit, through `velnor-ci release-gate`: the workflow file, the exact commit, the latest attempt,
  the `Required` job. A missing, pending, skipped or older run blocks. Today its gate checks only that
  the tag is on the default branch [C].
- **Next:** the push run on `main` builds the release binaries. A tag promotes those exact bytes after
  the gate instead of rebuilding them.
- **Squash changes the commit the tools embed** (`VELNOR_COMMIT`), so byte-equal promotion needs a
  signed release manifest that binds the binary digests to the tagged commit (Q15).
- **Hosting.** Releases stay on GitHub-hosted runners with attestation until the owner decides
  otherwise (Q5): a stated exception to decisions 9 and 10 until promotion lands. velnor-render pins hosted release runners (`HOSTED_RUNNER`) and verifies
  attestations with `--deny-self-hosted-runners` (`release_attest.rs`) [C].
- **`runner-v`.** `release-plz.toml` declares the runner release group, but no workflow publishes it.
  Generating one waits for the runner-crates decision (Q9, Q17).
- **Tests stay in the dev profile** [M]: testing the release build costs more CPU, shares few crates
  with the shipped binary and is still not the shipped code (the tools ship as musl with `panic =
  "abort"`, thin LTO and `strip`). Release bytes are one obligation keyed by closure, lockfile,
  toolchain, profile, target and, while the commit is compiled in, the commit. Both Linux targets are
  built natively (`ubuntu-24.04` and `ubuntu-24.04-arm`).
- **A tag runs one job:** gate, fetch by digest, re-hash, `gh attestation verify`, publish
  (idempotent, read back): about 15 s [E] against about 4 min today [M].

### 9.6 Tests and the floor

- **Correctness first.** The nextest preflight and the test step must accept the same arguments; a
  positive argv test keeps them in step. Clippy runs with `-D warnings` on test targets too.
- **The long tail** is measured per test binary on a worker (S5), not from job logs. The longest known
  suite is `receipt_integrity` in `velnor-runner-host` [M].
- **`velnor-ci-serial`** goes once the tests that change the process environment use private state.
- **Dependency `opt-level = 3` with assertions on** [M, one test]: 67% less CPU, the checks costing 5%
  of instructions; no target counts the gain until lab B1.

### 9.7 Agents verify through Velnor

After phase 0. An agent's verification runs through Velnor, on warm cluster nodes in the gate's own
sealed sandbox, so it counts. A Mac result is advice only. Agents ask before running anything, and
the label then mostly finds the work done. The gate's meaning and decisions 1–24 do not change.

- **Front end.** The verbs are the contract (`verify`, `ship`, `check <name>`, `resolve`, `wait`,
  each with `--json`), served by a per-user daemon, `velnord` (≤ 50 ms per call at P90 [T]). `velnor
  cargo` may take `mise.toml`'s cargo wrapper slot from `mbx` only after the generator stops emitting
  mbx steps. It maps exact argv shapes of contract obligations to *counting* runs, runs anything else
  locally as scratch, and stays only if count-only data show agents use it. The first release only
  counts and journals.
- **Trees, not pushes.** The daemon snapshots the worktree as a Git tree through a private index
  (tracked and untracked, never ignored files) and sends a thin pack to the node mirror under
  `refs/velnor/wip/` (seven-day expiry, secret scan on the delta). The node hashes the whole staged
  tree before step 1, and a later commit, push or PR of the same tree has the same key.
- **Where it runs.** Counting commands run on a warm node of the agent's site in the `interactive`
  class. The Mac's non-minting fast-red lane races it: the first red shows at once, and a local green
  is provisional. The `hk` hooks call `velnor check <name>`.
- **Trust tiers, shown to users:** T0 Mac native and T1 sealed Linux on a Mac are advice and never
  fill a slot; T2, a node-signed cluster candidate, fills one witness slot; T3 is trusted, on `main`
  after A1. The gate is unchanged: two T2-or-better witnesses per new key on distinct nodes, seeds and
  generations, then A1. An arm64 result never satisfies an amd64 key.
- **Verify and ship.** `velnor verify` fills both slots on two nodes (`--witnesses=1` leaves one to the
  label) against `merge-tree(queue tip, tree)`. `velnor ship` verifies, pushes, labels with the
  agent's App identity and enables auto-merge, an explicit act under decision 12. `velnor wait`
  returns an NDJSON failure packet (test id, signature, last lines, a `velnor repro` command) and
  cancels sibling work no other request shares on the first red.
- **Records first.** `velnor resolve` asks before running: a hit returns its record, in-flight work is
  joined, and the gate re-resolves under the current epoch. A failed record on an unchanged key
  returns as advice (`why-red`: red on `main`, a known flake, or new).
- **The PR verdict becomes a lookup:** the label resolves the head's own vector; hits read, misses and
  C5 run. Continuous verification of unpushed commits would start work no one asked for, so it stays
  off unless the owner decides otherwise.

## 10. The cluster

[M, snapshot] unless marked. The deployment manifests live outside this repository, and host
deployments stay operator-run (`docs/host-deployment-decision.md`).

- **Today.** Hetzner k3s with five workers. ARC runs three scale sets (tailrocks, ChainArgos, jackin);
  each runner pod is the official runner plus a privileged Docker-in-Docker sidecar. velnor-new's CI
  runs there on the label `ubuntu-26.04-scale-set`.
- **Role, after Q1.** k3s deploys the dispatchers, node agents, site services, RustFS and mirrors, and
  enforces quotas, priorities and network policy. Velnor places work; kube-scheduler places pods.
- **Sizes.** A build sandbox reserves 24 GiB and 16 CPU, so four fit per worker. Nextest's threads
  equal the class's fixed CPU count, which is in the key.
- **Reserved merge path.** One 16-CPU build slot and 2 gate CPUs per site for merge and gate work,
  plus A1's slot. Priorities: merge > gate > PR = other organisations' CI > batch; only batch is
  preempted.
- **Sites.** Each site has its own dispatcher, node agents and RustFS cluster. A request executes in
  one site (DE to FI is 35 ms over WireGuard). ChainArgos and jackin keep their ARC scale sets.

## 11. Delivery

Agents do all the work (decision 15). Coding does not set the pace. `main`'s CI and queue, reviews,
owner decisions, conformance and parity evidence, capacity and file conflicts do. Every item is 2–8
agent-hours, one PR under 400 refactored lines, and every replacing PR deletes what it replaces.

### 11.1 Phase 0 and phase 1

Phase 0 is section 3. Phase 1 follows:

- Rungs 1 and 3 of the reuse ladder, with records as GitHub cache entries, after Q8.
- The audits A1 and A2, the C5 classifier and the Git-metadata census.
- The release built on `main` and promoted at tag, with the signed manifest (Q15).
- A generated `runner-v` release workflow, after Q9 (Q17).

### 11.2 The runner stream (after Q1, Q9 and Q14)

A job type moves to Velnor Native when its corpus cases pass at 100% on Kubernetes, every mutant is
caught, N replayed pairs show no unexplained difference (bound 3/N at 95%), every step made to fail
is red as on the oracle, and P90 start latency is no worse than ARC's. Nothing moves back (decisions
16, 18); a divergence is fixed forward. Shadow sampling continues (decision 19).

| Cohort | Job types | Pairs (N) | Deleted in the same change |
|---|---|---|---|
| 1 | The small gate jobs: `policy`, `Required`, the lint jobs, `Cache`, `Cache maintenance` | 100–150 per type | Their ARC routes |
| 2 | The `rust` job | 300 | Its ARC route |
| 3 | The Docker-class tests (the `local-mode` lane of `velnor-runner-host`, velnor-local's Docker tests) | 150 each | The shared dind sidecar for velnor-new |
| 4 | `qualification`, `rust-cache-trim`; `release-rust-ci-tools` only if Q5 moves releases | 30 each; three release dry runs | Their routes; ARC's velnor-new scale set after the last move (Q9) |

The official runner's image, `.velnor/runner.lock` and `velnor-official` stay (decisions 19, 20).
No dated milestones are published before the phase-0 baseline (R9).

### 11.3 After phase 0

Ideas kept for after phase 0 (R13):

- One plan drives three targets: the generated YAML, Velnor Native, and local `velnor verify`.
- Selection becomes "what is not yet proven" instead of "what the diff touched", once per-package
  keys are proven; with whole-tree keys it would select everything on any edit. It needs no base
  commit, so `fetch-depth` stops mattering.
- Test binaries become the unit of reuse (nextest archive digests).
- The branch ruleset is generated with a drift check, like the tag rulesets.
- **Speculative queue:** the label run tests the PR on top of the current queue tip, so the queue
  group becomes a lookup. It may be the cheapest path to the proposed 3 / 8 min push-to-merge target.
  It needs measurement first.
- Plan edges (`Step.after`, proposed) let Velnor Native run independent steps of one job at the same
  time.

### 11.4 Lab experiments and their safe defaults

Each default is correct whichever way its lab goes.

| Lab | What it runs | Pass | Default until then |
|---|---|---|---|
| A. GitHub semantics | A lab repository with a ruleset, a merge queue and a test App: the label flow of section 9.2 in a generated `rust-ci.yml`; no status; App `pending` and `success`, then one more push; a required job skipped by `if:`; a status on the test-merge SHA only; labels changed with `GITHUB_TOKEN`, an App token and a user token; queue entries with no report, `failure` and `success` | The no-status, pending, foreign-source and skipped cases block; a queue entry lands only on `success`; the read-back matches | The Actions `Required` job, bound to the GitHub Actions app |
| B. Build matrix | One idle worker: the compile canaries (an older-time edit, an undeclared build-script input, a changed, removed and new input, a proc macro reading a file); dependency `opt-level`; reflink against a read-only overlay lower layer with 1, 4 and 6 sandboxes; the narrowed changed-set rule over 300 diffs with planted cases; seeding and lockfile changes; incremental on and off over 300 replayed diffs | Every canary rebuilds and digests match cold; a transport beats a cold build on more than 10% of requests; zero differences | Reflink; cold second witness; `CARGO_INCREMENTAL=0` for receipts |
| C. Shadow records | Beside production for 300 merges: whole-tree and per-obligation hits of each group against the PR's results; twin runs against two runs 30 min apart, on tests with seeded failure probabilities of 0.05–0.2 | Joint pass rates match (1−p)² | Groups execute every miss |
| D. Docker class | The Docker-needing test binaries, 200 runs each, under a private privileged daemon, a rootless daemon and a microVM, on both backends | Equal pass, fail and skip sets; nothing one request leaves is visible to the next | A private privileged daemon per request, or refusal |

## 12. Proving the speed

Nothing has run end to end and no comparator has raced this repository, so the speed claim waits on
this suite. **The contract is frozen first:** workload revisions, expected test IDs, features, Git
and toolchain versions, timeouts, the released helper and both exact subjects. Every arm must agree
with an independent cold oracle on every contracted case. **Workload:** 300 historical PR heads
replayed at their SHAs and arrival times, every class unweighted. **Baseline:** the generated
workflow on ARC at the phase-0 baseline. **Budget:** each self-hosted arm gets the same two FI workers
exclusively; rented arms at most the same vCPU concurrency; every arm reports CPU-seconds,
GiB-minutes, bytes and cost. The helper-pin regression check of section 1.4 is separate and earlier.

| # | Benchmark | Setup | Pass |
|---|---|---|---|
| S1 | Planted failures | 100 failing edits on every fast path: an older-time edit, an undeclared build-script input, a new sibling file, a clock shift, a stale generation, a data-file change that leaves the binary unchanged, a step skipped on push that the queue never ran, flaky tests at p = 0.05–0.2 | 100% red; a flaky test that fails with probability p is caught at the expected 1−(1−p)² rate; it gates every other run |
| S2 | **Control race** | Native against Official on the same warm workers, mirrors, tool store and generation, ≥ 30 interleaved pairs per job type at 15% and 80% CPU | Per-type deltas with intervals; an executor claim only at ≥ 5 s per job at P50 on ≥ 3 types |
| S3 | **Head-to-head** | The baseline; **a strong conventional baseline** on the same workers (persistent official-runner agents, reflink-cloned warm target, crates mirror, baked tools, auto-merge, ALLGREEN); Velnor with Official; Velnor Native; Velnor Native with agents' `verify`; rented arms if the owner allows them | The publication rule, on push-to-merge gross and net and on the verdict; every stage timed |
| S4 | Loaded replay | A day's trace with other organisations' load, A1 and release preparation; then a manifest edit, a cold toolchain, a lost worker, an ejected predecessor | Verdict P90 ≤ 1.5× quiet; merge-path wait P90 ≤ 10 s |
| S5 | Census | C5 classifier, read audit and a content-key would-hit counter on `main` and 300 heads; per-test-binary durations | C5 and whole-tree shares published; goals restated from them |
| S6 | Warm compile | 300 diffs over a clone, cold page cache, idle and 70–90% load, cold control; heads that add a source file reported apart | Slower witness P90 ≤ 25 s, ≤ 40 s loaded; digests equal cold |

**Publication rule** (pre-registered): "fastest" against an arm only if paired arrival-block bootstrap
intervals for the P50 and the P90 difference, at simultaneous 95%, lie strictly below zero per class,
each self-hosted pair interleaved at least five times. A tie, an unrun arm, a lost class or a
light-class-only win narrows the sentence; a false green or skipped work disqualifies. The strong
baseline stands in for Buildkite-style agents but keeps the Actions hop, so it bounds them on stages
2–9 only.

## 13. Top risks

| Risk | Detection | Containment |
|---|---|---|
| `main` stays red: a change merges without a green, test-running `Required`, or the generator renders what the pinned helper cannot run, or recorded metadata goes stale | The red check on drafts and unlabelled PRs; the schema check and the end-to-end tests | Phase-0 items 0, 1 and 3: the fault fails before it merges |
| A green check that verified nothing (draft, unlabelled, skipped `Required`) | Lab A; the state-based "done when" of phase-0 item 2 | `Required` is never skipped and is red without a verdict |
| A PR weakens its own gate | The trusted verifier's status, produced from `main` | `Velnor / Verified` is required; the checks inside the workflow catch accidents only |
| Merges keep going through the bypass | Ruleset read-back and the generated drift check | `bypass_actors` empty; no procedure uses an admin merge |
| A reuse gives a false green | Per-obligation records on the full key; shadow re-execution; 2% sampling; audit A1; the enforced read audit | Revoke by epoch; work runs again; landed commits in the window audited cold |
| Velnor Native departs from GitHub's semantics | The harness on every change, nightly and per official release; sampled shadow runs | Block the release; fix forward |
| GitHub changes the protocol | The harness within 14 days of each release; deadlines from the API with an alarm 21 days ahead | Fix before the deadline; a critical update pages at once |
| A stale status write survives a policy change | The success index; read-back; lab A's partition case | One fenced writer; emergency revocation writes `failure` |
| A backend or store behaves differently | Per-backend conformance; content verification on read | Capability fallbacks; slower, never wrong |
| RustFS is young and has open issues on node return, replication backfill and small writes | The cluster test; per-tier hit and error rates; replication lag | Routes stay `local` until the test passes; any S3 store can replace it |
| Stale compile state | Content-proven inputs; the cold second witness; A1 digests; canaries | Retire the generation; build cold |
| The cluster is saturated | Per-class CPU and memory watermarks | Batch work preempted first; reserved merge path |
| A speed claim is published before it is measured | Section 12's pre-registered rule; no dated target before the baseline | The claim stays "designed to cut push-to-merge" |
| Plans collide with a fast-moving `main` | Issues with state-based "done when" checks, linked to the PRs that address them | Other sessions are told before issues are created (R12) |

## 14. Owner questions

The [owner sheet](../research/velnor-runner-2026-11/owner-questions.md) holds the full text, options
and defaults. The design runs on each default until the owner answers.

| Id | Question | Recommendation | Blocks |
|---|---|---|---|
| Q1 | Amend `docs/goals.md` line 16 (executor), lines 11–12 (layout) and line 75 (rule 11) so Velnor Native may execute the workflows velnor-gen writes, with GitHub Official as the reference | Approve the narrow text in the owner sheet | Sections 6 and 8; the runner stream |
| Q2 | The label flow: `velnor:verify`, `ci:run` and `ci:auto` retired, no quick set, unarmed commits red, one short red job per push until the App | Yes, as section 9.2 | Phase-0 item 2 |
| Q3 | Freeze the bypass to repairs now; later remove it, bind `Required` to the GitHub Actions app and allow auto-merge | Yes: the freeze now, the ruleset after a test-running green `main` | Phase 0 as a whole; item 6 |
| Q4 | Switch the queue to ALLGREEN | Yes | Phase-0 item 6 |

| Q5 | Release provenance: hosted and attested, or built by Velnor | Keep hosted for now | Cohort 4; release on Velnor |
| Q6 | Fork PRs: the generator's fork fence, or no forks | Keep the fence in the generator; velnor-new refuses forks before any job | Section 6.9 |
| Q7 | Job topology | One job per workspace; split only on measured gain | Section 9.1 |
| Q8 | Reuse of a verified result, with one sentence added to `docs/goals.md` | Approve rungs 1 and 2 under the rules of section 7.1; none before the answer | Every rung |
| Q9 | `velnor-runner-host` | Correctness fixes only; features frozen | New runner features |
| Q10 | Where evidence lives | Contracts evergreen in `docs/`; dated data in issues | — |
| Q11 | Who reviews the ten soundness and trust-boundary items | Fable 5.2 at xhigh, named in the issue text; Opus 5.5 high until answered | — |
| Q12 | A gate the PR cannot weaken | The reduction check, then the trusted verifier in phase 0, the App later | Section 9.3 |
| Q13 | Which post-merge checks (docs, alone-checks) must block landing? | Run them in the queue, alone-checks only for released crates, decided after the baseline | Phase-0 item 7 |
| Q14 | May Velnor Native execute the generator's plan for velnor-new without the general Actions protocol? | Yes | Section 6.4 |
| Q15 | Release subject across a squash (`VELNOR_COMMIT` embedded) | A signed manifest binds the digests to the tagged commit | Release promotion |
| Q16 | Emergency stop and revocation authority | A scoped, audited hold on the queue and tags | Section 6.10 |
| Q17 | Generate the `runner-v` release workflow | After Q9 | Section 9.5 |
| Q18 | The proposed speed targets, and the measured cost of the phase-0 checks | Keep them as proposals; accept the cost after the baseline | Section 1.4 |
| Q19 | Amend goals.md lines 45 and 59 for a native cache and storage | Yes, after Q1 | Section 8 |

Also in the sheet. Blocking phase 0: an agent identity without repository administration and the
status App (D7), and tool releases by agents (D9). Not blocking phase 0: a deploy identity and the
node changes (D8); recovery from a bad release (D15); per-type cutover counts; the general expression
evaluator (D4, asked when the full-compatibility stream is scheduled); a Velnor merge train instead of
GitHub's queue; high-clock nodes; one compile for both witnesses; rented comparator arms for S3; the
agents' `interactive` class priority; automatic verification of unpushed commits; agent sandboxes on
the cluster; a native arm64 lane.

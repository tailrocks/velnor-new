# Velnor in plain words

This is a short guide to the Velnor plan for the owner. The decisions behind it are the
[rulings](velnor-gen-rebase/rulings.md) (R1 to R13). The details are in the
[design](../../proposed/velnor-runner-architecture.md), the
[delivery plan](delivery-plan.md) and the [owner questions](owner-questions.md) (Q1 to Q19).
The evidence is in the [as-built reports](velnor-gen-rebase/as-built/) and the
[comparison](velnor-gen-rebase/comparison.md).

**How to read the numbers.** Every number has a label:

- **(measured)**: observed on velnor-new CI, 2026-11-16 to 2026-11-19. The delivery plan names
  the run behind each one.
- **(budget)**: a number we aim for. It becomes a target only after the W8 baseline (R9). W8 is
  the work item that measures the first green, test-running CI.
- **(proposed target)**: a number from the earlier design round. `docs/goals.md` does not contain
  it and you have not decided it (Q18).

**Nothing in the plan has run end to end yet.** Every figure for the new design is a budget or a
proposed target until the baseline and the benchmarks in section 8 exist.

## 1. The idea in one paragraph

The plan has one rule. A green check must mean that this exact commit was verified. Today it
does not always mean that. So the plan first makes the check honest. Then it makes the check
cheap, by proving each piece of work once and reusing the proof only when it matches exactly.
**velnor-gen** (the program that writes our CI workflows) becomes the verification compiler: it
writes down what must pass, and the rest of the system proves it. Speed comes after that. A fast
answer that could be wrong is not allowed.

## 2. What exists today

velnor-gen already writes our workflows. The old engine (`velnor-actions`, 12 crates, 450k
lines) is deleted.

| Part | What it is today |
|---|---|
| Generated workflows | `rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml`, `tag-ruleset-drift.yml` and the tools release workflow (`release-rust-ci-tools.yml`). Each has a digest header, but no CI step checks it yet, so a hand edit is not detected (W3). |
| Hand-written workflow | `ci.yml` (four lint jobs). It is the last one. |
| Pull request run | One big job called `rust`: plan, format, clippy, tests of the selected packages, the `local-mode` lane. Then `policy`, then `Required`. |
| Push to main | The same, plus docs, 18 "check crate alone" steps and cache saves. The one full job measured took 564 s, but it failed and ran no tests. The alone-checks were 48% of it, the lane guard 17%, docs 16%. |
| Plan step | `velnor-ci plan` picks packages from the diff using recorded metadata. If it is unsure, it selects everything. |
| Label CI | `pr_ci = "label"`: a ready PR gets a quick set (about 90 s). The tests run only with the label `ci:run` or `ci:auto`. A draft runs nothing and still gets a green check. |
| Runner | The official GitHub runner, on ARC (the Kubernetes controller that starts runners). |

The honest picture of the gate:

- **Main is red almost all the time.** After the cutover, Rust CI passed on 2 of 268 pull-request
  runs and 4 of 172 push runs (measured). The causes vary: lint failures, a recorded metadata file
  left stale, and a helper command or argument that the pinned helper or nextest rejected. They
  share one condition: each change merged without a green check that had run the tests.
- **Merges go in with a failing `Required`.** 167 of 173 merges after the cutover had a failing
  `Required` on the head commit (measured). The ruleset requires `Required` and allows an admin
  bypass, so they most likely went through it (inferred; the API does not report bypasses).
  "0.6 minutes from push to merge" measures that bypass, not CI.
- **`Required` can be green without tests.** It turns green on a draft, and on a ready PR with no
  label. PR #1799 merged that way 15 seconds after it was marked ready; its real run failed after
  the merge and main went red (measured).
- **Releases are not gated on a green main.** Tools 0.1.97 and 0.1.98 were published although the
  push run of the tagged commit had failed (measured).
- **The merge queue has not run since the cutover.** No test time exists yet: no labelled PR with
  tests has passed.
- **The team ships fast.** 48 commits landed in the 95 minutes after the panel's tip. Several are
  items of this plan already (#1768, #1754, #1764, #1763, #1770).

So the gap is correctness, not speed. The first answer to "how fast can it be" is: first make it
true.

## 3. The plan in six moves

### 3.1 velnor-gen is the verification compiler (R1)

velnor-gen already decides what the workflow runs. The plan makes it also write down what must
pass.

- Next to each workflow it writes a **verification contract**: a list of *obligations*. An
  obligation is one thing that must pass: format, lint, a set of tests, docs, a release build.
- Each obligation has an id, a recipe for its key, a reuse class and a due time. The file is
  sealed with the same digest header as `rust-ci.yml`.
- A **key** is a fingerprint of every input of the obligation. The generator fixes the first half
  (workflow, helper, toolchain, profile). `velnor-ci plan` fills in the second half at run time
  (the tree, the packages, the image). A test key that depends on built bytes waits until the
  build has made them.
- Each obligation ends up in one of three states: `Execute`, `Reused(receipt)` or
  `Excluded(reason)`. The gate can always say why something was required and whether it was
  proven.
- The gate stays `velnor_rust::decide`. It accepts a reuse receipt only when the key matches
  exactly.

### 3.2 Correctness first, in a dependency order (R2)

Nothing is bound in the ruleset until `Required` has been green on main with tests running. Phase 0
starts when you freeze the bypass: only a PR that repairs main may use it (Q3).

| Item | Step | Why |
|---|---|---|
| W0 | No green without tests | A draft and an unlabelled PR get a red `Required` at once. This closes the hole PR #1799 went through. |
| W1 | Green main, and tests that keep it green | velnor-gen checks every command it writes against the pinned helper's own list of commands. End-to-end tests cover what that list cannot see, and a stale metadata file fails a test. |
| W2 | An honest `Required` | See 3.3. |
| W4 | One lint lane | Move the `ci.yml` lint jobs into the generated workflow and delete `ci.yml`. The advisory scan still runs on every push to main and every night. |
| W3 | Generator freshness in the gate | Plan the `GeneratorCheck` step that exists but is never used. Delete `.velnor/config.toml` and the helper commands only the old engine used. It comes after W4, or the check would fail on `ci.yml`. |
| W5 | Release gate | The tools release needs the green push run of the tagged commit: the right workflow, the exact commit, the latest attempt (`velnor-ci release-gate`, fails closed). |
| W6, step 1 | Ruleset (you apply it) | Bind `Required` to the GitHub Actions app. Remove the bypass. Queue on ALLGREEN. Keep the other rules. A generated drift check fails if any of it is undone. |
| W8 | The first green baseline, measured | No new speed target is published before it. |
| W9 | Speed quick wins and selection hygiene | See section 4. |
| W10 | The contract in shadow, and the reduction check | See 3.1 and 3.4. |
| W7 | Decide what blocks landing | Then the push after a queue merge can reuse the queue's result, check by check, on W10's keys. |
| N26, W6 step 2 | The trusted verifier, then the ruleset requires its status | See 3.4. |

A change to the generator or the helper reaches CI only through a tools release and a pin, so
phase 0 needs three releases. Each needs your answer to D9.

Each step is a GitHub issue whose "done when" checks the state of main. If another session lands
the work first, the issue closes instead of being planned twice.

### 3.3 The label flow is what you asked for (R3)

| Situation | What happens |
|---|---|
| Ready, no label | Nothing runs. `Required` is red: "CI not requested". |
| Draft | Nothing runs. `Required` is red. |
| Label `velnor:verify` added | The latest commit runs. |
| Push while labelled | The new commit runs and cancels the old run. |
| Label removed | The run in flight is cancelled. `Required` turns red, also if it was green. |
| Unrelated label or title edit | Nothing changes. The event publishes no check and cancels nothing. |
| Base branch changed | The latest commit runs against the new base. |

- There is one label. `ci:run`, `ci:auto`, the `ci-run-label` job and the quick set on unlabelled
  PRs are deleted by W2. Until then they stay, because they are the only way to run a PR's tests.
- `Required` is never skipped on a pull-request event.
- A label put on a draft arms the PR when it becomes ready.
- The cost: an unlabelled PR shows one short red job per push until the Velnor App exists.
  This is accepted. A green check that verified nothing is worse.

### 3.4 A PR must not weaken its own gate (R4)

On PR and queue events GitHub runs the PR's own copy of the workflow. So a PR can delete any check
that the workflow runs on itself. It can also add a second workflow with a job named `Required`.
Once the bypass is gone, that is the realistic way to get a false green.

- **An accident guard (W3, W10).** The gate checks that generated files are fresh, and compares
  the PR's contract with the **base branch's** contract. It fails on a reduction you have not
  signed. This catches honest mistakes made through the configuration. It cannot stop a PR that
  removes the check.
- **A trusted verifier (N26).** A workflow that GitHub always takes from main checks the PR's tree
  as data. It writes a `Velnor / Verified` status through an App that a PR cannot use, and the
  ruleset requires that status (W6, step 2). Only then is the gate proof against a PR that edits
  its own workflow.
- **Later:** the `velnor-verify` App takes over and keeps state per PR.

### 3.5 The reuse ladder (R5)

Reuse takes the cheapest proof first. Each rung is added only when the one below is safe.

| Rung | Reuse |
|---|---|
| 1 | Same commit: a rerun of a verified commit is a lookup. |
| 2 | Queue to push: the push after a queue merge reuses the queue's result. |
| 3 | Same tree: a queue group whose tree is already verified is a lookup. |
| 4 | Same obligation key: per package, per test binary, only after a read audit shows the key covers every input. |

The same rules hold on every rung:

- **Evidence is per check.** A check is reused only when a record shows that this check ran and
  passed, and its full key is equal: tree, package selection, workflow, helper, toolchain, runner
  image. A green run proves nothing about a step it skipped. Today's lookup answers yes or no for a
  whole queue run, so rung 2 stays off until W7 replaces it.
- **Unclassified work runs fresh,** like anything that reads the clock, the network or randomness.
- **A rung turns on only after shadow comparisons** that were real would-be hits, each run again,
  with no mismatch and at least one hit.
- **No reuse before Q8.** `docs/goals.md` does not yet allow a receipt in place of a run.
- Records are GitHub cache entries named by the full key. Only push runs on main and the trusted
  verifier write records that others may trust. A queue run cannot read a PR's cache, so the
  verifier is the path from a PR's result to the queue. Until the lab proves it, rung 3 stays in
  shadow.
- The merge queue uses **ALLGREEN** (every commit in the group must pass). HEADGREEN (only the
  head must pass) is unsound with diff selection, because each entry is tested only on its own
  diff. This is Q4.

### 3.6 Velnor Native runs what velnor-gen writes (R6)

- For velnor-new, Velnor Native runs the **velnor-render dialect**: 16 pinned actions, one-command
  `run:` steps and a fixed set of expressions. It does not need the full GitHub Actions protocol
  for this. That removes most protocol work (items X02 to X12) and the native JavaScript-action
  work from our critical path. Whether Native may execute the plan without the Actions protocol
  at all is Q14.
- Full Actions compatibility stays the product goal for other repositories. It is a later stream
  and the owner gates it.
- The official runner stays as the switchable executor and the permanent reference. The executor
  is part of every result's key, so nothing is reused across executors.
- None of this starts until `docs/goals.md` is amended (Q1). Line 16 says the official runner
  executes every job.
- The runner, platform and storage crates get correctness fixes only (R7). New features wait for
  Q1 and Q9. They contradict "correctness first" while main is red.

### 3.7 Release is built once and promoted (R10)

- **Now (W5):** the generated tools release is gated on the green push run of the tagged commit.
- **Next:** the push run on main builds the release binaries. A tag promotes those exact bytes
  after the gate. Release no longer rebuilds them on hosted runners.
- A squash merge changes the commit the tools embed (`VELNOR_COMMIT`). So byte-equal promotion
  needs a signed release manifest that binds the binary digests to the tagged commit.
- Releases stay on GitHub-hosted runners, with attestation, until you decide otherwise (Q5).
- The `runner-v` release group is declared but has no workflow. Generating one waits for Q9 (Q17).

## 4. Why this can be the fastest runner

Start from what is measured, and it is not much. CI is free of the hosted-runner queue. A PR quick
run takes about 90 s. The one full push job measured took 564 s, failed and ran no tests. PR runs
with heavy lanes took P50 2.9 / P90 6.2 min, but on the older multi-job layout, and every one of
them failed or was cancelled. Test time is unknown. So the question is no longer "how do we escape
a 20-job queue". It is "how much of this work can we prove once and then skip".

Velnor removes work. It does not run the same work faster. The argument has five parts.

1. **A green main saves the cache.** A push saves the target cache only when it is green.
   Correctness is the first speed lever (R8).
2. **Stop verifying the same tree three times.** The PR, the queue and the push run the same tree.
   The reuse ladder (3.5) turns the second and third into lookups. Builds and cache saves still
   run on the push, so rung 2 saves lint and test time only.
3. **Decide where the post-merge tail runs.** Docs and the 18 alone-checks could block landing
   instead of running after it (Q13). In today's one job they would make each queue group about
   6 minutes later (estimate from one run). They cost no wait only in a second, parallel job, and
   W8 measures both shapes first.
4. **Place the paths the plan cannot place.** 40% of the last 396 commits touched a path that the
   plan could not place, so it selected everything. `[[inputs]]` entries fix that for paths with a
   proven owner, such as `generator/samples/**`. The workflow files and the `.velnor/`
   configuration always select everything, as `docs/goals.md` requires.
5. **Cheap measured wins:** the lane guard reads the test step's listing instead of listing three
   times; feature-lane lint and docs run only when the lane's package is selected; `policy` stops
   waiting for `rust`; the `velnor-ci` tests stop running one at a time; a multi-commit push
   fetches its base commit instead of selecting everything.

Native is an extra. For the generated dialect it can save per-job setup, such as Node start-up
for JavaScript actions and the Docker daemon each pod starts. That gain is unmeasured. It counts
only if the control race (S2) shows at least 5 s per job at P50 on three job types.

One idea is kept for after phase 0 because it may be the cheapest path to the proposed push-to-merge target:
a **speculative queue**. The label run tests the PR on top of the current queue tip, so the queue
group becomes a lookup. It needs measuring first (R13).

## 5. The honest limits

### 5.1 Targets are budgets until measured (R9)

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 |
|---|---|---|
| Proposed target (Q18) | 90 s / 4 min | 3 / 8 min |
| Trial budget after phase 0 | about 4 / 9 min | about 9 / 20 min |

- `docs/goals.md` sets other targets, and they bind: a run that selects no crate finishes within
  1 min; 30 s to the first cargo command; an ordinary PR's wait does not get worse, unless
  correctness needs a check and you accept its measured cost. W8 measures these, and Q18 asks you
  to accept the cost of the honest gate.
- The trial budgets are replaced by the W8 baseline once a green, test-running run exists.
- No dated target is published before that baseline. Dated targets as low as 45 s, given by three
  models, are not adopted.
- The 0.6 min "push-to-merge" after the cutover measures bypass latency, not CI. It is not a
  baseline.

### 5.2 What no runner can remove

- **GitHub's queue latency.** An event reaches Actions, a job gets scheduled, and the merge queue
  forms a group and merges it. These steps are GitHub's. They cost seconds to tens of seconds, and
  their size under the new CI is not yet measured. W8 will measure it. Push-to-merge is reported
  twice: gross, and net of GitHub's share.
- **The longest single test.** A test cannot be split across machines. Only fixing the test helps.
- **Paths that touch everything.** A commit that changes `.github/`, `.velnor/` or a root
  manifest affects every check. The P90 has no cheap floor for those.
- **Evidence that needs traffic.** Proving a new kind of reuse needs real merges. More agents
  cannot make them arrive faster.

### 5.3 Where the first answer may be slower

An unlabelled PR now shows a red `Required` until labelled. The first honest CI may be slower than
the bypass-merged one, because tests run and the cache is cold until main is green. The proposed
targets are about the steady state after the W8 baseline, not the first week.

## 6. How agents fit in

Agents write most of the code here. Every author is a project member (owner decision 8), so no
author is hostile, but an agent can still be wrong. So the gate must hold against honest mistakes.

- A later plan item (R13) lets one plan drive three targets: the generated YAML, Velnor Native,
  and a local `velnor verify` that agents run before they push.
- Selection becomes "what is not yet proven" instead of "what the diff touched", once per-package
  keys are proven. Until then selection by diff stays.
- Test binaries become the unit of reuse, keyed by nextest archive digests.
- Results from a Mac or an unverified run never count at the gate. They are advice.

None of this changes what the gate means. It only changes how soon the answer is ready.

## 7. How Velnor compares

Facts about other systems come from the [landscape dossier](runner-landscape.md) (mostly vendor
claims) and the [Dagger comparison](velnor-vs-dagger.md). No other system has been raced on this
repository. So every "should lead" below is a prediction until the benchmark gate (section 8)
runs.

| System | What it does well | Where Velnor should lead | Where it does not lead (yet) |
|---|---|---|---|
| GitHub-hosted runners | No operations. A clean VM per job. Larger machines on demand. GitHub's merge queue. | Verification reuse across PR, queue and push. No hosted-job queue on the verdict path. | Velnor needs a cluster that the project runs. |
| ARC with the official runner (today) | Runs the official runner exactly. Kubernetes-native. | Fewer repeated runs, no cold start for work that can be proven. | Mature. Native must pass conformance first. The executor-only gain is unmeasured (S2). |
| Buildkite-style persistent agents | Long-lived agents, git mirrors, warm state, timing-based test splits. | A tie on compile and tests (same cargo, same CPUs). A lead only through reuse and the verification contract. | Unmeasured. |
| Blacksmith, Depot, Namespace | High-clock CPUs, caches next to the machine, standby pools. Vendors claim about 2x. | They still start from an Actions job and repeat the run for PR, group and push. | Faster per thread on high-clock CPUs. No claim until a rented arm runs. |
| Bazel / Buck2 with remote execution | Cache and execution per action, hermetic by construction. Wins cold, wide builds. | Warm local cargo for small edits. No build rewrite. | Finer keys must be earned by audit. Bazel gets them by construction. |
| Dagger | Typed pipelines in eight languages. The same engine on laptop and CI. A content-addressed cache. Traces. A working product today. | The contract comes from the released generator and the base branch. Only `success` on an exact key passes. Existing YAML stays. | Dagger exists today. Velnor borrows its structured keys, named checks and OpenTelemetry traces. No Dagger code is reused. |

## 8. The honest speed claim and the benchmark gate

- **Today's claim:** none with a number. The plan is designed to cut the work per merge. How much
  is unmeasured.
- **After W8:** a budget for each stage, replaced by measured values.
- **After the benchmarks pass:** *the fastest measured push-to-merge on a specified velnor-new
  workload, at a stated resource budget, with the same verification contract.*
- **Never claimed:** "the fastest runner ever". A tie, an arm not run or a class lost narrows the
  sentence. One false green or skipped check disqualifies the result.

The benchmark suite S1 to S6 needs frozen contracts and two executors, so it runs in phase 3. A
separate, earlier check (N27) guards helper pins: a pin bump replays recent commits on the current
pin and on the candidate, and is blocked beyond a threshold fixed in advance.

| # | Benchmark | What it shows | Pass |
|---|---|---|---|
| S1 | Planted failures | 100 failing edits on every fast path (old-timestamp edit, hidden build input, new sibling file, clock shift, stale build, seeded flakes) | 100% red, or caught at the expected rate. It gates every other run. |
| S2 | Control race | Native against Official on the same warm machines, at least 30 interleaved pairs per job type | Per-type differences with intervals. An executor claim needs at least 5 s per job. |
| S3 | Head-to-head | Today's CI, a strong conventional baseline, Velnor with Official, Velnor Native | The publication rule below, on push-to-merge (gross and net) and the verdict. |
| S4 | Loaded replay | A busy day replayed with other load, then a manifest edit, a cold toolchain, a lost worker | Verdict P90 within 1.5x of the quiet value. |
| S5 | Census | How many checks read the clock or network, and how many keys stay whole-tree | Shares published. Goals restated from them. |
| S6 | Warm compile | 300 real diffs over a clone, idle and loaded, against a cold control | Slower run P90 at most 25 s. Outputs equal to cold. |

**Publication rule** (fixed in advance): Velnor is called faster than an arm only if, for each PR
class, the 95% intervals for both the P50 and the P90 difference sit entirely below zero. A claim
covers only the arms that ran.

## 9. Facts the plan must not repeat (R11)

| Claim | Correction |
|---|---|
| 31 alone-check steps | There are 18. |
| #1320 is open | It closed unmerged. |
| RUSTSEC-2026-0543 fails cargo-deny | It is already ignored. |
| The 5-minute queue wait adds 5 minutes per merge | Not when the minimum group size is 1. |
| `.velnor/rust-ci.toml` has `[[inputs]]` | The generator supports the key; this repository's file has no entry. The live plan does not read `.velnor/config.toml`. |
| The merge queue has never run | It ran before the cutover. It has not run since, and never with `Rust CI`. |
| Every red main had one cause | There were several (section 2). |

## 10. How the plan meets a fast-moving main (R12)

- The plan is delivered as GitHub issues with "done when" checks on the state of main. They link
  the PRs that already address them: #1608 to close as superseded, #1297 to close and let Renovate
  recreate.
- A question without a default, and every step only you can do, is an issue that blocks the work
  behind it.
- The `velnor-roadmap` and `velnor-runners-fc` sessions are told what is coming, so they do not
  open duplicates.
- The owner confirms the plan first. Nothing is created until then.

## 11. What the owner decides

The plan runs on a default for each question until you answer. The full sheet is
[owner-questions.md](owner-questions.md).

| Id | Question | What it blocks |
|---|---|---|
| Q3 | Freeze the bypass now; later remove it and bind the check (you apply it) | The start of phase 0; W6 |
| D9 | Agents cut tool patch releases from a green commit | Three release trains in phase 0 |
| Q2 | Confirm the label flow (3.3) | The honest `Required` (W2) |
| D7 | An agent identity and the status App | W6 and the trusted verifier |
| Q4 | Merge queue on ALLGREEN | W6 |
| Q12 | A gate the PR cannot weaken (3.4) | W10, N26, W6 step 2 |
| Q13 | What blocks landing a change | W7 |
| Q8 | Reuse a verified result, with one sentence for `docs/goals.md` | Every rung of the ladder (3.5) |
| Q18 | Proposed speed targets; the measured cost of the honest gate | W8 |
| Q1 | Amend `docs/goals.md` so Velnor Native may execute jobs | Every Native item (3.6) |
| Q14 | Native executes the plan without the Actions protocol | The size of the Native stream |
| Q5 | Where releases are built and hosted | Release promotion (3.7) |

The other questions (up to Q19) are in the sheet.

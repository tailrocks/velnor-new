# Velnor in plain words

This is a short guide to the Velnor plan for the owner. The decisions behind it are the
[rulings](velnor-gen-rebase/rulings.md) (R1 to R13). The details are in the
[design](../../proposed/velnor-runner-architecture.md), the
[delivery plan](delivery-plan.md) and the [owner questions](owner-questions.md) (Q1 to Q17).
The evidence is in the [as-built reports](velnor-gen-rebase/as-built/) and the
[comparison](velnor-gen-rebase/comparison.md).

**How to read the numbers.** Every number has a label:

- **(measured)**: observed on velnor-new CI, 2026-11-16 to 2026-11-19.
- **(budget)**: a number we aim for. It becomes a target only after the W8 baseline (R9). W8 is
  the work item that measures the first green, test-running CI.
- **(goal)**: a number the project has already committed to. It is met only when measurements
  show it.

**Nothing in the plan has run end to end yet.** Every figure for the new design is a budget or a
goal until the baseline and the benchmarks in section 8 exist.

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
| Generated workflows | `rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml` and the tools release workflow (`release-rust-ci-tools.yml`, since #1763). Each has a digest header, so a hand edit is detected. |
| Hand-written workflow | `ci.yml` (four lint jobs). It is the last one. |
| Pull request run | One big job called `rust`: plan, format, clippy, tests of the selected packages, the `local-mode` lane. Then `policy`, then `Required`. |
| Push to main | The same, plus docs, 18 "check crate alone" steps and cache saves. The full job takes 564 s (measured). The alone-checks are 48% of it, the lane guard 17%, docs 16%. |
| Plan step | `velnor-ci plan` picks packages from the diff using recorded metadata. If it is unsure, it selects everything. |
| Label CI | `pr_ci = "label"`: a ready PR gets a quick set (about 90 s). The tests run only with the label `ci:run` or `ci:auto`. |
| Runner | The official GitHub runner, on ARC (the Kubernetes controller that starts runners). |

The honest picture of the gate:

- **Main is red almost all the time.** After the cutover, Rust CI passed on 2 of 268 pull-request
  runs and 4 of 172 push runs (measured). The causes are small and repeated. The generator wrote
  a helper command or flag that the pinned helper could not run.
- **Merges go in with a failing `Required`.** 167 of 173 merges after the cutover had a failing
  `Required` on the head commit (measured). The ruleset requires `Required` and allows an admin
  bypass, so they most likely went through it (inferred; the API does not report bypasses).
  "0.6 minutes from push to merge" measures that bypass, not CI.
- **`Required` can be green without tests.** It turns green on a draft, and on a ready PR with no
  label. A skipped `Required` also counts as passing on an unrelated event.
- **The merge queue is configured but has never run** under the new CI. No test time exists yet:
  no labelled PR with tests has passed since the cutover.
- **The team ships fast.** 28 commits landed in the two hours after the panel's tip. Several are
  items of this plan already (#1768, #1754, #1764, #1763, #1765).

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

### 3.2 Correctness first, in a strict order (R2)

Nothing is bound in the ruleset until `Required` has been green on main with tests running.

| # | Step | Why |
|---|---|---|
| 1 | Green main, and a contract that keeps it green | velnor-gen reads the pinned helper's command schema. It refuses to write a step the pin cannot run. This removes the repeated cause of red. |
| 2 | An honest `Required` | See 3.3. |
| 3 | Generator freshness in the gate | Plan the `GeneratorCheck` step that exists but is never used. Delete `.velnor/config.toml` and the helper commands only the old engine used. |
| 4 | One lint lane | Move the four `ci.yml` jobs into the generated lint list. Delete `ci.yml`. |
| 5 | Release gate | The tools release needs the exact green push run of the tagged commit (`velnor-ci release-gate`, fails closed). |
| 6 | Ruleset (the owner applies it) | Bind `Required` to the GitHub Actions app. Remove the bypass. Queue on ALLGREEN. Allow auto-merge. Keep the four protections. A generated drift check fails if any of it is undone. |
| 7 | Decide what blocks landing | Then the push after a queue merge can reuse the queue's result. |
| 8 | The first green baseline, measured | No new speed target is published before it. |
| 9 | Speed quick wins and selection hygiene | See section 5. |
| 10 | The contract in shadow, plus the base-branch check | See 3.4. |

Each step is a GitHub issue whose "done when" checks the state of main. If another session lands
the work first, the issue closes instead of being planned twice.

### 3.3 The label flow is what you asked for (R3)

| Situation | What happens |
|---|---|
| Draft | Nothing runs. |
| Ready, no label | Nothing runs. `Required` is red: "CI not requested". |
| Label `velnor:verify` added | The latest commit runs. |
| Push while labelled | The new commit runs and cancels the old run. |
| Label removed | The run in flight is cancelled. |
| Unrelated label or title edit | The commit keeps its verdict. No new run can hide it. |

- There is one label. `ci:run`, `ci:auto`, the `ci-run-label` job and the quick set on unlabelled
  PRs are deleted. No third mode.
- `Required` is never skipped on a pull-request event.
- A label put on a draft arms the PR when it becomes ready.
- The cost: an unlabelled PR shows one short red job per push until the Velnor App exists.
  This is accepted. A green check that verified nothing is worse.

### 3.4 A PR must not weaken its own gate (R4)

Today the workflow and its policy come from the PR's own tree. A PR can edit `rust-ci.yml` or
`.velnor/rust-ci.toml` and loosen what `Required` demands. Once the bypass is gone, that is the
realistic way to get a false green.

- **Now (W10):** the gate compares the PR's contract with the **base branch's** contract. It fails
  on any reduction that is not signed by the owner for that exact diff (Q12).
- **Next:** a trusted `workflow_run` verifier checks out nothing, runs the released
  `velnor-gen check` on the tree as data, and writes a shadow `Velnor / Verified` status.
- **Later:** the `velnor-verify` GitHub App makes that status the required check.

### 3.5 The reuse ladder (R5)

Reuse takes the cheapest proof first. Each rung is added only when the one below is safe.

| Rung | Reuse | Why it is safe |
|---|---|---|
| 1 | Same commit: a rerun of a verified commit is a lookup | The key is the commit and the contract. |
| 2 | Queue to push: the push after a queue merge reuses the queue's result | A step is skipped on push only if the queue ran it. Safe since #1764. |
| 3 | Same tree: a queue group whose tree is already verified is a lookup | The tree digest is the key. |
| 4 | Same obligation key: per package, per test binary | Only after a read audit shows the key covers every input. |

- Phase-1 records need no new service. They are GitHub cache entries named by the verdict key.
  GitHub's own cache scopes enforce trust: a PR reads main's records and writes only its own.
- A separate `velnor-ledger` service comes only when a measured need exists.
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

- **Now:** the generated tools release is gated on the exact green push run.
- **Next:** the push run on main builds the release binaries. A tag promotes those exact bytes
  after the gate. Release no longer rebuilds them on hosted runners.
- A squash merge changes the commit the tools embed (`VELNOR_COMMIT`). So byte-equal promotion
  needs a signed release manifest that binds the binary digests to the tagged commit.
- Releases stay on GitHub-hosted runners, with attestation, until you decide otherwise (Q5).
- The `runner-v` release group is declared but has no workflow. Generating one is a phase-1 item.

## 4. Why this can be the fastest runner

Start from the new baseline, not the old one. CI is already free of the hosted-runner queue. A
PR quick run takes about 90 s and a PR run with heavy lanes takes P50 2.9 / P90 6.2 min
(measured, post-cutover, a 4.3-hour sample). A full push `rust` job takes 564 s (measured). Test
time is unknown. So the question is no longer "how do we escape a 20-job queue". It is "how much
of this work can we prove once and then skip".

Velnor removes work. It does not run the same work faster. The argument has five parts.

1. **A green main saves the cache.** Only a green push saves the target cache. A red main makes
   every run cold. Correctness is the first speed lever (R8).
2. **Stop verifying the same tree three times.** The PR, the queue and the push run the same tree.
   The reuse ladder (3.5) turns the second and third into lookups.
3. **Cut the post-merge tail.** The 18 alone-checks are 48% of a push job. Move the tail into the
   queue, in parallel, and restrict alone-checks to released crates.
4. **Place the paths the plan cannot place.** 40% of the last 396 commits touched a path that the
   plan could not place, so it selected everything. `[[inputs]]` entries for
   `generator/samples/**`, `.github/workflows/**` and `local/**` fix that. They are proven by
   replaying 400 commits.
5. **Shape the job from measured timings.** Small wins: the lane guard reads the test step's
   listing instead of listing three times; feature-lane lint and docs run only when the lane's
   package is selected; `policy` stops waiting for `rust`; the `velnor-ci` tests stop running one
   at a time.

Native is an extra. For the generated dialect it can save per-job setup, such as Node start-up
for JavaScript actions and the Docker daemon each pod starts. That gain is unmeasured. It counts
only if the control race (S2) shows at least 5 s per job at P50 on three job types.

One idea is kept for after phase 0 because it may be the cheapest path to the push-to-merge goal:
a **speculative queue**. The label run tests the PR on top of the current queue tip, so the queue
group becomes a lookup. It needs measuring first (R13).

## 5. The honest limits

### 5.1 Targets are budgets until measured (R9)

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 |
|---|---|---|
| Goal, unchanged | 90 s / 4 min | 3 / 8 min |
| Trial budget after phase 0 | about 4 / 9 min | about 9 / 20 min |

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
the bypass-merged one, because tests run and the cache is cold until main is green. The goal
numbers are about the steady state after step 8 of the order, not the first week.

## 6. How agents fit in

Agents write most of the code here. Every author is a project member (owner decision 8), so no
author is hostile, but an agent can still be wrong. So the gate must hold against honest mistakes.

- A later plan item (R13) lets one plan drive three targets: the generated YAML, Velnor Native,
  and a local `velnor verify` that agents run before they push.
- Selection becomes "what is not yet proven" instead of "what the diff touched". It needs no base
  commit, so `fetch-depth` stops mattering.
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

The benchmark suite S1 to S6 is now **generator-owned**. It is a workflow that replays recent
commits on the pinned helper and on a candidate. A pin bump lands only if the candidate is not
slower.

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
| `.velnor/rust-ci.toml` has `[[inputs]]` | It has none. The path rules live in `.velnor/config.toml`. |

## 10. How the plan meets a fast-moving main (R12)

- The plan is delivered as GitHub issues with "done when" checks on the state of main. They link
  the PRs that already address them: #1770 now, #1608 to close as superseded, #1297 to close and
  let Renovate recreate.
- The `velnor-roadmap` and `velnor-runners-fc` sessions are told what is coming, so they do not
  open duplicates.
- The owner confirms the plan first. Nothing is created until then.

## 11. What the owner decides

The plan runs on a default for each question until you answer. The full sheet is
[owner-questions.md](owner-questions.md).

| Id | Question | What it blocks |
|---|---|---|
| Q1 | Amend `docs/goals.md` so Velnor Native may execute jobs | Every Native item (3.6) |
| Q2 | Confirm the label flow (3.3) | The honest `Required` |
| Q3 | Remove the admin bypass and update the ruleset (you apply it) | Step 6 of the order |
| Q4 | Merge queue on ALLGREEN | Step 6, and queue reuse |
| Q5 | Where releases are built and hosted | Release promotion (3.7) |
| Q12 | The gate takes its rules from the base branch | The W10 check (3.4) |
| Q13 | What blocks landing a change | Step 7 of the order |
| Q14 | Native executes the plan without the Actions protocol | The size of the Native stream |

The other questions (up to Q17) are in the sheet.

# Rulings: the Velnor plan rebased on velnor-gen

| Field | Value |
|---|---|
| Author | Claude Opus 5.5 (`claude-opus-5-5`), panel manager |
| Date | 2026-11-20 |
| Inputs | the [nine answers](panel/), the [comparison](comparison.md), the [as-built reports](as-built/), main from `0eb2d76a5c` to `515d4c7ff1` |
| Status | Proposed. The owner decides. Nothing here changes GitHub settings, `docs/goals.md` or a hard decision |

These rulings settle every point where the panel split, and they are the single source for the updated [design](../../../proposed/velnor-runner-architecture.md), [user experience](../../../proposed/velnor-user-experience.md), [delivery plan](../delivery-plan.md), [owner questions](../owner-questions.md) and [explainer](../explainer.md). Where one of those disagrees with a ruling, the ruling wins.

## What changed under the plan

- **The old engine is gone.** `velnor-actions` and its 12 crates were deleted (#1747, −450k lines). velnor-gen writes every generated workflow: `rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml`, `tag-ruleset-drift.yml` and, since #1763, `release-rust-ci-tools.yml`. `ci.yml` (four lint jobs: Alint, Cargo Deny, Cargo Machete, Jactionlint) is the last hand-written workflow.
- **CI is one big job.** A PR runs `rust` (plan, fmt, clippy, tests of the selected packages, the `local-mode` lane), then `policy`, then `Required`. A push to main adds docs, 18 "check crate alone" steps and cache saves. The plan step (`velnor-ci plan`) chooses packages from the diff using recorded metadata and fails closed to "everything".
- **The team ships fast.** 48 commits landed in the 95 minutes after the panel's tip (`0eb2d76a5c..515d4c7ff1`), among them several of the panel's first items: #1768 (nextest preflight), #1754 (alint and deny), #1764 (post-merge skip), #1763 (generated release workflow), #1770 (an ignored event publishes no `Required`), and helpers 0.1.96 to 0.1.98 with their pins.
- **Correctness is the gap, not speed.** Since the cutover main has been red almost continuously, and most merges went in with a failing `Required` through the admin bypass. `Required` is also green on a draft and on a ready PR without a label, without running tests: PR #1799 merged that way 15 seconds after it was marked ready (run 44599847337), and main went red. Tools 0.1.97 and 0.1.98 were published although the push run of the tagged commit had failed (runs 44596703249, 44599294031). The merge queue has not run since the cutover, and `Rust CI` has never run on `merge_group`.

## Rulings

### R1. velnor-gen is the verification compiler

All nine models agree, and six call it the most important change. The plan's centre moves from the runner to the generator.

- velnor-gen emits a **verification contract** next to the workflow: one obligation per thing that must pass (format, lint, a test set, docs, a release build), each with an id, a key recipe, a reuse class and when it is due. It is written to a sealed file with the same digest header as `rust-ci.yml` (Fable's `.velnor/verify.lock`).
- The **definition half** of every key is fixed at generation (workflow digest, helper digest, toolchain, profile). The **input half** (tree, packages, image, binary digests) is resolved by `velnor-ci plan` at run time. A test-run key that depends on built bytes waits until the build has produced them (Astra, Sol).
- Selection keeps `RustSelection`. A ledger hit does not turn a package into "not selected". Each obligation gets a disposition, `Execute`, `Reused(receipt)` or `Excluded(reason)`, so the gate can always say why something was required and whether it was proven (Astra, Sol; Muse Wonder's `None` conversion is rejected).
- The gate stays `velnor_rust::decide` and is extended to accept a reuse receipt only when its key matches exactly.

### R2. Phase 0 is correctness, in a dependency order

Nothing reaches the ruleset until `Required` has been green on main **with tests running**. Main went red for several reasons: lint failures, a metadata file left stale after a manifest edit, and helper skew. One condition let all of them land: **a change merged without a green, test-running `Required`**, through the bypass or on a green check that ran nothing. Phase 0 removes that condition first.

It starts with **the bypass freeze** (Q3): the admin bypass is used only for a PR that repairs main. Then, in the order of work (the ids are the delivery plan's):

- **W0. No green without tests.** A draft and an unlabelled ready PR get a red `Required` at once. `ci:run` and `ci:auto` stay until W2, and every phase-0 PR carries one of them, so its tests run.
- **W1. Green main, and tests that keep it green.** velnor-gen reads the **pinned helper's command schema** and refuses to generate a step the pin cannot parse. End-to-end tests render, plan and preflight with the pinned helper and nextest, and a manifest edit without regeneration fails a test.
- **W2. An honest `Required`** (owner decision 12, R3).
- **W4. One lint lane.** Alint and Jactionlint move into `[[lint]]`, `cargo deny` and `cargo machete` stay only in `policy`, and `ci.yml` is deleted. The advisory scan still runs on every push to main and every night.
- **W3. Generator freshness in the gate.** Plan `StepKind::GeneratorCheck`, run it from the pinned release, and make `Required` depend on it. Delete `.velnor/config.toml` and the dead engine commands. W3 follows W4, because that deletion makes the check report `ci.yml` as an unmarked file.
- **W5. Release gate.** The tools release requires the green push run of the tagged commit: the workflow file, the exact commit, the latest attempt, fail closed (`velnor-ci release-gate`). #1608 is superseded.
- **W6, step 1. Ruleset (owner applies).** Bind `Required` to the GitHub Actions app, empty the bypass list, switch the queue to ALLGREEN and keep every other rule. Auto-merge is a separate setting. Agents act under an identity that cannot edit the ruleset (D7). A generated drift check fails if any of it is undone.
- **W8. The first green baseline**, with push reuse off, before any new speed target.
- **W9. Speed quick wins and selection hygiene** (R8).
- **W10. The contract in shadow and the reduction check** (R1, R4). It can start once W3 is in.
- **W7. Decide what blocks landing**, then let the push after a queue merge reuse the queue's result per obligation, on W10's keys (R5).
- **N26, the trusted verifier, then W6, step 2** (R4).

Generator and helper changes reach CI only through a tools release and a re-pin (goals rule 12), so the items ship in three release trains, each under D9. Each item is an issue whose "done when" checks state on main, so work that another session lands first closes the item instead of being planned again.

### R3. The label flow is exactly what the owner asked for

- One label, `velnor:verify`. `ci:run`, `ci:auto`, the `ci-run-label` job and the quick set on unlabelled PRs are deleted by W2, not earlier: until then they are the only way to run a PR's tests. `pr_ci = "label"` is rewritten in place; no third mode.
- Draft: nothing runs. Ready without the label: nothing runs. Label added: the latest commit runs. Every push while labelled runs and cancels the previous run. Label removed: the run in flight is cancelled and `Required` on that commit turns red, also when it was green (decision 12: an unlabelled PR never has a green required check).
- `Required` is **never skipped** on a pull-request event and is **red** ("CI not requested") on a draft and on an unlabelled PR. A label carried by a draft arms the PR when it becomes ready.
- An unrelated label added or removed, or a title or body edit, publishes nothing named `Required` and cancels nothing: it takes the ignored-event path main already has. A change of the base branch is not ignored; the latest commit runs against the new base.
- Phase 0 reuses no earlier verdict by commit SHA. A check on GitHub belongs to a commit, so two PRs with one head commit share the newest `Required`; the lab records it, and state per PR waits for the App.
- The cost: an unlabelled PR shows one short red job per push until the Velnor App exists. Accepted, because a green check that verified nothing is worse.

### R4. A PR must not be able to weaken its own gate

On `pull_request` and `merge_group` events GitHub runs the candidate's own copy of the workflow, with the helper it pins. A check that the workflow runs on itself can be removed by the same PR, and binding `Required` to the GitHub Actions app (integration 15368) accepts a job of that name from any workflow file. Agents do the work (decision 15) and can be wrong, so this is the realistic false-green path once the bypass is gone.

- **Accident guards (W3, W10).** Inside the workflow, the generator check and a comparison of the PR's contract with the base branch's (velnor-gen's "check does less" detection) turn red on a stale file and on a reduction made through configuration. They do not stop a PR that deletes the check, adds a second workflow with a job named `Required`, or pins an older helper.
- **The trusted verifier (N26, in phase 0).** A `workflow_run` workflow, which GitHub always takes from the default branch. It checks out nothing and runs main's pinned helper on the candidate tree as data: freshness, reductions, the helper pin, and that the `rust-ci.yml` run of that exact commit passed on its latest attempt. It writes `Velnor / Verified` through an App whose key only `main` can use, and the ruleset requires that status from that App (W6, step 2).
- **What W6 may claim.** After step 1: nothing merges on red, on a draft, without the label or around the queue. After step 2: a PR cannot weaken its own gate.
- **A signed reduction.** The owner signs, with an SSH key listed in main's `.velnor/allowed_signers`, the two contract digests, the list of reductions and an expiry. A rebase voids the signature (this answers D17).
- GitHub's own "required workflow" rule would do the same without an App, but it needs an organisation ruleset, which the organisation's plan lacks (an option in Q12). **Later**, the `velnor-verify` App takes over and keeps state per PR.

### R5. Reuse climbs a ladder, cheapest proof first

1. **Same commit:** a rerun of a commit already verified is a lookup.
2. **Queue to push:** the push after a queue merge reuses the queue's result (`skip_push_after_merge_group`).
3. **Same tree:** a queue group whose tree equals an already-verified tree is a lookup (Opus's `ReusePoint::SameTree`, generalising `Step.skip_after_merge_group`).
4. **Same obligation key:** per-package and per-test-binary keys, only after a read audit proves the key covers every input.

Rules for every rung:

- **Evidence is per obligation.** An obligation is reused only when a record shows that it ran and passed, and the record's full resolved key equals the request's: tree, package selection, workflow and helper digests, toolchain, runner image and executor. Today's lookup returns one yes or no for a whole queue run, and the queue and the push can select differently, so rung 2 stays off until W7 replaces it.
- **Unclassified work runs fresh,** like anything that reads the clock, the network or randomness, until the classifier (V12) and the Git-metadata census (V14) have classified it.
- **A rung turns on only after shadow comparisons** counted as real would-hits, each re-executed, with at least one valid hit; the hit rate is published.
- **No rung before Q8.** Reuse satisfies a selected check with a receipt instead of a run, which `docs/goals.md` ("green means verified") does not yet allow.

Phase-1 records need no new service: they are GitHub cache entries keyed by the full key (Fable). A cache scope says who may read an entry, not who observed the result, and a queue run cannot read a PR's scope. So trusted records are written only by push runs on main and by the trusted verifier, which is also the path from a PR's result to the queue; until the lab proves that path, same-tree reuse stays in shadow. A `velnor-ledger` service comes only on a measured need.

Audit A1 keeps a frontier on main's first-parent history and audits every landed commit, whether or not it had a push run. Selection by diff stays until per-package keys are proven: with whole-tree keys, "what is not yet proven" would select everything on any edit.

The **queue uses ALLGREEN**. HEADGREEN is unsound with diff selection, because each group entry is tested only on its own diff (Fable; verified in the comparison).

### R6. Velnor Native executes what velnor-gen writes

The design's executor half stays, with a narrower first target:

- For velnor-new, Velnor Native runs the **dialect velnor-render emits**: 16 pinned actions, one-command `run:` steps and a fixed set of expressions. Opus and Fable both reached this. It removes most of the Actions-protocol items (X02–X12) and the native JavaScript-action items from velnor-new's critical path.
- Full GitHub Actions protocol compatibility stays the product goal for other repositories, as a later stream gated by the owner.
- GitHub Official stays the switchable executor and the permanent reference. The executor's identity is part of every result, so nothing is reused across executors.
- None of this starts before `docs/goals.md` is amended (Q1 in the owner sheet), because goals.md line 16 says the official runner executes every job.

### R7. No runner, platform or storage crates in phase 0

Three models put them there. That contradicts the panel's own "correctness first" verdict and adds review load while main is red. The runner crates (`velnor-runner-*`) get correctness fixes only, and new features wait for Q1 and Q9.

### R8. Speed levers, in order

The one full push job measured (run 44584072510, `57b65af0db`, helper 0.1.95) took 564 s and **failed**: Lint failed, Test failed after 15 s and ran no tests, and the target cache was restored. The 18 alone-checks were 48% of it, the lane guard 17% and docs 16%, shares of a job without its largest part. PR runs with heavy lanes took 2.9 / 6.2 min at P50 / P90, on the multi-job layout that #1594 replaced, and every one failed or was cancelled. Test time is unknown. W8 measures all of it again on green runs.

1. **A green main.** A push saves the target cache only when it is green.
2. **Stop verifying the same tree three times** (R5). Rung 2 saves lint and test time; builds and cache saves still run.
3. **Decide where the post-merge tail runs** (Q13); alone-checks only for released crates. Steps of one job run in series, so moving docs and alone-checks into the queue makes each group later by their run time (about 6 min on the run above) unless they go into a second, parallel job, which needs a measured gain (Q7). W8 measures both shapes first.
4. **Map the paths the plan cannot place.** 40% of the last 396 commits touched one and selected everything. `[[inputs]]` entries name only paths with a proven owner, such as `generator/samples/**`. The paths `docs/goals.md` says select everything stay wide: the workflow, `.velnor/` configuration and locks, the toolchain file, cargo configuration. `.github/workflows/**` is never narrowed, and the replay publishes the unknown-path and select-everything counts separately.
5. **Cheap measured wins:** the lane guard reads the test step's listing; feature-lane lint and docs run only when the lane's package is selected; `policy` no longer waits for `rust`; the `velnor-ci` tests stop running one at a time; a multi-commit push fetches its base commit instead of selecting everything.
6. **Shape from measured timings.**

### R9. Targets are budgets until measured

- **`docs/goals.md` sets the binding targets:** a run that selects no crate finishes within 1 min; 30 s from job start to the first cargo command; the generator's tests under 1 min; an ordinary PR's wait and runner-minutes do not get worse, except for a check correctness needs, with its cost measured and accepted by the owner. W8 measures these. Phase 0 adds such checks, so Q18 asks the owner to accept their measured cost.
- **Proposed targets, not in `docs/goals.md`:** PR verdict P50 ≤ 90 s, P90 ≤ 4 min; push-to-merge P50 ≤ 3 min, P90 ≤ 8 min. They come from the earlier design round (the decision log, H5), not from an owner decision (Q18).
- **Trial budgets after phase 0:** PR verdict about 4 / 9 min, push-to-merge about 9 / 20 min, to be replaced by the W8 baseline. The post-cutover "0.6 min push-to-merge" measures bypass latency and is not a baseline.
- No dated target is published before a green, test-running baseline exists. The dated targets three models gave (as low as 45 s) are not adopted.
- The "fastest runner" claim waits for the S1–S6 suite, which needs frozen contracts and executor arms that exist only in phase 3. **N27 is a separate, earlier check for helper pins:** a pin bump replays recent commits on the current pin and on the candidate, and is blocked beyond a threshold fixed in advance from W8's run-to-run spread.

### R10. Release is built once and promoted

- **Now:** the generated tools release is gated on the green push run of the tagged commit (W5). Until W5 lands, a release PR shows that run and its `Required` conclusion, and the release that ships the new gate is published under that manual rule.
- **Next:** the push run on main builds the release binaries. A tag promotes those exact bytes after the gate, instead of rebuilding them on hosted runners.
- **Squash changes the commit the tools embed** (`VELNOR_COMMIT`), so byte-equal promotion needs a signed release manifest that binds the binary digests to the tagged commit (Astra, Sol).
- **Release hosting:** releases stay on GitHub-hosted runners with attestation until the owner decides otherwise (Q5). This is a stated exception to decisions 9 and 10 until the promotion item lands.
- **`runner-v`:** the release group is declared but has no workflow. Generating one (N07) waits for Q9 and Q17.

### R11. Facts the plan must not repeat

These claims appeared in the brief, in panel answers or in an earlier draft of this plan, and are wrong:

| Claim | Correction |
|---|---|
| 31 alone-check steps | There are 18 |
| #1320 is open | It closed unmerged |
| RUSTSEC-2026-0543 fails cargo-deny | It is already ignored |
| The 5-minute queue wait adds 5 minutes per merge | It does not apply when the minimum group size is 1 |
| `.velnor/rust-ci.toml` has `[[inputs]]` | The generator supports the key; this repository's file has no entry. The live plan does not read `.velnor/config.toml` |
| The merge queue has never run | 289 `CI` group runs before the cutover; none since, and none of `Rust CI` |
| A digest header means a hand edit is detected | No CI step runs `velnor-gen check` today (W3) |
| Every red main had one cause, helper skew | Several causes (R2) |
| `ci.yml` has a generator-samples lint | It has four jobs and no such check |

Report A's timings came from the multi-job layout that #1594 replaced. Gemini 4 Carbon cited files that do not exist, and Gemini 4 Argon and Muse Wonder cited some stale paths; [the comparison](comparison.md#4-hallucination-check-cited-items-that-do-not-exist-at-0eb2d76a5c) lists them, and none of those claims is used.

### R12. How the plan meets a fast-moving main

- The plan is delivered as GitHub issues with state-based "done when" checks, linked to the PRs that already address them (#1608 to close as superseded; #1297 to close and let Renovate recreate).
- An unanswered owner question and an owner action are issues that block their dependants natively, so the roadmap's ready query never starts a gated item.
- Before issues are created, the `velnor-roadmap` and `velnor-runners-fc` sessions are told what is coming, so that they do not open duplicates.
- The owner confirms the plan first. Nothing is created until then.

### R13. Ideas kept for after phase 0

- One plan drives three targets: the generated YAML, Velnor Native, and local `velnor verify` for agents.
- Selection becomes "what is not yet proven" instead of "what the diff touched", once per-package keys are proven. It needs no base commit, so `fetch-depth` stops mattering.
- Test binaries become the unit of reuse (nextest archive digests).
- The branch ruleset is generated with a drift check, like the tag rulesets already are.
- **Speculative queue:** the label run tests the PR on top of the current queue tip, so the queue group becomes a lookup. It may be the cheapest path to the proposed 3 / 8 min push-to-merge target. It needs measurement first.
- Plan edges (`Step.after`) let Velnor Native run independent steps of one job at the same time.

# Rulings: the Velnor plan rebased on velnor-gen

| Field | Value |
|---|---|
| Author | Claude Opus 5.5 (`claude-opus-5-5`), panel manager |
| Date | 2026-11-20 |
| Inputs | the [nine answers](panel/), the [comparison](comparison.md), the [as-built reports](as-built/), main from `0eb2d76a5c` to `a75aae380f` |
| Status | Proposed. The owner decides. Nothing here changes GitHub settings, `docs/goals.md` or a hard decision |

These rulings settle every point where the panel split, and they are the single source for the updated [design](../../../proposed/velnor-runner-architecture.md), [user experience](../../../proposed/velnor-user-experience.md), [delivery plan](../delivery-plan.md), [owner questions](../owner-questions.md) and [explainer](../explainer.md).

## What changed under the plan

- **The old engine is gone.** `velnor-actions` and its 12 crates were deleted (#1747, −450k lines). velnor-gen writes every generated workflow: `rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml` and, since #1763, `release-rust-ci-tools.yml`. `ci.yml` (four lint jobs) is the last hand-written workflow.
- **CI is one big job.** A PR runs `rust` (plan, fmt, clippy, tests of the selected packages, the `local-mode` lane), then `policy`, then `Required`. A push to main adds docs, 18 "check crate alone" steps and cache saves. The plan step (`velnor-ci plan`) chooses packages from the diff using recorded metadata and fails closed to "everything".
- **The team ships fast.** 28 commits landed in the two hours after the panel's tip. They include several of the panel's own first items: the nextest preflight fix (#1768), alint and deny (#1754), the post-merge skip fix (#1764), the generated release workflow (#1763), helper 0.1.96 (#1765, pinned in #1773) and the batched-inputs version gate (#1777). #1770 (ignored events must not skip `Required`) is open.
- **Correctness is the gap, not speed.** Since the cutover main has been red almost continuously, and most merges went in with a failing `Required` through the admin bypass. `Required` also turns green on a draft and on a ready PR without a label, without running tests. The merge queue is configured but has never run.

## Rulings

### R1. velnor-gen is the verification compiler

All nine models agree, and six call it the most important change. The plan's centre moves from the runner to the generator.

- velnor-gen emits a **verification contract** next to the workflow: one obligation per thing that must pass (format, lint, a test set, docs, a release build), each with an id, a key recipe, a reuse class and when it is due. It is written to a sealed file with the same digest header as `rust-ci.yml` (Fable's `.velnor/verify.lock`).
- The **definition half** of every key is fixed at generation (workflow digest, helper digest, toolchain, profile). The **input half** (tree, packages, image, binary digests) is resolved by `velnor-ci plan` at run time. A test-run key that depends on built bytes waits until the build has produced them (Astra, Sol).
- Selection keeps `RustSelection`. A ledger hit does not turn a package into "not selected". Each obligation gets a disposition, `Execute`, `Reused(receipt)` or `Excluded(reason)`, so the gate can always say why something was required and whether it was proven (Astra, Sol; Muse Wonder's `None` conversion is rejected).
- The gate stays `velnor_rust::decide` and is extended to accept a reuse receipt only when its key matches exactly.

### R2. Phase 0 is correctness, in a strict order

Nothing reaches the ruleset until `Required` has been green on main **with tests running**. The order is:

1. **Green main, and a contract that keeps it green.** Every time main went red after the cutover, the cause was the same kind of thing: the generator rendered a helper command or flag the pinned helper could not run. The fix is structural. velnor-gen reads the **pinned helper's command schema** and refuses to generate a step the pin cannot parse, so a regeneration that the pin cannot run fails before it merges.
2. **An honest `Required`** (owner decisions 15–17, R3).
3. **Generator freshness in the gate.** `StepKind::GeneratorCheck` exists but nothing plans it; plan it, run it from the pinned release, and make `Required` depend on it. Delete `.velnor/config.toml` and the helper commands that only the deleted engine used.
4. **One lint lane.** Move the four `ci.yml` jobs into `[[lint]]` and delete `ci.yml`.
5. **Release gate.** The generated tools release requires the exact green push run of the tagged commit (`velnor-ci release-gate`, fail closed). #1608 is superseded by doing this in the generated workflow.
6. **Ruleset (owner applies).** Bind `Required` to the GitHub Actions app, remove the bypass, switch the queue to ALLGREEN, allow auto-merge, and keep the four existing protections (deletion, non-fast-forward, linear history, pull request). A generated drift check fails if any of it is undone.
7. **Decide what blocks landing**, then let the push after a queue merge reuse the queue's result (R5).
8. **The first green baseline**, measured, before any new speed target is published.
9. **Speed quick wins and selection hygiene** (R8).
10. **The contract in shadow, plus a base-branch check** (R4).

Each item is written as an issue whose "done when" checks state on main, so work that another session lands first closes the item instead of being planned again.

### R3. The label flow is exactly what the owner asked for

- One label, `velnor:verify`. `ci:run`, `ci:auto`, the `ci-run-label` job and the quick set on unlabelled PRs are deleted. `pr_ci = "label"` is rewritten in place; no third mode.
- Draft: nothing runs. Ready without the label: nothing runs. Label added: the latest commit runs. Every push while labelled runs and cancels the previous run. Label removed: the run in flight is cancelled.
- `Required` is **never skipped** on a pull-request event and is **red** ("CI not requested") on any commit that has no verdict. A label carried by a draft arms the PR when it becomes ready.
- An unrelated label or a title edit keeps the commit's existing verdict; it never starts a run that could hide it.
- The cost: an unlabelled PR shows one short red job per push until the Velnor App exists. Accepted, because a green check that verified nothing is worse.

### R4. A PR must not be able to weaken its own gate

Today the workflow and its policy come from the PR's own tree, so a PR can edit `rust-ci.yml` or `.velnor/rust-ci.toml` and loosen what `Required` demands. Owner decision 8 says agents are trusted but fallible, so this is the realistic false-green path once the bypass is gone.

- **Now (phase 0, W10):** the gate compares the PR's contract with the **base branch's** contract and fails on any reduction that is not signed by the owner for that exact diff (owner decision 17; it reuses velnor-gen's existing "check does less" detection).
- **Next:** a trusted `workflow_run` verifier that checks out nothing, runs the released `velnor-gen check` on the tree as data, and writes a shadow `Velnor / Verified` status.
- **Later:** the `velnor-verify` GitHub App makes that status the required check.

### R5. Reuse climbs a ladder, cheapest proof first

1. **Same commit:** a rerun of a commit already verified is a lookup.
2. **Queue to push:** the push after a queue merge reuses the queue's result (`skip_push_after_merge_group`, safe since #1764), with a golden rule that a step is skipped on push only if the queue ran it.
3. **Same tree:** a queue group whose tree equals an already-verified tree is a lookup (Opus's `ReusePoint::SameTree`, generalising `Step.skip_after_merge_group`).
4. **Same obligation key:** per-package and per-test-binary keys, only after a read audit proves the key covers every input.

Phase-1 records need no new service: they are GitHub cache entries keyed by the verdict key, so GitHub's own cache scopes enforce the trust rule (a PR reads main's records and writes only its own; Fable). A separate `velnor-ledger` service comes only when a measured need exists.

The **queue uses ALLGREEN**. HEADGREEN is unsound with diff selection, because each group entry is tested only on its own diff (Fable; verified in the comparison).

### R6. Velnor Native executes what velnor-gen writes

The design's executor half stays, with a narrower first target:

- For velnor-new, Velnor Native runs the **dialect velnor-render emits**: 16 pinned actions, one-command `run:` steps and a fixed set of expressions. Opus and Fable both reached this. It removes most of the Actions-protocol items (X02–X12) and the native JavaScript-action items from velnor-new's critical path.
- Full GitHub Actions protocol compatibility stays the product goal for other repositories, as a later stream gated by the owner.
- GitHub Official stays the switchable executor and the permanent reference. The executor's identity is part of every result, so nothing is reused across executors.
- None of this starts before `docs/goals.md` is amended (Q1 in the owner sheet), because goals.md line 16 says the official runner executes every job.

### R7. No runner, platform or storage crates in the first ten

Three models put them there. That contradicts the panel's own "correctness first" verdict and adds review load while main is red. The runner crates (`velnor-runner-*`) get correctness fixes only, and new features wait for Q1 and Q9.

### R8. Speed levers, in order

Measured on main today: a full push `rust` job takes 564 s, of which the 18 alone-checks are 48%, the lane guard 17% and docs 16%. A PR quick set takes about 90 s. No labelled PR with tests has passed since the cutover, so test time is unknown.

1. **A green main.** Only a green push saves the target cache, so a red main makes every run cold.
2. **Stop verifying the same tree three times** (R5).
3. **Move the post-merge tail into the queue in parallel**, and restrict alone-checks to crates that are released.
4. **Map the paths the plan cannot place.** 40% of the last 396 commits touched such a path and selected everything; `[[inputs]]` entries for `generator/samples/**`, `.github/workflows/**` and `local/**`, proven by replaying 400 commits.
5. **Shape from measured timings**, and the small wins: the lane guard reads the test step's listing instead of listing three times; feature-lane lint and docs run only when the lane's package is selected; `policy` no longer waits for `rust`; the `velnor-ci` tests stop running one at a time.

### R9. Targets are budgets until measured

- **Goals, unchanged:** PR verdict P50 ≤ 90 s, P90 ≤ 4 min; push-to-merge P50 ≤ 3 min, P90 ≤ 8 min.
- **Trial budgets after phase 0:** PR verdict about 4 / 9 min, push-to-merge about 9 / 20 min, to be replaced by the W8 baseline.
- The post-cutover "0.6 min push-to-merge" measures bypass latency, not CI, and is not a baseline.
- No dated target is published before a green, test-running baseline exists. The dated targets three models gave (as low as 45 s) are not adopted.
- The "fastest runner" claim still waits for the S1–S6 benchmark suite. It now runs as a **generator-owned benchmark**: a workflow that replays recent commits on the pinned helper and on a candidate, and a pin bump lands only if the candidate is not slower.

### R10. Release is built once and promoted

- **Now:** the generated tools release is gated on the exact green push run (R2.5).
- **Next:** the push run on main builds the release binaries. A tag promotes those exact bytes after the gate, instead of rebuilding them on hosted runners.
- **Squash changes the commit the tools embed** (`VELNOR_COMMIT`), so byte-equal promotion needs a signed release manifest that binds the binary digests to the tagged commit (Astra, Sol).
- **Release hosting:** releases stay on GitHub-hosted runners with attestation until the owner decides otherwise (Q5).
- **`runner-v`:** the release group is declared but has no workflow. Generating one is a phase-1 item.

### R11. Facts the plan must not repeat

These claims appeared in the brief or in panel answers and are wrong:

| Claim | Correction |
|---|---|
| 31 alone-check steps | There are 18 |
| #1320 is open | It closed unmerged |
| RUSTSEC-2026-0543 fails cargo-deny | It is already ignored |
| The 5-minute queue wait adds 5 minutes per merge | It does not apply when the minimum group size is 1 |
| `.velnor/rust-ci.toml` has `[[inputs]]` | It has none; the path rules live in `.velnor/config.toml` |

Report A's timings came from the multi-job layout that #1594 replaced. Gemini 4 Carbon cited files that do not exist, and Gemini 4 Argon and Muse Wonder cited some stale paths; [the comparison](comparison.md#4-hallucination-check-cited-items-that-do-not-exist-at-0eb2d76a5c) lists them, and none of those claims is used.

### R12. How the plan meets a fast-moving main

- The plan is delivered as GitHub issues with state-based "done when" checks, linked to the PRs that already address them (#1770 now; #1608 to close as superseded; #1297 to close and let Renovate recreate).
- Before issues are created, the `velnor-roadmap` and `velnor-runners-fc` sessions are told what is coming, so that they do not open duplicates.
- The owner confirms the plan first. Nothing is created until then.

### R13. Ideas kept for after phase 0

- One plan drives three targets: the generated YAML, Velnor Native, and local `velnor verify` for agents.
- Selection becomes "what is not yet proven" instead of "what the diff touched". It needs no base commit, so `fetch-depth` stops mattering.
- Test binaries become the unit of reuse (nextest archive digests).
- The branch ruleset is generated with a drift check, like the tag rulesets already are.
- **Speculative queue:** the label run tests the PR on top of the current queue tip, so the queue group becomes a lookup. This is the cheapest path to the 3 / 8 min push-to-merge goal. It needs measurement first.
- Plan edges (`Step.after`) let Velnor Native run independent steps of one job at the same time.

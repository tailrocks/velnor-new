# Owner questions: decision sheet

This sheet lists every question in the Velnor plan that only the owner can answer, in the order to answer them. Questions that block phase 0 come first. Sources: the [rulings](velnor-gen-rebase/rulings.md), the [panel comparison](velnor-gen-rebase/comparison.md), the [delivery plan](delivery-plan.md) and the [design](../../proposed/velnor-runner-architecture.md).

**How to read it.**

- **Needed by** names the delivery-plan item that waits for the answer (W1–W10 are phase 0).
- The plan runs on each **default** until you answer. Where only you can act (apply a ruleset, amend `docs/goals.md`), there is no default: the work waits.
- Numbers keep their labels: (measured), (estimate) and (target).
- Q ids are new. D ids are older questions that still stand, reworded where the ground moved.

## Answer first: phase 0

### 1. Q2: The label flow

- **Question.** Adopt exactly this flow? One label, `velnor:verify`. A draft runs nothing, and a ready PR without the label runs nothing. Adding the label runs the latest commit. Each push while it is attached runs and cancels the previous run. Removing it cancels the run in flight. `Required` is never skipped on a PR event and is red ("CI not requested") on any commit without a verdict. A label added while the PR was a draft arms it when it becomes ready.
- **Today (measured).**
  - A draft shows a green `Required`.
  - A ready PR without a label runs format, lint and policy, and `Required` turns green without tests.
  - `ci:run` runs tests once and removes itself; `ci:auto` runs on every push.
  - Removing a label is not a trigger.
  - An unrelated label or a title edit starts a run whose `Required` job is skipped, which GitHub counts as a pass. #1770 is fixing that part.
- **At stake.** Your rule that nothing merges without a green verdict on its latest commit. Today a PR can merge with no tests at all.
- **Options.**
  - (a) As above.
  - (b) As above, but keep the format and lint quick set on unlabelled PRs, with `Required` still red.
  - (c) Keep `ci:run` and `ci:auto`, and only make `Required` red without a verdict.
- **Recommendation.** (a). The cost is one short red job per push on unlabelled PRs until the Velnor App exists. Eight of nine models chose (a).
- **Default.** (a). It is your flow from decision 12; this question confirms the label name and the deletions.
- **Needed by.** W2. Also answers the old D16 (draft label arms at "ready": yes) and D19 (compile before the label: no).

### 2. Q13: Which checks must pass before a change lands

- **Question.** Docs (`cargo doc`) and the 18 "check crate alone" steps run today only after merge, on the push to main. Should they run in the merge queue, so that a break blocks landing instead of turning main red afterwards?
- **Today (measured).** On a full push, those steps are 64% of the 564 s `rust` job. They never run on a PR or in the queue.
- **Options.**
  - (a) Run them in the queue, in parallel with the tests; the push only audits.
  - (b) Keep them after merge.
  - (c) (a), and run alone-checks only for crates that are released.
- **Recommendation.** (c). A break is caught before it lands, and the queue is not slower because the steps run in parallel. The push after a queue merge can then reuse the queue's result (ruling R5).
- **Default.** (b), today's behaviour. The push-reuse step (W7) waits.
- **Needed by.** W7.

### 3. Q3: Stop bypass merges, bind the gate, allow auto-merge

- **Question.** Once main has been green with tests running for three consecutive pushes (W1, W2), change ruleset `protect-main`? The change:
  - Bind `Required` to GitHub Actions (integration 15368).
  - Empty the bypass list.
  - Set `allow_auto_merge` to true.
  - Keep the four existing rules: deletion, non-fast-forward, linear history and pull request.
  - Agents use auto-merge and never do catch-up merges.
- **Today (measured).**
  - The ruleset requires `Required` with no integration binding.
  - Admins can bypass, and most of the about 170 merges since the cutover went in with a failing `Required`.
  - The merge queue is configured but has never run, and auto-merge is off.
- **Options.**
  - (a) As above, after the green gate.
  - (b) As above, plus a merge freeze now: bypass only for PRs that repair main.
  - (c) Keep the bypass.
- **Recommendation.** (b). Every bypass merge onto a red main can hide a second failure, and it keeps the target cache cold. The freeze costs little because the repairs are small and mostly landed.
- **Default.** None. You apply the ruleset; W6 waits.
- **Needed by.** W6. Replaces D2 and D3.
- **Warning.** A ruleset update by `PUT` replaces the whole rule list. The prepared JSON keeps all six rules; one panel answer's JSON would have deleted four protections.

### 4. Q4: The merge queue uses ALLGREEN

- **Question.** Switch the queue from HEADGREEN to ALLGREEN?
- **Why.** CI selects tests by diff. In a group, each entry is tested only on its own diff, so under HEADGREEN entry A can merge on the strength of B's run, which never tested A's packages. ALLGREEN requires every entry's own run to pass.
- **Options.** ALLGREEN; keep HEADGREEN.
- **Recommendation.** ALLGREEN. The 5-minute minimum wait does not apply when the minimum group size is 1. Set it to 0 for clarity, and expect no time saving from that change.
- **Default.** None: part of the ruleset change in Q3.
- **Needed by.** W6.

### 5. Q12: The gate's rules come from the base branch

- **Question.** Today the workflow and its policy come from the PR's own tree, so a PR can edit `rust-ci.yml` or `.velnor/rust-ci.toml` and loosen what `Required` demands. Make the gate compare the PR's verification contract with main's, and fail on any reduction you have not signed for that exact diff?
- **At stake.** Once the bypass is gone, this is the realistic way a false green gets in: an agent "fixes" a red test by weakening the check. Agents do all the work (decision 15), and an agent can be wrong.
- **Options.**
  - (a) The contract check in phase 0 (W10), then a trusted verifier, then the Velnor App.
  - (b) Wait for the App.
- **Recommendation.** (a). velnor-gen already detects "check does less" (`weaken.rs`), so the first version is cheap.
- **Default.** (a).
- **Needed by.** W10. Also answers D17: only you approve a reduction, by signing the exact diff, with an expiry.

### 6. D7: An agent identity

- **Question.** Create a GitHub identity for agents that can label, push and enable auto-merge but cannot change rulesets or settings?
- **Why.** Agents act under your identity today, which can edit the ruleset and bypass it. A label added with `GITHUB_TOKEN` does not trigger workflows, so the label flow needs a real identity anyway.
- **Recommendation.** Create it now. The `velnor-verify` App, which writes `Velnor / Verified`, comes later (ruling R4) and is asked again then.
- **Default.** None: agents keep using your identity.
- **Needed by.** W2's lab cases and W6.

## Before runner work starts

### 7. Q1: Amend `docs/goals.md` for the Rust executor

- **Question.** `docs/goals.md` line 16 says GitHub's official runner executes every job. That contradicts your decisions 1 and 20 (Velnor Native is the default executor). Approve this replacement text for line 16?

  > New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. On GitHub's machines, GitHub's official runner executes jobs. On the project's own machines, `velnor-runner` executes them with one of two executors behind one interface: **Velnor Native**, a Rust executor for the workflows `velnor-gen` writes, which is the default; or **GitHub Official**, GitHub's unmodified runner, which the project may choose per job type in its configuration. The executor's identity is part of every result, so no result is reused across executors, and Velnor never switches executors on its own when a job fails. GitHub Official is also the permanent reference: a reference run never produces a verdict, a cache entry or a release, and any difference between the two is a defect in Velnor Native.

- **The same approval covers:**
  - Rule 11 (line 75): "with GitHub's official runner, started by the Velnor runner" becomes "with the executor the project uses, started by the Velnor runner".
  - Lines 11–12: the code now lives in `crates/`, not `generator/` and `ci-helper/`.
  - A new sentence under "Correct before fast": "A result is reused only when everything that could change it is part of its key and is equal. A result that cannot be keyed is produced again."
- **Options.**
  - (a) Approve as written.
  - (b) Approve, and also rewrite lines 20–24, 43, 45 and 59 (the macOS-first runner, forks, the cache) now.
  - (c) Reject.
- **Recommendation.** (a). Lines 43, 45 and 59 are separate decisions (Q6, Q8).
- **Default.** None. All executor, platform and storage work waits; phase 0 does not.
- **Needed by.** Every X, P, ST and C item.

### 8. Q14: Velnor Native runs what velnor-gen writes

- **Question.** For velnor-new, may Velnor Native run only the dialect velnor-render emits, with full GitHub Actions compatibility built later as a product stream for other repositories? The dialect is 16 pinned actions, one-command `run:` steps and a fixed set of expressions.
- **At stake.** It takes most of the Actions-protocol and JavaScript-action work off velnor-new's critical path, and so brings Native's first real job much sooner (estimate). Full compatibility stays a goal.
- **Options.**
  - (a) The dialect first; full compatibility later, gated by you.
  - (b) Full protocol compatibility first, as the old plan had it.
- **Recommendation.** (a).
- **Default.** (a).
- **Needed by.** X stream. Parks the old D4 (which expression evaluator) until the full-compatibility stream starts.

### 9. Q8: Reusing results despite hard decision 3

- **Question.** Hard decision 3 forbids a second skip mechanism. The plan reuses a verified result instead of running it again. Approve reuse under a soundness contract?
- **The contract.**
  - A result is reused only when its key covers every input and matches exactly.
  - Every reuse records a receipt that the gate checks.
  - The push after merge audits every reused tree.
  - Per-package keys come only after a read audit proves them.
- **The reuse ladder** (ruling R5):
  1. Same commit.
  2. Queue to push.
  3. Same tree.
  4. Same obligation key.
- **Recommendation.** Approve, starting with the first two steps (queue to push is already built and safe since #1764).
- **Default.** Only same-commit reruns are reused.
- **Needed by.** W7, V stream.

### 10. Q9: The runner crates

- **Question.** The `velnor-runner-*` crates are a macOS-first scale-set host that serves no production queue today. Keep fixing them, and make them the GitHub Official executor's host, or freeze them until Q1?
- **Options.**
  - (a) Correctness fixes only until Q1, then rebuild them as the Official host.
  - (b) Keep developing them now.
  - (c) Delete them.
- **Recommendation.** (a).
- **Default.** (a).
- **Needed by.** P and F streams. Replaces D13 and D14.

### 11. D8: Deploy identity and node changes

- **Question.** When runner work starts, may agents deploy Velnor to the cluster and change nodes (XFS volumes, mirrors), with manifests kept in the operator repository?
- **Recommendation.** Yes, after Q1. Nothing in phase 0 needs it.
- **Default.** None: cluster work waits.

## Releases

### 12. Q5: Where release binaries are built

- **Question.** Keep building tools releases on GitHub-hosted runners with GitHub's attestation, or build them on Velnor and promote them with a pinned signer?
- **Recommendation.** Keep hosted for now (eight of nine models). Move to "built once on main's push and promoted at the tag" once the release manifest (Q15) exists.
- **Default.** Hosted.

### 13. Q15: The release subject across a squash merge

- **Question.** The tools embed the commit they were built from, and a squash merge changes it, so bytes built on the PR or in the queue are not the bytes of the tagged commit. Bind release binaries to the tagged commit with a signed manifest instead of rebuilding?
- **Recommendation.** Yes. It is the precondition for "build once, promote".
- **Default.** Rebuild at the tag (today).

### 14. D9: Agents may release tool patches

- **Question.** May agents cut `tools-v` patch releases without asking each time, when the generated release workflow's gate (exact green push run) passes?
- **Recommendation.** Yes. 0.1.95 and 0.1.96 were already released this way, with only an ancestry check as the gate.
- **Default.** One approval each.

### 15. Q17: A `runner-v` release workflow

- **Question.** `release-plz.toml` declares the `runner` group, but no workflow releases it. Generate one now, or wait for Q9?
- **Recommendation.** Wait for Q9.
- **Default.** None built.

### 16. D15: Recovery from a bad Velnor release

- **Question.** If a released helper breaks CI, every PR fails, including the PR that would fix it. Approve the recovery path?
- **Options.**
  - (a) A re-pin PR to the previous helper verifies itself, because the pin is the input. No bypass is needed while the gate runs on Actions.
  - (b) (a), plus a break-glass verifier when the gate moves to the App.
- **Recommendation.** (a) now, (b) when the App lands.
- **Default.** (a).

### 17. Q16: An emergency stop

- **Question.** Who may pause the merge queue and tag creation in an emergency, and how is it recorded?
- **Recommendation.** You, plus a scoped, audited hold that agents may request but not apply.
- **Default.** You only, by hand.

## Product scope and process

### 18. Q6: Forks

- **Question.** You decided velnor-new takes no fork PRs. `docs/goals.md` line 43 still promises fork handling for the generator's users. Keep the fork fence in velnor-gen as a product feature, while velnor-new refuses forks in its settings?
- **Recommendation.** Yes (seven of nine models). Changing line 43 is a separate decision.
- **Default.** Yes.

### 19. Q10: Where dated evidence lives

- **Question.** `AGENTS.md` asks for evergreen docs. Keep contracts in `docs/`, and put dated measurements and plans in issues and PR descriptions? This research folder would then be summarised into evergreen docs and the dated parts moved to issues once the backlog exists.
- **Recommendation.** Yes.
- **Default.** Yes.

### 20. Q11: Which models implement and review

- **Question.** The backlog names an implementation and a review model per issue. Use only the seats the repository's `exec:*` labels define?
- **Recommendation.** Yes. Use Fable only where the item is genuinely hard, such as soundness and trust boundaries.
- **Default.** Yes.

### 21. Q7: Job shape

- **Question.** CI is one job per workspace today. Split it only when committed timings show a gain?
- **Recommendation.** Yes. velnor-gen already decides the shape from `.velnor/rust-ci-timings.json`; that file does not exist yet, and W8 creates it.
- **Default.** Yes.

## Later: not needed now

| Id | Question | Recommendation | Default |
|---|---|---|---|
| D11 | Cutover counts per job type before Native takes a job type | As written in the design, counted per obligation kind | As written |
| D18 | A Velnor merge train instead of GitHub's queue | Not now. Measure GitHub's share first, and try the speculative queue (the label run tests the PR on the current queue tip) | GitHub's queue |
| D20 | Rent high-clock nodes | One, for the benchmark only | No rental |
| D21 | One compile for both witness runs | No | No |
| D22 | Rent comparator runners for the benchmark | One vendor arm plus GitHub larger runners | No rented arm; a narrower claim |
| D23 | Verify an agent's unpushed commits automatically | Not yet | Off |
| D24 | Agent work above PR work | Yes, within a budget | Equal to PR work |
| D25 | Agents working inside cluster sandboxes | A two-agent pilot after `velnor verify` ships | No |
| D26 | A native arm64 lane | Not now | Nothing built |

## Closed by events

- **D1** (release engine 0.1.81) and **D14** (delete the engine crates): the engine was deleted in #1747.
- **D9** as worded (engine releases): replaced by question 14.
- **D2, D3**: replaced by Q3 and Q4.
- **D16, D19**: answered inside Q2.
- **D17**: answered inside Q12.
- **D4**: parked by Q14.
- **D13**: replaced by Q9.

## Already decided

Your binding decisions 1–24 are in [owner-decisions.md](owner-decisions.md). Three of them need `docs/goals.md` to change before the work they describe can start:

| Decisions | What they say | Goals change |
|---|---|---|
| 1, 20 | The Rust executor | Q1 |
| 8 | No fork PRs | Q6 |
| 22–24 | A native cache, storage tiers and mirrors | Line 59, a later decision |

All other decisions stand as written.

## Summary

| # | Id | Question | Needed by | Recommendation | Default |
|---|---|---|---|---|---|
| 1 | Q2 | Label flow | W2 | `velnor:verify`, nothing without it, red without a verdict | Same |
| 2 | Q13 | What must pass before landing | W7 | Docs and released crates' alone-checks in the queue | After merge |
| 3 | Q3 | Bypass, binding, auto-merge | W6 | Freeze bypass now; apply after a green main | Waits |
| 4 | Q4 | ALLGREEN queue | W6 | Yes | Waits |
| 5 | Q12 | Gate rules from the base branch | W10 | Yes, contract check now | Yes |
| 6 | D7 | Agent identity | W2, W6 | Create now; App later | Your identity |
| 7 | Q1 | goals.md executor amendment | X, P, ST, C | Approve as written | Runner work waits |
| 8 | Q14 | Native runs the velnor-gen dialect | X | Yes | Yes |
| 9 | Q8 | Reuse under a soundness contract | W7, V | Approve | Same-commit only |
| 10 | Q9 | Runner crates | P, F | Fixes only, then Official host | Same |
| 11 | D8 | Deploy identity and node changes | after Q1 | Yes | Waits |
| 12 | Q5 | Release hosting | R | Hosted for now | Hosted |
| 13 | Q15 | Release subject across squash | R | Signed manifest | Rebuild |
| 14 | D9 | Agents cut tool patches | R | Yes, behind the gate | One approval each |
| 15 | Q17 | `runner-v` release | R | After Q9 | None |
| 16 | D15 | Recovery from a bad helper | R | Self-verifying re-pin | Same |
| 17 | Q16 | Emergency stop | R | You plus an audited hold | You only |
| 18 | Q6 | Forks in the product | — | Keep the fence in velnor-gen | Same |
| 19 | Q10 | Where dated evidence lives | — | Issues; evergreen docs | Same |
| 20 | Q11 | Models per issue | backlog | `exec:*` seats only | Same |
| 21 | Q7 | Job shape | W8 | Split only on measured gain | Same |

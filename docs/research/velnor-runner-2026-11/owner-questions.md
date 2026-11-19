# Owner questions: decision sheet

This sheet lists every question in the Velnor plan that only the owner can answer, in the order to answer them. Questions that block phase 0 come first. The [rulings](velnor-gen-rebase/rulings.md) are the source; the [delivery plan](delivery-plan.md), the [design](../../proposed/velnor-runner-architecture.md) and the [explainer](explainer.md) repeat the answers given here and nothing else.

**How to read it.**

- **Needed by** names the delivery-plan item that waits for the answer (W0–W10 and N26 are phase 0).
- The plan runs on each **default** until you answer. Where it says "no default", the work waits: the question becomes an `[OWNER]` issue that blocks its items.
- Numbers keep their labels: (measured), (estimate) and (target). A measured number names its run.
- Q ids are new. D ids are older questions that still stand, reworded where the ground moved.

## Answer first: phase 0

### 1. Q3: Freeze the bypass now, then remove it

- **Part 1, now: the freeze.** Until the ruleset changes, use the admin bypass only for a PR that repairs main and names the red run it repairs?
- **Why now.** Most of the about 170 merges since the cutover went in with a failing `Required` (measured). PR #1799 merged on the green check of a draft run that ran no tests, and main went red (run 44599906946). No green main holds while this continues.
- **Part 2, later: the ruleset.** Once W0–W5 are done and the last three pushes to main are green with tests run, change ruleset `protect-main`:
  - Bind `Required` to GitHub Actions (integration 15368).
  - Empty the bypass list.
  - Keep the other rules as they are: deletion, non-fast-forward, linear history, pull request (squash only, thread resolution, stale reviews dismissed) and the merge queue's sizes and timeout.
  - As a separate repository setting, set `allow_auto_merge` to true. Agents use auto-merge and never do catch-up merges.
- **Later again (W6, step 2).** When the trusted verifier is proven (Q12), the ruleset also requires `Velnor / Verified`.
- **Options.** (a) Both parts. (b) Part 2 only, with no freeze. (c) Keep the bypass.
- **Recommendation.** (a). The repairs are small and mostly landed.
- **Default.** None. Phase 0 starts when you declare the freeze, and you apply the ruleset.
- **Needed by.** W0 and W1 (part 1); W6 (part 2). Replaces D2 and D3.
- **Warning.** A ruleset update by `PUT` replaces the whole rule list. The delivery plan (W6) builds the payload from the live ruleset, so all six rules survive, and reads every field back.

### 2. D9: Agents may release tool patches

- **Question.** May agents cut `tools-v` patch releases without asking each time, from a commit of main whose push `Required` passed?
- **Why now.** A change to the generator or the helper reaches CI only through a release and a pin. Phase 0 needs three (the delivery plan's trains A, B and C).
- **Today (measured).** The release gate checks only that the tag is on main. 0.1.97 and 0.1.98 were published although the push run of the tagged commit had failed (runs 44596703249 and 44599294031).
- **Options.**
  - (a) Yes. Until W5 enforces it, the release PR shows the push run id, its attempt and the `Required` conclusion of the commit to tag.
  - (b) One approval each.
- **Recommendation.** (a). A release from a red commit, when main cannot be green without a helper fix, still needs your approval, naming the red run and the fix.
- **Default.** (b). Each train waits for you.
- **Needed by.** W0, W1 (train A); W2, W3, W4, W5 (train B); W7, W9, W10, N26 (train C).

### 3. Q2: The label flow

- **Question.** Adopt exactly this flow? One label, `velnor:verify`. A draft runs nothing, and a ready PR without the label runs nothing. Adding the label runs the latest commit. Each push while it is attached runs and cancels the previous run. Removing it cancels the run in flight and turns `Required` red, also on a commit that was green. `Required` is never skipped on a PR event and is red ("CI not requested") on a draft and on an unlabelled PR. A label added while the PR was a draft arms it when it becomes ready.
- **Today (measured).**
  - A draft shows a green `Required`.
  - A ready PR without a label runs format, lint and policy, and `Required` turns green without tests.
  - `ci:run` runs tests once and removes itself; `ci:auto` runs on every push.
  - Removing a label is not a trigger.
  - An unrelated label or a title edit publishes no `Required` and cancels nothing. That part is done.
- **At stake.** Your rule that nothing merges without a green verdict on its latest commit (decision 12, which also says an unlabelled PR never has a green required check).
- **Before the new flow lands.** W0 makes drafts and unlabelled PRs red at once. `ci:run` and `ci:auto` stay until W2 replaces them, because until then they are the only way to run a PR's tests. Do not delete them by hand.
- **Options.**
  - (a) As above.
  - (b) As above, but keep the format and lint quick set on unlabelled PRs, with `Required` still red.
  - (c) Keep `ci:run` and `ci:auto`, and only make `Required` red without a verdict.
- **Recommendation.** (a). The cost is one short red job per push on unlabelled PRs until the Velnor App exists. Eight of nine models chose (a).
- **Default.** (a). It is your flow from decision 12; this question confirms the label name and the deletions.
- **Needed by.** W2. Also answers the old D16 (draft label arms at "ready": yes) and D19 (compile before the label: no).

### 4. D7: An agent identity and the status App

- **Question.** Create two GitHub identities? (1) One for agents, which can label, push and enable auto-merge but cannot change rulesets or settings. (2) The `velnor-verify` App with `statuses: write` only, whose key is a secret of an environment that only `main` can use.
- **Why.** Agents act under your identity today, which can edit the ruleset and bypass it; emptying the bypass list means little while they do. The trusted verifier (Q12) needs an identity that a PR's own workflow cannot use.
- **Recommendation.** Create both now. The App gains webhooks later, when it takes over the label flow (ruling R4).
- **Default.** None. Agents keep using your identity, and W6 and N26 wait.
- **Needed by.** W6 (the agent identity); N26 (the App).

### 5. Q4: The merge queue uses ALLGREEN

- **Question.** Switch the queue's grouping strategy from HEADGREEN to ALLGREEN?
- **Why.** CI selects tests by diff. In a group, each entry is tested only on its own diff, so under HEADGREEN entry A can merge on the strength of B's run, which never tested A's packages. ALLGREEN requires every entry's own run to pass. The lab records this with two overlapping entries before the change (W6).
- **Options.** ALLGREEN; keep HEADGREEN.
- **Recommendation.** ALLGREEN. The 5-minute minimum wait does not apply when the minimum group size is 1. Set it to 0 for clarity, and expect no time saving from that change.
- **Default.** None: part of the ruleset change in Q3.
- **Needed by.** W6.

### 6. Q12: A gate the PR cannot weaken

- **The problem.** On PR and queue events GitHub runs the PR's own copy of the workflow. A PR can therefore delete any check the workflow runs on itself. Binding `Required` to GitHub Actions also accepts a job named `Required` from any workflow file the PR adds. Agents do all the work (decision 15), and an agent can be wrong: it can "fix" a red test by weakening the check.
- **Question.** Approve this path?
  1. **An accident guard (W10).** The gate compares the PR's verification contract with main's and fails on a reduction you have not signed. It catches reductions made through `.velnor/rust-ci.toml`. It cannot stop a PR that removes the comparison.
  2. **A trusted verifier (N26).** A workflow that GitHub always takes from main checks the PR's tree as data and writes `Velnor / Verified` through the App of D7. The ruleset then requires that status. Only this step makes the gate proof against a PR that edits its own workflow.
- **Your signature (answers D17).** You approve a reduction with an SSH signature over the two contract digests, the list of reductions and an expiry of at most 7 days. Your public key is listed in main's `.velnor/allowed_signers`. A rebase voids the signature.
- **Options.**
  - (a) Both steps, in phase 0.
  - (b) (a), and until step 2 lands a code-owner review by you on every PR that touches `.github/workflows/`, `.velnor/rust-ci.toml`, `.velnor/rust-ci.lock` or `actions/setup-velnor/`.
  - (c) GitHub's own "required workflow" rule instead of step 2. It needs an organisation ruleset, which the organisation's Free plan does not have (measured: the API answers "Upgrade to GitHub Team"), so it costs money.
  - (d) Wait for the full App.
- **Recommendation.** (a).
- **Default.** (a).
- **Needed by.** W10, N26, and W6 step 2.

### 7. Q13: Which checks must pass before a change lands

- **Question.** Docs (`cargo doc`) and the 18 "check crate alone" steps run today only after merge, on the push to main. Should they run in the merge queue, so that a break blocks landing instead of turning main red afterwards?
- **Today (measured, one run).** On run 44584072510 those steps took 360 s of a 564 s `rust` job. That run failed and ran no tests, so 360 s is the only solid number. They never run on a PR or in the queue.
- **The cost.** The steps of one job run in series. In today's one job, (a) and (c) make every queue group about 6 minutes later (estimate from that run). They add no wait only in a second, parallel job, and a second job needs a measured gain (Q7). W8 measures both shapes first.
- **Options.**
  - (a) Run them in the queue; the push only audits.
  - (b) Keep them after merge.
  - (c) (a), and run alone-checks only for crates that are released.
- **Recommendation.** (c), decided after W8's numbers.
- **Default.** (b), today's behaviour.
- **Needed by.** W7 (where the steps run). W7's push reuse also waits for Q8.

### 8. Q8: Reusing a verified result instead of running it again

- **Question.** `docs/goals.md` says a check is skipped only when the change provably cannot affect it, and that the required check passes only when everything selected ran and passed. The plan would satisfy a selected check with a receipt of an earlier run. Approve that, with this sentence added under "Correct before fast"?

  > A result is reused only when everything that could change it is part of its key and is equal. A result that cannot be keyed is produced again.

- **The contract.**
  - Evidence is per check: a record shows that this check ran and passed, and its full key (tree, package selection, workflow, helper, toolchain, runner image) equals the request's.
  - A check that reads the clock, the network or randomness, or that is not yet classified, always runs.
  - A kind of reuse turns on only after shadow comparisons that were real would-be hits, each run again, with no mismatch. 300 clean comparisons bound the error rate near 1%; the design's rule C6 asks for about 3,000 per kind (0.1%).
  - Every commit that lands is audited afterwards without reuse.
- **The reuse ladder** (ruling R5):
  1. Same commit.
  2. Queue to push.
  3. Same tree.
  4. Same obligation key.
- **Options.** (a) Approve the sentence and rungs 1 and 2 under the contract, with C6's count. (b) As (a), with 300 comparisons for rungs 1–3. (c) No reuse.
- **Recommendation.** (a). Rungs 3 and 4 come back to you with their evidence.
- **Default.** None. Every request runs fresh, and the reuse items wait.
- **Needed by.** W7's reuse, N25, V01, V04, DG03, N18, V10 and BM01. The hard decisions do not apply here: they bind only the old engine.

### 9. Q18: Speed targets, and the cost of an honest gate

- **Question, part 1.** The plan used to call "PR verdict 90 s at P50 and 4 min at P90" and "push-to-merge 3 min and 8 min" goals. They are not in `docs/goals.md`. They come from the earlier design round, and you have not decided them. Keep them as proposed targets, to be judged after the first baseline (W8)?
- **Question, part 2.** `docs/goals.md` says an ordinary PR's wait and runner-minutes must not get worse, except for a check that correctness needs, "with its cost measured and accepted by the owner". Phase 0 adds such checks: the red check on unlabelled PRs, the generator check, the reduction check and the verifier's extra hop. Accept their cost once W8 has measured it?
- **Recommendation.** Yes to both. W8 reports the targets `docs/goals.md` does set (a run that selects no crate within 1 min; 30 s to the first cargo command; the generator's tests under 1 min) next to the proposed ones.
- **Default.** The proposed targets stay proposals, and nothing is published as a goal. The cost comes back to you with W8's numbers.
- **Needed by.** W8.

## Before runner work starts

### 10. Q1: Amend `docs/goals.md` for the Rust executor

- **Question.** `docs/goals.md` line 16 says GitHub's official runner executes every job. That contradicts your decisions 1 and 20 (Velnor Native is the default executor). Approve this replacement text for line 16?

  > New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. On GitHub's machines, GitHub's official runner executes jobs. On the project's own machines, `velnor-runner` executes them with one of two executors behind one interface: **Velnor Native**, a Rust executor for the workflows `velnor-gen` writes, which is the default; or **GitHub Official**, GitHub's unmodified runner, which the project may choose per job type in its configuration. The executor's identity is part of every result, so no result is reused across executors, and Velnor never switches executors on its own when a job fails. GitHub Official is also the permanent reference: a reference run never produces a verdict, a cache entry or a release, and any difference between the two is a defect in Velnor Native.

- **The same approval covers:**
  - Rule 11 (line 75): "with GitHub's official runner, started by the Velnor runner" becomes "with the executor the project uses, started by the Velnor runner".
  - Lines 11–12: the code now lives in `crates/`, not `generator/` and `ci-helper/`.
- **Options.**
  - (a) Approve as written.
  - (b) Approve, and also rewrite lines 20–24, 43, 45 and 59 (the macOS-first runner, forks, the cache) now.
  - (c) Reject.
- **Recommendation.** (a). Lines 43, 45 and 59 are separate decisions (Q6, Q19), and the reuse sentence is Q8.
- **Default.** None. All executor, platform and storage work waits; phase 0 does not.
- **Needed by.** Every X, P, ST and C item.

### 11. Q14: Velnor Native runs what velnor-gen writes

- **Question.** For velnor-new, may Velnor Native run only the dialect velnor-render emits, with full GitHub Actions compatibility built later as a product stream for other repositories? The dialect is 16 pinned actions, one-command `run:` steps and a fixed set of expressions.
- **At stake.** It takes most of the Actions-protocol and JavaScript-action work off velnor-new's critical path, and so brings Native's first real job much sooner (estimate). Full compatibility stays a goal.
- **Options.**
  - (a) The dialect first; full compatibility later, gated by you.
  - (b) Full protocol compatibility first, as the old plan had it.
- **Recommendation.** (a).
- **Default.** (a).
- **Needed by.** X stream. It parks the old D4 (which expression evaluator). D4 is asked again when item X06b of the full-compatibility stream is scheduled, and blocks only that item.

### 12. Q19: A native cache and storage beyond GitHub's free limits

- **Question.** `docs/goals.md` says the design stays inside GitHub's free limits (line 45) and uses at most 8 GB of GitHub's cache (line 59). Your decisions 22–24 ask for a native `actions/cache`, storage tiers and mirrors on the project's own disks. Amend both lines for self-hosted runs?
- **Recommendation.** Yes, after Q1, with the new limits stated per site.
- **Default.** None. The items that retire GitHub's cache (C09) and build the cache index (ST03) wait.
- **Needed by.** C09, ST03.

### 13. Q9: The runner crates

- **Question.** The `velnor-runner-*` crates are a macOS-first scale-set host that serves no production queue today. Keep fixing them, and make them the GitHub Official executor's host, or freeze them until Q1?
- **Options.**
  - (a) Correctness fixes only until Q1, then rebuild them as the Official host.
  - (b) Keep developing them now.
  - (c) Delete them.
- **Recommendation.** (a).
- **Default.** (a).
- **Needed by.** P and F streams. Replaces D13 and D14.

### 14. D8: Deploy identity and node changes

- **Question.** When runner work starts, may agents deploy Velnor to the cluster and change nodes (XFS volumes, mirrors), with manifests kept in the operator repository?
- **Recommendation.** Yes, after Q1. Nothing in phase 0 needs it.
- **Default.** None: cluster work waits.

## Releases

### 15. Q5: Where release binaries are built

- **Question.** Keep building tools releases on GitHub-hosted runners with GitHub's attestation, or build them on Velnor and promote them with a pinned signer?
- **Recommendation.** Keep hosted for now (eight of nine models). Move to "built once on main's push and promoted at the tag" once the release manifest (Q15) exists.
- **Default.** Hosted. This is a stated exception to your decisions 9 (every job on Velnor) and 10 (a release never rebuilds): the tag rebuilds on hosted runners, about 4 minutes per release (measured), until item R04 lands.

### 16. Q15: The release subject across a squash merge

- **Question.** The tools embed the commit they were built from, and a squash merge changes it, so bytes built on the PR or in the queue are not the bytes of the tagged commit. Bind release binaries to the tagged commit with a signed manifest instead of rebuilding?
- **Recommendation.** Yes. It is the precondition for "build once, promote".
- **Default.** Rebuild at the tag (today).

### 17. Q17: A `runner-v` release workflow

- **Question.** `release-plz.toml` declares the `runner` group, but no workflow releases it. Generate one now, or wait for Q9?
- **Recommendation.** Wait for Q9.
- **Default.** None built.
- **Needed by.** N07, which also waits for Q9.

### 18. D15: Recovery from a bad Velnor release

- **Question.** If a released helper breaks CI, every PR fails, including the PR that would fix it. Approve the recovery path?
- **Options.**
  - (a) A re-pin PR to the previous helper. Its own run uses the helper it pins, so it can pass. An older pin is a reduction, so once the verifier (N26) is required you sign it like any other reduction. No bypass is needed.
  - (b) (a), plus a break-glass verifier when the gate moves to the App.
- **Recommendation.** (a) now, (b) when the App lands.
- **Default.** (a).

### 19. Q16: An emergency stop

- **Question.** Who may pause the merge queue and tag creation in an emergency, and how is it recorded?
- **Recommendation.** You, plus a scoped, audited hold that agents may request but not apply.
- **Default.** You only, by hand.

## Product scope and process

### 20. Q6: Forks

- **Question.** You decided velnor-new takes no fork PRs. `docs/goals.md` line 43 still promises fork handling for the generator's users. Keep the fork fence in velnor-gen as a product feature, while velnor-new refuses forks in its settings?
- **Recommendation.** Yes (seven of nine models). Changing line 43 is a separate decision.
- **Default.** Yes.
- **Needed by.** W2, which keeps the fence when it rewrites the label flow.

### 21. Q10: Where dated evidence lives

- **Question.** `AGENTS.md` asks for evergreen docs. Keep contracts in `docs/`, and put dated measurements and plans in issues and PR descriptions? This research folder would then be summarised into evergreen docs and the dated parts moved to issues once the backlog exists.
- **Recommendation.** Yes. The two documents under `docs/proposed/` already hold no dated run data; it is in this folder.
- **Default.** Yes.

### 22. Q11: Who reviews the hardest items

- **Question.** The roadmap's models are Haiku, Sonnet and Opus, set by the labels `exec:*` and `effort:*`. Ten items decide soundness or a trust boundary (W2, W10, N26, V01, V04, DG03, L03, V07, V08, C05). May their reviewer be Fable 5.2 at xhigh effort, named in the issue text? No new label is needed: a reviewer is never a label.
- **Options.** (a) Yes. (b) Roadmap models only: Opus 5.5 at high effort.
- **Recommendation.** (a).
- **Default.** (b).
- **Needed by.** The issue files (the `review` field of those ten items).

### 23. Q7: Job shape

- **Question.** CI is one job per workspace today. Split it only when committed timings show a gain?
- **Recommendation.** Yes. velnor-gen already decides the shape from `.velnor/rust-ci-timings.json`; that file does not exist yet, and W8 creates it.
- **Default.** Yes.
- **Needed by.** W8, and W7 if Q13 moves steps into the queue.

## Later: not needed now

| Id | Question | Recommendation | Default |
|---|---|---|---|
| D4 | Which general expression evaluator (full-compatibility stream only) | Asked when X06b is scheduled | Parked |
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
- **D9** as it was worded (engine releases): replaced by the new D9 above (tool releases).
- **D2, D3**: replaced by Q3 and Q4.
- **D16, D19**: answered inside Q2.
- **D17**: answered inside Q12.
- **D13**: replaced by Q9.

## Already decided

Your binding decisions 1–24 are in [owner-decisions.md](owner-decisions.md). Some of them need `docs/goals.md` to change before the work they describe can start:

| Decisions | What they say | Goals change |
|---|---|---|
| 1, 20 | The Rust executor | Q1 |
| 8 | No fork PRs | Q6 |
| 10, 11 | Reuse a verified result | Q8 |
| 22–24 | A native cache, storage tiers and mirrors | Q19 |

The plan takes these exceptions to a decision until a named item ends them:

| Decision | Exception | Ends with |
|---|---|---|
| 9 (every job on Velnor) | Tools releases build on GitHub-hosted runners | Q5 |
| 10 (a release never rebuilds) | The tag rebuilds the binaries | R04 (after Q15) |
| 10 (the same work is done once) | Each new key is verified by two witnesses, and audit A1 runs landed commits again. This buys confidence, not speed; its runner-minutes are measured in W8 and in benchmark S3 | Stays, by decision 11 |

All other decisions stand as written.

## Summary

| # | Id | Question | Needed by | Recommendation | Default |
|---|---|---|---|---|---|
| 1 | Q3 | Bypass freeze now; ruleset later | W0, W1; W6 | Both | None: phase 0 waits |
| 2 | D9 | Agents cut tool patches | Trains A, B, C | Yes, from a green commit | One approval each |
| 3 | Q2 | Label flow | W2 | `velnor:verify`, nothing without it, red without it | Same |
| 4 | D7 | Agent identity and status App | W6, N26 | Create both now | None: W6 and N26 wait |
| 5 | Q4 | ALLGREEN queue | W6 | Yes | None: part of Q3 |
| 6 | Q12 | A gate the PR cannot weaken | W10, N26, W6 step 2 | Accident guard, then trusted verifier | Same |
| 7 | Q13 | What must pass before landing | W7 | Docs and released crates' alone-checks in the queue, after W8 | After merge |
| 8 | Q8 | Reuse, with a goals sentence | W7, N25, V stream | Rungs 1 and 2 under the contract | None: no reuse, those items wait |
| 9 | Q18 | Proposed targets; cost of the gate | W8 | Keep as proposals; accept the measured cost | Proposals; cost returns with W8 |
| 10 | Q1 | goals.md executor amendment | X, P, ST, C | Approve as written | None: runner work waits |
| 11 | Q14 | Native runs the velnor-gen dialect | X | Yes | Yes |
| 12 | Q19 | Cache and storage limits in goals.md | C09, ST03 | Amend after Q1 | None: those items wait |
| 13 | Q9 | Runner crates | P, F | Fixes only, then Official host | Same |
| 14 | D8 | Deploy identity and node changes | after Q1 | Yes | None: cluster work waits |
| 15 | Q5 | Release hosting | R | Hosted for now | Hosted |
| 16 | Q15 | Release subject across squash | R04, R06, DG03 | Signed manifest | Rebuild |
| 17 | Q17 | `runner-v` release | N07 | After Q9 | None built |
| 18 | D15 | Recovery from a bad helper | — | Re-pin PR, signed once N26 is required | Same |
| 19 | Q16 | Emergency stop | L05 | You plus an audited hold | You only |
| 20 | Q6 | Forks in the product | W2 | Keep the fence in velnor-gen | Same |
| 21 | Q10 | Where dated evidence lives | N21 | Issues; evergreen docs | Same |
| 22 | Q11 | Reviewer of the ten hardest items | Issue files | Fable 5.2 xhigh, named in the text | Opus 5.5 high |
| 23 | Q7 | Job shape | W8, W7 | Split only on measured gain | Same |

# Owner questions: decision sheet for the finalize meeting

This sheet lists every question in the Velnor design that only the owner can answer, in the order
to answer them. Questions that block work come first. Sources: the
[architecture](../../proposed/velnor-runner-architecture.md) section 13, the
[delivery plan](delivery-plan.md) section 3.3 and the [decision log](decisions.md).

**How to read it.**

- **Needed by** is the plan's clock: "hour 6" means 6 hours after the start; "day 3" means hours 72
  to 96.
- The plan runs on each **default** until you answer. Where an answer is something only you can do
  (create an App, apply a ruleset), there is no default: the work it blocks waits.
- Numbers keep their labels: (measured), (measured, proxy), (estimate) and (target).
- There are 21 open questions: D1–D4, D7–D9, D11, D14–D26. No D5 appears in the current lists.
  D6, D10, D12 and D13 are already settled (see the end).

## Answer first: hours 0–12

### 1. D3: Apply the first ruleset update

- **Question.** Change the `main` ruleset now: bind both required checks to GitHub Actions
  (integration 15368), empty the bypass list, switch the merge queue to ALLGREEN, build depth 2,
  timeout 90 minutes, minimum wait 0?
- **Example.** Ruleset 24396608 names `Required verdict` and `Required` with no `integration_id`, so
  any App, or a workflow token with `statuses: write`, can post them. An admin bypass carried 7 of
  136 merges in three days, and three commits whose own `Required` failed sit on `main` (measured).
- **At stake.** Correctness today: unverified commits can land. Throughput: depth 2 with about
  11-minute groups lands about 8 merges an hour (estimate), above the 2.44 that arrive (measured).
- **Options.** (a) The JSON as prepared (delivery plan section 3.4). (b) Also depth 4 and a
  15-minute timeout now. (c) Wait.
- **Recommendation.** (a) now. Depth 4 and 15 minutes only once a group verdict takes ≤ 5 min at P90
  and paired false failures are ≤ 3%: at today's 8.1% false failures, depth 4 disturbs 29% of
  entries.
- **Default.** None: you apply it. The plan's first change (B05) waits, and so does every phase-0
  merge.
- **Blocks.** B05, and through it all of phase 0 (B02, B03, B14), the day-0 milestone.

### 2. D1: Release the engine version that moves gate jobs off hosted runners

- **Question.** Dispatch the prepared velnor-actions 0.1.81 release (#1345)?
- **Example.** Eight gate jobs still run on GitHub's `ubuntu-26.04` hosted runners. On the worst day
  they waited 57.0 minutes of a 71.8-minute run for the Free plan's 20 hosted slots (measured).
- **At stake.** Most of phase 0's gain: an agent PR's cycle falls from 94 to about 34 minutes at P50
  (estimate). The hosted queue costs the Plan job 5.5 minutes at the median (measured).
- **Options.** Release now; later.
- **Recommendation.** Now, with D9 in one approval.
- **Default.** None: the gate jobs stay on hosted runners.
- **Blocks.** B02, then B03 (gate jobs to the cluster) and B08 (cancel stall).

### 3. D9: Pre-approve CI-only engine releases

- **Question.** May agents release engine versions that change only CI configuration without a
  separate approval each time?
- **Example.** The day-0 batch (B06–B13: no `edited` trigger, no `-j2` caps, macOS legs paused,
  narrower per-push workflows) ships as engine release 0.1.82.
- **At stake.** An engine release takes about 104 minutes with macOS legs and about 27 without,
  plus the wait for approval (estimate). Each approval round can add hours overnight.
- **Options.** Pre-approve CI-only releases; approve each one.
- **Recommendation.** Pre-approve (the design folds this into D1: "one approval for the batch").
- **Default.** One approval for each release.
- **Blocks.** B14, and so the day-0 deletions.

### 4. D2: Auto-merge, and no catch-up merges

- **Question.** Should agents turn on auto-merge when they open a PR, and stop merging `main` into
  their branches?
- **Example.** 0 of 40 recent PRs used auto-merge; the median PR waited 7.5 minutes between green
  and entering the queue (measured).
- **At stake.** The catch-up rule adds about 0.47 extra PR runs per PR, about 12.5 minutes each PR
  (estimate: 0.47 × 26.7). The queue already tests against the latest `main`. With auto-enqueue, 60–75% of
  merge groups match the tree the PR passed (estimate), against 16% today (measured).
- **Options.** Yes; no.
- **Recommendation.** Yes.
- **Default.** None: agents keep today's procedure.
- **Blocks.** B04; part of the 94 → 34 minute cycle.

### 5. D8: Deploy identity and node changes

- **Question.** Create a deploy identity for the `velnor-system` namespace, and approve the node
  changes: XFS work volumes, reservations (3 CPU, 6 GiB), a 2 GiB eviction threshold, taints for
  Docker-test nodes, and a rebuild of each FI worker's data volume so RustFS gets its own logical
  volume?
- **Example.** Job directories sit on the ext4 OS disk, where copying a warm build costs 8.6 s and
  10.75 GiB. On XFS a reflink clone costs 0.25 s and 2.5 MB (both measured).
- **At stake.** Every warm-build gain needs XFS. Velnor's Kubernetes backend cannot deploy without
  the identity.
- **Options.** Approve all; approve the identity and XFS but not the RustFS volume rebuild (storage
  stays `local`); wait.
- **Recommendation.** Approve all.
- **Default.** None. Conformance can still start on the Docker backend, but nothing reaches the
  cluster.
- **Blocks.** P04 (Kubernetes backend), X15 (deploy), C01–C04 and C11 (stores, mirrors, XFS), ST04
  (RustFS), S01 and S02 (cluster policy, control plane); cohort 1 at day 2.1.

### 6. D7: The `velnor-verify` App and an agent identity

- **Question.** Create the GitHub App that writes `Velnor / Verified` (permissions `statuses:
  write`, `checks: write`, `pull_requests: read`, `contents: read`, `metadata: read`), and an agent
  identity that cannot change the ruleset?
- **Example.** Today agents label and merge under the owner's identity, which can edit the ruleset.
- **At stake.** The App gate is the design's core: one status, only `success` passes, and no Actions
  job can satisfy it. It is planned for day 4.2 (P90 6.5).
- **Options.** Create both; create the App only.
- **Recommendation.** Both. Its status becomes the gate only after lab A and shadow parity.
- **Default.** None. The plan asks by hour 12 (the design's table says day 2).
- **Blocks.** L01 (lab A), L02 (`velnor-control`), and so the App gate, the label flow and
  `velnor ship`, which labels with the agent identity.

## Days 1–3

### 7. D15: Recovery from a bad Velnor release

- **Question.** Approve the recovery path for a Velnor release that breaks CI, with no admin merge?
- **Example.** A Velnor release that fails every job also fails the PR that would fix it.
- **At stake.** Without a path, one bad release deadlocks `main`. Nothing moves back after a job type
  moves (decisions 16, 18), so this must exist before cohort 1 moves.
- **Options.** (a) Redeploy the previous image digest only. (b) (a), plus a pre-approved break-glass
  verifier: a minimal released binary runs the full check list cold, twice, on two nodes, with the
  reference-qualified executor, and writes `success` only if every check passes in both, with an
  audit record and a page. (c) Allow an admin merge (the design rejects it).
- **Recommendation.** (b). It is a real green on the exact tree, and the bypass list stays empty.
- **Default.** (a), redeploy only.
- **Blocks.** Safe cutover of cohort 1 (G01 at day 2.1).

### 8. D11: Cutover counts per job type

- **Question.** Accept the evidence each job type needs before it moves to Velnor?
- **Example.** Cohort 1 (the gate jobs) needs 150 replayed real-commit pairs per type with no
  unexplained difference. Cohort 2 (26 crate jobs) needs 300; cohort 3 (Docker tests) 150 each;
  cohort 4 (releases) 30 each plus three dry runs per workflow.
- **At stake.** N clean pairs bound the divergence rate below 3/N at 95% (150 pairs: under 2%).
  Gathering them takes about 10–26 hours per cohort (estimate). There is no way back after a move.
- **Options.** As written; fewer pairs (faster, weaker bound); more.
- **Recommendation.** As written.
- **Default.** As written.
- **Blocks.** G01 (each cohort move).

### 9. D16: A label added while the PR was a draft

- **Question.** If an agent adds `velnor:verify` while the PR is a draft, should verification start
  when the PR becomes ready for review?
- **Example.** An agent opens a draft, labels it early, then marks it ready. Under the default,
  nothing runs until it re-applies the label or pushes.
- **At stake.** One wasted round trip per such PR. No correctness effect: a verdict belongs to its
  commit.
- **Options.** Yes (the label states intent; the reconciler reads it from state); no (the literal
  reading of decision 12, step 2).
- **Recommendation.** Yes.
- **Default.** No.
- **Blocks.** The arming rule in L02 and L04, and its lab A cases (L01).

### 10. D4: Which expression evaluator ships

- **Question.** Ship (A) velnor-legacy's port of GitHub's legacy `Expressions2` evaluator, or (B) a
  port of GitHub's new `Sdk/Expressions` and `WorkflowParser`?
- **Example.** The latest official runner (v2.338.2) computes the new evaluator in compare mode but
  returns the legacy result. Velnor refuses every job where the two differ, so on every accepted job
  A and B give the same answer.
- **At stake.** Correctness is equal. Cost: A is 3 agent-hours, B about 28. A conflicts with
  decision 3 (latest APIs, nothing legacy).
- **Options.** A; B. Only one ships (decision 18).
- **Recommendation.** B: it complies with decision 3 and ruling Q4.
- **Default.** A, because it matches what the latest runner returns. B is built in parallel.
- **Blocks.** X06a or X06b's production path. If B, it must pass the full corpus before cohort 2
  (day 3.2).

### 11. D17: Who may remove a check or a test

- **Question.** Who may approve a *reduction*: an obligation or test removed from the required list,
  or a flaky test quarantined by owner exception?
- **Example.** A PR deletes a slow test file. The head's plan lists fewer tests than `main`'s, so
  the check fails (rule C3).
- **At stake.** A reduction is the one way a real check can silently disappear. Without an approver,
  even a legitimate deletion blocks, and a flaky test can only be quarantined when another blocking
  check is shown to catch the same planted fault.
- **Options.** The owner only, through a signed inventory entry; the owner or a named reviewer;
  nobody.
- **Recommendation.** The owner only, through a signed entry.
- **Default.** No reduction is accepted.
- **Blocks.** Reduction entries in G03 (the inventory) and the owner-exception form of quarantine
  (V16).

### 12. D19: Compile before the label

- **Question.** May Velnor compile and test a ready PR's pushes before the label, at idle priority,
  so the label finds the work done? Decision 12 says an unlabelled push starts nothing.
- **Example.** An agent pushes, then labels 2 minutes later; the label then starts a 15–25 s compile
  and the tests.
- **At stake.** About 15–25 s off label-to-verdict (estimate), for about 5–10 runner-minutes per
  speculated head, 45% of which are superseded.
- **Options.** No (keep decision 12; Velnor only fetches before the label); yes, at idle priority.
- **Recommendation.** No. `velnor verify` before pushing gives the same lookup when the agent asks.
- **Default.** No.
- **Blocks.** Nothing; a yes would add work.

## Days 4–7

### 13. D20: Rent high-clock nodes

- **Question.** Rent high-clock machines (Ryzen 9950X or EPYC 4564P class) now?
- **Example.** The workers are EPYC 9454P throughput parts; one rustc front end and the longest test
  run on one core.
- **At stake.** 1.3–1.5× on the compile and longest-test chain (estimate): the helper class about
  25 → 18 s, but only with two such nodes per site, because a green waits for the slower of two
  runs on distinct nodes. One node per site shortens only reds and audits. Rental cost is not
  estimated in the sources.
- **Options.** One now, for the benchmark only; two per site now; none.
- **Recommendation.** One now for BM15's ratio (it also shows whether both CPU models can share one
  fingerprint); two per site only if the ratio is ≥ 1.15.
- **Default.** No rental.
- **Blocks.** BM15 (hardware floor).

### 14. D22: Rent comparator runners for the benchmark

- **Question.** May the benchmark rent one of Namespace, Blacksmith or Depot, plus GitHub larger
  runners, and replay velnor-new's PR heads on them?
- **Example.** Without a rented arm, "fastest" can only mean faster than our own baselines.
- **At stake.** How wide the speed claim can be. Runs for about a week, measurement only; nothing
  they produce counts as a verdict.
- **Options.** One vendor arm plus GitHub larger runners; more arms; none.
- **Recommendation.** One vendor arm and GitHub larger runners, configured as each vendor
  recommends.
- **Default.** No rented arm; the claim names only the arms that ran.
- **Blocks.** BM03's rented arms; BM10 (the optional remote-execution arm) also lists D22.

### 15. D24: Agent work above PR work

- **Question.** May the agents' `interactive` class outrank PR-class work, including other
  organisations' CI? The design ranks other organisations' CI equal to PR work.
- **Example.** Ten agents each run a 10 s test while ChainArgos' PRs fill the cluster.
- **At stake.** Keeping an agent's loop near the Mac's 7.2 s nextest P50 (measured) under load. The
  class has a budget of 30 CPU-minutes an hour per identity and never touches the merge, gate or
  audit reservations.
- **Options.** Equal to PR work; above PR work within the budget.
- **Recommendation.** Above PR work, within the budget.
- **Default.** Equal to PR work.
- **Blocks.** The priority classes in S01 and the remote counting lane (AN03).

### 16. D21: One compile for both runs

- **Question.** After compile reuse is proven, may one compile feed both test runs (the second runs
  the first's binary on another node)?
- **Example.** Today each run compiles its own binary, so a stale build on one node is cross-checked
  by the other.
- **At stake.** No wall time; about 3 runner-minutes per merged PR (estimate). Against it: losing
  the cross-check the two-run rule and ruling Q12 rely on.
- **Options.** No; yes.
- **Recommendation.** No.
- **Default.** No.
- **Blocks.** Nothing; it would change how runs are scheduled after day 7.5.

## Days 8–10

### 17. D14: Delete the engine crates

- **Question.** Delete the engine crates from velnor-new once velnor-gen's workflow replaces the
  engine's CI (work item G04)?
- **Example.** The engine's orchestrator and renderer are about 275k lines.
- **At stake.** Code to maintain and review against the risk of breaking another user of them.
- **Options.** Delete after checking for other consumers; keep.
- **Recommendation.** Delete, only after checking for other consumers.
- **Default.** Out of this plan: they stay.
- **Blocks.** Nothing in this plan.

### 18. D18: A Velnor merge train

- **Question.** May Velnor land merges itself, fast-forwarding `main` by compare-and-swap after
  verifying the exact resulting tree, instead of GitHub's merge queue? Decision 7 keeps GitHub's
  queue.
- **Example.** A green PR waits 19 s for GitHub to form its group and 26 s to merge, even when the
  group's tree was verified seconds earlier.
- **At stake.** About 45 s per merge (measured medians), about half of the 85 s goal estimate, more
  in bursts. It would change decision 7.
- **Options.** Not now, measure first; adopt.
- **Recommendation.** Not now. Measure GitHub's share on 50–200 auto-merged PRs and shadow a train
  that lands nothing (BM14), then decide on the numbers.
- **Default.** GitHub's queue.
- **Blocks.** Nothing; BM14 brings the numbers.

### 19. D23: Automatic verification of unpushed commits

- **Question.** May `velnord` verify an agent's committed, unpushed work automatically, at batch
  priority, opt-in per worktree? Decision 12 starts verification only from the label or an explicit
  request.
- **Example.** An agent commits, keeps editing for two minutes, then ships; today nothing runs until
  it asks.
- **At stake.** First red at commit plus 1–2 minutes; label verdict about 35 s → 3–5 s (estimate).
  Cost: at least 70% of commits are superseded; about 21 runner-hours a day at the goal, 265 with
  whole-tree keys (estimate).
- **Options.** Off; on, opt-in per worktree.
- **Recommendation.** Not yet. Ship explicit `verify` and `ship` first; ask again with a month's
  commit rate, cost per head and cluster slack.
- **Default.** Off.
- **Blocks.** Nothing.

### 20. D25: Agents working inside cluster sandboxes

- **Question.** May agents work inside cluster sandboxes (a sticky shell beside the mirror and warm
  builds that never produces evidence), or on a dedicated agent worker?
- **Example.** An agent's build waits on a 61 s edit-build on its Mac while a warm node already has
  the build.
- **At stake.** The whole local-to-remote gap. A dedicated worker costs about $200 a month
  (estimate).
- **Options.** No; a two-agent pilot; all agents; a dedicated worker.
- **Recommendation.** A two-agent pilot after `verify` ships (about day 6.5). The shell never mints
  results.
- **Default.** No.
- **Blocks.** DG06 (the node-local request socket) and DG09's cluster side.

### 21. D26: A native arm64 lane

- **Question.** Add a native arm64 lane for the shipped aarch64 binaries (Hetzner Ampere nodes or a
  `linux/arm64` worker), additive only? Today aarch64 musl bytes get only a qemu smoke test.
- **Example.** A memory-ordering bug that shows only on arm64 hardware.
- **At stake.** Native arm64 coverage. No verdict time.
- **Options.** Not now; an advisory lane; a gating lane.
- **Recommendation.** Not now; an advisory lane when aarch64 releases matter.
- **Default.** Nothing built.
- **Blocks.** Nothing.

## Summary

| # | Id | Question | Needed by | Recommendation | Default if unanswered | Blocks |
|---|---|---|---|---|---|---|
| 1 | D3 | First ruleset update | hour 2 (first change) | Yes, as prepared | None: waits | B05, all of phase 0 |
| 2 | D1 | Release engine 0.1.81 | hour 0 | Yes, now | None: gate jobs stay hosted | B02, B03 |
| 3 | D9 | Pre-approve CI-only engine releases | day 0 | Yes | One approval each | B14 |
| 4 | D2 | Auto-merge, no catch-up merges | hour 0 | Yes | None: today's procedure | B04 |
| 5 | D8 | Deploy identity and node changes | hour 6 | Yes | None: Docker-only conformance | P04, X15, C01–C04, ST04, S01; cohort 1 |
| 6 | D7 | `velnor-verify` App, agent identity | hour 12 (design: day 2) | Yes | None: waits | L01, L02, App gate, `ship` |
| 7 | D15 | Recovery from a bad Velnor release | day 1 | Redeploy plus break-glass verifier | Redeploy only | Safe cutover (G01) |
| 8 | D11 | Cutover counts per job type | day 1 | As written | As written | G01 |
| 9 | D16 | Draft label arms at "ready" | day 2 | Yes | No | L02, L04, L01 cases |
| 10 | D4 | Expression evaluator A or B | day 3 | B | A | X06a/X06b; cohort 2 if B |
| 11 | D17 | Who approves a reduction | day 3 | Owner only, signed | None accepted | G03 entries, V16 |
| 12 | D19 | Compile before the label | day 3 | No | No | Nothing |
| 13 | D20 | Rent high-clock nodes | day 4 | One, for BM15 | No rental | BM15 |
| 14 | D22 | Rent comparator runners | day 5 | One vendor + GitHub larger | None; narrower claim | BM03 arms, BM10 |
| 15 | D24 | Agent class above PR work | day 6 | Yes, within budget | Equal to PR work | S01, AN03 |
| 16 | D21 | One compile for both runs | day 7 | No | No | Nothing |
| 17 | D14 | Delete the engine crates | day 8 | After a consumer check | Kept | Nothing in plan |
| 18 | D18 | Velnor merge train | day 8 | Not now; measure (BM14) | GitHub's queue | Nothing |
| 19 | D23 | Auto-verify unpushed commits | day 10 | Not yet | Off | Nothing |
| 20 | D25 | Agents in cluster sandboxes | day 10 | Two-agent pilot | No | DG06, DG09 |
| 21 | D26 | Native arm64 lane | day 10 | Not now | Nothing built | Nothing |

## Already decided

These close earlier questions. They are listed so you can see they are settled.

**Your binding decisions 1–24** (`owner-decisions.md`):

- 1, 20: Velnor Native is the default executor; GitHub Official is a deliberately configured
  alternative behind one interface; the executor is part of every result key.
- 2, 19: the official runner is the permanent reference (conformance, shadow runs, "run both and
  diff"), never a source of verdicts or a fallback.
- 3: latest GitHub APIs only. 4: no Node in Velnor Native; a closed action set.
- 5, 14, 17: review everything before reuse; velnor-legacy stays archived.
- 6: experimental dependencies judged on evidence (blaze was rejected on it).
- 7: GitHub stays the platform, merge queue included. 8: no fork PRs.
- 9: macOS release builds paused. 10: build once, verify once, releases promote.
- 11: correctness first; zero false greens. 12: CI only on request through the label flow; only a
  green latest commit merges.
- 13, 21: one core, Kubernetes and Docker backends, branching on capabilities.
- 15: agents do the work, sized by the real constraint. 16, 18: per-job-type migration; migrate and
  delete; no moving back.
- 22–24: native `actions/cache`, configurable storage tiers with RustFS per site, node-local
  mirrors.

**Questions closed by those decisions and the rulings:**

- D6 (the lab's reference executor): decision 19.
- D10 (two witnesses, ALLGREEN): ruling Q10 and Q12.
- D12 (where release bytes are built): ruling Q6, in the merge group, promoted by digest.
- D13 (park the old runner's open PRs): decision 16.
- The gate object (ruling Q2): one App commit status, `Velnor / Verified`; no Actions job required.
- Label events (Q3): a reconciler reads state every 10 s; webhooks only shorten the delay.
- Two PRs at one commit share a verdict (Q9); a verdict belongs to its commit, so labels per SHA
  need no question.
- Tests stay in the dev profile; release bytes are built once (Q6). Trimmed sandboxes are unsound
  (Q5).
- Landscape rulings: no speed claim names Buildkite-style agents, rented runners or remote execution
  until their arms run (C02–C04); warm pods stay the default, sandbox start ≤ 1 s P90 (C12); the
  full checkout hash stays (C13); a file-access trace never licenses a narrower key (X01).
- Rejected: optimistic merge (L17), queue bisection (L16), a peer-to-peer blob tier (L11), a Mac
  Docker run as an arm64 witness (A08).
- Agent-native points settled: explicit verbs first, the wrapper only if usage earns it; a tree is a
  tree, committed or not; `verify` fills both run slots by default; early work is charged once, to
  whoever asked; the first `velnor cargo` release only counts.

**Marked "owner decision" in the decision log's idea table, but settled by the final design** (so
not on the open list):

- N12 (bind required checks to an integration) is part of D3. N31 (merge train) is D18. N08 (agent
  workspaces on the cluster) is D25. I104 (high-clock nodes) is D20.
- I064 (dependencies at `opt-level = 3` with assertions on): adopted in the design; its gain counts
  only after lab B1.
- I105 (buy workers): the design sets the trigger (peak CPU above 70% after dedupe, or the merge
  wait P90 above 10 s for two weeks). Buying at that point is still your spend.
- N32 (one counted retry for untouched tests): not carried into the final list; the design keeps
  retries at 0 (section 1.3). Raise it if you want it back.

# Design feedback by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

# Design feedback by Claude Fable 5.2

Reviewed: the draft `velnor-runner-architecture.md`, the decision log, the comparison, the pack and the code. I did not write the draft. References such as §6.6, `I068`, `N09`, `C4` and "work item 6" are the draft's.

Tags:
- **[M]** I measured it for this review, read-only. Scripts are in `fable-5.2/scripts/`. Inputs: the pack's `ci-raw/jobs_compact.json` and `runs_ci.tsv`, my PR timelines of 200 merged PRs (`raw/pr_rows.json`, `raw/prfiles_p*.json`), and 17 pushes joined to their runs (`raw/fb/`).
- **[P]** Taken from the pack, the log or a proposal, as cited.
- **[E]** My estimate, with the arithmetic.

New measurements used below:

| Id | What | Result | Source |
|---|---|---|---|
| F1 | Push → workflow run exists | `pull_request` runs: 6 s median, 5–12 s (n=14). `push`-event runs: 1–2 s (n=29) | `raw/fb/push_to_run.json` |
| F2 | Run created → first job record | merge_group: 2 s, max 4 s (n=50). pull_request: 2 s median, but 22% wait over 10 s; p90 445 s, max 2,203 s (n=78) | `prejob.py` |
| F3 | Cause of the F2 tail | 13 of 17 delayed PR runs got their first job within 1 s of the end of the cancelled run they superseded | `prejob_cause.py` |
| F4 | Job hops | Previous job done → next job created: 0 s p50, 1 s p90. → started on an idle runner: 2 s p50, 3 s p90 (n=158). All cluster jobs, created → started: 13 s p50, 25 s p90 (n=4,060) | `hops.py` |
| F5 | Merge-group reuse replay, 142 queue merges | Table in H5 | `closure_replay.py`, `closure_window.py` |
| F6 | Head commit → push | 11 s median; at least 30 s for 38%, at least 60 s for 27% (n=1,009) | `commit_to_push.py` |
| F7 | Queue arrivals per active hour | Merges: median 3, p90 5, max 8. Enqueues: max 12 | `raw/pr_rows.json` |

## Verdict

- **No.** It would reach about 3–4 min push-to-merge at the same confidence, not the minimum. Every obligation still executes inside a chain of three GitHub jobs, which costs 30–45 s of fixed overhead plus 8 s before the first job exists [M F1, F2, F4; P my proposal §1.3 for the per-job setup and finalisation]. The three mechanisms that set the minimum (start on the push webhook, verify on the queue tip, verify before the push) are stretch items after weeks 12–20.
- **It commits to about twice what its own arithmetic gives.** Phase 3's segment table sums to 1.6 min with a 55 s test lane that the pack measures at 10 s median (§A T10). The "moved base" it prices at 6 min happens in 11–22% of merges once push to enqueue takes 2–5 min [M F5].
- **Ambition: 6/10.** Nine moonshots are named and none is committed, while the largest build (dispatcher, protocol client and hot Worker, 23.5k lines) buys at most 2 s per job start over ARC with idle runners [M F4].
- **Soundness: 7/10.** The C1–C7 contract and the checked Listener/Worker seam are the best work in the panel, but four holes lower confidence silently (H2, H3, H4, H9) and several [M] tags are borrowed from another machine or another mechanism (H10).

## Correctness and confidence holes

**H1. High. A superseding push waits for the run it cancels, and the new workflow keeps the cause.**
- Evidence: `ci.yml:57-59` puts each PR in a concurrency group with `cancel-in-progress`. `required-gate` runs on `if: ${{ always() }}` (`ci.yml:621`), so a cancelled run must still schedule and finish `Required` before the group frees. F2 and F3: 22% of PR runs waited 26–2,203 s, and 13 of 17 started within 1 s of the old run's end. Merge groups never wait.
- Direct case [M, jobs API]: run 44235439701 was cancelled at 04:46:32Z on 11-18. Its `Required` job was created then, started at 05:22:43Z and ended at 05:22:46Z. The superseding run 44239586594 got its first job at 05:22:47Z, 2,203 s after it was created.
- The `velnor-gen` golden has the same pair (`rust-ci.yml:20-22`, `:1644`).
- Effect: §6.6 budgets "push → offer 5–10 s", and M1's kill test would blame GitHub. On the cluster the wait becomes cancel wind-down plus one job, 15–30 s [E], for the 27% of PR runs that supersede another (1.28 of 4.77 per merged PR [P, my proposal §1.5]).
- Fix, phase 0: `Required` runs on `!cancelled()`. Better: no concurrency group on PRs; Velnor cancels superseded work itself. Gate: F2's PR p90 at most 5 s.

**H2. High. A gate run may link artifacts built by code that is not in the landed tree.**
- Evidence: §4.2 lets `pr/<number>` be reused by "that pull request's later pushes and its merge-group candidates". §5.3 makes the PR scope the post-build `target/`. Build scripts and proc macros of an earlier push wrote that directory.
- Example: push 1's build script writes `OUT_DIR/schema.rs`. Push 2 stops generating it but still includes it. Cargo never cleans `OUT_DIR`, so the warm build passes and a cold build fails. The merge group lands a tree that no clean build accepts. That breaks C1.
- Fix: a run whose receipts can become `trusted` builds from a `trusted` generation plus its own delta. `pr/` scopes serve PR feedback only. Cost: the delta build, 10–20 s for the generator and 45–80 s for `velnor-runner-host` [E]; T4 removes most of it.

**H3. High. Compile reuse sits outside the contract.**
- Evidence: §4.1 says "cargo's own fingerprints already play this role". C6 samples receipts only. The nightly run "selects everything" (§6.2) but is not stated to build cold. Each generation is built "from the previous one" with no end (§5.3).
- Why it matters: cargo decides freshness from the dep-info of the last build, so it cannot see a file it did not read then (a new `foo/mod.rs` beside `foo.rs`, an untracked proc-macro read). Today every build is cold, so nothing of this kind can hide today.
- The project already has the counter-measure the draft dropped: targets older than `VELNOR_BASE_MAX_AGE_DAYS` are removed "so the run builds cold" (`ci-helper/crates/velnor-ci/src/cli.rs:73`).
- velnor-legacy deleted "the mtime pin and the persistent target layer because pinned mtimes let stale artifacts read as fresh" (`velnor-legacy/crates/velnor-runner/src/stable_workspace.rs:3-4`). `velnor-ci inputs restore` is more careful, because it restores a time only when the blob matches. The blind spots above and the stale `OUT_DIR` of H2 remain, and nothing audits them. §3.8 does not list this lesson.
- Fix: the nightly and every-20th-merge runs start from an empty `target/` in a fresh pod and compare results and test-binary digests with the warm path. A lineage restarts from empty on every lockfile or toolchain change and at least daily. One disagreement retires the lineage.

**H4. Medium. §1.2 is wrong when the change itself introduces the nondeterminism.**
- Claim: a duplicate run "adds information only about the test's own nondeterminism, not about the change".
- Counter: a regression that fails with probability p per execution is caught today with 1 − (1 − p)³: 49% at p = 0.2, 14% at p = 0.05. One execution gives 20% and 5%. The 5% paired-rerun sample measures the fleet, not this change.
- Fix: execute every obligation whose closure the change touched three times in parallel. Those are the only tests that run, so the cost is small (`velnor-ci`: 781 tests, 10 s median per pass, §A T10 [P]). Three executions restore 49% and 14%; five give 67% and 23%. Any failure blocks.

**H5. Medium. The reuse numbers behind M2 and M4 are wrong in both directions.**

Replay of 142 queue merges at package level [M F5]. An unowned file counts as touching every package. "Window" is the time between the tree the PR verdict tested and the merge.

| Window | Same tree (exact-tree receipts hit) | Nothing to run (closure keys) | Package obligations to re-run |
|---|---|---|---|
| As measured (last push → merge) | 15% | 27% | 49% |
| 60 min | 12% | 22% | 65% |
| 20 min | 52% | 61% | 25% |
| 10 min | 70% | 75% | 15% |
| 5 min | 78% | 83% | 9% |
| 2 min | 89% | 92% | 3% |

- Too high: §5.2's "closure: 50–88%" quotes Claude Opus 5.5's 88%, whose denominator is every package of a full run (342 of 2,910). The same replay had 24 of 97 merge groups needing nothing [P `proposals/opus-5.5.md:303`]. Against the PR's own selected set, half re-runs today.
- Closure keys add 3–10 points over exact-tree keys at every window. The window decides. M4 is not what "removes most of the second verification" (§6.6).
- Too low: at phase-3 speed (push to enqueue in 2–5 min) the base is unchanged in 78–89% of merges. "A moved base is the common case" describes today. The 6 min commitment prices the 11–22% case as the median.
- Fix: commit phase 3 on the 3.3 min the draft computes, move M5 ahead of M4, and use closure keys for what they do deliver (H6, T2).

**H6. Medium. Exact-tree receipts cannot satisfy a full merge group, and M5 predicts the wrong tree.**
- Hard decision 3 runs merge groups in full. A PR run produced receipts only for what it selected, at a tree `main` never had. The unselected obligations have no `tree/` receipt. The "plan and verdict" merge group that §4.3 commits to therefore exists only with owner question 2 or with closure keys. §6.6 says so; §4.3 does not.
- A merge group is built on the queue tip, not on `main`. Branch `gh-readonly-queue/main/pr-1246-26145e69…` sits on PR 1228's merge-group commit `26145e69` while `main` was `27e6c55c` (`fable-5.2/raw/runs_recent.tsv`, runs 44354328553 and 44354335028). `main ⊕ PR` matches only when the queue is empty.
- Fix: predict `queue tip ⊕ PR` (T2).

**H7. Medium. Three admission claims have no mechanism yet.**
- `X-ScaleSetMaxCapacity` is documented as "your scale set's maximum capacity", and the reference listener sends the constant `MaxRunners` (actions/scaleset v0.5.0, the pack's `S`: `README.md:16,49`, `listener/listener.go`). "Equal to free slots" (§3.3, and my own proposal) is an untested reading that could stop assignment when most slots are busy.
- "Responses contain at most 50 messages" (`S/README.md:46`). "Acquires offers in priority order" sees a window, not the backlog.
- I079 promises merge-group-first "by priority class under ARC" in phase 1. A pod's priority is fixed before a job is assigned to it, which is the fact the log uses to reject I043.
- Fix: one scale set per event class, chosen in `runs-on` from `github.event_name`. It works under ARC in phase 1. Test the header on velnor-lab with 2 slots and 5 queued jobs.

**H8. Medium. Dispatcher restart and failover are weaker than §3.7 says.**
- `JobCancel` arrives only on the broker session. A reopened session can conflict for up to 4 min (`MessageListener.cs:52-55, 552-564` [P §F.1.2]). A run superseded during that gap keeps its 16-CPU pods.
- Reopening needs each busy identity's RSA key, so the journal holds runner keys and the lease records hold job tokens at rest.
- The standby "takes over by Kubernetes Lease" on a cluster with one k3s server (§B.4.3 [P]). The lease API dies with velnor-237.
- Fix: the agent treats `renewjob` "job not found" as cancel (`JobDispatcher.cs:781` [P §F.2.4]). Secrets stay on tmpfs and sealed. Control-plane HA comes before the dispatcher, not in phase 4.

**H9. Medium. C5 is incomplete.**
- The engine's task model already marks `uses_network`, `uses_clock` and `uses_random` (`crates/velnor-actions-rust/src/tasks.rs:70-72`). C5 has no randomness.
- Also missing: commit identity (`.git`, tags, `GITHUB_SHA`, `GITHUB_REF`), the CPU count and the environment. `velnor-runner-host/build.rs:70-80` reads the Git object store on its release path. For an obligation that reads any of these, "the same tree" is not "the same thing on the same bytes" (§4.1).
- Fix: no `.git` and a scrubbed environment in build and test sandboxes; a fixed CPU count per class in the definition digest; `uses_random` joins C5.

**H10. Medium. Numbers tagged [M] that measure something else.**
- §6.6 "warm rebuild 8.1–8.8 s": my mbx-warm build of the generator workspace on an Apple M5 Max (`fable-5.2/notes.md` M15), not plain cargo on an EPYC 9454P.
- "35–99 s to rebuild `velnor-runner-host` after a one-file edit": whole-crate compile times on the M5 under varying load (M7). The one-function edit took 10.9 s with incremental compilation on. The draft never says whether `CARGO_INCREMENTAL` is on. The setting is worth 8× on the largest crate, and it breaks M4's bit-identical premise.
- Owner question 8: "−66% … debug assertions and overflow checks stay on". I measured −66% with debug assertions off in dependencies (M14; my proposal §8a #4).
- §6.6 "lanes 55 s": one loaded log of mine. The pack's median for that step is 10 s, p90 62 s (§A T10, n=148). Work item 4 parallelises these tests in phase 1, yet phase 3 still budgets 55 s.
- Phase 1 "runner-min per merged PR ≤ 150": the phase's own items give about 212 [E: 39.3 per PR run after −4.5 setup, −5.6 tests, −3.5 `-j`; 47.2 per merge group; 3.49 × 39.3 + 1.28 × 11 + 1.25 × 47.2 + 2]. It needs T3.
- "Push → offer 5–10 s [E], not measured": F1 + F2 give 8 s to the first job record today.
- Fix: re-tag them. Work item 6 measures all of them on a worker, with incremental on and off.

**H11. Medium. "Longest test ≤ 60 s" is committed on two loops, but the unit of cost is `Journal::open`.**
- `shape.rs:98-106` loops six cases. The single-case test at `shape.rs:60` pays the same per case. My isolated run took 142.5 CPU-s for six cases, about 24 s each on an M5 (M6).
- `Journal::open` has 93 call sites in 41 files of `velnor-runner-host`. On 11-18, 62 tests exceeded 120 s under 96-way contention. The suite grew from 232 to 382 tests, and tenfold in summed time, in two days (M6).
- After the split, the floor is every journal-opening test (40–50 s on an EPYC core [E: 24 s × 1.7–2]) and the suite's CPU sum.
- Fix: the phase-1 gate becomes "`Journal::open` on a template takes at most 1 s, measured", plus a per-test budget in CPU-seconds measured alone.

**H12. Low. The evidence for I110 and I111 does not test what was rejected.**
- Issue #585 ran a 5-crate lab on GitHub-hosted runners with caches moved by `actions/cache`. "Every mbx setting recompiled 10–68" counts cargo `Compiling` lines when the payload lacked source mtimes and the mbx object store ("the target payload does not transport the object store"). Cargo time was 1.0–3.8 s for the variants that restored a payload, against 0.3 s for plain cargo.
- The only velnor-new row is mbx behind a 1.92 GB GitHub-cache restore. The comment itself says "Plain cargo is unverified at velnor-new's size" (#585 results comment, read 2026-11-18).
- Plain cargo over snapshots may well be right. It has not been compared with a node-persistent store.
- Fix: one extra day in work item 6: both engines on the runner workspace, four scenarios, counting rustc processes instead of log lines.

**H13. Low. Receipt keys live on nodes that run privileged dind.** An agent on a docker-class node can sign `trusted` receipts for any tree for 7 days. Fix: one key per node and class; a docker node's key is valid for docker-class obligations only; the audit re-executes on a different node.

## Where the design is too timid

**T1. It makes GitHub jobs start faster instead of taking the work out of them.**
- Tuned step: phase 3 builds `velnor-proto`, `velnor-dispatch` and `velnor-sandbox` (23.5k of the 50.5k budgeted lines) to bring job start under 1 s. F4: an idle ARC runner already starts the next job 2 s after the previous one ends (p90 3 s); it needs only a larger `minRunners`, at 45 MiB per idle pod (§B.2.2 [P]). The dispatcher buys at most 2 s per hop. N05's hot Worker is likely to fail its own "under 1 s" kill test [E: a .NET process start costs a few hundred ms; nobody measured it]. The 4 s of "Set up job" and the 5 s of job finalisation are elsewhere [P, my proposal §1.3].
- Bolder: obligations never execute inside a GitHub job. `ensure(tree, obligations)` is an idempotent call on the node agents. The push webhook calls it 1–2 s after the push (F1: GitHub itself creates `push`-event runs in 1–2 s). The workflow shrinks to one witness job, `Required`, which calls the same `ensure`, streams the logs and exits with the verdict. This is N09 as the architecture, not as a phase-4 fallback "if GitHub's own latency is the larger term".
- Numbers for the median PR: verification = webhook 2 + merge tree and plan 2 + delta build 15 + lanes 10 = 29 s [E: 15 s is 8.8 s on an M5 scaled 1.7×; lanes from §A T10]. The witness is ready at 6 + 2 + 3 + 4 = 15 s (F1, F2, F4, setup). Verdict = 29 + 5 s to complete the job, **about 35 s**. The draft has 96 s in phase 3 and 42 s in phase 4, and phase 4 also needs the dispatcher, the hot Worker and the App. A three-hop shape cannot reach a minute: 3 × 10–15 s of hop overhead plus 8 s before the first job leaves 7–22 s for all work.
- Risk: a late or lost webhook (the witness's own `ensure` is the fallback: slower, never wrong). Velnor's merge tree may differ from GitHub's merge ref (exact keys turn that into a miss). PR code runs without a job (same-repository only, and with no token at all).
- Confidence: C1 and C4 get stronger. The agent starts and observes every result, and the job's own report disappears as a source of evidence. `docs/goals.md:16` holds literally: the official runner executes the only job.
- Experiment, 2–3 days on velnor-lab: a webhook receiver, one warm pod, a one-job workflow that polls for the result. 50 pushes. Record webhook arrival, first cargo command and verdict.

**T2. It verifies twice and plans to make the second run cheap. Verify once, on the tree that lands.**
- Tuned step: phase 3 re-runs the PR's obligations in the merge group when the base moved. M5 speculates on `main ⊕ PR` (H6).
- Bolder: the PR verdict runs on `queue tip ⊕ PR`, built from the trusted generation, with keys by input closure (P1). When GitHub creates the merge group, the Velnor App checks the keys and posts `Required` on its SHA within 2 s. Only obligations whose closure both sides touched run then.
- Numbers (F5): with a 2–5 min window, 83–92% of merge groups run nothing and 3–9% of package obligations re-run. That is 1.08–1.17 verifications per merged PR, against 2 in the draft's moved-base case and 3 today. Push-to-merge = verdict + 5 (auto-merge) + 19 + 2 + 26 = verdict + 52 s [P: 19 s and 26 s from `proposals/opus-5.5.md:52`]. GitHub's queue is then 45 of about 90 s. Stretch: Velnor merges the verified commit itself, bors-style, through an App with bypass: verdict + 5 s.
- Risk: a queued PR ahead fails and the prediction is wasted (3 of 40 queue exits were failed checks [P S2]). Owning the merge changes how GitHub marks a PR as merged.
- Confidence: the same or higher. The landed commit has a receipt for every obligation key, checked at the merge-group SHA. A wrong prediction costs time only.
- Experiments: F5 is the replay M4 schedules for phase 4, and it took an hour with data the panel already had. Then two weeks of shadow prediction of queue-tip trees (kill below 70% match), and one day on velnor-lab where an App posts the status on `merge_group.checks_requested` to measure enqueue → merged.

**T3. The cheapest large lever, selection that works, waits for the phase-2 cutover.**
- Tuned step: I068 arrives with `velnor-gen` in weeks 3–6. Until then the heavy jobs run in 97.4% of PR runs [P], because `plain_group` sets `undeclared_reads: base.package.has_build_script` and declares no inputs (`crates/velnor-actions-rust/src/tasks.rs:253-254`).
- Bolder: fix it in phase 1 in the current engine. Declare the inputs of the three crate-root build scripts and the test reads the plan already names. Merge groups stay full under hard decision 3, so the gate does not change.
- Numbers [E from §A T3 and T10 medians]: a generator or ci-helper PR (47%) then runs Plan 45 s, `velnor-ci` 76 s, verdict 49 s, `Required` 6 s and the hops: about 3.5 min instead of 10.9, and 8 runner-min instead of 52.9. Phase 1's "≤ 150 runner-min" becomes reachable: 139 against 212 without it (H10). The contention behind the 1.2–2.4× stretch falls before any cache exists.
- Risk and confidence: a missed input shows up as a merge-group failure, never as a false green.
- Experiment, 1 day: replay the last 200 PRs through the fixed planner in shadow and compare the selected jobs with the jobs that failed.

**T4. It treats the test floor as two functions and the compile floor as fixed.**
- Tuned step: split two loops; "runner PRs about 4 min"; engine PRs "5–6 min until fixed" with no work item; I064, I073 and I104 parked as experiments.
- Bolder, in order of yield:
  1. Dependencies at `opt-level = 3` in every root. With warm generations it costs nothing per PR: dependencies rebuild only on a lockfile change (764 CPU-s once, M14).
  2. A pre-migrated journal template, so `Journal::open` takes under 1 s.
  3. Split `velnor-runner-host` into 8–10 crates. A leaf edit recompiles about 15k lines instead of 157k, and fewer tests have the change in their closure.
  4. Build the image-publication fixture (89 s, §A T10) as a cached artifact. 86% of PRs do not touch the engine.
  5. Decide `CARGO_INCREMENTAL` (H10).
  6. Rent two high-clock nodes this week (order of €100–200 a month each [E, list price to confirm]).
- Numbers: the runner-host suite summed 1,857 s for 232 tests on 11-16, so about 3,000 CPU-s for today's 382 [E]. × 0.34 (M14) × 0.55 (#1349's claim [P]) = 560 CPU-s = 35 s on 16 threads. A runner PR verdict becomes 15–45 s build + 35 s tests + 15 s = **1.1–1.6 min**, against "about 4 min". An engine PR becomes about 2 min, against 5–6.
- Risk: optimised dependencies can hide an overflow or undefined behaviour that a debug build catches.
- Confidence: keep debug assertions and overflow checks on in dependencies and measure again (my −66% had them off). The nightly cold run uses the plain profile.
- Experiment, 1–2 days on one worker: CPU-seconds of the 20 slowest tests under four profile variants; a branch with the template; `cargo build --timings` for a one-function edit with incremental on and off.

**T5. "Before the push" is phase 5, and it aims at the wrong moment.**
- Tuned step: M8 after week 20, as a "preflight call" before pushing.
- F6: agents push 11 s (median) after they commit. A preflight at commit time hides 11 s.
- Bolder: the cluster is the agent's test command. `velnor verify` sends the working tree as a Git tree object to the node mirror and calls the same `ensure` as T1. The agent gets its answer from 96-thread nodes, and the later push finds receipts.
- Numbers: a preflighted verdict is GitHub's latency only, 15–20 s (F1, F2, F4). The 0.87 failed and 1.28 cancelled PR runs per merged PR [P] move before the push: 4.77 → 2.62 PR runs per merged PR if all of them are caught first [E].
- Risk: agents skip it. A macOS-only failure is missed, as it is today.
- Confidence: the same. Only results that Velnor executed count, as N07 already says.
- Experiment, 2 days: log the duration of every local test run on the agents' machines. If the median exceeds 30 s, this overlap is worth more than any change to the runner.

**T6. The calendar is paced for people.**
- The repository merged 142 PRs through the queue in three days (F7). "At least 50 merges" is one day of traffic, not a phase.
- Every moonshot's "cheapest experiment" takes days and depends on no phase, yet they sit behind phases 2–4 (weeks 6–20).
- Bolder: run the M1, M3, M4, M5, M6 and M7 experiments and the hardware rental in week 1, beside phase 0. Let them choose the architecture before anyone writes 23.5k lines of dispatcher.
- Risk and confidence: none to confidence; every experiment runs on velnor-lab or on one drained worker. The cost is about two weeks of agent work spent before the phase-0 numbers are in.

## The hardest problems

**P1. One verification per landed tree, trusting nothing unproven.**
- Why it is hard: a verdict is reusable only if every input is in its key; GitHub, not Velnor, builds the candidate; and today the three runs of one merge see three different trees.
- Attack:
  1. **The key is the hash of what the sandbox contains.** An obligation's sandbox is a sparse checkout: the directories of the package's dependency closure (30 packages; the graph comes from the manifests), its declared extra inputs, and the workspace's manifests and lockfile. Git already stores a hash per directory. What is not in the key is not on disk, so C2 holds by construction for repository files, negative lookups included. The repository already works this way in two places: Rust jobs check out without `docs/**` and fail on `non_input_present` (§C.4 [P]), and generator tests run under `PATH=/nonexistent` (`generator/.config/nextest.toml`).
  2. **Selection is a lookup, not a diff.** The plan lists every obligation with its key, and whatever has no receipt runs. PR, merge group, push and nightly use one rule. Hard decision 3 then holds literally: the merge group verifies everything, mostly by receipt.
  3. **Verify on the queue tip, from trusted artifacts** (T2, H2).
  4. **Audit by outputs.** Two executions of one input key must give the same test-binary digest; a mismatch flags an undeclared input or nondeterminism. The output digest audits the key instead of being the key, so incremental compilation stays possible. The nightly run uses a full checkout and a cold build, which catches a closure that is declared too small.
- Numbers: H5's table; 1.08–1.17 verifications per merged PR.
- Kill: more than 20% of packages cannot build and pass in a closure-only checkout after their inputs are declared (a script over 30 packages, 2 days), or queue-tip prediction matches under 70%.

**P2. Zero cold start, zero queue, placement and "before the push" are one problem.**
- The quantity: the time from "this tree exists" to "a compiler runs on a warm disk". The draft's own figures give 20–25 s after the push in phase 3 (5–10 + 10 + 3 + 2) and 17 s in phase 4 (8 + 5 + 2 + 2).
- Attack: one idempotent `ensure`, four callers: the agent's `velnor verify`, the push webhook, the witness job and the merge-group webhook.
- Placement: Velnor's scheduler picks the node that holds the generation and the PR scope and has idle CPU. GitHub's choice of runner decides only where a witness prints logs.
- Cold start: agents keep pre-created sandboxes with no Worker in them. The target view is an overlayfs mount over the frozen generation (constant time) or a reflink clone (per file). Time both in work item 6. First cargo command at push + 3–4 s [E: webhook 1–2, merge tree and plan 2].
- Queue: foreground work is admitted in CPU-seconds, merge path first. Everything else runs in idle-class cgroups (P5).
- Failure handling: all four callers are idempotent on the key. Velnor cancels a superseded head within a second, without waiting for GitHub (H1).
- Kill: webhook arrival P50 is worse than the 8 s GitHub needs to create the first job record (F1 + F2).

**P3. The 200–270 s test and the 157k-line crate.**
- T4 gives the order. The principle: the floor is CPU-seconds per obligation divided by threads, and both terms are code.
- A test-time budget in CPU-seconds measured alone: 10 s, blocking for new tests from phase 1, with a per-package sum. The draft's 30 s of wall time under load is neither tight nor stable, and the suite grew tenfold in two days (H11).
- The crate split pays four times: compile delta, tests executed, receipt misses and flake exposure all follow closure size.
- Kill for the split: on a branch, a leaf edit still rebuilds more than 30% of the lines.

**P4. Flake-free required checks without retries that hide bugs.**
- Today: 8.1% false failures per full run, so about 15% of merges hit one [P S1, N14]. The draft counts flakes and files issues, but each one still ejects a queue entry and rebuilds everything behind it.
- Attack by class:
  1. **Infrastructure** (CPU starvation, 2 s kills, the `event.json` race): sized sandboxes, test threads equal to sandbox CPUs, batch work in idle cgroups.
  2. **Old nondeterminism in code the change did not touch:** with closure keys these tests do not execute, so they cannot fail this change. A generator or ci-helper PR is exposed to `velnor-ci` only: 2 of 124 runs, 1.6% [P S1].
  3. **Nondeterminism in what the change touched:** stricter than today. Three executions, any failure blocks (H4), and a test the PR added or edited gets no second chance.
  4. **What remains:** re-execute the failed obligation once, at once, in a fresh sandbox. If it passes, the run is green, the ledger records a flake, the budget is charged and an issue opens. The cost is seconds, not an ejection. This is what agents do today by re-queueing (17% of queued PRs were queued more than once [P, my proposal §1.1]), made visible and bounded. It needs the owner's decision, because the required profile has retries 0.
- Target: at most 0.5% of merges ejected by a false failure. Kill step 4 if recorded flakes rise week over week.

**P5. A shared cluster at 92–95% CPU.**
- Aim: make latency independent of utilisation.
  1. Remove the demand: 347 → about 25 runner-minutes per merged PR is −93% (Targets).
  2. Make the rest preemptible in microseconds. Warmers, audits, stress runs, speculation and the nightly run go into cgroups with `cpu.idle = 1` (kernel 6.12 has it; the privileged agent writes it). They use every idle cycle and yield at once. Pod preemption kills work; this does not.
  3. Honest requests fix the weights. CFS shares follow requests, so a 16-CPU job outweighs another organisation's 2-CPU pod under contention, and both still burst when the node is idle.
  4. Foreground sandboxes get cpusets aligned to the EPYC's 6-core L3 groups, so a neighbour cannot evict their cache.
  5. The SLO is pressure, not utilisation: CPU PSI "some" under 2% for merge-path sandboxes. velnor-240 showed 8.4% at 86 busy cores [P, my proposal §1.6].
- Experiment, 1 day on one worker: a runner-host build and test with and without a 96-thread spinner in an idle cgroup. Pass if the slowdown is under 5%.

## Decisions you would overturn

| Id | The log decides | I would decide | Evidence |
|---|---|---|---|
| I003, I004, N10, N11 | Dispatcher and node-held leases replace ARC in phase 3 | Experiment-gated behind T1. Build `ensure` and the witness under ARC first; build the dispatcher only for a measured residual over 5 s per run | F4: idle-runner start 2 s p50, GitHub hop latency 0–1 s. Placement, ordering and fork refusal move to Velnor's scheduler. H7, H8 |
| N05 | Hot Worker, experiment-gated in phase 3 | Run its one-day experiment in week 1 and expect a kill | Its kill test is "saves under 1 s"; assignment to start is 2–3 s in total (F4) and the larger terms are "Set up job" and finalisation |
| N06 | Dispatcher completes receipt-satisfied jobs and the plan job | Reject for the plan; skip by job-level `if:` and the witness | The process that holds every runner key would parse PR-controlled content. A skipped job already costs no runner (`ci.yml:649`) |
| N09, N07 | Experiment-gated, phases 4 and 5 | Adopt as the phase-3 core | T1, T5, F1, F6 |
| N01, I055 | Exact-tree key is the committed ledger key | Input-closure key from the start; the exact tree is its special case | H5 (15% hit today), H6 (cannot cover a full merge group) |
| N02 | Closure key is the test-binary digest | Key by input; the digest audits it | P1: no bit-identical builds needed, the key exists before the build, and incremental compilation stays possible (10.9 s against 82–99 s, M7) |
| N03 | Speculate on `main ⊕ PR` | Verify on `queue tip ⊕ PR` | H6 |
| I054 | Merge-group selection is a separate owner decision | It follows from lookup; the shadow run and the audit stay | P1 |
| I068 | Phase 2 | Phase 1 | T3; `tasks.rs:253-254` |
| I064 | "Experiment" | Adopt for every root once generations are warm; measure with assertions on first | T4, H10 |
| I073 | Experiment-gated | Adopt | T4, P3 |
| I104 | After phase 2 | Week 1 | T4, T6 |
| I110, I111 | mbx rejected, plain cargo adopted | Head-to-head in work item 6 before either is final | H12 |
| I079 | Merge-group-first by priority class under ARC | One scale set per event class | H7 |
| I015 | Capacity header equals free slots | Test first; the documented meaning is the maximum | H7 |
| I032 | The PR scope is read by the PR and its merge group | By the PR only | H2 |
| I065 | New tests over 30 s blocked from phase 2 | 10 CPU-s measured alone, from phase 1, plus a per-package sum | H11 |
| N14 | Retries stay 0; a flake fails the run | Keep 0 for tests the PR touched; one counted re-execution otherwise (owner decision) | P4 |

I agree with the log against my own proposal on I009 (lease records), I031 (merge groups do not write the trusted scope) and I082 (pre-created pods).

## Targets

My phases: **0** = the draft's phase 0 plus H1. **1** = the draft's phase 1 plus T3, H7's scale sets, T4 items 1–2 and every week-1 experiment. **2** = generations, warmer, one job per root, lookup receipts in the plan job, T4 items 3–5. **3** = `ensure` on the webhook, the witness job, closure keys, queue-tip verification, the App's status on merge groups; no dispatcher. **4** = `velnor verify` as the agents' default and Velnor's own merge step.

| Phase | | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Cache and reuse |
|---|---|---|---|---|---|
| 0, week 1 | committed | 12 / 20 min | 25 / 60 min | about 347, unchanged | none |
| | stretch | | 20 / 45 min | | |
| 1, weeks 2–3 | committed | 4 / 10 min | 12 / 30 min | ≤ 150 | tools, toolchain, action archives 100% from the node; compile 0 |
| | stretch | 3.5 / 8 min | 10 / 22 min | ≤ 120 | |
| 2, weeks 3–6 | committed | 2 / 6 min | 5 / 15 min (9 / 20 if merge groups stay full) | ≤ 45 | ≥ 95% of units fresh without a lockfile change; ≥ 65% of merge groups execute nothing |
| | stretch | 80 s / 4 min | 3.5 / 10 min | ≤ 30 | ≥ 75% |
| 3, weeks 6–10 | committed | 60 s / 3 min | 2.5 / 6 min | ≤ 25, batch reported apart | ≥ 97% of units; ≥ 85% of merge groups execute nothing; ≤ 0.5% of merges ejected falsely |
| | stretch | 40 s / 2 min | 100 s / 4 min | ≤ 15 | ≥ 92% |
| 4, after | stretch | 30 s / 2 min; 20 s after `velnor verify` | 90 s / 3 min; 50 s with Velnor's merge step | ≤ 10; ≤ 2.6 PR runs per merged PR | |

Arithmetic:
- Phase 0: verdict = the 10.9 min zero-queue floor [P] plus cluster hops. Merge = 12 + 0.3 + 12 + 0.75.
- Phase 1: median PR 3.5 min (T3); the merge group stays full at about 7 min: 3.5 + 0.3 + 7 + 0.75 = 11.6. Runner-minutes: 139 (H10).
- Phase 2: verdict = 8 + 10 (plan hop) + 45 (root hop: 3 + 3 + 2 + 20 + 12 + 5) + 10 = 73 s. Merge = 1.25 + 0.4 + 0.4 + 0.45 = 2.5 min when nothing re-runs, which is 78% of merges at a 5 min window (H5). The rest add one PR-sized run.
- Phase 3: verdict 35 s (T1) with a 1.7× margin. Merge = verdict + 52 s (T2). P90 comes from runner PRs at 1.1–1.6 min (T4) plus a moved-base re-run. Runner-minutes: 4.77 × 1.8 + 1.25 × 0.5 + 0.2 + 4 for warmers = 13.4.
- Phase 3 is committed only if the week-1 experiments pass (T1 webhook under 8 s; P1 closure checkout for at least 80% of packages; queue-tip match at least 70%). If they fail, the draft's own phase-3 design still supports 90 s / 4 min for the verdict and 4 min push-to-merge P50: its segments with the measured 10 s lane sum to 55 s, and its unchanged-base case (3.3 min) covers 78–89% of merges.

## Anything missing

1. **A metric for the whole loop.** "Last push → merge" ignores 0.87 failed and 1.28 cancelled PR runs per merged PR [P]. Add first push → merge and time to first red. Order work to fail fast (previously failed, then changed, then the rest) and post a check at the first failing obligation.
2. **The feedback loop that produced 11-18.** Slow verdicts widen the window, a wider window moves bases, moved bases re-run, re-runs slow verdicts. H5 shows both ends: 89% same-tree at 2 min, 12% at 60 min. The system has two stable states. It needs an alarm and a control at a window over 10 min: pause speculation and batch work, serve merge groups first.
3. **A break-glass path after ARC leaves.** Four weeks after the cutover, Velnor verifies Velnor's own code with no fallback. Keep the hosted workflow as a permanent fallback, exercise it weekly, and run the cluster only on released agent versions.
4. **Webhook ingress.** The mirror already depends on push webhooks (§5.2), but nothing says where they land (traefik is disabled and the firewall is closed, §B.4.3 [P]), how they are authenticated, or how a gap is replayed.
5. **`CARGO_INCREMENTAL` and the test profile** are not decided anywhere (H10).
6. **Auto-merge by the App, not by procedure.** 0 of 40 PRs enabled it [P S2]. The Velnor App enables it when a PR opens unless a label says no.
7. **A budget for batch work.** Five nodes rebuild `main` on every merge, plus audits, stress runs and speculation. "Runner-minutes" hides that and the move from 2-CPU to 16-CPU pods. Report foreground and batch CPU-minutes apart.
8. **Global invalidation.** An image, toolchain or helper bump changes every definition digest. Build the next generation and seed its receipts before the pin lands, or the hour after the merge is cold for everyone.
9. **The other organisations' pod templates.** Nothing in §7 changes them: 2-CPU requests and no limit under ARC. P5's weights protect velnor-new only if those classes are honest too.
10. **Phase 0's first item is not live.** PR #1332 merged on 2026-11-18 at 20:36 UTC, but `ci.yml` at its merge commit `7088ebbe` still has 8 jobs on `ubuntu-26.04`. The engine change is in; the regenerated workflow is not.

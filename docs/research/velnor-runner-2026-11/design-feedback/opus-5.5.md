# Design feedback by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

# Design feedback on the draft runner architecture (Claude Opus 5.5)

Read: the draft, the decision log, the comparison, the evidence pack, all proposals and reviews.
New measurements below are from the pack's raw data (`~/.cache/ci-ambition/evidence/ci-raw/`) and my
earlier PR timelines; every script is in `opus-5.5/work/fb/` and reruns in seconds. Tags as in the
draft: [M] measured, [E] estimate with arithmetic, [T] target.

## 1. Verdict

No, not as written. The committed path lands at about 6–8 min P50 push-to-merge in phase 3. Its only
recommended route to 6 min, merge-group selection (owner question 2), breaks C1 under the ruleset's
HEADGREEN grouping. Built in a different order, the same components reach about 2.5 min P50 in phase 3.
**Ambition 6/10:** the moonshots are named, but the cheapest big wins (App-posted verdict, preflight, cross-tree receipts) sit in phases 4–5.
**Soundness 6/10:** protocol and code facts hold; the push-to-merge model has two load-bearing errors and misses a measured 60–626 s stall.

## 2. Correctness and confidence holes

**H1. Critical: merge-group selection is unsound under HEADGREEN, and C1 already fails for intermediate
commits.** The ruleset is HEADGREEN with up to 10 PRs per merge (decision log S3). Under HEADGREEN a PR
whose own merge-group checks failed still merges when a later entry of its group passes.
`velnor-ci plan` with `on_main = "affected"` diffs a merge group against its own parent, the queue entry
ahead (`ci-helper/crates/velnor-ci/src/event.rs:43-44`; help text at `src/cli.rs:53`). Sequence: entry
A fails its selected tests; entry B (main+A+B) selects only B's change and passes; GitHub lands A and B
together. A's failing change lands with no passing result anywhere. Today only `Plan::validate` and
`Required` rejecting `not_affected` on merge groups prevent this (`docs/hard-decisions.md:208-210`), and
that is the protection owner question 2 removes. The two gates the draft puts on question 2 cannot see
the failure: the file-read audit and the shadow plans both judge selection on one commit and never model
a failed predecessor. Separately, C1 says "every commit that lands on `main`". Under HEADGREEN, A's
commit lands as an ancestor even with full runs, and that is true today. **Fix:** never skip relative to
the queue predecessor. Skip only on a receipt whose key equals the obligation's key in the candidate
(T3), or switch to ALLGREEN before any merge-group selection. Restate C1 as "every commit that becomes
the tip of `main`", and mark intermediate commits for bisection.

**H2. High: "with an unchanged base the merge-group run executes nothing" (§6.6, §4.3) is false while
merge groups stay full.** A PR run executes only the obligations it selects. A full merge group needs a
passing receipt for every obligation of its tree, and with exact-tree keys none exists for the
unselected ones. Take a generator-only PR, the median class: its merge group re-runs the runner root,
the root workspace and `local`. That is a warm full run of 3–5 min [E], not 30 s. The 30 s figure and
the phase-3 P50 of 6 min both depend on question 2, so on H1. **Fix:** keys under which main's results
cover unchanged roots and packages (T3). A full merge group, which keeps hard decision 3, then finds a
receipt for every obligation without base-relative skipping.

**H3. High: superseding pushes wait for the cancelled run, and the draft never models it.** [M] PR runs
created while their cancelled predecessor was still running were 33 of 105 sampled runs. They waited
p50 60 s, p90 626 s, max 2,203 s for GitHub to create their first job; every other PR run waited 1–2 s.
The first job appears at the second the predecessor ends (p50 gap 0 s). On 11-18 the wait was p50
611 s, p90 1,894 s (`work/fb/prejob.py`). The cause is the per-PR `concurrency` group with
`cancel-in-progress` (`.github/workflows/ci.yml:57-59`) combined with the protected `Required` job,
which is hosted and `if: ${{ always() }}` (`ci.yml:614-621`). A cancelled run therefore ends only after
a hosted runner has run that job. The `always()` is deliberate, because GitHub counts a skipped required
check as passing (`docs/hard-decisions.md:216-219`). velnor-gen keeps both choices
(`generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml:20-22,1644`, with
`runs-on: ubuntu-latest`). On top of that, `pull_request` triggers on `edited` (`ci.yml:7`) so that a
base retarget re-plans (`hard-decisions.md:220`). [M] 469 of
2,558 PR runs (18.3%) started on an unchanged head SHA, and 332 of them cancelled an identical run
still in flight (`work/fb/samesha.py`). Each one discards finished work, then waits for the
predecessor's teardown. **Fix:** the Velnor App posts `Required` as an App-bound status (N12) instead
of a job. A cancelled run then leaves neither a pending job nor a skipped check that counts as green.
Drop `edited` and let the App re-plan on a base retarget. Write a receipt per obligation as it finishes,
not at job end, so a restart on the same tree reuses what was done.

**H4. High: the heaviest root runs in the smallest class.** The runner root's `local_pair_public` and
`local_host_public` test binaries, and the `velnor-local` tests, need a Docker daemon in CI
(`crates/velnor-runner/.config/nextest.toml:5-11`, `local/.config/nextest.toml:5-11`). With one job per
root, the runner root must therefore run in the docker class. It is touched by 55 of 100 merges
(`work/fb/rootreuse.py`). The docker class is 8 CPU plus 4 for dind, on one tainted node per site with
one idle pod each (§3.6, §7). That is half the build class, for the job that sets the floor. **Fix:**
make the Docker-needing binaries a separate obligation that runs in a docker pod on the same node and
consumes the build pod's archive by reflink (about 1 s). Or give build pods a rootless Docker runtime
(I085) in phase 2, not phase 5.

**H5. Medium: −66% test CPU is quoted for a configuration nobody measured.** Fable measured dependencies
at `opt-level = 3` *without* debug assertions (`proposals/fable-5.2.md:345`). The draft attaches −66% to
"debug assertions and overflow checks stay on" (§13 question 8, M7). My profile of the slowest test
shows debug-assertion precondition checks (`Layout::from_size_align_unchecked::precondition_check`) in
turso's hot path (`work/bench/sample-top-of-stack.txt`), so part of the measured gain came from turning
the assertions off. **Fix:** measure both settings and give the owner two numbers.

**H6. Medium: enforcing inputs by making undeclared files absent is not fail-closed.** §4.3 says an
undeclared read "finds no file and fails the test". A test that probes instead (`exists()`, `read_dir`,
skip-if-missing fixtures, optional configuration) takes another branch and passes. fanotify never
reports a failed open, so the audit is blind to the same case. **Fix:** in the run that produces a
receipt, log every lookup outside the declared set that returns `ENOENT` (seccomp user notification on
`openat` and `stat`, or a FUSE tripwire view). The audit must also compare restricted-view results with
an unrestricted run of the same binary, not only re-run inside the same view.

**H7. Medium: nothing audits the compile cache.** C6 re-executes a reused *result*. Suppose a reflinked
snapshot yields a stale binary, for example because a clone reset mtimes so outputs look newer than an
edited source. That binary passes, its pass becomes a receipt, and the audit re-runs the same stale
binary. **Fix:** the nightly full run builds from an empty target directory and compares test-binary
digests and results with the warm build of the same tree; one mismatch disables snapshots. Run M4's
digest experiment on two commits that do not touch the crate, not on one commit built twice, so
commit-dependent embedding shows up.

**H8. Medium: the audit tripwire will fire on flakes.** "One disagreement disables that kind of reuse"
(C6). At about 50 obligations per merge, 100 merges a week and 2% sampling, the audit makes about 100
re-executions a week. If 0.1–0.3% of obligation runs fail falsely, a spurious disagreement arrives every
3–10 weeks [E]. **Fix:** on a disagreement, re-run the obligation 5× on the original tree. A flaky test
becomes a C7 incident; only a deterministic disagreement turns reuse off.

**H9. Medium: merge-first admission depends on offer semantics nobody has seen.** §3.3 acquires "offers
in priority order" while advertising capacity equal to free slots. If GitHub sends no more `JobAvailable`
messages than `X-ScaleSetMaxCapacity` allows (§F.1.3), Velnor never sees the merge job queued behind
ten PR jobs. Nothing in the pack shows which way it works. **Fix:** from phase 0 the generator emits
`runs-on: ${{ github.event_name == 'merge_group' && 'velnor-merge' || 'velnor-build' }}`. That gives
merge jobs their own scale set and reserved capacity, under ARC now and the dispatcher later. Then test
on velnor-lab: capacity 1, five queued jobs, log which offers arrive and which are re-offered.

**H10. Low: numbers tagged stronger than their basis.** §5.2 tags "plain cargo compiled 0 crates on
unchanged trees" as [M], but it was measured on a 5-crate lab repository. velnor-new's only #585 run
used mbx and was slower warm (345 s) than cold (291 s) (comparison, "Numbers that conflict"). It is [E]
until work item 6 measures it. The phase-3 budget also uses a 55 s single-threaded `velnor-ci` suite,
although work item 4 parallelises that suite in phase 1. The measured step is 7 s p50 and 62 s p90,
because the planner filters tests (`work/fb/steps.py`).

## 3. Where the design is too timid

**T1. The targets assume today's moved-base rate in the regime the design removes.**
- **Draft:** phase-3 P50 is 6 min because "a moved base is the common case" (§6.6, from S4's 0 of 40 and 11 of 60).
- **Bolder:** commit push-to-merge P50 ≤ 4 min in phase 2 and ≤ 2.5 min in phase 3, with no merge-group selection.
- **Mechanism:** the base moves only when another PR lands between a PR's last run starting and its
  merge group being created. That window is hours today and becomes V + about 1 min, where V is verdict time.
- **Numbers:** [E] the replay (`work/fb/window.py`) uses the last-push times of 98 merged PRs
  (11-16 15:00 to 11-18 06:00) as arrivals. The merge-group tree equals the PR-tested tree in 89% of
  merges at V = 1 min, 83% at 2.5 min and 53% at 11 min (today's zero-queue floor). At twice today's
  demand it is 80% and 69%. Sanity check: on the actual slow history the same mapping gives 15 of 100
  (`rootreuse.py`), in line with S4's 18%. For those 83–89%, push-to-merge ≈ V + 10 s auto-merge + 19 s
  + 5 s verdict + 26 s ≈ V + 1 min.
- **Risk:** demand rises as CI speeds up (the ×2 rows), and a backed-up queue widens the window again.
- **Confidence:** exact-tree keys only, so C1–C7 are unchanged.
- **Experiment:** work item 3's tested-tree log plus this replay on each week's data. The claim is
  killed if the hit rate at the measured V falls below 70%.

**T2. Neither the merge group nor the verdict needs a job.**
- **Draft:** an App-posted `Required` in phase 4 (I013); the plan stays a job.
- **Bolder, phase 2:** the Velnor App handles `merge_group checks_requested` and PR check suites
  itself. It runs the released planner on the node mirror, looks up receipts and posts `Required`.
  Jobs run only for obligations without receipts, and each root job plans itself.
- **Numbers:** [E] a merge group drops from 30 s to about 5 s (webhook 1 s + plan 2–3 s + lookup). PR
  verdict drops 13–20 s (the plan job's 10 s and `Required` job's 10 s in §6.6, minus 3–5 s for the
  App). It also removes H3's 60/626 s stall at its root.
- **Risk:** the App becomes code that can authorise merges. Bind `Required` to it (N12), keep the
  workflow's `Required verdict` as a second required check for a month, and shadow it first.
- **Confidence:** the plan still comes from the released helper (rule 12), and every obligation still
  needs a passing result.
- **Experiment:** a week of shadow posting to a non-required check name. Zero disagreements with the
  workflow's `Required` over at least 500 events.

**T3. Cross-tree reuse waits for phase 4 and its hardest form.**
- **Draft:** closures keyed on test-binary digests, needing reproducible builds and a namespace executor (M4).
- **Bolder:** input-addressed keys at the granularity the repository already has: per root in phase
  2, per package in phase 3. Digest keys remain the phase-4 refinement.
- **Mechanism:** the five Cargo roots are independent. Only `ci-helper` reaches outside its own root,
  into five `generator` crates (`work/fb/roots.py` over `cargo metadata`). A root's key is the Git tree
  hashes of the root, its cross-root dependencies and the shared files (toolchain pin, `mise.*`,
  `.github/`, `.velnor/`), plus the definition digest. The hashes are free from the commit. Each root
  job uses a sparse checkout of exactly those paths, with H6's `ENOENT` log for probes. A PR run
  executes every obligation of each root it touches, which is cheap for `generator` and `ci-helper`.
  Package keys in phase 3 add cargo's package graph; cargo's dep-info already covers compile-time reads.
- **Numbers:** [E] replay over 100 real merges (`work/fb/rootreuse.py`), root-jobs executed by merge groups:
  500 full, 425 with exact-tree receipts, 171 with exact-tree plus merge-group selection, 125 with
  per-root keys. Per-root keys need no base-relative skipping (H1). In the short-window regime, 85–90 of
  98 merge groups execute nothing (`window.py`).
- **Risk:** keys too narrow if shared inputs are undeclared. 17 of 98 PRs touched a file the replay
  conservatively mapped to every root.
- **Confidence:** a root's receipt covers exactly what its job could read. Main's receipts come from
  merge groups whose code landed, which is today's trust, and any shared-file change invalidates every root.
  Reuse across trees, which C4 restricts, is safe here for a stated reason. Everything that could forge
  a root's result is inside its key: the root's files, `.github/` and the pinned helper. The ledger
  accepts a receipt only for an obligation the released planner assigned to the reporting job, which it
  identifies by the peer credentials on the node socket.
- **Experiment, 1–2 days on velnor-lab:** run each root from its sparse checkout. Every failure names
  an undeclared cross-root input. Then replay 200 merges with the final declarations.

**T4. A 60 s verdict for the median PR belongs in phase 3, not as a phase-4 stretch.**
- **Basis:** [M] today's jobs for the median class take 64 s (`velnor-gen`) and 76 s (`velnor-ci`)
  p50, including 10 s of tool preparation, 5 s of set-up, 3 s of checkout and 12–14 s of building. Test
  steps take ≤ 3 s and 7 s p50 (`work/fb/steps.py`).
- **Arithmetic:** the draft's own phase-3 parts are push → offer 8 s, hot start 3 s, clone 2 s and
  warm build 9 s, then lanes max(tests 7 s, clippy 8 s, docs 5 s), for about 30 s. Add 3 s with T2,
  or 20 s more if the plan and `Required` jobs remain.
- **Commit:** ≤ 60 s for the median class in phase 3, with a stretch of ≤ 35 s.
- **Experiment:** the draft's own M1 test on a hand-warmed pod in phase 2.

**T5. Start CI at commit time: preflight in phase 3, not phase 5.**
- **Basis:** [M] a PR's final commit reaches its first CI run after p50 42 s and p90 320 s (100 merged
  PRs, `work/fb/commit2ci.py`). That head start exists whether or not agents test locally, so M8's kill
  test ("agents rarely test before pushing") asks the wrong question.
- **Mechanism:** a post-commit hook pushes to the Velnor mirror and calls preflight on `main ⊕ commit`.
  The ledger adds in-flight join: a request whose key is already executing waits for that execution
  instead of starting a duplicate. The same join serves H3's same-SHA restarts and speculative candidates.
- **Numbers:** [E] with V of 45–60 s, the median verdict lands 5–20 s after the push.
- **Risk:** batch work spent on commits that are never pushed. Pushes on a PR are 41.7 min apart at
  p50, so this is at most one wasted preflight per push.
- **Confidence:** only Velnor-executed linux/amd64 runs on the same key count.
- **Experiment:** a week-long hook that only records commit time and tree. It measures the head start
  and how often the pushed tree equals the preflighted one.

**T6. Remove the runner root's rebuild instead of renting clock speed for it.**
- **Basis:** `velnor-ci` deletes `incremental/` before saving the target cache
  (`ci-helper/crates/velnor-ci/src/io/cache.rs:49,90-115`), so no snapshot carries incremental state.
  [M] With a warm target and no incremental state, a one-file edit in `velnor-runner-host` takes 61 s
  wall and 159 CPU-s to rebuild one crate (my M5 bench under load, `work/bench/incr.out`).
- **Bolder:** keep incremental state for workspace crates in the PR-scope and main snapshots, and
  adopt I073 (split the 157k-line crate). One crate is one unit for both rebuilds and receipts.
- **Numbers:** [E] 61 s drops to 10–20 s per configuration, three to six times faster, which is
  typical for one large crate.
- **Risk:** rare incremental-compilation bugs, and incremental directories of several GB.
- **Confidence:** H7's nightly cold build compares results; one divergence turns incremental off.
- **Experiment, 1 day:** on a worker, touch 10 representative files with incremental off and on, run
  the suite on both builds and compare.

**T7. Retry only flakes the change cannot have caused.**
- **Draft:** retries stay at 0, so every flake ejects a merge-queue entry and rebuilds everything behind it.
- **Bolder:** nextest retries once only tests whose package closure the candidate did not change, so
  their inputs equal main's, where they passed. Each such pass is reported in a non-blocking `Flaky`
  check and counted against C7. Tests whose closure changed get no retry.
- **Numbers:** [E] at the phase-1 budget of 3% false failures per run, about 0.1% of merge groups fail
  falsely. Each avoided ejection saves the re-enqueue, the rebuild of entries behind it and 45 s of
  GitHub latency.
- **Confidence:** a test with the same closure as main cannot fail because of the change. This is
  today's "re-run the failed job", minus the hour and plus the record.
- **Experiment:** replay S1's 10 failures against the closures of their PRs.

**T8. Bold, for the owner: a Velnor merge train in place of GitHub's queue.**
- **Basis:** GitHub's queue adds 19 s + 26 s per merge [M], plus auto-merge latency, and caps how many
  entries build at once.
- **Mechanism:** Velnor serialises landings by fast-forwarding `main` to the exact commit whose
  obligations hold. A Git fast-forward is a compare-and-swap, so every tip is verified by construction,
  which is stronger than HEADGREEN. The ruleset swaps its merge-queue rule for the App's `Required` on
  the pushed SHA.
- **Numbers:** [E] about 45 s less per merge.
- **Risk:** squashed PRs show as closed unless the train lands them through the merge API, and the
  queue UI goes away. A Velnor outage stops merges, so the fallback is to re-enable the queue rule.
- **Experiment:** 100 lab merges against measured queue latency.

## 4. The hardest problems

**P1. Verify each landed tree exactly once, so the merge group becomes a lookup.**
- **Attack, in order of yield:**
  1. Shrink the window: verdict ≤ 1–2.5 min, auto-merge, and the App verdict (T2). That gives 83–89% exact-tree hits (T1).
  2. Use per-root, then per-package, keys that cover every obligation of the tree, so a full merge group
     finds receipts without base-relative skipping (T3, H1, H2).
  3. Write receipts per obligation as each finishes, and add in-flight join, so restarts, preflights
     and the merge group never run the same key twice.
  4. Keep speculation (M5) for the residue. When `main` moves under a green, enqueued PR, verify
     `main' ⊕ PR` at batch priority; the replay says that is about 10–15% of merges, not all of them.
- **Target:** ≥ 90% of merge groups execute nothing in phase 3, ≥ 97% in phase 4, and ≤ 1 runner-min
  per merge group.

**P2. The runner root's serial floor.** It is touched by 55% of merges, carries the heaviest compile
and the longest tests, runs two configurations and needs Docker.
- **Tests:** split the loops (M7), clone a journal template, and measure dependency `opt-level`
  honestly (H5). The 6-case `shape.rs` test took 124 s wall and 108 s of CPU, about 20 s per case, mostly
  in turso's SQL parsing and allocation (my M5 run, `work/bench/slow.83596.log`); the target is ≤ 15 s per case.
- **Compile:** incremental state in PR scopes plus the I073 split (T6), taking a one-file edit from 61 s to 10–20 s.
- **Docker:** the Docker binaries become their own obligation on the same node (H4).
- **Configurations:** default and `local-mode` run as two pods cloned from one snapshot.
- **Hardware:** a high-clock node for the runner class only after the four steps above, since what
  remains is single-threaded codegen and the longest test.
- **Numbers:** [E] from a 3.7–8.8 min job (§A T6) to a verdict P50 ≤ 3 min in phase 3: start 3 s + build
  20 s + tests about 60 s with 16 threads and cases ≤ 15 s + lanes. ≤ 90 s in phase 4.

**P3. CI that starts before the push.** The head start is measured: 42 s p50 and 320 s p90 from final
commit to first CI run.
- **Attack:** a post-commit hook pushes to the Velnor mirror and preflights `main ⊕ commit`. The PR run
  joins the in-flight work or finds receipts (T5).
- **Next step:** agent workspaces on the cluster (N08). The agent's own `cargo nextest` then runs as a
  Velnor-observed obligation on linux/amd64, so the agent's own run is the verification and no
  separate CI execution is needed.
- **Target:** verdict ≤ 20 s after the push for preflighted PRs in phase 4, not phase 5.

**P4. Zero queue on a shared, bursty cluster.**
- **Facts:** workers were sampled at 92–95% CPU. The seven hottest pods, at 70–84 cores each, were all
  velnor-new cold builds (§B.2.2). There were 2,558 PR runs in 15 days, 45% of them cancelled (§A T7).
- **Attack:**
  1. Cut demand where it starts. Warm snapshots remove cold compiles (696 CPU-s for a cold runner
     workspace), same-SHA restarts disappear (18.3% of PR runs, H3), cancelled work is kept as
     receipts, and merge groups become lookups. Runner-minutes per merged PR fall from 285–347 to ≤ 20 [E].
  2. Reserve rather than preempt: a fixed merge and gate slice per node (one 16-CPU merge pod, two
     1-CPU gate pods) that PR work never takes. PR work bursts into the rest, and batch work is preempted.
  3. Set CPU requests per class so a cold build cannot starve a neighbour's tests that have tight timeouts.
  4. Buy capacity only on measured Velnor-side wait.
- **Target:** merge and gate wait p90 ≤ 2 s and PR job wait p90 ≤ 10 s in phase 3.

**P5. Flake-free required checks.**
- **Attack:**
  1. Closure-scoped retries (T7).
  2. Burn-in: each new or changed-closure test runs 20× in parallel on spare threads in the PR run. A
     10%-flaky new test is caught before merge with probability 1 − 0.9²⁰ = 88%, for seconds of work,
     because the changed set is small.
  3. Size timeouts from the pod's CPUs instead of the fixed 2 s kill (`ci-helper/.config/nextest.toml`,
     `slow-timeout` 2 s with `terminate-after = 1`).
  4. Keep the draft's paired sampler and quarantine lane.
- **Target:** false failures per merge-group run ≤ 0.5% in phase 3 and ≤ 0.1% in phase 4, from 8.1% today.

## 5. Decisions I would overturn

| Id | Draft | Instead | Evidence |
|---|---|---|---|
| I054 | Owner decision; recommends merge-group selection in phase 3 | No, as specified. Full merge groups served by receipts (T3); any selection only under ALLGREEN | H1: HEADGREEN (S3); `event.rs:43-44`; `hard-decisions.md:208-210` |
| (e), M2 | "Exact trees alone do not get there" | They do once the window is short: 83–89% of merge groups | T1 replay of 98 arrivals |
| I013 | App posts `Required` in phase 4; plan stays a job | Phase 2; each root job plans itself | H3 (60/626 s stall); T2 |
| N01 | Ledger in phase 3 | Phase 2: it needs no dispatcher, and work item 10 depends only on items 3 and 8 | §11 |
| N02, I041 | No cross-tree reuse before digest closures in phase 4 | Per-root keys in phase 2, per-package keys in phase 3 | T3: 500 → 125 root-jobs |
| N07, N09 | Preflight in phase 5; push-webhook start in phase 4 | Post-commit preflight with in-flight join in phase 3 | T5: 42 s p50, 320 s p90 head start |
| N14 | Retries stay at 0 | One retry for tests whose closure the change did not touch, reported | T7 |
| I064 | Owner decision quoted at −66% | Re-measure with assertions on, then decide | H5 |
| I084, §3.6 | Docker class 8+4 CPU, one node per site | Docker binaries as their own obligation; rootless Docker (I085) in phase 2 | H4 |
| I073 | Split `velnor-runner-host` only if an experiment passes | Adopt: one 157k-line crate is the unit of rebuild and of receipts | T6: 61 s per one-file edit |
| I015, I079 | Priority by acquisition order behind a capacity header | A `velnor-merge` scale set from phase 0; test offer semantics on the lab | H9 |
| I030, I044 | Warmer rebuilds `main` on all 5 nodes after every merge, uncounted | Promote the landed tree's own post-build snapshot on landing, as receipts are; warm other nodes lazily | [E] 5 nodes × 0.5–4 min per merge ≈ 2.5–20 runner-min per merge |
| Triggers | `edited` kept, in velnor-gen too | Drop it; the App re-plans on a base retarget | H3: 18.3% of PR runs restart on an unchanged SHA |

## 6. Targets

Committed · stretch. P50 and P90 are over all merged PRs; the "median class" is PRs touching only
`generator/` or `ci-helper/` (47%). Cache hit is compile units fresh on PRs without a lockfile or
toolchain change; MG is the share of merge-group obligations held by receipts.

| Phase | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Cache hit |
|---|---|---|---|---|
| 0, week 1 | 10 / 20 min · 8 / 15 | 20 / 45 min · 15 / 35 | ≤ 270 · 240 | tools: none yet |
| 1, weeks 2–3 | 6 / 12 min · 4 / 9 | 12 / 30 min · 9 / 20 | ≤ 140 · 110 | toolchain, tools, actions ≥ 99% |
| 2, weeks 3–6 | 2 / 6 min (median class ≤ 75 s) · 1.25 / 4 | 4 / 12 min · 3 / 8 | ≤ 35 · 25 | compile ≥ 97% · 99%; MG ≥ 75% · 85% |
| 3, weeks 6–12 | 1.5 / 4 min (median class ≤ 60 s) · 1 / 2.5 (≤ 35 s) | 2.5 / 8 min · 1.75 / 5 | ≤ 20 · 12 | compile ≥ 98% · 99.5%; MG ≥ 90% · 95% |
| 4, weeks 12–20 | 1 / 2.5 min · 0.5 / 1.5; ≤ 20 s after a preflighted push | 1.75 / 5 min · 1.25 / 3 | ≤ 10 · 6 | MG ≥ 97% · 99% |
| 5, merge train (owner) | as phase 4 | stretch 0.75 / 2 min | as phase 4 | as phase 4 |

Arithmetic:
- **Phase 0:** today's zero-queue run is about 11 min. Gates move to the cluster, `edited` restarts
  stop, and the `-j2` cap goes (`maintenance-helpers` is the last job in 102 of 154 runs). Push-to-merge
  is then about 9.5 min of PR verdict + 0.5 min to enqueue + 9.5 min of full merge group + 0.5 min ≈ 20.
- **Phase 2:** push-to-merge ≈ V of 2 min + auto-merge 10 s + 19 s + 5 s of receipt verdict + 26 s ≈
  3 min, for the 85–90% of merges whose merge group executes nothing at V = 2 min (T1, T3). The rest re-run one root,
  about 3 min more.
- **Phase 3, verdict:** the 50th percentile falls just past the median class (≤ 60 s) into
  root-engine PRs (8.5%, about 1.5–2 min), so the all-PR P50 is 1.5 min.
- **Phase 3, runner-minutes:** 4.77 PR runs per merged PR (Fable) less 18% same-SHA restarts ≈ 3.9 runs.
  3.9 runs × 3.5 runner-min (median class 1.3, runner class about 7) ≈ 14, plus merge group ≤ 1, release
  duties 2 and snapshot promotion 2 ≈ 19.
- **False failures per merge-group run:** ≤ 3% in phase 1, ≤ 0.5% in phase 3 (T7, P5), ≤ 0.1% in phase 4.
  Velnor-side wait p90 for merge and gate jobs is ≤ 2 s from phase 3.

## 7. Anything missing

- **Pre-job admission is a stage of its own (H3), and the metric hides it.** The draft's clock starts
  at the first CI run on the final head, so it misses both this stall and any gain from preflight.
  Start push-to-merge at the final commit's creation, and report admission (run created → first job
  created) as its own span in work item 3.
- **In-flight join and streamed receipts.** Receipts help only after an execution finishes. Preflight,
  speculation, `edited` restarts and cancel-then-restart on the same tree all need single-flight per
  key, with receipts written per obligation as soon as each passes.
- **The ledger now authorises merges, and nobody designed its trust root.**
  - Each node's signing key is held by the agent, outside every pod, with rotation and revocation on node compromise.
  - Receipts go into an append-only log.
  - The App verifies signatures and the current key set before posting `Required`.
  - Audit re-executions run on a different node from the one that produced the receipt.
  - If the App is down, nothing bound to it can merge, so the degraded mode must be written down: who
    re-binds the check, and how.
- **The root-workspace restructure removes the cheap boundaries.** The move to one root workspace
  (#1290/#1350, I057) deletes the root boundaries that make per-root keys and sparse-checkout
  enforcement trivial. Per-package keys must exist before it lands, or cross-tree reuse falls back to
  exact trees.
- **Agent procedure sits on the critical path and belongs in tooling, not prose.** It shows up as
  `edited` churn, a 42 s commit-to-push lag, pushes 41.7 min apart at p50, and auto-merge used by 0 of
  40 PRs. A `velnor pr` wrapper should open the PR with auto-merge on and install the post-commit hook.
- **GitHub's own latency becomes the largest term.** Below a 1-min verdict, the measured 19 s and 26 s
  plus the unmeasured auto-merge → enqueue delay make up most of push-to-merge. Measure that delay now
  with a 20-PR auto-merge trial (`AutoMergeEnabledEvent` → `AddedToMergeQueueEvent`); that is what
  decides T8.
- **Intermediate commits under HEADGREEN.** Release tooling and bisection must skip commits whose own
  merge-group run failed (H1).
- **Push runs on `main` queue behind each other today.** Pre-job p90 is 870 s and the maximum 6,402 s
  (`work/fb/prejob.py`), because the shared `refs/heads/main` group never cancels in progress.
  velnor-gen's per-run group removes this. Keep it for release duties unless releases must be ordered.

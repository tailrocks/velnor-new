# Is this the fastest runner? Critique by Muse Wonder

| Field | Value |
|---|---|
| Model | Muse Wonder |
| Model id | `muse-wonder-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Velnor: is it the fastest possible CI runner for velnor-new?

Review of the [final design](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md),
the [evidence pack](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/evidence.md),
[decisions](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/decisions.md),
[delivery plan](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/delivery-plan.md),
[owner decisions](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md),
[rust-only rulings](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md)
and the four experiments. Tags follow the design: [M] measured, [E] estimated, [T] target.

## Verdict

Not justified as stated. Under its binding constraints (GitHub platform/queue kept, zero false
greens, two witnesses, no reuse without a proven key), Velnor is a credible candidate for the
fastest *correct* design yet written for this repo — but "fastest runner ever" is unproven: no
end-to-end Velnor run exists, and no head-to-head on the same SHAs against GitHub-hosted larger
runners, ARC on warm bare metal, Namespace/Depot/Blacksmith-style runners, RunsOn/Ubicloud fleets,
a Buildkite-style agent fleet, or Bazel/RBE remote execution was run. Warm official-runner fleets
or RBE could plausibly beat it on wall time alone, at lower confidence. Claim only "fastest at
this confidence, on this repo, once §5 below passes" — nothing stronger is supported.

## Where the time goes, per stage

Design §3, one typical PR (the 47% `generator/`/`ci-helper/` class), labelled. "Goal" is [T]
with the design's expected value in brackets; only the Today column is broadly [M].

| Stage | Today | Goal [T] (expected) | Basis / floor |
|---|---|---|---|
| 1. Event, webhook, offer | push→run 1–2 s; run→job 2 s p50 / 226 s p90; hosted queue 5.5 min med [M] | webhook 1–2 s; reconciler ≤10 s | [M] offer ≈5 s queued / 8 s running (ruling V5); floor is GitHub webhook + App write, ~2 s |
| 2. Acquire (Actions jobs only) | 13 s p50 / 27 s p90, n=1,124 [M] | off verdict path | Obligations bypass Actions; release/image jobs still pay JIT acquire |
| 3. Sandbox start | ARC pod ~6 s (436 starts) [M] + dind probe | ≤0.3 s | Warm pool claim [E]; reflink clone 0.25 s is [M], pod claim itself unmeasured |
| 4. Checkout | 6 s/job × 31–39 jobs [M] | ≤1 s | Mirror + reflink [E]; whole-tree hash 0.1–0.3 s [E: SHA-1 ~1 GB/s/core] |
| 5. Toolchain/tools | 10 s med / 18 s p90 + mise 2 s + downloads 4.4 s/job [M] | ≈0 s | Read-only store [E]; plausible, stores are proven tech |
| 6. Dependency fetch | 3.5–6.1 s/job; sources cache 0/48 hits [M] | 0 s (~3 s on lockfile change) | Site mirror [E] |
| 7. Compile | 196–497 crates/job; 81 s runner-host vs 14 s velnor-ci; 8.5 min critical path [M] | ≤25 s (15 s) | Warm generation: 0 crates / 0.31 s unchanged [M, idle]; changed-closure time [E] |
| 8. Tests | velnor-ci 10/62 s; runner-host 192/417 s; floor 125–138 s [M] | ≤60 s (10 s) | Per-root keys + parallel twins [E]; floor is §8.5's un-done test programme |
| 9. Reporting | 1.48 GB upload 36–43 s; `Required verdict` 49 s [M] | ≤1 s | Streaming results [E] |
| 10. Required check | hosted queue 1.6/24.3 min + `Required` 1.9/21.4 min [M] | ≤1 s after last receipt | One App status; floor is one GitHub API write + read-back |
| 11. Merge-queue entry + run | green→enqueue 7.5 min (0/40 auto-merge) [M]; group 19.9/84.1 min [M] | ≤30 s (≈25 s) | Exact-tree hit 60–75% [E replay]; else per-obligation delta |
| 12. Merge | 26 s [M] | 26 s | GitHub's queue (decision 7); irreducible while GitHub is the platform |
| **Total** | **PR 26.7/92.3 min; push-to-merge 85/457 min [M, proxy]** | **PR ≤90 s/≤4 min; merge ≤3/≤8 min (≈35 s / ≈85 s expected)** | Goal row is [T], expected column is [E] |

Three honesty notes on this table.

First, the [M] baseline is itself a proxy: push-to-merge is timed from the first observable CI
run at the final SHA, not a push timestamp (design §1.1), over 98 merged PRs; the worst-day
57.0-of-71.8 min hosted queue (evidence T5, 2026-11-18) is one loaded day, not a steady state.
The design says so; any "fastest" comparison must reuse one cohort and one clock for all sides.

Second, the goal bounds do not sum to the goal. Adding the ≤ bounds for stages 1, 3, 4, 7, 8,
9, 10, 11, 12 gives ~145 s, above the 90 s PR-verdict P50. The 90 s is reachable only at the
*expected* values (compile 15 s, tests 10 s), i.e. only when per-root keys shrink the changed
obligation set to ~10 s of tests. That enrolment is the load-bearing assumption, not a detail.

Third, the true floor, with GitHub kept, is not near zero. Group formation 19 s median + merge
26 s [M] is 45 s of GitHub time on every landing unless a speculative record already covers
the group tree; even a zero-time PR verdict floors push-to-merge at ~47–50 s. For a changed-
code PR the floor is the longest single obligation chain after sharding, times the slowest
witness, plus stages 1+3+4+10 (~4 s). Today that chain is 125–138 s behind the 258–274 s looped
tests, with single suites at 679 s (`non_inputs_differential`) and 924.8 s (receipt integrity)
in the runner-host class. Until §8.5's floor programme is measured, P90 ≤4 min is arithmetically
impossible for that class (25% of PRs), however fast the runner around it is. The design
acknowledges this by dating the goal after the floor fix; the "fastest ever" slogan does not.

What is genuinely measured and strong: reflink clone 0.25 s / 2.5 MB and 0 crates in 0.31 s on
an unchanged tree; cold plain-cargo build 92 s for 357 crates on an idle 96-thread worker
([experiments/mbx-cache-cluster.md](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/mbx-cache-cluster.md));
mbx seeding a cold node in 39.9 s vs 92 s cold; hosted-queue dominance (T4/T5); 45% cancelled
PR runs and three full runs per merge (T1/T7). What is estimated: every goal-stage number,
the 60–75% exact-tree group hit rate (entry replay, ruling Q11), runner-minutes 110→37 per
merged PR (§4, warm 16 / cold 22 min per full tree are [E]), and all under-load behaviour —
every build experiment ran on nearly idle nodes, while evidence §B measured workers at
92–95% CPU on 2026-11-18. No joint trace under load exists yet, by the design's own §1 rule.

## The strongest objections

1. **The claim has no comparator run.** Evidence: §5.3 admits the Native-vs-Official speed
   difference is "unmeasured"; nothing in the pack runs Namespace, Depot, Blacksmith, RunsOn,
   Ubicloud, Buildkite agents, GitHub larger hosted runners, or any RBE (EngFlow/NativeLink/
   bazel-remote) on this repo. Likelihood this voids "fastest ever": certain as a matter of
   logic — an unraced claim. Refute by: §5 benchmark 1 on identical SHAs/cohorts; Velnor must
   win push-to-merge P50/P90, not a stage micro-benchmark.
2. **The test floor, not the runner, sets P90.** Evidence: 679 s and 924.8 s single suites,
   125–138 s floor, 356 test binaries, `test-threads=1` (§8.5, [M] from 28 job logs). The fixes
   (one test per loop, private temp dirs/ports, sharding, 340 fewer link passes) are unbuilt
   and some change test semantics — shared ports/dirs are exactly why threads were pinned.
   Likelihood: high that at least one long suite resists splitting. Measurement: per-binary
   wall/CPU inventory on a worker, then post-fix longest-binary wall; pass if no receipt-minting
   binary exceeds 60 s (goal) on the merge-gate corpus.
3. **Per-root keys may enrol almost nothing.** Evidence: 79/86 files using `CARGO_MANIFEST_DIR`
   climb out of their package and 70 test files list directories (ruling Q5), so "most test
   keys stay whole-tree" (§6.1). Yet the 90 s goal needs tests at ~10 s expected, which needs
   enrolled per-root keys. These two statements are in tension the design does not resolve
   numerically. Likelihood: high for test obligations, lower for check/clippy/doc lanes.
   Measurement: shadow read-audit enrolment census — share of obligations (and of test wall
   time) that enrol per-root; goal survives only if ≥80% of typical-PR test wall enrols.
4. **Compile-reuse soundness is asserted, not yet earned.** Evidence: the older-mtime false
   green is reproduced (ruling Q16); the defence is a stack of mechanisms (content-proven
   generation manifest, post-run dep-info audit, cold second witness, A1 digests, hourly
   canaries) none of which has run at scale; graduation needs ~3,000 clean comparisons per
   Cargo root/kind (§6.4–6.5). Until then a cold witness (22 [E] runner-min/full tree) caps
   both speed and runner-minutes at the ≈6/18 min row, not the goal row. Likelihood: medium
   that one unit class (build scripts, proc macros, `OUT_DIR`) stays cold for months.
   Measurement: Lab B2 canaries + digest-equality rate over the first 3,000 comparisons,
   reported per unit class, with zero unexplained mismatches tolerated.
5. **Capacity: the design doubles work on a saturated cluster.** Evidence: two witnesses per
   new key + A1 third execution + 2% sampling, on 5×96-thread workers measured at 92–95% CPU,
   shared with ChainArgos (60) and jackin (40) scale sets; reflink clones do not share page
   cache (14.29 GB per read clone [M]), capping ~4–6 build sandboxes/node; the reserved merge
   path is ~7.7% of CPU. At 95% utilisation queueing delay grows ~19× (design §9's own figure).
   Likelihood: high that P90 under real load misses 4 min even if P50 hits 90 s. Measurement:
   replay a 2026-11-18-load day against the built system; pass if PR verdict P90 ≤4 min and
   merge-path wait p90 ≤10 s at that load, twins included in runner-minutes.
6. **Keeping GitHub's queue caps the ceiling.** Evidence: decisions 7/12 keep GitHub merge
   queue, ruleset and status as the gate; 19 s + 26 s [M] medians are GitHub's, depth 2
   serialises groups, and ALLGREEN ejects rebuild everything behind a failure. A system free
   to run its own speculative merge train (Buildkite-style, or RBE-backed) could pipeline
   deeper. Likelihood: certain as a floor; acceptable only because the owner chose GitHub.
   Measurement: none can refute it within the constraints — so the claim must be scoped
   "fastest on GitHub's queue", or the constraint must be re-opened explicitly.
7. **Twin independence is assumed.** Evidence: ruling Q12/Lab C2 — today's two executions are
   hours apart; Velnor's are simultaneous on two nodes with (proposed, unbuilt) seeded order
   shuffling (§6.2 admits nextest has no shuffle configured and none is claimed yet). Correlated
   flakes (same seed, same minute, same fixture state) would silently halve detection while
   doubling false-red cost: at 8.1% [M] false failures, two witnesses already fail falsely
   ~15.5% of the time vs the 3%/1% budget (§1.3). Likelihood: medium. Measurement: Lab C2 as
   specified (20 tests, seeded p=0.05–0.2, joint pass vs (1−p)²); if correlated, groups re-run
   changed obligations and the ≈25 s group estimate grows by the full delta.
8. **Storage and transport are the least-proven layer.** Evidence: RustFS 1.0.0 is two months
   old with open node-return/replication issues (#9480/#8768/#9161); all RustFS numbers are
   one loaded Mac, single-node ([experiments/rustfs.md](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/rustfs.md));
   mbx lost to plain cargo on a same-node lockfile change (88.4 s vs 67.4 s [M]) and uploaded
   6.8 GiB for an 11-crate change. The design correctly keeps transports off the verdict path
   until ST06/Lab B3 pass — but then cold-node verdicts pay the 92 s cold build, and the
   "magically fast cache" contributes nothing to the headline number. Likelihood: medium that
   L1 stays `local`-only well past day 3. Measurement: ST06 cluster test + Lab B3 crossover
   under load; pass if a transport beats cold on >10% of requests at equal provenance.
9. **A Rust reimplementation of GitHub Actions is a cost centre, not a speed feature.**
   Evidence: `velnor-exec` budgeted 20k lines, 240+65+30 conformance cases, 95% mutant kill,
   per-release requalification within 14 days, forever (§5.7); velnor-legacy's god files are
   37k lines each and 60/100 recent commits were fixes. The speed it buys over driving the
   official runner in the same warm pools is unmeasured (§5.3) and plausibly small: Official's
   known costs are 4–5 s/job Worker setup [M, ARC era] + 87–147 ms per JS action [M], while
   Native must still run the same cargo/test work that dominates. Likelihood: medium-high that
   ≥80% of the wall-time win is available from label flow + ledger + warm generations under
   the Official executor alone, at a fraction of the 820 agent-hour plan. Measurement: paired
   Native/Official runs per job type on identical backend/store/generation (§5.7's own gate),
   published before cohort 2; if Native's edge is <10% of verdict wall, the claim belongs to
   the ledger design, not the executor, and sequencing should flip.
10. **Delivery arithmetic assumes the thing being replaced.** Evidence: 820 building + 120
    review agent-hours ≈2.8 fleet-days, but the plan's own §1 says dates are set by velnor-new's
    current 94-min agent PR cycle, reviews, and evidence traffic (3,000-comparison graduations
    and 300-merge shadow runs take calendar time no fleet size shortens). Likelihood: high that
    the goal row lands weeks after day 10–13.5. This does not refute the design, only the date.
    Measurement: phase-0's predicted 94→34 min PR cycle, checked on the first 20 agent PRs.

## Missed speed

Concrete gains left on the table, at the same confidence (keys, witnesses, A1 unchanged).
All figures [E] from design-measured inputs; none counted in the design's totals.

| Gain | Mechanism | Estimated saving |
|---|---|---|
| Speculate on every push, not just queue prefixes | While a PR is draft/unlabelled, verify `merge-tree(queue tip, head)` at batch priority on each push; label then joins in-flight records (§6.2 already allows joins) | Group-stage ≈25 s and most PR compile/test wall hidden before the label; push-to-merge approaches the ~50 s GitHub floor for the 60–75% hit class |
| Test-case sharding across nodes | Split each long binary's test IDs (already inventoried per obligation, §6.1) across 2–4 nodes, longest-first; receipt binds shard layout (§6.2 provides for this, default off) | 679 s suite → ~180–250 s at 4 shards incl. binary transfer; makes the 60 s/binary goal reachable without rewriting tests |
| Remote per-unit compile for cold nodes | Graduate mbx/`mbx-cache` (or an RBE-style CAS for cargo units) onto the verdict path where Lab B3 proves it wins | 92 s → 39.9 s [M] cold-node build; ~50 s on every generation-miss verdict |
| Buy the high-clock node now, not "after floor fixes" | §9 trials 1.3–1.5× serial [E] later; rustc, link and single-threaded tests are serial-bound today | 15–40 s on a typical verdict's compile+longest-test chain, immediately |
| Faster link path | `mold`/`lld` + split-binaries work (§8.5's 356→~16 binaries) measured as its own item, not bundled into "floor fixed" | Link is inside the 81 s build step [M]; even 20% off build wall is ~5 s/verdict and ~16 runner-min/PR at whole-tree keys |
| Namespace slots instead of pods on Kubernetes | Node agent carves cgroup/namespace slots (panel idea I082/F-B13, experiment-gated in decisions, not in the design's critical path) | Sandbox 0.3 s → ~0.02–0.05 s and, more valuable, page-cache sharing pressure per slot falls; small per verdict, real at 10,000 jobs/day |
| Deeper speculative queue pipelining | Verify ordered queue prefixes for depth 4 before groups form (design speculates, but sizes depth 2 until verdicts are ≤5 min) | At 2.44 entries/h [M] arrivals are sparse; speculation is nearly free and converts group runs into lookups |

Two non-gains, for the record: dropping a witness or trusting PR-scope receipts would be
faster and is correctly refused (decision 11); and blaze is correctly rejected — it built
none of the five workspaces and produced a clippy false green
([experiments/blaze.md](/Users/donbeave/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/experiments/blaze.md), [M]).

## What to measure first to earn the "fastest" claim

In order; each with its pass bar. Until 1–3 pass, publish no comparative claim at all.

1. **Same-cohort end-to-end race.** Replay ≥100 merged PRs (event-level, identical SHAs and
   arrival times) through (a) today's ARC+hosted path, (b) Velnor as built, (c) the strongest
   runnable alternative — at minimum the Official executor in Velnor's own warm pools, plus
   one external fast-runner (Namespace or Depot class) on the same workflows. Pass: Velnor
   push-to-merge P50 and P90 lowest, with runner-minutes and false-red counts reported per
   side; ≥5 interleaved repetitions per side, median and range, errors counted, outputs
   diffed (a fast side that skipped work is disqualified, not faster).
2. **Loaded stage trace.** One full day at 2026-11-18 load (workers ≥90% CPU, all three
   tenants), every stage of §3 timestamped. Pass: PR verdict ≤90 s P50 / ≤4 min P90 and
   push-to-merge ≤3/≤8 min *in that trace*, not on idle nodes; every [E] stage number above
   replaced by its measured value or the goal row restated.
3. **Floor inventory.** Per-binary wall/CPU for every receipt-minting test binary on a worker.
   Pass: longest binary ≤60 s and the typical-PR critical chain (compile + longest binary,
   slowest witness) ≤50 s; list any binary exempted and its PR-class share.
4. **Enrolment census (objection 3).** Shadow read-audit over ≥300 merges. Pass: ≥80% of
   typical-PR test wall time enrolled per-root with zero contract violations; otherwise the
   90 s goal is withdrawn and re-derived from whole-tree numbers.
5. **Reuse graduation evidence.** Lab B2 canaries (incl. planted older-mtime edit) and the
   ~3,000 warm-vs-cold digest comparisons per root. Pass: zero unexplained mismatches, every
   canary rebuilds and fails as planted; publish the per-class cold holdouts that remain.
6. **Twin independence (Lab C2) and group hit rate (Lab C1)** over ≥300 merges. Pass: joint
   pass rates within interval of (1−p)², and measured exact-tree/per-obligation hit rates
   replacing the 60–75% replay estimate in §3 stage 11.
7. **Cold-node crossover (Lab B3/ST06).** Pass: transport (mbx/file-delta/layers) beats cold
   build on >10% of real requests under load, RustFS cluster survives the #9480 node-return
   sequence; until then all claims are stated for warm-node hits only.

If 1–7 pass, the earned sentence is: "fastest verified push-to-merge measured for
tailrocks/velnor-new at zero false greens, against [the named sides], on [the cohort]."
That is a strong, true claim. "Fastest runner ever" is not earnable by any finite
measurement, and saying it before benchmark 1 exists would be marketing, not engineering.

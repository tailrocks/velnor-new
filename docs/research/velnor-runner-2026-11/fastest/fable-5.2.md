# Is this the fastest runner? Critique by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Is Velnor "the fastest runner ever"? Review by Claude Fable 5.2

Reviewed: `docs/proposed/velnor-runner-architecture.md` (v3) and the research folder
(`decisions.md`, `delivery-plan.md`, `owner-decisions.md`, `evidence.md`, `experiments/`,
`rust-only-rulings.md`). Tags follow the design: **[M]** measured, **[E]** estimated, **[T]** target.
"Mine" marks a figure from my own notes (`panel/fable-5.2/notes.md`) or from reading velnor-new at
`5ae93781e0`. Nothing here was run against GitHub or the cluster for this review.

## Verdict

1. **Not justified as worded.** Nothing of Velnor has run end to end; every figure below the ≈ 6 / 18 min interim row is [E] or [T], so "fastest runner ever" is a design goal today, not a result.
2. **Against GitHub-hosted runners and ARC, for this repository: yes, with high confidence.** The gain is measured waste removed (57 of 71.8 min queueing, about 49 s of fixed overhead × 31–39 jobs, three full runs per merge), and most of it arrives in phase 0, before any Velnor code runs.
3. **Against a Buildkite-style persistent agent on the same workers: a tie on compile and tests** (same cargo, same CPUs, a warm target either way); Velnor leads only through record reuse.
4. **Against Namespace-, Blacksmith- or Depot-style fast runners: ahead on overhead and reuse, behind per thread.** The design's own estimate is 1.3–1.5× [E] for a high-clock node on the serial floor that sets P90; no measurement against any of them exists.
5. **Against Bazel or Buck2 with remote execution: not shown.** Their per-action hermetic cache gives by construction what Velnor's per-root and per-package keys must earn through enrolment.
6. **Supportable claim:** "designed to be the fastest GitHub-Actions-compatible, cargo-native verification path for velnor-new", to be proven by the benchmarks in the last section.

## Where the time goes, per stage

The typical PR of section 3 (the 47% class that touches only `generator/` or `ci-helper/`), at the
goal. "Floor" is what remains on today's EPYC 9454P workers with GitHub as the platform.

| Stage | Goal [T] (estimate) | What is measured, what is estimated | True floor |
|---|---|---|---|
| 1. Event reaches Velnor | 1–2 s | [E]. No webhook delivery time is in the pack. The nearest [M] is ref update → run created: 3 s p50, 5 s p99, five outliers of 122–424 s (V5, n = 656) | GitHub's delivery, about 1–3 s. Only a pre-push `velnor verify` beats it |
| 1b. Fetch, `merge-tree`, plan, resolve | not in the table | Unbudgeted. Today: fetch 0.75 s, planner 1.6 s [M, mine, one run] | about 1 s with a resident planner; 2–3 s as designed |
| 2. Acquire | off the path | By construction: obligations are not Actions jobs | 0 |
| 3. Sandbox start | ≤ 0.3 s | Reflink clone 0.25 s [M], one clone, idle node. The warm pool and attaching a clone to a pre-created pod are unbuilt [T] | about 0 with pre-cloned sandboxes |
| 4. Checkout | ≤ 1 s | [T]. Tree hash 0.1–0.3 s is [E]. Nothing built | about 0.3 s |
| 5. Tools | ≈ 0 s | [T], sound by construction (read-only mount) | 0 |
| 6. Dependency fetch | 0 s; ≈ 3 s on a lockfile change | [T]. The real lockfile cost is in stage 7: 67.4 s [M] | 0 |
| 7. Compile | ≤ 25 s (15 s) | [E]. Measured: 0.31 s for a no-op, 92 s cold, 67.4 s lockfile change (runner root, idle). The 15 s rests on today's `velnor-ci` build step, 14 / 23 s [M] under ARC and mbx. **No clone-plus-real-edit build is measured** | One whole-crate rustc pass plus test targets and links: about 8–15 s here; ≥ 59 s for `velnor-runner-host` [M] |
| 8. Tests | ≤ 60 s (10 s) | `velnor-ci` 10 / 62 s [M] at `test-threads = 1`. Slow class: 192 / 417 s, floor 125–138 s [M] | Longest single test: about 3–5 s for the helper class in parallel; 125–138 s for the slow class until code changes |
| 9–10. Report, required status | ≤ 1 s each | [T]. One status POST | about 0.3 s |
| 11. Queue entry and group | ≤ 30 s (≈ 25 s) | 19 s enqueue → group [M]. Green → enqueue under auto-merge has **zero observations** (0 of 40 PRs used it). Exact-tree hit 60–75% is [E] from a replay; 16% [M] today | GitHub's, about 20–25 s |
| 12. Merge | 26 s | [M], median under load | GitHub's, 26 s |
| **PR verdict** | **≤ 90 s (≈ 35 s)** | [T] ([E]) | about 20–25 s on this hardware [E, mine] |
| **Push-to-merge** | **≤ 3 min (≈ 85 s)** | [T] ([E]) | about 70 s, of which about 45 s is GitHub's queue |

Three readings of the table:

- **The runner owns about 5 s of the 35 s.** Stages 1b, 3, 4, 5, 6, 9 and 10 sum to about 5 s. The
  other 30 s are GitHub's delivery, rustc and the tests. A runner cannot be "the fastest ever" on
  time it does not own; it can only have the lowest overhead, and a persistent agent has about the
  same.
- **Push-to-merge is more than half GitHub.** 19 s + 26 s [M] of the 85 s estimate is queue latency
  that decision 7 keeps. The stretch goal of 100 s leaves 55 s for everything else.
- **The measured rows are today's waste, not Velnor's speed.** The only Velnor-path measurements
  are P2 (clone 0.25 s, no-op build 0.31 s) and the RustFS and mbx experiments. All are single runs
  on idle nodes.

## The strongest objections

**1. The speed is the ledger's, not the runner's.** *Evidence:* section 8.1: "a PR and a merge group
have no Actions run"; section 5.3: the executors' speed difference is "unmeasured as a difference",
and Official adds 4–5 s of Worker set-up per job. Obligations run the same `velnor-ci obligation
exec` under either executor. So Velnor Native (about 30k budgeted lines plus the conformance
harness) buys about 5 s of wall on the verdict path. *Likelihood:* near certain. *Measurement:* the
paired run section 5.3 already promises; pass if Native beats Official by ≥ 10% of PR verdict P50.

**2. At provably equal confidence the number is 6 / 18 min, not 90 s.** *Evidence:* section 1.4:
whole-tree keys with a cold second witness give ≈ 6 / 18 min [E]. The goal needs per-root keys and
graduated compile reuse, each admitted after about 3,000 clean comparisons. Zero failures in 3,000
bounds the error rate below about 0.1% at 95%; it does not prove zero. A1 re-runs only *changed*
obligations (section 6.5), so a reused obligation that a missed input should have invalidated is
caught only by the read audit on executed runs and by 2% sampling. *Likelihood:* the gap is real by
construction; a false green from it is unlikely but not excluded. *Measurement:* 200 planted
regressions that act only through cross-root reads, stale times and environment; pass at 0 greens.

**3. P90 is set by the slow class, and its floor is serial.** *Evidence:* 25% of merged PRs touch
`velnor-runner-host` and 10.5% touch a `Cargo.lock` [M, mine, 200 PRs], so more than 10% of PRs sit
in a slow class. There, one crate compiles in 59 s [M] (lib-test in parallel), tests have a
125–138 s floor [M], and a lockfile change costs 67.4 s [M] before tests. That is about 3.5–4 min
with no queue, no flake and no Docker class [E, mine], against P90 ≤ 4 min. *Likelihood:* high until
the floor programme lands. *Measurement:* replay 100 runner-root and 30 lockfile PR heads; pass at
P90 ≤ 4 min under load.

**4. The median rests on a class mix that the project itself will change.** *Evidence:* 47% of PRs
are the fast class and 3.5% touch no Rust root [M, mine]: 50.5%, so the median PR sits half a point
inside the fast class. The design adds an `executor/` root of about 94k budgeted lines whose PRs
will dominate during and after delivery; its compile and test floor is unknown. Section 5.7 runs
the conformance corpus (335 cases × two executors × two backends) "on every executor change", and
the design does not say whether that is a merge obligation. If it is, executor PRs cannot reach
90 s. *Likelihood:* high. *Measurement:* recompute the class mix weekly; time conformance per PR.

**5. The warm-compile figure is extrapolated from a no-op.** *Evidence:* P2 compiled 0 crates.
Clones do not share page cache: each fully read clone holds 14.29 GB [M], so every new sandbox
reads the rlibs it links from NVMe (disk speed unmeasured; `fio` is still a phase-0 item). The base
generation also lags `main` by A1's SLO (2 h at P90), so Δ includes other merges, not only the PR's
change. *Likelihood:* medium; a few seconds on small roots, more on the runner root. *Measurement:*
lab B2 extended to 300 replayed real diffs per root, cold page cache; pass at P90 ≤ 25 s for the
helper and generator roots.

**6. Compile reuse may never graduate on some roots.** *Evidence:* graduation needs equal binary
digests, warm against cold, 3,000 times per root. The runner root builds C code through build
scripts (`aws-lc-sys`, turso) and 357 crates; one non-reproducible unit "keeps its dependants cold"
(section 7.3). No reproducibility run exists. *Likelihood:* medium (I would put one affected unit at
30–50%). *Measurement:* 20 cold builds on two nodes, digests of every test binary; pass at 100%.

**7. Two witnesses double the flake exposure, and flakes set P90.** *Evidence:* 8.1% false failures
per run [M] gives 15.5% for a pair (ruling Q12). Any failure blocks and retries are 0, so above 10%
the P90 verdict is two cycles. The 3% budget is a target; the causes named (CPU starvation, the
`event.json` race) are hypotheses. ChainArgos and jackin keep ARC pods that burst without CPU
limits. *Likelihood:* medium. *Measurement:* 300 rerun pairs under replayed 11-18 load; pass ≤ 3%.

**8. The latency budget assumes a free warm slot.** *Evidence:* four build sandboxes per worker, so
20 in the cluster; a whole-tree request fans out to lanes × roots × two witnesses. Workers measured
92–95% CPU on 11-18. Only merge work has a reserved slot. *Likelihood:* high in bursts, low on
average (goal-row load is about 6% of slot-hours [E, mine]). *Measurement:* the design's own bar,
joint traces under load; report queue residence as its own stage.

**9. The read audit has an unmeasured cost on the hot path.** *Evidence:* eBPF on the exits of six
file syscalls for every execution (section 6.4); one retained suite spawns about 45,000 `git`
processes. "Overhead is measured in shadow": not yet. *Likelihood:* low for the helper class,
medium for file-heavy suites. *Measurement:* suite wall with and without the probes; pass ≤ 3%.

**10. Per thread, the hardware is not the fastest available.** *Evidence:* EPYC 9454P workers; the
design's own 1.3–1.5× [E] for a high-clock node, deferred until "after the floor fixes". The serial
floor (one rustc front end, one long test) is exactly what such CPUs shorten and what fast-runner
vendors sell. *Likelihood:* high that a gap exists; its size is unmeasured. *Measurement:* the
runner-root cold build and the 20 slowest tests on one rented high-clock node.

## Missed speed

Further gains at the same correctness. All figures are [E, mine] unless tagged.

| # | Gain | Number | Status in the design |
|---|---|---|---|
| 1 | High-clock nodes for the verdict and merge classes, in phase 1 | 1.3–1.5× on stages 7–8 [E, design]: helper class 25 → about 18 s; slow class about 200 → 140–155 s | Deferred; an owner decision (I104) |
| 2 | Split `velnor-ci` (15.2k source and 25.7k test lines in one `it` binary) and `velnor-gen` (27.7k) along module lines | Any edit recompiles the whole crate and its test binary today: 14 s build + 10 s tests [M] → about 5 + 3 s for a typical edit. Per-package keys buy nothing for a one-package helper without this | Absent. The crate split named is the runner's |
| 3 | Promote pre-push `velnor verify` from the stretch (day 16–18) to straight after the ledger | The verdict becomes a lookup: about 35 s → 3–5 s on a hit. It is the only item that beats stages 1, 7 and 8 together | Stretch |
| 4 | A resident planner and pre-cloned sandboxes | Stage 1b 2–3 s → about 0.5 s; stage 3–4 about 1.3 s → 0.3 s. About 3 s of 35 (9%) | Not budgeted |
| 5 | After graduation, the second witness runs the first witness's digest-bound binary instead of compiling again | No wall gain; about −3 runner-min of the goal row's 12 per merged PR. It gives up the second compile as a cross-check, so it needs an owner call | Not considered |
| 6 | A shared read-only lower layer (overlayfs) for the generation | Removes the per-sandbox re-read (up to 14.29 GB [M] for a fully read target) and the six-sandbox cap | Lab B2 candidate |
| 7 | Changed tests first, and `cargo check` before the full build | Time to first red about 5 s instead of compile plus tests. No effect on green | Metric exists; no mechanism |
| 8 | A Velnor merge train in place of GitHub's queue | −45 s per merge [M: 19 + 26 s], 53% of the goal's 85 s | Blocked by decision 7 (N31) |
| 9 | Verify unlabelled pushes at idle priority into candidate records | The label then resolves by lookup | Blocked by decision 12 |

Items 8 and 9 are the two largest remaining gains, and both are owner decisions, not design gaps.
Not available at the same correctness, and rightly rejected: incremental compilation for receipts
(10.9 s against 61 s [M], Q17), test selection inside a binary, a different codegen backend, and
blaze.

## What to measure first to earn the "fastest" claim

Run these before the word is used. Each compares identical commits on matched cohorts.

1. **End-to-end fast path.** Replay 100 merged `generator/` and `ci-helper/` PR heads: label webhook
   → `Velnor / Verified`, every stage timed, stage 1b included. Pass: P50 ≤ 35 s and P90 ≤ 90 s idle;
   P50 ≤ 90 s and P90 ≤ 4 min under replayed 11-18 load.
2. **Head-to-head with reuse off** (pure runner speed, every obligation executed), 100 heads across
   all PR classes, cold and warm, lockfile changes included, against: GitHub-hosted `ubuntu-26.04`;
   today's ARC scale set; a persistent Buildkite-style agent on the same worker; one commercial fast
   runner (Namespace, Blacksmith or Depot) with its own cache. Pass: Velnor wins P50 and P90 in
   every class; publish every class it loses.
3. **Overhead per obligation.** Stages 1b, 3–6, 9 and 10 summed. Pass: ≤ 5 s at P90, and lower than
   the persistent agent's.
4. **Warm compile on real diffs.** 300 replayed diffs per root over a clone, cold page cache, with
   warm-against-cold digests on two nodes. Pass: helper and generator P90 ≤ 25 s; digests equal in
   100%.
5. **Serial floor by hardware.** The runner-root cold build and the 20 slowest tests on a 9454P and
   on one high-clock node. Pass: ratio < 1.15, or the claim waits for the faster nodes.
6. **GitHub's share.** 50 auto-merged PRs: `success` → enqueue → group → merge. Pass: P50 ≤ 55 s.
   This fixes the floor no runner can cut and removes the unobserved estimate in stage 11.
7. **Same confidence.** 200 planted regressions (cross-root reads, stale times, environment, a
   dropped test) and 300 rerun pairs under load. Pass: 0 greens; false failures ≤ 3%.

If 1–7 pass, "the fastest GitHub-Actions-compatible runner measured on velnor-new" is earned.
"Ever" needs benchmark 2 to include every competitor named, and even then it holds for this
repository only.

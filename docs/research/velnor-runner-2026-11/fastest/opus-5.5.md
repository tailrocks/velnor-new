# Is this the fastest runner? Critique by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Is Velnor "the fastest runner ever"? An independent review (Claude Opus 5.5)

Read: `docs/proposed/velnor-runner-architecture.md` (v3) and `docs/research/velnor-runner-2026-11/`
(decisions, delivery plan, owner decisions, evidence, experiments, rulings). I checked a few claims
against velnor-new `main` (5ae93781e0) read-only. Tags: **[M]** measured, **[E]** estimated,
**[T]** target, **[C]** read from code, **[mine]** my own estimate or count.

## Verdict

**Not justified as stated; plausible but unproven, and only for one PR class.** Against GitHub-hosted runners and today's ARC,
Velnor will almost certainly be much faster here: the baseline wastes most of its time (57 of a 71.8 min run queued on hosted
gate jobs, 0 of 48 cache restores, three full runs per merge, `-j2` caps). Against the strongest alternatives (Buildkite-style
agents with persistent workspaces, Namespace/Blacksmith-style runners with cache volumes, Bazel with remote execution) nothing
was measured. Velnor has a real structural edge on small changes (content-keyed verdict reuse, no Actions dispatch on the
verdict path) and probably loses to a fanned-out RBE on wide changes. Every goal number is [T], about 20% of landed commits
would fall back to whole-tree keys, and the executor the claim names ("runner") is not on the verdict path at all.

## Where the time goes, per stage

The design's own budget (section 3) covers the 47% class that touches only `generator/` or `ci-helper/`.
"Floor" is what physics and GitHub allow at the same contract, by my estimate unless tagged.

| Stage | Design goal (est.) | What is actually measured | True floor | Who owns it |
|---|---|---|---|---|
| 1. Webhook to Velnor | 1–2 s; reconciler 10 s | Run created about 3 s after the event, jobs 1–2 s later (ruling Q14) [M] | about 1 s | GitHub |
| 2. `ensure`: fetch, `merge-tree`, planner, ledger lookup | not budgeted | nothing | 0.3–1 s [mine]: one GitHub round trip plus a planner run | Velnor |
| 3. Sandbox from warm pool | ≤ 0.3 s [T] | reflink clone of a warm target 0.25 s [M]; pod start not measured | about 0.1 s | Velnor |
| 4. Checkout from mirror | ≤ 1 s [T]; full-tree hash 0.1–0.3 s [E] | nothing (only today's 6 s [M]) | about 0.1 s; the full hash is the floor | Velnor |
| 5–6. Tools, crates | 0 s [T] | today 10 s + 4.5 s per job [M] | 0 | Velnor |
| 7. Compile | ≤ 25 s (15 s) [T/E] | no-op over a clone 0.31 s; lockfile change 67.4 s; cold 92 s; all on an **idle** 96-thread node at 32 jobs [M]. **A typical source edit over a generation was never measured.** | Non-incremental rebuild of the changed crate plus dependents plus links. About 8–20 s for `velnor-ci` (40.6k LOC, one crate) [mine]. 59 s for one `velnor-runner-host` rebuild [M] | Velnor plus crate shape |
| 8. Tests | ≤ 60 s (10 s) [T] | `velnor-ci` 10 s / 62 s P50/P90 serial (`test-threads 1`) [M]; runner-host 192 / 417 s; floor 125–138 s plus 924.8 s and 679 s single suites [M] | the longest test in the closure. ci-helper kills at 2 s and generator at 2 s / 10 s, so about 2–10 s for this class; ≥ 125 s for the runner class until the code changes | **the repository's tests** |
| 9–10. Report, status | ≤ 1 s [T] | nothing | about 0.5 s (one REST write and a read-back) | Velnor |
| **PR verdict** | **≤ 90 s [T] (≈ 35 s [E])** | — | **≈ 15–35 s [mine] for this class** | |
| 11. Enqueue → group | ≤ 30 s (≈ 25 s) | 19 s median under load [M] | about 19 s; it is not a proven floor | GitHub |
| 11b. Group verification | exact-tree lookup for 60–75% [E, replay] | 16% equal trees today [M] | the C5 set plus misses (see objection 3) | Velnor |
| 12. Merge | 26 s [M] | 26 s [M] | 26 s | GitHub |
| **Push-to-merge** | **≤ 3 min [T] (≈ 85 s [E])** | — | **≈ 60–80 s; GitHub owns about 45 s of it** | |

What this shows:
- The stretch target (30 s verdict, 100 s push-to-merge) is close to the real floor for the helper and
  generator class. The design is well aimed there.
- About half of the best possible push-to-merge time belongs to GitHub (decision 7). No runner can
  improve that half, so a "fastest runner" claim should not take credit for it or blame for it.
- The two biggest measured costs today are hosted queueing (stages 1, 10, 11) and the code's test
  floor (stage 8). The first is removed by moving gate jobs off hosted runners, which phase 0 already
  does without Velnor (delivery plan: agent PR cycle 94 to about 34 min [E]). The second is removed by
  rewriting tests (section 8.5), not by the runner.
- Measured Velnor-specific speed facts: reflink 0.25 s, a no-op build 0.31 s, an mbx cross-node seed
  39.9 s, and a cold build 92 s on an idle node. That is all. Each one is a single run on idle nodes.

## The strongest objections

**1. The executor named in the claim is not on the verdict path.** Section 3 stage 2 says obligations are
not Actions jobs. Section 5.3 says Native vs Official speed is "unmeasured as a difference". Velnor Native
(about 30k lines with native actions) speeds up only release, image and scheduled jobs. Every PR-verdict
gain comes from the ledger, warm generations, mirrors and the label flow, which would work under either executor. *Likelihood:* certain; the design says so.
*Measure:* paired Native vs Official runs of release and image jobs on the same backend and generation.
Separately, attribute the PR-verdict gain by stage. Call the product "the fastest verifier", not the
fastest runner, unless the paired runs show a large difference.

**2. The headline covers one cohort, and the P90 is set by whole-tree keys.** The goal arithmetic covers
the 47% generator/ci-helper class, and the design says "the runner class costs more". Every per-root
contract includes `.github/`, `.velnor/`, `.config/`, `.cargo/`, and the root `Cargo.*`, `mise.*` and lint
files (section 6.4). On `main` since 2026-11-01, **42 of 214 first-parent commits (20%)** touched one of
those paths: `.github/` 29, `.velnor/` 23, `mise.toml` 6, `Cargo.toml`/`Cargo.lock` 4 each [mine, `git
log`]. Those heads fall back to whole-tree keys, so every obligation runs. The slowest obligation then
sets the verdict: runner-host tests at 192 / 417 s, the 679 s differential suite and the 924.8 s test,
until they are fixed or deleted. One PR in five above the 4-minute line makes a P90 ≤ 4 min unreachable.
*Likelihood:* high. *Measure:* the share of labelled heads with any whole-tree key, and per-class P90 on at
least 200 matched heads (the design's own N40 rule).

**3. As written, the C5 rule takes in the target class.** C5 obligations run on every request, and an
"instant" verdict needs zero of them (N120). An obligation is C5 when "its sources or workspace closure
read the clock" (section 6.1). In code:
- The `velnor-ci` library reads the wall clock (`ci-helper/crates/velnor-ci/src/io/mod.rs:210-216`
  `now_seconds()`, used for base expiry) and has an HTTP client (`src/io/http.rs`) [C].
- 12 files of its single `tests/it` binary read the clock [C].
- `velnor-gen`'s single `tests/it` binary uses `Instant` deadlines in three files [C].

Read literally, the `velnor-ci` test obligations are C5 forever, and possibly `velnor-gen`'s as well. Then
rebase-only pushes, re-runs and exact-tree merge groups still execute them twice. The wall-time cost is
small (10 / 62 s), but the claimed 60–75% exact-tree group reuse and the runner-minute totals do not hold
for the class the goal is built on. If the rule is read loosely (monotonic deadlines ignored, the expiry
path judged unreachable from tests), soundness rests on judgment the design does not specify.
*Likelihood:* high that the rule needs a sharper definition; medium for the speed impact.
*Measure:* run the planner's C5 classifier on `main`. Report the share of obligations and of
obligation-seconds marked C5, per class.

**4. A green waits for the slower witness, and that witness is slow by design.** Until compile reuse
graduates, witness 2 builds cold: 92 s on an idle node for the runner root [M]. After graduation it
builds over a *different* verified generation (section 7.3). Usually that is an older one, so its changed
set Δ is larger, and `main` moved 42–71 merges a day on 11-16 to 11-18 [mine]. Speeding up witness 1
(incremental, a hot node) cannot shorten time to green. *Likelihood:* high.
*Measure:* the distribution of witness-2 minus witness-1 compile time over 300 replayed diffs.

**5. Every compile number comes from an idle cluster.** The mbx experiment ran on nodes at 0–15% CPU.
On 2026-11-18 workers sat at 92–95% CPU [M], shared with ChainArgos and jackin. Each fully read clone
holds its own 14.29 GB of page cache [M], which caps a node at six build sandboxes. Two witnesses per new
key, A1, speculation and canaries double or triple CPU per key. At 95% utilisation, waiting is about
19× the service time (N38). Only one 16-CPU merge slot per site is reserved. *Likelihood:* medium-high for
P90, low for P50. *Measure:* replay the 11-18 arrival trace (46 merges, 13 PR runs at 71.8 min median)
against the real cluster load.

**6. Much of the claimed speed is repository work, not runner work.** The goal needs the test-floor
programme (125–138 s floor, 924.8 s and 679 s suites to ≤ 60 s), the crate split, removing `-j2` caps,
deleting `velnor-runner-host`, and `test-threads 1` lifted in ci-helper. Those changes speed up *any*
runner, GitHub-hosted included. *Likelihood:* certain. *Measure:* an ablation. Run today's CI with the
floor fixes and phase 0 on the existing ARC scale set, then run Velnor with and without them.

**7. There is no strong baseline.** Every comparison is against a degraded setup: a Free-plan hosted gate
queue, ephemeral pods with no state, 2-CPU requests that burst to 84 cores, and no auto-merge. A
conventional strong setup would capture much of the gain: ARC or Buildkite agents on XFS work volumes,
a reflink-cloned warm `target/` per root, the existing crates-mirror DaemonSet, a baked tool image,
gate jobs on the cluster, and auto-merge. The remaining Velnor-only advantage is about 13 s acquire, 6 s
pod start and the Actions offer tail (226 s P90 run-to-job, 626 s P90 superseding stall) [M], plus
content-keyed verdict reuse. That is real but much smaller than "26.7 min → 90 s".
*Likelihood:* high that the ratio over a strong baseline is far below the ratio over today.
*Measure:* benchmark B4 below.

**8. The group-reuse rate is a model.** 60–75% comes from replaying 172 queue entries over three days (Q11),
and holds only with auto-merge, which 0 of 40 PRs use. Each miss puts a changed-obligation re-run (twice)
on push-to-merge. *Likelihood:* medium. *Measure:* lab C1 on 300 live merges.

**9. Evidence gates set the dates.** Per-root keys and compile reuse each need about 3,000 clean
comparisons per kind or root, partly from replays. Replays of past commits are not independent samples of
future changes, so graduation by day 7.5–10 is optimistic. Until then, keys stay whole-tree and the App
gate estimate is ≈ 6 / 18 min [E], slower than a strong conventional baseline might reach.
*Likelihood:* medium. *Measure:* the graduation counter per kind, with a held-out set of live comparisons.

## Missed speed at the same correctness

1. **Narrow the ancestor-directory clause** (from objection 2). Use the read audit to enrol *which*
   obligations read `.github/**` and `.velnor/**`. Likely only the generator's golden tests and the
   planner do, not `velnor-ci`'s unit tests. That moves up to about 20% of heads off whole-tree keys and
   brings P90 down from the whole-tree verdict (minutes) toward the per-root verdict. It is the biggest
   P90 lever in reach. Soundness is unchanged: the contract is still enforced on every execution.
2. **A fast-red lane.** Incremental compilation is 5–6× faster per edit: 1.5 vs 9.2 s and 2.8 vs 12.2 s
   (blaze B3, a loaded Mac), and 10.9 vs 61 s for runner-host (ruling Q17). Run an incremental build in a
   non-minting lane, run the changed tests first, and post a failure to `Velnor / Details` at once. The
   status turns `failure` when a minting witness confirms it. Time to the first actionable red improves
   by about 8–50 s. Greens are untouched, so the zero-false-green contract holds.
3. **Witness 2 over a twin generation of the same commit.** The builder makes two independent cold builds
   of each landed tip on two nodes and verifies equal digests, so each node holds "its" generation of
   the same commit. Witness 2's Δ then equals witness 1's instead of reaching back to an older tip. This
   removes objection 4's penalty, and the two compiles stay independent per request. Cost: one extra cold
   build per landed tip, about 92 s × 32 cores of batch CPU. Prove it in lab B first.
4. **Test-level fan-out for receipts.** The inventory already lists exact test IDs. Ship the test binary
   once by digest, split the IDs into partitions, and give each partition its own witness slots. The test
   stage drops from about total CPU / 16 to max(longest test, total CPU / N cores). For `velnor-ci`, 781
   tests run serially today in 10 s / 62 s; with the 2 s per-test kill already enforced, the parallel
   floor is about 2–3 s. The design keeps fan-out off for receipts "until lab C". Put that lab first.
5. **Prepare on unlabelled pushes, at batch priority.** Section 6.2 already allows hedging for preparation:
   fetch, clone and a compile whose digest must match later. Compiling the closure of every push to a
   ready PR under `cpu.idle` takes the compile stage (15–25 s [E]) out of label-to-verdict. It runs no
   verification, so the green semantics of decision 12 do not change. It needs an owner ruling, because
   45% of pushes are superseded.
6. **The full-tree hash at checkout.** Seal bases with fs-verity, or keep a digest the agent checks once
   per base. Then hash only Δ: about 0.1–0.3 s off every obligation start [E]. This is small, but it is
   paid on every witness of every obligation.
7. **Reorder the delivery plan.** Rent a high-clock node earlier: 1.3–1.5× on the serial compile and on
   the longest test [E, I104]. Measure `debug = line-tables-only` for receipt builds (−20 to −27% CPU,
   I108) before the goal gate, not after. Capture a real link line before deciding anything about
   mold or wild.
8. **Report push-to-merge net of GitHub.** Not a speed gain, but a claim about "the fastest runner"
   should exclude the 19 s + 26 s GitHub owns, or state it as a fixed term.

## What to measure first to earn "fastest"

| # | Benchmark | Pass criterion |
|---|---|---|
| B1 | Source-edit rebuild: 300 real diffs from the helper and generator class, replayed at the 16-CPU class. Witness 1 over the latest generation, witness 2 as configured (cold, or another generation), plus a cold control. Run on an idle node and a node at 90% load. | Slower witness P50 ≤ 15 s and P90 ≤ 25 s, idle; P90 ≤ 40 s loaded; every warm test binary equals the cold digest. |
| B2 | C5 and whole-tree census of the released planner on `main` and the last 200 merged heads. | The share of obligations and obligation-seconds marked C5 per class, and the share of heads with any whole-tree key, both published. Goals are restated per cohort from these numbers. |
| B3 | Label-to-`success` on at least 200 matched heads per class, peak and quiet, with intervals (N40). | Helper and generator class P50 ≤ 90 s, P90 ≤ 4 min; the runner and whole-tree cohorts reported beside it, never folded in. |
| B4 | **The baseline arms**, all run on the same replayed heads: (a) today's CI; (b) a strong conventional setup on the same cluster (warm reflink target per root, crates mirror, tool image, gate jobs on the cluster, auto-merge, the same test fixes); (c) a hosted fast-runner arm with persistent cache volumes, if the owner allows one; (d) Velnor. | Velnor at least 2× faster than the best other arm at both P50 and P90 on push-to-merge net of GitHub's share. Below that, the word is "faster", not "fastest". |
| B5 | Planted-failure campaign on every fast path: 100 failing edits, covering the Q16 older-mtime edit, an undeclared build-script input, a new sibling file outside a contract, a clock-shifted test, a stale generation, and a flaky test at p = 0.05–0.2. | 100% red, or detected at the measured (1−p)² rate. No arm counts as "faster" until it passes the same campaign. |
| B6 | Load replay of the 2026-11-18 arrival trace on the shared cluster, ChainArgos and jackin included. | Verdict P90 under load ≤ 1.5× the quiet P90; merge-path wait P90 ≤ 10 s (the design's own trigger). |
| B7 | Paired Native vs Official runs on release and image jobs: same backend, tool store and generation. | A published per-job-type difference (decision 20 requires it). Unless it is large, the "fastest" claim moves from the runner to the verifier. |

If B1–B6 pass, a supportable claim for this repository is: *"push-to-merge within about 1 minute of
GitHub's own queue floor for the common PR, at zero observed false greens, at least 2× faster than the
best conventional setup we measured."* "The fastest runner ever" cannot be supported by evidence that
compares only against this repository's own degraded baseline.

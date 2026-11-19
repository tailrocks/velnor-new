# Is this the fastest runner? Critique by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Is Velnor "the fastest runner ever"? Independent review

Scope: architecture v3 plus evidence, decisions, delivery plan, owner decisions,
rulings and experiments (mbx, blaze, RustFS). Metric: push-to-merge / PR verdict at
zero false greens (C1–C7, two witnesses + audit A1). Vendor figures are labelled as
claims; only [M] numbers are treated as measurements of this repository.

## Verdict

No: "fastest runner ever" is not justified, and "fastest possible for this repository" is unproven.
For this repo Velnor would very likely beat GitHub-hosted runners as used here (Free plan, 20 slots)
and ARC as configured here (2-CPU requests, `emptyDir`, cold pods) by a large margin — but both are handicapped baselines.
Against the best alternatives — Namespace/Depot-style warm runners, a well-tuned ARC or Buildkite agent fleet
with persistent warm caches, and Bazel/Buck2 remote execution (RBE) — superiority is asserted from estimates, not measured.
Most of the win is doing less work (label flow, 3 runs → 1 ledger lookup, no hosted gate queue), which any executor could keep.
The native executor's own delta is explicitly unmeasured; every goal number is [T]/[E]. Earn "fastest here" with the benchmarks in §5 first.

## Where the time goes, per stage

Design §3 gives one typical PR (47% class: `generator/` or `ci-helper/` only). "Today"
is mostly [M]/[M, proxy]; "Goal" is [T] with an [E] in brackets. Nothing end-to-end at
the goal has been measured under load — the design itself says no goal counts as met
until joint traces under load support it.

| Stage | Today | Goal [T] ([E]) | Limiter; evidence |
|---|---|---|---|
| Event → start | run→job 2 s p50 / 226 s p90; hosted queue 5.5 min med [M] | webhook 1–2 s; reconciler ≤10 s | GitHub queue; today [M], goal tail unmeasured |
| Acquire (Actions jobs) | 13 s p50 / 27 s p90, n=1,124 [M] | off verdict path | Runner availability; sound removal |
| Sandbox start | ARC pod ~6 s + dind probe [M] | ≤0.3 s | Pool hit rate; clone 0.25 s [M, idle, warm only] |
| Checkout | 6 s × 31–39 jobs [M] | ≤1 s | Network → local hash/CPU; hash 0.1–0.3 s [E] |
| Toolchain / fetch | ~10 s + mise 2 s + fetch 3.5–6.1 s per job [M] | ~0 s (3 s on lock change) | Network; warm ≈0, cold = one fetch/site [E] |
| Compile | 196–497 crates/job; 8.5 min critical path [M] | ≤25 s (15 s) | rustc/link CPU; cold 92 s vs warm-noop 0.31 s [M, idle] |
| Tests | host 192/417 s; floor 125–138 s [M] | ≤60 s (10 s) | Longest test; cache cannot beat it either way |
| Reporting / Required | 1.48 GB upload 36–43 s; verdict queue 1.6/24.3 min [M] | ≤1 s + ≤1 s | Uplink + hosted queue; App write unmeasured |
| Queue entry + group | green→enqueue 7.5 min; group 19.9/84.1 min [M] | ≤30 s (~25 s) | Tree-hit rate; needs 60–75% [E], today 16% [M] |
| Merge (GitHub) | 26 s [M median] | 26 s | GitHub service; irreducible (decision 7) |
| **Verdict / push-to-merge** | **26.7/92.3 min; 85/457 min [M, proxy]** | **≤90 s/≤4 min; ≤3/≤8 min [T] (~35 s; ~85 s [E])** | Cold witness + longest test; targets, not runs |

Measured [M]: today's queues/duplication (45% cancelled; 18.3% unchanged heads; 3 runs
per merge; 4.4× crate builds); idle-node physics (92 s cold, 0.25 s clone, 39.9 s mbx
fill, 67.4 s one-crate lock change — all single-run, warm page cache, so they bound
rather than predict loaded latency); GitHub offer ~5–8 s; tree equality 16% today.
Estimated [E]/[T]: warm/cold costs (16/22 runner-min), 37–110 runner-min/PR, mirror
savings, the 35 s verdict and every date. No goal number yet excludes the confounders
— cold-vs-warm mix, cache hits doing no work, idle nodes — because no end-to-end run exists.

**The true floor** at the same confidence, staying on GitHub (decision 7) and cargo:

- GitHub queue + merge: ~19 s + ~26 s medians = ~45 s of push-to-merge that no runner
  removes. Stretch push-to-merge 100 s therefore leaves ~55 s for *everything* else.
- Two witnesses: latency = max(warm, cold) until reuse graduates, so the cold build
  *is* the latency (92 s idle, runner workspace). Runner-minutes ≥2× single-run CI.
- Longest test 924.8 s; 125–138 s floor behind the loops [M]. Until code fixes land
  (≤60 s goal), no runner beats ~2 min on the runner class; fan-out cannot beat it.
- Lockfile changes: 43–67 s even warm [M]; a new toolchain invalidates all keys at once.
- Physics: 14.29 GB page cache *per reflink clone* (no sharing [M]) → 4–6 build
  sandboxes per 125 GiB node, capping parallelism exactly when load peaks.

So the design's P50 (~35 s verdict, ~85 s push-to-merge) sits *close to* the floor for
the small-PR class *if* per-root keys enrol, reuse graduates, the floor is fixed and
nodes are idle. The P90 (4 min / 8 min) has no such support: every tail input
(contention, cold witness, group miss, C5 set, GitHub tail) is currently [E].

## The strongest objections

Each objection states evidence, likelihood it materially breaks the "fastest" numbers,
and the measurement that settles it. "Likely" = expect it without contrary data.

### 1. Per-root keys may enrol almost nothing; the goal depends on them

Evidence: 79 of 86 `CARGO_MANIFEST_DIR` files climb out of their package; 70 test files
list directories; every per-root contract must also include root/ancestor configs
(`rustfmt.toml`, `clippy.toml`, `Cargo.*`, `.cargo/`, `.github/`, …). The design admits
"most test keys stay whole-tree". Whole-tree keys mean any one-line change invalidates
every obligation: verdict = full run, twice. The ≤90 s goal and ≤50 runner-min goal
both require per-root enrolment (App-gate whole-tree estimate is ~6/18 min, ~110
runner-min). Likelihood: **high** that enrolment covers a minority of test time.
Settle it: run the eBPF read audit in shadow over ≥300 merges; report per-obligation
would-hit rate and runner-minutes avoided. Pass for the goal: ≥70% of test runner-min
enrolled with zero out-of-contract reads. Cost of failure: goal slips to the App-gate
row permanently — still ~4× better than today, not "fastest possible".

### 2. Until graduation the cold witness sets latency; graduation may never come for some roots

Evidence: second witness builds cold and binary digests must equal until ~3,000 clean
warm-vs-cold comparisons *per Cargo root / obligation kind*; then 2% sampling forever.
At 2.44 queue entries/hour [M] (≈60 merges/day, maybe ~100 labelled heads/day), 3,000
comparisons is weeks per kind *after* the system exists — inconsistent with "goal by
day 10". Digest equality also assumes reproducible builds at a fixed path (lab C06
open); one non-reproducible unit keeps its dependants cold forever. Likelihood:
**high** that the day-10 goal date is wrong; **medium** that ≥1 root never graduates.
Settle it: count comparisons/day per kind in shadow from day 1; publish the
non-reproducible-unit list. Pass: a dated graduation forecast per root from observed
rates, not from the delivery plan.

### 3. Twin independence is assumed, not shown — and hedging cannot shorten a green

Evidence: ruling Q12/Lab C2 unrun. Both witnesses share image, tool store, mirror
code, generation family and cluster; common-mode faults (stale generation, bad
materialiser, shared clock) pass both. Seeded-order batches via nextest filters are
"until built, no order difference is claimed" — i.e. today twins differ mainly by
node. Hedged copies must all pass (correctly), so racing only finds reds faster.
Likelihood: **medium** that twins are measurably correlated, forcing group re-execution
(the design's fallback), which deletes the exact-tree latency win. Settle it: Lab C2
as specified (20 seeded tests, p=0.05–0.2, twins vs 30-min-apart runs). Pass: joint
pass rate within sampling error of (1−p)².

### 4. The 60–75% exact-tree rate is circular: it needs the speed it is meant to produce

Evidence: 16% today [M]; 83/74/67/37% at 1/2.5/5/20 min windows requires
`merge-tree(queue tip, head)` verification *and* auto-enqueue inside the window.
Today 0 of 40 PRs use auto-merge and median green→enqueue is 7.5 min [M] (≈60% even
then). If verdict is 6 min (App gate) not 90 s, the window widens and the hit rate
falls; bursty agent arrivals (the actual workload) worsen it further. Speculation on
prefixes/bypass trees then burns the runner-minutes the ledger saved. Likelihood:
**high** that early hit rates land at 40–60%, not 75%. Settle it: Lab C1 shadow ledger
for 300 merges *with* auto-merge enabled in shadow accounting. Pass: measured group
delta runner-minutes ≤ the budget's 25–40% miss assumption.

### 5. All speed numbers are idle-node numbers; the cluster is measured saturated and shared

Evidence: workers sampled at 92–95% CPU; `velnor-runner-host` 3.7 → 8.8 min under load
(2.4×) [M]; pods request 2 CPU and burst to 84 cores (a named flake cause); three orgs
share five workers (tailrocks 60, ChainArgos 60, jackin 40 max runners). Every compile
[M] (92 s cold, 0.31 s warm) was taken on idle nodes, single workload, one commit.
Reserved merge capacity protects one slot per site; PR-verdict P90 — the headline
metric — is exactly what contention eats. Likelihood: **high** that loaded P90 is
2–3× the idle estimate. Settle it: replay a recorded busy day's arrival trace against
the new scheduler (or shadow-run it) and report verdict P50/P90 at 70/85/95%
utilisation. Pass: P90 ≤4 min at 85% with ChainArgos/jackin quotas honoured.

### 6. The executor delta — the thing "runner" names — is the smallest, least-proven part

Evidence: §5.3: Native vs Official speed difference "unmeasured as a difference".
Interpreter cost is 87–147 ms per JS action [M]; Worker setup 4–5 s/job [M, ARC era].
Hypothesis H1 verdict: "the language is not where the time is; locality is". The big
savings (label flow, ledger, mirrors, generations, promotion) attach to *either*
executor — and in principle to ARC/Buildkite agents given the same ledger. Meanwhile
conformance risk is large: ≥240+65+30 cases, evaluator fork D4 unresolved (legacy
`Expressions2` port is what v2.338.2 returns; the compliant port is ~28 h more),
`background: true` ×105 uses absent from legacy, Official cache-v2 endpoint still
open. A single semantic divergence is a false green or red, not a slowdown.
Likelihood: **high** that Native saves seconds/job, not minutes; **medium** that
conformance, not coding, gates every cohort date. Settle it: paired Native/Official
runs, same node/generation/store, per job type (design §5.7). Pass for the marketing
claim: publish the delta with intervals; if it is <10 s/job, call Velnor a fast
*verification system*, not the fastest runner.

### 7. C5, the full-tree check and the always-on read audit are unpriced taxes on every verdict

Evidence: C5 obligations (clock/network/event/randomness/Docker/commit-identity) run
fresh on *every* request, including "instant" rebase verdicts; 11 test files read the
clock today and the C5 census is not done. Every execution also pays a full staged-tree
hash and, once enrolled, eBPF tracing of six syscalls including `ENOENT`s — overhead
"measured in shadow", i.e. unknown. Compile-heavy Rust workloads open very many files;
even 5% audit overhead on a 60 s verdict is 3 s, and a slow C5 obligation sets the
floor for rebase-only pushes. Likelihood: **medium-high** that C5 + audit add 5–20% and
kill the "seconds" rebase story for some roots. Settle it: publish the C5 inventory
with per-obligation times; A/B verdict latency audit-on vs audit-off on the runner
workspace. Pass: C5 critical path ≤10 s and audit overhead ≤3% at P50.

### 8. Storage and delivery risks are carried as if they were free

Evidence: RustFS 1.0.0 is two months old, ~1 patch/10 days, open node-return (#9480),
replication-backfill (#8768) and small-PUT (#9161) issues; *all* local numbers are one
loaded Mac, single process, no Linux/XFS/multi-node/WireGuard test (phase-1 ST06
pending). Async scanner-driven replication has "scheduling bounds, not delivery SLAs",
so cross-site warmth at 35 ms DE↔FI is hopeful. The delivery plan (820 agent-hours,
goal day 10, P90 day 13.5) must flow *through today's 26.7/92.3-min CI and queue*,
with review and owner decisions (D1–D17) on the critical path. Likelihood: **medium**
that L1 stays `local` for months (correct but colder); **high** that evidence gates,
not agent-hours, set real dates. Settle it: ST06 on real workers (node loss/return,
fsync cost per durability mode, 8.7 GiB snapshot vs 92 s cold build). Pass: L1 hit
beats cold build on >10% of requests under load, else delete the tier from the claim.

## Missed speed

Gains below keep C1–C7 (same keys, witnesses, audits). Numbers are [E] unless marked.

- **Faster serial hardware first, not last.** Workers are EPYC 9454P (throughput parts).
  The design trials a high-clock node (1.3–1.5× serial [E]) only *after* floor fixes,
  yet rustc/link and the longest test are serial-bound today. One high-clock node per
  site for the critical-path lane: expect verdict −20–35% on compile/link-bound PRs.
  Namespace advertises ~1.9× single-core vs GitHub (vendor claim); Velnor leaves this on the table.
- **Linker and debuginfo tuning, budgeted.** Line-tables-only debuginfo is cited at
  −20–27% CPU (I108) but never enters the §3 budget; add mold/lld A/B, `share-generics`
  and codegen-unit tuning per root. Expect −10–25% cold-build CPU; must re-prove digest
  reproducibility. Note tension: deps at `opt-level=3` cut test CPU 67% on *one* test
  [M] but raised its build 470 s → 778 s wall — net verdict effect per root is unknown.
- **Fail-fast ordering for time-to-first-red.** Run historically flaky/changed tests
  first and cancel the twin on first red (witness rules already allow: any failure
  blocks). Zero effect on green latency; large effect on the metric developers feel.
  Expect first-red −50%+ on broken PRs at no runner-min cost.
- **Pre-label speculation at batch priority.** The label flow saves runner-minutes but
  serialises human/agent labelling into push-to-merge. Speculatively verifying ready,
  unlabelled heads when idle (results already content-keyed) trades ~5–10 runner-min
  per speculated head for up to the full verdict time on label. The design speculates
  queue prefixes but not this; for "fastest push-to-merge" it is the cheaper direction.
- **Overlayfs shared page cache (Lab B2) and tmpfs for hot targets.** Reflink clones
  each re-read 14.29 GB [M]; a shared lower layer could lift the 4–6 sandbox/node cap
  and cut cold-read time under concurrency. Worth more under load than any micro-cache.
- **Cross-node test fan-out, once Lab C allows it.** Splitting the longest binaries
  longest-first across idle nodes attacks the 125–138 s floor directly; bounded below
  by the longest single test, so pair it with splitting looped tests (already planned).
- **What "fastest possible" would still beat this by:** (a) Bazel/Buck2 + RBE
  (BuildBuddy/NativeLink class): per-action caching and elastic remote workers beat
  cargo's crate granularity and five fixed nodes on cold/distributed builds — rejected
  here implicitly by staying on cargo, correctly on migration cost, but then
  "fastest possible" needs that caveat; (b) leaving GitHub's merge queue for an owned
  merge train removes the ~45 s floor — excluded by decision 7; (c) managed warm
  fleets (Namespace, Depot: vendor claims up to 3× vs GitHub-hosted, cache to
  ~1,000 MiB/s) already sell warm start + fast cache without 820 agent-hours of
  executor maintenance. None was benchmarked on this repo; §5 B6 fixes that.
  Correctly rejected on evidence: blaze (built 0 of 5 workspaces, missed a clippy
  `deny` = false green, slower cold [M]) and GitHub cache restore (44–61 s > build [M]).

## What to measure first to earn the "fastest" claim

Rules for every benchmark: pre-register the claim sentence; run both sides tuned as
production (same commit, node, generation warmth); interleave A/B/A/B ≥5 runs per side
and report median + range; count errors and verify outputs/digests, not just exit 0;
name the limiter (CPU, I/O wait, queue) from an untimed profile; report the micro
result as a share of end-to-end verdict/push-to-merge. A gap inside run-to-run noise
is "no measurable difference"; an untuned or single-run result is inconclusive.

1. **B1 Executor delta.** Native vs Official vs tuned ARC, same node/store/generation,
   per job type. Pass to claim runner speed: Native faster with the gap > noise on
   ≥3 job types; publish seconds/job either way.
2. **B2 Loaded verdict replay.** One recorded busy day replayed (or shadow-run) at
   70/85/95% utilisation with all three orgs. Pass: PR verdict P50 ≤90 s, P90 ≤4 min
   at 85%; otherwise the goal is an idle-machine number.
3. **B3 Key-enrolment census.** Read-audit shadow, 300 merges (Objection 1) + C5
   inventory with times (Objection 7). Pass: ≥70% test runner-min enrolled; C5 path ≤10 s.
4. **B4 Warm-vs-cold truth.** Per root: cold, warm-noop, warm-typical-diff and
   lockfile-change builds, interleaved, digests checked. Pass: warm-typical ≤25 s and
   digest-equal ≥99.9%; list every non-reproducible unit (Objection 2).
5. **B5 Queue experiment.** Lab C1+C2 (300 merges): exact-tree rate, group delta cost,
   twin joint-pass vs (1−p)². Pass: misses ≤40% and twins independent (Objections 3–4).
6. **B6 Outside baseline.** Same 10 representative PRs on (a) GitHub-hosted large
   runners, (b) one managed warm fleet (Namespace or Depot), (c) Velnor prototype,
   same obligation inventory. Pass for "fastest here": Velnor wins push-to-merge P50
   *and* P90 with error counts equal. Without B6, say "faster than our old CI" only.
7. **B7 Floor audit.** Longest-test histogram after floor fixes; no test >60 s (goal)
   proven by nextest report, not by suite wall time. Until B7 passes, no sub-2-min
   claim on the runner class is credible.

Bottom line: build it — the dedupe, mirrors, generations and App gate are well-founded
and worth ~5–10× on this repo's current numbers. But call it what the evidence supports
today: a fast, correctness-first verification system with a promising native executor.
"Fastest runner ever" needs B1–B7, on this repo, under load, against tuned rivals.

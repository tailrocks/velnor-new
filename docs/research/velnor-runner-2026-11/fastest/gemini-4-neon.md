# Is this the fastest runner? Critique by Gemini 4 Neon

| Field | Value |
|---|---|
| Model | Gemini 4 Neon |
| Model id | `gemini-4-neon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Critical Review: Is Velnor "The Fastest Runner Ever" for `velnor-new`?

## Verdict
**No — the "fastest runner ever" claim is not yet justified, and the current design is not the fastest possible system for `velnor-new` at zero false greens.**
- **vs. GitHub-hosted runners (`ubuntu-26.04`) & stock ARC:** Velnor wins decisively (10–30× on push-to-merge), but most of that win comes from moving off the 20-slot Free-plan queue, bypassing Actions on PR/merge-group verdicts, node-local stores, and XFS reflink generations — not from rewriting `Runner.Worker` in Rust (`decisions.md` H1–H2).
- **vs. tuned ARC / `velnor-official` sharing the same ledger & stores:** Unproven. On the verdict path Actions jobs are gone anyway, and the native-vs-official delta (`§5.3`) is unmeasured (only 4–5 s Worker setup + 87–147 ms/action is cited).
- **vs. Namespace-style fast runners (Namespace.so, Blacksmith, WarpBuild):** Velnor beats them on cross-PR/merge-group reuse and zero Actions-hop overhead, but loses on single-thread compile/test latency because Hetzner EPYC 9454P nodes lag high-clock desktop/workstation CPUs by ~1.3–1.5× (`I104`).
- **vs. Buildkite-style persistent agents & Bazel/Buck2 + RBE:** A Buildkite-style orchestrator on high-clock nodes with the same ledger + a CAS merge train (N31) beats Velnor's push-to-merge floor by ~45–60 s with far less complexity; Bazel/Buck2 + RBE beats Velnor on wide/lockfile rebuilds and hermeticity-by-construction (no eBPF read-audit fallback or cold second-witness tax), though warm local Cargo still wins on tiny leaf edits.
- **Bottom line:** Calling Velnor "the fastest runner ever" is premature until its unmeasured hot-path stages are benchmarked under load and its self-imposed floors (45 s GitHub merge queue, EPYC clock ceiling, whole-tree/C5 fallbacks, `CARGO_INCREMENTAL=0`) are addressed or explicitly bounded.

## Where the time goes, per stage

Baseline and Goal (`§3`, `§1.4`) for the **47% PR class** (`generator/` or `ci-helper/` only), labelled, with basis tags (**[M]** measured, **[E]** estimated, **[T]** target) and the true achievable floor at zero false greens.

| Stage | Today | Design Goal [T] (Est.) | Basis in docs | True floor (w/ GH MQ) | True floor (w/o GH MQ) |
|---|---|---|---|---|---|
| 1. Webhook / trigger | 1–2 s run; 5.5 m hosted Q [M] | 1–2 s (reconciler ≤10 s) | **[E]** (webhook untested; V5 ref→run 3–5 s, 5 outliers 122–424 s) | 0.5–1 s | 0.5–1 s |
| 2. Acquire | 13 s p50 / 27 s p90 [M] | 0 s (bypassed on verdicts) | **[C]** (App status replaces Actions job; Actions jobs stay ~5–8 s [M]) | 0 s | 0 s |
| 3. Sandbox start | ~6 s pod + dind probe [M] | ≤ 0.3 s | **[M] partial** (0.25 s = `target/` reflink only; pod attach, eBPF, 2nd node unmeasured) | ~0.1 s (direct cgroup + overlayfs) | ~0.1 s |
| 4. Checkout + plan + Δ | 6 s × 31–39 jobs [M] | ≤ 1 s | **[E]** (0.1–0.3 s tree SHA-1 is pure arithmetic; `merge-tree`, mirror fetch race, `velnor-ci plan`, mtime stamp omitted) | ~0.15 s (`fs-verity`/ro-overlay + Δ hash) | ~0.15 s |
| 5. Toolchain & tools | 10–18 s + 2 s mise + 4.4 s dl [M] | ≈ 0 s | **[E]** (ro bind mount; realistic) | 0 s | 0 s |
| 6. Dependency fetch | 3.5–6.1 s (0/48 hit) [M] | 0 s (≈ 3 s lockfile) | **[M]** (mirror 2.7–3.3 s [M]) | 0 s (≈ 2 s lockfile) | 0 s |
| 7. Compile (incl. clippy/doc) | 14 s `velnor-ci`, 81 s `runner-host` [M] | ≤ 25 s (15 s) | **[E]** (**zero** cluster edit-rebuild data; only 0-crate 0.31 s [M] & lockfile 67.4 s [M] exist; `INCR=0` + 2nd witness on `G_{n-1}` unmeasured) | 4–8 s (high-clock + split crates + `wild`/`mold`, or 2–3 s w/ proven `INCR=1`) | 4–8 s |
| 8. Tests | 10 / 62 s `velnor-ci`; 192 / 417 s `runner-host` [M] | ≤ 60 s (10 s) | **[E]** (ignores 53% non-helper PRs, new 94k-LOC `executor/`, seeded nextest filter-batching tax) | 3–6 s (high-clock + parallel + pipelined link/run) | 3–6 s |
| 9–10. Report + status | 36–43 s upload + 49 s verdict + hosted Q [M] | ≤ 2 s | **[E]** (status write + read-back = 2 US API RTTs ≈ 0.5–1 s) | ~0.5 s | ~0.5 s |
| **PR Verdict (P50)** | **26.7 min [M, proxy]** | **≤ 90 s [T] (≈ 35 s [E])** | **Unmeasured end-to-end** (Day 5.2 is ≈ 6 / 18 min [E]) | **~8–16 s** | **~8–16 s** |
| 11. MQ enqueue + group | 7.5 m enqueue + 19 s grp + 19.9 m run [M] | ≤ 30 s (≈ 25 s) | **[M] 19 s** + **[E]** exact-tree hit (60–75% Q11) assuming **0 C5 tests** | 20–22 s (if 0 C5 & tree hit) | ~1–2 s (CAS train) |
| 12. Merge landing | 26 s [M] | 26 s | **[M]** GitHub MQ median | ~26 s (GH MQ floor) | ~0.5 s (API ref FF) |
| **Push-to-Merge (P50)** | **85 min [M, proxy]** | **≤ 3 min [T] (≈ 85 s [E])** | **45 s of the 85 s est. is GitHub MQ dead time** | **~55–65 s** | **~10–19 s** |

**Key takeaway from the table:** Almost every Goal latency number (stages 1, 3, 4, 7, 8, 9, 10, and group lookup in 11) is **[E]**, not **[M]**. The only measured cluster build numbers are a 0-crate no-op (`0.31 s`), a cold build (`92.0 s`), and a lockfile downgrade (`67.4 s` plain / `43.4 s` mbx). Meanwhile, the single largest irreducible term in the 85 s push-to-merge estimate is **45 s of GitHub merge-queue overhead** (19 s + 26 s).

**What the §3 table hides (the other 53% of PRs and pre-graduation regimes):**

| Regime / PR class | Design number | Why it diverges from the 35 s / 85 s headline |
|---|---|---|
| Day 0 (Phase 0 config only) | ≤ 12 min verdict / ≤ 35 min merge [E] | Half the total win happens before any Velnor Native code exists (`delivery-plan.md` §3.4). |
| Day 5.2 (App gate live, whole-tree, cold 2nd witness) | ≈ 6 / 18 min verdict; ≈ 9 / 30 min merge [E] | Cold 2nd witness sets wall time until 3,000 clean comparisons/root graduate compile reuse. |
| Lockfile / dependency bump | 67.4 s plain / 43.4 s mbx-B [M] + tests | Measured for a 1-crate `fastrand` downgrade (11 crates); larger bumps scale much worse. |
| Root workspace PRs (`velnor-actions-*`, ~350k LOC) | Unbudgeted in §3 | Holds `non_inputs_differential` (679 s [M]), `receipt-integrity` (924.8 s [M]), `actions-cli` (238 s [M]). |
| New `executor/` workspace (~94k LOC, 15 crates) | Unbudgeted in §3 & §4 | Replaces `velnor-runner-host` at F01, adding 335-case dual-executor conformance + 30 mutants. |
| Merge-group base misses (25–40% of groups, Q11) | Unbudgeted in §3's 25 s stage 11 | Misses must recompute `merge-tree`, delta-compile, and run two witnesses on the group SHA. |

## The strongest objections

### 1. The Rust Actions executor does not drive the speedup; the verdict hot path bypasses GitHub Actions altogether
- **Evidence:** `decisions.md` H1 admits "Rust everywhere beats JS" **failed as stated** (Node startup is 87–147 ms/action [M]; idle ARC starts a job in 2–3 s [M]; native speedup is unmeasured). After Step 4 (`§8.1–8.2`), PR and merge-group verdicts have **no Actions run** — `velnor-control` invokes `ensure()` and posts `Velnor / Verified`. Yet the plan budgets ~40k+ LOC for `velnor-exec`, `velnor-dispatch`, `velnor-native-actions`, and `velnor-conformance`, which sit off the verdict path (serving release/image/scheduled jobs).
- **Likelihood:** **Certain** (explicitly documented in `§5.3` and `decisions.md` H1–H2).
- **Measurement:** Execute work item `O04`: paired runs of `velnor-native` vs. `velnor-official` sharing the exact same K8s backend, node mirrors, read-only tool store, and warm XFS generation across all 4 cohorts; report P50/P90 deltas.

### 2. GitHub's Merge Queue enforces a ~45 s push-to-merge floor and a 25–40% exact-tree miss rate
- **Evidence:** The single headline metric is **push-to-merge** (`§1.1`). Measured GitHub queue latency is `19 s` (enqueue → group) + `26 s` (green → merge) = `45 s` (`§3`), eating 53% of the 85 s Goal estimate and 45% of the 100 s Stretch target even with zero-second verification. Ruling Q11's entry replay (172 entries) shows `25–40%` of merge groups still miss exact-tree equality even with tip-targeted PR checks and auto-merge. Decision 7 keeps GitHub's queue and defers a CAS fast-forward train (`N31`).
- **Likelihood:** **High** (based on measured queue medians and entry replay).
- **Measurement:** Instrument `enqueue → checks_requested`, `checks_requested → Velnor/Verified`, and `Velnor/Verified → main ref update` across 200 auto-merged PRs, and shadow-compare against a Velnor CAS fast-forward train (`N31`) for tree-match rate and end-to-end latency.

### 3. The repo's actual tests defeat both "instant" merge-group verdicts (C5) and narrower keys (per-root / per-package)
- **Evidence:** `§6.1` reports two fatal repo facts: **(a)** *"79 of 86 files using `CARGO_MANIFEST_DIR` climb out of their package and 70 test files list directories, so most test keys stay whole-tree"* — directly undermining the Goal (requires per-root keys) and Stretch (requires per-package keys); **(b)** *"11 test files read the clock today"* (C5) and `root.rs:22` calls `git rev-parse`, yet *"An 'instant' verdict needs zero C5 obligations and zero misses"* (`§6.1`), and C5 obligations run fresh with **two witnesses** on every merge group (`§6.3`). Even one C5 test on `main` eliminates instant merge-group verdicts.
- **Likelihood:** **High** (counts are directly measured from the repo).
- **Measurement:** Run `V07` (eBPF read audit) and `V12` (C5 classifier) in shadow across 100 real PRs and merge groups; measure the exact percentage of obligations eligible for narrower keys and the mandatory C5 wall time per exact-tree merge group.

### 4. Section 7.3's dep-info coverage rule triggers workspace-cold rebuilds whenever a PR adds a file
- **Evidence:** `§7.3` steps 2–3 state: *"Δ is every path whose blob or mode differs from M_G, plus every path added or removed. If a path in Δ is outside every unit's declared inputs (dep-info, `rerun-if-changed`, ...), every workspace unit of that Cargo root builds cold."* Any PR adding a new `.rs` module, integration test, or golden sample introduces a path absent from generation `M_G`'s dep-info, tripping a full workspace-unit cold rebuild of that Cargo root (and compounding the cost of `CARGO_INCREMENTAL=0`).
- **Likelihood:** **High** (direct logical consequence of `§7.3` step 3 as specified).
- **Measurement:** Replay 100 historical `velnor-new` PR diffs against `§7.3`'s exact dep-info rule; measure the share of PRs forced to rebuild all workspace units cold and the resulting P50/P90 compile penalty.

### 5. The §3 latency budget cherry-picks the easiest 47% PR class and omits the new ~94k-LOC `executor/` workspace
- **Evidence:** The 35 s verdict / 85 s push-to-merge estimate covers only PRs touching `generator/` or `ci-helper/`. It omits the 53% touching root crates (`velnor-actions-*`, ~350k LOC, `non_inputs_differential` 679 s, `receipt-integrity` 924.8 s) and completely ignores the build/test cost of the brand-new `executor/` workspace (`§5.2` line budgets sum to **94k LOC** across 15 crates + 335-case dual-executor `velnor-conformance` + 30 mutants!) that replaces `velnor-runner-host`. Furthermore, **no single-file source edit** has ever been timed on an EPYC worker with `CARGO_INCREMENTAL=0` (on a faster-clock M5 Max, even a 1-line edit in the 4-crate `S-gen` subset took 9.2–12.2 s, `blaze.md` B3; `velnor-ci` alone is 40.6k LOC).
- **Likelihood:** **High**.
- **Measurement:** Time a 1-line edit in `velnor-ci`, `velnor-gen`, `velnor-actions-orchestrator`, and a 94k-LOC proxy workspace on an EPYC 9454P worker with `CARGO_INCREMENTAL=0`, parallel `clippy`+`doc`+`nextest` lanes, full-tree hash, and eBPF audit under 70% cluster load.

### 6. XFS reflink clones duplicate 14.29 GiB of page cache each, and 3-lane target cloning + RustFS co-location risk severe memory thrashing
- **Evidence:** `mbx-cache-cluster.md` E1d proved XFS reflink clones do **not** share page cache (`14.29 GiB` resident RAM per fully read clone of `crates/velnor-runner`). Meanwhile, `§8.1` / `N45` splits `test`, `clippy`, and `doc` into **separate target directories** per sandbox, `§9` packs 4 build sandboxes (24 GiB reservation, 16.8 GiB peak RSS observed) onto 125.5 GiB workers, and `§7.1` co-locates RustFS on the same workers. Under 92–95% load (`evidence.md` T6), `runner-host` already slowed 2.4× (`3.7 → 8.8 min`). Overlayfs (which *does* share page cache) is deferred to Lab B2.
- **Likelihood:** **Medium-High** (page-cache duplication and load slowdown are [M]; 3-lane + RustFS interaction is untested).
- **Measurement:** Expand Lab B2 to 4–6 concurrent sandboxes running parallel `test`+`clippy`+`doc` lanes alongside an active RustFS pod on one 125 GiB worker; compare PSI, `fincore` evictions, and P50/P90 wall time for XFS reflink vs. overlayfs.

### 7. Parallel two-witness execution + older-generation `G_{n-1}` + nextest filter-batching inflate tail latency and flake ejections
- **Evidence:** `§1.2` requires every new key to run twice in parallel on two nodes over **two different verified generations** (or one cold until 3,000 clean comparisons/root), taking `max(witness_1, witness_2)`. Witness 2 on `G_{n-1}` has a strictly larger diff (or builds cold for 92 s+ pre-graduation). Because `cargo-nextest` 0.9.151 lacks built-in shuffling, Witness 2 runs tests in **seeded filter-expression batches** (`§6.2`), adding sequential startup overhead and per-batch straggler tails. Zero retries also doubles false-failure probability (`8.1% → 15.5%` today; `3% → 5.9%` at intermediate budget), and if Lab C2 finds twins correlated, the merge group must re-run changed obligations anyway.
- **Likelihood:** **Medium-High** (both batching overhead and `G_{n-1}` straggler delta are unmeasured).
- **Measurement:** Benchmark seeded nextest filter-batching vs. native nextest on `velnor-ci`/`velnor-gen`, measure `max(T(G_n), T(G_{n-1})) / T(G_n)` over 50 sequential commits, and run Lab C2's correlation experiment.

### 8. Low-clock server CPUs (EPYC 9454P) and mandatory `CARGO_INCREMENTAL=0` surrender 2–5× on the serial critical path
- **Evidence:** All 5 workers are 96-thread EPYC 9454P/9454 server chips tuned for throughput, not single-thread frequency (`§B.0`). `decisions.md` I104 acknowledges high-clock nodes cut serial compile/test chains by `1.3–1.5×`, yet `§9` defers trialling even one node until after test-floor fixes. Simultaneously, `CARGO_INCREMENTAL=0` (Q17) slows edit-rebuilds by `3–6×` (`1.5–2.8 s` vs. `9.2–12.2 s` in `blaze.md` B3; `10.9 s` vs. `61 s` in `I073`) purely because incremental CGUs break warm-vs-cold binary digest equality.
- **Likelihood:** **Certain** (both gaps are measured).
- **Measurement:** Run identical single-file edit + serial test benchmarks on one rented Ryzen 9950X / EPYC 4564P node vs. EPYC 9454P, and run Lab B4 to measure actual verdict divergence of `CARGO_INCREMENTAL=1` across 300 historical diffs.

### 9. Unmeasured syscall-audit, full-tree-hash, and RustFS 1.0.x overheads threaten P90 predictability
- **Evidence:** Every checkout hashes 110 MiB across thousands of small files (`§7.4`: `0.1–0.3 s` assumes linear 1 GB/s SHA-1 and ignores syscall/dentry traversal on uncached XFS clones). Every per-root run attaches eBPF probes to `openat`/`openat2`/`newfstatat`/`statx`/`readlinkat`/`getdents64` (`§6.4`), which rustc/cargo invoke hundreds of thousands of times per build — overhead is unmeasured. RustFS 1.0.0 is 2 months old with zero Linux/NVMe measurements and open issues on node return (`#9480`), replication backfill (`#8768`), and small PUT regressions (`#9161`).
- **Likelihood:** **Medium** (RustFS is well-fenced by `ST06` fallback to `local`; eBPF and tree-hash taxes hit every single run).
- **Measurement:** Benchmark full-tree verification + eBPF tracing overhead (wall time and CPU %) during a full `cargo nextest` build+run on an EPYC worker, plus `ST06` on Linux XFS NVMe.

## Missed speed

Concrete further gains at **identical zero-false-green confidence**:

1. **Move serial-critical-path builds and tests onto 1–2 high-clock nodes now (Ryzen 9 9950X / 7950X / EPYC 4564P):**
   - *Gain:* **30–40% faster** single-crate rustc/link/serial-test wall time — cutting the 15 s warm compile to **~9–10 s** and a 25–60 s test floor by **~8–20 s** per verdict, and cold serial chains from ~92 s to **~60 s** (`I104` estimates 1.3–1.5×).
   - *Correctness:* Execution fingerprint already carries CPU/arch (`§6.1`); zero soundness change.
2. **Replace GitHub's Merge Queue with a Velnor CAS fast-forward merge train (`N31`):**
   - *Gain:* Eliminates the **45 s** [M] GitHub MQ floor (`19 s` + `26 s`) and raises tree-match rate from `60–75%` to **~95%+**, cutting Goal push-to-merge estimate from `85 s` to **~40–45 s** (and Stretch from `100 s` to **~50–55 s**).
   - *Correctness:* Equal or stronger (every landed commit verified on its exact resulting tree; atomic ref compare-and-swap via GitHub API). Requires revisiting Decision 7 for the queue engine only.
3. **Read-only overlayfs lower layers + `fs-verity` / immutable sealed trees + direct cgroup/namespace sandboxes (`I082`):**
   - *Gain:* Cuts page-cache RAM per sandbox from **14.29 GiB → ~0.5–1 GiB** (raising safe concurrency from 4 to 6–8 build slots/node and preventing T6's 2.4× load slowdown); cuts sandbox attach + checkout from `1.3 s` to **< 0.15 s**; and because the kernel enforces immutability on a verified read-only lowerdir, checkout only hashes the upperdir Δ (**~0.3–1.5 s → < 10 ms**).
   - *Correctness:* Kernel-enforced write isolation and cryptographic lowerdir immutability (stronger than mutable reflink + post-hoc scan).
4. **Fix `§7.3` step 3 for added files + split monolithic crates (`velnor-ci` 40.6k LOC, `velnor-gen` 27.4k LOC) + `wild`/`mold` linker + `debug=line-tables-only` (workspace) / `0` (deps):**
   - *Gain:* Scoping added-file invalidation to the owning crate + its reverse-dep closure (plus build-script crates without `rerun-if-changed`) saves **10–60 s** on PRs that add files; splitting `velnor-ci` cuts edit-rebuild by **40–60% (~5–15 s)** under `CARGO_INCREMENTAL=0`; `wild`/`mold` + reduced debuginfo (`I108`, not yet credited in `§3`) cuts link + target I/O by **20–40% (~2–6 s)** and shrinks generations by ~24%.
   - *Correctness:* Same `rustc` checks and line-number backtraces; crate splits and crate-scoped added-file invalidation preserve full dep-info soundness.
5. **Eliminate C5 non-determinism (11 clock-reading test files) and fence `CARGO_MANIFEST_DIR` fixture paths:**
   - *Gain:* Unlocks **true ~2 s exact-tree verdicts** on 60–75% of merge groups and 100% of rebases (**10–30 s saved** per exact-tree merge group) and makes per-root / per-package keys actually hit on real PRs instead of falling back to whole-tree (**30–50% fewer runner-min**).
   - *Correctness:* Deterministic fake clocks/pinned timestamps keep identical assertions and make tests less flaky.
6. **Single deterministic compile feeding two independent test witnesses + native `--shuffle` in `cargo-nextest` + pipelined test execution:**
   - *Gain:* Once compile reuse graduates (with A1 cold audit + 2% sync cold sampling), compiling once on `G_n` and running two test witnesses against the content-addressed binaries eliminates the `G_{n-1}` compile straggler (**5–25 s** on P50/P90) and halves compile CPU; a small native shuffle patch to `cargo-nextest` removes filter-batching overhead (**2–10 s**); launching each test binary as soon as it finishes linking (instead of waiting for `--list-type binaries-only` across the whole root) overlaps linking and testing (**5–15 s**).
   - *Correctness:* Preserves two independent test runs in different orders on different nodes, plus cold A1 verification of every landed commit.
7. **Cluster-wide test/build fan-out (`N04` / Buck2-RBE style) for the P90 wide-PR & lockfile tail:**
   - *Gain:* Recording deterministic shard manifests in receipts enables cross-node test fan-out on wide root-workspace / `executor/` / lockfile PRs, pulling P90 PR verdict from `4 min` down toward **~60–90 s**.
   - *Correctness:* Exact test-ID inventory (`C3`) + deterministic shard layout in the execution manifest (`§6.2`) prevents dropped tests.

**Summary of missed gains (non-additive where overlapping):**

| Lever | Where it cuts | Estimated saving (zero false greens) | Scope / constraint |
|---|---|---|---|
| High-clock serial nodes (Ryzen 9950X / EPYC 4564P) | Stages 7–8 (serial compile + tests) | −13 to −30 s verdict (−30–40%) | Hardware only (~$150–250/mo) |
| CAS fast-forward merge train (`N31`) | Stages 11–12 (GitHub MQ + misses) | −40 to −45 s push-to-merge | Amends Decision 7 (queue only) |
| Overlayfs + `fs-verity` lowerdir + direct cgroups | Stages 3–4 + contention tail | −1.2 s idle; avoids 2.4× load slowdown | Replaces pod + reflink default |
| Crate-scoped added-file rule + split `velnor-ci` + `wild`/`mold` + `I108` | Stage 7 (compile + link) | −10–60 s on new-file PRs; −7–20 s edits | Code/config inside existing rules |
| Fake clocks in 11 C5 files + scoped test fixtures | Stage 11 + per-root/pkg hits | −10–30 s exact-tree MQ; −30–50% r-min | Repo test hygiene |
| Single compile → 2 test witnesses + native nextest shuffle + pipelined link/test | Stages 7–8 (2nd witness tail) | −10–35 s P50/P90 verdict | Post-graduation + nextest patch |

## What to measure first to earn the "fastest" claim

Run #1, #3, #4, and #6 **before building ~94k LOC of `executor/`** — they require only existing cluster nodes, one rented high-clock box, and a small harness, and they settle whether the Goal/Stretch numbers are even reachable on this architecture.

1. **Cluster single-file edit matrix (Lab B-edit):** 1-line edit in `velnor-ci`, `velnor-gen`, `velnor-actions-orchestrator`, and a new-file-added edit, on an EPYC 9454P worker with `CARGO_INCREMENTAL=0`, parallel `clippy`+`doc`+`nextest`, full-tree hash, and eBPF audit at 70% cluster load (≥ 5 reps each).
   - *Pass:* Median total compile + checkout + audit ≤ 25 s for `ci-helper`/`generator`; added-file PR does not trigger root-wide cold rebuild; eBPF + tree-hash overhead ≤ 1.5 s combined.
2. **Paired executor shootout (`O04`):** Identical jobs run on `velnor-native` vs. `velnor-official` on the exact same K8s backend, node mirrors, tool store, and warm XFS generation (≥ 30 pairs/cohort).
   - *Pass:* `velnor-native` beats `velnor-official` by a statistically significant margin at P50 and P90 on every cohort, justifying the ~40k+ LOC native executor over an official-runner + ledger architecture.
3. **Reflink vs. overlayfs under multi-lane contention (Lab B2+):** 4 and 6 concurrent build sandboxes running parallel `test`+`clippy`+`doc` lanes alongside RustFS on one 125 GiB worker.
   - *Pass:* Zero OOM/evictions, P90 build+test slowdown vs. idle ≤ 1.25× (vs. today's 2.4× in T6), and adopt whichever storage layer wins.
4. **C5 & key-narrowing census (`V07` + `V12` shadow):** 100 real PRs + 100 merge groups audited by eBPF and the C5 classifier.
   - *Pass:* ≥ 80% of `ci-helper`/`generator` obligations qualify for per-root keys (not whole-tree), and C5 wall time on an exact-tree merge group is ≤ 3 s (or 0 s after fixing the 11 clock-reading files).
5. **Two-witness straggler & batching overhead (Lab C2+):** Compare `max(T(G_n), T(G_{n-1}))` with seeded nextest filter-batching against a single warm witness across 50 sequential commits, plus C2 correlation check.
   - *Pass:* Second-witness + batching overhead adds ≤ 20% at P90, and twin joint pass rate matches `(1 − p)²` within tolerance.
6. **Hardware clock A/B (`I104`):** Run Benchmark #1 + the surviving slowest test suites on one rented high-clock node (Ryzen 9950X / EPYC 4564P) vs. EPYC 9454P.
   - *Pass:* Either EPYC 9454P is within 15% of the high-clock node (refuting Objection 8) or high-clock nodes are added for the serial critical path.
7. **End-to-end matched-cohort external comparison:** Run 50 real `velnor-new` PRs (all classes, including `executor/` and wide PRs, with zero false greens on a planted-fault corpus) on Velnor vs. (a) tuned ARC + warm XFS + ledger, (b) a Buildkite-style persistent agent on the same hardware, and (c) a Namespace/Blacksmith-class high-clock runner.
   - *Pass:* Velnor achieves P50 PR verdict ≤ 60 s, P90 ≤ 3 min, push-to-merge P50 ≤ 90 s (or documents the exact 45 s GitHub MQ floor), beats every baseline on matched cohorts, and catches 100% of planted faults.

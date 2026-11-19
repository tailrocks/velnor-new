# Review by Muse Wonder

| Field | Value |
|---|---|
| Model | Muse Wonder |
| Model id | `muse-wonder-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

# Cross-critique: Proposals B, G, E (Velnor runner research)

Method: claims checked against `evidence.md` (T/B/C/E/F sections) and cited code. A number with no method, no denominator, or a denominator the pack defines differently counts as unproven.

## Proposal B

### Weakest assumptions

1. **Reflinked `target/` gives 92–96% first-push hits in 0.5–1.2 s.** The clone time (14.7 GB, ~45k files) is an estimate at 40k–100k extents/s with no local measurement and no hit-rate denominator. Only measured warm data point the other way in shape: mbx hits 40–74% *within one job* (T9a); remote restore of 1.92 GB took 44–61 s, slower than cold (#585). Cross-job reuse is unmeasured because every job starts from `emptyDir` (B §3.3). Settles it: `velnor-lab` A/B on runner-host — reflink a sealed `main` target, `inputs restore`, count recompiled crates cold/warm/second-push. The §5 arithmetic (24–38 s typical PR) inherits this unproven number.
2. **Tree-equal `merge_group` fast-pass + affected-only selection keeps confidence.** T7: PR-head tree equalled merge-group tree in only **7/45 (15.6%)**; 34/45 heads were ≥1 commit behind base. B quotes 15.6%, then projects "~50% once merges finish in ~1 min" with no mechanism — faster merges do not stop the base advancing. The 8 s path is the rare path. The common path rests on planner completeness, contradicted by G's plan artifact (8 packages with `undeclared_reads`, incl. every heavy job). A nightly `mode=all` net detects a bad merge a day late; it is not same confidence at merge time. Settles it: replay 200 merged PRs through the selector, diff executed tests vs the runs that merged them.
3. **Sandboxes start <50 ms (namespace) / ~100 ms (KVM with guest `dockerd` live).** No measurement for either. B's own legacy survey undercuts the second: Firecracker backend had an *empty cache transport* (`velnor-legacy/.../execution/backend.rs:129-137`). Snapshot-restoring a VM with live Docker, seeded layers, CoW workspace and net policy, then proving cleanliness, is a research project. The plan admits KVM lands Phase 3, yet Summary claims 40–75 s push-to-merge unconditionally. Settles it: prototype slot, 100 starts, p50/p90 to first step.
4. **Native `velnor-worker` with "instant" <60 s `OfficialRunner` fallback.** Assumes only the launched binary differs. But the native worker also reimplements Twirp updates, `FeedStreamUrl`, 4 MiB `AppendBlob` logs, OIDC and the 39-var split; a wire change can break sandbox assumptions (env injection, cache URLs) the official binary then inherits. Legacy is the cited counterexample: 37k LOC native clones, 0 accepted speedup (E.7), incidents #670–#672/#389/#408. A config toggle contains only if the official path runs continuously, which is never scheduled. Settles it: weekly differential runs on real jobs before Phase 4 carries any required check.
5. **One workspace (−54.6% lock entries) speeds the typical PR.** 1,160→527 deduplication is real lockfile arithmetic; compile time follows critical-path crates, not entry count. Unification can *widen* rebuilds for leaf PRs, and `CheckAlone` (+1 s, #585 M4) exists because unification hides missing-feature breaks. Needs before/after builds of 20 representative PRs, not a lockfile diff.

### Factual errors

- **+5 min merge-queue wait charged to every merge.** Ruleset 24396608 (evidence C): `min_entries_to_merge: 1, max_entries_to_build: 2`, HEADGREEN. With minimum 1, a single ready entry does not sit out the timer — E §5 states this correctly. B overstates both baselines (33.8/141.2 min) and the saving from zeroing it. Real lever is `max_entries_to_build: 2`, mentioned only in passing.
- **Baselines are sums of medians from different cohorts.** 16.0 (11-17 PR, T5) + 12.8 (11-17 merge_group) added as stages of one PR. Medians do not add; cohorts differ; review/enqueue time is zero by construction. G's timelines and E's landed-commit check show that gap is minutes to tens of minutes. "Honest push-to-merge today" was never measured.
- **"1,000 workflow runs saturated the 20-job hosted limit."** T11 counts runs *created* 11-18 (CI 348, image candidate 306, qualification 267) and 86 queued (25 non-CI). No evidence ties a 20-concurrent limit to this org or equates run count with concurrency. G flags the public-repo exemption as open; B states it as fact.
- **CCD topology as measured fact.** "8 CCDs × 6 cores × 32 MB L3" asserted from the model string. B §1.6: CPU model, SMT, NUMA *not observable* read-only, in no owner doc. The pinning config (`reservedSystemCPUs: "0-5,48-53"`, 7 clean CCDs) breaks silently if enumeration differs.
- **State seal softened.** T9d: producer failed 16/18 (7 tip, 9 Clippy/checks); root/runner **0/18**. B's ancestor-or-equal promotion cures only the 7; the 9 mean the producer pipeline is red half the time. Deleting `state-seal.yml` sidesteps this.

### Missed bottlenecks or risks

- **Single tests floor the test stage.** T10 + both rivals: individual tests 271–534 s. No LPT sharding yields a 25–38 s stage containing a 534 s test. B's fixes (#1333/#1349 report 40–45% on specific tests) are asserted as 50–60 s/test → 3–6 s. Longest-test floor never named.
- **Queue throughput.** At `max_entries_to_build: 2`/HEADGREEN a third PR waits a full group slot. Raised to 5 in one bullet, never modelled at achieved service rate — the failure G quantifies (demand 7–8 merges/h vs ~1.9/h on 11-18). Minimum push-to-merge at peak is queueing, not latency.
- **Flakes under load.** Runner-host 3.7→8.8 min 11-16→11-18 (T6); ~10% of push runs fail on verified SHAs (G). B packs 35 warm slots with "coalescable" CCDs, no admission math showing 2 s nextest timeouts (`generator/`/`ci-helper/`) stop firing. Faster dispatch into saturation worsens flakes; each queue flake ejects a PR.
- **One coordinator, two sites.** Per-run site co-location is good; single `velnor-coordinator` + async `main` sync (~12 s DE→FI) has no stated stale-warmth or busy-site behaviour. E's per-site reserve and G's capacity header handle this better.

### Ideas worth taking

- **Path-mismatch root cause** (hosted `/home/runner/work/_temp` vs scale-set `/home/runner/_work/_temp` into the cache version hash, `velnor-new/local/crates/velnor-cache-rules/src/lookup.rs:28-32`; `StateCompat` quoting, `.../state_fragments.rs:15-21`). Best diagnosis in any proposal. Adopt verbatim.
- **In-sandbox `node24`, never native action clones** — legacy's 37k-LOC lesson learned, tarball tax (4.17 s/job) still removed. Right call on H1.
- **Ancestor-or-equal `main` promotion**, `0555` snapshots, per-PR write scopes. Correct trust shape; all three converge here.
- **Phase 1 two-pool split** (`velnor-fast` no DinD; `velnor-docker` 1 s probe): removes 3–4 s probe tax from ~90% of jobs (B §3.3) in a week.

### Verdict

No, not as stated. Diagnosis excellent; Phase 1–2 direction sound. But 40–75 s push-to-merge rests on unmeasured reflink hits, a fast-pass T7 shows fits ~16% of merges, test stages below the longest test, and a +5 min baseline error flattering the gain. Biggest flaw: **every load-bearing number after Phase 2 is an estimate stated with measured-number confidence**, so the plan cannot tell the week it starts missing.

## Proposal G

### Weakest assumptions

1. **Verdict cache is "a proof" via sandbox-enforced inputs.** Rests on Landlock deny-by-default ("kernel 6.12; to verify on the nodes"). Nodes run 6.12 (B §1.1) but ABI, build-script/proc-macro/`rustc` probing and subprocess coverage are unverified, and Docker/network suites — the hardest — are exempted. One leaked read returns wrong greens, the forbidden failure. Settles it: run runner-host/actions-cli under enforcement on a worker, count undeclared-read failures; a large count makes recording-mode remediation the schedule.
2. **≥90% verdict hits from PR mix.** Keyed by executable digest + read set + environment + image. Lockfile PRs (10.5%), toolchain/profile/fixture changes bust whole suites; interleaving (34/45 behind base, T7) means merge-group digests routinely differ from warming PR digests. 2% re-execution misses crate-specific read-set errors until merged. Settles it today: for 200 merges, fraction of merge-group binaries bit-identical in the prior PR run.
3. **L2 "falls back to L1 automatically".** After acquisition the worker owns completion; there is no hand-back. Mid-job discovery means failing the job or rescuing the official Worker (settings, credentials) into the same sandbox, eroding the key-isolation claim. E names this ownership problem; G waves at it. Settles it: from 200 runs' logs, show 100% of out-of-profile steps are routed *before* acquisition.
4. **Elastic slots remove contention without queueing.** `X-ScaleSetMaxCapacity = free slots` queues at GitHub across *all* repos in the group (velnor-new, lab, ruxel, tuiscotti, termpane; B §3.2). ~15 heavy + 5 medium slots, merge groups needing several slots in one site: head-of-line blocking recreates the queue inside the dispatcher. "One busy slot on average" is the averaging that failed 11-18 (T8 medians 46–110 min).
5. **Slow suites yield on schedule.** §5.5 budgets runner PRs ~2 min after #1333/#1349 plus a −66% profile change (owner question 5, undecided) whose PRs measure 40–45% on particular tests; the 10 s/test budget means rewriting dozens of >120 s tests. A multi-month test program presented as a phase dependency.

### Factual errors

- **Free-plan 20-job limit as mechanism.** `gh api orgs/tailrocks` → free + documented limit, with G conceding the public-repo exemption is unstated. §1.1 still frames the wait as self-inflicted hosted exhaustion without measuring concurrent hosted jobs. T11 supports contention (86 queued, all hosted), not the specific limit. The fix does not need the overclaim.
- **"Heavy jobs ran in 97.4% of PR runs" unreconciled.** Matches T3 sample (77/77) but never defines "ran" against E's 25-executed/39-listed and T2's 0 s jobs (`local-mode` 59/77). A 3 s job counts like a 500 s one, overstating cost share. The 12-job one-file `ci-helper` change proves selection failure alone.
- **347 runner-min/merged PR** multiplies medians and *assumes* 15 min × 1.28 cancelled runs. Evidence excludes cancelled runs from runner-min explicitly (45% of PR runs uncounted). Direction (~3× per-run) right; 347 is not measured and must not anchor "≤12 per merge" (29×).
- **Slot geometry.** "8 L3 groups of 6 [M cores; grouping to verify with `lscpu`]" — [M] covers cores only; the grouping driving elastic slots is unverified (B §1.6: NUMA unobservable). Better flagged than B; still load-bearing.

### Missed bottlenecks or risks

- **Queue at target latency.** G alone measures the queue (402 min median 11-18; ~2 merges/run; demand 7–8/h), then targets P50 ≤3 min without showing drainage at `max_entries_to_build: 2` (owner Q6 undecided) + HEADGREEN + flake ejection. Per-run speed buys less push-to-merge than claimed if 2 stands.
- **Flakes treated as capacity only.** 10/96 verified-SHA push failures may be genuine nondeterminism surviving an idle cluster. Cpusets fix the 2 s-timeout subclass; no rerun analysis splits classes. If half are real, the verdict cache *caches* flaky passes.
- **Gate lane contention.** Plan/verdict/Required for every repo funnel to reserved slots on velnor-237 (20 threads). At ~90 s PRs, gate throughput sets fleet capacity. Unsized.
- **Legacy optimism.** "1,152/1,153 tests pass" on a loaded M5, selected modules. Pack full run: 8,172/8,222, 50 macOS failures (E), and *no native-runner performance numbers anywhere*. Reusing `protocol.rs` is reasonable; it does not de-risk parity.

### Ideas worth taking

- **Push-to-merge timeline method** (GraphQL, 200 merges, queue-add vs merge). Reframes the problem: on the bad day the queue is the product. Replace every other baseline with it.
- **Flake metric**: push failures on green-merge-group SHAs (10/96). Cheap, permanent, same-confidence. Adopt as a phase gate.
- **Capacity-header admission + L0→L1→L2 ladder with line budgets** (≤8k/≤15k/≤12k). Only credible anti-bloat mechanism in the three.
- **Enforced-inputs direction** for selection: skipping as proof beats planner guesswork, whatever enforcement mechanism survives verification.
- **`opt-level=3` deps in test profile** (142.5→47.9 CPU-s slowest class, measured). Largest single-suite lever anyone measured; correctly an owner decision.

### Verdict

Closest to the truth on where time goes; only proposal measuring push-to-merge. Reaches the minimum *only if* sandbox enforcement holds — unverified on the nodes and exempted for the suites hardest to speed up otherwise. Biggest flaw: **the ≤3 min end-state is gated on a mechanism the proposal itself marks "to verify"**, with no fallback target if enforcement is porous; without the verdict cache its own numbers leave PRs at 2–6 min plus a second group run.

## Proposal E

### Weakest assumptions

1. **12.0 min replay as baseline.** Assumes moved gates queue at the cluster's 12 s median with no added contention. E's own data warn: fresh merge-group cluster queue p90 already 118 s, workers at 93%, durations inflate 2.4× under load (T6). Reserves are proposed, but 12.0/14.1 min headlines Phase 1 before that condition holds. It is a lower bound, not an expectation. Settles it: the Phase 1 canary.
2. **85–96% combined compiler hit rate.** Denominators exclude Cargo-fresh units and count "eligible unchanged-unit lookups" — unmeasured, since nothing crosses jobs today (0 cross-job hits sampled; G: 2,106 builds/475 versions). T9a's 40–74% is within-job pass-to-pass and cannot extrapolate across differing paths/features/profiles. The 26–51 s saving is arithmetic on an assumed numerator. Settles it: G's `generator` experiment (22.5 s→8.8 s) repeated on runner-host.
3. **Multi-minute tests fixed as a parallel precondition.** A 271–274 s test exceeds the whole 50–120 s test budget. Journal templates, fixture-by-digest, case splitting: plausible, unmeasured end-to-end; #1333's 17.9→10.3 CPU-s is 42% on one operation. Phases 3–4 exits (p50 ≤4 min) silently assume the condition.
4. **Two fresh verifications are affordable.** Retaining full PR + merge-group execution, 0% reuse at rollout, gives 4.5–10 min machine time *with an available queue slot*. At build-concurrency 2 and 11-18 arrivals, an available slot is the exception. No queue model at own service rate — B's sin, quieter.
5. **Fenced journalled control plane without recreating velnor-host.** Persisted reservations, acquire-intent states, fencing epochs, quarantine, crash tests per mutation: the machinery whose 62.7k-line journal rivals cite as what not to copy. No size budget (contrast G's caps); Phase 5 native comparison starts only atop this foundation. Risk: a third multi-year runner whose first win (routing) needs none of it.

### Factual errors

- **"Pack conflates job records with execution" (25 executed/39).** T7 explicit: "all 39 jobs execute… median 39 for each event", counting timestamped jobs incl. 0 s. E's fresh cohort counts non-skipped conclusions differently and calls the pack wrong instead of differently defined. If 14 are condition-skipped, selection works better than G claims; if 0 s no-ops, they still pay dispatch/reporting. Never reconciled job-by-job, so selection savings cannot be sized. G/T3 (heavy 77/77) is the careful reading.
- **State seal without the split.** Counts right (6 generator, 4 ci-helper, 0 root/local/runner; T9d) but "tip advancement and producer failures" omits 7-vs-9, which decides whether E's manifest redesign fixes the dominant half. B is sharper here.
- **Baseline hygiene.** Inherits pack cohorts (26.7/92.3) without T2's success-only split (27.7/79.2); fresh cohort (24 PR runs, completion-weighted, 6 failures) quoted to 0.1 min. Minor, but this proposal's virtue is measurement hygiene.

### Missed bottlenecks or risks

- **Other hosted workflows.** Routing moves CI's 8 jobs; Source qualification (~2,400 hosted-s/run per PR event, G; 267 runs 11-18, T11) and image candidate (306) still compete for the pool serving release/fork lanes. 11-18 repeats at smaller amplitude.
- **No flake budget.** Exit criteria lack any flake metric. At ~10% verified-SHA failure (G), full merge-group re-execution *maximises* flake exposure at the worst point: queue ejection. Needs a rerun policy; has none.
- **Judged at Phase 1.** Routing target (p50 ≤13 min) is quiet-day performance made reliable. Worthwhile, but if the program is judged there, the costly phases (control plane; native at ≥30 s/≥15%) never clear their gates — H2/H3 answered "no" by attrition, fleet left on ARC with better requests.
- **Site failure mid-run.** Single control plane deferred to later quorum; valid leases finish, new claims stop. DE control loss strands FI capacity when most needed. B's async sync and G's standby dispatcher name degraded modes; E does not.

### Ideas worth taking

- **DAG critical-path method** (last prerequisite per join; Ledger CLI starts path 8/18, maintenance-helpers ends it only 4/18 fresh vs 34/51 historically). Corrects "longest job" intuition; exposes the 52 s helper-building Ledger CLI as a first-class target.
- **Trust model**: same-repo PR and merge-group code is untrusted; a job never promotes its own bytes or signs its own success. Strictest boundary of the three (vs B/G merge-group writes needing owner exceptions). Adopt as invariant.
- **Executor independent of scheduler, promoted only on paired win** (≥30 s p90 or ≥15% verified jobs/h). The honest form of H2; keeps ARC+official as measured control. Adopt the gate under any architecture.
- **Placement by predicted finish** (wait+provision+transfer+residual; warm node waiting 20 s loses to idle). Only rule degrading gracefully when warmth and load disagree.
- **Exact merge evidence for push consumers** (provenance + obligation identity; full path when incomparable), preserving exact-SHA release gates. Captures most push saving without tree-equality shortcuts.

### Verdict

Yes on confidence, no on minimum time. Only proposal never buying speed with unproven equivalence; its gates would catch what B and G ship. But two full fresh verifications + 0% reuse floor push-to-merge at 4.5–10 min before queueing, which B/G plausibly beat *if* reuse validates. Biggest flaw: **it never tests whether the expensive duplicate — merge-group re-execution — can be safely retired**, optimising for never being wrong into a reliable second place.

## Cross-cutting

**Agreement, evidence-backed.** (a) 11-18 was hosted gate queueing: 57/71.8 min (T5), 86 queued all hosted (T11), cluster queue 0.2 min (T4). Moving gates first is unanimous and correct. (b) H1 fails: interpreter cost 0.5–2% by three accountings (B ms-logs, G 0.6%, E ~9 s/494 s); win is locality, not language. (c) Cache failures are identity defects: 48/48 misses, 3,636/3,636 skips (T9a/b); B's `_work`/`work` root cause sharpest. (d) Push re-run is waste: 120/159 identical SHA (T7). (e) Contention bimodal: 0–1% vs 92–95% (B §2), runner-host 3.7→8.8 min (T6), 83.7 cores on 2-CPU request. (f) blaze ineligible under pinned 1.99.1 — unanimous, from blaze's own docs. (g) ARC pod-per-job + privileged DinD + 5 s probe taxes non-Docker jobs.

**Conflicts, and what evidence supports.**

| Question | B | G | E | Evidence supports |
|---|---|---|---|---|
| Push-to-merge today | 33.8 / 141.2 min, sums of medians | 402 min median 11-18, timelines | 44.3 min, n=12 landed commits | G's method; B invalid, E a pilot |
| Jobs executed per PR run | Assumes 39 | Heavy jobs in 97.4% | Median 25 of 39 | Pack T7/T3; E unreconciled |
| mbx | Retire for cargo + reflink | Keep as compile layer | Qualify; sccache as rival | Unsettled; needs runner-host A/B |
| Test-result reuse | Tree receipt only | Verdict cache, enforced | 0% at rollout | E today (T7: 15.6%); G if verified |
| Merge-group after PR | 8 s / affected-only | Cached full run <1 min | Fresh re-execution | Unmeasured — largest open number |
| Native worker | Phase 4, flag fallback | L2 last, budgeted | Only on paired win | E/G; legacy refutes B (E.7) |
| 5 min queue wait | Every merge | Not charged | Refused (min = 1) | E, ruleset 24396608 (C) |

In prose: the mbx dispute is the clearest case of two true measurements
pointing opposite ways. B's #585 result (plain cargo 0 crates in 0.3 s warm;
every mbx variant recompiling 10–68) was taken on the workflow B wants to
replace; G's concurrent-build result (two cold builds at 1.07× one) was taken
on `generator`, the smallest workspace. Neither ran runner-host, the suite
that sets the path. Likewise the executed-jobs dispute is definitional, not
factual: the pack counts timestamped job records including 0 s jobs (T7
method note), E counts non-skipped conclusions in a fresh cohort. Both can
be true; only G's runner-minute weighting (heavy jobs 77/77 in T3) tells the
owner what selection failure actually costs.

**Hypotheses re-judged.** H1 *fails* (unchanged; ≤2% three ways). H2 narrows to *partly holds*: replacing ARC dispatch/DinD is supported everywhere (G 14 s pod/job, T4 12 s, B probe accounting); replacing the Worker is supported nowhere — no controlled native-vs-official comparison exists, legacy argues against doing it early. H3 *partly holds, weaker*: all three keep k3s for lifecycle, remove it from the per-job path — plumbing, not scheduler of record. H4 *holds* (strengthened by B's diagnosis); winning mechanism (reflink vs mbx vs site tier) undecided. H5 moves to *holds in principle, unproven in every mechanism*: tree-match fits 15.6% (T7), enforced inputs unverified, planner already skips yet heavy suites run. That gap, plus the 271–534 s single-test tail, decides 1 minute vs 5.

**What none of the three solves.** First, the longest single test. Every
end-state number in every proposal is hostage to tests of 271–534 s that no
sharding, caching or dispatch improvement touches; only test engineering
does, and only G budgets it as a rule (10 s/test) while making its targets
depend on it anyway. Second, merge-queue throughput at the target service
rate: all three optimise per-run latency, only G measures the queue, and
none shows the queue draining at `max_entries_to_build: 2` under 11-18
arrival rates. Third, confidence accounting for flakes: a ~10% failure rate
on verified SHAs (G) means "same confidence" is already not today's
confidence on any given merge, and no proposal sets a flake budget the
faster system must meet.

**Net recommendation.** Ship the shared Phase 1 now (cluster gates, no-DinD
pool, push de-duplication, `max_entries_to_build` review). Adopt E's trust
model and promotion gate, G's push-to-merge/flake metrics and capacity
admission, B's cache root-cause fixes. Fund two experiments before choosing
an end-state: runner-host warm-build A/B (reflink vs mbx) and merge-group
digest-overlap measurement. Both are weeks away and decide G's verdict
cache vs E's fresh re-execution more reliably than another proposal round.

---

## Key (added by the manager after the review)

- Proposal B: Gemini 4 Carbon
- Proposal G: Claude Fable 5.2
- Proposal E: GPT-6.2-Sol

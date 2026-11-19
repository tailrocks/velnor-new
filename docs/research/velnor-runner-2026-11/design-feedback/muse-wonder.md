# Design feedback by Muse Wonder

| Field | Value |
|---|---|
| Model | Muse Wonder |
| Model id | `muse-wonder-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

# Design feedback: Velnor runner architecture (draft of 2026-11-19)

Reviewer: Muse Wonder 1.0. Arch:line = draft architecture, D:id = decision log, §A/§B/§C = evidence pack; code at 95de9b704b. Direction: ambition first, same confidence.

## Verdict

No: committed phases reach neither the minimum time nor provable same confidence; only stretch phase 4-5 aims at ~1 min, on unproven M4/M5.
Ambition 5/10: queue/cache/dedupe are well attacked, but every hop-deleting mechanism (closure receipts, short-circuit, speculation, preflight) is stretch or gated to phase 4+.
Soundness 6/10: the queue/cache core is evidence-backed, yet the ledger, hot Worker, and audit as written can silently turn a red tree green (H1-H4).

## Correctness and confidence holes

**H1. Exact-tree keys ignore event context (Critical).** Key = tree + definition
digest (Arch:299-300); C2 allows it alone in phases 0-3 (Arch:50-53). A merge-group
run and PR run of one tree still differ in ref, event payload, PR number, token
context, and required obligation set; four reviewers in the comparison note an
equal tree does not show the queue's checks ran. C5 says event-readers run fresh
(Arch:61-63) but nothing in phases 0-3 classifies event-reading; the only detector
(fanotify audit) arrives in phase 3 as telemetry (Arch:341-344). *Fix:* include
event class, ref kind, and allowed event-field digest in the key; obligations are
event-sensitive by default until an enforced run proves purity; merge-group reuse
requires the queue obligation-set digest to be a proven subset of the PR run's.

**H2. Job-reported receipts mint cross-run trust (Critical).** C4 lets a job's own
report back an exact-tree receipt (Arch:57-60). PR code influencing its report (bad
dependency, build script, report-writing test) can then bank a pass its merge group
reuses while executing nothing (Arch:323-324). Today a job can fake only its own
run. *Fix:* only agent-observed results (agent started the processes) create
receipts other runs consume; job reports affect only their originating run. Add a
lab negative test: a forged passing report must yield zero reusable receipts.

**H3. Closure keys omit most observable state (Critical, phase 4).** Key = binary
digest + declared inputs + definition (Arch:301-304); enforcement is a mount
namespace with binary, inputs, toolchain, private /tmp, no network (Arch:330-335).
Tests also observe env, argv, cwd via `..`, hostname, CPU count/features, /proc,
umask, rlimits, timezone, enumeration order. None is keyed or contained. M4 accepts
80% enforcement pass (Arch:607-610) — 20% failing proves declarations routinely
incomplete, yet the 80% would be trusted; cross-node bit-identical binaries
(Arch:607-608) are unmeasured, with embedded absolute paths the obvious breaker.
*Fix:* key the full execution fingerprint (env allowlist, argv, cwd tree, rlimits,
CPU-feature mask, kernel/image digest); require 100% enforcement pass for closure
admission; run the two-node digest test in week 1 (T2), not phase 4.

**H4. Audit sampling cannot catch a false green pre-merge (High).** C6 re-executes
2% exact-tree / 10% closure reuse (Arch:64-67). At 200 merges and 1% wrong
receipts, expected detections = 0.04; a systematically wrong obligation lands
dozens of times first, and nightly/every-20th runs (Arch:65-66) detect only after
landing. *Fix:* shadow at 100% re-execution, graduate per obligation kind after
~3,000 clean reuses (rule of three: <0.1% false-green at 95% confidence); any
disagreement retires that key version permanently, not just "that kind" (Arch:66).

**H5. Receipt short-circuit makes the dispatcher a green-mint (High).** The
dispatcher "completes the job itself through completejob… no pod" (Arch:234-237) —
called the largest payoff, then deferred as a phase-4 owner decision. A dispatcher
bug/compromise can green any job with no Worker; plan outputs feeding `needs` add
stale-plan risk from non-tree inputs. *Fix:* short-circuit only non-plan jobs fully
covered by agent-observed receipts (H2); independently re-derive the obligation-set
digest from the released plan; log the receipt chain in the check; keep a permanent
5% execute-anyway sample for short-circuited jobs.

**H6. Hot-Worker rotation races its job (High).** Sandbox keeps a Worker waiting
and "replaces it every 25 s" (Arch:230-233) vs the Worker's 30 s wait
(`Worker.cs:23`). A message at second 24 leaves 1 s before rotation kills the
Worker that accepted it; hand-off writes `.runner` then pipes the message
(Arch:192-196) with no ack the Worker read it (`JobRunner.cs:47`). *Fix:*
generation-tagged handshake — no rotation after dispatch until first-step ack or
30 s expiry, jittered per pod; M3's experiment (Arch:601-605) must include
kill-at-the-boundary fuzz, not 50 happy-path timings.

**H7. File-read audit cannot make selection retroactively sound (High).** Fanotify
sees opens, not directory steering, negative lookups, build-script probes, or
already-mapped files; worse, it observes executed obligations, while a
selection-missed file belongs to one that never ran, so absence proves nothing.
Two weeks of such shadows (Arch:428-430) measure the selected set only. *Fix:*
audit is telemetry; merge-group selection ships with declared inputs, the full-run
backstop (Arch:431), and a standing shadow running *skipped* obligations on sampled
merge groups; one unexplained disagreement disables selection for that obligation.

**H8. Warmer duplicates the scarcest resource, off the books (High).** Every node
rebuilds latest main per Cargo root and freezes a generation (Arch:214-216): 5
workers × 5 roots on a cluster sampled at 92-95% CPU (§B.2.1), pods bursting to
83.7 cores (§B.2.2). Warmer/speculation/audit CPU is never charged to the ≤40
runner-min target (Arch:108), so it can be "met" by moving work off-ledger. The
≥95% fresh claim (Arch:370) rests on 0-crate compiles in 3 lab runs (D:I036) —
from a 5-crate repo; the one velnor-new run was slower warm (345 s) than cold
(291 s) — and cargo fingerprints embed paths, so a differently-pathed reflink
clone can invalidate the generation. *Fix:* build the seed once per site, ship as
reflink seeds, refresh per node on delta only; work item 6 (Arch:691-694) runs on
`velnor-runner-host` including the changed-path case; all section-6 targets charge
warmer+speculation+audit CPU to the causing merge.

**H9. Depth 8 is gated on the wrong rate (Medium).** Depth 8 "once false failures
≤2%" (Arch:460): P(≥1 failure in 8) = 1−0.98⁸ ≈ 14.9%, each ejecting/rebuilding
the suffix; at today's 8.1% (D:S1: 10/124 on green SHAs) ≈49%. Genuine failures
count too (3/40 queue exits, D:S2). *Fix:* gate depth on simulated merges/hour fed
real arrivals and total (flake + genuine) failure rates; adopt 8 only if it beats
4 by >20% in simulation.

**H10. Build pods cannot be scheduled as sized (Medium).** 16 CPU, 32 GiB request=limit, "at most 4 running" (Arch:511-516) requests 128 GiB on ~125.5 GiB-allocatable workers (§B.0), before the system reserve (Arch:518). *Fix:* 3 pods at 32 GiB or 24 GiB requests (peak 15.7-16.8 GiB, §B.2.2); print the arithmetic.

**H11. Phase gates cannot verify their targets (Medium).** "At least 50 merges" (Arch:655) gives ~5 points beyond P90 and 0.5 expected events at the 1% budget (Arch:109). *Fix:* n≥300 or 4 weeks, Wilson intervals beside each P50/P90.

**H12. Two verdict writers can race in phase 4 (Medium).** Workflow emits both required names (Arch:418-420) while the App also posts `Required` (Arch:487-488): slow job vs eager App can disagree on one obligation set; forks (Arch:253) have no App path. *Fix:* one writer per repo/phase behind a flag; App shadows two weeks first.

**H13. Cancelled-run waste is unpriced (Medium).** 45% of PR runs cancel (§A T7:
1,156/2,558); the 285-347 runner-min baseline (Arch:35) includes them, yet the
only lever is instant slot release plus a rejected debounce (D:I074). Three PR
runs at 52.9 runner-min (Arch:34) already cost ~159 of the ≤40 target (Arch:108).
*Fix:* report runner-min split completed/cancelled/superseded; dispatcher-level
15 s same-head offer coalescing cancelling the older acquisition pre-hand-off;
re-derive the target from that accounting in phase 1.

## Where the design is too timid

The draft tunes hops (hot Worker, warm pods, reflinks) but defers every
hop-*deleting* mechanism to stretch phases behind gates. Each item below keeps
C1-C7: nothing skips unless an identical, agent-observed, enforced result exists.

**T1. Native obligation executor in phase 1, not a phase-5 maybe.** The Worker is
executor "in every phase this document commits to" (Arch:227); native is funded
only if a hot Worker leaves >15 s/run or >10%, in phase 5 (Arch:238-243; D:I005/
I007). *Bolder:* from phase 1 the dispatcher runs velnor-gen-profile obligations
natively via `velnor-ci` in the sandbox (checkout, cargo/nextest/lint/doc lanes,
step status via Results); Worker stays for arbitrary `uses:` jobs, chosen
pre-step in the same identity (no lease moves, Arch:241-243). *Numbers:* draft's
own per-job costs sum to ~20 s (pod ~6 s [M, Arch:507], actions ~4.4 s [M,
Arch:369], tools 10 s med/18 s p90 [M, Arch:366]); removing only the Worker/action
layer (~8 s/job) over 25 executed jobs saves ~200 s/run and ~16 s on the
plan+heaviest-root critical path; the honest unmeasured range is 0.5-20 s/job —
measure in week 2, not month 5. *Risk:* env/post-step/cancel drift. *Confidence:*
Worker is oracle; adopt per obligation after 100 paired shadows, zero diffs
(D:I007 gate, phase 1). *Experiment (days):* checkout + one cargo lane for the
`generator/` root on velnor-lab, 20 paired runs; kill if <3 s/job or any diff.

**T2. Closure receipts: phase-2 committed, not phase-4 stretch.** Stretch today
(Arch:326-327; default D:I041). *Bolder:* M4's experiments (two-node digests,
namespace-enforced suites, 200-merge replay, Arch:610-614) run week 1 on
`velnor-runner-host`; obligations passing at 100% (H3) enter closure reuse in
phase 2 under H4 shadow graduation. *Numbers:* merge group = 64.6 runner-min
(Arch:34); at the draft's 50-88% closure estimate (Arch:373; 88% replay of 97
groups), second verification falls from ~90-150 s execution to ~5-10 s lookup,
saving ~32-52 runner-min/merge before other wins. *Risk:* nondeterminism,
incomplete declarations → false greens. *Confidence:* enforced namespace + full
fingerprint (H3), 100% shadow until ~3,000 clean reuses per kind; C5 never
reuses. *Experiment (days):* the three M4 measurements on one root; kill per
suite on unpinnable digest differences.

**T3. Speculation + webhook-start in phase 1, not phase 4.** M5/N09 are phase-4
(Arch:616-622) though a wrong guess "wastes idle capacity and never changes a
verdict" (Arch:619-620). *Bolder:* on each push webhook and main advance, verify
`main ⊕ PR` for top queue candidates at batch priority (cap 10% cluster CPU,
`tree/` receipts), and start plan+build on the webhook so arriving jobs find
receipts (Arch:583-585). *Numbers:* GitHub floor = 19 s enqueue→run + 26 s
finish→merged (Arch:490-491) = 45 s no runner removes; pre-verified candidates
collapse the merge-group run to the ~30 s receipt case (Arch:492) vs a ~150 s
re-verification; at 50% prediction hit, expected saving ≈ 0.5×120 s = 60 s/merge
for capped idle CPU. *Risk:* wasted batch work; merge-tree divergence from
HEADGREEN construction. *Confidence:* receipts keyed by exact tree; a mismatch
is a miss. *Experiment (days):* shadow predictor now (mirror `git merge-tree` vs
real merge-group trees, Arch:620-622) while executing speculatively in parallel.

**T4. PR-light / queue-full as default, not a phase-4 experiment.** N15 is
experiment-gated (D:N15), mentioned only as a fallback (Arch:593-595). *Bolder:*
phase-2 default — PR pushes run plan, format, and proven <30 s obligations; the
merge group (still the gate) runs the full selected set. *Numbers:* PR 52.9 +
merge-group 64.6 runner-min today (Arch:34); PR-light at ~15 cuts the pair to
~80 (−32%) and returns feedback in ~30-45 s, vs the thorough run paid 3×/branch
(median 3 runs/branch, §A T7). *Risk:* failures surface in-queue where ejection
rebuilds the suffix (H9); raising queue failure from 3/40 (D:S2) toward 15% can
cut merges/hour despite faster PRs. *Confidence:* nothing lands without the full
selected set in the merge group; nightly/every-20th full runs unchanged
(Arch:431). *Experiment (days):* replay 97 recorded merge groups under PR-light
sets; simulate merges/hour at 7.5%/12%/15% failure; adopt only if 12% still
beats today's throughput.

**T5. Real headroom: a merge/gate lane + burst, not equal sharing.** Other orgs
share at equal PR priority (Arch:530-532) and more workers are rejected until
wait p90 >15 s after phase 3 (Arch:785; D:I105) — at 92-95% sampled CPU
(§B.2.1), where wait scales as ρ/(1−ρ): ~19× service at 95%, ~2.3× at 70%.
*Bolder:* cordon one worker (96 threads) for merge+gate with guaranteed
admission; burst overflow to rented warm nodes; other orgs keep quotas on four
workers. *Numbers:* moving merge/gate work 95%→70% cuts its queue multiplier
~8× (19→2.3), the difference between committed wait ≤5 s p90 (Arch:104) being
credible or aspirational at peak. *Risk:* one fewer PR/batch worker; cold burst
nodes. *Confidence:* capacity changes only waiting, never what runs; burst nodes
execute identical obligations from site seeds. *Experiment (days):* cordon one
worker for merge/gate classes 3 days under ARC labels; compare Velnor-side wait
and duration p90/p50 vs prior week. No new code.

**T6. One acquisition, fanned tests — delete root-job hops now.** One GitHub job
per root (Arch:412-414) repays assignment/pod/checkout/setup per hop; the cluster
test executor is phase-4 stretch (Arch:442-445). *Bolder:* one dispatcher
acquisition per run; binaries fetched once to the site store; agent fans cases
same-site longest-first (N04) from phase 2 where arithmetic passes.
*Numbers:* 781 ci-helper tests: 55 s on one forced thread (Arch:479) vs 12 s on
8 threads (Arch:487-488); blocker is 1.48 GB at ~12 s today vs ~1.2 s at
10 Gbit/s (Arch:782) — a network measurement, not a rewrite; deleting 2-4 hops
at ~9 s (6 s pod + ~3 s start) saves ~18-36 s/verdict. *Risk:* partial fan-out,
aggregation bugs. *Confidence:* every expected case must report exactly once or
the job fails (Arch:443-444); GitHub sees one job/verdict. *Experiment (days):*
iperf3 same-site + one-suite fan-out on velnor-lab; kill per M6's own criterion
(Arch:624-629) if summed time ≤ longest test + transfer.

## The hardest problems

**P1. The 258-274 s single-test floor.** Loops of 40 and 6 cases
(`image_publication_public.rs:85-92`; `shape.rs:97-106`; D:S8) floor any verdict
near 4 min; sharding cannot split one test. Draft: split loops, template DB,
≤60 s / ≤15 s (Arch:435-437). *Attack:* price fixture, not cases. (1) Split per
case *and* share one pre-migrated template per suite via XFS reflink clone per
case (ms, not migration replay). (2) Run the 46 cases as nextest cases across
the T6 executor longest-first; floor = slowest case + clone, with per-case
fixture cost measured before committing (the comparison's caveat). (3) Journal
schema: validate once per schema-hash per binary, linear not quadratic (#1333),
with a committed benchmark. (4) Block merges on tests >30 s from phase 2
(Arch:438-439, sooner), budget falling 30→15→10 s. *Kill:* a case with
unclonable shared state sets its suite's stated floor; do not hide it in a P50.

**P2. Zero cold start, including GitHub's unmeasured latency.** Committed
assignment→cargo ≤5 s (Arch:105), ≤1 s stretch, but push→offer is "not
measured" (Arch:476) and one run waited 497 s before GitHub created its first
job record. *Attack:* (1) Three clocks from day 1 (webhook→JobAvailable,
→acquire, →first step), P50/P90 each; the SLO names GitHub's segment so it is
escalable with data. (2) Webhook-start (T3) overlaps that latency with real work.
(3) Velnor-side budget: pre-created pod 0 s + clone 0.5-2 s (work item 6) + hot
handoff <0.5 s with the H6 handshake. (4) If push→offer P50 >20 s despite
webhook-start, restate publicly as "≤60 s after first offer" and file the
measurement with GitHub (M1's kill rule, Arch:583-586, as a reporting duty).

**P3. Cross-event verdict reuse without trusting anything unproven.** Exact-tree
reuse is sound but rare: 0/40 (worst day) to 11/60 merge groups tested the PR
tree (D:S4) because main moves during long waits. *Attack:* make reuse common
by construction. (1) Shorten PR runs (T1/T6), shrinking the move window.
(2) Speculatively verify the moving candidate (T3). (3) Graduate closure reuse
per obligation under enforcement + shadow (T2/H3/H4), reusing the 50-88% whose
closure did not change. (4) The verdict is a mechanical coverage check: Required
passes iff every selected obligation holds an agent-observed receipt *for this
run's full keys* (tree/closure + definition + event class + fingerprint) or
executes now. Publish weekly reuse rate, shadow disagreement rate, and
post-merge failure rate — the three numbers proving confidence did not move.

**P4. Placement and zero queue on a saturated shared cluster.** GitHub binds no
job to a node (`generatejitconfig` has no job id; §F.1.3; D:(a)); three orgs
share at 92-95% CPU (§B.2.1), so ≤5 s p90 wait (M9) is not an algorithm.
*Attack:* (1) Own acquisition with per-class identity pools so PR bursts never
consume gate/merge identities (Arch:208-212). (2) Make placement matter less:
site-seed warmth everywhere (H8 fix), PR scope as preference, idle-cold beats
busy-warm on predicted finish (Arch:217-220). (3) Create headroom (T5); batch
work strictly preemptible and capped. (4) Honest capacity advertisement
(Arch:172-175) queues excess at GitHub visibly; replay the busiest recorded hour
(Arch:650-651) before phase 3, not after.

**P5. CI before the push; a queue that never re-verifies.** The endpoint —
≤20 s after a preflighted push (Arch:103), push-to-merge P50 ≤2.5 min
(Arch:662) — needs verification overlapping editing. *Attack:* (1) Preflight API
in phase 2, not 5: agent commits trigger `velnor-ci preflight <tree>`; Velnor
verifies `main ⊕ commit` and banks `tree/` receipts (Arch:640-642). Mac-local
runs stay rightly rejected (arm64 vs linux/amd64, Arch:642-643) — preflight
replaces them with cluster execution of the identical tree. (2) Agent workspaces
on cluster-adjacent reflink storage, so preflight pays only the delta build.
(3) On push, the PR run is a receipt lookup plus event-sensitive obligations
(H1), posting in seconds. (4) Instrument the edit→push window first
(Arch:643-645): under ~30 s, preflight cannot pay and webhook-start (P2) is the
honest floor; then delete phase 5 instead of deferring it forever.

## Decisions you would overturn

**D:I003 (dispatcher phase 3) → lab phase 1, production phase 2.** ARC queues
fine (0.2 min, §A T4) but fork refusal pre-pod (N11), key-out-of-sandbox,
short-circuit, and merge-first admission all need owned acquisition; building
phases 1-2 machinery on ARC (Arch:660) then discarding it wastes the quarter.
The seam is proven (Worker needs `.runner` + message; job-token renewal, D:S6).
Pull work item 9 forward; ARC stays the labelled fallback (Arch:665).

**D:I005+I007 (native gate >15 s/>10%, phase 5) → build phase 1, gate per obligation.**
The gate is right, its timing wrong: the number is unmeasured (0.5-20 s/job) and
the committed stack spends ~20 s/job on deletable hops (T1). Run I007's 100
paired / zero-diff test in phase 1 on `generator/`; adopt per kind. Deferring
strands the design if the answer is 15 s.

**D:I041+N02 (no cross-tree reuse phases 1-3; closure phase-4 stretch) → committed phase 2.**
I041 answered the panel's unsound mechanisms (traced reads, unenforced digests);
N02 answers them (output-addressed, enforced, audited) and the prize is 50-88%
of merge-group test obligations (Arch:373). Keeping the second verification —
the largest removable cost after the queue — until weeks 12-20 forfeits it for a
quarter. M4 week 1 (T2); closure from phase 2 for 100%-enforced obligations,
H4-graduated; exact-tree stays the per-obligation fallback.

**D:I054 (merge-group selection: owner decision, phase 3) → phase 1, skipped-set shadow.**
`docs/goals.md:86` already defaults to selection; hard decision 3 is the
exception. The draft's precondition (fanotify audit + clean shadows,
Arch:428-430) cannot prove safety (H7: it observes executed work). Stronger and
earlier: selected set gates from phase 1; skipped obligations run as
non-blocking shadow on 20% + nightly/every-20th (Arch:431); one disagreement
disables that obligation's selection. A docs-only merge group then costs
plan+verdict vs 64.6 runner-min (Arch:740) from phase 1.

**D:N06 (short-circuit phase-4 owner decision) → committed with the dispatcher.**
The draft calls it the largest payoff (Arch:236-237) then makes it optional and
late; with H2 agent-observed receipts, independent obligation-digest
re-derivation, logged receipt chains, and a permanent 5% execute-anyway sample
(H5), a fully-receipted job finishing in ~1-30 s podless (Arch:733, 746-747) is
what makes push-to-merge ≤4 min reachable. Late decision = late headline.

**D:N15 (PR-light phase-4 experiment) → default phase 2, simulation-gated.**
Queue throughput is simulable today from the 97-group replay at failure rates
bracketing 3/40 (D:S2); waiting until phase 4 costs ~117→~80 runner-min/merge
for no new information (T4). Adopt if simulated merges/hour at 12% queue
failure beats the full-PR baseline; else record why not.

**D:N17 (distributed compilation rejected) → reopen for cold builds only.**
The 170 s serial chain of a 696 CPU-s cold build bounds one chain, not a
workspace cold build after lockfile/toolchain changes (10.5% of PRs, Arch:367),
where independent crates can compile across site nodes; the cold path sets P90.
Trial site-shared content-addressed compilation for cold builds only (no second
wrapper on the warm path, D:I109). Kill if the serial chain dominates there too.

**D:I105 (no more workers for now) → rent burst now, buy on the draft's trigger.**
At 92-95% CPU (§B.2.1) no scheduler yields ≤5 s p90 wait (Arch:104; T5
arithmetic). One rented merge/gate-lane node for phases 1-3 tests headroom vs
placement as the binding constraint for a month's cost. Keep the purchase
trigger (wait p90 >15 s post-dispatcher) for owned hardware.

**Upheld:** D:I002 (no permanent native ban); D:I111/I036 (plain cargo over
reflink snapshots, mbx retired — subject to H8's runner-host measurement);
D:N16 (coverage skipping rejected — unexecuted coverage proves nothing); D:I063
(both runner-host configurations run); D:N12 (bind required checks to one
integration now: ruleset 24396608 lets any app or `statuses: write` token post
them, D:S3).

## Targets

Committed = the phase does not close without it. Stretch = report, do not
promise. Cache hit % = of selected crate-units, % cargo reports fresh (no
recompile), weekly; receipt hit % = obligations met by lookup / selected, shown
alongside. Baselines: push-to-merge 85/457 (Arch:30); zero-queue run ~11 min
(Arch:33); 285-347 runner-min/merged PR (Arch:35); cross-job reuse 0 (Arch:38).

| Phase | PR verdict P50/P90 (median PR) | Push-to-merge P50/P90 | Runner-min/merged PR* | Fresh % / receipt % |
|---|---|---|---|---|
| 0 (wk 1) committed | ≤8 min / ≤20 min | ≤20 min / ≤45 min | ≤220 | ≥30% / n/a |
| 1 (wk 2-4) committed | ≤4 min / ≤10 min | ≤10 min / ≤25 min | ≤130 | ≥70% / tree ≥20% MG |
| 2 (wk 4-8) committed | ≤90 s / ≤4 min | ≤4 min / ≤10 min | ≤60 | ≥90% / closure ≥40% MG tests |
| 3 (wk 8-14) committed | ≤60 s / ≤3 min | ≤3 min / ≤8 min | ≤35 | ≥95% / ≥60% MG |
| 4 stretch | ≤30 s (preflighted ≤20 s post-push) | ≤2 min / ≤5 min | ≤15 | ≥97% / ≥80% |

\* Charged with warmer + speculation + audit CPU (H8); excluding them the
draft's ≤40 (Arch:108) is not comparable to the 285 baseline. Arithmetic: (1)
the draft commits 2.5 min / 6 min at phase 3 (Arch:101) while its own phase-4
segment sum for the median PR is ~42 s (Arch:487-488) from mechanisms mostly
available earlier — committing 60 s takes its numbers seriously; P90 ≤3 min
absorbs runner-workspace PRs (35-99 s rebuild + tests, Arch:483-485). (2)
Push-to-merge floor is GitHub's ~45 s (Arch:490-491) + one verdict; P50 ≤3 min
leaves ~2 min for queue + receipt lookup, the T3/T4 common case. (3) Removing
push (~62, Arch:34), PR-light + closure (~−50), and cancellation coalescing
(H13) supports ≤60 then ≤35. (4) Flake budget rides along: ≤3% at phase 1,
≤1% at phase 3 (Arch:109) on n≥300 (H11); hitting time over budget = failure.

## Anything missing

**The human segment of push-to-merge is unowned.** Timeline push→merge
(Arch:23-25) with 0 required approvals (D:S3) measures machines only for
auto-merged PRs; reviewed PRs mix human latency into a machine SLO. Split
push→Required, Required→enqueue, enqueue→merged from phase 0; hold the design
to the machine segments only.

**Dispatcher compromise = fleet-wide green forgery, given one line.** It holds
identities, broker sessions, `completejob` (N06), and receipt signing keys
(Arch:308-310); only hand-off mTLS is specified (Arch:189). Add sealed key
custody with rotation, an append-only receipt log anchored outside the cluster
daily, `trusted` promotion requiring warmer key + landed-SHA mirror check, and
the H2 forgery test before phase 2.

**Single points fail before their fixes.** One k3s server (velnor-237, §B.4.3)
with HA as a phase-4 purchase (Arch:783); per-site ledger/store are single
services (Arch:164-165) with no stated backup. Ledger loss is safe only if
detectable (Arch:344-345); silent corruption is the danger. Add backup/restore
drills, an hourly known-answer receipt canary, and two k3s servers *with* the
dispatcher.

**Determinism is assumed, not engineered.** Digests bet on reproducible builds;
jobs set `SOURCE_DATE_EPOCH=0` but nothing pins `--remap-path-prefix`,
proc-macro randomness, build-script host probes, or parallel codegen. Budget
remediation (remapping in the definition digest, hermetic build env) as phase-2
work; a T2 digest mismatch needs an owner, not an ownerless M4 kill.

**Test-data lifecycle has no owner.** Template DBs (P1), goldens, and the 1.48 GB archive (Arch:372) lack invalidation: a migration change must retire every derived template on every node first. Key templates by schema hash in the definition digest; agents refuse stale templates (build cold, report).

**Generality past velnor-new is asserted.** Five roots/per-root jobs are this repo's shape; other `velnor-gen` projects (one/many roots, Docker tests, private deps — "a store rule", D:I050) stress different joints. Run phase-2 gates on velnor-lab *and* one differently-shaped repo (lab-first, `docs/goals.md:76`).

**Post-merge truth-telling is absent.** Audits prove confidence pre-merge;
nothing watches for a slow leak after: revert rate, main-red rate, failures on
receipt-passed trees. Add all three weekly with pre-registered alarms; rising
main-red with green audits means the audit measures the wrong thing.

**Cost has no axis.** Purchases appear (Arch:781-782) but no envelope bounds burst nodes, %-capped speculation (unbounded as volume grows), 5× warmer duplication (H8), or 3.5 TB FI disks (Arch:403-404). Report EUR/merged PR beside runner-minutes from phase 1.

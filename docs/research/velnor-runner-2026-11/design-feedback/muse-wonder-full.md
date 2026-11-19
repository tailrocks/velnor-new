# Design feedback by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

# Design feedback: Velnor runner architecture (draft) — Muse Wonder Full

## Verdict

No: as written this design does not reach the minimum push-to-merge time at the same confidence. It credibly reaches push-to-merge P50 ~6 min (from 85 min [M]) but leaves a further 3–5× on the table by keeping GitHub job granularity, GitHub merge-queue semantics, and cargo `target/` snapshots, and it has receipt/selection holes that would silently cut confidence if built literally. **Ambition 6/10:** removes queues, triplication, and cold caches, but tunes inside GitHub's shape instead of removing the shape — GitHub's own 19 s + 26 s latencies alone already break the 60 s moonshot and the design only attacks them in phase 4–5. **Soundness 6/10:** the ledger + C1–C7 framing is the right spine and better than any single proposal, but C2/C4 as written are exploitable and the audit is underpowered; fixable to 8/10 without changing the architecture.

## Correctness and confidence holes

Severity: **C**ritical = can produce a false green; **H**igh = likely false green or large slowdown; **M**edium = unproven load-bearing assumption.

**H-1 [C] C4 lets a job mint its own exact-tree receipt.** Draft §1.2 C4: "A job's own report may back an exact-tree receipt only." A PR author controls test code; a test that prints pass without testing mints a `tree/` receipt the merge group then reuses without execution. Today that lie only greens its own PR run; with receipts it also greens the merge group that is supposed to be the independent gate, and after landing its receipts promote to `trusted` (§4.2). Fix: delete the exception. Every reusable receipt, including exact-tree, is observed and signed by the node agent from processes it started (§4.3 test executor) or from Worker exit + step telemetry the agent corroborates. A job report alone never creates a receipt.

**H-2 [C] The definition digest does not capture every input (C2 is false as specified).** §4.1 lists command, config, toolchain, image, sandbox class. It omits: kernel/CPU features, absolute paths embedded by rustc (checkout path differs hosted vs scale-set, evidence S7), environment (`CARGO_*`, `RUSTFLAGS`, debuginfo profile §8 rank 10), locale/timezone, nextest wrapper + agent versions, git config/submodule state, and helper *data* (advisory DB, deny lists). Two runs with the same key can execute different bytes. Fix: key = tree + *full* hermetic manifest hashed by the agent at execution time (env allowlist dump, `/proc/cpuinfo` flags class, kernel major, image digest, all tool versions, remapped path prefix). Fail closed: any unmanifested env var widens to execute-always (C5).

**H-3 [H] Exact-tree equality does not mean identical verification.** A PR run and a merge-group run on the same tree differ in `event.json`, `GITHUB_REF`, base SHA, workflow ref, secrets availability, and checkout merge-ref vs head. §4.3 reuses across them anyway; C5 claims to exclude event-reading obligations but nothing detects an event read except post-hoc fanotify (§4.4), which runs *after* the receipt is already reusable. Evidence: the `event.json` rewrite race itself (I023, `ActionRunner.cs:140`) proves steps read event data implicitly. Fix: key includes event class + digests of every file under `$GITHUB_EVENT_PATH`/`$RUNNER_TEMP` the sandbox exposes; enforce by mount namespace from phase 3 for *all* receipt-eligible obligations, not only closure reuse in phase 4.

**H-4 [H] Closure reuse assumes reproducible test binaries; nothing makes them reproducible.** M4 requires bit-identical binaries across nodes. Rust embeds absolute paths, and the design never mandates `--remap-path-prefix`, fixed `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH`, or stable link order. Without that, closure hit rate collapses to ~0 (safe but useless) or, if digests are normalised loosely to force hits, becomes unsound. Fix: add a reproducibility gate as work item 6b: build one commit on two nodes at different paths and diff every test binary; do not design phase 4 targets until the diff is zero with named flags.

**H-5 [H] Selection soundness rests on declarations Rust does not enforce.** §6.2: Cargo package graph + `rerun-if-changed`. But `build.rs` without the directive reruns on *any* package change by cargo default and can read outside the package; proc-macros and `include_str!`/`include_bytes!` read arbitrary files with no declaration; tests read fixtures by relative path (e.g. samples in `golden.rs:19-24`, decisions I039). "Unknown file selects everything" (C3) does not help: the *reader* is known, the *read* is not. Two weeks of shadow plans (§6.2) cannot prove absence of a rare conditional read. Fix: invert the burden — an obligation is selectable only if it has passed under mount-namespace enforcement with declared inputs (same gate as H-3). Everything else always runs. Fanotify stays as telemetry, never as the soundness argument (decisions I037/I069 already say this; the draft's phase-3 merge-group selection violates it).

**H-6 [H] The audit cannot catch the failures it is meant to catch.** C6: 2% of exact-tree reuse, 500 samples (work item 10). If a systematic key bug corrupts 1% of reuses, P(miss in 500) = 0.99^500 ≈ 0.7% — fine — but if it corrupts 0.1% (still ~1 false green per 1,000 merges, catastrophic at 50 merges/week ≈ 2–3/year slipping for months), P(miss) ≈ 61%. Nightly + every-20th-merge full runs verify `main` tip, not each landed tree, so a bad tree that was receipt-greened is never re-executed. Receipt expiry (7 days, §4.2) bounds nothing relevant. Fix: (a) audit 100% of closure reuses for the first 1,000, then sequential testing to a stated false-green upper bound (e.g. <1/10,000 at 95% confidence needs ~30,000 clean reuses — say so); (b) every landed tree gets one full execution within 24 h on batch capacity, receipt or not, and a disagreement auto-reverts; (c) report the bound, not "0 disagreements."

**H-7 [H] Receipt short-circuit (N06) breaks provenance and outputs.** §3.5: dispatcher `completejob`s with "the receipt in the log and no pod." GitHub then shows a green job with no steps, while ruleset 24396608 today requires `Required verdict`/`Required` with *no* `integration_id` (S3) — anyone can already post them, and short-circuited jobs make real and forged greens indistinguishable until N12 lands. Worse, downstream `needs` read job outputs published via `completejob`; a stale receipt republishes stale outputs silently. Fix: N12 (bind checks to one integration) is a *precondition* for N06, not a parallel owner question; short-circuit must replay the original job's outputs verbatim, attach receipt ids as check annotations, and be forbidden for any job with outputs consumed by `needs` until output-equality is proven per obligation.

**H-8 [M] Hot Worker churn is unpriced and racy.** `Runner.Worker` waits 30 s (`Worker.cs:23`); §3.5 replaces it every 25 s in every idle pod. With ~5 workers × (4 gate + 2 build) ≈ 30 idle pods, that is ~1.2 .NET starts/s forever, plus a 5 s window where a job racing the replacement gets a dying Worker. The 87–147 ms/action figure measures interpreter cost, not Worker start; nobody has measured message→first-step hot vs cold. Fix: run the M3 experiment *before* phase 3 commits to pre-created pods everywhere; if hot saves <2 s P50, drop the churn and keep only pre-created pods (6 s pod start [M] is already off-path).

**H-9 [M] "Every node warm for main" hides a second CI system.** §3.4/§5.3: each node incrementally rebuilds every `main` advance per root. At peak 7–8 enqueues/hour (decisions (e)) and ~3 roots, the warmer does ~5 nodes × 3 roots × 8 merges = 120 incremental builds/hour at batch priority on a cluster already sampled at 92–95% CPU (evidence §B.2.1; headline). Runner-minutes per merged PR (§1.4) excludes warmer/speculation/audit CPU, so the metric improves by moving cost off-book. Fix: count *all* CPU-seconds per merged PR (I117) including warmer and audit; build each `main` generation once per site and distribute by reflink seed / site store (§5.1 L2 already exists) instead of N independent rebuilds; warm lazily on first miss if distribution exceeds rebuild.

**H-10 [M] Reflink ≤2 s and "0 crates fresh" are lab numbers.** Work item 6 is correctly gated, but phase-2 targets already bank them (≥95% units fresh, §5.2). A 6–9 GB `target/` is tens of thousands of files; XFS reflink is per-file metadata work, and cargo fingerprints also check mtimes the design restores via `velnor-ci inputs restore` (§5.3) — one missed mtime recompiles the world slowly-but-correctly, silently missing the gate. Moving the kubelet root to XFS (§7, `PLAN.md:45`) is a node rebuild, not a flag. Fix: phase 2 exit gate must be measured on `velnor-runner-host` (35–99 s one-file rebuild [M]), not generator/`ci-helper`; publish clone P50/P90 by file count; keep an mbx fallback until then (see §5 below).

**H-11 [M] Plain cargo over mbx overreads experiment #585.** Decisions (d)/I111 retire mbx because plain cargo compiled 0 crates warm "in 3 of 3 runs." Comparison §"Numbers that conflict" notes those runs are a 5-crate lab repo; the one velnor-new run used mbx, recompiled 46 crates, and was *slower warm (345 s) than cold (291 s)*. Nobody has run plain cargo warm on runner-host. Retiring the incumbent on that evidence repeats the error the panel was convened to avoid. Fix: head-to-head on runner-host, same node, warm snapshot vs mbx store, before phase 2 (see overturned I111).

**H-12 [M] Flake measurement is biased and quarantine cuts confidence.** §1.3 measures flakes by re-running 5% of green SHAs "on idle capacity." The dominant suspected cause is CPU starvation under load (2-CPU requests bursting to 84 cores, §B.2.2; 2 s kill in `ci-helper/.config/nextest.toml` — verified: `test-threads=1`, `slow-timeout 2s`). Idle reruns systematically undercount load flakes. And a quarantined test in a "visible non-blocking lane" means broken code merges — that *is* lower confidence, however visible. Fix: paired reruns must run under representative load (same priority class, co-scheduled); quarantine requires a compensating check or an explicit owner-accepted confidence reduction recorded as such, with an expiry.

**H-13 [M] Dispatcher identity pool vs GitHub semantics is hand-waved.** §3.3 step 3 holds "as many idle JIT identities as capacity advertised" with broker sessions. JIT runners are single-use and version-gated (63-day window, §F.6.1); session/registration rate limits, org runner-group visibility, and what happens when GitHub assigns more jobs than identities (head-of-line blocking behind a long docker job) are unaddressed. ARC's 0.2 min median queue (evidence §A T4) sets a bar the dispatcher must beat under burst, not just at median. Fix: load-test the dispatcher on velnor-lab replaying the busiest hour (M9 experiment) *including* GitHub-side assignment behaviour, not only Velnor-internal placement.

**H-14 [M] The node agent is a single point of total compromise.** Privileged DaemonSet (§3.2) that signs receipts, harvests caches, and holds leases: one sandbox escape from same-repo PR code (build class is unprivileged but shares the kernel) can mint `trusted` receipts and poison snapshots for every later merge. No key rotation, no transparency log, no threshold signing is specified. Forks-on-hosted (§3.6) does not help: the threat is a compromised dependency or agent-authored PR, not only forks. Fix: receipts signed with per-node keys in a transparency log verified by the gate; promotion to `trusted` requires the same key verified by two independent agents (warmer + one re-execution) for the first month; treat same-repo PR code as untrusted for *signing* purposes even if trusted for *scheduling*.

## Where the design is too timid

Each item: **bolder alternative / mechanism / numbers / risk / confidence / cheapest experiment.**

**T-1. Keeping GitHub's merge queue and check flow, then optimising inside it.**
- Bolder: Velnor owns the merge train. The Velnor App maintains the queue,
  computes candidate trees itself, verifies them speculatively, and merges via
  API when receipts hold. GitHub merge queue is disabled; `Required` is posted
  by the App (draft only dares this in phase 4 for one check).
- Mechanism: webhook on push/auto-merge → candidate = `git merge-tree main⊕PR`
  on the mirror → verify obligations → fast-forward `main` when green.
  Batching, bisection on failure, and priority are Velnor's, not HEADGREEN's.
- Numbers: GitHub adds enqueue→run 19 s [M] + finish→merged 26 s [M] = 45 s
  floor before any work (§6.6), plus push→offer 5–10 s [E, unmeasured].
  That floor alone is 75% of the 60 s verdict moonshot.
  Owning the train removes ~45–60 s per merge and, more importantly, removes
  the serial rebuild-the-queue-on-ejection cost: at 8.1% false failure,
  a depth-8 queue ejects and rebuilds constantly (1−0.919^8 ≈ 49% of batches
  disturbed). A Velnor train bisects instead of rebuilding all.
- Risk: reimplementing merge semantics (conflicts, protections) wrong.
  Mitigated by shadow mode below.
- Confidence: identical obligations, identical keys; only *who merges* changes.
  Ruleset still requires the App's check (N12 first).
- Experiment (days): shadow train for 2 weeks — predict every merge-group tree
  and verdict, compare to GitHub's. Kill if tree match <95% or verdict
  disagreement >0. This is M5's experiment; run it in phase 0, not phase 4.

**T-2. Push-webhook start (N09) and preflight (N07/N08) deferred to phases 4–5.**
- Bolder: start verification on the push webhook in phase 1, and make agent
  pre-push verification the *primary* path by phase 2. The GitHub job becomes
  a receipt lookup, not the trigger.
- Mechanism: mirror receives push → plan computed by released helper →
  obligations execute immediately on batch→PR priority escalation when the
  real job arrives. Agents call `velnor-ci preflight` (same endpoint) before
  `git push`; the push then finds `tree/` receipts warm.
- Numbers: draft phase-3 budget (§6.6) spends 5–10 s push→offer + 10 s plan
  before any build. Webhook start hides all of it.
  If agents already spend ≥60 s testing locally before pushing (M8's "must be
  true" — currently uncounted), preflight moves the entire 1.6 min median
  verdict off the push→verdict clock: verdict ≤20 s *committed*, not stretch.
  Even at 0 s local testing, webhook start saves 15–20 s of 96 s ≈ 18%.
- Risk: wasted work on pushes superseded within seconds (45% of PR runs are
  cancelled today, evidence §A T7). Mitigate with 10 s debounce + instant
  cancel; wasted work runs at batch priority.
- Confidence: unchanged — same obligations, earlier start. Preflight receipts
  are agent-observed (H-1 fix), keyed identically.
- Experiment (days): instrument agents for one week: time from last local test
  to push, and push→offer latency from webhook logs. If median local-test→push
  gap >30 s, preflight is the largest lever in the whole design. Nobody has
  measured either number; both fit in work item 3.

**T-3. One job per Cargo root with YAML steps, instead of one verification graph.**
- Bolder: abolish per-root GitHub jobs. One `verify` job checks out once and
  the agent's executor runs the obligation DAG (build once per configuration,
  then fmt/lint/doc/tests as parallel tasks across the pod and, where M6
  arithmetic pays, the site). GitHub sees one job; Velnor sees a graph.
- Mechanism: `velnor-gen` emits the DAG, not 25–39 jobs. The Worker runs a
  single step: `velnor-ci verify --plan <digest>`. All parallelism, caching,
  and receipt checks are inside, where they can share one checkout, one
  `target/`, and one test-binary build.
- Numbers: today 25 executed jobs × (10 s tools [M §A T9b] + 4.4 s actions [M]
  + 6 s checkout + 12–14 s start [M]) ≈ 25 × 33 s ≈ 825 s of duplicated
  overhead per run, of which the draft removes only the per-job part that
  survives "one job per root" (~5 roots → still ~165 s).
  One job removes ~130 s more and, decisively, removes 24 job-start tails
  from the P90: P90 today is set by the slowest of 25 queues.
  Draft phase-3 P90 ≤8 min vs P50 ≤2.5 min is mostly this tail.
- Risk: one big job loses GitHub's per-job rerun and log separation.
  Mitigate: obligation-level logs in the site store, rerun = re-execute only
  missing receipts (§4.3 already promises this).
- Confidence: same obligations; the DAG is emitted by the released generator
  and its inventory is diffed against today's (work item 8 parity, kept).
- Experiment (days): hand-run one root's full obligation set as a single
  script on a warm worker vs as separate jobs; compare wall + CPU-seconds.
  Half a day. If the win is <60 s P50, keep per-root jobs.

**T-4. `target/` snapshot cloning instead of a content-addressed build graph.**
- Bolder: stop cloning 6–9 GB `target/` directories. Cache at rustc-invocation
  granularity (sccache-style remote execution cache owned by Velnor, or mbx
  done right) with outputs addressed by input digest, shared across nodes via
  the site store. A job materialises only the outputs it needs.
- Mechanism: `RUSTC_WORKSPACE_WRAPPER` (workspace crates execute locally for
  debuggability; dependencies — 435–522 crates downloaded per top job [M §A
  T9a] — are content-addressed). Fingerprints, not mtimes, decide freshness,
  so H-10's mtime-restore fragility disappears.
- Numbers: clone cost scales with `target/` size (9.0 GB root [M]) and file
  count on every job; a CAS fetch scales with the *delta* (median PR touches
  1–3 crates). Cold lockfile builds (10.5% of PRs [M]) become "fetch the new
  crates' outputs built by the warmer once" instead of "every node rebuilds."
  Warmer cost (H-9) drops from N rebuilds to 1 build + distribution:
  ~5× less warmer CPU at 5 workers.
- Risk: rustc-wrapper is reported as a "weakening" by `velnor-gen`
  (`weaken.rs:89-93`, S9) — that rule must be revisited with evidence, not
  obeyed blindly. Wrapper bugs can return wrong outputs; content addressing
  + audit sampling (H-6) is the containment.
- Confidence: outputs keyed by full input digest (compiler version, flags,
  inputs); a wrapper bug that changes an output changes its key or is caught
  by sampled re-execution. No weaker than cargo fingerprints over cloned
  mtimes — arguably stronger.
- Experiment (days): sccache + local disk cache on one worker for
  runner-host, cold/warm/one-file-edit, vs reflink snapshot. Two days.
  The draft rejects this (I109/I110) without the experiment; run it.

**T-5. Native executor gated at ">15 s per run" (I005) — the gate measures the wrong thing.**
- Bolder: build the restricted native executor for the `velnor-gen` profile in
  phase 3 alongside the dispatcher, not as a phase-5 maybe. Its value is not
  87–147 ms/action; it is deleting per-job Worker start, action download/
  resolve, step telemetry round-trips, and checkout/cache JS entirely for the
  90% of jobs Velnor generates itself.
- Mechanism: dispatcher classifies the job message; generated-profile jobs run
  natively (checkout from mirror alternates, node stores, `velnor-ci verify`);
  everything else runs the hot Worker. Choice before first step, same identity
  (draft §3.5 already concedes this is safe).
- Numbers: per job today: start 12–14 s [M] + action resolve/download ~4.4 s
  [M] + checkout 6 s + tools 10 s. Native path with warm stores: start ~1 s +
  checkout-via-alternates ~1–2 s + tools 0 s (mounted) ≈ saves ~25–30 s/job.
  On a 5-job critical path that is serial only at plan/verdict hops (~2 hops
  × 25 s = 50 s), but on runner-minutes it is 25 jobs × 25 s ≈ 10 runner-min
  per run (~19% of 52.9). The 15 s/run gate would *pass*; the timidity is
  scheduling it two phases late, after building hot-Worker machinery (H-8)
  that the native path then discards.
- Risk: compatibility drift. Contained by the restricted profile + I007
  differential gate (100 paired jobs/root, zero semantic diffs) — keep the
  gate, move the work earlier.
- Confidence: identical obligations; native executor is an executor, not a
  verifier. Receipts still agent-observed.
- Experiment (days): prototype the native path for *one* generated job shape
  (plan job) on velnor-lab; measure message→verdict vs hot Worker, 50 runs.
  If <10 s saved, defer. Cost: days, not a phase.

**T-6. Merge-group depth 4→8 and "selection later" instead of speculative pipelining now.**
- Bolder: verify speculative candidates for the next N queue positions
  continuously from phase 1 (under ARC, as ordinary jobs on synthetic refs),
  so that by the time GitHub creates the merge group the receipts usually
  exist. Depth becomes a Velnor pipelining parameter, not a GitHub setting.
- Mechanism: on every `main` move, for each auto-merge PR in queue order,
  verify `main⊕PR` at batch priority; on failure, verify `main⊕PR1⊕PR2`
  bisection variants. Results land as `tree/` receipts (phase 3) or, before
  the ledger, as warm PR scopes + recorded verdicts that make the real run
  mostly cache hits.
- Numbers: S4: 0/40 (worst day) to 11/60 (18%) of merge groups test an
  already-verified tree. Exact-tree receipts alone therefore save ~nothing
  until PR runs shrink — the draft admits this (§6.6) yet still commits
  phase-3 P50 ≤6 min assuming selection. Speculation inverts it: if the
  queue holds Q PRs and each candidate takes V≈2.5 min, pipelining Q
  candidates costs Q×V batch CPU but cuts queue residence from Q×V serial
  to ~V. At Q=4, that is ~7.5 min off push-to-merge P50 on busy days —
  larger than any single phase-3 item.
  Wasted-work bound: a wrong guess costs one batch-priority run; with tree
  match rate p, waste fraction = (1−p). At p≥0.5 (M5 kill threshold) waste
  ≤50% of speculative CPU, all preemptible.
- Risk: batch work starves real work at 92–95% CPU. Mitigate: speculation
  only when Velnor-side wait p90 <5 s *and* node CPU <70% over 5 min;
  preempt instantly (draft priorities already allow this).
- Confidence: speculation never changes a verdict; the real merge group still
  requires receipts for *its* tree or executes. A wrong guess is wasted CPU.
- Experiment (days): M5 shadow prediction (2 weeks, zero execution) started
  in phase 0. If match ≥50%, fund execution; if <50%, kill — but know by
  week 2, not week 12.

**T-7. Docker class preserved as privileged dind on tainted nodes.**
- Bolder: eliminate dind from the required path. Build images with a
  daemonless builder (BuildKit in rootless mode as a site service, or
  `docker buildx` against a shared per-site builder); tests that need Docker
  talk to the site builder over the agent socket. No privileged pod ever runs
  PR code.
- Mechanism: classify "needs Docker" at plan time into (a) builds an image,
  (b) runs containers in tests. (a) → site builder service. (b) → test
  executor starts containers as sibling sandboxes via the agent, observed
  like tests (H-3 enforcement applies).
- Numbers: dind costs 3–4 s/job (§8 rank 5) + 57 probe failures/hour [M §B.6]
  + a whole privileged attack surface + 2 tainted nodes stranded for other
  work. The win is less seconds than risk: it removes the only privileged
  PR-code execution in the design, which also derisks H-14.
- Risk: some Docker test semantics (networking, volumes) differ. Inventory
  first: count jobs actually declaring Docker today (draft never counts them).
- Confidence: image digests pinned (C5 already requires digest pinning for
  comparability); builder outputs content-addressed like test binaries.
- Experiment (days): run the Docker-declaring test suites against a rootless
  BuildKit service on velnor-lab; count failures. If >5% need real dind,
  keep a microVM docker class (I086) instead of privileged pods.

## The hardest problems

**P-1. Breaking the 200–274 s single-test floor (and the next floors behind it).**
- Facts: 40-case loop 258–274 s (`image_publication_public.rs:85-92`, verified
  loop at :88 over 40 cases at :94) and 6-case loop 203–271 s
  (`shape.rs:97-106`, S8). Splitting 40 ways gives ~6.5 s/case *only if*
  per-case fixture cost does not repeat; reviewers doubt the 7 s figure for
  exactly this reason (comparison, reviews). Behind them sit the 89 s image
  fixture (evidence T10), journal migrations, and `velnor-runner-host`
  rebuild 35–99 s [M]. The floor is a staircase, not a step.
- Attack: (1) Split loops *and* hoist shared fixture construction into a
  content-addressed fixture built once per closure (template DB cloned by
  reflink, fixture image built by warmer) so per-case cost is test logic only.
  Measure per-case fixture vs logic split first with nextest timing + perf;
  if fixture >50%, splitting alone fails and fixture caching is the fix.
  (2) Attack CPU directly: dependencies at `opt-level=3` measured −66% CPU on
  the slowest class [M, fable-5.2] — run it as the default experiment, not an
  owner-question maybe (I064). 270 s × 0.34 ≈ 92 s before any split; split ×
  opt ≈ 2–3 s/case. Debug assertions stay on; overflow checks stay on.
  (3) Make the floor structural: test-time budget blocks at 15 s by phase 2
  (not 30 s), and any test over budget must declare why it is indivisible.
  Indivisible tests get a dedicated high-clock slot (T-8/hardware) instead of
  taxing every PR.
- Kill criterion: if after fixture-hoist + opt-level the slowest *case* still
  exceeds 30 s, the test is really an end-to-end scenario — move it to the
  nightly full run with owner sign-off and replace it in the required profile
  with smaller obligations that cover the same properties. Say plainly that
  this trades PR-time confidence for nightly confidence; do not pretend
  sharding solved it.

**P-2. Zero cold start when GitHub itself is the cold start.**
- Facts: Velnor-side start can reach ≤1–3 s (pre-created pods, hot Worker),
  but push→offer is unmeasured [E], enqueue→merge-group-run is 19 s [M], and
  merge→merged is 26 s [M]. Sum of GitHub-controlled latencies ≈ 50–65 s.
  A "60 s verdict" that starts the clock at offer arrival is accounting, not
  speed. Push-to-merge starts at `git push`.
- Attack in order: (1) Measure push→offer properly (webhook timestamp vs
  first `JobAvailable`, 1 week, P50/P90 by hour). (2) Start on webhook (T-2):
  plan + build run before GitHub creates jobs; jobs find receipts. This hides
  push→offer entirely if verification ≥ GitHub latency, which at ≥60 s of
  work it almost always is. (3) Own the merge train (T-1) to remove the
  19 s + 26 s merge-queue latencies; the App merges when receipts hold.
  (4) For the residual Velnor start: pre-created pods + reflink/CAS (T-4) +
  native plan execution (T-5) target assignment→first-cargo ≤2 s P50.
  MicroVM snapshot restore (I086) only if pods + hot Worker measure >3 s.
- Honest bound: with (2)+(3), push-to-merge floor ≈ max(verification,
  GitHub webhook delivery ~1–5 s) + merge API ~2–5 s. ~60 s end-to-end is
  then a verification problem (P-1, P-3), not a GitHub problem. Without
  (2)+(3), commit to ≥2 min and stop calling 60 s a moonshot.

**P-3. Reusing verdicts across PR, merge queue, and main without trusting anything unproven.**
- Facts: exact-tree reuse hits 0–18% of merge groups today (S4) because
  `main` moves during long waits. Package-level replay claims 88% of
  merge-group obligations are reusable (opus-5.5), but reviewers call it
  "a guess wearing a measured costume" (comparison) and per-root jobs
  collapse the granularity. The gap between 18% and 88% *is* the design.
- Attack: three concentric mechanisms, each independently gated.
  (a) Shrink the window: faster PR runs directly raise exact-tree hit rate.
  Model: if `main` advances every ~15 min at peak and PR verdict takes V,
  P(base moved) ≈ 1−exp(−V/15min). V=11 min → ~52% moved; V=2.5 min → ~15%;
  V=1 min → ~6%. Speed *is* a reuse mechanism; publish hit rate vs V weekly.
  (b) Speculative candidates (T-6) convert "moved base" from a miss into a
  pre-verified hit whenever `merge-tree` predicts GitHub's candidate.
  (c) Closure receipts (N02) for the remainder, but only with H-2/H-3/H-4
  fixed: reproducible binaries + enforced inputs + agent-observed results.
  Order matters: (a) is free, (b) needs only exact-tree receipts, (c) is the
  research project. The draft sequences (c) as the phase-4 headline and (b)
  as a side moonshot; invert them.
- Proof bar: replay 200 historical merges offline computing, per obligation,
  whether (a)/(b)/(c) would have hit. Report hit rate by mechanism with
  denominators. Do not commit phase-4 targets until the replay exists —
  it needs only git history + recorded tested trees (N13), no new infra.

**P-4. Flake-free required checks on a shared cluster at 92–95% CPU.**
- Facts: 8.1% false failures per full run [M S1]; two runs per merge →
  ~15% of merges hit a false failure (1−0.919², N14). Workers sampled
  92–95% CPU (§B.2.1) with 2-CPU requests bursting to 84 cores; job
  durations stretch 1.2–2.4× under load (runner-host 3.7→8.8 min, §A T6).
  Flakes and saturation are the same problem: timeouts written for idle
  machines firing on loaded ones (2 s kill, H-12).
- Attack: (1) Remove work before adding capacity: dedupe (×3→×1) + selection
  + cancellation hygiene (45% cancelled runs still burn CPU until cancelled;
  cancel propagation today waits for hosted gates). Draft cuts runner-minutes
  ~80% [E]; if true, 92% CPU becomes ~20–30% and most load flakes vanish
  without touching a test. Verify with CPU-seconds accounting (H-9), not
  runner-minutes. (2) Make timeouts load-aware: nextest slow-timeout scaled by
  pod CPU guarantee, and honest Guaranteed pods (request=limit for CPU on
  test slots) so a test's CPU is *reserved*, not contended. This costs burst
  on cold builds — accept it for test steps, keep burst for builds via
  separate classes. (3) Attack residual true flakes (races, ordering) with
  the paired-rerun sampler *under load* (H-12 fix) and a hard budget:
  any test flaking twice in 7 days blocks its owning team's queue priority
  until fixed — social pressure with teeth, quarantine only as H-12 allows.
- Target arithmetic: flake-free means ≤0.3% per run (draft stretch). At 50
  merges/week × 1 run (post-dedupe), 0.3% ≈ 1 false ejection per 7 weeks.
  At today's 8.1% × 2 runs it is ~7–8 per week. Depth-8 queue (I075) is
  only safe below ~2%: P(batch disturbed) = 1−(1−f)^8; f=8.1% → 49%,
  f=2% → 15%, f=0.3% → 2.4%. Gate depth on measured f, as the draft says —
  but measure f under load or the gate lies.

**P-5. Job placement GitHub does not let you control.**
- Facts: `generatejitconfig` takes `{name, workFolder}` with no job binding
  (§F.1.3); GitHub assigns to whichever idle runner it chooses. Seven
  reviewers agree affinity/label tricks cannot place per-job (comparison).
  The draft's answer — own acquisition, place after assignment (§3.4) — is
  correct but arrives in phase 3, leaving phases 0–2 cache-blind.
- Attack: make placement *unnecessary* first, optimal second.
  (1) Warm-everywhere for `main` (draft phase 2) already removes the biggest
  term; strengthen with T-4 CAS so "warm" means "outputs fetchable from site
  store in ≤5 s" on *any* node, not "this node's disk." Placement then only
  decides PR-scope locality (one delta build) and site stickiness.
  (2) Before the dispatcher, use the one lever ARC *does* give: one scale set
  per class pinned by node affinity to node subsets (gate/build/docker, DE/FI).
  Coarse, but it bounds cross-site artifact traffic (35 ms DE–FI, §B.5) now.
  (3) With the dispatcher, place by predicted finish time (draft §3.4) but
  seed predictions from recorded per-obligation durations (work item 3);
  until history exists, idle-beats-warm is the whole policy — a busy warm
  node at 92% CPU loses to an idle cold node by more than a delta build.
  Publish placement regret (chosen finish − best possible) weekly; if regret
  <5 s P50 after (1), stop optimising placement entirely.
- Kill criterion: if CAS fetch P90 ≤ delta-rebuild P50, per-job placement is
  dead weight — delete the predictor and keep only site stickiness + load
  balancing. The cheapest placement is the one you prove you do not need.

## Decisions you would overturn

- **I003 (dispatcher in phase 3) → phase 2.** Every phase-3 headline (placement, fork refusal N11, merge-first admission, short-circuit) needs owned acquisition; two phases of ARC template work are thrown away. ARC queues 0.2 min [M] so it is a fine fallback, but build the dispatcher on velnor-lab in parallel from week 1 (work item 9 has no dependency except owner Q1) and cut over when it wins, not on a calendar.
- **I005/I007 (native executor phase 5, >15 s gate) → phase 3 for generated profile.** Keep the I007 differential gate; move the work (T-5). The gate as written measures interpreter ms, not job-start + download + checkout the native path actually deletes (~25–30 s/job [E] above).
- **I054 (merge-group selection: owner decision, phase 3) → reject until H-5 gate passes.** Shadow parity for 2 weeks cannot prove selection sound for conditional reads. Require mount-namespace-enforced declarations per obligation first; until then merge groups run the PR's *selected* set re-executed (or receipt-satisfied), never a narrower set than the PR run verified.
- **I075 (depth 2→4 now) → hold at 2 until flakes measured under load.** At f=8.1%, depth 4 disturbs ~29% of batches (1−0.919^4); deeper speculation on a flaky queue multiplies rebuilds (N14 arithmetic). Raise depth only when load-representative f ≤3%, then 4, then 8 at ≤1% — stricter than the draft's ≤2% for 8.
- **I082 rejected (no pod per job) → experiment-gated, not rejected.** Pre-created pods cost 6 s [M] off-path but still cost image/pod churn and kubelet accounting per job. A node-agent sandbox (namespaces/cgroups, Fable/Carbon) is the only path to ≤1 s start (M3) without hot-Worker churn (H-8). Keep pods for phase 3; fund the sandbox spike alongside the M3 experiment instead of closing the door.
- **I091 (forks on hosted until microVM) → microVM fork class in phase 3, not phase 5.** Hosted forks keep the Free-plan 20-job pool (S5) on the fork critical path and give fork contributors the worst experience indefinitely. A snapshot-restored microVM (I086) is also the docker-class answer (T-7); build it once, earlier.
- **I105 rejected (more EPYC workers) → owner decision with a trigger now.** "Buy when p90 >15 s after phase 3" waits 12 weeks to discover saturation. Decide the trigger and budget now (e.g. sustained worker CPU >70% at peak after dedupe, or Velnor wait p90 >10 s for 2 weeks) so procurement is not the critical path. Same for I104 high-clock node: rent in phase 1 to measure P-1, not after phase 2.
- **I111/I110 (retire mbx for plain cargo) → experiment-gated (H-11).** Run mbx vs plain-cargo-over-snapshots vs CAS (T-4) on runner-host before retiring anything. `velnor-gen` calling mbx a "weakening" (`weaken.rs:84-88`) is a policy assertion, not a measurement.
- **N06 (short-circuit phase 4, experiment-gated) → phase 3 but only after N12.** It is the largest single saver (whole jobs) and needs no closure receipts — exact-tree suffices. The timidity is sequencing it after M4. Move it up; gate it on integration-bound checks (H-7), not on phase number.
- **N15 (PR-light/queue-full experiment) → reject.** It saves PR time by discovering failures in the queue, where each failure rebuilds everything behind it. With 3/40 queue exits already failed checks *with* full PR runs (S2), PR-light raises queue failures and push-to-merge P90 while flattering PR-verdict P50. If PR verdict must shrink, shrink verification (P-1/P-3), do not defer it to the serial resource.
- **N16 rejected (coverage-based skipping) → keep rejected, but for the record:** agreed. Coverage proves execution, not dependence (layout/statics/timing). Closure receipts are the sound version. No overturn; listed so the rejection is not revisited under speed pressure.

## Targets

Committed = fund and gate on it; stretch = experiment must prove it first. Cache hit % = compile units fresh/fetched without rustc execution (state the denominator every time; I117). Runner-minutes below count *all* cluster CPU (jobs + warmer + speculation + audit) per merged PR — the draft's job-only numbers in parentheses.

| Phase | PR verdict P50/P90 | Push-to-merge P50/P90 | Runner-min/merged PR | Cache hit % |
|---|---|---|---|---|
| 0 (wk 1) committed | ≤11 / ≤20 min (zero-queue floor [M]) | ≤30 / ≤75 min | ≤280 all-in | n/a (measure) |
| 1 (wk 2–3) committed | ≤5 / ≤10 min | ≤12 / ≤30 min | ≤160 (draft job-only 150) | ≥80% deps |
| 2 (wk 3–6) committed | ≤2.5 / ≤6 min | ≤7 / ≤16 min | ≤80 (draft 60) | ≥90% units, PRs w/o lockfile change |
| 3 (wk 6–12) committed | ≤90 s / ≤4 min median PR*; ≤4 min all-PR P50 | ≤4 / ≤10 min | ≤50 (draft 40) | ≥95% units |
| 3 stretch | ≤60 s median PR | ≤3 / ≤7 min | ≤30 | ≥97% |
| 4 stretch | ≤45 s median; ≤20 s preflighted | ≤2 / ≤5 min | ≤20 | ≥98% + ≥70% MG obligations by receipt |

*Median PR = generator/`ci-helper` only (47% [M]); always report all-PR numbers beside it or the target flatters itself. False failures: ≤3% phase 1, ≤1% phase 3 committed (agree with draft), ≤0.3% stretch — measured under load (H-12). Longest test: ≤60 s phase 1, ≤15 s phase 2 committed (draft makes 15 s stretch; P-1 attack makes it committable — if the fixture/opt experiment fails, demote with evidence). Velnor-side wait p90 ≤5 s phase 3 (agree), ≤1 s stretch only with owned merge train (T-1); without it, GitHub's 45 s floor makes sub-minute push-to-merge dishonest at any Velnor speed.

## Anything missing

- **Human/agent waiting is unmodelled.** Push-to-merge includes review and agent think-time; 0/40 auto-merge [M S2] and 7.5 min green→enqueue median are *procedure*, not runners. If agents batch pushes hourly, a 60 s verdict saves nothing end-to-end. Instrument push→review→enqueue as first-class spans (work item 3) and set an agent-procedure target alongside runner targets.
- **Cancellation storms.** 45% of PR runs cancelled, 1,156 runs [M §A T7]; cancelled runner-minutes are excluded from every baseline. At ~15 min assumed per cancelled run (comparison) that is ~17k runner-min in 15 days — comparable to all successful PR work. Fast cancel propagation + push debounce + "superseded receipts still count" (a cancelled run's completed obligations should still mint receipts) are missing wins.
- **Cost and capacity planning.** No €/merge, no Hetzner power/egress accounting, no model for ChainArgos/jackin growth. At 480 threads shared by 3 orgs (maxima 60+60+40 runners, §B.3.2), one org's retry storm is everyone's outage. Quotas (§7) need *burst* semantics and a noisy-neighbour drill, neither specified.
- **Supply-chain and update treadmill.** Runner 63-day deprecation, toolchain bumps invalidating every snapshot at once, crates-mirror compromise, GitHub App token scope for the merge train/App-posted checks: each needs a named owner and a rehearsed rotation. A toolchain bump day currently means N nodes × cold rebuilds simultaneously (H-9) — plan the stampede.
- **Observability of the ledger itself.** Receipt hit rate by mechanism (tree/speculative/closure), placement regret (P-5), warmer lag (main advance→snapshot ready), and audit bound (H-6) need dashboards *before* phase 3, or the first false green will be debugged from GitHub logs that short-circuited jobs no longer produce.
- **Non-Rust and release paths.** macOS release builds, docs publishing, and image publication (89 s fixture) stay on hosted/other paths with no targets. Push-to-merge can hit 2 min while push-to-*release* stays hours; say which metric the owner actually feels.
- **Exit strategy.** If GitHub deprecates JIT scale sets or tightens self-hosted terms, the dispatcher is stranded. ARC fallback covers weeks, not a platform change. The obligation DAG + ledger (T-3) is the portable asset — another reason to invest there over protocol machinery.

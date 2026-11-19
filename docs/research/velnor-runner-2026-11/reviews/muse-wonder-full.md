# Review by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

# Cross-critique: Proposals H, C, D

Scope note: claims below were re-checked against `evidence.md` and, where
named, against code in velnor-new @ `95de9b704b`, `actions/runner` v2.338.0
and ARC. "[verified]" means I read the cited code myself in this session;
everything else is judged against the pack.

## Proposal H

### Weakest assumptions

1. **Four-way nextest sharding divides the slow test steps by four.**
   The arithmetic `246 s × 0.55 / 4 = 33.8 s` assumes test time is divisible.
   Proposals C and D, from job logs, show the runner-host and actions-cli
   steps last exactly as long as one looped `#[test]` (270.8 s of a 270.9 s
   step; median longest case 260.7 s of a 260.8 s step). A nextest partition
   cannot split cases inside one test. H never proposes splitting the looped
   tests, so its heaviest shard still carries a ~4-minute floor. Evidence
   that would settle it: per-test durations from one runner-host log, which
   the pack's raw logs already contain.
2. **PR #1333 + #1349 multiply to a 0.55 test factor, and a new v51 template
   removes the rest.** The two percentages come from different PRs, different
   test subsets, and are multiplied without a joint measurement. C's CPU
   sample puts the time in `turso_parser` prepare/translate on every journal
   open, not only in migration replay; the next-slowest tests are 75–118 s,
   so even a perfect fix leaves a >75 s step. The v51 pre-migrated template
   is new work presented as a step in the arithmetic. Settle by benchmarking
   the two PRs together on one heavy test binary, cold and warm.
3. **tmpfs `/tmp` saves 30–60 s on runner-host tests.** This assumes SQLite
   fsync on disk dominates. The only direct profile anyone took (C, macOS
   `sample`) is CPU-bound in SQL parsing. The pack contains no fsync or
   IO-wait measurement. tmpfs may still help in general, but crediting it
   with up to a minute on the critical path is unproven. Settle with
   `iostat`/IO PSI or a tmpfs A/B on one worker.
4. **A pre-warmed pod can be bound to a chosen cache slot after
   `JobAvailable` in <1.5 s.** Two mechanisms are hand-waved: (a)
   `velnor-node-agent` "bind-mounts slot s into the standby pod's shared work
   volume" — Kubernetes cannot add mounts to a running pod, and no
   mount-propagation design is given; (b) pickup stops the clock at
   `exec Runner.Listener`, excluding broker session creation and job
   assignment, which H's own H2 analysis counts as two mandatory network
   calls. Settle with a spike: one standby pod, timed from `acquirejobs` to
   first step line.
5. **Warm mbx hit rates of 95–99% of units, hence 42–50 s typical PRs.**
   The warm probe cited (`compiled-state-persistence.md`) is a same-machine
   no-change/leaf-change measurement on `velnor-runner-cli`. H extrapolates
   it across five workspaces, three compile passes (test/clippy/doc), two
   feature configurations, and reflink-cloned `target/` directories shared
   with Cargo's own fingerprinting — two cache layers whose interaction
   (stale fingerprints, `OUT_DIR` paths, proc-macro freshness) is never
   tested. Removing `mbx clean` so Cargo fingerprints short-circuit the
   wrapper is asserted, not shown. Settle with one warm-slot prototype run
   on runner-host, reporting Cargo `fresh` counts and mbx hits separately.

### Factual errors

- **mold "3×–5× over GNU `ld.bfd`" (ranked table #6, §7).** Wrong baseline.
  Rust 1.99 links `x86_64-unknown-linux-gnu` with rust-lld by default (since
  1.90), and velnor-new sets no linker override [verified: no linker config
  in root `Cargo.toml` or `.cargo/config.toml`]. No link time is measured
  anywhere in the pack (T10 has no link timer), so the "20–45 s per job"
  saving has no numerator. C and D both get this right.
- **"All pod `emptyDir` scratch lands on the unstriped 3.5 TB OS disk
  (`kubectl` stats summary)".** The pack states the opposite epistemic
  status: "Where emptyDir volumes physically live is not directly stated…
  the kubelet root dir was not read. Unknown." (`evidence.md:492`). Stats
  summary proves nodefs and imagefs are different filesystems; it does not
  prove which one holds kubelet's root dir. Same for the "linear
  (concatenated) LVM" claim: it cites `PLAN.md:76`, a plan, and the pack
  lists stripe layout as not observable read-only (`evidence.md:500`).
- **Summary range "26.7 to 77.8 minutes".** This mixes populations: 26.7 is
  the scaleset-era PR median (n=77, success+failure, T2) and 77.8 is the
  11-18 sampled PR median (T2 by-day); the success-only 11-18 figure H's own
  table uses is 71.8 (T5). The 15-day population median is 17.5 (T1). The
  range overstates the typical day by construction.
- **Bug 1's second mechanism is redundant and unverified.** H claims both
  velnor's `compat` hash and `@actions/cache`'s path-version hash cause the
  sources miss. The `compat` construction is real [verified:
  `ci.yml:329` / the State seal workflow hash absolute `$RUNNER_TEMP/...` lines
  into `sources-compat`], so *if* `RUNNER_TEMP` differs across lanes the key
  differs. But H never prints `RUNNER_TEMP` or either computed key from an
  actual hosted and scale-set log of the same run. D claims the missing key
  "exactly matched" Plan's key. One of them is wrong; H's "traced" wording
  hides that the decisive values were never observed.
- **"All 9 remaining failures crashed … exit code 125".** The pack says 9 of
  16 State producer failures exited at `Clippy` / `Wait for parallel checks`
  (T9d discussion, `evidence.md:423`). H measured one job log
  (`134981831502`) and promoted it to all nine. The `event.json` mechanism
  itself checks out [verified: `ActionRunner.cs:141` calls
  `WriteWebhookPayload()`, `ExecutionContext.cs:1335` is a non-atomic
  `File.WriteAllText`, and `task_producer.rs:99-106` reads
  `GITHUB_EVENT_PATH`] — but a sub-millisecond overwrite window explaining
  9/9 failures needs the other eight logs shown.
- **Slot arithmetic contradicts the proposed CPU manager.** §6 reserves 6 of
  96 threads, then gives K=6 pods `requests.cpu: 14, limits.cpu: 16`
  (limits alone sum to 96), while §8a enables `cpu-manager-policy=static`,
  which only pins Guaranteed pods (request == limit, integer). Burstable
  14/16 pods get no static pinning, so the CCD-locality claim does not
  follow from the stated spec.

### Missed bottlenecks or risks

- **Push-to-merge is never measured.** H optimises PR wall time and stops.
  The brief's metric includes green-to-enqueue delay and merge-queue
  throughput (C: median 7.5 min to enqueue, 0/100 auto-merge,
  `max_entries_to_build: 2` capping merges at ~1.7/h on 11-18; D: pre-job
  admission p90 25.45 min on 11-18, before any job record exists). H's
  45-second Phase 3 PR can still wait hours behind a depth-2 queue.
- **Confidence change disguised as dedup.** Replacing the default-features
  runner-host test run with `cargo check` drops every test that is
  cfg-excluded under `--features local-mode` (and vice-versa) without
  enumerating them. Owner question 4 flags it, but the 6.23 runner-min
  saving is already spent in §2/§5 targets. That is not "same confidence"
  until the two test inventories are diffed.
- **The `gha_cache` redirect is not implementable as described.** On the
  official-Worker path, `ACTIONS_RESULTS_URL` / `ACTIONS_CACHE_URL` come
  from the job message's `SystemVssConnection` data (`evidence.md:1234,
  1304-1305`); the Worker sets them per job. "Redirecting" them from the pod
  environment will be overwritten, and proxying Results twirp while
  intercepting Cache twirp on the same base URL is a MITM design that is
  never specified. Legacy's service is 17,140 lines (`evidence.md:1015`),
  not the "~2,500 LOC trimmed" port H budgets.
- **Tree-SHA verdict reuse ignores event context.** Only 7/45 sampled PR head
  trees equal the merge-group tree (T7), and tree equality says nothing
  about event payload, secrets, advisory DB state, or
  `merge_group`-specific inputs. The check-runs API lookup also trusts a
  check *name* (`Required`) whose semantics differ across the verdict/gate
  pair. The pack's supported case is narrower: identical SHA on push after
  merge_group (120/159).
- **Timeline.** Phase 0 (days 1–2) promises PR median ≤6.5 min, but D's
  replay of routing alone yields 11.37 min because the 8.5 min fan-out
  survives; H needs the fan-out halved in the same two days by changes
  (-j16, tmpfs, baked tools) whose individual gains are the unproven items
  above. Eighteen days to replace ARC, land a Kubernetes runtime, a node
  agent, reflink slot lifecycle, a cache service and Sysbox — with
  qualification — is not credible next to C's 8–10 weeks for less.
- **Other tenants vanish.** "150+ PR runs per hour" and the K=6 capacity
  plan allocate all five workers to tailrocks; the pack shows three scale
  sets sharing the nodes (max 60/60/40, headline facts) and H's own affinity
  section later gives two FI nodes to other orgs. The numbers are never
  reconciled.

### Ideas worth taking

- **The `event.json` race diagnosis and fix** (copy `GITHUB_EVENT_PATH` once
  before any `background: true` step; never read it from parallel steps).
  Mechanism verified in code; cheapest correctness fix in any proposal, and
  it explains a failure class the pack left at "9 at Clippy/Wait".
- **Bug 2's compat-drift analysis**: `ImageOS`, `store` and `checkout`
  absolute paths hashed into state `compat` [verified at
  the State seal workflow construction and `ci.yml:707`], with scale-set
  `ImageOS=velnor-arc-ubuntu24` (`evidence.md:553`). Whatever the final
  cache design, "never hash lane-specific absolute paths or image identity
  into portable keys" should be a hard rule, with a test that computes keys
  under both lane environments.
- **Zero-download actions via `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`** and a
  read-only pre-verified toolchain seed. All three proposals converge here;
  H's costing (set-up-job action downloads, 706 sample runner-minutes for
  `Prepare pinned tools`, T9b) is the clearest.
- **Retiring the State seal workflow** in favour of node/site mbx rather than
  repairing the GitHub-cache bundle path (65–107 s transfers, 10 GB cap,
  tip races). Shared with C and D; H states the <15 s push-skip rule most
  operationally.

### Verdict

No. H correctly identifies the hosted queue, the dead caches and one real
race, and its cache-key forensics are the best of the three, but its
headline times (42–50 s typical, 45 s after Phase 3) are built by
multiplying unmeasured factors — sharding a step that is one indivisible
test, a 0.55 journal factor, tmpfs and mold gains against wrong baselines —
while never measuring the merge half of push-to-merge. Its single biggest
flaw is treating the critical path as divisible when the logs show it is
one looped test per heavy job; until those tests are split, every total
under ~4 minutes for an affected PR is arithmetic, not engineering.

## Proposal C

### Weakest assumptions

1. **A fanotify read set makes a verdict reusable across runs.** This is the
   load-bearing claim and it does not hold as stated. (a) A trace records
   what one execution read; it cannot prove what an unexecuted branch would
   read, so it verifies the run that happened, not the job's input closure.
   (b) Attribution on shared read-only mounts "through the process's
   cgroup" glosses over fanotify semantics: marks are per mount/filesystem,
   events carry a pid (with reuse and namespace mapping problems), page-cache
   and mmap reads, `/proc`, environment, time, DNS and git metadata are
   outside the two traced mounts. (c) Hashing file contents after the job
   ends is TOCTOU-unsafe; the hash must be taken at open. (d) The key
   includes command + environment but PR and merge-group runs differ exactly
   in event context — include it and reuse never hits, exclude it and
   event-sensitive checks are unsound. `docs/hard-decisions.md:199-204`
   names in-job read verification as the precondition for *selection* on
   push/merge_group, with full runs as backstop; C stretches it into
   cross-run verdict caching. Settle with a prototype: trace one real
   runner-host job, perturb one traced and one untraced-but-read input, and
   show the key changes both times.
2. **88% of merge-group package-jobs disappear.** C labels it a
   "package-level approximation" and admits runtime cross-crate reads are
   missed and root files conservatively select everything — then still
   spends the number in the summary, the 80–88% runner-minute target, and
   Phase 3's gate ("≤20% of package-jobs"). With honest tracing, shared
   fixtures, `mise.toml`/workflow/tooling reads and generated files will
   pull many of those jobs back. The direction is right; the magnitude is
   a guess wearing a measured costume.
3. **`velnor-dispatch` can replace `Runner.Listener` and feed the official
   `Runner.Worker` over its pipe.** The `spawnclient` ProcessChannel
   (`JobDispatcher.cs:427-502`) is an internal, unversioned
   Listener↔Worker protocol. Replacing the Listener means reimplementing
   broker sessions, message handling, renewal, cancellation propagation,
   Worker spawn/crash semantics and self-update — the fragile half of the
   official runner — to save ~10 s per PR by C's own estimate against a
   tuned setup. The lifecycle also double-books acquisition: step 2 has the
   dispatcher call run-service `acquirejob` and "hold the full job message",
   step 6 has the Worker complete the job "against the run service", without
   tracing who owns acquire/renew/complete in the real protocol (pack §F).
   Settle with a spike that runs one real job end to end under a Rust
   pipe-holder before Phase 4 is planned around it.
4. **Splitting looped tests gives a ~40× step speedup for free.** The
   diagnosis is excellent [verified: 40-case array at
   `crates/velnor-actions-cli/tests/image_publication_public.rs:86`], but
   `274 s / 40 ≈ 7 s` assumes cases are independent and share setup for
   free. The same job spends 89 s in "Prepare image publication fixture"
   (C §1.3); if per-case isolation needs fresh registry/fixture state, split
   cases replicate that cost and total CPU rises even as wall falls. The
   journal test similarly burns the same turso parse CPU in parallel rather
   than removing it. Settle by splitting one suite and measuring both wall
   and runner-minutes.
5. **The push-to-merge baseline: median 85.1 min, p90 456.6 (n=98).**
   "Push" is the first PR run on the final head SHA, so the clock includes
   agent/human idle time after the last push and excludes all earlier
   pushes; the sample is PRs merged 11-16…11-18 only, i.e. it is dominated
   by the 11-18 regime (median 399.5 that day). Scripts live in the author's
   scratch and cannot be re-run by a reviewer. D's differently-selected
   sample gives 48.73/373.40. Neither is the pack's number, because the pack
   never measures push-to-merge — C deserves credit for measuring it at all,
   but the 12–25× headline improvement is computed against C's own most
   favourable baseline.

### Factual errors

- **"The evidence pack puts it at 79% counting only Plan's queue"
  (C §1.2).** Wrong. T5's 57.0 min is the *sum* of Plan + Required verdict +
  Required queue medians (`evidence.md:195`), 57.0/71.8 ≈ 79%. Plan alone is
  21.6 min (30%). Ledger CLI does queue in parallel with Plan (T3/T4), but
  that does not make the pack's figure Plan-only.
- **"PR runs execute a median of 17 of 31 jobs today" (C §2, §5.1) vs the
  pack.** The pack's T7 says jobs executed/run median 39 for every event,
  and T3 shows top jobs running 77/77. The truth is a counting dispute the
  pack's Method note exposes: "ran" counts jobs with both timestamps
  *including 0 s jobs* (`evidence.md:403`). D recounts 25 executed of 39
  records by conclusion. C's 17/31 may be defensible under its own counting,
  but it never defines it, and it contradicts the numbers it cites as [EP]
  elsewhere. As written, a reader cannot reconcile 17, 25 and 39.
- **Setup = 31.8% + teardown = 9.7% of runner time.** C's step aggregation
  risks the non-additivity the pack warns about: test, clippy and doc steps
  overlap ("steps are not additive", `evidence.md:404`). Any percentage
  built by summing step durations over jobs with parallel background steps —
  including the 15.7% JS-action share — needs the overlap handling stated;
  it is not.
- **"Rust 1.99 already links with rust-lld"** is correct (and H's error),
  but C then prices mold at "links … ≤5.5 s per test unit including
  codegen" from a macOS `cargo --timings` run. macOS linking (Mach-O/dSYM)
  does not price ELF DWARF linking of ~19 test binaries on the EPYC nodes.
  Same flaw in the debuginfo table: macOS CPU-seconds extrapolated to Linux
  with "I expect more". D refuses exactly this inference; D is right.
- **GitHub Free / 20 concurrent hosted jobs as the queue explanation.** The
  pack never states the org plan; C's `gh api orgs/tailrocks` read is [M]
  but unverifiable here, and the concurrency limit for a public repository
  is not shown to be the binding 20. T11 shows 86 queued jobs of which only
  61 are CI; C's hosted-minute totals for the competing workflows
  (6,400 + 1,040 min) are [Est] from 10-run samples. The queue cause is
  plausible, not established — and it matters, because Phase 0's gate
  (hosted queue ≤1 min) assumes cluster admission, not a plan change, fixes
  it.

### Missed bottlenecks or risks

- **Verdict-reuse failure mode is silent false green, and detection is too
  weak.** A nightly canary and 2% random re-execution detect reuse errors
  *after* merges have landed on them; with no push re-verification (also
  C's proposal), a systematically missed input class (e.g. an untraced
  `include_str!` through a computed path, hard-decisions' own named residual
  risk) merges broken code until the nightly run. "Same confidence" needs a
  pre-merge argument, not sampling. D's start-at-0% stance on cross-event
  test reuse is the correct default.
- **Forks routed to GitHub-hosted re-enter the original queue.** "Free for
  this public repo" ignores that hosted concurrency is the resource whose
  saturation caused 11-18; fork PRs would be the only PRs still taking
  5–57 min of gate queue, and first-contributor approval/maintenance of a
  second lane is unbudgeted. The supporting claim "PRs are limited to
  collaborators, so volume is about 0" is uncited; the pack contains no
  fork count.
- **`max_entries_to_build` 2 → 6 is unmodelled.** With `HEADGREEN` grouping,
  wider speculation multiplies invalidated merge-group runs when a front
  entry fails; on 11-18 the queue collapsed at 64–82 min per group run, so
  6-wide speculation before dedup lands could triple wasted fan-out. C
  gives no queueing model (D at least bounds throughput: two 5-min groups ≈
  24 builds/h).
- **No treatment of pre-job admission.** D measured 497 s before the first
  job record on a real run and a 25.45 min admission p90 on 11-18. C's
  "GitHub creates the run and the plan job: about 3 s" budgets nothing for
  this; whatever GitHub does before `created_at` is outside all three
  designs, and only D names it.
- **Trusted-builder write race.** "On each main advance, builds main into
  the trusted mbx namespace" reintroduces, in milder form, the tip race that
  killed state seal (7/16 at `Check state tip`) unless publication is keyed
  purely by content with no "latest" pointer. C §4 GC note says
  content-addressed keys avoid the race, but the builder's trigger,
  fencing and partial-publication behaviour are never specified.
- **High-clock hardware spend** rests on boost clocks "quoted from memory
  and not fetched" (C's words). Sustained all-core frequency on a desktop
  part vs a 48-core EPYC under 30 concurrent jobs is a different comparison;
  1.3–1.5× on the serial rustc frontend is the best case of the best case.

### Ideas worth taking

- **Measuring push-to-merge at all, and splitting it into segments**
  (PR CI, green→enqueue, queue→merge, GitHub's own 19 s + 26 s latencies).
  This is the brief's actual metric; H never touches it and D samples it
  more narrowly. Adopt C's segmentation as the reporting standard, whatever
  baseline validation does to the exact medians.
- **The looped-test diagnosis** — one `#[test]` iterating 40 (or 6) serial
  cases sets the step duration — with the fix list in §5.5 (split cases,
  parallelise the 12 `threads-required = "num-test-threads"` golden tests
  [verified in `~/Projects/tailrocks/velnor-new/.config/nextest.toml`],
  fix `-j2`). This is the highest-value code finding in the panel; D
  independently confirms the single-case-equals-step medians.
- **Placement-after-assignment as the architectural question.** C is the
  only proposal to state clearly *why* ARC + per-pod Listener cannot do
  cache-affine placement (GitHub gives an acquired job to any idle runner;
  `generatejitconfig` takes only `{name, workFolder}`). Even if the answer
  is H's generate-JIT-after-acquire trick rather than a Rust Listener, the
  final design must answer C's formulation.
- **"Affinity is second-order if every node is pre-warmed from main"** —
  an honest deflation of H4's strongest form, with the missing NIC
  measurement (`iperf3`, Phase 0) named instead of assumed LAN speeds.
- **Agent-procedure fixes with zero engineering cost**: auto-merge on PR
  open (0/100 today), drop catch-up merges of main (17 found in 15 sampled
  PRs), each of which buys a full PR run that the merge queue makes useless.

### Verdict

Closest to the goal, but not at the same confidence as written. C has the
best metric, the best critical-path diagnosis, and honest provenance tags —
then spends its credibility on fanotify verdict reuse, which converts
"we traced what ran" into "we know what would run" without the prototype or
the soundness argument that `docs/goals.md` ("skipped only when the change
provably cannot affect it") demands. Its single biggest flaw is that the
88% merge-group saving — the difference between its 3–7 min push-to-merge
and D's 4–9 — rests entirely on that unbuilt, unproven reuse mechanism;
remove it, and C is an excellent, more conventional design that should say
so.

## Proposal D

### Weakest assumptions

1. **The 12-second replay proves routing yields 11.37 min in production.**
   The replay is the best counterfactual in the panel — it reconstructs
   Required completion to the second on 50 runs — but it substitutes the
   cluster *median* queue (T4: 0.2 min med, 0.3 p90, max 4.5,
   `evidence.md:167`) for every hosted wait while retaining hosted job
   durations and assuming no new contention. D's own §8a shows workers at
   92–94% in one sample and 160 maximum runners across three orgs, and T6
   shows job durations inflating under load (runner-host median 3.7 min on
   11-16 → 8.8 on 11-18, `evidence.md:202-204`). Moving Plan, Ledger, lints
   and both gates onto the cluster adds exactly the load the replay holds
   constant. D labels it "before cluster contention", then spends it as the
   Phase 1 gate (p50 ≤12, p90 ≤20). Settle by routing the gates for one
   cohort and comparing against the replay prediction at matched load.
2. **Warm placement is achievable while keeping ARC and the official
   Worker.** D's placement rule minimises queue + materialisation + compile
   time over allowed slots, but D's own protocol section concedes the
   blocker: scale-set offers carry limited metadata, GitHub may assign any
   eligible job to any idle runner in the set, and `generatejitconfig`
   takes only `{name, workFolder}` (`evidence.md:1169`). "If offers cannot
   identify the configuration, bind actual identity after assignment, then
   fetch" is not affinity; it is a same-site fetch after random placement.
   Coarse label pools are the only mechanism actually specified. This is
   C's placement-after-assignment problem, restated but not solved. Settle
   with a spike showing a chosen job landing on a chosen warm node through
   the real offer path.
3. **mbx hit rates of 72.5% local / 90.4% local-or-site.** Every input is
   invented: 75% warm placement × 90% exact match + 25% × 20%, site covering
   65% of misses, two-thirds eligibility. D's own count is 98.6% of
   hit-or-miss lookups but only 40.7% of all categories, "much of it
   within-job reuse" — i.e. there is no cross-job hit measurement at all
   (pack per-pass shares 40–74% use yet another denominator,
   `evidence.md:324`). Cross-job persistence additionally depends on
   reflink semantics D lists as unverified (`reflink=1`, §8a.3), path
   remapping, and build-script guarantees left at "preserve until
   qualified". The Phase 2–4 gates (≥70/80/85% object hits) are these
   assumptions restated as acceptance criteria.
4. **Halving maintenance, CLI and both runner-host durations is an
   engineering plan.** Phases 2–3 are gated on replays that halve job
   durations (9.47, then 6.97 min), but no intervention is measured to halve
   anything. Splitting the looped tests — correctly diagnosed; median
   longest case 260.7 s of a 260.8 s CLI step — assumes cases are equal and
   independent (`261/4 ≈ 65 s` "if the cases cost equally") while the same
   CLI job spends 89 s median preparing its image-publication fixture
   (`evidence.md:251`), which split cases may replicate. For maintenance,
   the phases D prices (pin-mirror 83.8 s, diagnostics-build 106.6 s) are
   Cargo builds, not separable fixture prep; splitting preparation from
   execution moves that CPU, it does not remove it. D also notes some
   local-mode single tests exceed 15 min, then sets a Phase 4 p90 ≤10 min
   that those tests break until they are individually fixed.
5. **The new generator cutover plus proved affected selection delivers
   3–5 min warm PRs.** The pack is blunt: the `velnor-gen`/`velnor-ci` pair
   "is not wired in yet" (`evidence.md:26`), and D adds a root-workspace
   migration and dogfood qualification across five workspaces on top.
   Selection soundness requires complete positive input declarations for
   the opaque maintenance program (which launches the CLI, image inventory,
   Python/Bun tests and archive guards); D's fail-closed rule — correct in
   itself — means every undeclared input selects the whole scope, so the
   saving is whatever fraction of PRs both avoid root/tooling paths (12 of
   the last 100 first-parent changes touched them, by D's count) and have
   fully declared closures. That fraction is never estimated per cohort, so
   "warm ordinary PR" does the work the arithmetic should.

### Factual errors

- **"The missing primary source key in 48/48 Rust logs exactly matched the
  paired Plan's successful key" is tautological, not a measurement.** Rust
  jobs do not recompute the sources key; they consume
  `needs.plan.outputs.sources_key` [verified: `ci.yml:671`]. A match is by
  construction and excludes none of D's own candidates (service URL, ref
  visibility, `@actions/cache` path-version). The state `compat` hash, by
  contrast, *is* recomputed per job with lane-specific inputs (`ImageOS`,
  `store`, `checkout` absolute paths; see the H section). D's framing
  sends the reader to check the key formula last for the one family where
  the formula cannot differ, and never separates the two families.
- **Summary: maintenance-helpers spends "roughly four minutes preparing
  fixtures".** No fixture-preparation phase appears in D's own phase
  medians or in the pack (T10: one `Execute named check` step, 486 s median,
  96% of the job, `evidence.md:245`). The named phases are Cargo builds
  (job 134191252252: 44.43 s + 88 s + 36.78 s of builds before 245.9 s of
  nextest). "Fixtures" mislabels compilation and inflates what "split
  preparation from execution" can save.
- **"PRs had 39 job records but only 25 executed at median; groups and
  pushes executed 39."** The pack reports jobs executed/run median 39 for
  every event (T7, `evidence.md:211`) under a method that counts
  zero-duration records (`evidence.md:403`). D counts success/failure
  conclusions — defensible, but then §2's "the median PR already skips 14
  of 39 records" (load-bearing for its H5 verdict) is an artefact of
  D's counting against the pack's denominator, never reconciled. Between
  C's 17/31, D's 25/39 and the pack's 39, no reader can tell how much
  selection already happens.
- **The 45-second empty-selection budget is below measured job durations.**
  D allows 20 s for "protected plan and remaining lightweight checks", but
  Plan's median *duration* (excluding queue) is 0.9 min and Required
  verdict's 0.7 min in the scaleset era (`evidence.md:177,182`). Warm
  cluster execution will shrink these, but 45 s end-to-end requires Plan,
  the lints and both gates to run in less time than Plan alone takes today
  with its work already minimal.
- **Phase 1's runner-minute gate (≤55) permits regression.** D's own
  measured median is 52.92 runner-min/PR. A gate above the baseline cannot
  detect the contention cost the replay assumes away.
- **Legacy test numbers measure D's sandbox, not legacy.** The 537
  full-workspace failures come from a scratch sandbox that "rejected tests
  that create hard-coded global `/tmp` paths"; the pack's less restricted
  run had 8,172 passes / 50 failures (`evidence.md:1058`). D caveats this,
  but the numbers still sit in the architecture section as qualification
  evidence for what to port. They qualify nothing except the sandbox.

### Missed bottlenecks or risks

- **Pre-job admission is diagnosed, then dropped from every target.** D
  found 497 s before the first job record on run 44307369619 and a 25.45 min
  admission p90 on 11-18 — the best new bottleneck finding in any proposal.
  The 3–5 min and 4–9 min budgets then allow 10–15 s for admission. D names
  no lever over whatever GitHub does before `created_at`; if admission
  correlates with the same hosted saturation that caused 11-18, routing may
  fix it, but that is a hypothesis D never states, let alone tests.
- **Push-to-merge targets assume away the merge queue's human and grouping
  behaviour.** D's own join gives trigger-to-merge p50/p90 48.73/373.40 min
  including people, other PRs and re-entry (28 of 150 queue PRs needed 2
  group runs, 7 needed 3–4; T7). The ≤9 min median target is conditioned on
  "automatically enqueued PRs with immediate admission", but unlike C (0/100
  auto-merge today, drop catch-up merges) D proposes no agent-procedure
  change to make that condition true, and `max_entries_to_build: 2` with
  HEADGREEN invalidation stays as a throughput cap D models (24 builds/h
  at 5-min groups) but never revisits as a setting.
- **No fix for the journal CPU inside the long tests.** D says "profile
  journal creation, fsync, Docker fixtures" and stops. C's profile puts the
  time in `turso_parser` prepare/translate per journal open, and the pack
  lists PRs #1333 (validate schema once, linear not quadratic) and #1349
  (cache historical-schema journals) as already-landed levers
  (`evidence.md:950`). Splitting looped cases without the parse fix
  replicates the same CPU across shards — wall falls, runner-minutes (D's
  own Phase 3 gate, ≤32) may not.
- **The 9-of-16 State producer failures at `Clippy`/`Wait for parallel
  checks` (`evidence.md:423`) get instrumentation, not a diagnosis.** H's
  `event.json` non-atomic-overwrite mechanism (verified in the H section)
  is the only explanation on the table; D's "expose restore/save outcomes"
  would not have found it.
- **External-PR isolation is a fallback to the broken lane.** If the
  VM/Kata/Firecracker boundary is unavailable, D routes forks to "an
  isolated hosted runner" — re-entering the hosted queue whose saturation
  caused 11-18, with no fork volume measured to show the cost is small.
  The pack contains no fork count; neither does D.
- **No effort estimate anywhere.** Phases have gates but no durations. The
  node agent, site CAS, signed verifier, generator cutover and workspace
  migration are the panel's largest single work programme after H's, and
  "start in velnor-lab, then measure" is not a schedule. C's 8–10 weeks for
  less scope is the only calibrated comparison available.

### Ideas worth taking

- **The replay method itself**: reconstruct the original critical path to
  the second, then change one variable. Whatever the final design, adopt
  this as the evaluation harness for routing, class sizing and queue
  settings — it is the only causal (not additive) estimate in the panel,
  and it correctly retains pre-job admission instead of hiding it.
- **Three clocks and pre-job admission as a named stage** (runner-minutes,
  verdict time, push-to-merge; admission before `created_at` owned
  separately). Adopt as reporting standard alongside C's segmentation.
- **Build-once test archives with an exact case inventory per shard** —
  reject missing, duplicated, unexplained-skipped and
  zero-when-nonempty shards. This is the confidence-preserving way to
  shard, and neither H nor C specifies the join this tightly.
- **Verdict reuse starting at 0% cross-event**, group-to-identical-main
  only (the pack-supported 120/159 same-SHA case, `evidence.md:207`), with
  signed receipts and fresh trials preserved where repetition is the
  detection mechanism. The correct default, against H's tree-SHA reuse and
  C's fanotify reuse.
- **The native-worker adoption gate**: ≥30 s or ≥10% additional p50 saving
  on the matched warm workload, no worse p90, zero unexplained semantic
  differences, official Worker as oracle — with per-hypothesis falsifiers
  throughout §2. This converts H2 from an identity question into a
  measurement, which is where it belongs.
- **§8a's zero-credit discipline**: no gain claimed for THP, writeback,
  SMT, governor or linker without a canary; capture the real Linux link
  command before pricing mold (the pack's "default `cc`/system linker" at
  `evidence.md:785` is itself unverified against rust-lld-by-default since
  1.90); verify `reflink=1` before depending on it. Adopt wholesale.

### Verdict

Most likely of the three to keep the same confidence, and its first move
(reserved cluster gates, official Worker) is the only headline claim in the
panel backed by a causal replay — but no, it does not reach minimum
push-to-merge as written. Everything after the 11.37 min routing step is
replays of assumed halvings and invented cache-hit inputs, gated on a
generator that is not wired in and a warm-placement mechanism that GitHub's
assignment model, by D's own protocol analysis, does not currently allow.
Its single biggest flaw is that asymmetry: forensic rigour about today,
arithmetic optimism about tomorrow — the 3–5 min target depends on the
placement-and-hit-rate chain (assumptions 2–3) for which D provides neither
a mechanism nor a measurement.

## Cross-cutting

**Where all three agree, and the evidence backs them.** The hosted gate
queue is the bad-day bottleneck (11-18: 57.0 of 71.8 min in Plan + Required
verdict + Required queues, T5/`evidence.md:195`; cluster jobs queue
0.2/0.3 min, `evidence.md:167`), so moving the eight hosted jobs to reserved
cluster capacity is the correct first move in every proposal. The caches
are dead as built: sources restore misses on cluster lanes, no restored
toolchain in 58/58 applicable logs (D §1; T9a), all 3,636 sampled state
restores skipped, mbx remote unset everywhere. All three retire the
state seal GitHub-cache bundle path (65–107 s transfers at 13.6–22.4 MB/s,
10 GB cap, tip races) in favour of node/site persistence, pre-stage tools
and actions so jobs download ~nothing, and treat the push-after-merge_group
run on an identical SHA (120/159, `evidence.md:207`) as the safest dedup.
C and D independently establish the panel's most important workload fact —
the heavy steps are single looped `#[test]`s (median longest case equals
step median) — and all three converge on build-once plus sharding, though
only C and D see that the looped tests must be split first.

**Where they conflict, and which side the evidence supports.**

- *Native execution now, later, or only if it wins.* H builds a new runtime
  around pre-warmed pods and a Rust stack in 18 days; C replaces
  `Runner.Listener` with a Rust dispatcher feeding the official Worker over
  an internal pipe; D keeps the official Worker and admits a native one
  only past a ≥30 s/≥10% paired gate. The measured stakes support D:
  cluster dispatch is ~12–13 s median, JS action overhead on the critical
  path is single-digit seconds (D §2), and the 11.37 min replay is achieved
  with the official Worker's recorded times. Nothing measured justifies
  reimplementing broker sessions, renewal and Worker lifecycle first.
- *How far verdict reuse may go.* H reuses by tree SHA, C by fanotify read
  sets (88% of merge-group package-jobs gone), D starts cross-event reuse
  at 0% and allows only group-to-identical-main receipts. The evidence
  supports D: only 7/45 PR head trees equal the group tree (T7), tree/SHA
  equality ignores event context, a read trace cannot prove what an
  unexecuted branch would read, and `docs/goals.md` permits skipping only
  what a change "provably cannot affect". C's mechanism may still be right
  for *selection within* a run, with full runs as backstop — the narrower
  claim `docs/hard-decisions.md:199-204` actually makes.
- *Cache affinity: the design centre or second-order?* H builds reflink
  slots and a node agent around affinity; D writes a placement cost
  function with no assignment mechanism to execute it; C argues affinity is
  second-order if every node is pre-warmed from main. On current evidence C
  is safest: there is no cross-job hit measurement from anyone (denominators
  of 40.7%, 40–74% and 98.6% are all within-job or per-pass), no NIC
  throughput number (C at least schedules `iperf3`), and no demonstrated
  way to steer a specific GitHub-assigned job to a specific node. Local
  *persistence* is proven valuable against the 13.6–22.4 MB/s GitHub path;
  per-job *placement* is unproven.
- *Why sources restore misses.* H: the `compat` hash includes lane-specific
  absolute paths. D: the missing key "exactly matched" Plan's key, so look
  at URL/ref/version. The code supports a split verdict: for the sources
  family the key is computed once in Plan and passed via
  `needs.plan.outputs.sources_key` (`ci.yml:671`), so D's match is
  tautological and H's mechanism cannot apply there; for the state family
  the compat hash *is* recomputed with `ImageOS`/`store`/`checkout`
  absolutes, where H's mechanism is real. Neither proposal prints the two
  lanes' computed keys or `RUNNER_TEMP` from one run, which would settle the
  sources half in an afternoon.
- *The linker.* H prices mold at 3×–5× over `ld.bfd`; C and D note Rust
  ≥1.90 defaults to rust-lld and no link timer exists anywhere (T10). C/D
  are right; D is most right — capture the production link command before
  pricing anything, since the pack's own "default `cc`/system linker"
  (`evidence.md:785`) is also unverified.
- *How fast, honestly.* H: 42–50 s typical. C: 3–7 min push-to-merge. D:
  3–5 min to verdict, 4–9 to merge. The floors refute H (one indivisible
  test of 234–261 s in each heavy cohort; fan-out span 8.2–13.5 min today,
  T5) and bound C/D: their ranges are reachable only after the looped tests
  are split, the journal parse cost is removed, and admission/queue
  behaviour outside the runner (D's 25.45 min admission p90; C's 7.5 min
  median green-to-enqueue, 0/100 auto-merge) is fixed by procedure, not by
  runner design. On timelines, C's 8–10 weeks for a subset of H's scope is
  the only estimate that survives contact with the component count.

**Hypotheses judged differently after reading all three.** H1 (native Rust
actions dramatically speed CI): downgraded to *fails* in its dramatic form —
D's ~6 s of JS-action time on an 11.37 min critical path is decisive; native
adapters are a correctness/maintenance choice, not a speed lever. H2
(from-scratch Rust runner beats official + ARC): downgraded to *unproven,
probably second-order* — the wins all three actually price come from
routing, persistence and selection around the official Worker; D's paired
adoption gate is the right way to ever revive it. H3 (Kubernetes as the
base): *partly holds*, more weakly than stated — k3s places pods in ~12 s,
but useful parallelism is set by the build graph, jobserver coordination
and three-tenant admission, and one unbounded pod already ate 83.7 CPUs
(`evidence.md:517`). H4 (node-aware local caches): split — local
persistence *holds* against the measured GitHub transfer rates, but the
strong affinity form is *unproven* and, per C, possibly unnecessary if all
nodes are pre-warmed from main; no proposal measured a cross-job hit. H5
(workflows select exactly the needed verification): *partly holds* only in
D's fail-closed, declared-closure form with scheduled full-run backstops;
in the strong cross-run-reuse form (H's tree SHA, C's traced read sets) it
is *unproven* and, as written in both, would trade confidence for speed —
exactly what the brief forbids.

---

## Key (added by the manager after the review)

- Proposal H: Gemini 4 Argon
- Proposal C: Claude Opus 5.5
- Proposal D: GPT-6.2-Astra

Note from the manager: this review was written in two sessions. Meta returned HTTP 503 and 504 errors on long-context requests to this model, which stalled the first session after it finished Proposal H. A fresh session given the same inputs kept that section unchanged and wrote Proposal C, Proposal D and Cross-cutting. The partial first attempt is kept in the scratch directory.

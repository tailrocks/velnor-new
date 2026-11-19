# Comparison of nine Rust-only redesigns

**Method.** Nine independent answers to `rust-only-brief-v1.md` (12 binding owner decisions; sections 1, 2, 2b, 3-12) were read in full and tabulated side by side. Everything is extraction: numbers, file:line citations and claims are quoted or closely paraphrased from each answer, and "—" means the model is silent. Display names are used throughout. No figure was re-measured and no claim was checked against the code, so where models disagree about a fact the disagreement is recorded in "Contested questions" with the evidence each cites. Three caveats for reading the tables. First, the models define their clocks differently: Opus starts from the later of the final push and the label event; Fable from the request (label or push while labelled); Astra and Sol add separate label-to-verdict and human-hold clocks; Muse Wonder, Muse Wonder Full and the Gemini 4 models time a labelled final push. Second, the "today" baselines differ by model (for example 285-347, 372, 460-490, 477.73 and 515 runner-minutes per merged PR) because each counted different workflows. Third, in the latency table Muse Wonder's "today" cell for the first stage reads "3.7 s median" where Opus and Fable give 3.7 min for the same measurement; it is copied as written. All "M" figures trace to the shared evidence pack; the models tag their own numbers as measured, estimated or target.

## Targets

PR verdict is label (or labelled push) to green `Required`; push-to-merge is the final labelled push to merged on `main`. Today's measured baselines quoted by the models: PR verdict 26.7 / 92.3 min P50/P90, push-to-merge 85 / 457 min.

| Model | PR verdict P50/P90 committed | PR verdict stretch | Push-to-merge P50/P90 committed | Push-to-merge stretch | Runner-min per merged PR (and per release/cache) |
|---|---|---|---|---|---|
| Claude Opus 5.5 | <= 60 s / <= 3 min | 25 s / 90 s (about 2 s after pre-push verify) | <= 2 min / <= 5 min | 70 s / 3 min | about 17 sandbox-min per merged PR (today about 515 runner-min); release 2.2 sandbox-min (today 3.9 tag rebuild + ~180 for 3 CI runs) |
| Claude Fable 5.2 | P50 <= 45 s, P90 <= 3 min (phase 2, week 7) | 25 s / 90 s; 12 s when agent ran velnor verify first | <= 2 min / <= 6 min | 75 s / 3 min | <= 30 runner-min per merged PR (22 phase 2; 12 phase 3) vs 460-490 today; per release 0.25 vs 3.2-7.2 (tools) or 68-104 (product) |
| GPT-6.2-Astra | 90 s / 240 s (labelled, review-complete, normal admitted load) | 30 s / 100 s | 180 s / 480 s from final push to merge | 90 s / 240 s | ~30.5 runner-min per merged PR all-attributed (commit <=35; core CI 19); today 477.73 (PR #1234). Cache 1.37 GB/merge internal (0.5 ordinary) vs >=8.6 GB logged today. Release 3-5 runner-min (18-25 incl prebuilt) vs 103.97 today (77.48 macOS) |
| GPT-6.2-Sol | 90 s / 250 s (labelled, merge-ready final push) | 35 s / 110 s | 3 min / 8 min | 75 s / 4 min | 32 runner-min per merged PR (14 P/preflight + 6 Q + 1 M + 2 superseded + 2 warmers + 4 audits/oracle + 3 optimised/preview); stretch 14; today 285.3 CI (3x52.9+64.6+62) or 372.2 with seal/preview/qualification/image. Cache: 0 GitHub compile-cache GB, 0.60 GB cross-node per warm edit, 1 GB retained. Release (Linux) 8-18 runner-min vs 71.37/103.97 today |
| Muse Wonder | <= 90 s / <= 4 min (phase 3, labelled cohort) | 60 s / 2.5 min; phase-4 pre-push 30 s / 100 s | <= 3 min / <= 8 min | 1.75 min / 5 min; phase 4 100 s / 4 min (50-75 s with merge train) | <= 35 runner-min per merged PR (stretch 15); today 285-347 est. Per release 5-10 runner-min (publish-only) vs est. 120-200 today; cache: GitHub cache 0 on verification path, node delta only (2.5 MB clone + changed outputs; last 3 generations/root, 40% volume cap) |
| Muse Wonder Full | 90 s / 4 min | 35 s / 2 min (phase 3 stretch 60 s / 2.5 min, <=20 s after preflighted push; phase 4 30 s / 100 s) | 3 min / 8 min | 100 s / 4 min (about 50-75 s with merge train, stretch only) | <= 35 runner-min per merged PR (stretch <= 15) vs 285-347 today; per release <= 5 (stretch <= 2) vs full rebuilds; GitHub cache 0 on cluster path (today 3.49 GB held, 0 restores); node store last 3 generations/root ~10.6 GiB logical each, shared by reflink, 40% volume cap |
| Gemini 4 Neon | <= 60 s / <= 3.0 min (phase 3) | <= 20 s / <= 75 s (<= 8 s preflighted) | <= 2.0 min / <= 6.0 min | <= 55 s / <= 2.5 min (<= 25 s with Velnor train) | <= 25 runner-min per merged PR incl batch (stretch <= 12) vs 285-347 today (true ~450-520 incl cancelled/state seal); cache <= 0.15 GB delta per PR (+~1.5 GB mbx-cache on 10.5% lockfile PRs), 0 GB GH cache vs 3.49 GB churn; per release <= 2.0 min (stretch 0.5) vs ~60-110 min; release cache 0.05-0.8 GB vs ~6-12 GB |
| Gemini 4 Carbon | <= 25 s / <= 55 s (calc 17 s / 47 s) | <= 12 s / <= 25 s (calc 9 s / 22 s) | <= 75 s / <= 150 s (calc 72 s / 143 s) | <= 60 s / <= 105 s (calc 58 s / 99 s) | <= 8.0 runner-min per merged PR (3.2 PR + 2.4 MG/spec + 2.0 main warmer + 0.4 audit), stretch 3.5; today 347; cache GH 3.49 GB -> 0; cluster disk written ~966 GiB -> ~0.6 GiB; per release 0.5 runner-min (0 rustc) vs 38-240 |
| Gemini 4 Argon | 45 s / 150 s (phase 3) | 18 s / 55 s (<= 10 s after pre-push velnor verify) | 110 s / 4.5 min (arithmetic 61 s / 146 s) | 65 s / 120 s (<= 16 s with Velnor CAS merge train) | 24 runner-min per merged PR (1.3 labelled PR runs x 9.5 + 0.15 x MG delta 4.0 + ... + 1.5 builder + 2.0 speculation + audit ~5), stretch 11; today 285-347; per release 1.5 (0.8 stretch) vs 42-68; GH cache 3.49 GB (ceiling 10.54) -> 0; L1 NVMe per worker 88 GiB (3 main generations x 5 roots incl debug/clippy/doc/--release musl + 4 GiB tools + 2 GiB mirror + 15 GiB mbx store) |

### Stated theoretical floor

| Model | Stated theoretical floor and what it consists of |
|---|---|
| Claude Opus 5.5 | ~50 s fixed: hook 1 s + plan 0.5 + C(delta) + T(delta) + report 0.3 + enqueue 2 + GitHub creates group 19 + lookup 1.5 + GitHub merge 26. Median PR ~60 s; pre-pushed ~50 s; runner-host PR ~80 s. Irreducible: GitHub queue 45 s, delivery 1-2 s, rustc critical path, slowest affected test, ~6 API RTTs, human label time |
| Claude Fable 5.2 | 66 s push-to-merge: event->job held 9 s + completejob->check green 2 + auto-merge->enqueue 5 + enqueue->group 19 + group job lookup 5 + group green->merged 26 (55 s of it is GitHub merge queue; PR check 11 s). With pre-push verify verification overlaps the first 9 s; typical V=23 s gives 80 s. Irreducible: GitHub event/queue latency, rustc per changed crate + link, longest changed test (125-138 s today, 12-20 s after), signing 2-4 s |
| GPT-6.2-Astra | 54 s illustrative fast path for preverified change: 2 s ingress + 1 proof + 1 status + 2 enqueue + 19 s group creation (M) + 2 proof + 1 status + 26 s finalisation (M). Says GitHub supplies no fixed lower bound; new input adds longest causal chain (serial link, longest unsplittable test, live observations); human label wait unbounded |
| GPT-6.2-Sol | 56.2 s center: 4 push observe + 0.2 bind + 2 publish head + 2 enqueue + 19 group (M) + 3 group proof/publish + 26 merge (M); no SLA. Novel small warm edit adds ~24 s (8 compile + 16 tests). Lower bound for novel input = longest dependency path, total uncached work / capacity, unavoidable transfer; irreducible: GitHub exact-SHA observation/queue/ref update |
| Muse Wonder | Verdict ~35 s typical (~15-20 s pre-verified): webhook 1-2 s + fetch/merge-tree/plan ~2 s + delta compile 10-15 s + longest test <=15 s stretch/60 committed + lookup/completion ~5 s. Push-to-merge ~80 s via GitHub queue (50-65 s pre-verified; GitHub queue 45 s = 19 s + 26 s). Via Velnor merge train (stretch, would conflict with decision 7) ~40 s (10-25 pre-verified). Irreducible: webhook, delta serial rustc chain, longest test, completion round trips, GitHub queue, C5 obligations (network/clock/random/publishing) always execute |
| Muse Wonder Full | ~20 s PR verdict, ~65 s push-to-merge for median PR: webhook 1 s + fetch/merge-tree/plan 1 + sandbox 0.3 + delta compile 8 + tests 5 + witness 3 + GitHub queue 45 (19+26). Dominated by GitHub 45 s queue and single longest test. Irreducible: one RTT to learn of push, one rustc pass, one longest changed test, 45 s GitHub queue while GitHub owns merge. Velnor merge train removes the 45 s (stretch, owner decision) |
| Gemini 4 Neon | Cold push on GitHub MQ ~60-65 s (1.5 + 0.4 + 3.5-5 compile + 5-8 tests + 15 s Actions job offer overlapped + 45 s queue + 2); preflighted on GH MQ ~64 s Actions / ~50 s App check run; absolute floor ~3.8-4.5 s preflighted (~15-18 s cold) only with App-native Required check run AND Velnor CAS merge train (git merge-tree 0.3 + receipt 0.4 + CAS ref update 0.8). GitHub MQ 19+26 = 45 s declared an absolute platform floor |
| Gemini 4 Carbon | Floor A (inside decisions 7 & 12, Actions Required): ~58 s push-to-merge, ~8.6 s verdict (8.0 s GitHub job offer + 19 + 8.5 MG job + 26). Floor B (App check run as sole Required, still GH MQ): ~47 s push-to-merge, 0.7 s pre-verified verdict / 3.2 s cold leaf. Floor C (Velnor CAS merge train replaces GH queue): 1.3 s pre-verified / 3.5 s un-preflighted. Components: push 0.45 s, mount 0.025 s, leaf compile 1.1 s, tests 0.4 s, job offer 8.0 s, GH MQ 45 s |
| Gemini 4 Argon | Push-to-verdict floor 8.5-15 s, bounded by GitHub JobAvailable emission (8-15 s), compute ~0.05 s when pre-verified. Push-to-merge floor 55-61 s with GitHub MQ (1.0 + 0.05 + 0.25 + 2.2-6.2 compile + 2.5-8 tests + 8-15 job offer + 46 MQ) or 10-16.5 s with Velnor CAS merge train (1.5 s merge transition). GitHub MQ 19 + 26 = 45 s |

## Latency budget

Per stage from brief section 2. Each cell is the model's design time (committed unless noted), with today's time in parentheses where the model gives it. All models trace the typical generator or helper PR (47% class) with the label present, except Astra and Sol, whose cells are central/slow scenario pairs (typical / slow in seconds).

| Stage | Claude Opus 5.5 | Claude Fable 5.2 | GPT-6.2-Astra | GPT-6.2-Sol | Muse Wonder | Muse Wonder Full | Gemini 4 Neon | Gemini 4 Carbon | Gemini 4 Argon |
|---|---|---|---|---|---|---|---|---|---|
| 1. GitHub event, webhook, job offer | 1.5 s webhook (today: run 1-2 s, first job 3.7 min P50 / 26.3 P90) | ~9 s to job offer (6 s run create + 2 + 1), but webhook at 1-2 s starts work early (today push->run 6 s, run->first job 2 s median/p90 445 s; Plan waits 5.5 min median hosted) | 3 s typical / 8 s slow to ingress; offer 10/25 in parallel (today 2/226 s run creation->first job; 60/626 s superseders) | committed 8/24 s, stretch 4/12 (today PR created->first job 3.7/26.3 min; Plan queue 5.5/23.1 min) | webhook 1-2 s; ensure starts on webhook, GitHub job creation ~15 s off critical path (today push->run 1-2 s; run->first job 3.7 s med/26.3 s p90 [as written]; admission 2 s p50/226 s p90/2,203 s max) | 2 s (webhook 1-2 s starts ensure; GitHub creates job ~15 s in parallel) (today run 1-2 s; admission 2 s p50/226 s p90/2,203 s max; supersede wait 60 s p50/626 s p90) | 1.5 s webhook; 0 s stall (today run create 15-20 s + admission 2 s p50/226 s p90/2,203 s max); stretch 0 s via velnor verify | 1.2 s webhook starts ensure (job offer 8 s/15 s overlapped); 0.0 s post-push with velnor verify (today 2 s/226 s p90, max 2,203 s) | 1.2 s / 2.5 s webhook; stretch 0.8 s (today 2 s / 226 s; supersede 60 s p50/626 s p90; GitHub takes 8-15 s to emit JobAvailable) |
| 2. Acquire | 0 s on PR path; Actions jobs elsewhere <=0.3 s (today set-up 5.3 s, scale-set assignment 12 s) | 0.25 s acquirejob, pipelined with sandbox assignment; pre-registered JIT sessions (today 13 s p50 / 25 s p90 job created->started) | 0.45 s / 1.4 s (today 12/18 s incl assignment) | 0.7/1.5 s; stretch 0.5/1 (today in 12/18 s scale-set created->started) | broker msg -> first step <=1 s P50/<=2 s P90; placement before acquire (today idle ARC 2-3 s; on-demand pod 6 s + 12-14 s job start) | <= 1 s broker msg -> first step (today idle ARC 2-3 s, on-demand 12-14 s; 8 hosted gate jobs queue 4.1 min median/23.6 p90) | 0.2 s pre-acquired JIT pool (today hosted Plan queue 5.5 min med/23.1 p90; scale-set 12 s); stretch 0.0 s with App Check Run | 0.18 s / 0.35 s overlapped (today hosted gates 246/1,416 s each; cluster 12/18 s) | 0.18 s / 0.40 s; stretch 0.08/0.20 (today 12 s / 23.6 min incl hosted gate queue 4.1 min P50) |
| 3. Sandbox start | 0.3 s: namespace <=0.05 s + reflink 0.25 s (today ARC pod 6 s, max 16) | cell 5-20 ms; view 0.3-0.5 s (reflink 0.25 s) (today pod 5.6 s median / 9.8 s p90 + dind) | 0.05 / 0.20 s overlapped with acquire, leased pool namespace (today U within ARC) | 0.6/1.5 s overlapped; stretch 0.3/1 (today in queue/setup; ephemeral runner + DinD) | pre-created single-use pod claimed <0.2 s; reflink 0.25 s (today pod ~6 s; dind sidecar; emptyDir ext4) | <= 0.3 s pre-created cgroup slot (today pod ~6 s [436 starts] + dind; 57 probe failures/h) | 0.015 s native namespaces + cgroup v2 + overlayfs (today 12-14 s ARC on-demand / 2-3 s idle + 3-4 s DinD probe) | 0.03 s / 0.08 s; stretch 0.02 s (today 12-14 s cold ARC; 2-3 s warm minRunners) | 0.05 s / 0.15 s pre-warmed pod/cgroup sandbox; stretch 0.01 s clone3 (today 6 s / 14 s) |
| 4. Checkout | 0.6 s site mirror + diff write (today 6 s/job; Plan full clone 12 s) | 0.1 s inside view (739 of 6,913 files; mirror fetched on webhook) (today 4.2 s per job, 11.2 s Plan) | 1.5 / 4 s local Git objects (today 6/8 s; Plan 12 s) | 1/4 s; stretch 0.6/2 (today 6/8 s maintenance helpers) | <=1 s warm native checkout from node mirror (today 6 s med; 8.6 s + 10.75 GiB target copy) | <= 1 s sparse checkout from node mirror (today 6 s median) | 0.15 s local bare mirror materialise; stretch 0.05 s (today 6 s med / 8 s p90 per job x 31) | 0.30 s / 0.60 s; stretch 0.05 s (today 6.0/8.0 s per job x up to 39) | 0.15 s / 0.35 s; stretch 0.05/0.12 (today 6 s / 8 s per job x 25-39) |
| 5. Toolchain and tools | 0 s node tool store (today 14-15 s/job) | 0 s verified read-only dm-verity image (today tools 9.8 + mise 2.4 + setup 4.2 s per job) | 0.4 / 1 s verified immutable tool set (today mise 2/5 + tools 10/18 + Rust 1/2 s) | 0.6/2 s; stretch 0.2/1 (today tools 10/18, mise 2/5 s) | <=0.5 s read-only L1 stores, ~100% hit (today toolchain restored 0/58, sources 0/48; tools 10 s med/18 p90; mise 2 s) | 0 s read-only stores (today 10 s median/18 p90 per job; toolchain restored 0/58; action downloads ~4.4 s/job) | 0.005 s read-only bind-mount (today 10 s med/18 p90 tools + 2 s mise + 1 s verify) | 0.01 s / 0.02 s read-only bind mounts (today 14.0/26.0 s per job; 0/58 hits) | 0.02 s / 0.05 s read-only bind mount (today 14 s / 26 s per job) |
| 6. Dependency fetch | 0 s; 3 s if lockfile changed (today 4 s fetch + 1 s restore) | 0 s; <=1 s per new lockfile (10.5% PRs) (today 4.6 s per job; sources cache 0/48 hits) | 0.5 / 3 s offline Cargo inputs (today 3.5-6.1 s; 2.7-3.3 via mirror; sources cache missed 48/48) | 1/5 s; stretch 0.3/2 (today lookup 1/1 s, 48/48 misses, 435-522 downloads) | crates mirror + L1 sources >=99% hit; fetch overlapped with plan <=2 s (today 3.5-6.1 s internet; 2.7-3.3 mirror) | 0 s on hit; ~3 s on miss from site crates mirror (today 3.5-6.1 s direct; 2.7-3.3 mirror; sources cache 0/48) | 0.0 s (89.5% PRs); 1.5 s lockfile change via crates-mirror (today 3.5-6.1 s; 0/48 hits; 435-522 crates/job) | 0.02 s / 0.80 s; stretch 0.01/0.30 (today 4.5/6.1 s; 0/48 hits) | 0.00 s / 0.25 s (2.7 s on 10.5% lockfile PRs) (today 4.5 s / 7.1 s) |
| 7. Compile | 25 s committed / 8 s stretch incremental (today 8.5 min critical path) | 10 s typical class; 67 s runner PR today -> 15-25 s after crate split (today 14 s velnor-ci, 81 s runner-host cold; critical path builds CLI 4x at -j2, 381 s) | 24 / 68 s (16 s delta + 8 s link; slow = 67.4 s lockfile rebuild) (today helper 14/23, runner-host 81/116, local-mode 88/126 s) | 25/65 s; stretch 8/30 (today ci-helper 14/23, runner-host 81/116, CLI fixture 89/115 s) | median delta 15 s (8.8 s M5 x1.7); unchanged 0.31 s; cold node via mbx-cache 39.9 s vs 92 s (today cold 92 s/357 crates; 46-497 crates/job; lockfile change 67.4 s) | 15 s delta (8.8 s M5 x 1.7); unchanged 0.31 s/0 crates (today 196-497 crates/job; 81 s runner-host, 14 s velnor-ci) | 12 s median PR; 0.31 s unchanged; 25-40 s split runner-host; stretch 6 s with incremental + dep opt-level 3 (today 81-88 s build + 70-75 clippy + 60-65 doc runner-host; 14-16 s velnor-ci) | 8.5 s / 24.0 s; stretch 0.30 s / 8.0 s (today 81/126 s runner-host+clippy; maintenance-helpers 486 s) | 6.5 s / 22.0 s (0.25 s clone + 6.2 s delta velnor-ci/gen; 18-22 s split runner crates) (today 14 s / 126 s per job; maintenance 486 s) |
| 8. Tests | 10 s affected-only (today 192-246 s runner jobs, 486 s maintenance) | ~10 s changed closures, run twice in parallel (today 258/238/192-246 s heavy jobs ran in 97.4% of PR runs) | 29 / 99 s (25/90 longest lane + 4/9 inventory) (today helper 10/62, runner-host 192/417, maintenance 486/570 s) | 40/130 s; stretch 16/50 (today ci-helper 10/62, orchestration 78/104, host local 246/497 s) | ~10 s lanes median PR; Journal::open <=1 s; no test >60 s (phase 2); changed obligations x3 parallel (today floor 125-138 s; loops 258-274 s; 924.8 s receipt test; maintenance 486 s) | 10 s lanes for median PR; run 3x parallel (today runner-host 192 s med/417 p90; actions-cli 238; velnor-ci 10 s/62; floor 125-138 s) | 10 s median PR (3x parallel); <= 45 s runner-host/actions-cli after template; stretch 5 s median, <= 15 s worst (today 10/62 s velnor-ci; 192-246 med/417-497 p90 runner-host; 238 actions-cli; 486 maintenance) | 6.0 s / 18.0 s; stretch 3.0/9.0 s (today 192/497 s; actions-cli 238/284; local-mode 246/497) | 4.5 s / 28.0 s (velnor-ci 2.5 s x 3 reps) (today 10 s / 497 s) |
| 9. Result reporting | 0.3 s one check-run PATCH (today queue 1.6 + 0.7 run + 1.9 Required queue min) | 1.5 s step logs + completejob (today job end 5 s, report upload 2.9 s, Required verdict 49 s of which 32.7 s downloads) | 1.2 / 4 s (today 3 s upload, 3 merge, 1 publish) | 2/4 s; stretch 1/3 | Results twirp streaming; binaries via site store by digest (today nextest archive 1.48 GB upload 36-43 s; ~825 s/run overhead est.) | 1 s streaming to Results (today nextest archive 1.48 GB upload 36-43 s) | 1.0 s Results Twirp + WS + completejob; stretch 0.4 s App Check Run PATCH (today 1-2 s flush + 0.7 + 0.1 min verdict jobs + 3.5-38.6 min queue) | 0.40 s / 0.80 s; stretch 0.30/0.60 (today 36/65 s artifact upload + Required verdict 42 s) | 0.35 s / 0.80 s; stretch 0.15/0.30 (today 12 s / 45 s artifact upload + 37 artifact downloads) |
| 10. Required check (PR verdict) | 38 s committed / 20 s stretch (today 30.4 min P50, P90 99.3) | 0 s extra: witness job is the check; App check +0.2 s (today two hosted jobs, queue 1.6 and 1.9 min median) | 2 / 6 s; one protected App writer (today hosted verdict queue 1.6/24.3 min, wrapper 1.9/21.4) | 4/8 s; stretch 2/5 (today verdict 0.7 min after 1.6/24.3 min queue; Required 0.1 min after 1.9/21.4 min queue) | verdict = last receipt + ~5 s job completion; single writer (today three hosted gates waited 57.0/71.8 min on 11-18) | 5 s after last receipt; witness job completes with verdict (today 3 serial hosted gates; queue 57.0 of 71.8 min on 11-18) | ~25 s compute, ~30-45 s p50 waiting for GH job offer (15 s) (today 26.7/92.3 min; 10.9 min unqueued); stretch ~15-20 s, <= 8 s preflighted | 0.10 s / 0.20 s; the cluster Required witness IS the check (today 6 s run + 114/1,284 s hosted queue) | 13.0 s / 55.0 s median/heavy PR verdict (committed <= 45 s P50 / <= 150 s P90); stretch 7.5/25 s or 1.5 s pre-verified (today 26.7/92.3 min; 10.9 unqueued) |
| 11. Merge-queue entry and run | 22 s (enqueue 2 + group 19 + lookup) (today green->enqueue 7.5 min + 19 s + group run 24.4 min) | auto-merge enqueue ~5 s + 19 s + job 2 s + lookup/complete 3 s (today green->enqueue 7.5 min, run 19.9 min P50/84 P90) | enqueue 4/15 + group creation 19/55 + group 8/105 s (today 19 s median enqueue->group; group wall 19.9/84.1 min) | 52/170 s (4+19+29 / 10+45+115); stretch 24/85 (today green->entry 7.5/87.9 min; entry->group 19 s; group 19.9/84.1 min) | enqueue->run 19 s; group lookup 30-60 s when hit, >=85% satisfied by receipt (today group 64.6 runner-min; tree equal 0-18%) | ~60 s: enqueue seconds + 19 s + lookup + 26 s; 78-89% groups execute nothing (today green->enqueue 7.5 min med/88 p90; group 64.6 runner-min; 0/40 auto-merge) | 2 s auto-merge enqueue + 19 s MG creation + 2 s receipt hit = ~23 s (85%+ hit) (today green->enqueue 7.5 min med/88 p90 + 19 s + MG run 19.9/84.1 min); stretch 0 s with Velnor train | 29.5 s / 56.3 s (2 s auto-merge + 19 s MQ + 8.5 s receipt hit); stretch 28.0/42.0 s (today manual wait 450/5,280 s + 19 s + MG run 1,194/5,046 s) | 22.0 s / 65.0 s (2 s auto-merge + 19 s MQ + 1.0 s lookup) (today green->enqueue 7.5/88 min + 19 s + MG run 19.9/84.1 min = 27.4/172.1 min) |
| 12. The merge | 26 s, irreducible GitHub (today 26 s) | 26 s (today 26 s) | 26 / 55 s (today 26 s median) | 20/60 s; stretch 12/45 (today group finish->merged 26 s) | 26 s (today 26 s; green->enqueue 7.5 min median, 0/40 auto-merge) | 26 s (included above) (today worst-day P50 400 min incl 278-282 min queue entry->merge) | 26 s irreducible on GH MQ; 3 s with Velnor CAS train (today 26 s) | 26.0 s / 40.0 s; stretch 21.0/35.0 s (today 26/45 s + 19.4 min duplicate main push run) | 26.0 s + 1.2 s post-merge push Required with 100% receipt hit (today 26 s + 21.7/59.2 min post-merge push run) |
| Total / traced path | 86 s committed / 68 s stretch (today 85.1 min P50, 456.6 P90) | verdict 25 s (2+0.5+0.5+10+10+1.5); merged at 80 s (today verdict 26.7/92.3 min; push-to-merge 85/457 min) | Additive ~62/195 s to PR verdict; ~119/425 s to merge; commit 90/240 and 180/480 (today 26.7/92.3 min PR run; 85/457 min push-to-merge) | Committed sums 82.3/243.5 s verdict and 154.3/473.5 s merge; stretch 32.6/106 and 68.6/236 (today 26.7/92.3 min; 85.1/456.6 min) | verdict ~35 s median arithmetic (committed <=90 s); push-to-merge ~2 min median (committed <=3 min). Per-class: generator/helper 47% ~35 s; runner workspace 25% 1.5-3 min then 60-90 s after split; engine 8.5% 5-6 min then ~60 s (today 85/457 min; PR run 26.7/92.3 min) | verdict ~35 s median arithmetic (2+2+15+10 = 29 s verification + 5 s); push-to-merge ~2 min (today 26.7/92.3 min run wall; 85/457 min) | ~85 s p50 (35+2+19+2+26 = 84 s) committed <= 2.0/6.0 min; weighted P50/P90 verdict ~35 s/~115 s; runner-host class ~2.1 min, lockfile ~2.7 min (today 85/457 min) | PR verdict 16.6 s / 47.0 s; push-to-merge 72.1 s / 143.3 s (today 26.7/92.3 min; 85/457 min) | 61 s P50 / 146 s P90 arithmetic -> committed <= 110 s / 4.5 min; stretch 48/76 -> 65/120 s (today 85/457 min) |

## Duplicate-work audit

Union of every duplicated operation any model listed. "Today" is the cost each model states (runner-minutes unless marked; none re-verified here). Disposition is eliminate (E), reuse by key or promote (R), or keep as a duplicate (K). Where models agree, one line covers them.

| # | Operation | Found by | Today, as stated | Dispositions |
|---|---|---|---|---|
| 1 | Runs for unrequested pushes (draft, unlabelled, ready-for-review) | All nine | 45% of 2,558 PR runs cancelled (1,156); 4.77 PR runs per merged PR (Fable); wasted per merged PR: Carbon ~95, Argon 72-115, Neon ~120-160 | E, all: label trigger |
| 2 | Re-run on unchanged head (`edited`, reopened, manual) | All nine | 469 of 2,558 PR runs (18.3%); 52.9 runner-min each | E/zero, all; drop `edited` (all but Astra, Sol who say suppress/replay a proof) |
| 3 | Rebase-only push, same tree | All nine | Full run, 52.9 | R, instant: Fable 11 s, Muse Wonder Full ~5 s, Neon ~2 s, Carbon <1 s, Argon 0.5 s; others "lookup" |
| 4 | Push N then N+1 | All nine | Heavy jobs in 97.4% of runs (Fable, Neon, Carbon, Argon); new run waits 60 s P50 / 626 s P90 (Astra, Sol, Muse Wonder) | R changed closures only; join in-flight work; never wait for the cancelled run |
| 5 | Concurrent, stacked, identical-tree PRs | All nine | 0 cross-PR reuse today; each PR pays 52.9 | R by content key, in-flight join; Astra, Sol also require signed receipts and re-derived keys |
| 6 | Agent pre-push test, then CI | All nine | Unmeasured (Opus, Muse Wonder Full); agent waits 11 s median, >60 s in 27% (Fable); ~15 min (Carbon); 15-40 min (Argon); 1-25 min on Mac (Neon) | R only if run through `velnor verify` in the sealed Linux sandbox; Mac runs never count |
| 7 | Plan job and planner | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Carbon, Argon | 0.9 min plus hosted queue 5.5 min median / 23.1 P90; 45-77 s repeated in qualification and seal (Fable) | R: released planner, keyed by tree, <=2 s (Fable 0.5 s) |
| 8 | Ledger CLI build, upload, 31 downloads | Opus, Fable, Astra, Sol, Muse Wonder Full, Carbon, Argon | 0.8-0.9 min per run; Fable 22 s + 31 x 3.2 s = 2 min; Argon 2.7 runner-min + 18 min queue | E: released helper in tool store or image |
| 9 | Checkout | All nine | 4.2-6 s per job x 31-39; 2.7-3.5 min per run; Argon 117 checkouts, 11.7 runner-min per merge | R: node Git mirror fetched on webhook; sparse or diff write |
| 10 | Toolchain, mise, tool install | All nine | 14-15 s per job; 7.5-8 min per run (Opus, Fable); Argon 27.3 per merge; 706 runner-min in sample (Muse Wonder) | E: read-only verified stores |
| 11 | Action download and JS overhead | Opus, Fable, Muse Wonder, Muse Wonder Full, Argon | Opus 17 s/job, 8.8 min/run; Fable 148 s/run; others 4.4 s/job; Argon 8.6 per merge | E: native actions |
| 12 | Crate fetch | All nine | 4-5 s per job, 2.3-2.4 min per run; sources cache 0 of 48 hits | R: once per lockfile, offline |
| 13 | Third-party dependency compile | All nine | Opus 2,260 compilations of 463 unique crates (4.9x); Fable 2,106 of 475 (4.4x), 6.3-21.7 GB per job discarded; 196-497 crates per job | R: once per lockfile/toolchain/profile via snapshot |
| 14 | Workspace crates compiled in several configurations (runner-host default and `local-mode`, lib vs test) | Opus, Fable, Astra, Sol, Muse Wonder Full, Neon, Carbon, Argon | Opus 404-407 crates twice; Fable four ways (34.6+82.5+99.0 s M5); Muse Wonder Full 11.7 runner-min/run | R once per closure key; extract `velnor-runner-local` and split runner-host (Fable, Muse Wonder Full, Neon, Carbon, Argon) |
| 15 | Clippy, doc, check-alone lanes | Opus, Fable, Astra, Sol, Muse Wonder, Neon, Carbon, Argon | Clippy 36-75 s, doc 30-65 s per job; Argon ~38 runner-min per merge | R per key; K as separate parallel lanes (different outputs) |
| 16 | `maintenance-helpers` `-j2`, image-publication fixture, repeated CLI builds | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | `-j2` caps cost 4.5 of 8.5 min (Opus); fixture 89 s per job; Fable >=9 CLI builds per event, 381 s | R once; remove 8 caps; fixture into warm `--all-targets` |
| 17 | Test execution across PR, merge group, main | All nine | ~25 min per run (Opus); most of 1,838 nextest-s per run (Fable) | R by receipt; K changed obligations x2 (Opus, Fable), x3 (Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon), distinct samples (Astra, Sol) |
| 18 | Lints (fmt, deny x5, machete, jactionlint, alint) | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon | 1.2 min plus hosted queue; Fable 8 hosted jobs 3.8 min | R keyed; pin RustSec DB by commit (most); Astra, Sol keep a live freshness check |
| 19 | `Required verdict` / `Required` aggregation hops | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Carbon, Argon | 0.8 min + 3.5 min queue (Opus); 49 s (Fable); three hosted gates 57.0 of 71.8 min (11-18) | E: ledger query or witness job |
| 20 | Full CI on merge group | All nine | 64.6; PR tree equals group tree in 7 of 45 (Opus, Astra, Sol), 0-18% (Muse Wonder, Muse Wonder Full) | R exact-tree else per-obligation lookup; estimate 78-89% identical (Sol disputes) |
| 21 | Full CI on push to main | All nine | 62.0; same SHA as merge group in 120/159; false failure 8.1% (10/124) | E/lookup; Astra, Sol keep one real Actions witness for the release gate; cold audit replaces it |
| 22 | State seal producers and sealers | All nine | Opus 19.7/push; Fable 683 s; Sol 11.93-21.20; seals 0/18 success; GitHub cache 3.49 GB | E: builder generations or CAS |
| 23 | Cache prune | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | ~0.5 min (Opus); 2 x 25 s per push (Fable) | E |
| 24 | Source qualification on every PR event | All nine | Opus 24.0/run; Fable 1,007 s/event = 80 per merged PR; Sol 31.8-43.15; 267 runs on 11-18 | R once per key, on demand or in merge group only |
| 25 | Job image candidate build and attest on every push | All nine | Opus 3.4/push; Fable 200 s (~20 per PR); Sol 5.57-5.85; 306 runs on 11-18 | R once per image-input key; `images/**` and main only |
| 26 | Source preview Linux build and qualify on main push | Opus, Fable, Astra, Sol, Neon | Opus 7.4; Fable 410-534 s; Astra 28.65 (PR #1234); Sol 7.73 | R: once per product closure; this is the release artifact |
| 27 | macOS and Darwin builds | All nine | Opus 22.6 per push; Fable 1,063-1,406 s; Sol 46.53-77.48 per release; Neon 13 jobs | E: paused in phase 0, target-keyed so it can return |
| 28 | Release binary rebuild at tag (velnor-ci, velnor-gen) | Opus, Fable, Sol, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | Opus 3.9 (1.8+1.4+2.2); Fable 190-434 s; Carbon 38-240 per release | R promote the bytes built once at merge or on main |
| 29 | Product release builds (SDK, images, host) | Opus, Fable, Astra, Sol, Carbon | Opus ~29; Fable 4,105-6,238 s (27 jobs); Astra 103.97 (77.48 macOS) | R promote Linux; macOS paused |
| 30 | `<bin>-<target>.tar.gz`, `SHA256SUMS`, attestations | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Argon | <0.2 min each (Opus); Astra 0.4-1.1 each; Sol 0.40-1.33 | K once: compute at build, verify and attest at promotion |
| 31 | Release smoke on shipped bytes | Opus, Fable, Astra, Neon, Carbon | Opus: today only `--version`/`check-tag`; Fable qualification 204-258 s after merge | New or moved: test shipped bytes once (Opus ~10 s) |
| 32 | release-plz set-up and release eligibility repeated | Fable, Neon, Argon, Sol | Set-up twice per release run (Fable, Neon); eligibility 6 times (Argon); 1.55-2.08 min (Sol) | R once |
| 33 | Embedded commit id forces a rebuild per commit | Opus, Fable, Astra, Neon | `option_env!("VELNOR_COMMIT")` at `io/mod.rs:78` (Opus, Fable); `build.rs` SHA (Astra) | Opus: build in group, group SHA becomes main's; Fable: stamp after link; Astra: exact main build |
| 34 | Nightly, scheduled, freshness runs | All nine | Opus ~60 min/day when enabled; Carbon ~65 per night; Fable freshness 87-134 s weekly | K as audit on batch (Fable keeps freshness fresh) |
| 35 | Cold audit of landed main commits (new) | All nine | Does not exist today | K deliberately: Opus tip audit <=2 h; Fable A1 per commit; Carbon 6 h; others 24 h |
| 36 | Conformance runs (oracle twice) | All nine | Fable 2,377-2,856 s per dispatch; Carbon ~90 | K, lab only, never merge path |
| 37 | Per-node `main` rebuild by warmers | Opus, Fable, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | ~120 incremental builds/h at 8 merges/h (Muse Wonder, Muse Wonder Full) | R one builder per site; Fable none (merge-group output harvested) |
| 38 | Catch-up merges of `main` into PR branches | Opus, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | 7 of 15 PRs, 17 commits (Opus) | E: agent rule |
| 39 | 1.48 GB nextest archive upload | Opus, Muse Wonder, Muse Wonder Full | 36-43 s per job | R: changed binaries by digest to site store |
| 40 | Docker daemon in every pod | Opus, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon | 3-4 s per job, 57 probe failures per hour | E: only 3 Docker test binaries get a daemon |
| 41 | GitHub cache save and restore | All nine | 0 of 3,636 state restores; 1.92 GB restore 44-61 s | E from verification path |
| 42 | Speculation and races kept for speed | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Carbon, Argon | Carbon ~1.5 min batch CPU saves 20-45 s; Muse Wonder Full: fetch 39.9 s vs build 92 s, 43.4 vs 67.4 s | K with numbers; wrong guess only wastes batch CPU |

Single-model rows: Fable also counts tools/sources producers (28 of 72 failed) and `velnor-actions-cli` built at least 9 times per event; Astra adds release-plz intent reconciliation and registry `.crate` bytes; Sol adds exact `.crate` publication; Muse Wonder adds the qualification workflow becoming the corpus seed and the release version-bump PR kept once.

### Per merged PR and per release totals (today -> design)

| Model | Runner-minutes and cache GB per merged PR and per release (today -> design) |
|---|---|
| Claude Opus 5.5 | Per merged PR: ~515 runner-min today (23 macOS) -> ~17 sandbox-min (audit ~65%); cache: 14,000 package compilations discarded, ~0.6 GB GitHub-cache restores -> ~1.5 GB delta per main generation per site, 2.5 MB reflinks, 0 GB GitHub cache. Per release: 3.9 min tag rebuild + 3 full CI runs (~180) -> 2.2 sandbox-min once, 0 rebuilds |
| Claude Fable 5.2 | Per merged PR: 460-490 runner-min today (155-170 hosted) -> 22 (phase 2) / 12 (phase 3): verification 13.4, MG 1.8, audits 6.7, speculation+push 0.6. Cache: >=1.4 TB compile output written and discarded + 3.49 GB GH cache restored by 0/3,636 -> 9 GiB written (3.1 GiB delta x1.7 + 4 GiB cold audit); ~3 GiB per generation; ~75 GiB per warm node. Per release: 3.2-7.2 or 68-104 runner-min -> 0.25; 1-3 GB built-and-discarded -> 0 compiled, ~30 MB assets |
| GPT-6.2-Astra | Per merged PR: 477.73 runner-min (PR #1234 across 2 heads, 2 group candidates, main) -> 30.5 (19 core + 4 optimised products/smokes + 2 source/image + 1 warm/speculation + 4 audit/oracle + 0.5 control; commit <=35). Cache >=8.600 GB logged -> 1.37 GB/merge (0.50 ordinary; hot receipt-only KB; target zero external CI cache GB). Per release 103.97 runner-min (77.48 macOS), 0 GB explicit cache -> 3-5 runner-min (18-25 incl prebuilt once), 0 build-cache GB |
| GPT-6.2-Sol | Per merged PR: 285.3 CI runner-min (3x52.9+64.6+62) or 174.8 success-only; 372.2 with seal 12.95 + preview 25.45 + qualification 31.8 + 3x5.57 image -> 32 (stretch 14). Cache: witnessed Plan transfer 0.43 GB (5x0.086), state publication 0.097-1.457 GB, repo cache stock 3.495 GB -> 0 GitHub compile-cache GB, 0.60 GB cross-node per warm edit, 1 GB new retained. Per product release: 71.37/103.97 runner-min (Linux 24.83/26.48), 1.321 GB published -> 8-18 runner-min (6-14 main + 2-4 publisher), 0 compile-cache GB, 0-1.2 GB transfer, ~1.16 GB outward |
| Muse Wonder | Per merged PR: 285-347 runner-min (3-4.8 PR runs x 52.9 + MG 64.6 + push 62.0) -> <=35 (stretch 15); cache GitHub 3.49 GB active with 0/48 hits + 10.75 GiB per-job target copy -> GitHub cache 0, node cost delta only. Per release: est. 120-200 runner-min -> 5-10 (publish only); new 10.6 GiB target dirs per release -> 0 new GiB |
| Muse Wonder Full | Per merged PR: 285-347 runner-min (3-4.8 PR runs x 52.9 + MG 64.6 + push 62.0) -> <=35 (stretch <=15). Cache: GitHub 3.49 GB held (0 restores on cluster) -> 0; node last 3 generations/root ~10.6 GiB logical shared by reflink. Per release: full rebuilds per target in three product workflows + release-rust-ci-tools -> <=5 (stretch <=2) copy/checksum/attest/publish only |
| Gemini 4 Neon | Per merged PR: 285-347 runner-min today (true ~450-520 incl cancelled and state seal) -> <= 25 (PR delta ~2.5 + 3x changed ~1.5 + MG lookup ~0.2 + main builder ~3 + speculation ~4 + 24 h cold audit ~12), stretch <= 12. Cache: 0 GB reused, 3.49 GB GH cache churn + ~35 GB/job ephemeral disk I/O -> <= 0.15 GB XFS/overlay delta (+~1.5 GB mbx-cache on lockfile PRs), 0 GB GH cache. Per release: ~60-110 runner-min, 6-12 GB -> <= 2.0 runner-min (0 compile), 0.05-0.8 GB |
| Gemini 4 Carbon | Per merged PR: 347 runner-min -> 8.0 (stretch 3.5) (3.2 PR + 2.4 MG/spec + 2.0 main warmer + 0.4 audit). Per release 38-240 -> 0.5. GH cache 3.49 GB -> 0. Cluster disk written ~966 GiB (10.75 GiB ext4 copy x 30 jobs x ~3 runs) -> ~0.6 GiB (<150 MB upperdir delta x 4 lanes) |
| Gemini 4 Argon | Per merged PR: 285-347 runner-min -> 24.0 (stretch 11). Per release: 42-68 -> 1.5 (stretch 0.8). GH cache 3.49 GB (up to 10.54 GB ceiling) -> 0. L1 NVMe per worker 0 -> 88 GiB (64 GiB stretch) |

## Key design choices

Cells are paraphrases of each model's own text. "—" means the model is silent on that point.

### (a) How `Required` is produced and how the label flow is enforced

| Model | `Required` is produced by | Label flow and the skipped-job trap |
|---|---|---|
| Claude Opus 5.5 | App-owned check run `Required`, bound to the App by ruleset `integration_id`; no Actions run per push | Unlabelled head gets a queued "Not requested" App check, which blocks. Cites the docs rule that a skipped job reports Success |
| Claude Fable 5.2 | Actions job `Velnor / Required` inside a reusable-workflow callee, plus a second App check `Velnor / Ledger`; both required | Caller is skipped when unlabelled, so the callee check is never created and stays pending. Says "observed, not documented"; lab proof planned |
| GPT-6.2-Astra | App commit status `Velnor / Verified` (pending/failure/success) bound to the App, plus a PR-scoped admission witness via the required-reviewer rule | Never a conditional job (skipped and neutral count as success). A genuine Actions `Required` stays only on the main push for the release gate |
| GPT-6.2-Sol | App-owned `Required` commit status bound by `integration_id`; any Actions coordinator job has a different name | Statuses remove skipped/neutral. Same-SHA aliasing between two PRs closed with managed topic refs and a ref-writer gateway |
| Muse Wonder | One Actions job `Required`, bound to integration 15368, with no job-level `if:` | First step is a native gate: label absent or draft means the job completes with conclusion failure ("CI not requested"). Skip never occurs |
| Muse Wonder Full | Actions job `Required` with a label `if:`, bound to 15368 | Acknowledges the skip hole; adds an always-running one-step job that fails without the label, and the executor refuses success without the label |
| Gemini 4 Neon | Reusable-workflow caller `ci` calls `ci-verify.yml`; check is `ci / Required`, bound to 15368; App-native check run in phase 4 | Caller `if:` skipped means callee never expands, so the check stays Expected. States skipped, neutral and success all pass |
| Gemini 4 Carbon | Two required checks: Actions `Required` (15368, label `if:`) and App check `velnor/verdict` | App check is queued "Not requested" on unlabelled heads (absent also blocks), closing the skipped-equals-success hole |
| Gemini 4 Argon | Always-run Actions job `Required` (15368), no job-level `if:`; `velnor-ci ensure --require-pr-label` | Fails closed with conclusion failure when unlabelled and on `unlabeled`. Rejects job-level `if:` because skipped counts as success |

### (b) Executor architecture and crates

| Model | Architecture and crates |
|---|---|
| Claude Opus 5.5 | `velnor-exec` per Actions job under a node agent; executor off the merge path. Crates gh, control, dispatch, exec, actions-native, node (youki libcontainer), store, conformance |
| Claude Fable 5.2 | wire, dispatch, exec (library in node agent), actions, node, ledger, store, hooks, conformance; about 62k lines; executor is a task in the agent, cells not pods |
| GPT-6.2-Astra | Extends nested `velnor-runner` workspace (github, core journal/fencing, exec, actions, host) plus `velnor-verify` planner/receipts and App controller; `ensure`/`execute`/`promote` |
| GPT-6.2-Sol | Listener in node daemon, fresh worker process per job; crates protocol, exec, actions, host, receipts and controller, conformance; pure work starts before offer, job joins |
| Muse Wonder | `velnor-dispatch` (placement before acquire), exec, hooks (label state machine), ledger, node; single-use UID-deleted pods per class; line budgets |
| Muse Wonder Full | Privileged DaemonSet agent carves cgroup/namespace sandboxes, no pod per job; dispatcher, hooks, ledger; `Required` is a witness job joining running work |
| Gemini 4 Neon | Ten crates, 58k LOC: proto, expr, dispatch (Raft/SQLite lease), exec, actions, node with `velnor-sandbox` (under 15 ms), ledger, hooks, store, conformance |
| Gemini 4 Carbon | proto, expr, dispatch (HA pair), exec, native-actions, sandbox (Landlock, seccomp, Firecracker driver), hooks+ledger, node+store, conformance; at most 60.5k LOC |
| Gemini 4 Argon | dispatch (pre-registered JIT identities), node (pre-created sandboxes, in-process exec), exec, native-actions, ledger (out-of-pod Ed25519 signer), store, hooks; 3-server k3s |

### (c) Expression evaluator

| Model | Choice and claim |
|---|---|
| Claude Opus 5.5 | Port the NEW evaluator (`Sdk/Expressions`, `WorkflowParser`, about 8k Rust lines); says latest runner still returns the legacy result (`PipelineTemplateEvaluatorWrapper.cs:312-318`) |
| Claude Fable 5.2 | Fresh port of the new evaluator (about 3.5k lines); says `Expressions2` is the "Legacy" one (`ExecutionContext.cs:1546-1562`). Legacy copy kept test-only; C# fuzzer vs both |
| GPT-6.2-Astra | One fresh Rust evaluator for the "current contract", reconciled against the latest release; says Expressions2/DTPipelines names "describe observed semantics" |
| GPT-6.2-Sol | Fresh evaluator "using the current oracle semantics"; archived expression tests as evidence only; does not say which official evaluator is authoritative |
| Muse Wonder | Exactly one evaluator, `Expressions2` copied from velnor-legacy (3,565 lines, v2.337.0); DTExpressions explicitly not copied |
| Muse Wonder Full | Copies the same velnor-legacy module; notes the Worker evaluates twice (`Wrapper.cs:63-75`); corpus carries cases where the two official evaluators disagree |
| Gemini 4 Neon | `velnor-expr` copies legacy `Expressions2` (calls it DTExpressions2 v2.337.0); no claim about which evaluator the latest runner returns |
| Gemini 4 Carbon | Copies velnor-legacy `expression/*` (3,565 LOC) plus `context_value.rs` (1,740 LOC) as "exact DTExpressions2 semantics" |
| Gemini 4 Argon | Copies the same 3,565-line module "without the legacy DTPipelines dual-evaluator wrapper" (cites `Wrapper.cs:58-75`); `hashFiles` native |

### (d) Native actions approach

| Model | Approach |
|---|---|
| Claude Opus 5.5 | Closed set from every `uses:` in `.github/` plus velnor-gen samples; native attest (OIDC, Fulcio, Rekor); adds `create-github-app-token`; github-script refused |
| Claude Fable 5.2 | Closed by action, SHA and input (unsupported input refuses the job); checkout via bare mirror alternates; supports background steps (105 uses in ci.yml) |
| GPT-6.2-Astra | Registry keyed owner/repo/path/commit/impl digest; public effects still performed when requested; local cache bytes only after service resolves the entry |
| GPT-6.2-Sol | Registry by repo/path/commit/manifest digest; byte reuse keeps original service authorisation (prefix hit cannot pass); github-script becomes typed Rust ops |
| Muse Wonder | Closed native set; unknown action or SHA refused before the job; velnor-gen emits only qualified SHAs; attest once at build time |
| Muse Wonder Full | Native actions in executor; mise action only exports PATH; no downloads; Node start is only 87-147 ms, so gain is locality not language |
| Gemini 4 Neon | Closed set incl. `create-github-app-token`, `setup-velnor`; parity suite against upstream JS actions; Node24, Docker, github-script refused |
| Gemini 4 Carbon | Registry compiled in; pre-job refusal of unqualified SHAs, `@v4` tags, github-script, node24, docker; oracle mode mirrors real Cache v2 and Artifacts v4 writes |
| Gemini 4 Argon | Compiled in, zero tarballs; adds `create-github-app-token`; says velnor-gen hardcodes hosted runners in `release.rs:29`, `release_binaries.rs:42-47` |

### (e) Conformance oracle and cutover criteria

| Model | Oracle design | Cutover criteria and drift detection |
|---|---|---|
| Claude Opus 5.5 | Lab pools oracle vs velnor; corpus real jobs, semantics, refusals, record/replay faults | 100% on 2 nightlies, 300 shadow jobs over 2 weeks with zero diffs, 12 planted bugs, start <=1 s P90; poll 15 min, <=2 h; canary 15 min |
| Claude Fable 5.2 | Six layers (about 600 semantics, 150 actions, 187 jobs, refusals, wire shapes, 1M expressions/night); mutants | Two slices: witness job week 4-5 (150 merges twin), rest week 7-8 (300 merges); mutation >=95%; hourly poll plus diff of about 40 files; canary 10 min |
| GPT-6.2-Astra | Paired real jobs, id bijection, virtual-time fault replay, positive controls | 100%, zero secret leaks; two clean passes plus 7 days clean canaries; poll 15 min, suite <=60 min, full <=4 h; says latest is v2.338.2 |
| GPT-6.2-Sol | Separate lab runner group; floor 240 semantic, 60 fault, 24 job shapes | 100% of declared cells; 200 real trees over 2 weeks; poll 30 min, probe 15 min, alert 30 min; says latest v2.338.2 |
| Muse Wonder | Paired dispatch; exact equality after normalisation; signed allow-list | 100% twice, 2 weeks and 300 merges twin, no P90 slowdown, start <=2 s; within 24 h of release; canary 10 min |
| Muse Wonder Full | Same shape; adds label-flow cases | Same four gates; ARC scale sets deleted in the cutover change; within 24 h of release |
| Gemini 4 Neon | Four suites; strict serde unknown-field logging | Five gates incl. step 1 <=500 ms P90, kill -9 faults, rollback <60 s; poll 2 h, <=4 h; 10 planted bugs; canary 10 min |
| Gemini 4 Carbon | 420 cases (85 replay, 240 semantics, 65 fault, 30 refusal) | 420/420 twice, 200 shadow runs, chaos kill -9; poll 30 min, SaaS canary hourly; rollback <30 s |
| Gemini 4 Argon | Suites A-C; also reads runner deprecation dates | 100% twice plus 10/10 planted faults, 2 weeks 300 merges, start <=0.5 s P50, 3-server k3s, 60 s rollback; poll hourly, canary 10 min |

### (f) Compile and cache

| Model | Snapshot / hot path | mbx, mbx-cache | Incremental | Release-profile strategy |
|---|---|---|---|---|
| Claude Opus 5.5 | Reflink clone of warm main snapshot, fixed path | Transport for main generations only, never in sandbox | On from phase 2; audits non-incremental | Debug tests; release built once in release PR's merge group; smoke on shipped musl bytes |
| Claude Fable 5.2 | Reflink of verified generation = merge group's own build, harvested | File-delta sync default; mbx-cache only if it beats it | Off (`CARGO_INCREMENTAL=0`) | Debug tests; release once at merge; commit stamp patched after link |
| GPT-6.2-Astra | Private XFS clones; reuse only with per-unit provenance | Optional, about 100 GiB/site, only if measured cheaper | — | Debug tests; optimised product once on verified candidate or main |
| GPT-6.2-Sol | Fixed-path XFS clones; immutable CAS for objects | Local mbx removed; mbx-cache experiment (>10% miss, 2-week P90 win) | Private unless audited | Debug gate; optimised Linux payload once on landed main |
| Muse Wonder | Reflink hot path; overlayfs only as remedy | Cross-node filler; keep if >=10% builds or >=20 min/day | Off until measured | Builder builds release once per main tip, promoted |
| Muse Wonder Full | Reflink hot path; overlayfs remedy | Hash-sync default; mbx-cache experimental, predicted to retire | Off until measured | One `verified` profile for tests and shipped bytes (opt 2, assertions on, unwind, no LTO) |
| Gemini 4 Neon | Overlayfs/reflink warm snapshot (overlay shares page cache) | Gated fill, builder to warmers only | On, ephemeral overlay, phase 4 | `ci-test` profile (deps opt 3); `K_rel` release built on main after merge |
| Gemini 4 Carbon | Multi-lower overlayfs plus zstd layer streaming | Replaced entirely | Off (`incremental=false`) | `--profile ci` (deps opt 2); release tarballs built in merge group |
| Gemini 4 Argon | Reflink plus hard-linked immutable `deps/` base; overlayfs fallback | Cross-node fill | `CARGO_INCREMENTAL=1` leaf snapshots, phase 4 | Deps opt 2 in tests; `K_rel` obligations in merge group |

All nine reject blaze (0 of 5 workspaces build, missed clippy `deny`, ignores the 1.99.1 pin).

### (g) Test scheduling and the single-test floor

| Model | Scheduling | Floor attack | Target |
|---|---|---|---|
| Claude Opus 5.5 | nextest longest-first; cross-node only when it pays | Journal template + FICLONE, one test per case, optimised deps (-66%), high-clock node | <=30 s phase 2, <=10 s phase 3 |
| Claude Fable 5.2 | LPT over free threads, binaries by digest, hedged restart past P95 | Template x0.55, opt deps x0.34, whole cores x0.75, high clock x0.7 | None >60 s phase 2, >20 s phase 3 (12-14 s) |
| GPT-6.2-Astra | Expensive first; move only missing binaries; inventory check | Journal template <=1 s, split 40 and 6 cases; warns dep-opt trial had assertions off | <=60 s, stretch <=15 s |
| GPT-6.2-Sol | nextest archive/partition, same-site stealing; 10 s tail beats 1.2 s transfer | Template <=1 s, split loops, remove 8 `-j2` caps | <=60 s, stretch 15 s |
| Muse Wonder | In-sandbox lanes; fan-out phase 4 if summed/threads > longest + transfer | Template, one test per case, prefix clones, crate split | <=60 s phase 2 |
| Muse Wonder Full | Same; spreading phase 4 | Template, prefix clones, de-serialise 12 goldens, split runner-host (156,822 LOC) | <=60 s phase 2, 15 s stretch |
| Gemini 4 Neon | Fan-out when summed CPU/slots > longest + transfer; 150-350 MB by BLAKE3 | OnceLock template, split 40/12/10/6/4-case loops, `test-threads=1` removed, 6-crate split | 924.8 s to <=60, <=15, <=8 s |
| Gemini 4 Carbon | EWMA per test, LPT across up to 5x96 threads | Merge 356 test binaries to 1 per package, `gix` rewrite of 45,000-process test, template | No test >12-15 s |
| Gemini 4 Argon | Site-wide LPT fan-out in phase 4 | Template <=10 ms, prefix clone, unroll loops, 6 crates | <=60 s phase 2, 15 s stretch |

### (h) Verdict reuse and the soundness case

| Model | Key completeness | Detection | Automatic fallback |
|---|---|---|---|
| Claude Opus 5.5 | Closure tree hashes, all manifests, toolchain, env, CPU class; eBPF read audit incl. ENOENT | Twin run on 2 nodes; 2% re-exec after 3,000 clean; tip audit <=2 h | Kind drops to exact-tree keys; policy epoch |
| Claude Fable 5.2 | Sparse cell of the closure; every root manifest in key; `CARGO_BIN_EXE` digests; time and randomness cannot be keyed | A1 cold build per landed commit, digest compare; A2 nightly and time-shifted | Package, root, exact tree, always run |
| GPT-6.2-Astra | Domain-separated manifest incl. compiler, executables, absence; read traces only propose declarations | Shadow 3,000, 2% sampling, nightly cold audits | First reused-pass/cold-fail revokes class and descendants |
| GPT-6.2-Sol | Manifest incl. executables, libraries, negative lookups, CPU facts; traces do not prove completeness | Cold shadow of new classes; audit every 20th landing | Revoke lineage; uncached full proof |
| Muse Wonder | Per-root then per-package tree hashes; sparse sandbox; enrol only if restricted equals unrestricted | 2% re-exec after about 3,000; nightly and 20th-merge cold; 24 h full audit | Retire key version; epoch bump |
| Muse Wonder Full | Same; ENOENT logged; uncomparable work always runs | Same, plus every landed tree in full within 24 h | Retire key version |
| Gemini 4 Neon | BLAKE3 over epoch, closure trees, ALL workspace manifests, env; only declared paths mounted | 3,000 shadow, 2% cross-node, every landed commit cold within 24 h | 5x rerun, then epoch bump |
| Gemini 4 Carbon | blake3 incl. `CARGO_BIN_EXE` digests; pivot_root with declared files only, so ENOENT | 3,000 shadow, then 5% re-exec; every landed commit cold within 6 h | `POLICY_EPOCH` invalidates class, halts queue entries |
| Gemini 4 Argon | Key struct (epoch, sparse input tree, lock subset, definition, manifest); `K_bin` in phase 4 | 3,000 shadow, 2% sampling, nightly cold landed trees | Receipt invalidation by epoch |

### (i) Merge queue

| Model | Grouping | Settings | Speculation |
|---|---|---|---|
| Claude Opus 5.5 | ALLGREEN (phase 2) | build 5, timeout 15 | Trees T_i plus ejection trees (k<=2); kill below 70% match |
| Claude Fable 5.2 | ALLGREEN | build 8, wait 0, timeout 15 | Merge-tree onto queue tip when green with auto-merge, plus trees minus one entry; kill <70% |
| GPT-6.2-Astra | ALLGREEN now | build 2 then 4, 8 only if gain; merge 1/1; timeout 20 | Only after paired replay, waste <5%; no depth rise without measured gain |
| GPT-6.2-Sol | ALLGREEN | build 2, merge 1, timeout 90 then 20 | Main + PR + up to 4 ordered prefixes; >=70% match; says only 7/45 trees equal |
| Muse Wonder | HEADGREEN first, ALLGREEN later | build 2 to 4/8; timeout 90 to 20 | Prefixes and ejection trees (7 for 4 PRs); kill <70% |
| Muse Wonder Full | HEADGREEN until bypass trees verified, then ALLGREEN (owner call) | build 2, 4, 8; max merge 10 | Same; also optional Velnor merge train (kill <95% match) |
| Gemini 4 Neon | HEADGREEN, ALLGREEN once >=70% | build 2, 4, 8; timeout 90, 45, 20 | 7 trees at depth 4 (4 prefixes, 3 bypass) |
| Gemini 4 Carbon | ALLGREEN | build 6, merge 6, wait 0, timeout 15 | 2k-1 trees (11 for k=6) |
| Gemini 4 Argon | HEADGREEN phases 0-2, ALLGREEN phase 3 | build 2, 4, 8; timeout 20 | Prefix and bypass trees (7 for 4 PRs) |

### (j) Release promotion

| Model | Where bytes are built once | How release consumes them |
|---|---|---|
| Claude Opus 5.5 | Release PR's merge group (musl x64 and arm64), `VELNOR_COMMIT` = group SHA; qemu smoke | Fetch by digest from `trusted` receipts; gate accepts App `Required` plus ledger verdict; never rebuilds |
| Claude Fable 5.2 | Per closure key at merge time (30-40 s); attested in main push job | Tag job: gate, fetch, re-hash, `gh attestation verify`, publish (about 15 s); commit stamp patched after link |
| GPT-6.2-Astra | When final identity known: verified candidate or main | Protected main job signs manifest; release has zero compiler invocations; idempotent publish with readback |
| GPT-6.2-Sol | Landed main builds and qualifies optimised Linux bytes | Promote signed manifest; migrate `gate.rs` and `product-release.yml` together; exact `.crate` adapter |
| Muse Wonder | Builder, once per main tip, profile in key | Promote; digest equality, rebuild is a blocking bug; attest once at build |
| Muse Wonder Full | `main` build in the `verified` profile, once | Copy, checksum, attest, publish (<=5 runner-min); strip is the only transform |
| Gemini 4 Neon | Builder on main computes `K_rel` after merge; release-PR bump built once | Tag promotes in <15 s, zero compile; verifies SHA256SUMS against signed receipt |
| Gemini 4 Carbon | Merge group, in parallel with tests | Promote exact tarball; nightly cold `--release` ELF compare; mismatch aborts |
| Gemini 4 Argon | `K_rel` obligations in merge group / release PR | Receipt lookup, fetch from store, never runs cargo build |

### (k) The cluster: placement, warm pools, isolation, both sites

| Model | Placement and warm pools | Isolation | Both sites and other orgs |
|---|---|---|---|
| Claude Opus 5.5 | Min predicted finish; twin on another node; last 3 generations warm per node; cpu.weight/cpu.idle classes | userns, cgroup v2, seccomp, XFS quotas; rootless dockerd experiment | Per-site control; receipts replicate async (wait <=1 s); ChainArgos/jackin keep ARC |
| Claude Fable 5.2 | Cells made ahead; exclusive complex-aligned cpusets; two verified generation replicas per site | Cells with quota, seccomp; docker class on tainted nodes | PR pinned to a site by number; receipts replicate about 2 s; others stay on ARC with weights |
| GPT-6.2-Astra | Ready sandboxes and idle JIT identities; tenant floors 160/100/80 + 120 elastic | Namespaces; rootless OCI or private VM; arm64 guest | One fenced operation index, single write leader; both sites serve local reads |
| GPT-6.2-Sol | 2-4 JIT sessions per site; slot classes 24/10/4 GiB; 4 threads and 12 GiB reserved | Namespaces; rootless services or warm VMs | Third-site ledger witness; says single k3s cannot survive site loss |
| Muse Wonder | Single-use pods; reserved 16-CPU slot per site; merge > gate > PR priority | Pods with seccomp, no token; 2 tainted docker nodes | Request stays in one site; ChainArgos/jackin migrate after 4-week soak |
| Muse Wonder Full | Agent-carved sandboxes (pods fallback); 4 build slots of 16 CPU/24 GiB per worker | Netns loopback-only for tests; docker class | Own dispatcher/ledger per site; receipts are hints across sites |
| Gemini 4 Neon | Native sandboxes, up to 6 per worker sharing overlay page cache; lone job 48-64 threads | Namespaces; docker on velnor-105 and velnor-190 | Independent per-site services; 3-server k3s; per-org floors and disk caps |
| Gemini 4 Carbon | Namespace sandboxes (30 ms), CCD-aligned 24 threads; jobserver burst | Landlock, seccomp; Firecracker microVM for Docker suites | Per-site autonomy; 3-node k3s etcd; ARC deleted phase 1 |
| Gemini 4 Argon | 2 pre-created sandboxes per worker; 4 slots x 22 CPUs; reserved slot per site | Namespaces, loopback-only netns; docker class | FI 288 and DE 192 threads, site stickiness; ChainArgos/jackin keep 30% share |

### (l) Confidence extras: double runs and cold audits

| Model | Double (or more) runs | Cold audits |
|---|---|---|
| Claude Opus 5.5 | Twin: changed obligations run twice in parallel on two nodes (matches today's two pre-merge runs) | Cold tip audit within 2 h (<=12 a day); 2% sampling; daily lineage restart |
| Claude Fable 5.2 | Two before merge, in parallel (kept for confidence, 0 s wall) | A1 cold on every landed commit; A2 nightly, every 10th merge, +8/+30-day shifted clock |
| GPT-6.2-Astra | Distinct required samples; fresh runs for real-time/race tests; drops triple for deterministic inputs | Nightly independent cold landed-tree audits |
| GPT-6.2-Sol | Distinct seeds/schedules in parallel only where diversity remains; no auto retry | Cold shadow; every 20th comparable landing |
| Muse Wonder | Three parallel executions of changed obligations | Nightly and every-20th cold; full audit of every landed tree within 24 h |
| Muse Wonder Full | Three parallel | Same |
| Gemini 4 Neon | Three parallel | Every landed commit cold within 24 h; every 20th merge |
| Gemini 4 Carbon | Three parallel, each name must report 3/3 | Every landed commit within 6 h; 5% re-exec |
| Gemini 4 Argon | Three-fold flake guard on changed obligations | 2% sampling plus cold landed trees |

## Critiques of v2

Union of v2 errors raised, with who raised each. v2 means `~/.cache/ci-ambition/panel/v2/velnor-runner-architecture.md` and `decisions.md`.

| # | Critique of v2 | Raised by |
|---|---|---|
| 1 | Starts verification on every push webhook (`ensure` on push, N09/N18, §3.4, §6.1); no label flow, no pending state, no `labeled`/`unlabeled` handling | All nine |
| 2 | `Required` stays an Actions job; integration 15368 is every Actions job; no analysis of the skipped-equals-success trap or workflow edits; no risk row for the gate | Opus, Fable, Muse Wonder (no `integration_id`), Neon, Carbon, Argon, Astra |
| 3 | HEADGREEN kept: a failed entry can land (#1276); commits land without a verdict on their own tree | Opus, Fable, Astra, Sol, Neon, Carbon, Muse Wonder Full (left open) |
| 4 | Three parallel executions of changed code is one more than today's two pre-merge runs | Opus, Fable; Astra and Sol say drop unconditional triple execution |
| 5 | Cold build only nightly or every 20th merge; a stale generation can serve 20 merges; detection 24 h | Opus, Fable |
| 6 | Builder recompiles each `main` generation after the merge group built it ("a merge group never writes the trusted scope"): three compiles per change | Fable; Astra ("build once globally, not per site") |
| 7 | Evaluator row: `Expressions2` is the runner's own "Legacy" evaluator and the new one is absent | Opus, Fable; Sol replaces the copied runtime part of N60 with a fresh evaluator |
| 8 | Closed action set misses `actions/create-github-app-token` (6 uses in velnor-gen release goldens) | Opus, Fable, Astra, Sol, Neon, Carbon, Argon |
| 9 | Executor scope omits background steps (105 uses), `hashFiles`, sparse checkout (44 uses), input-level refusal | Fable |
| 10 | Sparse-checkout sandbox is unsound: cargo cannot load the workspace without every member (measured, Opus); feature unification changes (Neon) | Opus, Neon; Astra and Sol say observed read sets or narrow closures cannot prove completeness |
| 11 | Keeps fork class, fork-conditional `runs-on`, hosted macOS, hosted runners; macOS disable not in phase 0 | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon |
| 12 | Release path untouched: rebuild at tag, source preview rebuilt per push, profile tension unresolved, no promotion of artifacts | Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon |
| 13 | Duplicate audit misses state seal, source qualification, image candidates, source preview | Opus, Fable, Astra |
| 14 | Pod per request puts k3s API on the hot path; fixed 16-CPU pools cap a lone PR; static reservation | Opus, Fable, Muse Wonder Full, Neon, Carbon; Sol and Astra add RAM/page cache and non-HA control plane |
| 15 | mbx-cache as default cross-node layer (N54), per-site builds, 6.8-8.7 GiB per change | Fable, Astra, Sol, Muse Wonder Full, Carbon |
| 16 | Rejects reuse of PR-produced receipts across pushes and stacked PRs (I032) | Neon, Carbon, Argon; Astra wants sealed pre-push and cross-PR single flight now |
| 17 | XFS reflink clones duplicate 14.29 GB page cache each; v2 leaves overlayfs as remedy | Neon, Carbon, Argon |
| 18 | `CARGO_BIN_EXE_*` digest hashing deferred to phase 4; executable digest too late in the key | Sol, Carbon |
| 19 | Incremental compilation "off until measured" though measured edits are 4-6x faster | Opus |
| 20 | Metric clock starts at first CI run, hiding label delay; targets assume every push verifies | Muse Wonder, Astra, Sol, Opus |
| 21 | Cancelled-run stall only moved to cluster, not removed | Muse Wonder Full |
| 22 | Missing 63-day runner window, webhook reconciliation, `merge_group.destroyed`, `check_run.rerequested`, drift detection from production traffic | Opus |
| 23 | Release polling 24 h and no live-SaaS canary; harness checked by only 10 planted differences; no wire layer | Fable, Carbon |
| 24 | Keeps ARC and the official runner in production through phases 0-2 | Carbon, Astra, Sol, Neon |
| 25 | One whole-corpus cutover; verification before push only a phase-3 stretch | Fable |
| 26 | Open Q12 (automatic retry of untouched tests) masks regressions | Neon, Carbon |
| 27 | Merge train (v2 Q11) conflicts with decision 7 | Fable (closed), Astra, Sol (remove); Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon keep it as owner-decision stretch |
| 28 | Queue depth gated on false-failure rate alone | Muse Wonder Full |
| 29 | velnor-gen hardcodes hosted runners in `release.rs:29` and `release_binaries.rs:42-47` | Argon |
| 30 | Fix test setup and transfer cost before buying 1.3-1.5x serial hardware; LOC budget too high (71k vs 58k) | Sol; Neon |

## Contested questions

Each question gives the positions, the evidence each cites, and what would settle it.

### Q1. Does a skipped required job count as success, and does a skipped reusable-workflow caller leave the inner check pending?

- **Skipped job = success:** all nine say yes. Evidence: GitHub docs "using conditions to control job execution" (Opus); status-checks and troubleshooting-required-status-checks pages (Fable, Astra, Sol, Neon, Carbon, Argon); `hard-decisions.md:216-219` (Muse Wonder).
- **Skipped caller never creates the inner check, so it stays Expected:** Fable (says "observed behaviour, not a documented promise") and Neon (says "verified against GitHub Docs"). Muse Wonder Full and Carbon instead keep a label `if:` on `Required` and add a second guard. Opus, Astra, Sol avoid the question with App-written checks or statuses.
- **Settle:** lab PR, unlabelled push, read the merge box and `required_status_checks` result for each shape (Fable work item 5).

### Q2. Which object may be the required check: App check run, App commit status, Actions job, or caller job?

- Opus: App check run bound by `integration_id`. Astra, Sol: App commit status (Astra adds required-reviewer witness; Sol managed topic refs). Fable: Actions job behind caller plus App `Velnor / Ledger`. Muse Wonder, Argon: Actions job that fails closed. Muse Wonder Full: `if:` job plus failing guard job. Neon: caller job, App check later. Carbon: Actions `Required` plus App `velnor/verdict`.
- Evidence: `ci-helper/crates/velnor-ci/src/gate.rs:15,127-159,277-288` and `product-release.yml:86` require exactly one Actions `Required` job in the main push run (cited by Opus, Fable, Astra, Sol, Muse Wonder, Muse Wonder Full, Neon, Argon). Astra and Sol note a foreign-App check is rejected by the gate today.
- Settle: owner choice of the release-gate contract; lab test that a failing-closed job, a status and a check each block merge and queue entry.

### Q3. Do GitHub App webhooks fire when a label is changed with `GITHUB_TOKEN`?

- **Delivered (asserted):** Neon, Carbon, Argon ("suppression sits in the Actions trigger router"), Muse Wonder Full ("suppression applies to run creation, not webhook delivery").
- **Not established; test needed:** Opus ("docs document no matching suppression"; week-1 test via `GET /app/hook/deliveries`), Fable ("page states no exception"), Astra, Sol ("docs do not settle delivery").
- Muse Wonder avoids it: agents label with App token or UI.
- Evidence cited: docs "Triggering a workflow" and "Webhook events and payloads"; no model measured it.
- Fallbacks: reconciler every 30 s (Opus), 60 s (Fable, Neon), 10 s watchdog with App relabel (Neon, Carbon, Argon), `workflow_dispatch` (Fable, Muse Wonder Full, Carbon).
- Settle: lab workflow labels with `GITHUB_TOKEN`; inspect App delivery list.

### Q4. Does the latest official runner still return the legacy evaluator's results, and which evaluator is "legacy" under decision 3?

- Opus and Fable: yes. Without the compare flag the runner uses the legacy evaluator (`ExecutionContext.cs:1546-1562`); with it the new evaluator runs but "still returns the legacy result" (`PipelineTemplateEvaluatorWrapper.cs:224-318`, `:312-318`; Fable also `Constants.cs:175`). `Expressions2` is that legacy evaluator, so porting new is required.
- Muse Wonder, Neon, Carbon, Argon: copy velnor-legacy `Expressions2` as the single non-legacy evaluator; Argon cites `Wrapper.cs:58-75` "without the dual-evaluator wrapper". Muse Wonder Full also copies but notes the Worker evaluates twice (`:63-75`) and carries disagreement cases.
- Astra, Sol: fresh evaluator for the "current contract", silent on which official one that is.
- Settle: read the wrapper at the lab-pinned release (note Q15 on version); oracle compare-mode mismatch telemetry.

### Q5. Does cargo refuse `-p` builds when other members' sources are missing, and is a sparse sandbox sound?

- Opus: measured on a two-member workspace with cargo 1.99.1; full tree plus eBPF read audit instead of sparse.
- Neon: stable cargo unifies features only over passed `-p` packages, so compile `--workspace` with all manifests present (`hard-decisions.md:486-513`).
- Fable (cell holds only closure paths, every root manifest in key), Muse Wonder, Muse Wonder Full, Carbon, Argon: sparse sandbox, undeclared read fails with ENOENT. Astra, Sol: start from full tree; narrow only with enforced, equivalence-checked closure.
- Settle: re-run Opus's scratch workspace; per-package enrolment experiment (restricted equals unrestricted at 100%).

### Q6. Where and when are release bytes built, and which profile do tests use?

- Debug tests, release once in the release PR's/any merge group: Opus, Fable, Carbon, Argon (Neon and Muse Wonder build post-merge on main by a builder; Astra and Sol on verified candidate or main).
- One profile for both: Muse Wonder Full (`verified`, opt 2, assertions on, unwind, no LTO).
- Evidence: Fable measured R3 (release-profile test build shares 15 of 90 crates; 203-186 CPU-s vs 74-103 debug); Opus: cargo ignores `panic` for test targets; Muse Wonder Full: dependency opt-level cuts CPU 66%.
- Related fact: `VELNOR_COMMIT` embedded via `option_env!` (`io/mod.rs:78`). Opus says the group SHA becomes main's SHA; Fable stamps after link; Astra needs an exact main build.
- Settle: check merge-queue squash SHA semantics; size cost of `verified` profile (Muse Wonder Full measures in phase 1).

### Q7. Is dependency `opt-level` optimisation (-66% test CPU) valid with assertions on?

- Carbon, Argon, Neon: yes, "zero loss of assertion coverage" (Carbon cites `142.5 -> 47.9 CPU-s`, `design-feedback.md:115`).
- Opus, Fable: measured with dependency assertions off; must be measured with them on. Astra: that trial also "paid 764 CPU-s rebuilding dependencies". Muse Wonder Full: likewise unmeasured with assertions on.
- Settle: four-way matrix `opt-level` x `debug-assertions` on a worker (Neon work item 2, Muse Wonder Full phase 1).

### Q8. Can an Actions workflow's check (dispatched run) satisfy the PR's required check?

- No: Astra, Sol ("`workflow_dispatch` job's check cannot satisfy a PR's required check"), Neon.
- Open: Fable lists "a dispatched run's check satisfies the PR's required check" as a lab proof; Muse Wonder Full and Carbon rely on dispatch as fallback and say the dispatched `Required` job reports.
- Settle: lab PR with a dispatched run on the head ref.

### Q9. Do two open PRs at the same head SHA share a green?

- Raised only by Astra and Sol (commit statuses are per SHA/context). Astra: PR-scoped witness plus block while a peer lacks a request. Sol: managed topic refs, ref gateway, distinct SHAs.
- Others do not address it.
- Settle: lab, two PRs from one SHA, label one.

### Q10. HEADGREEN versus ALLGREEN, and queue depth

- ALLGREEN now: Opus (5), Fable (8), Astra (2 then 4/8), Sol (2), Carbon (6).
- HEADGREEN first: Muse Wonder, Muse Wonder Full, Neon, Argon (then ALLGREEN once speculation holds).
- Evidence: #1276 landed beneath #1037 (Sol); 1 of 15 days (Fable); 1 of 31 failed group commits (Muse Wonder Full); at 8.1% false failure depth 4 disturbs 29%, depth 8 49% (Muse Wonder, Muse Wonder Full); 2% gives 14.9% at depth 8 (Astra).
- Settle: owner decision; replay with all failures counted.

### Q11. How often will the merge-group tree equal the PR-tested tree?

- 78-89% at a 1-5 min window: Opus, Fable, Muse Wonder, Muse Wonder Full, Neon, Carbon (89%), Argon; Astra cites 78-83% as hypothesis.
- Sol: only 7/45 equal today; v2 replays used integration trees, so they "do not transfer to exact heads". Opus also reports 7/45 as today's figure.
- Evidence: v2 §6.6 replays (98 and 142 merges); measured 7 of 45 and 0-18%.
- Settle: 50-merge shadow of queue-tip prediction (kill below 70%).

### Q12. How many pre-merge executions of a changed test preserve today's confidence?

- Two: Opus (twin on two nodes), Fable (matches today's PR run + group run; third is cold audit).
- Three parallel: Muse Wonder, Muse Wonder Full, Neon, Carbon, Argon (1-(1-p)^3 = 49% at p=0.2).
- Distinct required samples only where independence is shown: Astra, Sol (3 trials give 0.488 vs 0.2).
- Evidence: today's two pre-merge runs plus a push run; v2's three parallel executions.
- Settle: owner decision (v2 Q12) and a paired-rerun flake sampler.

### Q13. Should Velnor replace GitHub's merge queue (merge train)?

- Yes as stretch or optional: Muse Wonder (~40 s floor), Muse Wonder Full, Neon (~4 s), Carbon (1.3 s), Argon (10-16.5 s); each marks it an owner decision.
- No: Fable (closed by decision 7), Opus ("irreducible while GitHub lands merges"), Astra, Sol (remove the train).
- Settle: decision 7 text; owner call.

### Q14. How long does GitHub take to offer an Actions job, and is the offer on the verdict path?

- Stated 8-15 s: Carbon (8 s), Argon (8-15 s), Neon, Muse Wonder, Muse Wonder Full (~15 s); Fable 9 s (6 + 2 + 1); Opus 1.5 s webhook only, since verdict is an App check.
- Effect: Carbon, Neon, Argon say the offer bounds verdict latency unless `Required` is App-written; Opus, Astra, Sol avoid the offer entirely.
- Settle: measure push-to-first-job and dispatch-to-run (Fable work item 5).

### Q15. What is the latest official runner version, and what is the update deadline?

- Astra, Sol: latest is v2.338.2 (2026-10-26, tag `d1eb12d4`), different from the supplied `7afce9e` source (v2.338.0); Sol notes it adds broker-session ownership on deletion and a WebSocket probe. Others use 2.338.0.
- Deadline: 63 days (Opus, Fable, Neon, Muse Wonder and Full); Astra and Sol add an immediate exception for critical updates.
- Settle: `GET /repos/actions/runner/releases/latest`; diff the two tags.

### Q16. Reflink clones versus overlayfs, and the cross-node transport

- Overlay or shared inodes: Neon, Carbon, Argon (hard-linked `deps/`); Astra, Sol (shared immutable inodes); Muse Wonder, Muse Wonder Full, Opus (experiment or remedy).
- Reflink plus file-delta sync: Fable, Muse Wonder Full; mbx-cache gated: Opus, Muse Wonder, Neon, Argon, Sol, Astra; zstd layers: Carbon.
- Evidence: E1d, 14.29 GB per fully read clone; M3 39.9 s vs 92 s; L1 43.4 s vs 67.4 s; Carbon claims full seed 13 s vs 39.9 s (estimate).
- Settle: phase-1 head-to-head (Fable, Muse Wonder Full).

### Q17. Incremental compilation on or off

- On: Opus (phase 2), Neon and Argon (phase 4). Off: Fable, Carbon. Until measured: Muse Wonder, Muse Wonder Full. Private/audited: Astra, Sol.
- Evidence: `blaze.md` B3 (1.5-2.8 s vs 9.2-12.2 s); v2 §6.3 (10.9 s vs 61 s).
- Settle: worker measurement plus non-incremental audit equality.

### Q18. Is interim use of the official runner under ARC before cutover compatible with decision 1?

- Allowed until a gate: Fable (slice 1 in week 4-5, "incumbent, not fallback"), Muse Wonder, Muse Wonder Full, Neon (phases 0-1), Argon (until phase 2).
- Not allowed at all: Astra, Sol. Not from phase 1: Carbon ("delete ARC phase 1"). Opus keeps verdicts off the executor, so the merge path never uses it.
- Settle: owner reading of decision 1.

### Q19. Docker-needing tests: privileged class, rootless, or microVM

- Tainted nodes with privileged dind: Fable, Muse Wonder, Neon, Argon. Rootless experiment with that fallback: Opus. Rootless or VM: Astra, Sol. Firecracker: Carbon. microVM if >5%: Muse Wonder Full.
- Settle: phase-2 experiment with the three Docker test binaries.

## Unique ideas

Ideas raised by one or two models.

| Idea | Who |
|---|---|
| eBPF syscall read audit (incl. ENOENT) as the key-completeness check | Opus |
| `cpu.idle` batch class with `cgroup.freeze` under memory pressure | Opus |
| Release smoke on shipped musl bytes with qemu aarch64 | Opus; Carbon and Neon smoke too |
| Binary string scan that banned legacy endpoint paths are absent | Opus (Muse Wonder and Neon do a CI grep) |
| Send the newest certified version as `runnerVersion` to the broker | Opus |
| Caller-skipped reusable workflow plus App second check as two independent locks | Fable |
| Stamp `VELNOR_COMMIT` after link into a fixed-size section | Fable |
| Hedged test shards (race a restart past P95) | Fable |
| Time-shifted audit lane at +8 and +30 days via clock shim | Fable |
| Merge-group build output harvested as the next generation (no separate builder) | Fable |
| Mutation-testing of the conformance harness (>=95% of 30 mutants) | Fable |
| Two-slice executor cutover (witness job first) | Fable |
| Canary PRs every 10 min that must never be mergeable | Fable |
| Protocol surface diff of about 40 source files between runner tags | Fable |
| App commit status instead of check run, so no same-named check on the PR test-merge ref | Astra, Sol |
| PR-scoped admission witness through the required-reviewer rule | Astra |
| Label controller revokes status and cancels auto-merge before removing the label | Astra |
| Managed topic refs and ref-writer gateway; exclusive head-SHA claims | Sol |
| Pure sealed work starts on label/group admission before the job offer | Sol (Fable, Muse Wonder and others start `ensure` on webhook) |
| Candidate cannot weaken its own tests: baseline reductions need trusted plan approval | Sol |
| Third failure-domain ledger witness; fenced leader across sites | Sol, Astra |
| Not-requested job fails with a distinct gate reason excluded from false-failure accounting | Muse Wonder |
| Reserved merge-path slot per site (queueing arithmetic rho/(1-rho)) | Muse Wonder, Muse Wonder Full, Neon, Argon |
| Per-class verdict targets so the median cannot borrow other classes' numbers | Muse Wonder |
| Custom `verified` Cargo profile shared by tests and shipped bytes | Muse Wonder Full |
| Always-failing guard job as belt and braces beside a label `if:` | Muse Wonder Full |
| Race a local rebuild against a cross-node fetch, chosen by prediction | Muse Wonder Full |
| 10 s watchdog with App relabel (DELETE then POST) when a label run is suppressed | Neon, Carbon, Argon |
| Overlayfs sandboxes (6 per worker sharing page cache), lone job 48-64 threads | Neon |
| Hosting agents in cluster workspaces on the same stores | Neon, Argon |
| Merge 356 integration test binaries to about 30; `gix` rewrite of the 45,000-process test | Carbon |
| Multi-lower overlay layers with 8-layer compaction and zstd streaming | Carbon |
| Firecracker microVM for Docker suites | Carbon |
| Pre-push git hook streaming a thin pack before `git push` ends | Carbon |
| Hard-linked immutable `deps/` base to share page cache | Argon |
| Out-of-pod Ed25519 signer daemon | Argon |
| Read `registration_deprecates_at` / `runtime_deprecates_at` for drift | Argon |

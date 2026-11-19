# Rust-only redesign by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor, Rust only: redesign by Claude Fable 5.2

Judged independently of design v2 (`v2/velnor-runner-architecture.md`), which I did not write. References such as "v2 §4.2" are to it; `R/` is `~/.cache/ci-ambition/refs/actions-runner/src/` at `7afce9e` (2.338.0); `L/` is velnor-legacy's `crates/velnor-runner/src/`.

Tags on every number:
- **[M]** measured by me, read-only. This round: `scripts/wfruns.py`, `wfmean.py`, `wf_struct.rb`, `profile_exp.sh` with raw output in `raw/ro/`; earlier rounds: `notes.md` (M1–M17) and `design-feedback.md` (F1–F7).
- **[P]** taken from the evidence pack, the cluster and blaze experiments or v2, as cited.
- **[E]** my estimate, with the arithmetic.

New measurements of this round [M]:

| Id | What | Result |
|---|---|---|
| R1 | Job-seconds per run of the non-CI workflows (50, 50, 30, 30 most recent completed runs, 2026-11-18) | Source qualification 1,007 s mean (1,993 s when it succeeds), on every PR event; Job image candidate 200 s, on every push to every branch; State seal 683 s per `main` push; source-preview jobs of `product-release-generator.yml` 1,693 s per `main` push, 1,267 s of it on macOS |
| R2 | Release workflows, job-seconds per run | `release-rust-ci-tools.yml` 190–434 s (three cold `--release` builds); `product-release.yml` 4,105–6,238 s over 27 jobs |
| R3 | Profile experiment, `generator` and `ci-helper` at `d6be276a6c`, cargo 1.99.1, `-j6`, `CARGO_INCREMENTAL=0`, Apple M5 under load 90–126, so CPU-seconds only | Debug test build 74.4 and 103.2 CPU-s. Release binary (`opt-level = "s"`, thin LTO, `panic = "abort"`, strip) 117.5 and 179.6 CPU-s. Release-profile test build in the same target directory afterwards: 203.3 and 186.1 CPU-s, because 75 of 90 and 97 of 116 crates compile again (tests need `panic = "unwind"`). Tests: 11.9 CPU-s debug, 6.0 release, the same 613 passes and 27 failures |
| R4 | Live repository state | Ruleset 24396608 unchanged (two required checks with no `integration_id`, HEADGREEN, build 2). Last 100 PRs: 93 by the owner's user account, 7 by `renovate[bot]`, 0 from forks, 0 with auto-merge. No `velnor:verify` label exists |
| R5 | Official runner 2.338.0 | The evaluator it obeys is the one its own source calls "Legacy" (`R/Runner.Worker/ExecutionContext.cs:1546-1562`); the new one runs only in compare mode and its result is discarded (`R/Runner.Worker/PipelineTemplateEvaluatorWrapper.cs:224-318`). Background steps exist (`R/Runner.Worker/BackgroundStepCoordinator.cs:29,149-175`) and `ci.yml` uses 105 of them |

## 1. Summary

- **One gate, started only on demand.** A single Actions job, `Velnor / Required`, executed by Velnor's native Rust executor, exists only when the PR carries `velnor:verify` (`labeled`, or `synchronize` while labelled) and on every `merge_group`. An unlabelled head has no check at all, so the ruleset blocks it. A second required check, posted by the Velnor App from the ledger, makes the gate immune to workflow edits and to GitHub's rule that a skipped job counts as success.
- **Every unit of work is an input-addressed action.** Fetch, compile, test, package, attest: each runs once in a sealed sandbox whose contents are its key, leaves outputs in a content-addressed store and a signed receipt in a ledger. PR, merge group, `main`, release, audits and an agent's pre-push `velnor verify` all call the same idempotent `ensure(key)`.
- **Compile:** plain cargo over a reflinked snapshot of the trusted `main` generation at a fixed path (0.56 s, 0 crates for an unchanged tree [P]). The next generation is the landed merge group's own build output, harvested, never rebuilt. blaze is rejected on evidence; `mbx-cache` survives only if it beats a file-delta sync head to head.
- **Release promotes, never rebuilds.** Release-profile binaries are built once per closure key at merge time, qualified once on the exact bytes, packed and attested once; a tag promotes them by digest in about 15 s instead of 190–6,238 job-seconds [M R2]. Tests stay in the debug profile: testing the release profile costs 29–67% more CPU and shares 15 of 90 crates with the shipped binary [M R3].
- **Same confidence, stated as samples:** tests whose closure the change touched run twice in parallel before merge and once cold after landing (today: PR run, merge group, push run). Every landed commit gets a cold audit of its changed closures; a full cold audit runs nightly and on every 10th merge. The queue moves to ALLGREEN.
- **Committed** (phase 2, week 7): PR verdict P50 ≤ 45 s, P90 ≤ 3 min; push-to-merge P50 ≤ 2 min, P90 ≤ 6 min; ≤ 30 runner-minutes per merged PR against about 460–490 today [E from M].
- **Stretch** (phase 3): verdict 25 s / 90 s, and 12 s when the agent ran `velnor verify` first; push-to-merge 75 s / 3 min; ≤ 12 runner-minutes.
- **Floor:** 66 s push-to-merge, of which 55 s is GitHub's merge queue [M+E, section 9].

## 2. The latency budget

The typical PR touches only `generator/` or `ci-helper/` (47% of 200 merged PRs [M]). It carries the label and has auto-merge on; the clock starts at `git push`.

| # | Stage | Today | Design | Bottleneck, and how it goes |
|---|---|---|---|---|
| 1 | GitHub event, webhook, job offer | Push → run 6 s; run → first job 2 s median, p90 445 s behind a cancelled run [M F1–F3]. `Plan` then waits for a hosted runner: 5.5 min median, 23.1 min p90 [P §A T4] | The webhook reaches Velnor in 1–2 s [E, work item 5 measures it] and verification starts. The job offer follows about 9 s after the event (6 + 2 + 1 [M+E]) and joins the running work | The Free plan's 20 hosted slots and the `always()` job of a cancelled run. No hosted job is left; one job; job-level concurrency |
| 2 | Acquire | Job created → started 13 s p50, 25 s p90; 2–3 s on an idle runner [M F4] | 0.25 s: `acquirejob`; the first `renewjob` overlaps the first step's set-up [E: one call at 120 ms RTT on the path, P §B.5] | ARC makes a pod and a JIT runner per job. The dispatcher holds registered JIT sessions already |
| 3 | Sandbox start | Pod scheduled → runner container 5.6 s median, 9.8 s p90, plus a dind sidecar [M M13] | Cell 5–20 ms [E]; view 0.3–0.5 s (reflink clone 0.25 s [P], then the delta) | A pod per job. Cells are created before the work arrives |
| 4 | Checkout | 4.2 s per job in 31 jobs; 11.2 s in `Plan` [M] | 0.1 s inside the view: the closure is 739 of 6,913 files [M, E] | Network fetch and lazy blobs. The node mirror fetched the commit on the webhook |
| 5 | Toolchain and tools | Tools 9.8 s, mise 2.4 s, "Set up job" 4.2 s, per job [M]; 10 s median, 18 s p90 [P §A T9b] | 0 s: a read-only verified image, mounted when the cell is made | Installation into `RUNNER_TEMP` in every job |
| 6 | Dependency fetch | 4.6 s per job [M]; the sources cache missed 48 of 48 times [P] | 0 s; ≤ 1 s once per new lockfile, 10.5% of PRs [M; E from the 2.7–3.3 s full fetch, P §B.4.2] | Fetch per job. One fetch action per lockfile digest |
| 7 | Compile | Every job cold: 14 s `velnor-ci`, 81 s `velnor-runner-host` [P §A T10]; 4.4× duplicate crates per run; the critical-path job builds the CLI four times at `-j2`, 381 s [M] | 10 s for this class [E: 14 s is the whole cold test build of `ci-helper`, P]; 67 s for a runner PR today [P], 15–25 s after the crate split [E] | No compile result crosses a job. Warm generation, one build per root |
| 8 | Tests | The path is `maintenance-helpers` 258 s, `actions-cli` 238 s, `runner-host` 192–246 s, though the PR touched none of them: heavy jobs ran in 97.4% of PR runs [M, P] | Changed closures only: `velnor-ci` and `velnor-gen`, about 10 s [P §A T10], twice in parallel | Selection that selects everything. Closure keys |
| 9 | Result reporting | Job end 5 s; report upload 2.9 s per job; `Required verdict` 49 s, 32.7 s of it artifact downloads [M] | 1.5 s: step logs and `completejob` [E: 4 calls] | Artifacts as the transport between jobs. The ledger is the transport |
| 10 | The required check | Two more hosted jobs: queue 1.6 and 1.9 min median, 24 and 21 min p90 [P §A T4] | 0 s: the job is the check; the App's check adds 0.2 s [E] | Serial gate jobs |
| 11 | Merge-queue entry and run | Green → enqueue 7.5 min median, nobody uses auto-merge [P]; enqueue → run 19 s [M]; the run 19.9 min P50, 84 min P90 [P §A T2]; 2 entries build at a time | Auto-merge enqueues in about 5 s [E]; 19 s [M]; job 2 s [M]; lookup and completion 3 s [E] | A full second run. Same tree, same keys: a lookup |
| 12 | The merge | 26 s [M] | 26 s [M] | GitHub's own; irreducible |
| | **Total** | PR verdict 26.7 / 92.3 min; push-to-merge 85 / 457 min [P] | Verdict at 25 s (2 + 0.5 + 0.5 + 10 + 10 + 1.5); merged at 80 s (25 + 5 + 19 + 5 + 26) [E] | |

- **Other classes** [E]. A runner PR (25%) takes 67 s of compile and, once no test exceeds 60 s, 60 s of tests: 2.4 min. An engine PR (14%) takes about 2.5 min once the 89 s fixture build is part of the warm build. So P50 sits in the small class (generator, helper, local, no Rust: 58%) and P90 in the runner and engine classes.
- **Push-to-merge P90** [E]. A merge group whose tree differs from the PR-verified tree re-runs the closures that moved: 11–22% of merges when the window is 2.5–5 min [P v2 §6.6]. A slow class with a moved base (0.39 × 0.2 = 8% of merges) costs 3 + 3 + 0.9 = 6.9 min; the 90th percentile is one of the two, 4–5 min. I commit 6.
- **Release artifacts are not on this path.** Their lane starts with the PR's verification at batch priority and takes 30–40 s [E, arithmetic in 6.6]; the merge group needs it at about 51 s (25 + 5 + 19 + 2).
- **The verdict cannot beat 11 s** while it is an Actions job: 6 + 2 + 1 s until Velnor holds the job, 2 s to complete it [M+E].

## 2b. Duplicate-work audit

Read from the 16 workflow files at `d6be276a6c`, the generator's goldens (`generator/samples/*/expected/`), `ci-helper/` and the job logs of earlier rounds. "MG" is the merge group. Costs are per occurrence unless stated.

| # | Operation | Stages that run it | Same input? | Today's cost | Decision | Mechanism |
|---|---|---|---|---|---|---|
| 1 | A run for a push nobody asked to verify | PR: `opened`, `synchronize`, `ready_for_review` | — | 4.77 PR runs per merged PR, 1.28 of them cancelled; 45% of all PR runs cancelled [M, P §A T7] | Eliminate | Label trigger (section 8) |
| 2 | Re-run on an unchanged head (`edited`, manual re-run) | PR | Yes | 18.3% of PR runs, 469 of 2,558 [P v2 §1.1] | Zero cost | No `edited` trigger; receipts by tree |
| 3 | Rebase-only push, same tree | PR push N, N+1 | Yes | A full run, 52.9 runner-min [P] | Reuse: verdict in 11 s | New SHA, same tree, same keys |
| 4 | Push N+1 that changes one package | PR push N, N+1 | Partly | The heavy jobs again, 97.4% of runs [M M9] | Re-run changed closures only | Per-package closure keys |
| 5 | Checkout | Every job of every workflow (61 uses) | Yes | 4.2 s × 39 jobs = 2.7 min per run [M] | Once per request and root | Sparse view from the node mirror |
| 6 | Toolchain and tool install, with its verification steps | Every job | Yes | 15 s × 31 jobs = 8 min per run [M, P §A T9b] | Eliminate | Verified read-only image keyed by the pins |
| 7 | Action download ("Set up job") | Every job | Yes | 148 s per run [M M17] | Eliminate | Native actions |
| 8 | `cargo fetch` | Every Rust job, `Plan` (5 roots), seal, qualification | Yes | 4.6 s × 31 = 2.4 min per run [M] | Once per lockfile digest | Fetch action; vendor tree in the store |
| 9 | Plan | `CI`, and again in `Source qualification` (PR) or `State seal` (`main`) | Yes | 45–77 s each [M R1] | Once | Released planner in the ledger, keyed by tree: 0.5 s [E] |
| 10 | Ledger CLI: build, upload, 31 downloads | Every run | Yes | 22 s + 31 × 3.2 s = 2 min per run [M] | Eliminate | The released helper is in the tool image |
| 11 | Third-party crates | Every Rust job | Yes | 2,106 crate builds for 475 distinct, 4.4×; cargo is 2,012 of 5,032 job-seconds per run [M M5]; 6.3–21.7 GB written per job and discarded [M M13] | Once per generation | Warm snapshot (section 6) |
| 12 | `velnor-actions-cli` | `Ledger CLI`, `maintenance-helpers` (4 builds), its own job, 3 Source qualification jobs, Job image candidate, state producers | Yes | At least 9 builds per PR event; 381 s in `maintenance-helpers` alone [M] | Once per closure key | One root build; consumers take the binary by digest |
| 13 | `velnor-runner-host` | Root and runner workspaces; default and `local-mode`; lib and lib-test | No (lockfile, feature, cfg) | 34.6 + 82.5 + 99.0 s on an M5 [M M7, M17] | Remove two of four by code: extract `velnor-runner-local`; unit tests into a test target | Section 6 |
| 14 | The same tree compiled per event | PR, MG, push | MG = push in 120 of 159; PR = MG in 0–18% today [P] | Three cold builds, each ≥ 195 GB of discarded output (31 jobs × 6.3 GB) | Once | Compile action keyed by (generation, tree) |
| 15 | State producers and seals (cache warmers) | `main` push | Yes: the commit CI just built | 683 s per push; each producer builds twice; bundles of up to 1.5 GiB, 3.49 GB live in GitHub's cache [M R1, P §C.3, §A T9c] | Eliminate | The landed MG's build output is the next generation |
| 16 | Clippy, doc and "check alone" lanes | Every request | No (different outputs) | 36–75 s and 30–65 s per job [P §A T10] | Keep, in parallel | Lane targets inside the generation |
| 17 | Tests of packages the change did not touch | PR, MG, push | Yes | Most of 1,838 nextest-seconds per run [M M5] | Eliminate | Receipt with the same closure key |
| 18 | Tests of touched closures, PR then MG | PR, MG | Yes when the tree is equal: 78–89% at a 1–5 min window [P v2 §6.6] | A second full run, 64.6 runner-min [P] | Reuse | Receipts; the rest re-run what moved |
| 19 | Everything again on `main` | Push | Yes, same SHA as the MG | 62 runner-min; found nothing; failed falsely 8.1% [P] | Off the path | The cold audit A1 takes its place (section 6) |
| 20 | Second execution of touched tests | PR | Yes | New: +10 s of CPU for the typical PR, +560 CPU-s for `runner-host` after the test-floor work [E] | **Keep as a duplicate**, not for speed | Decision 11: it holds today's two pre-merge samples; 0 s of wall time |
| 21 | Unchanged parts across concurrent, stacked or identical PRs | PR × PR | Yes | Every PR pays everything | Reuse | Same keys hit `main`'s or the base PR's receipts; in-flight join |
| 22 | The agent's own test run, then CI | Pre-push, PR | Yes (tree) | Not measured; agents leave 11 s median, over 60 s in 27% of cases, between commit and push [M F6] | Reuse | `velnor verify` runs in the same cells |
| 23 | alint, jactionlint, cargo-machete, cargo-deny × 5 | PR, MG, push | Yes | 8 hosted jobs, 3.8 min per run plus the queue [M M12] | Once per key | Keyed obligations; advisory database pinned by commit |
| 24 | `Required verdict` and `Required` | Every run | — | 49 s + 5 s and two hosted waits [M] | Eliminate | The witness job is the verdict |
| 25 | Source preview, Linux build and qualify | `main` push; built again by the product release | Yes | 410–534 s per push [M R1] | Once: this *is* the release artifact | Release-artifact action, promoted |
| 26 | Source preview, Darwin | `main` push | — | 1,063–1,406 s per push on macOS [M R1] | Paused (decision 9) | Legs disabled in phase 0 |
| 27 | Job image candidate and its attestation | Every push to every branch, queue branches included | Yes unless the image inputs changed | 200 s; about 6 per merged PR = 20 min [M R1, E] | Once per image-input key | Keyed action; attest only what `main` holds |
| 28 | Cache prune | After `CI` and `State seal` on `main` | — | 2 × 25 s per push [M] | Eliminate | No GitHub cache entries remain |
| 29 | Source qualification | Every PR event | Yes: its 3 jobs each build the CLI, and CI built it too | 1,007 s per event; 80 runner-min per merged PR, hosted [M R1] | Once per key, on demand | Obligations in the plan, fed by row 12 |
| 30 | `velnor-ci` and `velnor-gen` for 3 targets | `rust-ci-v*` tag | Yes: the commit is on `main` | 190–434 s; the bytes are only asked for `--version` [M R2] | Promote | Built, qualified, packed and attested at merge time |
| 31 | SDK, runner images, `velnor-host` | Product release | Linux SDK: yes | 4,105–6,238 s, 27 jobs [M R2] | Promote Linux; macOS paused | Same |
| 32 | release-plz set-up (download, `rustup`, full-history checkout, `release check-config`) | `release-pr` and `tag` jobs of the generated `release.yml` | Yes | Twice per release run; velnor-new does not run it yet [P golden] | Once | One job; tools from the image; history from the mirror |
| 33 | `<bin>-<target>.tar.gz`, `SHA256SUMS`, attestations | Release | Yes | Rebuilt per release | Once | Deterministic pack keyed by the binary's digest |
| 34 | The embedded commit id (`ci-helper/crates/velnor-ci/src/io/mod.rs:78`) | Release build | It makes every commit a new input | A rebuild per commit | Remove the cause | Stamp after link (section 6) |
| 35 | Nightly full run (the generator emits cron `15 8 * * *`) | Schedule | Partly | A full run | Keep: it is audit A2 and the lineage restart in one build | Section 6 |
| 36 | Freshness, tag-ruleset drift | Schedule | No: they read the network | 87–134 s weekly; seconds daily [M] | Keep, always fresh | Uncomparable class |
| 37 | Tools and sources producers | `main` push | Yes | 28 of 72 tool-producer jobs failed [P §A T9d] | Eliminate | Rows 6 and 8 |
| 38 | Conformance pairs | Lab | The second run is the point | 2,377–2,856 s per dispatch [M] | Keep, in velnor-lab | Keyed by (executor, oracle, case) |

**Duplicates I keep, with their numbers.**
- Row 20. It is the one duplicate kept for confidence instead of speed. Without it a regression that fails with probability 0.2 is caught before merge with 20% instead of today's 36% (1 − 0.8²). The owner can set it to one execution and accept that.
- The cold audits. They are decision 11's detection, off the path, on batch CPU: about 6.7 runner-minutes per merged PR [E below].
- Speculation on the queue tip. 122 of 193 merge groups were built on an unlanded group commit [P]; starting `ensure` for that tree when the PR turns green gains the 24 s GitHub needs to create the merge-group job (19 + 2 + 3). A right guess costs nothing extra; a wrong one wastes only the closures that differ, at batch priority.
- Two copies of data per site (Git mirror, generation replicas): a local copy that beats a fetch, not repeated work.

**Totals** [E from M]. A runner-minute in the design is one occupied cell for one minute.

| | Today | Design, phase 2 | Design, phase 3 |
|---|---|---|---|
| Per merged PR: runner-minutes | 460–490: CI 331–347 (3.49 × 52.9 + 1.28 × 15 + 1.25 × 64.6 + 0.75–1 × 62), Source qualification 80, Job image 20, source preview 21–28, State seal 9–11, prune 1. 155–170 of them hosted | 22: verification 13.4 (1.5 requests × 8.9 by class mix), MG 1.8, audits 6.7, speculation and push 0.6 | 12, once no test exceeds 60 s and the big crate is split |
| Per merged PR: cache | ≥ 1.4 TB of compile output written and discarded (7 runs × 31 jobs × 6.3–21.7 GB [M M13]); 3.49 GB in GitHub's cache, restored by 0 of 3,636 cluster jobs [P] | 9 GiB written (3.1 GiB delta by class mix × 1.7 builds + 4 GiB cold audit); 3 GiB kept per generation; about 75 GiB per warm node at rest | The same |
| Per release: runner-minutes | 3.2–7.2 (rust-ci tools) or 68–104 (product) [M R2] | 0.25: one job that looks up, signs and publishes | The same |
| Per release: cache | 1–3 GB of release targets built and discarded [E: 3 targets × 0.4–0.9 GB, R3] | 0 compiled; about 30 MB of assets, the bytes `main` qualified | The same |

## 3. The native Rust runner

### 3.1 Crates and components

| Crate | Runs as | Does | Budget | Source |
|---|---|---|---|---|
| `velnor-wire` | library | Wire types and clients: scale-set admin and sessions, broker, run service, Results, OIDC, cache v2, artifacts v4, attestations | 7k lines | velnor-new's `velnor-runner-github` (6.9k lines of wire types, no HTTP client [P §D.3]); the broker, run-service and Results parts of `L/protocol.rs` |
| `velnor-dispatch` | one active, one standby per site | Scale-set sessions; a pool of registered JIT runner sessions; matches offers to requests; acquires; renews every lease; journal | 5k | Fresh |
| `velnor-exec` | library in the node agent | Job model, templates and expressions, step runner, background coordinator, composites, commands, masker, post steps, cancellation, Results streaming | 14k | Fresh around copied leaves (3.9) |
| `velnor-actions` | in the executor | The closed action set (section 4) | 8k | `L/checkout.rs` and `L/git_mirror.rs` adapted; clients fresh |
| `velnor-node` | privileged DaemonSet | Cells, views, stores, harvest, observation of exits and test inventories, cleanup by cgroup | 10k | Fresh |
| `velnor-ledger` | library and one service per site | Planner call, keys, receipts, `ensure`, in-flight join, promotion, audit scheduling | 6k | Fresh |
| `velnor-store` | library and one service per site | Content-addressed blobs, generation replication, eviction | 4k | Fresh |
| `velnor-hooks` | one per site | App webhooks in; the App's check run, dispatch fallback and cancel out | 2.5k | Fresh |
| `velnor-conformance` | velnor-lab | Section 5 | 6k | `qualification.yml`, velnor-local's mitmproxy recorder [P §D.4.6] |

About 62k lines; crossing a budget stops work for review, as in v2.

### 3.2 The protocol client, latest APIs only

1. **Scale-set admin** with the App's installation token: sessions, long-poll with `lastMessageId` and `X-ScaleSetMaxCapacity`, `acquirejobs`, `generatejitconfig` [P §F.1.3].
2. **Broker**, per JIT identity: `POST {broker}/session` (`R/Sdk/WebApi/WebApi/BrokerHttpClient.cs:143`), `GET {broker}/message` with `sessionId`, `status`, `runnerVersion`, `os`, `architecture`, `disableUpdate` (`:70-101`), `POST {broker}/acknowledge` (`:225-239`).
3. **Run service:** `acquirejob` (`R/Sdk/RSWebApi/RunServiceHttpClient.cs:70-85`; 404, 409 and 422 are final, `:104-111`), `renewjob` (`:184-197`), `completejob` (`:123-152`; a 404 is terminal, `:169-170`).
4. **Results:** the twirp receiver for step logs, job logs and step summaries and `WorkflowStepsUpdate` for the timeline (`R/Sdk/WebApi/WebApi/ResultsHttpClient.cs:610-632` [P]); live lines over the feed websocket.
5. **Service endpoints** come from the job message's `SystemVssConnection` (`ACTIONS_RESULTS_URL`, `ACTIONS_RUNTIME_TOKEN`, `ACTIONS_ID_TOKEN_REQUEST_URL`, `ACTIONS_CACHE_SERVICE_V2` [P §F.4.1]) and feed the native cache, artifact and OIDC clients.

- **Handled messages:** `RunnerJobRequest`, `JobCancel`, `ForceTokenRefresh`, `RunnerRefreshConfig` and the broker migration (`R/Runner.Listener/Runner.cs:699-838`). A self-update message raises an alert; Velnor never downloads a runner.
- **Refused, with a named error:** the inline `PipelineAgentJobRequest` (`:679`), a reference with an empty `run_service_url` (the legacy path, `:731-738`), AES session keys, `_apis/distributedtask` timelines and logs, cache v1, artifacts v3. A check in CI searches the binary's endpoint strings for them.
- **Version:** Velnor sends the official version it last passed conformance against, so GitHub's 63-day rule [P §F.6.1] becomes the deadline for passing each release.

### 3.3 The job-message model

- **Typed where Velnor executes, recorded where it does not.** `AgentJobRequestMessage` (`R/Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs:27-47,486-508` [P]) becomes: `steps` (script, repository action or control flow; `condition`, `continueOnError`, `timeoutInMinutes`, `background`, `contextName`), `TemplateToken` trees with their `fileTable`, `contextData`, `variables` with `isSecret`, `maskHints`, `resources.endpoints`, `jobOutputs`, `defaults`, `environmentVariables`.
- **Admission before the first step.** Every `uses:` of the job and, transitively, of the local composites at `github.sha` (read from the node mirror) must be in the qualified set at a qualified SHA. A job container, service containers, a Docker or Node reference refuse the job: `completejob` with `Failed` and one annotation naming the action and the supported set. `velnor-gen` cannot emit such a job, and a plan-time obligation rejects a workflow file that contains one before it can land.
- **Gate authority.** The job that carries the required check name succeeds only if the ledger's verdict for the tree of `github.sha` is a pass over the inventory the released planner computes. Its own steps cannot make it green (v2 C4, enforced in the executor).
- **Drift sensor.** Every unknown field, enum value, endpoint data key or variable name in a production message is counted and alerted within minutes.

### 3.4 The expression engine

- **Fact (R5).** At 2.338.0 the official runner evaluates with `GitHub.DistributedTask.Expressions2` and its own source labels that "Legacy". The new `GitHub.Actions.Expressions` with `WorkflowParser` (6,947 and 16,071 C# lines) runs only when the server sets `actions_runner_compare_workflow_parser` (`R/Runner.Common/Constants.cs:175`), and then the wrapper returns the legacy result (`PipelineTemplateEvaluatorWrapper.cs:318`). velnor-legacy's engine is a transcription of the legacy one [P §E.1.1].
- **Production engine:** a fresh port of the new evaluator, about 3.5k Rust lines, with the step functions the runner adds (`always`, `cancelled`, `failure`, `success`, `hashFiles`: `R/Runner.Worker/StepsRunner.cs:93-97`). That is where GitHub is going, and decision 3 excludes the legacy one.
- **Twin, test-only:** velnor-legacy's `L/expression/` (3,565 lines, 989 of them tests) is copied into the harness, never into the product.
- **Proof that the choice does not matter here:** the official L0 tests (`R/Test/L0/Sdk/ExpressionParserL0.cs`, `R/Test/L0/Worker/Expressions/ConditionFunctionsL0.cs`); a differential fuzzer in the lab, where a small C# host links the oracle release's `Sdk.dll` and Rust must equal both official evaluators on generated expressions and contexts; and a `velnor-gen` test that every expression it can emit lies in the subset where the two official evaluators agree. An expression on which they disagree is reported and banned from generated workflows.
- **`hashFiles`** is native. The official runner spawns its bundled Node for it (`R/Runner.Worker/Expressions/HashFilesFunction.cs:73,206` [P]); the port must give the same digest for the same tree (match order, symlinks, dotfiles, negative patterns, no match), checked on the corpus.
- **.NET formatting** of numbers, `toJSON`, `fromJSON` and `format` comes from `L/runner_json.rs` (1,248 lines, copied).
- **Templates:** only what the runner evaluates at run time (step env, inputs, `if`, display name, `continue-on-error`, `timeout-minutes`, job outputs, `defaults.run`), plus a YAML parser for composite `action.yml` with `runs.using: composite` as the only accepted value.

### 3.5 Step, composite and post-step semantics

The state machine follows `R/Runner.Worker/StepsRunner.cs` line by line; each bullet is a conformance case family.

- **Order.** Steps run from a queue. When it empties, unwaited background steps are awaited, then post steps are popped last-in-first-out (`:56-74`); a post step is registered when its action's main stage ran, and shares that action's saved state (`R/Runner.Worker/ExecutionContext.cs:335-369`).
- **Per step.** The `env` context is the global environment overlaid with the step's evaluated `env` (`:108-128`). The `if` is evaluated against the job status; false skips, an evaluation error fails the step (`:200-241`). A failed step updates the job status that later conditions read (`:273-279`).
- **Outcome and conclusion.** The command result merges into the step result, then `continue-on-error` turns a failure into success while `outcome` keeps it (`:350-353`).
- **Timeouts and cancel.** `timeout-minutes` is evaluated per step, and expiry fails the step with the official message (`:301,:328`). A job cancel re-evaluates the running step's condition and cancels it unless the condition still holds, so `always()` steps survive (`:154-184`); a cancelled step is `Canceled` (`:336`).
- **Background steps.** `background: true` starts a step under a coordinator with at most 10 in flight; `wait`, `wait-all` and `cancel` are control steps; an implicit wait-all precedes the post steps (`R/Runner.Worker/BackgroundStepCoordinator.cs:29,149-175`; `R/Runner.Worker/JobExtension.cs:549`). `ci.yml` has 105 of them [M R5], and the witness job's lanes are background steps.
- **Composites.** Steps are inlined in a child scope with their own `inputs` and `steps` contexts; outputs follow the manifest's `outputs.*.value`; nested composites, nested native actions and their post steps work as upstream (`R/Runner.Worker/Handlers/CompositeActionHandler.cs`, 577 lines).
- **Script steps.** `bash --noprofile --norc -e -o pipefail {0}` and `sh -e {0}` (`R/Runner.Worker/Handlers/ScriptHandlerHelpers.cs:16-19`), the script file in `runner.temp`, `working-directory`, `defaults.run`, and the "Run …" header with shell and env.
- **Stopping a process.** SIGINT, 7.5 s, SIGTERM, 2.5 s, then the tree is killed (`R/Runner.Sdk/ProcessInvoker.cs:32-33,443-464`); Velnor then kills the cell's cgroup, so nothing survives a step.
- **"Set up job" and "Complete job"** exist as records with Velnor's own text (`R/Runner.Worker/JobExtension.cs:70,774`); the oracle allow-lists their content.

### 3.6 Contexts, env, secrets and masking

- **Contexts:** `github`, `runner`, `job`, `steps` (scoped per composite), `env`, `inputs`, `matrix`, `strategy`, `needs`, `vars`, `secrets`, from `contextData` plus the values the runner sets itself (`github.action`, `github.workspace`, the file-command paths, `runner.temp`, `runner.tool_cache`).
- **Environment of a step:** a fixed, scrubbed baseline; job env; `GITHUB_ENV` additions; step env; the `GITHUB_*` and `RUNNER_*` defaults; `GITHUB_PATH` entries in order. A script step gets only the OIDC pair of service variables (`R/Runner.Worker/Handlers/ScriptHandler.cs:317-318`).
- **Commands:** `set-output`, `save-state`, `add-mask`, `add-path`, `set-env` (off unless allowed, as upstream), matchers, `debug`, `notice`, `warning`, `error`, `group`, `endgroup`, `echo`, `stop-commands` (`R/Runner.Worker/ActionCommandManager.cs:234-809`). Files: `GITHUB_ENV`, `GITHUB_OUTPUT`, `GITHUB_PATH`, `GITHUB_STATE`, `GITHUB_STEP_SUMMARY`, with heredoc delimiters and the blocked-variable rule (`R/Runner.Worker/FileCommandManager.cs:127-353`).
- **Masking.** Every secret variable and mask hint, each under the eleven encoders the official host registers (three Base64 alignments, command line, expression, JSON, URI, XML, trimmed quotes, two PowerShell forms: `R/Runner.Common/HostContext.cs:103-113`), plus `add-mask` values; multi-line secrets mask per line. One masker filters every byte that leaves the node: live feed, log blobs, annotations, summaries, receipts and Velnor's own traces.
- **Annotations:** 10 per type and step, 4,096 characters (`R/Runner.Worker/ExecutionContext.cs:144-146`).
- **Obligation cells get no token** and no event file (v2 C5, kept): the job's credentials stay in the witness cell.

### 3.7 Cancellation, timeouts and lock renewal

- **Renewal** every 60 s (`R/Runner.Listener/JobDispatcher.cs:776`); steps start only after the first success [P `:380-410`]; a failing renewal is retried until `lockedUntil` plus 5 min (`:810`). The loop is written fresh, because the upstream overload dereferences a null request (v2 N10).
- **Renewal lives in the dispatcher,** not in the cell, so a starved or dead node cannot lose a lease.
- **Cancel.** `JobCancel` from the broker sets the job's token; conditions are re-evaluated; post steps run; `completejob(Canceled)` follows. The witness job only waits on the ledger, so it completes in under 1 s [E]; its claims are dropped, and executions another request needs keep running.
- **Job and step timeouts** come from the message and are enforced by the executor's own timers, not by the cell.

### 3.8 Logs and timeline via Results

- A line passes the masker, then goes to the live feed in batches of at most 250 ms (best effort, as upstream) and to the step's append blob, which is sealed at step end and followed by `CreateStepLogsMetadata`. Every state change sends `WorkflowStepsUpdate`. Step summaries and the job log go up at the end.
- The official queues poll every 500 and 1,000 ms (`R/Runner.Common/JobServerQueue.cs:33-36`), and a job takes a median 5 s from its last step to completion [M M13]. Velnor flushes on events: 1.5 s [E].
- Obligation output streams from the agents into the witness job's steps, so GitHub shows cargo and nextest live, one step per lane.

### 3.9 Crash safety, and what is copied

- **Journal discipline** from velnor-new's runner: persist the intent, make the call, record the outcome; unknown is never absent [P §D.4.3]. The store is SQLite with one row per transition, not the Turso journal whose `Journal::open` is today's test floor.
- **Dispatcher:** the request and intent before `acquirejob`; the sealed message and token, encrypted, after. The standby resumes renewals within 30 s.
- **Executor:** intent, run and result per step. After a crash a job is completed once, `Failed` with an infrastructure annotation, with its own token; a lost `completejob` response is retried and a 404 ends it.
- **Ledger:** an execution's intent (key, node, epoch) before a cell starts; the result by compare-and-swap on the epoch, so a fenced node reports nothing (v2 §3.9, kept).
- **Node death:** obligations are scheduled again by `ensure`. A witness job on the dead node fails as infrastructure, and the App re-runs it once; receipts make the re-run a lookup.

| From | Copied, with tests, after an oracle pass | Not copied |
|---|---|---|
| velnor-legacy | `expression/` as the test twin (3,565 lines); `workflow_command.rs` (1,521), `command_files.rs` (481), `script_step.rs` (2,160), `runner_json.rs` (1,248); `checkout.rs` and `git_mirror.rs` (6,741); the broker, run-service and Results calls of `protocol.rs`; `attestation.rs` (692); its incidents (#311, #329, #389, #408, #670–#672 [P §E.6]) as regression cases | `executor.rs` and `runner.rs` (37k lines each); the `distributedtask` paths (11 uses in `protocol.rs` [M]); `gha_cache.rs`, a cache server for v1 and v2; Node sidecars; Docker lease, BuildKit, Firecracker; the scale-set lane, which starts the official runner |
| velnor-new | `velnor-runner-github` wire types; `velnor-runner-core` lifecycle and ownership types; the journal discipline; `velnor-runner-attestation` | The Docker pair executor; the Turso journal |
| Written fresh | Dispatcher, cells, ledger and `ensure`, store, the new-evaluator port, templates, background coordinator, composite handler, cache v2 and artifact v4 clients, sparse checkout, the harness | |

### 3.10 Starting a job in the fewest milliseconds

- **Registered sessions wait for jobs.** The dispatcher keeps at least 16 JIT runner identities per scale set registered and long-polling the broker. ARC registers a runner after the job exists; here the broker message lands on a waiting session.
- **Work starts before the job exists.** The webhook arrives 1.5 s after the event [E]; the mirror fetches the commit (0.75 s [M]); merge tree and plan take 0.5 s [E]; the view takes 0.5 s; the first cargo command runs about 3 s after the push.
- **Acquire is pipelined with the sandbox.** GitHub's offer arrives at about 9 s. The dispatcher matches `workflowRunId` to the request and, in the same instant, sends `acquirejobs` and assigns a ready witness cell (5–20 ms). `acquirejob` (0.25 s) is then the only call on the path, and the first `renewjob` overlaps the first step. First step ≤ 0.3 s after the broker message [E], against 2–3 s on an idle ARC runner and 13 s for all cluster jobs [M F4].
- **Nothing is downloaded or started cold:** no action archives, no runner process (the executor is a task in the agent), warm HTTP/2 connections to broker, run service and Results (a TLS handshake is two round trips, 240 ms).
- **Snapshot and restore** applies to build state, not to processes: the reflinked generation is the snapshot (0.25 s [P]). No process is expensive enough to checkpoint.

## 4. Native actions

The set is closed three ways: by action, by qualified SHA and by input. The inputs below are every input the 16 workflow files, the 17 composites and the generator's goldens pass [M: `scripts/wf_inputs.rb`]; any other input refuses the job before its first step, so nothing is ever silently ignored. Each implementation must leave the same outputs, files, cache entries, artifacts and annotations as the upstream action at the qualified SHA; the harness checks that.

| Action (uses in repo; in goldens) | Inputs supported | Native design | Local instead of the network | Step time today → design |
|---|---|---|---|---|
| `actions/checkout` (61; 75) | `ref`, `fetch-depth` 0, 1, 2 or an expression, `persist-credentials`, `sparse-checkout` with cone mode off, `submodules: false`; outputs `ref`, `commit` | The working tree is written from the node's bare mirror; `.git` points at the mirror through `objects/info/alternates`, with a `shallow` file so history depth equals upstream's; credentials are persisted and removed in a post step only when asked | Objects: the mirror fetched the commit on the webhook; a miss costs one fetch of that SHA with the job's token | 4.2 s mean, 11.2 s with full history [M] → 0.1–0.5 s [E: 739–6,913 files, no network] |
| `actions/upload-artifact`, `actions/download-artifact` (80, 72; 66, 45) | `name`, `path`, `if-no-files-found`, `retention-days`, `overwrite`, `include-hidden-files`, `compression-level`; `name`, `pattern`, `merge-multiple`, `artifact-ids`, `path` | An artifacts v4 client (create, signed upload, finalise; list, signed download) that writes the same zip layout and outputs | The zip also goes to the site store by digest; a download in the same site reads it there. The upload to GitHub stays, because the artifact must exist for users and for the oracle | Upload 2.9 s, download 2.5–3.2 s [M] → 0.3–0.5 s and 0.05 s [E]. Most uses disappear with the one-job shape |
| `actions/cache`, `/restore`, `/save` (2, 18, 5; 0, 178, 108) | `path`, `key`, `restore-keys`, `lookup-only`, `fail-on-cache-miss`; outputs `cache-hit`, `cache-primary-key`, `cache-matched-key` | A cache service v2 client with upstream's version hash and archive format, so entries are interchangeable with the oracle's; the root action saves in a post step | A restore whose entry is in the node store is a reflink clone (0.25 s for 10.6 GiB [P]); GitHub is asked only for the entry record | 44–61 s for the 1.92 GB entry, 13.6–22.4 MB/s from a pod [P v2 §2, §B.4.2] → 0.4 s [E]. Workflows generated for Velnor contain no cache step |
| `jdx/mise-action` (55; 61) | `version`, `sha256`, `install`, `install_args`, `cache`, `cache_save`, `env` | Puts the pinned mise and tools on `PATH` and exports what the action would. `install: true` checks that every pinned tool is in the image and fails by name if one is not | Everything: the tools are in the verified image | Set-up 2 s, binary download 4.6 s, tool install 10 s [P §A T9b, M M5] → 0.02 s [E] |
| `jdx/mr-boxington-action` (12; 0) | `toolchain`, `version`, `backend`, `working-directory` and the four cache switches | mbx from the image, configured as the action would | Everything | 1 s median, 2 s p90 [P] → 0.02 s. Not emitted for Velnor |
| `actions/attest-build-provenance`, `actions/attest` (8, 1) | `subject-path`; `predicate-type`, `predicate` | OIDC token with the Sigstore audience, an ephemeral key, a Fulcio certificate, a DSSE envelope over the in-toto statement, a Rekor entry, then `POST /repos/{owner}/{repo}/attestations`. `gh attestation verify` must accept it, and the statement must equal upstream's once times and certificates are removed | Nothing: a signature has to be public | 2–4 s [E: four calls], as today; but once per artifact digest instead of on every push (row 27) |
| `actions/create-github-app-token` (0; 6). v2's table omits it | `client-id`, `private-key`, `owner`, `repositories`, `permission-*`; output `token`, masked | An RS256 App JWT, `POST /app/installations/{id}/access_tokens` scoped as asked; the post step revokes the token | Nothing | 1.5 s [E] → 0.4 s |
| `asamarts/alint` (1) | `config`, `path`, `format`, `fail-on-warning`, `version` | Runs the pinned alint from the image | The binary | The job is 18 s today [P §A T4] → the lint itself, 2–5 s [E] |
| `tailrocks/velnor-new/actions/setup-velnor` (1; 122) | `version`, `sha256-*` | Checks the helper in the image against the given digest and exports the same `PATH`; a version that is absent is fetched once into the store by digest | The release download | 2–3 s [E] → 5 ms |
| Local composites (`./.github/actions/*`, 16; `declared-task-0` 279 times) | — | Composite semantics (3.5) | — | Unchanged; they leave the verification path when `velnor-gen` owns the workflow |
| `actions/github-script` (3) | — | None: it is JavaScript. `velnor-ci` commands replace the ruleset-drift check and the two qualification steps (v2, kept) | — | — |
| `submodules: recursive`, `lfs: true`, job containers, services, the Docker and pinned-Node actions (all only in `qualification.yml`) | — | Refused; they become the refusal cases of the corpus | — | — |

- **Tools arrive by image, never by step.** The `apt-get install musl-tools`, `rustup toolchain install` and `curl` of release-plz in today's release workflows become content of the tool image. A pin change builds a new image once, as a keyed action, during the first verification of the PR that changes the pin.
- **A new action SHA** (Renovate) fails `velnor-gen`'s closed-set test until the harness has passed at that SHA; the generator test and the executor's refusal are the two sides of one list.

## 5. The conformance oracle

`velnor-conformance` runs in velnor-lab against two scale sets: `oracle`, the latest official runner under ARC, and `velnor`, the candidate executor. One commit dispatches each case to both; jobs are paired by workflow, job, matrix and attempt, and everything is read back through GitHub's API.

### 5.1 Corpus

| Layer | Content | Size |
|---|---|---|
| A. Semantics | Generated from a case grammar: every expression function and coercion pair; every context; `if` × status functions × job states; `continue-on-error` × outcome and conclusion; timeouts; env precedence; file commands with heredoc edge cases; workflow commands and `stop-commands`; masking × 11 encoders × multi-line; annotation caps; background steps with `wait`, `wait-all`, `cancel` and the implicit wait; composites three deep with outputs and post steps; post-step order under failure and cancel; a cancel at every step boundary | About 600 one-job cases |
| B. Actions | Each native action × its supported inputs × edge cases, and interchange: a cache or artifact saved by one runner is restored by the other; `gh attestation verify` on both | About 150 |
| C. velnor-new | Every job of every workflow under each event that triggers it, at pinned commits, with recorded failures, cancels and re-runs; releases publish to lab repositories | 187 jobs today [M: `raw/wf_struct.txt`] |
| D. Refusals | Actions outside the set, a wrong SHA, an unknown input, Docker and Node actions, containers, services, the legacy message forms replayed from fixtures | About 40 |
| E. Wire | The oracle's HTTP traffic recorded through velnor-local's mitmproxy recorder, reduced to a shape model per endpoint: method, path, query keys, header names, JSON keys and types, ordering | Every endpoint of 3.2 |
| F. Expressions | A million generated expressions and contexts per night against both official evaluators through a C# host | — |

### 5.2 Normalisation and comparison

- **Normalised away:** ids, timestamps, durations; runner name, version and image; workspace, temp and tool-cache paths; tokens; ANSI codes; the text of "Set up job" and "Complete job"; action download lines; the interleaving of concurrent output (compared per stream).
- **Must be equal, exactly:** job and step status, conclusion and outcome; step names, numbers and order; step and job outputs; the environment each step sees; contexts as JSON; annotations (level, message, file, line, title); step-summary bytes; log lines per step; the workspace manifest after each step (path, mode, digest); cache effects (key, version, hit flags, restored files); artifacts (names, file lists, digests); post-step order; cancel and timeout conclusions. For layer E, Velnor sends no key the oracle never sends and omits none it always sends.
- **Anything else fails,** unless an owner-signed allow-list entry covers it; an entry expires after 30 days unless renewed.
- **A differing case is re-run three times.** A stable difference fails. An unstable one goes to a "nondeterministic" bucket that must stay under 1% with every member explained.
- **The harness is tested too.** It must catch ten planted differences (v2) and kill at least 95% of 30 executor mutants (swapped post-step order, a dropped masker encoder, a changed shell flag) before its green means anything.

### 5.3 Each new official release

An hourly poll of `actions/runner`'s latest release and of the runner image digest starts, on a change:
1. A new oracle image (5 min).
2. A **protocol surface diff**: the source diff between the two tags, restricted to about 40 files (`Sdk/RSWebApi`, the broker and Results clients, `Runner.Listener/{Runner,JobDispatcher,BrokerMessageListener}.cs`, `Runner.Worker/{StepsRunner,ExecutionContext,JobExtension,ActionCommandManager,FileCommandManager}.cs`, the handlers), as a report of changed endpoints, fields and constants (10 min).
3. Two corpus runs: new oracle against old oracle, which shows what GitHub changed whatever Velnor does, and new oracle against Velnor (about 40 min for 1,000 jobs on lab runners [E]).
4. Velnor raises the version it reports only after a green run.

The board is red or green within 2 h of a release [E]; the fix ships within 14 days, against GitHub's 63-day limit.

### 5.4 Protocol drift without a release, within hours

- **A canary every 10 min** in each site, through the production path: checkout, a cache round trip, an artifact round trip, an OIDC token, an attestation of a scratch subject, a step summary, a background step and a cancel, each read back through the API. Three failures in a row page the owner.
- **The drift sensor** on every production job message (3.3) and a shape check on every response from broker, run service and Results: an unknown key is counted, a missing required key is an incident.
- **A nightly corpus run** against the current oracle, compared with yesterday's oracle results, catches a server-side change that moves both runners.
- **A first-seen 4xx or 5xx class** from a protocol endpoint is an incident by itself.

### 5.5 Pass criteria for cutover, by slice

v2 gates one cutover on the whole corpus. The witness job uses a small part of the semantics, so I cut over in two slices.

| Slice | Jobs | Criteria |
|---|---|---|
| 1, week 4–5 | The generated `Velnor / Required` job: script steps, background steps, `wait-all`, no actions | Layers A and E for that surface and its jobs in C at 100% on five consecutive nights; one week or 150 merges of a non-blocking twin with equal verdicts, every difference fixed and replayed or shown a flake on both sides; fault injection at every remote call and step boundary; job start ≤ 2 s at P90 |
| 2, week 7–8 | Every other job: releases, images, scheduled work | All layers at 100% on five nights; mutation score ≥ 95%; two weeks or 300 merges of twins; the switch to the previous release drilled; then the change that routes them also deletes ARC's velnor-new scale sets |

The pass rate is published daily per layer from week 2. After slice 2, rollback is the previous Velnor release, never the official runner.

## 6. Compile, cache and test

### 6.1 The choice, on the measured numbers

| Option | Unchanged tree | Lockfile change, 11 crates | Cold node | Decision |
|---|---|---|---|---|
| Plain cargo over a reflink clone of the generation, fixed path | 0.25 s clone + 0.31 s, 0 crates, +2.5 MB [P cluster] | 67.4 s | 92 s cold build | **The hot path** |
| mbx with its local store | 0.32 s, but only through a path-keyed managed target that concurrent jobs at one path would share; 7.2 s and +1.94 GiB without it [P] | 88.4 s, +15.5 GiB, 6.8 GiB uploaded [P] | 93 s | Not on the path: slower where it matters, and a second definition of "same build" to audit |
| `mbx-cache` | — | 43.4 s on a second node after the first built it [P] | 39.9 s: 8.7 GiB at 215 MB/s [P] | Only if it beats the next row in a phase-1 head-to-head |
| File-delta sync of generations between nodes (manifest of path, size, BLAKE3; zstd) | — | About 40 s for the same 8.7 GiB on the same link; less compressed [E] | About 50 s for 10.6 GiB [E] | **Default across nodes**: the same bytes, no second key system |
| blaze canary 0.20261105.4 | Builds none of the five workspaces; cold 215.5 s against 17.9 s; edit rebuild 21.3 s against 1.5 s; 17.7–44 GB resident; its own nightly 1.101.0 instead of 1.99.1; a clippy `deny` lint not reported [P blaze] | | | **Rejected.** It would be a false green as built. Re-test when it builds all five workspaces on the pinned toolchain and passes 200 planted faults, clippy included, with none missed |

**Something better than all four: do not build twice.** v2 keeps a builder that compiles every `main` generation once per site after the merge group already compiled it: three compiles of each landed change. Here the merge group's build *is* the generation.

### 6.2 Generations

- **What one is.** Per Cargo root and lane (test build, clippy, doc): sources with recorded modification times, `target/` and the vendored registry, immutable, on the node's XFS volume, named by (lineage, tree hash of the root's closure, definition digest).
- **How it advances.** A cell that builds a merge group snapshots its build output when the build phase ends, before any test runs (0.25 s). When that exact SHA becomes `main`'s tip, the snapshot becomes the next generation by compare-and-swap, marked *provisional*. A root the change did not touch keeps its generation.
- **How it becomes trusted.** Audit A1 (6.5) builds the same tree cold and compares every output digest. Equal: *verified*. So every trusted generation equals a cold build of `main`, and a PR or merge-group build is never more than one delta away from one. A crate that is not bit-reproducible is listed and compared by verdict until it is fixed.
- **Replication.** Two replicas per root and site, updated by delta sync at batch priority; other nodes pull when placement needs them. Budget: the last three generations per root, about 75 GiB per warm node [E: 28 + 19 + 3 GiB, lanes × 1.5].
- **Rules kept from v2:** the fixed path `/w/velnor-new`; `velnor-ci inputs restore`'s time rule; a receipt-minting build starts from a trusted generation plus its own delta, never from an earlier push's `target/`; `CARGO_INCREMENTAL=0`.

### 6.3 What a key covers

The key of an action is the hash of what its cell contains (v2 C2). Decision 11's list, item by item:

| Input | How the key covers it |
|---|---|
| Sources, data files | Git tree hashes of the closure's paths. The cell holds only those paths; a read outside them fails and is logged |
| Toolchain and tools | The dm-verity root hash of the tool image, in the definition digest |
| Dependency versions | `Cargo.lock` is in the tree; cargo checks each vendored crate against its checksum |
| Features and their unification | Every `Cargo.toml` of the root is in every package key of that root, and the build command is the root's, so a manifest change elsewhere in the workspace changes the key |
| Env, flags, profile | The cell's environment is a fixed list in the execution manifest; command line, cargo profile and nextest profile are in the definition digest |
| Binaries a test starts (`CARGO_BIN_EXE_*`: 171 uses in 102 files [M]) | Built in the same cell from the same closure, their digests in the receipt; everything else on `PATH` is the image |
| Network | A loopback-only namespace. The fetch action alone has egress, and its output is addressed by the lockfile's digest |
| CPU count, path, kernel | Fixed per cell class, in the manifest |
| Time | **Cannot be keyed.** Receipts expire after 7 days; audit A2 runs at a new date every night; a time-shifted lane runs the suite at +8 and +30 days |
| Randomness | **Cannot be keyed.** Two executions before merge and one after; a key that ever produced two different results is never reused again |

### 6.4 Soundness of every reuse and skip

| Mechanism | (1) Why the key is complete | (2) Failure if it is not | (3) Detection | (4) Automatic fallback |
|---|---|---|---|---|
| Exact-tree receipt: rebase-only push, re-run, merge group on the PR's tree, push on the merge group's SHA | Whole tree, definition and manifest; nothing else is in the cell | A time-dependent or random test passes once | A1 on every landed commit; A2 nightly; the time-shifted lane; result disagreements per key | The key is marked non-reusable and runs every time |
| Closure-keyed receipt, per root then per package | The cell is a sparse checkout of the closure; an undeclared input is absent | A wrong closure: a test passes on a receipt though something it depends on changed | Enrolment only when the restricted run equals an unrestricted one in inventory and outcome; the read-miss log; A2 runs everything whatever the keys say | The package falls back to its root key, the root to the exact tree |
| Compile reuse from the generation | cargo's fingerprint: compiler, flags, features, dependencies, the files of the last dep-info, `env!` values; times set by the agent | A stale artifact: a new `mod.rs` beside `x.rs`, a proc macro that reads an untracked file, a stale `OUT_DIR` | A1: cold build of the same tree, digest of every test binary and release artifact compared, minutes after landing | The lineage is retired; that root builds cold (92 s [P]) until explained; commits since the last verified generation are re-audited |
| In-flight join | Same key, one execution | None new | — | — |
| Release promotion | Closure, release definition, target, tool image; the bytes' digest is in the receipt and in the attestation | Wrong bytes published | The publish job re-hashes and runs `gh attestation verify`; A1 rebuilds the artifact cold and compares | The release stops with the key named. A rebuild is never the fallback |
| Pre-push `velnor verify` | The same cells and keys; the tree arrives as Git objects | None new: only cells sign, never a developer's machine | The same audits | Switch per user |
| Tool image, vendored crates | Content-addressed; verity checks every read | A corrupt tool | A read error, which is red | Rebuild the image |
| No run for an unlabelled head | Not a reuse: no check exists (section 8) | GitHub reports a skipped job as success | The App's second check; a canary PR that must never be mergeable | The ruleset blocks; the merge group verifies regardless |

A new class runs in shadow, every reuse re-executed at batch priority, until 3,000 reuses agree (a 95% bound of 0.1%). A disagreement is re-run five times to tell a flake from a wrong key; a confirmed one raises the policy epoch as in v2 §4.4.

### 6.5 Audits and the confidence samples

| Audit | When | What | Cost [E] |
|---|---|---|---|
| A1 | Every landed commit, batch priority, 2–5 min after landing | Cold build of every root the commit touched; one execution of the tests in its changed closures; digest comparison with the warm path | 2.9 runner-min per merged PR; about 19,000 CPU-s a day at 40 merges (40 × (0.47 × 120 + 0.25 × 1,200 + 0.14 × 800)), 0.05% of 480 threads |
| A2 | Nightly, every 10th merge, and on any planner, image or toolchain change | Everything, cold, on all free threads; nightly also at +8 and +30 days, through a preloaded clock shim [E: test binaries link glibc dynamically] | 30 runner-min per run, 3.8 per merged PR |

- **Samples per changed test:** two before merge, in parallel, and A1 after it. Today: PR run, merge group, push run. Any failure blocks or, after landing, opens an issue with a prepared revert.
- **What v2 does instead:** three parallel executions before merge and a cold build on every 20th merge. That is one more foreground execution than today and a 20-merge window for a stale generation.
- **Flakes.** Only changed closures run, so a flaky test elsewhere can no longer eject anyone; this removes most of today's 8.1% false failures [P], which came from full runs. Retries stay at 0. A key with disagreeing results is listed, owned and never reused.

### 6.6 Profiles and release artifacts

Measured [M R3]: a release-profile test build shares 15 of 90 crates with the release binary, because the binary is `panic = "abort"` and tests need unwinding.

| Option | CPU per cold build, `generator` / `ci-helper` | What is tested |
|---|---|---|
| Test the release build | 320.8 / 365.7 CPU-s (binary, then tests) | Optimised code without debug assertions or overflow checks; still not the shipped panic strategy, nor musl |
| Debug tests, release binary built once | 191.9 / 282.8 CPU-s | Today's coverage; the shipped bytes get their own checks |

**Decision: debug tests; the release binary is built once per closure key at merge time, and its bytes are qualified once.**
- **Build.** One action per binary and target, in the release profile, keyed like any other. It starts with the PR's verification at batch priority and is required by the merge-group and `main` plans: 30–40 s, 500 CPU-s for both tools and two targets [E: 297 CPU-s cold on an M5, half of it dependencies already in the generation, × 2 targets × 1.7 for an EPYC core].
- **Qualify.** `--version`, `check-tag` and the black-box suites that run today only after the merge (source-preview qualify, 204–258 s [M R1]) or at release, keyed by the artifact's digest. Today a release build that does not link is found on `main`; here it fails the merge group.
- **Pack and attest.** A deterministic `tar` (sorted names, zero times and owner), `SHA256SUMS`, and the attestation by digest in the `main` push job, whose identity is the honest provenance.
- **Release.** A tag runs one job: gate, fetch by digest, re-hash, `gh attestation verify`, publish. About 15 s [E: 9 s offer, five calls], against 190–434 s and 4,105–6,238 s [M R2]. A missing receipt fails the job; nothing is rebuilt silently.
- **The stamp.** `option_env!("VELNOR_COMMIT")` makes every commit a new compile input, so each merge would rebuild both tools. The commit moves to a fixed-size link section patched after link; the compile then repeats only when the closure changes, about half of all merges [M: 47% touch `generator/` or `ci-helper/`].
- **arm64 Linux without a hosted runner.** `aarch64-unknown-linux-musl` is cross-compiled on the cluster with rust-lld and a pinned cross C compiler for `ring` (in `ci-helper/Cargo.lock` only [M]) and smoke-run under user-mode `qemu-aarch64`. The macOS legs are disabled.
- **Faster tests without touching release:** dependencies at `opt-level = 3` in the test profile cut the slowest test class by 66% with assertions off in dependencies [M M14]; with them on it is unmeasured, so it is measured first (v2 question 8, kept).

### 6.7 Tests: the floor and the cluster

**The single-test floor** is 125–138 s after the two loops are split [P v2 §6.3]; one such test alone is 142.5 CPU-s with no block I/O, in `turso_parser` and the allocator of a debug build [M M6]. In order:

| Step | Factor | Longest test |
|---|---|---|
| Journal template; expensive prefixes built once and cloned per case (v2 work item 5) | × 0.55 [P, PR #1349's claim] | 69–76 s |
| Optimised dependencies in the test profile | × 0.34 [M M14], to be confirmed with assertions on | 23–26 s |
| Long tests on whole cores: the cell's cpuset keeps the SMT sibling idle | × 0.75 [E] | 18–19 s |
| One high-clock node for the serial remainder | × 0.7 [P v2 §14] | 12–14 s |

Committed: no test over 60 s in phase 2 and over 20 s in phase 3, held by a blocking check on any new test above 10 CPU-s measured alone.

**Test-level scheduling.**
- **Unit:** a test binary's digest and a list of test names. After the build, binaries and their runtime files go to the site store by digest; a node fetches only what it runs (the whole archive is 1.48 GB [P]).
- **Scheduler:** longest first over every free thread of the site, from the CPU-seconds each test recorded in its receipts. A shard runs under nextest with the repository's profile, so groups, `threads-required`, wrappers and timeouts hold. The executor checks that every test of `nextest list` reported exactly once; a missing or duplicate case fails.
- **When it pays:** wall time is max(longest test, CPU ÷ threads) plus transfer. `runner-host` needs about 3,000 CPU-s today [E: 1,857 s for 232 tests unloaded [M], scaled to 382]: 47 s on 64 threads of one node, under a 125 s floor. After the floor work it needs 560 CPU-s [E] and one node gives 20 s. So a PR rarely needs a second node; A2 does (5,000–20,000 CPU-s over 400 batch threads: 12–50 s plus the floor), and so does a busy node.
- **Hedging:** a shard past its predicted P95 starts again on another node, and the first result wins when both end the same way. This is a race, allowed by decision 10; it costs at most 5% of test CPU [E] and removes the tail of a starved node.
- **Honest CPU:** nextest's threads equal the cell's CPUs; the 2 s kills in `ci-helper` and `generator` get sized timeouts.

## 7. Scheduling and the cluster

Five workers, 480 threads and 627 GiB: two in Germany, three in Finland, 35 ms apart; one k3s server [P §B]. Foreground demand in this design is small: 22 cell-minutes per merged PR × 40 merges a day is 15 cell-hours, 2–3% of the threads [E]. The cluster is sized for latency, so scheduling is about never waiting and never interfering.

### 7.1 Cells

A cell is a single-use sandbox the node agent makes ahead of need inside its own delegated cgroup: user, mount, PID and network namespaces, a seccomp profile, no capabilities, a read-only tool image, a writable XFS directory under a project quota, a tmpfs `/tmp`, `pids.max`, `memory.max` and a cpuset. Starting a process in a ready cell takes 5–20 ms [E]; a pod takes 5.6 s [M]. The cell is destroyed with `cgroup.kill` and its directory unlinked; nothing is reused.

| Class | CPUs | Memory | Network | Runs |
|---|---|---|---|---|
| gate | 1 | 2 GiB | GitHub API | Witness jobs, lints, the plan |
| fetch | 2 | 2 GiB | Crates mirror only | One fetch per lockfile digest |
| build | 24 = two core complexes [E: 9454P has eight of six cores; confirm with `lscpu`] | 24 GiB | Loopback | A root's build and its clippy and doc lanes |
| test | 12–64 | 16–32 GiB | Loopback | Test shards |
| docker | 12, plus a daemon inside the cell | 24 GiB | Loopback, registry mirror | The suites that need a daemon; v2's two tainted nodes until the rootless experiment passes |
| batch | Whatever is free, `cpu.idle`, killable | Capped | As its class | Audits, speculation, replication, shadows, the lab corpus |

- **Why not a pod per request (v2).** Pods cost 6 s to create, need the API server on the hot path, and cannot be made per test shard. Kubernetes still deploys the agents and services and holds the other organisations' quotas; the agent pod requests Velnor's share with guaranteed QoS.
- **Isolation fits decision 8.** The threat is buggy code from trusted authors: a runaway build, a fork bomb, a full disk, a leftover process, a write outside the work directory, a port clash, an accidental download. The cell stops each of these. There is no fork class, no microVM and no hosted runner in the design.
- **A cell never writes a store.** The agent harvests outputs, observes exits and test inventories, and signs receipts; an obligation has no token and no path to the ledger.

### 7.2 Placement, zero queue, zero cold start

- **Placement** is per obligation and minimises predicted finish: start delay, plus view set-up (0.5 s if the node holds the generation, a 40–50 s sync if not), plus the run time recorded in receipts. An idle warm node beats a busy warm one, which beats an idle cold one. A request stays in one site.
- **Zero queue** means eligible-to-started ≤ 100 ms at P90 and ≤ 2 s at P99 for foreground work. It holds because batch work is preempted in milliseconds (its cpuset shrinks; `cgroup.kill` frees its memory, and its work is idempotent), cells exist before the work, 48 GiB per node stays free or reclaimable, and no job runs on a hosted runner.
- **Zero cold start** means a cold path is paid once per new input and never by a job: sessions are registered, cells are made, the tool image is mounted, the mirror fetches on the webhook, and two verified replicas of each generation exist per site. A new lockfile costs one fetch (≤ 1 s), a new tool pin one image build, a node joining one sync before it advertises capacity.
- **The control plane is off the hot path.** Cells are not pods, so losing the single k3s server stops deployments, not verification. v2 makes three servers a precondition of the cutover; here it is hygiene.

### 7.3 Using all 480 threads

- **Foreground takes whole cores.** A foreground cell's cpuset is exclusive and aligned to core complexes; batch cells and the other organisations' pods lose those CPUs the moment it starts. That removes the 1.2–2.4× stretch under load (runner-host 3.7 → 8.8 min [P §A T6]) and the flakes of the 2 s timeouts.
- **Width is free, so use it:** a build cell has 24 threads; a test lane takes up to 64; second executions run beside the first.
- **Batch soaks the rest** at `cpu.idle`: A1 and A2, queue-tip and bypass-tree speculation, generation replication, the time-shifted lane, shadow re-execution of new reuse classes, the lab corpus. A full cold audit spreads over 400 threads in 12–50 s plus the test floor [E].

### 7.4 Both sites, and the other organisations

- **Each site is whole:** dispatcher with standby, ledger, store, hooks, mirror. The App's webhook name resolves to both. A PR is pinned to a site by its number, so its pushes stay warm; a site that is down or full hands its requests to the other.
- **Receipts replicate** between sites in about 2 s [E]; they are small. **Generations replicate** at batch priority. Whether a site syncs 8.7 GiB or replays a 67 s build locally is decided per root by measuring the link: a local build that beats a fetch is allowed, with the number shown.
- **ChainArgos and jackin stay on ARC** in their namespaces until `velnor-gen` can regenerate their workflows into the closed set. They are tenants of the nodes, not jobs Velnor runs. They get honest requests, a CPU and memory quota from two weeks of measured use, a weight below Velnor's foreground and above batch, and they never share a cpuset with a foreground cell.

### 7.5 Host and kernel tuning

| Rank | Change | Expected gain | Basis |
|---|---|---|---|
| 1 | Exclusive, complex-aligned cpusets for foreground cells; the SMT sibling left idle for long tests | No stretch under load; up to 25% on a single-threaded test | [P §A T6]; [E] |
| 2 | Work directories, stores and generations on the XFS data volume with project quotas | The 0.25 s clone instead of an 8.6 s, 10.75 GiB copy | [P cluster] |
| 3 | Kube and system reservations, a memory eviction threshold, `podPidsLimit` | No node-level surprises | [P §B.1.3]: none set today |
| 4 | Debuginfo: line tables for workspace crates, none for dependencies | −20 to −27% CPU on a `runner-host` rebuild; −10 to −24% generation size | [P v2 §8] |
| 5 | `amd-pstate` with the performance preference | 2–10% | [P v2 §8], measure first |
| 6 | Generations kept in page cache after a sync; dirty-page limits sized for link bursts; THP on `madvise` | Small; measure first | [E] |
| 7 | `iperf3` between workers in and across sites | Decides sync against replay (7.4) | Unmeasured |

Kept from v2: SMT on, kernel 6.12, flannel over WireGuard, no striping, no tmpfs work directories.

## 8. The merge queue and GitHub integration

GitHub documentation, fetched 2026-11-19 through a summarising tool, so quotes are as it returned them: **[W1]** `docs.github.com/en/pull-requests/reference/status-checks`; **[W2]** `…/collaborating-on-repositories-with-code-quality-features/troubleshooting-required-status-checks`; **[W3]** `docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow`; **[W4]** `docs.github.com/en/webhooks/webhook-events-and-payloads`; **[W5]** `docs.github.com/en/rest/actions/workflows`; **[W6]** `…/configuring-pull-request-merges/managing-a-merge-queue`.

### 8.1 The trigger: the owner's five steps, on GitHub's own events

```yaml
on:
  pull_request: { types: [labeled, unlabeled, synchronize] }
  merge_group:
  push: { branches: [main] }
  workflow_dispatch: { inputs: { pr: { type: string, required: true } } }
jobs:
  velnor:
    name: Velnor
    if: >-
      github.event_name != 'pull_request' || (
        github.event.pull_request.state == 'open' &&
        github.event.pull_request.head.repo.full_name == github.repository &&
        contains(github.event.pull_request.labels.*.name, 'velnor:verify'))
    concurrency:
      group: velnor-${{ github.event.pull_request.number || inputs.pr || github.run_id }}
      cancel-in-progress: ${{ github.event_name == 'pull_request' || github.event_name == 'workflow_dispatch' }}
    uses: ./.github/workflows/verify.yml   # its one job is named Required
  stop:
    name: Velnor stop
    if: github.event.action == 'unlabeled' && github.event.label.name == 'velnor:verify'
    concurrency: { group: 'velnor-${{ github.event.pull_request.number }}', cancel-in-progress: true }
    runs-on: velnor-gate
    steps: [{ run: 'true' }]
```

**The witness job** is all `verify.yml` contains: `runs-on: velnor-gate`; one background step per lane, `run: velnor-ci ensure --lane <root>`; a `wait-all`; then `run: velnor-ci gate`. It checks nothing out and builds nothing. The ledger plans from `github.sha` with the released planner, the agents execute in cells, and each step streams its lane's log over a socket that exposes only this job's request.

| Owner's step | What happens |
|---|---|
| 1. A draft PR receives pushes | `synchronize` without the label: `velnor` is skipped on GitHub's side, no job reaches a runner, and no check named `Velnor / Required` exists. The required check is pending, so the ruleset blocks |
| 2. Ready for review | `ready_for_review` is not a trigger. Nothing happens |
| 3. The label is added | `labeled`, and the label is now present: the job runs on the current head |
| 4. A push while labelled | `synchronize`: a new job. Job-level concurrency cancels the old one; Velnor completes it as cancelled in under 1 s |
| 5. The label is removed | `stop` cancels the job in flight through the shared group; later pushes skip |

Nothing else starts CI: `opened`, `reopened`, `edited`, `ready_for_review` and auto-merge are not triggers. Another label added to or removed from a labelled PR re-requests the same head; the request joins the running executions or finds their receipts, so it costs one witness job and no CPU, and the design does not depend on how GitHub treats the concurrency group of a skipped job. A fork's PR fails the `if` and stays pending for ever; that one line is all the fork handling there is.

**The trap this shape avoids.** "A job that is skipped will report its status as 'Success'. It will not prevent a pull request from merging, even if it is a required check" [W1]. Had the job named `Required` carried the `if:`, every unlabelled push would be green: a false green by configuration. Here the skipped job is the *caller*, whose check is named `Velnor`; `Velnor / Required` is never created, and a required check that was never reported stays pending and blocks [W2]. That a skipped caller creates no nested check is observed behaviour, not a documented promise, so work item 5 tests it and the design does not rest on it alone.

**The second lock.** The Velnor App posts its own check run, `Velnor / Ledger`, on a commit: `in_progress` while verifying, `success` only when the ledger holds a passing verdict for that tree. The ruleset requires both checks. Then no skipped-job rule, no edit to `ci.yml` or `verify.yml` in a PR, and no `runs-on` pointed elsewhere can open the gate. The executor adds gate authority (3.3): the Actions job is green only if the ledger says so.

**How label events reach Velnor.**
- **Channel A, the workflow run.** GitHub creates the job and offers it over the scale-set session, an outbound long-poll that needs no ingress. It is suppressed when the event was caused with `GITHUB_TOKEN`: such events "will not create a new workflow run", except `workflow_dispatch` and `repository_dispatch`; a push made that way creates runs that wait for approval; the documentation's own example is a label added with `GITHUB_TOKEN` that starts no label workflow [W3]. In velnor-new 93 of the last 100 PRs belong to the owner's user account and 7 to Renovate's App [M R4], and both start workflows normally.
- **Channel B, the App's webhook.** `pull_request` with `labeled` ("A label was added to a pull request"), `unlabeled` and `synchronize` ("A pull request's head branch was updated"), for an App with read access to pull requests; `merge_group` with `checks_requested`, for read access to merge queues [W4]. The page states no exception for `GITHUB_TOKEN` actors; suppression is a property of workflow runs. Work item 5 confirms it by labelling from a workflow.
- **Roles.** A is the source of truth: nothing depends on Velnor's ingress. B is the head start (work begins about 7 s before the offer) and the repair path.
- **Fallback.** When B shows a labelled head with no run after 20 s, the App calls `POST /repos/{owner}/{repo}/actions/workflows/ci.yml/dispatches` with the head branch and `inputs.pr`; the call returns the run id [W5], and `workflow_dispatch` always creates a run [W3]. The executor verifies `github.sha`, never the input. A reconciler lists labelled open PRs every 60 s and repairs any head without a run; missed deliveries are asked for again through the App's deliveries API.
- **If the lab shows a skipped caller reporting success,** the fallback becomes the trigger: the `pull_request` trigger goes, the App dispatches on `labeled` and on `synchronize` while labelled, and an unlabelled push creates no run at all, which is the documented pending case [W2].

### 8.2 The ruleset

| Setting | Today [M R4] | Design |
|---|---|---|
| Required checks | `Required verdict`, `Required`; no `integration_id` | `Velnor / Required` from GitHub Actions (15368) and `Velnor / Ledger` from the Velnor App. The same two apply to PR heads and to merge groups: the queue "will wait for required checks to be reported" on each group, and the workflow must trigger on `merge_group` [W6] |
| `strict_required_status_checks_policy` | false | false: the queue verifies the merged tree |
| Grouping | HEADGREEN | ALLGREEN |
| `max_entries_to_build` | 2 | 8 (at most 12 enqueues an hour today [M F7]) |
| `min_entries_to_merge`, wait | 1, 5 min | 1, 0 |
| `check_response_timeout_minutes` | 90 | 15 |
| Merge method | Squash | Squash |

- **Latest commit only.** "Required checks must pass on the latest commit SHA" [W2]; a push makes a new SHA with no check, and with the label present `synchronize` starts its verification.
- **ALLGREEN.** Under HEADGREEN a failed entry lands inside a group whose tip passed, as happened once in 15 days [P v2 §13]: a commit on `main` with no passing verdict on its own tree, which decision 12 excludes. v2 keeps HEADGREEN because an ejection is expensive; here it is a lookup of pre-verified trees (8.4).

### 8.3 No duplicate verification across PR, merge group and `main`

| Event | Tree | What executes |
|---|---|---|
| PR, labelled | `github.sha`: the head merged into its base | The closures the PR changed, tests twice; release-artifact lanes at batch priority |
| Merge group | The group's commit | Nothing when its tree equals the PR's (78–89% at a 1–5 min window [P]); otherwise the closures that moved, tests twice |
| Push to `main` | The merge group's SHA in 120 of 159 cases [P] | Nothing. The job promotes the generation, attests the artifacts, starts A1: 15 s |
| Release tag | A commit on `main` | Nothing: promotion (6.6) |
| Direct push to `main` | No receipts | Everything, then A2 |

- **Identical inputs only.** The PR's verdict as a whole serves a merge group only when the tree is identical, as decision 12 requires. For a different tree the merge group computes its own verdict, and a receipt serves an obligation only when that obligation's closure tree is identical. Two PRs that pass alone and fail together must share a closure, so that closure's key changes when the first lands and the second runs again.
- **Trusted scope.** A result is `candidate` until three things hold: the released planner computed its key from a commit on `main`; its signature, cell class and generation lineage verify; and A1 passed for that commit. Then it is `trusted`. That is the "verification" decision 10 asks for before a PR-produced result serves `main`. State without a key, such as a PR's own `target/`, is never promoted.
- **Stacked and concurrent PRs** need no mechanism of their own: equal closures give equal keys.

### 8.4 Speculation on the queue tip

- When a labelled PR with auto-merge turns green, Velnor computes `git merge-tree` of its head onto the current queue tip and calls `ensure` at batch priority, 24 s before GitHub's merge-group job exists. 122 of 193 merge groups were built on an unlanded group commit [P], so `main`-based receipts alone would miss whatever the entries ahead touched.
- For ALLGREEN it also verifies the trees left when any one entry ahead is ejected: n trees for n entries, mostly lookups.
- It runs in shadow first, predictions against GitHub's real merge-group trees over 50 merges, and is dropped below a 70% match (v2 M5, kept). A wrong guess wastes batch CPU, never a verdict.

### 8.5 The `Required` and release-gate constraints

- `velnor-ci release-gate` ties a required check to a push run only if it comes from GitHub Actions (`ci-helper/crates/velnor-ci/src/gate.rs:15,277-288`), and `product-release.yml` asserts exactly one such job in `ci.yml`'s push run [P v2 §6.4]. The push run keeps exactly one Actions job, now named `Velnor / Required`.
- One change renames it in `gate.rs`, the release workflows and the ruleset together. The App's check enters through `gate.rs`'s existing exemption list (`mod exemptions`).
- The gate gains one condition: the SHA's release-artifact receipts exist.

### 8.6 Auto-merge and agents

- 0 of the last 100 PRs use auto-merge [M R4], and the median PR waits 7.5 min between green and enqueue [P]. The agents' rule becomes one command when work is ready: add `velnor:verify` and enable auto-merge.
- Agents run `velnor verify` before pushing, never merge `main` into a branch, and read a pending check as "not requested", not as a failure to fix.
- Renovate adds the label through its own configuration if the owner wants its PRs verified unasked; it is still the label that starts CI.

### 8.7 The cancelled-run stall

- **Cause today:** workflow-level concurrency plus a `Required` job on `if: always()`, which must be scheduled on a hosted runner before the group frees. 22% of PR runs waited 26–2,203 s, and 13 of 17 started within 1 s of the old run's end [M F2, F3].
- **Design:** concurrency sits on a job Velnor runs. The cancel arrives through the broker, the witness completes as cancelled in under 1 s, and no `always()` job exists.
- **Velnor never waits for GitHub anyway.** The new head's `ensure` started on the webhook; executions of the old head that no other request needs are killed at once. What is left is GitHub's own cancel propagation, 2–4 s [E], visible only when the new verdict would otherwise be instant.

## 9. What would make CI not a bottleneck at all

**The absolute.** CI is not a bottleneck when nobody, human or agent, ever waits for it: the verdict exists before the decision to merge, and merging costs only what GitHub itself takes.

### 9.1 The floor for this repository

| Cost that remains | Seconds | Source |
|---|---|---|
| Event → Velnor holds the job | 9 | 6 s for GitHub to create a `pull_request` run, 2 s to its first job [M F1, F2], 1 s to acquire [E] |
| `completejob` → the check is green | 2 | [E] |
| Auto-merge → enqueued | 5 | [E] |
| Enqueued → the merge-group run exists | 19 | [M] |
| Merge-group job offered, looked up, completed | 5 | 2 s [M F2] + 3 s [E] |
| Merge group green → merged | 26 | [M] |
| **Push-to-merge floor** | **66** | 11 s for the PR check, 55 s for the merge queue |

- **Verification adds max(0, V − 9) s**, because its first 9 s overlap GitHub's job creation. V is 23 s for the typical PR (2 + 0.5 + 0.5 + 10 + 10), which gives 80 s; V is 0 when the tree was verified before the push, which gives 66 s.
- **Throughput has no floor that matters.** With 8 entries building, ALLGREEN and receipts, entries do not wait for one another: the queue carries more than 100 merges an hour [E: 8 entries per 55 s, less ejections] against 1.9–7.5 today [M, my proposal §1.1].

### 9.2 The design that reaches it

1. **Verification runs before the push.** The agent's harness calls `velnor verify` on each local commit. The tree goes to the node mirror as Git objects, and the same cells and keys produce the same receipts. This is still on demand: the agent asks, and nothing runs for a tree nobody asked about. Agents already leave a median 11 s, and over 60 s in 27% of cases, between commit and push [M F6]; the typical verdict needs 23 s. The label then only publishes a verdict that exists: 11 s.
2. **The merge group is a lookup,** by exact tree when `main` did not move and by queue-tip speculation when it did: 5 s.
3. **A release waits for nothing:** its artifacts were built, qualified and attested at merge time (15 s).
4. **Verification sits at its own floor:** closure keys, a verified generation one delta away, wide cells on whole cores, the big crate split, no test over 20 s.

With these, the agent waits 0 s for CI while it works (the verdict on commit N arrives while it writes commit N+1), and a finished PR merges 66 s after its last push.

### 9.3 The irreducible costs

| Cost | Size | Who owns it |
|---|---|---|
| GitHub's event-to-job and merge-queue latency | 9 + 2 + 55 s | GitHub. Decision 7 keeps the merge queue, so v2's question 11 (Velnor landing merges itself, about 15 s in all) is closed. Work item 5 still measures two things inside GitHub's surface: whether a dispatched run appears in 1–2 s like a `push` run instead of 6 s, and whether the 19 s and 26 s move with the queue's settings |
| One rustc run per changed crate, one link per affected binary | 10 s typical; 59 s for `velnor-runner-host` today [P], 10–15 s after the split [E] | The code's shape |
| One execution, two by choice, of each test whose inputs changed | Bounded below by the longest changed test: 125–138 s today, 12–20 s after 6.7 | The tests |
| One fetch per new dependency, one image build per new tool pin, one cold build per new toolchain | ≤ 1 s; 1–2 min; 92 s per large root [P] | Paid once, by the PR that introduces it |
| Signing | 2–4 s per artifact digest | Sigstore's round trips |
| Audits | 6.7 runner-minutes per merged PR | CPU, never latency |

Everything else is removable and is removed here: queueing, cold starts, per-job set-up, repeated compiles and tests, gate hops, release rebuilds.

## 10. Critique of v2

v2's core is right and I keep it: the key is the hash of what the sandbox contains (C2), only what Velnor observed is evidence (C4), `ensure` is idempotent with in-flight join, plain cargo runs over a reflinked snapshot at a fixed path, receipts are signed per class with a policy epoch, and the journal discipline and line budgets stand. Under the new decisions it is wrong or silent in these places.

| v2 section | What is wrong or missing | Here |
|---|---|---|
| §3.4 step 1, §6.1, §9 M1 and M3 | "A push to a pull request … calls `ensure`": every push starts heavy verification. That is decision 12's opposite. v2 has no label, no `labeled` trigger and no analysis of pending against skipped; a generated job named `Required` behind a job-level condition would be green when skipped [W1] | 8.1 |
| §12 | No risk row for the gate itself: a skipped job counted as success, a PR that edits the workflow, a `runs-on` pointed at a hosted runner. Integration 15368 is every Actions job, hosted ones included | The App's second check; gate authority |
| §1.2 C1, §6.5, question 3 | HEADGREEN stays, and commits that land without a verdict are "listed for bisection". Decision 12 requires a passing verdict on the exact merged tree | ALLGREEN from phase 2 |
| §1.2, §6.3 | "Every obligation whose inputs the change touched executes three times": one more foreground execution than today's two pre-merge samples, against decision 10 | Two, plus the cold audit as the third |
| §1.2 C6 | Cold builds only nightly and on every 20th merge, so a stale generation can serve 20 merges. The 24 h execution of every landed tree is warm | A1 cold on every landed commit, 0.05% of the CPU |
| §3.3, §4.2, §5.1, §5.5 | A builder compiles every `main` generation once per site after the merge group compiled it, because "a merge group never writes the trusted scope": three compiles per landed change | The merge group's build is harvested and verified by A1 |
| §5.1 | `mbx-cache` publishes every generation (6.8–8.7 GiB uploaded per change [P]) and is judged after four weeks | It must first beat a file-delta sync head to head |
| §3.1, the API table | The evaluator row misreads the source. `Expressions2` is the evaluator the runner's own code calls "Legacy" (`R/Runner.Worker/ExecutionContext.cs:1546-1562`); no `DTExpressions` tree exists; the new `Sdk/Expressions` and `WorkflowParser` are absent from the design. The row "node24-era runner semantics" means nothing without Node | 3.4 |
| §3.3, §3.5, work item 7 | The executor's scope omits background steps (105 uses in `ci.yml`), `hashFiles` (Node upstream), sparse checkout (44 uses; velnor-legacy's checkout refuses it [P §E.1.4]), `actions/create-github-app-token` (6 uses in the release goldens), and input-level refusal | 3.5, 4 |
| §6.4, §6.1 | The push run "does release duties", which rebuild. Promotion does not exist in v2. Missing with it: the source-preview builds (1,693 s per push [M R1]), the product release, the tools release, release-plz's jobs, the commit stamp, arm64 without a hosted ARM runner, the profile question (question 8 asks only about `opt-level` in tests) | 6.6 |
| §6.2–§6.4, phases 0 and 2 | Source qualification (80 runner-min per merged PR), Job image candidate (20) and State seal (9–11) are "narrow the trigger" and "retire later" notes, not duplicate work with a key | 2b rows 15, 27, 29 |
| §3.7, §3.8, §5.4 L3, §6.1, questions 4 and 7 | A fork class, a fork scope, a fork-conditional `runs-on`, hosted runners for forks and macOS, GitHub's cache for hosted workflows, the Free-plan question: all void under decisions 8 and 9. Disabling the macOS workflows is not in phase 0, although they take 1,267 s per `main` push | One `if` line; phase 0 |
| §3.8, §7, §3.9 | A pod per request puts the k3s API on the hot path, which then makes three servers a cutover condition; pods cannot be made per test shard | Cells |
| §7 | A static 16-CPU reservation per site for the merge path, and build pods with no CPU limit, which keeps the 1.2–2.4× stretch under load | Exclusive cpusets; batch preempted in milliseconds |
| §3.6 | A new official release is tested "within 24 h"; no wire-level layer; the harness is checked only by ten planted differences | 2 h; layer E; mutants |
| §10 | One cutover gated on the whole corpus, two weeks of twins and control-plane work, with "none [of the targets] depends on the executor cutover's date": the Rust-only decision has no date | Two slices, the first in week 4–5 |
| §9 M8 | Verification before the push is a phase-3 stretch, to be killed "if agents rarely test before pushing" | The main road to the floor (9.2) |
| §1.4 | Commits 90 s where its own arithmetic gives 35 s (§3.4), and clocks push-to-merge from a push that, under decision 12, starts nothing | Commit 45 s; clock from the request |
| Questions 11, 12 | A Velnor merge train conflicts with decision 7. A retry for tests the change could not have broken is moot: those tests no longer run | Closed |

**Decisions of v2 I would overturn:** `ensure` on every push; HEADGREEN; three executions; the builder and "a merge group never writes the trusted scope"; a cold build only on every 20th merge; one whole-corpus cutover; a pod per request; `Expressions2` as the engine; every fork, hosted and macOS path; release by rebuild; question 11.

## 11. Phases

Clocks: "verdict" runs from the request (the label, or a push while labelled) to `Velnor / Required`; "push-to-merge" from that request on the final head to merged, for PRs with auto-merge. "Hit" is the share of compilation units a foreground build takes from the generation, then the share of merge groups that execute nothing. "Conformance" is the lab corpus pass rate. A phase's gate needs 300 merges or four weeks, with intervals beside each figure.

| Phase | Scope | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min per merged PR | Hit | Conformance |
|---|---|---|---|---|---|---|
| 0, week 1 | Configuration only, on today's runners: the label trigger and ruleset; macOS legs off; every job on the cluster; Source qualification on demand; Job image candidate only when its inputs change; State seal and prune retired; agents label and enable auto-merge together; the week-1 experiments | ≤ 12 / ≤ 20 min | ≤ 25 / ≤ 60 min | ≤ 260 [E: 1.5 × 52.9 + 1.25 × 64.6 + 62 + 1.5 × 16.8 + 7], from 460–490 | 0%; 0% | Not built |
| 1, weeks 2–4 | The action plane, with the official runner still carrying the witness job: agent, cells, stores, generations with A1; ledger with exact-tree and per-root keys; `velnor-gen` emits the one-job workflow; the App's check; the tools release promoted; the test-floor work starts; in the lab, executor, dispatcher and harness layers A and E | ≤ 90 s / ≤ 6 min | ≤ 4 / ≤ 12 min | ≤ 60 | ≥ 95%; ≥ 60% | ≥ 90% of slice 1, published daily |
| 2, weeks 4–7 | **Slice 1 goes native.** Per-package keys; queue-tip speculation; ALLGREEN and 8 entries; product-release promotion; no test over 60 s; `velnor verify` opt-in; the other workflows regenerated into the closed set | **≤ 45 s / ≤ 3 min** (committed) | **≤ 2 / ≤ 6 min** (committed) | ≤ 30 | ≥ 97%; ≥ 85% | 100% of slice 1 before its cutover; ≥ 95% overall |
| 3, weeks 7–10 | **Slice 2 goes native; ARC's velnor-new scale sets are deleted.** Test-level scheduling with hedging; the big crate split and compiled once; no test over 20 s; the time-shifted lane; `velnor verify` in every agent's loop | ≤ 25 s / ≤ 90 s; 12 s when pre-verified (stretch) | ≤ 75 s / ≤ 3 min (stretch) | ≤ 12 | ≥ 98%; ≥ 90% | 100%, all layers; mutants ≥ 95% |
| 4, later | The other organisations onto Velnor; a Mac host as a cell class when macOS returns; a high-clock node; keys by test-binary digest | Floor: 11 s | Floor: 66 s | — | — | Per platform |

- **Phase 2's P90** holds only with "no test over 60 s"; without it the runner class keeps P90 near 5 min (67 s of compile and 192–246 s of tests [P]).
- **The bridge is short and one-way.** Until slice 1 the official runner under ARC is the incumbent that today's jobs already run on, not a fallback. Once a slice has cut over, that slice never returns to it.
- **A phase that misses its gate** does not start the next until the miss is explained (v2, kept).

**The first five work items.**

| # | Item | Scope | Acceptance |
|---|---|---|---|
| **1** | **On-demand trigger and waste removal** | 8.1's workflow shape and 8.2's ruleset on today's generator; the macOS legs of `release-rust-ci-tools`, `product-release-binary`, `product-release-generator` and `qualification` off, with `SHA256SUMS` and `setup-velnor` no longer naming macOS assets; Source qualification, Job image candidate, State seal and prune re-triggered or retired; all gates on the cluster | An unlabelled push creates 0 runner jobs and cannot merge, in the lab and in production; ≤ 1.6 PR runs per merged PR; 0 hosted jobs in a week; ≤ 260 runner-min per merged PR |
| **2** | **Agent, cells, stores, generations** | XFS work directories with quotas; the verified tool image; the mirror; the fetch action; generations with harvest and A1; delta sync against `mbx-cache`, head to head | An unchanged tree compiles 0 crates with a view in ≤ 1 s; a one-file edit compiles only its crate and dependents; A1 is digest-equal on 50 consecutive landed commits, with any non-reproducible crate listed; a cold node is warm in ≤ 60 s |
| **3** | **Ledger, `ensure`, the one-job workflow** | Exact-tree and per-root keys; receipts; in-flight join; `velnor-ci ensure`; `velnor-gen` emitting caller and witness; `Velnor / Ledger`; the rename in `gate.rs` and the release workflows | No inventory difference from today's workflow over 100 shadow runs; a forged passing report mints 0 receipts; a rebase-only push gets its verdict in ≤ 20 s; a merge group on an equal tree executes 0 obligations |
| **4** | **Release promotion** | Release-artifact actions for x86-64 and cross-built arm64; qualification by digest; deterministic pack; attestation on `main`; the stamp after link; the promote job for the tools release, then the product release | A tag publishes in ≤ 60 s with 0 crates compiled; published digests equal the receipts' and A1's cold rebuild; `gh attestation verify` passes; a missing receipt fails the job |
| **5** | **Measurement, lab proofs and shadows** | Request-to-verdict and request-to-merge collectors. Four lab proofs of the gate: a skipped caller leaves `Velnor / Required` pending; a label set with `GITHUB_TOKEN` reaches the App and starts no run; a dispatched run's check satisfies the PR's required check; how fast a dispatched run appears. Webhook arrival; the queue-tip shadow; cold on two nodes against warm; closure enrolment; the paired-rerun sampler | A daily report that reproduces today's 85 min; each lab proof recorded with run ids; any proof that fails switches 8.1 to its stated fallback before item 1 reaches production |

Items 6–11: the executor and dispatcher in the lab; the harness; native actions; the test floor; per-package keys with speculation; test-level scheduling.

**Owner decisions this plan assumes.**
1. Two pre-merge executions of touched tests (2b row 20), or one.
2. `Velnor / Ledger` from the Velnor App as a second required check, which needs `checks: write` for the App and one public HTTPS endpoint per site.
3. ALLGREEN and 8 building entries from phase 2.
4. Whether an unlabelled push may create a run object with no job (8.1 as drawn), or must create nothing at all (the dispatch variant).
5. Whether Renovate labels its own PRs.
6. arm64 Linux assets cross-built on x86-64, or paused together with macOS.

## 12. Risks

| Risk | Detection | Containment |
|---|---|---|
| **A false green from an incomplete key** | A1 compares verdicts and digests with a cold build on every landed commit; A2 runs everything whatever the keys say; shadow enrolment; the read-miss log | The class switches off by itself and falls back one level (package → root → exact tree → always run); the policy epoch rises, so queued entries that relied on it are verified again; landed commits that relied on it are listed and re-audited; a red one gets a prepared revert |
| **A false green from the gate itself:** a skipped job counted as success, a PR that edits the workflow, a hosted `runs-on` | Two canary PRs, checked every 10 min: one unlabelled with auto-merge, one whose workflow is trivialised; both must stay blocked. An alarm on any push to `main` whose tree has no receipts | Two independent locks (the Actions job and the App's check); gate authority in the executor; the merge group verifies regardless of how a PR got in |
| **GitHub breaks the protocol** | The 10-minute canary; the drift sensor on every job message; response shape checks; the hourly release poll with surface diff and corpus inside 2 h | There is no official-runner fallback, by decision 1, so the honest containment is speed: the wire layer is 7k isolated lines with recorded fixtures, a fix goes through the lab corpus and ships in hours, and meanwhile PRs stay pending and nothing merges unverified. A change that ships with a runner release gives up to 63 days. velnor-legacy recorded no server-side break in 4.5 months [P §E.6] |
| **The executor departs from GitHub's semantics** | The harness blocks every executor change; nightly; twins before each slice; mutants prove the harness | The release is blocked; the previous Velnor release is one switch away, drilled weekly; the generated subset keeps the surface small; an unknown feature is refused, never approximated |
| **A stale generation hides a failing build** | A1's cold digest comparison, minutes after landing | The lineage is retired; the root builds cold; commits since the last verified generation are audited again |
| **A release publishes wrong or unqualified bytes** | Re-hash against the receipt; `gh attestation verify`; A1's reproducible rebuild | The job fails closed with the key named; it never rebuilds silently |
| **Time bombs and randomness behind a receipt** | The +8 and +30 day lane; the 7-day expiry; A2 nightly; disagreement tracking per key | The key becomes non-reusable |
| **False failures grow** because touched tests run twice | A disagreement between the two executions is itself the flake signal, recorded per test; the paired-rerun sampler | A budget of 1% false-red verdicts; whole cores and sized timeouts; quarantine only under an expiring owner exception (v2 §1.3) |
| **Ingress, ledger, dispatcher or a site is down** | Health checks; the canary; the delivery backlog | Channel A still creates jobs, and the job's own `ensure` does the work; the standby dispatcher resumes in 30 s; the other site takes requests; with no ledger nothing is satisfied by lookup, so work runs fresh: slow, never wrong |
| **The label is forgotten** and finished PRs sit pending | Time from ready-for-review to label; PRs with auto-merge and no label | The agents' one-command rule; a daily list. No automatic labelling, by the owner's rule |
| **The new code grows like velnor-legacy** (804k lines [P]) | Line budgets checked in CI, 62k in all | Work stops at a budget; refusal over generality; nothing is copied without its tests and an oracle pass |
| **Tenants and batch work against foreground work** | Eligible-to-started P99; wait per organisation | Exclusive cpusets; quotas; batch at `cpu.idle`, killed first |
| **A cross-built arm64 binary misbehaves on real hardware** | The `qemu-aarch64` smoke run; a native run when ARM or Mac hardware returns | arm64 assets are held back on any qualification failure and marked as cross-built |

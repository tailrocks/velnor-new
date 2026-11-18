# Velnor runner research: decision log

**Method.** Eight models answered the same brief with the same [evidence pack](evidence.md), each
alone; each then critiqued three anonymised proposals. Seven critiques are complete; Muse Wonder
Full's stopped before its cross-cutting section and is not used. This log takes all 120 ideas from
the panel's idea inventory, keeps their ids, merges duplicates, and decides each one on evidence
rather than on how many models proposed it. Where a decision rests on a contested fact, the fact was
re-checked in code (velnor-new at `95de9b704b`, actions/runner v2.338.0 at `7afce9e`, ARC,
velnor-legacy) or with read-only GitHub queries on 2026-11-19; those checks are listed under
"Measurements added in synthesis". Midway, the owner asked for a more ambitious design. That request
added the ideas `N01`–`N17` and the moonshots in the
[architecture](../../proposed/velnor-runner-architecture.md); it changed no confidence rule. All
eight models then critiqued the draft design. The design review's comparison verified 44 holes
(`F-H01`–`F-H44`) and merged 24 bolder alternatives (`F-B01`–`F-B24`); their fixes were applied in
eight groups, one per credited model, and are recorded under "Design review". Ideas adopted there
take ids from `N18`. Proposals and reviews are in `proposals/` and `reviews/`, named by model.
"Committed" and "stretch" have the meaning the architecture gives them.

## Measurements added in synthesis

| Id | What | Result | How |
|---|---|---|---|
| S1 | Push runs on a SHA whose merge-group run passed | 10 of 124 failed (8.1%), every failure in a test step: runner-host or its local-mode variant in 5 runs, `velnor-ci` in 2, actions-cli in 2, maintenance-helpers in 1, velnor-local in 1 (overlapping) | CI runs 2026-11-04..19 joined by head SHA; failed jobs and steps from the jobs API |
| S2 | Auto-merge and queue exits, 40 most recently merged PRs | 0 of 40 enabled auto-merge; queue exits: 40 merged, 3 failed checks, 3 manual, 1 merge conflict; 6 PRs entered the queue more than once | GraphQL timeline events |
| S3 | Ruleset 24396608 | Requires `Required verdict` and `Required` with no `integration_id`; merge queue HEADGREEN, build 2, merge 1–10, wait 5 min, timeout 90 min | `gh api repos/tailrocks/velnor-new/rulesets/24396608` |
| S4 | Does a merge group test the tree its PR run tested? | 0 of 40 merge groups on 11-18 had the base the PR's last green run had merged with; 11 of 60 (18%) on 11-16 and 11-17, close to the pack's 7 of 45 head-tree matches (§A T7) | Base from the PR run's checkout log (`Merge <head> into <base>`) against the merge-group commit's parent |
| S5 | Plan and visibility | Org plan `free`; repository public | `gh api orgs/tailrocks`; `gh repo view` |
| S6 | Worker and Listener seam | The Worker reads `.runner` and the job message (`Runner.Worker/JobRunner.cs:47`), waits 30 s for the message (`Worker.cs:23`), takes it as a 4-byte type plus a length-prefixed UTF-16LE body (`Runner.Common/ProcessChannel.cs:70-81`, `StreamString.cs:18`). Renewal and force-fail use the job's own token (`Runner.Listener/JobDispatcher.cs:734-746,1208-1219,1244-1249`). Only `Runner.Listener` uses the RSA key manager | code read |
| S7 | Cache-miss mechanism | Lookup filters on `version` (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`); the restore path is `${{ runner.temp }}` (`.github/workflows/ci.yml:337`) | code read |
| S8 | Test floors and caps | 40-case loop (`crates/velnor-actions-cli/tests/image_publication_public.rs:85-92`); 6-case loop (`.../unbound_retirement/tests/shape.rs:97-106`); `--config build.jobs=2` eight times in `mise.toml:90-193`; twelve archive-guard goldens serialised (`.config/nextest.toml:17-19`); `ci-helper` tests on one thread with a 2 s kill (`ci-helper/.config/nextest.toml`) | code read |
| S9 | The generator's cache choice | `velnor-gen`'s expected velnor-new workflow caches `target/` with plain cargo and `velnor-ci inputs restore`, has no mbx step, emits only `Required`, and runs its steps in sequence; its tests report an mbx cargo wrapper and a `rustc-wrapper` as weakenings (`generator/crates/velnor-gen/tests/it/weaken.rs:84-93`) | code read; issue #585 results comment |
| S11 | mbx against plain cargo on the owner's Mac (owner-run) | Disk per extra checkout: APFS mbx 473 MiB, clone 457 MiB; HFS+ (no clones, like ext4) mbx 835 MiB, full copy 1,411 MiB (mbx 41% smaller). Both rebuilt the same 6 workspace crates at every new checkout path. A second node pulling from an `mbx-cache` server: 164 hits, 6 misses, 503.7 MiB downloaded. mbx stored about 230 MiB of never-reused workspace outputs per checkout; the owner's own store is about 485 GiB | `ci-helper` with its `generator` path dependencies, six checkouts, mbx 1.29.2, `CARGO_INCREMENTAL=0`; one workspace, one commit, no lockfile change |
| S10 | Disk layout | Root is ext4 on the OS disk (`velnor-runners/PLAN.md:45`); one linear XFS LV at `/var/lib/rancher` (`PLAN.md:72-80`); Rust not baked into the image (`velnor-runners/state/RESULT.md:115`) | doc read |

## Decisions by idea

Ids `I001`–`I120` come from the idea inventory; `N01`–`N17` were added in synthesis and `N18` onward
in the design review. "Same as" marks a merged duplicate.

### Runner and executor

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I001 | Start with GitHub's unmodified official runner as executor and keep it as the permanent fallback. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | The official `Runner.Worker` executes every job: under ARC in phases 0–2, under `velnor-dispatch` from phase 3. Interpreter cost is 87–147 ms per action (Claude Fable 5.2, job logs) and no native-versus-official measurement exists (evidence §E.7). |
| I002 | Never build a native step executor; the official runner stays the only executor. | Gemini 4 Argon | Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | Rejected | A ban is the wrong instrument. The native executor stays possible behind the same dispatcher and must pass a measured gate (I005, I007). |
| I003 | Replace ARC with a Velnor-owned Rust scale-set controller and dispatcher. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra | Experiment-gated (phase 4) | Behind the witness (N18; F-B11): an idle ARC runner already starts the next job in 2 s p50, 3 s p90 (Claude Fable 5.2 review, F4), and `ensure` moves placement, ordering and fork refusal into Velnor's scheduler without owning acquisition. Built only if the witness leaves more than 5 s per run. |
| I004 | Run GitHub's official Runner.Worker under a Velnor-owned listener that feeds it the job message. | Claude Opus 5.5, Claude Fable 5.2 | Gemini 4 Argon | Experiment-gated (phase 4) | Still verified feasible (S6), but only the gated dispatcher (I003) needs it. |
| I005 | Native Rust executor only for the restricted velnor-gen job profile, official runner as fallback. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full | Gemini 4 Argon | Experiment-gated (phase 5) | For arbitrary jobs only. Generated obligations already run on Velnor's own executor inside `ensure` (N18; F-B12 folded in), so this gate concerns replacing the Worker for everything else. |
| I006 | General native Rust worker that runs unmodified node24 actions in the sandbox; default engine in the final phase. | Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Gemini 4 Argon | Rejected | Its author measured 2.5–4.5 s per job. It carries the widest compatibility surface, which velnor-legacy never closed (804k lines, no accepted speedup, evidence §E.6–7). |
| I007 | Adopt a native executor only after shadow or differential runs and a pre-agreed measured threshold. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | — | Adopted | The gate for I005: 100 paired jobs per root, zero semantic differences, at least 15 s per run or 10% at p50, and p90 no worse. |
| I008 | Automatic fallback from the native lane to the official executor. | Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Experiment-gated (phase 4) | Applies only if the dispatcher is built: the executor choice is then made after reading the job and before the first step, so no lease moves between runners. |
| I009 | Crash-only node agent with no journal: kill the cgroup subtree, fail lost jobs, re-register. | Claude Fable 5.2 | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Rejected | After a crash, `completejob` needs the plan id, job id, run-service URL and job token (`RunServiceHttpClient.cs:123-160`), which exist only in the job message (Gemini 4 Carbon review). Replaced by small lease records (N10). |
| I010 | Keep velnor-new's journal-first rule (intent, call, record) and ownership proofs, mapped to Pod UIDs. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Fable 5.2, Gemini 4 Carbon | Adopted | The discipline, not the code: intent, then call, then record; absence proven by UID; unknown never counts as absent. |
| I011 | Evolve velnor-runner-host behind a Runtime trait with a Kubernetes runtime (PR #1324). | GPT-6.2-Astra, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Gemini 4 Carbon | Owner decision | Recommend no for the cluster. `velnor-runner-host` is 157k lines with about 60 open correctness issues (evidence §D.10), its tests set the CI critical path, and its unit puts the JIT config, including the runner key, in the job container. The cluster runtime is new code that reuses the wire types and the discipline. |
| I012 | Set line budgets for the new runner before any code is written. | Claude Fable 5.2, Muse Wonder Full | — | Adopted | Budgets: proto 10k, dispatch 12k, node 15k, sandbox-init 1.5k, ledger 6k, store 6k lines; crossing one stops work for review. velnor-legacy reached 804k lines with two 37k-line files (evidence §E.1). |
| I013 | Run Plan and Required verdict as controller-native computations, not as jobs. | Muse Wonder Full | — | Rejected | Superseded by the witness (N18): the official runner's job named `Required` reports the verdict, so no second verdict writer is needed. |
| I014 | Reserved capacity or a dedicated class for gate jobs (Plan, verdict, Required). | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | — | Adopted (phase 1) | On 11-18 the three serial gate waits took 57.0 of a 71.8 min run (evidence §A T5). |
| I015 | Advertise scale-set capacity equal to free slots so excess jobs wait at GitHub. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted-later (phase 3) | Excess demand waits at GitHub instead of in a Velnor queue, and the dispatcher chooses which offers to acquire first. |
| I016 | Take legacy's expression engine with its tests; do not import legacy's 37k-line executor or runner files. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | — | Adopted-later (phase 5) | As the conformance corpus for I005 only. No legacy executor code is ported. |
| I017 | Conformance harness (velnor-local) and a scheduled canary job through each executor path. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 3) | One canary job through each executor path every 10 min, plus the velnor-local scenarios before each release. |

### Protocol

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I018 | Classify cache group and trust pool from the scale-set JobAvailable message at offer time. | Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Astra | Rejected | For placement: an offer has no binding to a runner (`generatejitconfig` takes `{name, workFolder}`, evidence §F.1.3). Its `eventName` is still used to acquire merge-group jobs first. |
| I019 | Pre-seed ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE with the pinned action tarballs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Removes the action download, about 4.4 s per job (Claude Opus 5.5, measured). Resolution still runs (`ActionManager.cs:1232`). |
| I020 | Serve the Actions cache (and artifact) protocol from an in-cluster service through the standard env vars. | Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Sol | Experiment-gated (phase 5) | velnor-new's workflows reach node stores through `velnor-ci` and need no emulation. For other orgs' unmodified workflows only a dispatcher rewrite of the job message works; environment injection is overwritten (`NodeScriptActionHandler.cs:55-74`). |
| I021 | Use the official runner's job-started hook to bind trust scope, mount caches and refuse unauthorised jobs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Argon | — | Adopted (phases 1–2) | `velnor-job-start-hook` refuses unauthorised jobs under ARC. From phase 3 the dispatcher classifies the job before any pod sees it (N11). |
| I022 | Send renewjob in parallel with job setup instead of blocking before the worker starts. | Gemini 4 Carbon | — | Rejected | The Listener starts no Worker before the first renewal succeeds (`JobDispatcher.cs:393-405`). One round trip is worth that guarantee. |
| I023 | Copy GITHUB_EVENT_PATH before background steps to avoid an event.json truncation race. | Gemini 4 Argon | — | Adopted (phase 0) | Verified race: each action step rewrites `event.json` (`ActionRunner.cs:140`, `ExecutionContext.cs:1322-1336`) while a background step parses it (`task_producer.rs:99-106`). One log shows `producer_event_invalid` (Claude Fable 5.2 review). |

### Cache

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I024 | Read-only node store or image seed for the Rust toolchain and mise tools. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Tool setup is 10 s median and 18 s p90 per job, 706 runner-min in the sample (evidence §A T9b). Rust is not in the image (`velnor-runners/state/RESULT.md:115`). |
| I025 | Node-local bare git mirror for checkout. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | A node mirror kept current by push webhooks; unmodified `actions/checkout` borrows its objects through alternates. |
| I026 | One shared mbx object store per node per trust scope, used by concurrent jobs. | Claude Opus 5.5, Claude Fable 5.2 | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Experiment-gated (phase 0) | Part of cache candidate (a) in the head-to-head (N22); the per-slot lesson of velnor-legacy's mbx store (`mbx_store.rs:6-13`) is one of the layouts it measures. |
| I027 | Per-slot MBX_CACHE_DIR and target directories (legacy layout) to avoid mbx's registrar lock. | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2 | Experiment-gated (phase 0) | Part of cache candidate (a) (N22). |
| I028 | Three tiers: node-local NVMe first, a same-site service second, GitHub cache only as hosted fallback. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | L1 node, L2 site, GitHub cache only on hosted runners. Nothing synchronous crosses the 35 ms DE–FI link (evidence §B.5). |
| I029 | Retire state-seal and mbx export/import bundles for cluster jobs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Muse Wonder | Adopted (phase 2) | The GitHub-cache bundles restored 0 of 3,636 times (evidence §A T9b), and their compat key hashes paths and `ImageOS` (Claude Opus 5.5 review, F1). A node-persistent cache replaces them, whichever engine wins N22. |
| I030 | A dedicated trusted builder or publisher, not CI jobs, writes the trusted cache on each main advance. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 2) | The node warmer builds every main advance on every node and is the only writer of the trusted scope. Same as I044. |
| I031 | Let merge_group runs write the trusted cache scope. | Claude Fable 5.2, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Sol, Muse Wonder, Gemini 4 Carbon | Rejected | Unneeded once the warmer builds main, and a merge group may never land. |
| I032 | PR-scoped writable cache read only by that PR and dropped when it closes. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 3) | Narrowed (F-H16): the PR scope serves that PR's later pushes only. A request whose receipts can become `trusted` builds from a `trusted` generation plus its own delta, because cargo never cleans `OUT_DIR` and an earlier push's output could let a tree pass that no clean build accepts. |
| I033 | Keep the visible non-blocking Cache check: red when a unit that should have hit compiles. | Claude Fable 5.2, GPT-6.2-Astra, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | `docs/goals.md` design rule 3; velnor-gen already emits the `Cache` job. |
| I034 | Sampled re-verification on every run (MBX_VERIFY compiles, re-executed reused verdicts). | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | — | Adopted (phase 3) | The audit sampler re-executes 2% of exact-tree receipts and 10% of closure receipts; one divergence disables that reuse class until explained. |
| I035 | XFS reflink copy-on-write clones for per-job workspaces and target views. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Experiment-gated (phase 0) | Cache candidate (b) in the head-to-head (N22). |
| I036 | Cache whole target/ snapshots per scope and restore them by reflink. | Gemini 4 Carbon, Gemini 4 Argon | Muse Wonder | Experiment-gated (phase 0) | Candidate (b) (N22). Issue #585 does not decide it: a 5-crate lab on hosted runners, mbx without its store between ephemeral runs. On the Mac both engines rebuilt the same 6 workspace crates at a new path (decisions (d)). |
| I037 | Job-level verdict reuse keyed by the hash of every file the job read. | Claude Opus 5.5 | GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon | Rejected | As a verdict key: a read trace misses negative lookups and cannot speak for branches that did not run (GPT-6.2-Astra). Kept as an audit of declared inputs (I069). |
| I038 | Verdict cache per test executable with sandbox-enforced inputs, written by the agent. | Claude Fable 5.2 | Muse Wonder, Gemini 4 Carbon | Experiment-gated (phase 4) | Becomes N02: test binary digest plus enforced runtime inputs. Enforcement by mount namespace, not Landlock, which cannot restrict `stat` (Gemini 4 Carbon review). |
| I039 | Skip test binaries whose digest and environment class match a recorded pass, with an audit sample. | Muse Wonder Full | Muse Wonder, Gemini 4 Carbon | Experiment-gated (phase 4) | Same as I038. A digest alone misses runtime fixtures, such as the samples `golden.rs:19-24` reads. |
| I040 | Signed evidence receipts from a trusted verifier; test-verdict reuse starts at 0%. | GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Velnor services sign receipts; a job never does. Reuse starts with exact trees only. |
| I041 | Never cache test outcomes across different trees; cache the binaries and always execute the tests. | Muse Wonder, Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | Adopted (phases 1–3) | The default until N02's experiment passes; afterwards only enforced-hermetic obligations cross trees. |
| I042 | Dispatcher scores nodes by cache warmth, load and site. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted-later (phase 3) | Objective: least predicted finish time (queue, start, delta build, tests), so an idle cold node beats a busy warm one. |
| I043 | Placement through Kubernetes: cache-digest node labels, preferred node affinity, hashing fallback. | Muse Wonder, Muse Wonder Full | — | Rejected | Affinity acts when a pod is created, not when GitHub assigns a job. |
| I044 | Pre-warm every node from main after each merge. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 2) | Same as I030. Each node rebuilds main incrementally on every advance; the site store holds a copy for new nodes. |
| I045 | Serve run-local artifacts (plan, reports, test archives) node-locally instead of GitHub round trips. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | — | Adopted-later (phase 4) | Test archives and reports move through the site store. The velnor-new nextest archive is 1.48 GB and took 36–43 s to upload to GitHub (#585). |
| I046 | Local mirrors: wire in the crates mirror, add a registry pull-through and image pre-pull. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Crates mirror through `CARGO_REGISTRIES_CRATES_IO_*` variables, which survive a custom `CARGO_HOME`. `cargo fetch` drops from 3.5–6.1 s to 2.7–3.3 s (evidence §B.4.2); images are already local (0 real pulls of 1,424). |
| I047 | Fix hosted-versus-scale-set path drift (work against _work) and drop RUNNER_TEMP and ImageOS from cache keys. | GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | Verified: lookup filters on `version` (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`) and the path is `${{ runner.temp }}` (`ci.yml:337`), which differs between hosted and ARC runners. Fixed absolute paths until GitHub cache leaves the cluster path. |
| I048 | Jobs never get a writable host cache directory; a cache service with scoped credentials mediates writes. | GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Jobs never write a shared store. The node agent harvests outputs after the build phase. |
| I049 | Drop the head-equals-tip seal check: objects are valid by content and verified commit. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Moot once the warmer replaces the seal: content keys need no tip check. |
| I050 | Isolated fetcher for private dependencies; derived outputs never readable by forks. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted | `docs/goals.md:43`. velnor-new has no private dependencies today, so this is a store rule, not work. |

### Workflow

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I051 | Move the eight hosted gate and lint jobs onto the cluster (velnor-actions v0.1.81, PR #1332). | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | 57.0 of 71.8 min on 11-18 was hosted gate queue (evidence §A T5); cluster jobs queue 0.2 min (§A T4). |
| I052 | Narrow the triggers of Source qualification and Job image candidate, or move them to the cluster. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | Job image candidate ran 306 times and Source qualification 267 times on 11-18, all hosted (evidence §A T11), on an org with the Free plan's 20 concurrent hosted jobs (`gh api orgs/tailrocks`: `free`). |
| I053 | Skip the full CI run on push to main when merge_group passed the same SHA or tree. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | 120 of 159 green merge groups had a push run on the same SHA (evidence §A T7), and 10 of 124 such push runs failed on an already-green SHA (synthesis measurement). Release and publication duties stay on push. |
| I054 | Affected-only selection on merge_group runs. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Owner decision | Recommend yes in phase 3. Hard decision 3 keeps merge groups full until in-job read verification exists; I069 plus a shadow-parity soak supplies it, and `docs/goals.md:86` makes selection the generator's default. |
| I055 | Tree-equal fast-pass: merge_group reuses the PR verdict when the trees match. | Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra, GPT-6.2-Sol | Adopted (phase 3) | As exact-tree receipts (N01). Today only 0 of 40 (worst day) to 11 of 60 (quieter days) merge groups tested the tree their PR run had tested (S4), so this saves little until PR runs are short; the evidence pack's 7 of 45 agrees. |
| I056 | Cut velnor-new over to velnor-gen: one job per Cargo root. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | With parity: the local-mode configuration, the maintenance checks, both required check names or a ruleset change, and parallel lanes inside a root job (the generated workflow runs its steps one after another today). |
| I057 | Collapse to one root-workspace pipeline (plan-and-static, lint, build, test shards). | Gemini 4 Carbon, Gemini 4 Argon | — | Adopted-later (phase 2) | Follows the owner's root-workspace move. Package selection and 'check alone' steps stay inside the job: #585 found crates that pass workspace clippy and fail alone. |
| I058 | Fewer gate hops: lints folded into the plan job, Ledger CLI build replaced by a released helper. | Claude Opus 5.5, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1–2) | Ledger CLI heads the critical path in 8 of 18 runs (GPT-6.2-Sol); velnor-gen uses a released helper instead. |
| I059 | Remove build.jobs=2, --jobs 2 and compiler_process_budget=2 on the cluster. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Verified `--config build.jobs=2` at `mise.toml:90,108,128,137,148,157,170,193`. maintenance-helpers is the last job in 102 of 154 runs (Claude Opus 5.5, measured). |
| I060 | Dissolve maintenance-helpers into cached workspace jobs with declared inputs. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | 508 s job with no cache (evidence §A T10). Its archive-guard goldens are serialised on purpose (`.config/nextest.toml:17-19`) and move to their own lane. |
| I061 | Split looped tests into one test per case. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 1) | Verified loops over 40 cases (`image_publication_public.rs:85-92`, 258–274 s) and 6 cases (`shape.rs:97-106`, 203–271 s). No shard count shortens one test. |
| I062 | Fix the slow suites at the root: journal fixes #1333 and #1349, migration squash or template, injected sleeps. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | The unit of cost is `Journal::open` (93 call sites in `velnor-runner-host`): the phase-1 gate is a pre-migrated template that opens in at most 1 s, measured (F-H33, F-B17). |
| I063 | Deduplicate the runner-host default and local-mode test sweeps. | Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra, GPT-6.2-Sol | Rejected | `local-mode` gates production code and the release build refuses the feature, so one sweep leaves a configuration untested (GPT-6.2-Astra and GPT-6.2-Sol reviews). |
| I064 | Build runner-workspace dependencies at opt-level 3 in the test profile. | Claude Fable 5.2 | GPT-6.2-Astra, GPT-6.2-Sol | Owner decision | Recommend an experiment: −66% CPU measured on the slowest test class (Claude Fable 5.2). Dependencies only; debug assertions and overflow checks stay on. |
| I065 | Enforce a test time budget in CI: no test over 10 s, suite critical path at most 60 s. | Claude Fable 5.2 | — | Adopted (phase 1) | 10 CPU-seconds per test measured alone, outside a recorded list that must shrink; non-blocking in phase 1, blocking for new tests from phase 2 (F-H33, F-B17). |
| I066 | Build test binaries once per configuration; shards consume a nextest archive, balanced by recorded durations. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Build once per configuration in phase 2; the cluster test executor (N04) consumes the binaries in phase 4. |
| I067 | Shard only when summed test time over threads exceeds the longest single test. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Distribute only when summed time ÷ slot threads exceeds the longest test plus transfer. #585 found no threshold up to 60 s on hosted runners because a second job cost 19–65 s. |
| I068 | Select by Cargo package graph and audited reads, not coarse globs. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | In the current engine (F-B16): declare the crate-root build scripts' inputs. Verified cause: `undeclared_reads` is true for every crate with a build script (`velnor-actions-rust/src/tasks.rs:254`), so heavy jobs ran in 97.4% of PR runs. A generator or helper PR then runs about 3.5 min instead of 10.9 [E]. |
| I069 | Record each job's file reads with fanotify to verify selection. | Claude Opus 5.5 | Claude Fable 5.2, GPT-6.2-Astra | Adopted-later (phase 3) | As an audit, not an authority: an undeclared read widens selection and fails the non-blocking `Selection` check. |
| I070 | Enforce declared inputs with a deny-by-default sandbox (Landlock), network off by default. | Claude Fable 5.2 | — | Experiment-gated (phase 4) | Via mount namespaces (N02); Landlock leaves `stat` and `access` open. |
| I071 | Scheduled full run as the audit of selection and caches. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | Nightly full run (velnor-gen default) plus a full run every 20th merge; both re-execute a sample of reused results. |
| I072 | Plan and gate run from released trusted code, never from the candidate. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Gemini 4 Argon | — | Adopted | `docs/goals.md` design rule 12. |
| I073 | Split velnor-runner-host (157k lines) into 6-10 crates. | Claude Opus 5.5 | — | Experiment-gated (phase 2) | A one-file edit rebuilds all 157k lines of `velnor-runner-host` (35–99 s, Claude Fable 5.2). Split only if a branch shows the incremental rebuild falling without more link work. |
| I074 | Agents debounce pushes by 20-30 s; cancelled jobs release their slots at once. | Muse Wonder Full | — | Adopted (phase 3) | In part: cancelled jobs release slots at once. Push debounce rejected: it delays every push, and cancellation already stops superseded runs. |

### Merge queue

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I075 | Raise max_entries_to_build from 2 to 5 or 6. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Adopted (phases 0 and 3) | 2 → 4 now; 4 → 8 once false failures are at most 2% per run. On 11-18 the queue merged about 1.7–1.9 PRs per hour against 7–8 enqueued (Claude Opus 5.5, Claude Fable 5.2). |
| I076 | Cut min_entries_to_merge_wait_minutes from 5 to 0 or 1. | Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Rejected | `min_entries_to_merge` is 1, so the wait never applies; enqueue → merge-group run measured 19 s (Claude Opus 5.5). |
| I077 | Agents enable auto-merge on every PR. | Claude Opus 5.5 | — | Adopted (phase 0) | 0 of 40 recent merged PRs (synthesis sample) and 0 of 100 (Claude Opus 5.5) used auto-merge; green → enqueue is 7.5 min median, 88 min p90. |
| I078 | Stop catch-up merges of main into PR branches (the '0 behind' rule). | Claude Opus 5.5 | — | Adopted (phase 0) | A catch-up merge restarts a full PR run for no confidence: the merge group tests the real candidate anyway. |
| I079 | Serve merge_group jobs before PR jobs. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phases 1 and 3) | By priority class under ARC, by acquisition order under the dispatcher. |
| I080 | Track merges per hour and queue depth as first-class metrics with alerts. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | — | Adopted (phase 0) | Queue residence and merges per hour are the numbers 11-18 needed. |

### Kubernetes

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I081 | Warm pool of pre-created pods or sandboxes. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1 and 3) | ARC `minRunners` per class first, pre-created single-use pods later. Idle runner 3 s against 14 s on demand (Claude Fable 5.2, measured). |
| I082 | No pod per job: a long-lived node agent creates sandboxes (namespaces, cgroups) directly. | Claude Fable 5.2, Gemini 4 Carbon | — | Rejected | For phase 3: pre-created pods take the 6 s pod start off the critical path and keep kubelet accounting, OOM handling and NetworkPolicy. MicroVM sandboxes stay experiment-gated (I086). |
| I083 | Long-lived slot pods scrubbed and reused between jobs. | Muse Wonder, Muse Wonder Full | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Rejected | A job can leave daemons, images and config behind; legacy #275 shows a poisoned persistent store (evidence §E.6). |
| I084 | Start dind only for jobs that declare Docker; separate pool or class. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | 57 dind startup-probe failures in one hour (evidence §B.6), and most jobs never use Docker. |
| I085 | Replace privileged dind with a Sysbox or Kata RuntimeClass. | Gemini 4 Argon | — | Experiment-gated (phase 5) | For the docker class, after the microVM measurement. |
| I086 | KVM microVMs restored from a snapshot, with dockerd running, for Docker and fork jobs. | Gemini 4 Carbon | Claude Opus 5.5, Muse Wonder, Muse Wonder Full | Experiment-gated (phase 5) | For the fork class and as the zero-start sandbox (moonshot M3). |
| I087 | Honest per-class CPU and memory requests and limits. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Pods request 2 CPU and used up to 83.7 cores (evidence §B.2.2); runner-host grew from 3.7 to 8.8 min under load (§A T6). |
| I088 | Static CPU manager with exclusive cores for build pods. | Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2 | Experiment-gated (phase 2) | One node first. Exclusive CPUs stop the bursting that cold builds need. |
| I089 | kubelet reservations, memory eviction threshold, pod PID limit and PriorityClasses. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | No kube or system reservation and no memory eviction threshold today (evidence §B.1.3). |
| I090 | Site labels on nodes; keep a run's jobs in one site. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1 and 3) | Labels first; run stickiness with the dispatcher. |
| I091 | Fork and unknown-trust PRs stay on GitHub-hosted runners until stronger isolation exists. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Adopted | Public repository. The dispatcher also refuses fork jobs on trusted pools (N11) until a microVM class exists. |
| I092 | Per-org quotas or fair shares for tailrocks, ChainArgos and jackin. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full | — | Adopted (phase 1) | Three orgs share 480 threads; ARC maxima add to 160 runners (evidence §B.3.2). |
| I093 | HA control plane: add k3s server nodes beside the single velnor-237. | Claude Opus 5.5, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted-later (phase 4) | Reliability, not speed: the single k3s server holds every pool. |
| I094 | Keep ARC installed as the rollback path; cut over by label on a separate scale set. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | — | Adopted | A separate label keeps rollback a one-line change for four weeks after each cutover. |

### Host tuning

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I095 | Move the kubelet root directory and job work dirs onto the XFS data volume. | Claude Opus 5.5, Claude Fable 5.2, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Root is ext4 on the OS disk (`velnor-runners/PLAN.md:45`) and the XFS LV is `/var/lib/rancher` (`PLAN.md:72-80`). Reflinks need job directories on the store's filesystem. |
| I096 | Dedicated XFS (reflink, noatime) cache volume per node. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | The existing XFS LV, after `xfs_info` confirms `reflink=1`. |
| I097 | Set the CPU governor or EPP to performance after checking the current value. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Experiment-gated (phase 0) | Not observable read-only (evidence §B.1.6); estimates are 2–10%. Read it, then decide. |
| I098 | OS memory knobs: THP madvise and lower dirty-writeback thresholds. | Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra, GPT-6.2-Sol | Experiment-gated | Credit 0 s until a stall is measured. |
| I099 | tmpfs for $RUNNER_TEMP or the job workdir. | Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | Rejected | Reflinks cannot cross into tmpfs, tmpfs is charged to pod memory, and the slow tests are CPU-bound with no block I/O (Claude Fable 5.2, measured). |
| I100 | Stripe the LVM data volume on the multi-disk workers. | Claude Opus 5.5, Gemini 4 Carbon, Gemini 4 Argon | — | Rejected | FI workers have one data disk each (`velnor-runners/state/inventory.md`); DE only if device saturation is measured. |
| I101 | Raise inotify, file-descriptor and PID limits; verify WireGuard MTU (DinD --mtu=1420, MSS clamp). | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Limits above measured peaks; WireGuard MTU checked end to end. |
| I102 | Measure host facts first (governor, NPS, THP, NIC speed, fio, iperf3) before tuning. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full | — | Adopted (phase 0) | An owner-run, read-only script records governor, SMT, NUMA, THP, NIC speed, `fio` and `iperf3` per node. |
| I103 | Keep SMT on, kernel 6.12 and flannel over WireGuard. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | No evidence for a change. |

### Build settings

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I106 | Set CARGO_BUILD_JOBS and nextest threads to the slot's CPUs. | Claude Opus 5.5, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Fable 5.2 | Adopted (phase 1) | Parallelism follows the slot's CPUs, not a constant. |
| I107 | Use the mold linker for CI builds. | Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Fable 5.2 | Experiment-gated | Rust ≥ 1.90 already links `x86_64-unknown-linux-gnu` with rust-lld; measure link time before adding mold. |
| I108 | Reduce debuginfo in CI: line tables for workspace crates, none for dependencies. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | velnor-gen defaults to line tables (`generator/crates/velnor-model/src/repo.rs:332`); #585: −10% entry size with line tables, −24% without third-party debuginfo. |
| I109 | Benchmark sccache as a mutually exclusive alternative to mbx. | GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full | Rejected | velnor-gen reports a `rustc-wrapper` as a weakening (`generator/crates/velnor-gen/tests/it/weaken.rs:89-93`). |

### mbx and blaze

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I110 | Keep mbx; move its store to a node-persistent path and add a site remote. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Gemini 4 Carbon | Experiment-gated (phase 0) | Owner direction: mbx stays unless it measurably loses the head-to-head (N22) as candidate (a), with a node-persistent store and an `mbx-cache` server per site. On the Mac it used 41% less disk than a copied target without reflinks, and a second node got 164 hits and 6 misses from the server. If it wins, `velnor-gen`'s rule that reports an mbx cargo wrapper as a weakening (`weaken.rs:84-88`) changes with it. |
| I111 | Retire mbx as the cache; plain cargo over reflinked target snapshots plus velnor-ci inputs restore. | Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Experiment-gated (phase 0) | Cache candidate (b) (N22). Its evidence, #585, came from a 5-crate lab and is not decisive at velnor-new's size. |
| I112 | blaze is ineligible for required checks: it builds with its own toolchain and ignores the 1.99.1 pin. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | blaze builds with its own nightly and ignores the 1.99.1 pin. |
| I113 | blaze as optional local pre-push feedback for agents. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Rejected | #652: 6.5–8.4× slower than cargo in CI and 8 of 8 restored-state runs failed. Preflight (N08) gives agents faster feedback on the real toolchain. |
| I114 | Remove mbx clean and cache export/import from jobs once stores persist. | Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Follows from I111. |
| I115 | Verify mbx's remote protocol and upgrades in a spike; guard the single-vendor risk. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder Full | — | Adopted (phase 0) | Inside the head-to-head (N22): the remote protocol worked on the Mac (164 hits, 6 misses, 503.7 MiB to a second node); the cluster run checks it at velnor-new's size. |

### Hardware

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I104 | Buy or rent high-clock nodes for the serial critical path. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | Owner decision | Recommend renting one high-clock node for a month: the serial path is one 157k-line crate and the longest test (estimates 1.3–1.5×). |
| I105 | Buy more EPYC workers. | Muse Wonder, Muse Wonder Full | Claude Opus 5.5, Claude Fable 5.2, Gemini 4 Carbon, Gemini 4 Argon | Rejected | For now: caches and dedupe cut runner-minutes per merge by about 80% (estimate). Buy when Velnor-side queue p90 exceeds 15 s after phase 3. |

### Process

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I116 | Every phase ends on a numeric gate measured on velnor-new; a miss stops the next phase. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | Each phase in the design ends on a gate measured on velnor-new. |
| I117 | Matched-cohort comparisons with minimum sample sizes; report CPU-seconds and GiB-minutes too. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full | — | Adopted | Every comparison also reports CPU-seconds and GiB-minutes. |
| I118 | Measure push-to-merge from PR timelines and instrument per-job spans. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 0) | The metric the brief names; the evidence pack does not measure it. |
| I119 | Track flake rate as failures on SHAs that already passed. | Claude Fable 5.2 | — | Adopted (phase 1) | Measured: 10 of 124 push runs failed on a SHA whose merge group passed (8.1%), all in test steps. A paired-rerun sampler replaces the push run as the denominator. |
| I120 | Prove changes on velnor-lab before velnor-new. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon | — | Adopted | `docs/goals.md` design rules 12 and 14. |

### Added in synthesis

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| N01 | Verification ledger: content-addressed obligations with signed receipts, keyed by exact tree, obligation definition, toolchain and image. | Synthesis; owner direction (content-addressed verdicts) | — | Adopted (phase 3) | Makes 'never verify the same input twice' structural. Exact-tree keys carry today's confidence by construction. |
| N02 | Hermetic closure receipts: reuse a test result across trees when the test binary digest and its enforced runtime inputs match. | Synthesis; from Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full (I037–I039) | GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon (as proposed there) | Experiment-gated (phase 4) | Answers the reviewers' objections: output-addressed (a tampered build changes the digest), enforced by mount namespace (an undeclared read fails), sampled by the audit. Moonshot M4. |
| N03 | Speculative merge candidates: verify `main ⊕ PR` as soon as main moves, before GitHub creates the merge group. | Synthesis; owner direction (speculative merge queue) | — | Rejected | Replaced by N19: GitHub builds a merge group on the queue entry ahead, not on `main` (F-H04, verified: 122 of 193 merge-group runs were built on an unlanded group commit). |
| N04 | Cluster test executor: one GitHub job fans its test cases out over many sandboxes in the site. | Synthesis; extends GPT-6.2-Astra (worker groups inside a job) | — | Experiment-gated (phase 4) | Test-granularity scheduling without a GitHub job per shard (19–65 s each on hosted, #585). Moonshot M6. |
| N05 | Hot Worker: a pre-started `Runner.Worker` waits on its pipe in every idle pod. | Synthesis | — | Experiment-gated (phase 4) | Only with the gated dispatcher (I003). First time a Worker started on arrival in a pre-created pod. |
| N06 | Receipt short-circuit: the dispatcher completes a job whose obligations all hold by receipt, or the plan job itself, without a sandbox. | Synthesis; owner direction (native executor that owns acquisition) | — | Experiment-gated (phase 4) | Only with the gated dispatcher (I003); the witness (N18) covers most of its use. |
| N07 | Preflight: agents ask Velnor to verify a tree before pushing; the PR run then finds receipts. | Synthesis; owner direction (CI before the push) | — | Experiment-gated (phase 3) | `velnor verify` calls the same `ensure` (N18). Only Velnor-executed results count; attested runs on a developer Mac are rejected for the verdict (another platform, a key the agent holds). |
| N08 | Agent workspaces on the cluster, sharing node stores, so the agents' own test runs become receipts. | Synthesis | — | Owner decision | Phase 5. Moves 'local first' (`docs/goals.md` rule 11) onto the cluster. |
| N09 | Push-webhook start: plan and build begin on the push webhook, before GitHub creates the jobs. | Synthesis | — | Adopted (phase 3) | Now the core of N18, behind its experiment (M1): kill if webhook arrival P50 is worse than the 8 s GitHub needs to create the first job. |
| N10 | Node-held job leases: the node agent renews the job lock and fails a lost job with the job token. | Synthesis | Claude Fable 5.2 (no journal) | Experiment-gated (phase 4) | A dispatcher feature (I003). |
| N11 | Velnor refuses fork and unauthorised work before any sandbox exists. | Synthesis; from Claude Opus 5.5 (trust class from the job message) | — | Adopted (phase 3) | `ensure` acts only on same-repository heads, so a fork's tree never reaches a sandbox; fork PRs keep hosted runners. The dispatcher form is gated with I003. |
| N12 | Bind the required checks to an integration in the ruleset. | Synthesis | — | Owner decision | Recommend now. Ruleset 24396608 names `Required verdict` and `Required` with no `integration_id`, so any app, or a workflow token with `statuses: write`, can post them. |
| N13 | Record the tested tree hash and obligation digest in every run. | Synthesis | — | Adopted (phase 1) | Receipts need it, and the tree-match rate that decides N03's payoff can only be measured with it. |
| N14 | Flake budget: paired reruns as the denominator, nextest flaky reporting, quarantine only with owner sign-off. | Synthesis; extends Claude Fable 5.2 (I119) | — | Adopted (phase 1) | Two full runs per merge at 8.1% false failure each fail about 15% of merges for no reason (1 − 0.919²). |
| N15 | PR-light, queue-full: PR pushes run only fast obligations; the merge queue runs everything the change selects. | Synthesis | — | Experiment-gated (phase 4) | Keeps confidence, because the merge group stays the gate; the risk is throughput, since each queue failure rebuilds every entry behind it. Adopt only if non-flaky merge-group failures stay ≤ 5% (3 of 40 recent queue exits were failed checks with full PR runs). |
| N16 | Coverage-based test impact selection: skip tests that executed none of the changed code. | Synthesis (considered) | — | Rejected | A test that ran none of the changed code can still change behaviour through layout, statics, timing or undefined behaviour, so the skip is not a proof. Closure receipts (N02) take the sound part of the gain. |
| N17 | Distributed compilation across nodes for cold builds. | Synthesis (considered) | — | Rejected | A cold runner-workspace build is 696 CPU-s with a serial chain of about 170 s (Claude Fable 5.2), so the chain, not CPU, bounds it; warm snapshots make cold builds rare (10.5% of PRs touch a lockfile). |
| N18 | `ensure(tree, obligations)` on the node agents, called by the push and merge-group webhooks; one witness job named `Required` calls the same `ensure`, streams the logs and exits with the verdict. | Claude Fable 5.2 (T1); variants from GPT-6.2-Sol, Gemini 4 Argon, Gemini 4 Carbon, Muse Wonder Full, Muse Wonder, GPT-6.2-Astra (F-B01) | — | Adopted (phase 3) | Removes two of three job hops and the plan job; median verdict about 35 s [E]. `docs/goals.md:16` holds literally, since the official runner runs the only job. Behind M1's experiment. |
| N19 | Speculate on the queue tip: ordered prefixes `T_i = T_{i−1} ⊕ PR_i` plus single-ejection bypass trees, verified at batch priority. | Claude Fable 5.2 (T2), GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon, Muse Wonder Full, Gemini 4 Argon, Muse Wonder (F-B05) | — | Adopted (phase 3) | Read-only shadow from phase 0; kill below a 70% tree match. GPT-6.2-Astra's replay matched 36 of 45 groups with one predecessor. A wrong guess only wastes preemptible CPU. |
| N20 | Every obligation whose inputs the change touched executes three times in parallel; any failure blocks. | Claude Fable 5.2 (H4); GPT-6.2-Sol, GPT-6.2-Astra (F-H35) | — | Adopted (phase 2) | Restores the detection today's three full runs give by accident: 1 − (1 − p)³ is 49% at p = 0.2 and 14% at p = 0.05, against 20% and 5% for one run. |
| N21 | Cold differential build: nightly and every-20th-merge runs build from empty in a fresh pod and compare results and test-binary digests with the warm path; lineages restart on lockfile or toolchain changes and daily. | Claude Fable 5.2 (H3); Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol (F-H15) | — | Adopted (phase 2) | Cargo cannot see a file it did not read in the last build; only a cold build finds that staleness. The helper already forces cold builds by age (`velnor-ci/src/cli.rs:73`). |
| N22 | Decide the compile cache by a head-to-head on the cluster: (a) mbx with a node-persistent store and an `mbx-cache` server per site, (b) plain cargo over reflinked snapshots, (c) plain cargo over overlayfs generations. | Claude Fable 5.2 (H12), Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon; owner direction (F-B14) | — | Adopted (phase 0) | Work item 1, running now. mbx stays unless it measurably loses. Mac measurement: equal disk with clones, mbx 41% smaller without them, the same 6 crates rebuilt at a new path by both. |
| N23 | Webhook ingress: one authenticated endpoint per site; missed deliveries replayed from the App's delivery list every minute. | Claude Fable 5.2 (missing 4) | — | Adopted (phase 3) | Nothing on the nodes accepts webhooks today (§B.4.3); the witness's own `ensure` covers any gap. |
| N24 | Break-glass: the hosted workflow stays as a permanent fallback, exercised weekly; the cluster runs only released agent versions. | Claude Fable 5.2 (missing 3) | — | Adopted (phase 2) | Without it, Velnor verifies Velnor's own code with no way back once ARC's per-job workflow is gone. |
| N25 | Seed the next generation and its receipts before an image, toolchain or helper bump lands. | Claude Fable 5.2 (missing 8) | — | Adopted (phase 2) | A bump changes every definition digest; otherwise the hour after it is cold for everyone. |
| N26 | C5 widened: randomness, commit identity, CPU count and environment; build and test sandboxes get no `.git` and a scrubbed environment. | GPT-6.2-Sol (2), Claude Fable 5.2 (H9), Muse Wonder, Muse Wonder Full (F-H09) | — | Adopted (phase 2) | Verified: `velnor-runner-host/build.rs:69-80` reads a lock from the Git object store, and the engine's task model already marks `uses_random` (`tasks.rs:70-72`). |
| N27 | Run every moonshot experiment and the cache head-to-head in week 1, beside phase 0. | Claude Fable 5.2 (T6), GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Argon (F-B22) | — | Adopted (phase 0) | 142 PRs merged through the queue in three days; the experiments touch no required path and should choose the architecture before anyone builds it. |


## Load-bearing decisions

### (a) What puts a job on a warm node

**Options.** One label or scale set per cache family, emitted by the generator; a Velnor-owned
listener that receives the job before choosing a node; a native executor that owns acquisition;
Kubernetes affinity to cache-digest labels; or classifying the offer message.

**Who argued what.** Claude Opus 5.5 and Claude Fable 5.2 put the official Worker under a Velnor
listener. Muse Wonder and Muse Wonder Full used affinity and slot pools; Gemini 4 Argon classified
offers and bound caches to standby pods. GPT-6.2-Astra and GPT-6.2-Sol placed by class only until a
binding was proven. The reviews showed that every placement design using the unmodified Listener
fails: `generatejitconfig` takes `{name, workFolder}` and GitHub hands an acquired job to whichever
idle runner polls (evidence §F.1.3); affinity acts when a pod is created, not when a job is
assigned; and a running pod's volumes and priority cannot change (GPT-6.2-Sol and Claude Fable 5.2
reviews). Claude Fable 5.2's review added the point that decides most of it: with five nodes, every
node can hold the `main` caches.

**Decision (after the design review).** Velnor stops placing jobs and places work. Phase 2 makes
placement mostly irrelevant: `main`'s compiled outputs are published after every merge, so any node
is warm for the `main` scope, and class labels (gate, build, docker) route workflow jobs under ARC.
From phase 3 the push webhook calls `ensure`, and Velnor's own scheduler puts each obligation on a
node; the one witness job only reports, so GitHub's choice of runner decides where a log is printed,
nothing else (N18). Owning acquisition through a dispatcher stays possible and experiment-gated
(I003).

**Deciding evidence.** An idle ARC runner already starts the next job 2 s after the previous one
ends (Claude Fable 5.2 review, F4), so a dispatcher buys a few seconds per hop for 23.5k lines;
`ensure` removes the hops instead. S6 still shows that a dispatcher could do the Listener's job
without moving any credential into a sandbox, if one is ever built.

### (b) The executor

**Options.** The official Worker only (Gemini 4 Argon; the default for GPT-6.2-Astra and
GPT-6.2-Sol); a native executor for the `velnor-gen` profile (Claude Fable 5.2, Muse Wonder, Muse
Wonder Full; optional for Claude Opus 5.5); a general native worker (Gemini 4 Carbon).

**"Admit too late."** GPT-6.2-Sol and Gemini 4 Carbon showed that a job cannot be handed to a
different runner after `acquirejob` (a second acquire gets 409, `RunServiceHttpClient.cs:106-107`).
That objection holds only when the fallback is another runner. When the dispatcher acquires and then
chooses the executor itself, before the first step, no lease moves.

**Decision (after the design review).** Two executors with one boundary. Generated obligations,
which are plain cargo and nextest commands, run on Velnor's own executor inside agent-created
sandboxes behind `ensure` (N18); that is the native executor for the generated profile, and it
reimplements no step semantics (F-B12 folded in). GitHub's `Runner.Worker` runs every workflow job,
the witness included, unmodified. The hot Worker (N05), podless completion (N06) and a Worker
replacement for arbitrary jobs (I005) are experiment-gated behind the dispatcher, which is itself
gated behind the witness.

**Deciding evidence.** Measured interpreter cost is 87–147 ms per action (Claude Fable 5.2 logs).
Estimated native gains range from 0.5 to 20 s per job, and no proposal measured one. velnor-legacy
built a native lane in 804k lines with no accepted speedup and archived it (evidence §E.6–7).

### (c) ARC

**Decision (after the design review).** Keep ARC, with its template fixed: class scale sets, warm
pools, honest sizes, no dind where Docker is not needed, node mounts. It runs the witness and every
other workflow job. Replacing it is experiment-gated behind the witness (I003): built only if the
witness leaves more than 5 s per run that a dispatcher would remove.

**Deciding evidence.** ARC is not slow today: its jobs queue 0.2 min median (evidence §A T4) and an
idle runner starts a job in 2–3 s (Claude Fable 5.2). What ARC blocked, per-job placement and
refusing forks before a pod exists, now happens in `ensure`, which places obligations and acts only
on same-repository heads. GPT-6.2-Astra's condition for replacing ARC, a measured need, is not met.

### (d) The cache

**mbx or target snapshots (after the design review).** Experiment-gated across three candidates, and
**mbx stays unless it measurably loses** (owner direction, N22): (a) mbx with a node-persistent
store and an `mbx-cache` server per site, pull requests reading and only `main` pushes writing; (b)
plain cargo over reflinked target snapshots; (c) plain cargo over overlayfs generations. The draft
chose (b) on issue #585, and the design review showed that evidence does not decide it: #585 ran a
5-crate lab on hosted runners, and its mbx rows lost because the store was not carried between
ephemeral runs. The owner's Mac measurement (S11) found equal disk with clones, 41% less for mbx
without them, the same 6 workspace crates rebuilt by both at a new path, and working cross-node hits
from the server. The cluster run, at fixed checkout paths with a lockfile change and a page-cache
check, decides between them.

**One store or per slot.** Whichever engine wins: one fixed checkout path for every request, views
or clones per request from immutable generations, a per-PR scope harvested by the node agent after
the build phase, a disk budget with eviction (the owner's own mbx store is about 485 GiB), and a
cold differential build that audits the warm path (N21). Toolchains, tools, crate sources, the Git
mirror and action archives are single read-only node stores.

**Trust.** The `main` builder is the only writer of the trusted scope. A PR scope is written by the
agent, never by the job, and read only by that PR's later pushes: a request whose receipts can
become trusted builds from a trusted generation plus its own delta (F-H16). Merge groups never write
the trusted scope (a merge group may never land); their receipts become trusted when their SHA
lands. Forks get nothing.

**The 48 of 48 miss.** S7 confirms the mechanism: the cache version is derived from the path, and
`${{ runner.temp }}` differs between hosted and ARC runners. Fixed absolute paths fix it while the
GitHub cache is still on the cluster's path (phase 0); node stores make it moot from phase 2. The
`ImageOS` and path fields in the state key break restores the same way, so fixing only the tip check
would still restore nothing (Claude Fable 5.2 review).

### (e) The merge queue

**Decision.**

- **Depth:** `max_entries_to_build` 2 → 4 now, → 8 in phase 3 once false failures are at most 2% per
  run. On the worst day the queue merged about 1.7–1.9 PRs per hour against 7–8 enqueued.
- **Wait setting:** unchanged; it never applies with a minimum group of 1 (enqueue to merge-group
  run measured 19 s).
- **Auto-merge:** agents enable it when they open a PR (S2: 0 of 40) and stop catch-up merges.
- **Third run:** the push to `main` stops re-verifying a SHA its merge group passed (phase 1) and
  keeps its release and publication duties; a direct push runs everything.
- **Reuse of PR verdicts:** exact-tree receipts (phase 3, committed mechanism). S4 shows why that
  alone is not enough: on the worst day no merge group tested the tree its PR run had tested,
  because `main` moved during hours of waiting, and on quieter days only 18% did. Shorter PR runs,
  merge-group selection by change (owner decision, recommended once the file-read audit and a
  two-week shadow show no miss), closure receipts and speculation carry the rest. Speculation
  targets the queue tip, not `main`: 122 of 193 merge-group runs were built on another group's
  unlanded commit (F-H04), so Velnor verifies the ordered prefixes and single-ejection bypass trees
  from phase 3, with a read-only shadow from phase 0 (N19).

**Deciding evidence.** The ruleset itself (S3); 120 of 159 green merge groups repeated on push
(evidence §A T7); S1, S2, S4.

### (f) The critical-path floor

**Decision (after the design review).** The floor is fixed in code, starting in phase 1, and the
unit of cost is `Journal::open`, not two loops: it has 93 call sites in `velnor-runner-host`, and
the logs show the longest test still at 125–138 s once the two loops are excluded (F-H33). The
phase-1 gate is a pre-migrated journal template that opens in at most 1 s, measured, plus a budget
of 10 CPU-seconds per test measured alone outside a recorded, shrinking list. The loop splits,
`maintenance-helpers` without `build.jobs=2`, and `ci-helper` tests in parallel come with it.
Committed: no test over 60 s after phase 2; stretch: 15 s. The 60 s verdict moonshot applies to the
median PR, which touches only `generator/` or `ci-helper/` and is not bound by those tests.

**Deciding evidence.** Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra and GPT-6.2-Sol each
measured single tests of 203–274 s setting their suites; the proposals that divided suite time by a
shard count (Muse Wonder, Muse Wonder Full, Gemini 4 Argon) were contradicted by those measurements
and by the code.

### (g) The velnor-new runner journal and velnor-legacy

**Decision.** The cluster runtime is new code: `velnor-proto`, `velnor-dispatch`, `velnor-node`,
`velnor-sandbox`, `velnor-ledger` and `velnor-store`, within line budgets. It reuses
`velnor-runner-github`'s wire types, `velnor-runner-core`'s lifecycle and ownership types, the
intent-call-record discipline, UID-based absence proofs, the job-authority policy and velnor-local
as its conformance harness. Recovery state is small: a dispatcher journal of acquisitions and sealed
lease records on each node, because renewal and force-fail need only the job token (S6). A
crash-only agent with no journal is rejected: after a crash `completejob` needs the plan id, job id,
run-service URL and token, which exist only in the job message. `velnor-runner-host` keeps serving
the Mac host and is not ported to the cluster (owner decision, against the open proposal to add a
Kubernetes runtime to it). From velnor-legacy: its broker and run-service client and error taxonomy,
scale-set client and trust-scope model as reference code, its incidents as regression tests, its
expression engine as a corpus; not its 37k-line executor and runner files.

**Deciding evidence.** `velnor-runner-host` is 157k lines with about 60 open correctness issues
(evidence §D.10), its tests set the CI critical path, and its unit puts the JIT config, with the
runner key, inside the job container. A Kubernetes runtime inside it would put the cluster's
controller in the slowest crate.

### (h) Flakes, the Free plan and the other organisations

**Flakes.** 8.1% of push runs on already-green SHAs failed (S1). With two full runs per merge, about
15% of merges hit a false failure (1 − 0.919²). Budget: at most 3% per full run after phase 1 and 1%
after phase 3, measured by a paired-rerun sampler on 5% of green merge-group SHAs. Known causes go
first: CPU starvation under 2-CPU requests with 84-core bursts, the 2 s test kill in `ci-helper`,
and the `event.json` race (I023). Retries stay at 0 in the required profile; quarantine needs the
owner's sign-off and keeps the test running visibly.

**The Free plan.** Confirmed `free` (S5), which the project's own measurements describe as 20
concurrent hosted jobs shared across the org. Phase 0 moves every merge-path job and the two heavy
auxiliary workflows off hosted runners; forks and macOS release builds stay there. A paid plan is
not needed for this design; it is an owner question.

**Other organisations.** ChainArgos and jackin share the five workers through their own ARC scale
sets (maxima 60 and 40 against tailrocks' 60). They get guaranteed quotas, separate namespaces and
stores, and the same priority as velnor-new's PR jobs; only Velnor's batch work (warmer,
speculation, scheduled runs) is preempted. They stay on ARC until they choose Velnor.

## Hypotheses: final verdicts

| Hypothesis | Verdict | Numbers |
|---|---|---|
| H1. Rust everywhere, including native actions, speeds CI dramatically because native code beats interpreted code | **Fails as stated.** The language is not where the time is. Locality is, and Rust is still the right language for the parts that remove work | Interpreter cost 87–147 ms per action [M, Claude Fable 5.2]; action steps 6–17 s per job, mostly network [M]; tool setup 10 s median [M, §A T9b]. Local mirrors, read-only tools and the action archive cache recover 14–18 s per job with the official Worker [E]. A native executor is estimated at 0.5–20 s per job and was never measured |
| H2. A from-scratch Rust runner beats wrapping the official runner and ARC | **Partly holds, in a different place than hypothesised.** The Rust that wins is the verification path around the runner: `ensure`, the executor for generated obligations, the ledger and the webhook ingress. Owning job acquisition and replacing the Worker stay unproven and experiment-gated | Job start 12–14 s under ARC on demand, 2–3 s idle [M]; the witness design removes two of three job hops and reaches a verdict about 35 s after the push for the median PR [E], with the official Worker running the only job |
| H3. Kubernetes is the right base | **Partly holds.** Right as the fleet layer; wrong as the job scheduler of record, and its defaults work against CI | Pod start about 6 s [M]; `emptyDir` on the ext4 OS disk [M, S10]; 2-CPU requests with 83.7-core bursts [M, §B.2.2]; runner-host 3.7 → 8.8 min under load [M, §A T6] |
| H4. Local caches beat remote ones, and a placement-aware built-in cache is a big win | **Holds for locality; the placement half is weaker than hypothesised; which local engine is open.** Every node can hold the `main` caches, so placement mostly decides PR scope and site, not whether work is warm | GitHub cache restore of 1.92 GB took 44–61 s, longer than the cold build it replaced [M, #585]; cross-job reuse on the cluster today 0 [M, §A T9a]; on the Mac mbx and a cloned target cost about the same disk with clones and mbx 41% less without, and both rebuilt the same 6 crates at a new path [M, S11]; the cluster head-to-head decides the engine (N22) |
| H5. Workflows can run only what a change needs | **Holds, and with the queue fixes it is the largest lever.** The biggest removal is not finer selection but never verifying identical inputs twice; selection must be proven, not assumed | Push re-runs the merge group's SHA in 120 of 159 merges [M, §A T7] and fails falsely 8.1% of the time [M, S1]; heavy jobs ran in 97.4% of PR runs because build-script crates are forced [M, Claude Fable 5.2; S8]; median PR verdict 2.5 min committed, 60 s stretch [T] |

## Design review

The eight design critiques and the comparison that verified them are the input here; ids are the
comparison's. Each row says how the final design answers the finding, and in which review group it
was applied. A rejected recommendation says why.

| Id | Finding | Group | Resolution |
|---|---|---|---|
| F-B01 | Run obligations outside GitHub jobs; one witness job reports | 1 | Adopted as the phase-3 core (N18), behind M1's experiment |
| F-B11 | Build the dispatcher earlier, or only if still needed | 1 | Experiment-gated behind the witness: built only for a residual over 5 s per run (I003) |
| F-B12 | A native executor for the generated profile, early | 1 | Folded into `ensure`: generated obligations run on Velnor's executor; a Worker replacement stays gated (I005) |
| F-H04, F-B05 | Speculation must target the queue tip, not `main` | 1 | Ordered prefixes and bypass trees, shadow from phase 0, kill below 70% (N19) |
| F-B14 | Decide the compile cache by experiment | 1 | Three candidates, mbx kept unless it loses; work item 1; Mac numbers in S11 (N22) |
| F-H16 | A gate run may link artifacts built by code not in the landed tree | 1 | Trusted-capable requests build from a trusted generation plus their delta; the PR scope serves the PR only (I032) |
| F-H15 | Compile reuse sits outside the contract | 1 | C6 now includes the cold differential build and lineage restarts (N21) |
| F-H35 | One execution instead of three lowers detection of new nondeterminism | 1 | Obligations whose inputs the change touched run three times in parallel (N20) |
| F-B16 | Fix selection in phase 1, in the engine | 1 | Adopted (I068) |
| F-H33, F-B17 | The test floor behind the two loops is 125–138 s | 1 | Gate on `Journal::open` ≤ 1 s and 10 CPU-s per test (I062, I065); more in later groups |
| F-H09 | The exact-tree key misses inputs an obligation can observe | 1 | C5 widened; no `.git` and a scrubbed environment in sandboxes (N26) |
| F-H37 | The −66% profile gain was measured without debug assertions | 1 | Owner question 8 restated with both measurements |
| F-H38 | Numbers tagged [M] that are estimates | 1 | Re-tagged in §6.6; `CARGO_INCREMENTAL` stated (off in required builds until measured) |
| F-B22 | Run the experiments in week 1 | 1 | Adopted (N27) |
| F-H42 | Phase 0's first item is merged but not live | 1 | Recorded in phase 0 and work item 2: the workflow must be regenerated |

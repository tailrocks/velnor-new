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
eight groups, one per credited model, a ninth group settled the compile cache on the cluster
measurement (S12), and a tenth applied the owner's binding decision for a Rust-only stack (N57–N64);
all are recorded under "Design review" and in (b), (c) and (g). Ideas adopted there take ids from
`N18`. Proposals and reviews are in `proposals/` and `reviews/`, named by model. "Committed" and
"stretch" have the meaning the architecture gives them.

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
| S8 | Test floors and caps | 40-case loop (`crates/velnor-actions-cli/tests/image_publication_public.rs:85-92`); 6-case loop (`.../unbound_retirement/tests/shape.rs:97-106`); `--config build.jobs=2` eight times in `mise.toml:90-193`; twelve archive-guard goldens serialised (`.config/nextest.toml:17-19`); `ci-helper` tests on one thread (`ci-helper/.config/nextest.toml`), whose 2 s kill is in the default profile, not the CI profile (design review F-H36) | code read |
| S9 | The generator's cache choice | `velnor-gen`'s expected velnor-new workflow caches `target/` with plain cargo and `velnor-ci inputs restore`, has no mbx step, emits only `Required`, and runs its steps in sequence; its tests report an mbx cargo wrapper and a `rustc-wrapper` as weakenings (`generator/crates/velnor-gen/tests/it/weaken.rs:84-93`) | code read; issue #585 results comment |
| S11 | mbx against plain cargo on the owner's Mac (owner-run) | Disk per extra checkout: APFS mbx 473 MiB, clone 457 MiB; HFS+ (no clones, like ext4) mbx 835 MiB, full copy 1,411 MiB (mbx 41% smaller). Both rebuilt the same 6 workspace crates at every new checkout path. A second node pulling from an `mbx-cache` server: 164 hits, 6 misses, 503.7 MiB downloaded. mbx stored about 230 MiB of never-reused workspace outputs per checkout; the owner's own store is about 485 GiB | `ci-helper` with its `generator` path dependencies, six checkouts, mbx 1.29.2, `CARGO_INCREMENTAL=0`; one workspace, one commit, no lockfile change ([report](experiments/mbx-cache-mac.md)) |
| S12 | mbx against plain cargo over a reflinked warm target on two workers, in a throwaway namespace on 2026-11-18 | Plain cargo cold 92 s (357 crates); over a reflink clone of the warm target 0.25 s and 2.5 MB for the clone, 0.31 s and 0 crates for the build; today's full copy onto the ext4 `emptyDir` 8.6 s and 10.75 GiB. mbx cold 93 s, uploading 8.7 GiB; same node and path 0.32 s through its path-keyed managed target, 7.2 s and +1.94 GiB without it; second node with an empty store and a warm server 39.9 s, 496 hits, 0 misses, 8.7 GiB at about 215 MB/s. Lockfile change (11 crates): plain 67.4 s and +8.7 GiB; mbx 88.4 s and +15.5 GiB on the same node, uploading 6.8 GiB; 43.4 s on the second node. Clones held their own page cache (14.29 GB each when read). The local-path volume is XFS with reflink; `emptyDir` is ext4 without. mbx needed a loopback forwarder (no plain-HTTP remote with a token) and `MBX_REMOTE_TRUST_WRITES=true`; its store grew 8.8 → 15.6 GiB with garbage collection off | velnor-190 and velnor-5, the runner workspace, `cargo build --locked --workspace --all-targets` at `/w/velnor-new`, 32 jobs, `CARGO_INCREMENTAL=0`, rust 1.99.1, mbx 1.29.2, `mbx-cache` from source; nodes nearly idle; one commit ([report](experiments/mbx-cache-cluster.md)) |
| S10 | Disk layout | Root is ext4 on the OS disk (`velnor-runners/PLAN.md:45`); one linear XFS LV at `/var/lib/rancher` (`PLAN.md:72-80`); Rust not baked into the image (`velnor-runners/state/RESULT.md:115`) | doc read |

## Decisions by idea

Ids `I001`–`I120` come from the idea inventory; `N01`–`N17` were added in synthesis and `N18` onward
in the design review. "Same as" marks a merged duplicate.

### Runner and executor

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I001 | Start with GitHub's unmodified official runner as executor and keep it as the permanent fallback. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Rejected | As stated (official runner as the executor and a permanent fallback): decision 20 makes GitHub Official a supported executor the owner chooses deliberately per job type, and decision 19 its reference role; it is never an automatic fallback, and Velnor Native is the default. |
| I002 | Never build a native step executor; the official runner stays the only executor. | Gemini 4 Argon | Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | Rejected | The owner decided the opposite (N57): Velnor's native executor runs every job. |
| I003 | Replace ARC with a Velnor-owned Rust scale-set controller and dispatcher. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra | Adopted (phase 2) | By the owner's decision (N57): `velnor-dispatch` holds the scale-set sessions, the broker long-poll and every JIT runner identity, so it picks the node before it acquires a job. ARC serves velnor-new until the cutover (N64). |
| I004 | Run GitHub's official Runner.Worker under a Velnor-owned listener that feeds it the job message. | Claude Opus 5.5, Claude Fable 5.2 | Gemini 4 Argon | Rejected | No official Worker runs in production (N57). |
| I005 | Native Rust executor only for the restricted velnor-gen job profile, official runner as fallback. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full | Gemini 4 Argon | Adopted (phase 2) | Widened by the owner's decision (N57): the native executor runs velnor-new's full job set, not only the generated profile, with no official fallback. Claude Opus 5.5, Claude Fable 5.2, Muse Wonder and Muse Wonder Full proposed this native lane. |
| I006 | General native Rust worker that runs unmodified node24 actions in the sandbox; default engine in the final phase. | Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Gemini 4 Argon | Adopted (phase 2) | In part, by the owner's decision (N57): Gemini 4 Carbon's general native worker becomes the only executor, but without Node; the closed native action set replaces unmodified node24 actions (N62). |
| I007 | Adopt a native executor only after shadow or differential runs and a pre-agreed measured threshold. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | — | Adopted | As the conformance harness and the executor cutover gate (N58, N64): 100% of the lab corpus against the latest official release, and two weeks and at least 300 merges of twin-job shadow with every verdict difference explained. |
| I008 | Automatic fallback from the native lane to the official executor. | Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Rejected | No fallback to the official executor (N57); rollback is the previous Velnor release. |
| I009 | Crash-only node agent with no journal: kill the cgroup subtree, fail lost jobs, re-register. | Claude Fable 5.2 | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Rejected | After a crash, `completejob` needs the plan id, job id, run-service URL and job token (`RunServiceHttpClient.cs:123-160`), which exist only in the job message (Gemini 4 Carbon review). Replaced by small lease records (N10). |
| I010 | Keep velnor-new's journal-first rule (intent, call, record) and ownership proofs, mapped to Pod UIDs. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Fable 5.2, Gemini 4 Carbon | Adopted | Now with the code where it fits (N61): intent, then call, then record; absence proven by UID; unknown never counts as absent; ownership proofs, copied from velnor-new's runner. |
| I011 | Evolve velnor-runner-host behind a Runtime trait with a Kubernetes runtime (PR #1324). | GPT-6.2-Astra, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Gemini 4 Carbon | Rejected | Superseded by the owner's decision (N61): `crates/velnor-runner` wraps the official runner in Docker and is superseded as an executor; the cluster runs Velnor's native executor. |
| I012 | Set line budgets for the new runner before any code is written. | Claude Fable 5.2, Muse Wonder Full | — | Adopted | Budgets: dispatch 6k, exec 20k, native actions 10k, conformance 5k, hooks 3k, ledger 6k, node 15k and store 6k lines; crossing one stops work for review. velnor-legacy reached 804k lines with two 37k-line files (evidence §E.1). |
| I013 | Run Plan and Required verdict as controller-native computations, not as jobs. | Muse Wonder Full | — | Rejected | The release gate settles it (F-H20, verified): `velnor-ci release-gate` accepts a required check only from GitHub Actions (integration 15368) on a `push` run of the exact SHA (`gate.rs:15,127-159,277-288`), and `product-release.yml:86` asserts exactly one job named `Required` in that run. So `Required` stays a real job; Velnor's native executor runs it, and GitHub Actions still reports its check (N57). |
| I014 | Reserved capacity or a dedicated class for gate jobs (Plan, verdict, Required). | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | — | Adopted (phase 1) | On 11-18 the three serial gate waits took 57.0 of a 71.8 min run (evidence §A T5). |
| I015 | Advertise scale-set capacity equal to free slots so excess jobs wait at GitHub. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Rejected | Overturned (F-H23, verified): `X-ScaleSetMaxCapacity` is the scale set's total capacity; the reference listener sends the constant `MaxRunners` and velnor-new's own note says "total capacity, not free slots". A gated dispatcher advertises total capacity and keeps no idle pre-registered identities under backlog, after a lab test with capacity 2 and five queued jobs. |
| I016 | Take legacy's expression engine with its tests; do not import legacy's 37k-line executor or runner files. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 1) | By the owner's decision (N60): copied into velnor-new with its tests (3,565 lines, transcribed from actions/runner v2.337.0), reviewed, adapted and owned there, and admitted after it passes the oracle's expression corpus. Legacy's 37k-line executor and runner files are not copied whole. |
| I017 | Conformance harness (velnor-local) and a scheduled canary job through each executor path. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 1) | Expanded into `velnor-conformance` (N58): the latest official runner in the lab as the oracle; velnor-new's full job set plus semantics and refusal cases; normalisation; 100% pass with owner-signed allow-list entries; blocking on executor changes, nightly and within 24 h of each official release. velnor-new's `qualification.yml` is its seed. |

### Protocol

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I018 | Classify cache group and trust pool from the scale-set JobAvailable message at offer time. | Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Astra | Rejected | For placement: an offer has no binding to a runner (`generatejitconfig` takes `{name, workFolder}`, evidence §F.1.3). Its `eventName` is still used to acquire merge-group jobs first. |
| I019 | Pre-seed ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE with the pinned action tarballs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Until the executor cutover only: it removes the action download, about 4.4 s per job (Claude Opus 5.5, measured). After the cutover no action is downloaded (N62). |
| I020 | Serve the Actions cache (and artifact) protocol from an in-cluster service through the standard env vars. | Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Sol | Experiment-gated (phase 5) | Only for tools that speak the protocol themselves, such as BuildKit's `type=gha` cache: the native cache and artifact actions talk to cache service v2 and artifacts v4 and accelerate from the node and site stores by digest, so no JavaScript client needs emulation (N62). |
| I021 | Use the official runner's job-started hook to bind trust scope, mount caches and refuse unauthorised jobs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Argon | — | Adopted (phases 1–2) | `velnor-job-start-hook` refuses unauthorised jobs under ARC. From phase 3 the dispatcher classifies the job before any pod sees it (N11). |
| I022 | Send renewjob in parallel with job setup instead of blocking before the worker starts. | Gemini 4 Carbon | — | Rejected | The Listener starts no Worker before the first renewal succeeds (`JobDispatcher.cs:393-405`). One round trip is worth that guarantee. |
| I023 | Copy GITHUB_EVENT_PATH before background steps to avoid an event.json truncation race. | Gemini 4 Argon | — | Adopted (phase 0) | Verified race: each action step rewrites `event.json` (`ActionRunner.cs:140`, `ExecutionContext.cs:1322-1336`) while a background step parses it (`task_producer.rs:99-106`). One log shows `producer_event_invalid` (Claude Fable 5.2 review). |

### Cache

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I024 | Read-only node store or image seed for the Rust toolchain and mise tools. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Tool setup is 10 s median and 18 s p90 per job, 706 runner-min in the sample (evidence §A T9b). The toolchain is mounted read-only only with the workflow that expects it: today's workflow fails on a read-only `RUSTUP_HOME` (`ci.yml:427,438`, F-H32). |
| I025 | Node-local bare git mirror for checkout. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | A node mirror kept current by push webhooks; unmodified `actions/checkout` borrows its objects through alternates. |
| I026 | One shared mbx object store per node per trust scope, used by concurrent jobs. | Claude Opus 5.5, Claude Fable 5.2 | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Adopted (phase 1) | In changed form (S12): one node-persistent mbx store per node, used by the node's warmer and the builder, not by concurrent jobs. mbx's managed target is keyed on the checkout path, so concurrent jobs at the fixed path would share it; jobs run plain cargo over clones instead. |
| I027 | Per-slot MBX_CACHE_DIR and target directories (legacy layout) to avoid mbx's registrar lock. | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2 | Rejected | Moot (S12): jobs never run mbx, and each node's warmer runs one mbx at a time, so there is no registrar lock to avoid. |
| I028 | Three tiers: node-local NVMe first, a same-site service second, GitHub cache only as hosted fallback. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | L1 node, L2 site, GitHub cache only on hosted runners. Nothing synchronous crosses the 35 ms DE–FI link (evidence §B.5). |
| I029 | Retire state-seal and mbx export/import bundles for cluster jobs. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Muse Wonder | Adopted (phase 2) | The GitHub-cache bundles restored 0 of 3,636 times (evidence §A T9b), and their compat key hashes paths and `ImageOS` (Claude Opus 5.5 review, F1). Node snapshots replace them (N22, S12); the state seal and `cache-prune.yml` retire at the `velnor-gen` cutover. |
| I030 | A dedicated trusted builder or publisher, not CI jobs, writes the trusted cache on each main advance. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 1) | As build-once-per-site (F-B15, F-H31): one builder per site builds each `main` generation at batch priority and is the only writer of the trusted scope. It publishes through `mbx-cache` while N54 keeps the server, otherwise as changed files through the site store; its CPU is charged per merged PR. |
| I031 | Let merge_group runs write the trusted cache scope. | Claude Fable 5.2, Muse Wonder Full, Gemini 4 Argon | Claude Opus 5.5, GPT-6.2-Sol, Muse Wonder, Gemini 4 Carbon | Rejected | Unneeded once the builder builds `main`, and a merge group may never land. |
| I032 | PR-scoped writable cache read only by that PR and dropped when it closes. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (v3) | Reversed by decision 10: PR-produced results are reused for identical inputs as `candidate` receipts (the same PR's later pushes, other PRs, stacked PRs, merge groups) and become `trusted` only when their tree is on `main` and audit A1 has passed. The original concern (F-H16, stale build-script output) stays answered: a build that mints a reusable receipt reads only the trusted cache plus its own outputs. |
| I033 | Keep the visible non-blocking Cache check: red when a unit that should have hit compiles. | Claude Fable 5.2, GPT-6.2-Astra, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | `docs/goals.md` design rule 3; velnor-gen already emits the `Cache` job. |
| I034 | Sampled re-verification on every run (MBX_VERIFY compiles, re-executed reused verdicts). | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | — | Adopted (phase 2) | Strengthened (F-H14): a new reuse class runs in shadow with every reuse re-executed and graduates per obligation kind after about 3,000 clean reuses; after that 2% are re-executed, and every landed tree runs in full within 24 h (N48). A disagreement is re-run five times; a confirmed one retires that key version. The audit reports its bound: at 0.1% wrong reuses, 500 samples miss with probability 61%. |
| I035 | XFS reflink copy-on-write clones for per-job workspaces and target views. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | The hot path (N22): a reflink clone of the warm target took 0.25 s and 2.5 MB, against 8.6 s and 10.75 GiB for today's full copy onto the ext4 `emptyDir` (S12). |
| I036 | Cache whole target/ snapshots per scope and restore them by reflink. | Gemini 4 Carbon, Gemini 4 Argon | Muse Wonder | Adopted (phase 1) | The hot path (N22, S12): over a reflink clone of the node's warm snapshot of `main`, plain cargo compiled 0 crates of 357 in 0.31 s, against 92 s cold. Only the trusted `main` generation is snapshotted; PR builds write none. Issue #585 had not decided it: a 5-crate lab on hosted runners, mbx without its store between ephemeral runs. On the Mac both engines rebuilt the same 6 crates only because the path changed (S11). |
| I037 | Job-level verdict reuse keyed by the hash of every file the job read. | Claude Opus 5.5 | GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon | Rejected | As a verdict key: a read trace misses negative lookups and cannot speak for branches that did not run (GPT-6.2-Astra). Kept as an audit of declared inputs (I069). |
| I038 | Verdict cache per test executable with sandbox-enforced inputs, written by the agent. | Claude Fable 5.2 | Muse Wonder, Gemini 4 Carbon | Experiment-gated (phase 4) | Becomes N02: test binary digest plus enforced runtime inputs. Enforcement by mount namespace, not Landlock, which cannot restrict `stat` (Gemini 4 Carbon review). |
| I039 | Skip test binaries whose digest and environment class match a recorded pass, with an audit sample. | Muse Wonder Full | Muse Wonder, Gemini 4 Carbon | Experiment-gated (phase 4) | Same as I038. A digest alone misses runtime fixtures, such as the samples `golden.rs:19-24` reads. |
| I040 | Signed evidence receipts from a trusted verifier; test-verdict reuse starts at 0%. | GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Velnor services sign receipts; a job never does. Reuse starts with exact trees only. |
| I041 | Never cache test outcomes across different trees; cache the binaries and always execute the tests. | Muse Wonder, Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | Rejected | Superseded in phase 2 by input-addressed keys (N01): results cross trees only when the obligation's key, the hash of everything its sandbox contains, is equal. |
| I042 | Dispatcher scores nodes by cache warmth, load and site. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted-later (phase 3) | Objective: least predicted finish time (queue, start, delta build, tests), so an idle cold node beats a busy warm one. |
| I043 | Placement through Kubernetes: cache-digest node labels, preferred node affinity, hashing fallback. | Muse Wonder, Muse Wonder Full | — | Rejected | Affinity acts when a pod is created, not when GitHub assigns a job. |
| I044 | Pre-warm every node from main after each merge. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 1) | Every node is warm for `main`, but no longer by rebuilding it on every node (overturned in form, F-B15, F-H31): five nodes × three roots × eight merges an hour would be 120 incremental builds an hour. One builder per site builds each generation, and each node's warmer takes it into its snapshot. |
| I045 | Serve run-local artifacts (plan, reports, test archives) node-locally instead of GitHub round trips. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | — | Adopted-later (phase 4) | Test archives and reports move through the site store. The velnor-new nextest archive is 1.48 GB and took 36–43 s to upload to GitHub (#585). |
| I046 | Local mirrors: wire in the crates mirror, add a registry pull-through and image pre-pull. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Crates mirror through `CARGO_REGISTRIES_CRATES_IO_*` variables, which survive a custom `CARGO_HOME`. `cargo fetch` drops from 3.5–6.1 s to 2.7–3.3 s (evidence §B.4.2); images are already local (0 real pulls of 1,424). |
| I047 | Fix hosted-versus-scale-set path drift (work against _work) and drop RUNNER_TEMP and ImageOS from cache keys. | GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | Verified: lookup filters on `version` (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`) and the path is `${{ runner.temp }}` (`ci.yml:337`), which differs between hosted and ARC runners. Fixed absolute paths until GitHub cache leaves the cluster path. |
| I048 | Jobs never get a writable host cache directory; a cache service with scoped credentials mediates writes. | GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Jobs never write a shared store. The node agent harvests outputs after the build phase. |
| I049 | Drop the head-equals-tip seal check: objects are valid by content and verified commit. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Moot once the builder replaces the seal: content keys need no tip check. |
| I050 | Isolated fetcher for private dependencies; derived outputs never readable by forks. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted | `docs/goals.md:43`. velnor-new has no private dependencies today, so this is a store rule, not work. |

### Workflow

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I051 | Move the eight hosted gate and lint jobs onto the cluster (velnor-actions v0.1.81, PR #1332). | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | 57.0 of 71.8 min on 11-18 was hosted gate queue (evidence §A T5); cluster jobs queue 0.2 min (§A T4). |
| I052 | Narrow the triggers of Source qualification and Job image candidate, or move them to the cluster. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 0) | Job image candidate ran 306 times and Source qualification 267 times on 11-18, all hosted (evidence §A T11), on an org with the Free plan's 20 concurrent hosted jobs (`gh api orgs/tailrocks`: `free`). |
| I053 | Skip the full CI run on push to main when merge_group passed the same SHA or tree. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Refined (F-H20): the push run keeps one real Actions job named `Required`, because the release gate reads it, and skips each obligation the merge group verified on the same tree, never on the group run's conclusion alone. 120 of 159 green merge groups had a push run on the same SHA (evidence §A T7), and 10 of 124 such runs failed on an already-green SHA (S1). |
| I054 | Affected-only selection on merge_group runs. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Rejected | Withdrawn (F-H02, verified): `velnor-ci` diffs a merge group against the entry ahead (`event.rs:42-45`), so a failed predecessor's change is never selected, and HEADGREEN lands it. Merge groups stay full under hard decision 3 and are satisfied by key lookup (N01). The file-read audit and shadow plans that gated it judge one commit and cannot see this (F-H13). |
| I055 | Tree-equal fast-pass: merge_group reuses the PR verdict when the trees match. | Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra, GPT-6.2-Sol | Adopted (phase 2) | Generalised (F-H03): every obligation of the consuming plan needs a receipt with its own key. The consuming inventory is recomputed from the protected policy and the actual event, and evidence is matched per obligation, configuration and expected test inventory; a missing or zero-test report fails. With per-root keys a merge group reuses `main`'s receipts for untouched roots. Exact tree is the degenerate case, matching only 0–18% of merge groups today (S4). |
| I056 | Cut velnor-new over to velnor-gen: one job per Cargo root. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | As one job named `Required` with the obligation graph inside (F-B04): its single step calls `ensure`; separate jobs only where permissions or publication need them. With parity: the local-mode configuration, the maintenance checks, the required names, lanes with one target lane per check kind and one jobserver (F-H28), the example-binary fixtures and the mise pin (F-H41). |
| I057 | Collapse to one root-workspace pipeline (plan-and-static, lint, build, test shards). | Gemini 4 Carbon, Gemini 4 Argon | — | Adopted-later (phase 2) | Follows the owner's root-workspace move. Package selection and 'check alone' steps stay inside the job: #585 found crates that pass workspace clippy and fail alone. |
| I058 | Fewer gate hops: lints folded into the plan job, Ledger CLI build replaced by a released helper. | Claude Opus 5.5, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1–2) | Ledger CLI heads the critical path in 8 of 18 runs (GPT-6.2-Sol); velnor-gen uses a released helper instead. |
| I059 | Remove build.jobs=2, --jobs 2 and compiler_process_budget=2 on the cluster. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Verified `--config build.jobs=2` at `mise.toml:90,108,128,137,148,157,170,193`. maintenance-helpers is the last job in 102 of 154 runs (Claude Opus 5.5, measured). |
| I060 | Dissolve maintenance-helpers into cached workspace jobs with declared inputs. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | 508 s job with no cache (evidence §A T10). Its archive-guard goldens are serialised on purpose (`.config/nextest.toml:17-19`) and move to their own lane. |
| I061 | Split looped tests into one test per case. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 1) | Verified loops over 40 cases (`image_publication_public.rs:85-92`, 258–274 s) and 6 cases (`shape.rs:97-106`, 203–271 s). No shard count shortens one test. |
| I062 | Fix the slow suites at the root: journal fixes #1333 and #1349, migration squash or template, injected sleeps. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | The unit of cost is `Journal::open` (93 call sites): the phase-1 gate is a pre-migrated template that opens in at most 1 s. The logs show the floor after the two loop splits is still 125–138 s, plus a 924.8 s receipt-integrity test (F-H33), so expensive valid prefixes are built once with the real code and cloned per case, with each case keeping its own assertions. |
| I063 | Deduplicate the runner-host default and local-mode test sweeps. | Gemini 4 Carbon, Gemini 4 Argon | GPT-6.2-Astra, GPT-6.2-Sol | Adopted (phase 2) | Reframed (F-B18): extract `velnor-runner-local`, so the default and `local-mode` configurations both still run but stop compiling the 157k-line crate twice. Running only one configuration stays rejected: `local-mode` gates production code and the release build refuses the feature. |
| I064 | Build runner-workspace dependencies at opt-level 3 in the test profile. | Claude Fable 5.2 | GPT-6.2-Astra, GPT-6.2-Sol | Owner decision | Recommend an experiment: −66% CPU measured on the slowest test class (Claude Fable 5.2). Dependencies only; debug assertions and overflow checks stay on. |
| I065 | Enforce a test time budget in CI: no test over 10 s, suite critical path at most 60 s. | Claude Fable 5.2 | — | Adopted (phase 1) | 10 CPU-seconds per test measured alone, outside a recorded list that must shrink; non-blocking in phase 1, blocking for new tests from phase 2 (F-H33, F-B17). |
| I066 | Build test binaries once per configuration; shards consume a nextest archive, balanced by recorded durations. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 2) | Build once per configuration in phase 2; the cluster test executor (N04) consumes the binaries in phase 4. |
| I067 | Shard only when summed test time over threads exceeds the longest single test. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted | Distribute only when summed time ÷ slot threads exceeds the longest test plus transfer. #585 found no threshold up to 60 s on hosted runners because a second job cost 19–65 s. |
| I068 | Select by Cargo package graph and audited reads, not coarse globs. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | In the current engine (F-B16): declare the crate-root build scripts' inputs. Verified cause: `undeclared_reads` is true for every crate with a build script (`velnor-actions-rust/src/tasks.rs:254`), so heavy jobs ran in 97.4% of PR runs. A generator or helper PR then runs about 3.5 min instead of 10.9 [E]. |
| I069 | Record each job's file reads with fanotify to verify selection. | Claude Opus 5.5 | Claude Fable 5.2, GPT-6.2-Astra | Adopted-later (phase 3) | As telemetry only (F-H13): it sees the obligations that ran, and a missed file belongs to one that did not. An obligation is skipped only on a key that proves its inputs unchanged; an undeclared read widens the declarations and shows in the non-blocking `Selection` check. |
| I070 | Enforce declared inputs with a deny-by-default sandbox (Landlock), network off by default. | Claude Fable 5.2 | — | Experiment-gated (phase 4) | Via mount namespaces (N02); Landlock leaves `stat` and `access` open. |
| I071 | Scheduled full run as the audit of selection and caches. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | Nightly full run (velnor-gen default) plus a full run every 20th merge; both re-execute a sample of reused results. |
| I072 | Plan and gate run from released trusted code, never from the candidate. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Gemini 4 Argon | — | Adopted | `docs/goals.md` design rule 12. |
| I073 | Split velnor-runner-host (157k lines) into 6-10 crates. | Claude Opus 5.5 | — | Adopted (phase 2) | Overturned to adopted (F-B18): five or six crates along module boundaries and one test binary per package. A one-file edit cost 61 s and 159 CPU-s on a Mac (Claude Opus 5.5 review). Keeping incremental state is an experiment (10.9 s on a Mac, Claude Fable 5.2 review); input keys tolerate its non-identical binaries. |
| I074 | Agents debounce pushes by 20-30 s; cancelled jobs release their slots at once. | Muse Wonder Full | — | Adopted (phase 3) | In part: cancelled jobs release slots at once. Push debounce rejected: it delays every push, and cancellation already stops superseded runs. |

### Merge queue

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I075 | Raise max_entries_to_build from 2 to 5 or 6. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Adopted (phases 0–3) | Gated (F-H05, Muse Wonder Full): depth stays 2 until the under-load sampler has data, then moves only when a replay with real arrivals and all failures counted shows more merges per hour: about 4 at ≤ 3% false failures, 8 at ≤ 2% with queue-tip bypass trees. At 8.1%, depth 4 disturbs 29% of windows and depth 8 49%; one failure at the first slot invalidates the candidates behind it. |
| I076 | Cut min_entries_to_merge_wait_minutes from 5 to 0 or 1. | Muse Wonder Full, Gemini 4 Carbon | GPT-6.2-Astra, GPT-6.2-Sol | Rejected | `min_entries_to_merge` is 1, so the wait never applies; enqueue → merge-group run measured 19 s (Claude Opus 5.5). |
| I077 | Agents enable auto-merge on every PR. | Claude Opus 5.5 | — | Adopted (phase 0) | 0 of 40 recent merged PRs (synthesis sample) and 0 of 100 (Claude Opus 5.5) used auto-merge; green → enqueue is 7.5 min median, 88 min p90. |
| I078 | Stop catch-up merges of main into PR branches (the '0 behind' rule). | Claude Opus 5.5 | — | Adopted (phase 0) | A catch-up merge restarts a full PR run for no confidence: the merge group tests the real candidate anyway. |
| I079 | Serve merge_group jobs before PR jobs. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Carbon | — | Adopted (phase 0) | By scale set, not priority class (F-H23): a pod's priority is fixed before GitHub assigns it a job, so merge-group work gets its own `velnor-merge` scale set chosen in `runs-on` from `github.event_name`, plus the reserved merge path (N38). |
| I080 | Track merges per hour and queue depth as first-class metrics with alerts. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | — | Adopted (phase 0) | Queue residence and merges per hour are the numbers 11-18 needed. |

### Kubernetes

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I081 | Warm pool of pre-created pods or sandboxes. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1 and 3) | ARC `minRunners` per class first, pre-created single-use pods later. Idle runner 3 s against 14 s on demand (Claude Fable 5.2, measured). |
| I082 | No pod per job: a long-lived node agent creates sandboxes (namespaces, cgroups) directly. | Claude Fable 5.2, Gemini 4 Carbon | — | Experiment-gated (phase 2) | As elastic slots (F-B13): cgroup slots the agent carves instead of pods, 5–6 build slots per worker instead of 4 [E], measured beside the overlayfs experiment. Pre-created single-use pods stay the default. |
| I083 | Long-lived slot pods scrubbed and reused between jobs. | Muse Wonder, Muse Wonder Full | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Rejected | A job can leave daemons, images and config behind; legacy #275 shows a poisoned persistent store (evidence §E.6). |
| I084 | Start dind only for jobs that declare Docker; separate pool or class. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Extended (F-H30): the Docker-needing test binaries (`local_pair_public`, `local_host_public`, `velnor-local`'s Docker tests) are their own docker-class obligation beside the build, so the runner root, touched by 55 of 100 merges, builds in the build class. 57 dind probe failures an hour today (evidence §B.6). |
| I085 | Replace privileged dind with a Sysbox or Kata RuntimeClass. | Gemini 4 Argon | — | Experiment-gated (phase 5) | For the docker class, after the microVM measurement. |
| I086 | KVM microVMs restored from a snapshot, with dockerd running, for Docker and fork jobs. | Gemini 4 Carbon | Claude Opus 5.5, Muse Wonder, Muse Wonder Full | Experiment-gated (phase 5) | For the fork class and as the zero-start sandbox (moonshot M3). |
| I087 | Honest per-class CPU and memory requests and limits. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Corrected (F-H29): idle and running sandboxes share one memory budget. A worker has 109.5 GiB after the system, eviction margin and four gate pods, so build sandboxes request 24 GiB (peak observed 16.8 GiB) and four fit; 32 GiB would fit three. Pods request 2 CPU and used up to 83.7 cores today (evidence §B.2.2). |
| I088 | Static CPU manager with exclusive cores for build pods. | Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | Claude Opus 5.5, Claude Fable 5.2 | Experiment-gated (phase 2) | One node first. Exclusive CPUs stop the bursting that cold builds need. |
| I089 | kubelet reservations, memory eviction threshold, pod PID limit and PriorityClasses. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | No kube or system reservation and no memory eviction threshold today (evidence §B.1.3). |
| I090 | Site labels on nodes; keep a run's jobs in one site. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phases 1 and 3) | Labels first; run stickiness with the dispatcher. |
| I091 | Fork and unknown-trust PRs stay on GitHub-hosted runners until stronger isolation exists. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Rejected | Moot by decision 8: there are no pull requests from forks, so no fork class, no fork-conditional `runs-on` and no hosted fork path. |
| I092 | Per-org quotas or fair shares for tailrocks, ChainArgos and jackin. | Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full | — | Adopted (phase 1) | Three orgs share 480 threads; ARC maxima add to 160 runners (evidence §B.3.2). |
| I093 | HA control plane: add k3s server nodes beside the single velnor-237. | Claude Opus 5.5, Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (before the executor cutover) | Moved earlier (F-H24, verified: velnor-237 is the only k3s server): losing it stalls pod creation and cleanup in both sites. Once Velnor is the only executor (N57), the executor cutover waits until control-plane and site loss are qualified, with three servers across failure domains or independent admission per site. |
| I094 | Keep ARC installed as the rollback path; cut over by label on a separate scale set. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | — | Rejected | Overturned by the owner's decision (N57, N64): ARC serves velnor-new until the cutover gate passes and is deleted in the cutover change; rollback afterwards is the previous Velnor release, drilled weekly. |

### Host tuning

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I095 | Move the kubelet root directory and job work dirs onto the XFS data volume. | Claude Opus 5.5, Claude Fable 5.2, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Job work directories as local-path volumes, which are already XFS with reflink, while `emptyDir` is ext4 on the OS disk and cannot clone: a copied warm target costs 8.6 s and 10.75 GiB per job there (S12). XFS project quotas per volume; fixed-size eviction thresholds only if the kubelet root moves too (F-H32). |
| I096 | Dedicated XFS (reflink, noatime) cache volume per node. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | The existing XFS LV; reflink confirmed on the local-path volume of both measured workers (S12). |
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
| I106 | Set CARGO_BUILD_JOBS and nextest threads to the slot's CPUs. | Claude Opus 5.5, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Fable 5.2 | Adopted (phase 1) | Set explicitly to the sandbox's CPUs (F-H29): a pod without a CPU limit sees all 96 CPUs. |
| I107 | Use the mold linker for CI builds. | Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | Claude Fable 5.2 | Experiment-gated | Rust ≥ 1.90 already links `x86_64-unknown-linux-gnu` with rust-lld; measure link time before adding mold. |
| I108 | Reduce debuginfo in CI: line tables for workspace crates, none for dependencies. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | velnor-gen defaults to line tables (`generator/crates/velnor-model/src/repo.rs:332`); #585: −10% entry size with line tables, −24% without third-party debuginfo. |
| I109 | Benchmark sccache as a mutually exclusive alternative to mbx. | GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder, Muse Wonder Full | Rejected | velnor-gen reports a `rustc-wrapper` as a weakening (`generator/crates/velnor-gen/tests/it/weaken.rs:89-93`). |

### mbx and blaze

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I110 | Keep mbx; move its store to a node-persistent path and add a site remote. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Gemini 4 Carbon | Adopted (phase 1) | As the cross-node layer, approved by the owner (N54), not as the per-job hot path. Cold node 39.9 s against 92 s; a lockfile change on a node without the matching snapshot 43.4 s against 67.4 s, through per-crate reuse; on the same node plain cargo beat it, 67 s against 88 s (S12). Jobs run plain cargo, so `velnor-gen`'s rule that reports an mbx cargo wrapper as a weakening (`weaken.rs:84-88`) stands. |
| I111 | Retire mbx as the cache; plain cargo over reflinked target snapshots plus velnor-ci inputs restore. | Gemini 4 Carbon | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Argon | Adopted (phase 1) | In part (S12): plain cargo over reflinked target snapshots with `velnor-ci inputs restore`'s time rule is the hot path; mbx is not retired but kept as the cross-node layer while it passes its threshold (N54). |
| I112 | blaze is ineligible for required checks: it builds with its own toolchain and ignores the 1.99.1 pin. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | blaze builds with its own nightly and ignores the 1.99.1 pin. |
| I113 | blaze as optional local pre-push feedback for agents. | Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Rejected | #652: 6.5–8.4× slower than cargo in CI and 8 of 8 restored-state runs failed. Preflight (N08) gives agents faster feedback on the real toolchain. |
| I114 | Remove mbx clean and cache export/import from jobs once stores persist. | Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (phase 1) | Jobs run plain cargo over clones (S12); mbx lives only in the builder and the warmers. |
| I115 | Verify mbx's remote protocol and upgrades in a spike; guard the single-vendor risk. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Sol, Muse Wonder Full | — | Adopted (phase 0) | Done (S12): at velnor-new's size a second node got 496 hits and 0 misses. Production needs TLS, because mbx refuses a plain-HTTP remote with a token unless the host is loopback, and `MBX_REMOTE_TRUST_WRITES=true` for a writer outside a GitHub or GitLab push to a protected branch. The single-vendor risk is bounded: the hot path does not depend on mbx, and N54 retires the server if it stops paying. |

### Hardware

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I104 | Buy or rent high-clock nodes for the serial critical path. | Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full | Muse Wonder, Gemini 4 Carbon, Gemini 4 Argon | Owner decision | Recommend renting one high-clock node in phase 1 for the test-floor experiments, not after phase 2 (Claude Fable 5.2, Muse Wonder Full): the serial path is one 157k-line crate and the longest tests (estimates 1.3–1.5×). |
| I105 | Buy more EPYC workers. | Muse Wonder, Muse Wonder Full | Claude Opus 5.5, Claude Fable 5.2, Gemini 4 Carbon, Gemini 4 Argon | Owner decision | Set the trigger and budget now (Muse Wonder Full): buy a worker when worker CPU stays above 70% at peak after dedupe, or the eligible wait for merge work exceeds a p90 of 10 s for two weeks, so procurement is never the critical path. |

### Process

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| I116 | Every phase ends on a numeric gate measured on velnor-new; a miss stops the next phase. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted | Each phase in the design ends on a gate measured on velnor-new. |
| I117 | Matched-cohort comparisons with minimum sample sizes; report CPU-seconds and GiB-minutes too. | GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder, Muse Wonder Full | — | Adopted | Accounting rules (design review): charge foreground jobs, remote slots, cancelled attempts, discarded predictions, the builder, speculation and audits to the consuming repository; split runner-minutes into completed, cancelled and superseded work (F-H40: 1,156 of 2,558 PR runs were cancelled); count each occupied slot once; publish idle reserved core-hours, CPU-seconds and GiB-minutes beside runner-minutes. |
| I118 | Measure push-to-merge from PR timelines and instrument per-job spans. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (phase 0) | The metric the brief names; the evidence pack does not measure it. |
| I119 | Track flake rate as failures on SHAs that already passed. | Claude Fable 5.2 | — | Adopted (phase 1) | As a discordance measure (F-H34): S1's 10 of 124 is one direction only. Pairs are sampled in both directions under representative load, and no false-failure bound is claimed before about 300 pairs. |
| I120 | Prove changes on velnor-lab before velnor-new. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon | — | Adopted | `docs/goals.md` design rules 12 and 14. |

### Added in synthesis

| Id | Idea | Proposed by | Opposed by | Decision | Why |
|---|---|---|---|---|---|
| N01 | Verification ledger with input-addressed keys: the key covers everything that can change a result, per Cargo root and later per package; the exact tree is the degenerate key. | Synthesis; keys from Claude Opus 5.5 (T3), Claude Fable 5.2, Gemini 4 Argon, GPT-6.2-Astra, Muse Wonder Full (F-B07) | — | Adopted (v3) | Amended for v3: every sandbox holds the full tree (a sparse sandbox cannot load the workspace, and a trimmed one turns a real failure green: Claude Opus 5.5's measurement, settled by ruling Q5); every workspace manifest, the executor identity (decision 20) and the environment class are in the key, and per-package keys add the digests of executables a test spawns (decision 11). A read outside the key's paths, including `ENOENT`, demotes the receipt to exact-tree scope. |
| N02 | Closures finer than packages: reuse a test binary's result when its digest and its enforced runtime inputs match. | Synthesis; from Claude Opus 5.5, Claude Fable 5.2, Muse Wonder Full (I037–I039) | GPT-6.2-Astra, Muse Wonder, Gemini 4 Carbon (as proposed there) | Experiment-gated (phase 4) | Overturned as the key (F-B07, F-H12): input keys need no bit-identical builds. Where a digest ever joins a key it covers every executable the test can spawn (F-H10, verified: 171 `CARGO_BIN_EXE_*` uses in 102 test files). |
| N03 | Speculative merge candidates: verify `main ⊕ PR` as soon as main moves, before GitHub creates the merge group. | Synthesis; owner direction (speculative merge queue) | — | Rejected | Replaced by N19: GitHub builds a merge group on the queue entry ahead, not on `main` (F-H04, verified: 122 of 193 merge-group runs were built on an unlanded group commit). |
| N04 | Cluster test executor: one request fans its test cases out over many sandboxes in the site. | Synthesis; extends GPT-6.2-Astra (worker groups inside a job) | — | Experiment-gated (phase 4) | Moonshot M6. It fetches each changed test executable and its verified fixtures by digest; moving the 1.48 GB archive to four workers in 5 s from one sender would need 9.5 Gbit/s. |
| N05 | Hot Worker: a pre-started `Runner.Worker` waits on its pipe in every idle pod. | Synthesis | — | Rejected | Moot: no official Worker runs in production (N57). The native executor starts a job within 1 s of the broker message [T]. |
| N06 | Receipt short-circuit: the dispatcher completes a job whose obligations all hold by receipt, or the plan job itself, without a sandbox. | Synthesis; owner direction (native executor that owns acquisition) | — | Adopted (phase 2) | In native form (N57): Velnor's executor runs the job, and an obligation step satisfied by a receipt completes with a log that names the receipt. No synthetic completion is needed, and C4 holds because receipts are agent-observed. |
| N07 | Preflight: agents ask Velnor to verify a tree before pushing; the PR run then finds receipts. | Synthesis; owner direction (CI before the push) | — | Experiment-gated (phase 3) | `velnor verify` calls the same `ensure` (N18). Only Velnor-executed results count; attested runs on a developer Mac are rejected for the verdict (another platform, a key the agent holds). |
| N08 | Agent workspaces on the cluster, sharing node stores, so the agents' own test runs become receipts. | Synthesis | — | Owner decision | Phase 5. Moves 'local first' (`docs/goals.md` rule 11) onto the cluster. |
| N09 | Push-webhook start: plan and build begin on the push webhook, before GitHub creates the jobs. | Synthesis | — | Rejected | Overturned by decision 12: a push starts no verification; only the label flow does. A labelled head still starts at its webhook, before any Actions job exists. |
| N10 | Node-held job leases: the node agent renews the job lock and fails a lost job with the job token. | Synthesis | Claude Fable 5.2 (no journal) | Adopted (phase 2) | Part of the native executor and dispatcher (N57): renew every 60 s from `lockedUntil` with a loop written fresh (the upstream overload dereferences a null request when renewal fails, `JobDispatcher.cs:748-852`), keep the Listener's first-renewal rule and force-fail path, and journal what a standby needs to reopen sessions (F-H25). |
| N11 | Velnor refuses fork and unauthorised work before any sandbox exists. | Synthesis; from Claude Opus 5.5 (trust class from the job message) | — | Adopted (phase 2) | `ensure` acts only on same-repository heads, so a fork's tree never reaches a sandbox; the dispatcher reads each request before acquiring it, so unauthorised work is refused before a sandbox exists. Fork PRs keep hosted runners (N65). |
| N12 | Bind the required checks to an integration in the ruleset. | Synthesis | — | Owner decision | Recommend now. Ruleset 24396608 names `Required verdict` and `Required` with no `integration_id`, so any app, or a workflow token with `statuses: write`, can post them. |
| N13 | Record the tested tree hash, root keys and obligation digests in every run. | Synthesis | — | Adopted (phase 1) | Receipts need them, and the window-model replay (N34) and queue-tip shadow (N19) can only be measured with them. |
| N14 | Flake budget: paired reruns in both directions under load as the denominator, nextest flaky reporting, and quarantine only with a blocking equivalent or an expiring owner exception. | Synthesis; extends Claude Fable 5.2 (I119); GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full (F-H34) | — | Adopted (phase 1) | Two full runs per merge at 8.1% discordance each fail about 15% of merges for no reason (1 − 0.919²). No bound is claimed before about 300 pairs. A quarantined test in a non-blocking lane would lower confidence, so it needs a blocking check covering the same property or a recorded exception that expires. |
| N15 | PR-light, queue-full: PR pushes run only fast obligations; the merge queue runs everything the change selects. | Synthesis; Muse Wonder (T4) | GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Argon | Rejected | It moves failures into the serial queue, where each one rebuilds every entry behind it; 3 of 40 queue exits already fail with full PR runs (S2). One job per run and the test-floor programme make the full PR run short instead (F-B21). |
| N16 | Coverage-based test impact selection: skip tests that executed none of the changed code. | Synthesis (considered) | — | Rejected | A test that ran none of the changed code can still change behaviour through layout, statics, timing or undefined behaviour, so the skip is not a proof. Closure receipts (N02) take the sound part of the gain. |
| N17 | Distributed compilation across nodes for cold builds. | Synthesis (considered) | — | Rejected | A cold runner-workspace build is 696 CPU-s with a serial chain of about 170 s (Claude Fable 5.2), so the chain, not CPU, bounds it. Sharing complete outputs across a site's nodes is what `mbx-cache` does: a cold node in 39.9 s instead of 92 s (S12, F-B24). |
| N18 | `ensure(tree, obligations)` on the node agents, called by the push and merge-group webhooks; one witness job named `Required` calls the same `ensure`, streams the logs and exits with the verdict. | Claude Fable 5.2 (T1); variants from GPT-6.2-Sol, Gemini 4 Argon, Gemini 4 Carbon, Muse Wonder Full, Muse Wonder, GPT-6.2-Astra (F-B01) | — | Adopted (phase 3) | In changed form (N57): the webhooks still start `ensure` and the `Required` job joins the work, but Velnor's native executor runs that job; until the executor cutover a `velnor-ci witness` step on ARC does. Removes two of three job hops and the plan job; median verdict about 35 s [E]. Behind M1's experiment. |
| N19 | Speculate on the queue tip: ordered prefixes `T_i = T_{i−1} ⊕ PR_i` plus single-ejection bypass trees, verified at batch priority. | Claude Fable 5.2 (T2), GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Carbon, Muse Wonder Full, Gemini 4 Argon, Muse Wonder (F-B05) | — | Adopted (phase 3) | Read-only shadow from phase 0; kill below a 70% tree match. GPT-6.2-Astra's replay matched 36 of 45 groups with one predecessor. A wrong guess only wastes preemptible CPU. |
| N20 | Every obligation whose inputs the change touched executes three times in parallel; any failure blocks. | Claude Fable 5.2 (H4); GPT-6.2-Sol, GPT-6.2-Astra (F-H35) | — | Adopted (v3) | Ruled Q12 (Claude Opus 5.5, Claude Fable 5.2): two executions before merge, in parallel on two different nodes with different test orders, no retry, and the cold audit of every landed commit as the third, reported by the push run's `Required`. If lab C2 finds the twins correlated, the merge group re-runs the PR's changed obligations even on an identical tree. |
| N21 | Cold differential build: nightly and every-20th-merge runs build from empty in a fresh pod and compare results and test-binary digests with the warm path; lineages restart on lockfile or toolchain changes and daily. | Claude Fable 5.2 (H3); Claude Opus 5.5, GPT-6.2-Astra, GPT-6.2-Sol (F-H15) | — | Adopted (phase 2) | Cargo cannot see a file it did not read in the last build; only a cold build finds that staleness. The helper already forces cold builds by age (`velnor-ci/src/cli.rs:73`). A lineage restart builds cold and publishes into a fresh `mbx-cache` namespace, so it reads nothing an older lineage wrote. |
| N22 | Decide the compile cache by a head-to-head on the cluster: (a) mbx with a node-persistent store and an `mbx-cache` server per site, (b) plain cargo over reflinked snapshots, (c) plain cargo over overlayfs generations. | Gemini 4 Carbon (snapshots, page cache); Claude Fable 5.2 (H12) and Muse Wonder Full (H-11) (experiment gate); Gemini 4 Argon (cargo locks, jobserver); owner direction (F-B14) | — | Adopted (phase 0) | Ran and decided (S12): plain cargo over a reflinked warm node snapshot is the hot path; mbx and a per-site `mbx-cache` are the cross-node layer, kept by a measured threshold (N54); overlayfs is the remedy if the page-cache budget is exceeded (N56). The open measurements and the reproducibility gate C6 needs move to work item 1. |
| N23 | Webhook ingress: one authenticated endpoint per site; missed deliveries replayed from the App's delivery list every minute. | Claude Fable 5.2 (missing 4) | — | Adopted (phase 3) | Nothing on the nodes accepts webhooks today (§B.4.3); the witness's own `ensure` covers any gap. |
| N24 | Break-glass: the hosted workflow stays as a permanent fallback, exercised weekly; the cluster runs only released agent versions. | Claude Fable 5.2 (missing 3) | — | Rejected | Overturned by the owner's decision (N57): the official runner is no fallback. Self-hosting stays safe because the cluster runs only released executor versions, never the change under test, with the previous release installed and the switch back drilled weekly (N64). |
| N25 | Seed the next generation and its receipts before an image, toolchain or helper bump lands. | Claude Fable 5.2 (missing 8) | — | Adopted (phase 2) | A bump changes every definition digest; otherwise the hour after it is cold for everyone. |
| N26 | C5 widened: randomness, commit identity, CPU count and environment; build and test sandboxes get no `.git` and a scrubbed environment. | GPT-6.2-Sol (2), Claude Fable 5.2 (H9), Muse Wonder, Muse Wonder Full (F-H09) | — | Adopted (phase 2) | Verified: `velnor-runner-host/build.rs:69-80` reads a lock from the Git object store, and the engine's task model already marks `uses_random` (`tasks.rs:70-72`). |
| N27 | Run every moonshot experiment and the cache head-to-head in week 1, beside phase 0. | Claude Fable 5.2 (T6), GPT-6.2-Astra, GPT-6.2-Sol, Muse Wonder Full, Gemini 4 Argon (F-B22) | — | Adopted (phase 0) | 142 PRs merged through the queue in three days; the experiments touch no required path and should choose the architecture before anyone builds it. |
| N28 | HEADGREEN: C1 holds for group tips; a group's earlier commits are listed for bisection and release tooling; ALLGREEN offered to the owner. | Claude Opus 5.5 (H1), GPT-6.2-Astra, GPT-6.2-Sol (F-H01) | — | Rejected | Ruled Q10: ALLGREEN now; under HEADGREEN three commits whose own `Required` failed are on `main`. Depth 2, then 4 once a group verdict takes ≤ 5 min at P90 and paired false failures are ≤ 3%; nothing supports 8. |
| N29 | One receipt per obligation as it finishes, and in-flight join: a request whose key is executing waits for it; a cancelled request drops its claim. | Claude Opus 5.5, GPT-6.2-Sol, GPT-6.2-Astra, Gemini 4 Argon, Muse Wonder Full; Gemini 4 Carbon (F-B09) | — | Adopted (phase 2) | Recovers the work of the 18.3% same-SHA restarts and of superseded runs. A partial result satisfies nothing. |
| N30 | Ledger trust root: signing keys outside pods and outside the agent's general service, one key per node and sandbox class, an append-only receipt log per site, verification before a verdict, a sandbox-scoped agent socket, audits on another node, and a degraded mode. | Claude Opus 5.5; GPT-6.2-Sol (6), Muse Wonder Full, Muse Wonder, Claude Fable 5.2, Gemini 4 Carbon (F-H17) | — | Adopted (phase 2) | Receipts are shared state. A docker node's key signs docker-class obligations only and never a trusted receipt, because privileged dind runs PR code there. |
| N31 | A Velnor merge train: fast-forward `main` to the exact verified commit by compare-and-swap, in place of GitHub's merge queue. | Claude Opus 5.5 (T8), Muse Wonder Full (T-1), Claude Fable 5.2, GPT-6.2-Sol (lab only) (F-B10) | — | Owner decision | About 45 s less per merge [M: 19 s + 26 s of queue latency]; every tip verified by construction. Recommend a two-week shadow in phase 4, killed below a 95% tree match (owner question 11). |
| N32 | One counted re-execution for tests whose package closure the change did not touch, reported in a non-blocking `Flaky` check. | Claude Opus 5.5 (T7), Claude Fable 5.2 (F-B19) | GPT-6.2-Astra, GPT-6.2-Sol (keep the first failure as evidence until classified) | Owner decision | At a 3% false-failure rate about 0.1% of merge groups would still fail falsely [E]. Retries are 0 today, so this changes the profile (owner question 12). |
| N33 | Report the admission span (run created → first job) and the cancelled-run stall; drop `edited` restarts of identical work. | Claude Opus 5.5 (H3), Claude Fable 5.2 (H1), GPT-6.2-Astra, GPT-6.2-Sol (F-H06, F-H07) | — | Adopted (phase 0) | Verified: 33 of 105 sampled PR runs waited 60 s p50 and 626 s p90 (2,203 s max) for the run they superseded; 18.3% of PR runs restarted on an unchanged head. One job per run removes the stall's cause (no `always()` job left in a cancelled run). |
| N34 | Commit the targets on the short-window regime. | Claude Opus 5.5 (T1), Claude Fable 5.2 (H5), Muse Wonder Full (F-B06) | — | Adopted | Two replays (98 and 142 merges): the merge-group tree equals the PR-tested tree in 89% of merges at a 1–2 min window, 78–83% at 2.5–5 min and 52–53% at 11–20 min. |
| N35 | C4 without the exception: only results the agent observed from processes it started, with the expected test inventory, are evidence for another run; a forged passing report yields zero reusable receipts (lab test). | GPT-6.2-Sol (1), GPT-6.2-Astra, Muse Wonder (H2), Muse Wonder Full (H-1) (F-H08) | — | Adopted (phase 2) | The draft let a job's own report back an exact-tree receipt; a test that reports a pass without testing would then green its merge group, and the receipt would become trusted on landing. |
| N36 | An execution manifest joins every key: event class, allowed environment, toolchain and image digests, sandbox class and CPU count, path prefix, file modes and symlink targets. | GPT-6.2-Sol (2), Muse Wonder, Muse Wonder Full (F-H09) | — | Adopted (phase 2) | The same tracked bytes can run with different refs, events, environment or host runtime. An obligation is event-sensitive until an enforced run shows otherwise. |
| N37 | Epoch fencing and fault injection: the ledger persists an execution's intent before a sandbox starts, results land by compare-and-swap on that epoch, and every remote call and journal write is crash-tested in the lab. | GPT-6.2-Sol (7, 12), Claude Fable 5.2 (H8), Gemini 4 Carbon, GPT-6.2-Astra (F-H24) | — | Adopted (phase 2) | Without a fence, a restarted or partitioned node can report into an execution another node now owns. |
| N38 | Reserve the merge path: one 16-CPU build slot and 2 gate CPUs per site, about 7.7% of allocatable CPU; the wait clock starts at eligibility. | GPT-6.2-Sol, Claude Opus 5.5, GPT-6.2-Astra, Muse Wonder (T5), Claude Fable 5.2, Gemini 4 Carbon (F-B20) | — | Adopted (phase 1) | Waiting grows as ρ / (1 − ρ): about 19 times the service time at 95% utilisation. Reservation changes only waiting, never what runs. A three-day experiment cordons one worker as a merge and gate lane under ARC labels and compares queue waits (Muse Wonder). |
| N39 | A revocation epoch in every published verdict, and the released verifier authoritative while a new one is qualified. | GPT-6.2-Sol, GPT-6.2-Astra (F-H18) | — | Adopted (phase 3) | Turning a reuse class off does not reach a green check already posted or a partitioned site; the 7-day expiry is only a backstop. |
| N40 | Clocks and cohorts: start at the final commit's accepted push; report every PR class; gate each phase on at least 300 merges or four weeks, with intervals beside each P50 and P90, and claim a P90 only on at least 200 matched heads over peak and quiet periods. | GPT-6.2-Sol (13), Muse Wonder, Claude Opus 5.5, GPT-6.2-Astra (F-H39) | — | Adopted (phase 0) | The headline median covers 47% of PRs, and 50 merges cannot verify a P90 or a 1% rate. |
| N41 | A corpus of known-bad changes with green controls, replayed after every planner, sandbox or verifier change, warm caches included. | GPT-6.2-Astra; GPT-6.2-Sol | — | Adopted (phase 2) | It tests that the verifier finds defects in weeks when every real PR is green. A candidate that passes one bad change loses authority for that class. |
| N42 | Guardrails beside push-to-merge: time to the first actionable failure, first-push-to-merge, and failed attempts per merged change. | GPT-6.2-Astra | — | Adopted (phase 0) | 419 failed and 1,156 cancelled PR runs of 2,558 (evidence §A T1): a faster last push can coexist with slower feedback on every earlier edit. |
| N43 | Pin the RustSec advisory database by commit in the repository, advanced by an automated pull request. | Gemini 4 Carbon (F-H43) | — | Adopted (phase 1) | Verified: `cargo deny` runs with no offline flag (`ci.yml:127`), so it is a network read that can never be satisfied by receipt, and one new advisory ejects every queued entry. |
| N44 | Name the nextest profile in the generated workflow; give the `crates_mirror` and golden tests private temp directories and ports. | Gemini 4 Carbon (F-H36) | — | Adopted (phase 2) | Verified: the 2 s kill sits in `[profile.default]`, not `[profile.ci]`; the golden workflow's nextest commands pass no profile, so the kill would apply after the cutover. |
| N45 | Parallel lanes with one target lane per check kind, clippy and doc metadata pre-built by the `main` builder, the check-alone steps combined, and one CPU budget across lanes. | Gemini 4 Argon (2), Gemini 4 Carbon, GPT-6.2-Sol (F-H28) | — | Adopted (phase 2) | Verified: cargo holds an exclusive lock per target directory (`docs/hard-decisions.md:56-62`), so lanes on one target serialise, and separate targets without a shared budget can start 64–160 rustc processes in one pod. The budget is one jobserver shared by the sandbox's lanes; mbx's scheduler no longer applies, since jobs run plain cargo (S12). |
| N46 | The agent creates the target view after its sparse checkout, and the `main` builder builds at the request path. | Gemini 4 Argon (3) (F-H27) | — | Adopted (phase 2) | A checkout that cleans would wipe an earlier view, and the workspace path is part of cargo's fingerprints and `velnor-ci`'s compatibility key (`io/setup.rs:50`); the Mac measurement rebuilt the workspace crates at every new path (S11). |
| N47 | Enforce C5 for tests: a loopback-only network namespace and no event file in the sandbox; the trusted helper, not the candidate's nextest configuration, creates the namespace per test binary. | Gemini 4 Argon (8), Gemini 4 Carbon (F-H44) | — | Adopted (phase 2) | Verified: nextest wrappers are defined in the repository's own configuration (`crates/velnor-runner/.config/nextest.toml`), so the candidate could otherwise choose its own sandbox. |
| N48 | Execute every landed tree once in full within 24 h on batch capacity, whatever its receipts. | Muse Wonder Full (H-6) (F-H14) | — | Adopted (phase 2) | The nightly and every-20th-merge runs check only the tip, so a receipt-greened tree would otherwise never run; at 0.1% wrong reuses a 500-sample audit misses with probability 61%. |
| N49 | Ledger observability before phase 3: hit rate by mechanism, builder lag, the audit's false-green bound, and placement regret with its kill rule. | Muse Wonder Full | — | Adopted (phase 2) | Placement is dropped to site stickiness if a site fetch at P90 costs no more than a delta rebuild at P50, or regret stays under 5 s P50. |
| N50 | One job with the obligation graph inside, from phase 2. | Muse Wonder Full (T-3), Gemini 4 Carbon, GPT-6.2-Astra, Muse Wonder (F-B04) | — | Adopted (phase 2) | About 25 executed jobs × 33 s of per-job overhead ≈ 825 s a run today, and one job per root still leaves about 165 s. It keeps the job the release gate reads; in phase 3 the webhooks start its work before GitHub creates it (N18). |
| N51 | Docker without a privileged sidecar: a rootless BuildKit service per site for image builds, sibling sandboxes for containers in tests, a microVM class if more than 5% need a daemon. | Muse Wonder Full (T-7), Claude Opus 5.5 (H4), Gemini 4 Carbon (F-B23) | — | Experiment-gated (phase 3) | dind is the only privileged execution of PR code, costs 3–4 s a job and caused 57 probe failures an hour (evidence §B.6). |
| N52 | One verdict writer per repository and phase, behind a flag; a new writer shadows the old one first. | Muse Wonder (H12) (F-H21) | — | Adopted (phase 2) | Otherwise the workflow's required names and an App's `Required` could race once both exist. The witness is the only writer of `Required` for the repository. |
| N53 | Watch results after merge: revert rate, red-`main` rate, failures on receipt-passed trees, and cost per merged PR. | Muse Wonder | — | Adopted (phase 0) | The signal that catches a false green the audits missed; a risk detection in the design and part of work item 4. |
| N54 | Keep mbx plus a per-site `mbx-cache` as the cross-node layer, not the per-job hot path, while a measured threshold holds: at least 10% of build jobs would otherwise start cold or mismatched and are served from it, or at least 20 runner-minutes a day of foreground builds are saved; retire the server after four weeks below both. | Owner (approved); Claude Fable 5.2 (H12) and Muse Wonder Full (H-11) (experiment gate); Gemini 4 Carbon (snapshots, page cache); Gemini 4 Argon (cargo locks, jobserver) | — | Adopted (v3) | Kept with the threshold; it only fills nodes in the background (ruling Q16), its store on the site's RustFS (decisions 22, 23), and lab B3 keeps any transport only if it beats a cold build on more than 10% of requests. |
| N55 | Snapshot freshness rule: unchanged files keep the snapshot's recorded modification times, and every other file is newer than every output. | Synthesis (from the cluster run's method) | — | Adopted (phase 1) | Cargo's fingerprints for path crates compare times. The measurement extracted one archive whose file times are the commit's, so it never exercised a changed file with an older time. `velnor-ci inputs restore` already restores times only for unchanged files (`ci-helper/crates/velnor-ci/src/io/inputs.rs`); the agent applies the same rule where sandboxes have no `.git`, and work item 1 tests the case. |
| N56 | Page-cache budget for clones: a build sandbox's page cache counts inside its 24 GiB limit, about 7 GiB above the heaviest job's peak; memory pressure recorded per sandbox; overlayfs as the remedy. | Gemini 4 Carbon (F-B13) | — | Adopted (phase 1) | Measured (S12): reflinked files do not share page cache, and mbx restores by reflink on XFS too, so only hard links would share it. Each fully read clone held 14.29 GB; a no-op build reads only metadata. |
| N57 | Rust only: Velnor's native Rust executor runs every velnor-new job; GitHub's official runner (`Runner.Listener`, `Runner.Worker`) is not used in production, neither as the executor nor as a fallback. | Owner (binding, 2026-11-19); Gemini 4 Carbon (general native worker); Claude Fable 5.2, Muse Wonder and Muse Wonder Full (native lane) | — | Adopted (v3) | Amended by decision 20: Velnor Native is the default and the recommendation; GitHub Official is a supported executor chosen deliberately per job type in `.velnor/`, never an automatic fallback. Verdicts from either count, and the executor's identity is in every result key (N67). |
| N58 | The latest official runner release is the conformance oracle in the lab; `velnor-conformance` compares Velnor against it on step outcomes and conclusions, outputs, contexts and env (secrets masking included), expression results, the normalised timeline and logs, annotations and step summaries, cache and artifact effects, post steps, cancellation and timeouts. | Owner (binding); extends I007 and I017 | — | Adopted (v3) | Amended by decision 19: three reference modes (the conformance gate, sampled shadow runs of real jobs, on-demand diff), per platform backend; reference runs never produce verdicts, receipts, caches or releases (N68). |
| N59 | Latest APIs only: scale-set sessions, broker long-poll, JIT configuration, run-service acquirejob, renewjob and completejob, Results twirp for logs, timeline and step summaries, cache service v2, artifacts v4, the OIDC token endpoint, node24-era semantics. | Owner (binding) | — | Adopted (phase 1) | Out, with reasons: the legacy session with AES message encryption (replaced by JIT runners and the broker; it needs a private key on disk for no benefit); the legacy pipelines and distributed-task APIs (superseded by the run service and Results); the older `DTExpressions` evaluator (one evaluator only, `Expressions2`); cache v1 and artifacts v3 (deprecated by GitHub). velnor-legacy's `protocol.rs` still carries the legacy pipelines timeline API, which is not copied. |
| N60 | Copy, review and own velnor-legacy code in velnor-new where it targets the latest APIs and passes the oracle; velnor-legacy itself is not reactivated. | Owner (binding, with the owner's clarification) | — | Adopted (v3) | Amended by decision 14: velnor-legacy is very old; only code that makes sense is ported, and poorly designed code is re-architected (the god files `executor.rs` and `runner.rs` are split, `gha_cache.rs` is redesigned). Per-component verdicts are in design section 5.10. |
| N61 | velnor-new's `crates/velnor-runner` is superseded as an executor; its journal, crash recovery and ownership proofs, and its wire and lifecycle types where they match the latest APIs, are reused. | Owner (binding) | — | Adopted (v3) | Amended by decision 17: every velnor-new component is reviewed on its merits (the Turso journal and launch state machine are redesigned; `velnor-runner-core`, the scale-set wire types and the Docker spec audit are ported after review). The official-runner wrapper is reused for GitHub Official only if it passes review (decision 19). |
| N62 | No Node and no JavaScript actions: Velnor implements the closed action set of velnor-new's workflows natively in Rust and refuses any other action, or an unqualified SHA, before the job starts; `velnor-gen` guarantees the closed set. | Owner (binding); velnor-legacy's native adapters as precedent | — | Adopted (phase 1) | The set, from `uses:` lines in `.github/workflows/`: actions/checkout (61 uses), upload-artifact (77) and download-artifact (60), actions/cache with restore and save (24), jdx/mise-action (48), jdx/mr-boxington-action (12), actions/attest-build-provenance (8) and actions/attest (1), asamarts/alint (1), and local composite actions. actions/github-script (3 uses) runs JavaScript, so `velnor-gen` replaces it with `velnor-ci` commands; the qualification workflow's Docker and node24 test actions become refusal checks. The implementations use the node's Git mirror, node and site stores and tool store, and their effects must match the upstream actions in the harness. |
| N63 | `docs/goals.md:16` and the lines that assume the official runner change. | Owner (binding) | — | Adopted (phase 0) | Decision taken; the owner edits line 16 ("jobs are always executed by GitHub's official runner"), the vision paragraph (line 20) and design rule 11 (line 75). `docs/hard-decisions.md` binds only the existing engine and names no runner. This log does not edit those documents. |
| N64 | Executor cutover gate and rollback. | Owner (binding cutover rule); synthesis (criteria) | — | Rejected | Overturned by decisions 16 and 18: migration is per job type, each moving as soon as it passes conformance for that type, deleting its old route in the same change, with no moving back; a divergence is fixed forward and a bad release redeploys the previous image digest. |
| N65 | Fork pull requests and macOS release builds on GitHub-hosted runners. | Synthesis (scope question under N57) | — | Rejected | Answered by decisions 8 and 9: no fork pull requests, and macOS release builds are paused, so every velnor-new job runs on Velnor. |
| N66 | A platform abstraction (the `Platform` trait) with declared capabilities; Kubernetes and Docker backends now, microVM and Apple Containerization later; the core branches only on capabilities. | Owner (decisions 13, 21); the `Runtime` trait proposed in velnor-new #1324 | — | Adopted (v3) | The core describes environments by CPU, memory, mounts, network policy and lifetime. Capabilities: kernel shared or separate, how directories enter, shared page cache, snapshot and restore, warm pool, Docker, network policy, CPU pinning, cgroup kill, arch and OS. A missing capability falls back to a slower but correct path. Lifecycle, crash recovery and ownership proofs (Present / AbsentProven / Unknown) are part of the contract; conformance runs per backend. |
| N67 | One `Executor` interface with two executors: Velnor Native (default, recommended) and GitHub Official; the executor identity is part of every result key; the choice is configuration per job type, never an automatic switch. | Owner (decision 20) | — | Adopted (v3) | Results never cross executors, so switching costs one re-verification and cannot produce a false green. Both executors get the same platforms, warm pools, scheduling, label flow, `Required` check, logs, ledger and caches. The design shows the speed difference per job type (design section 5.3), measured by the paired runs. |
| N68 | The official runner as Velnor's permanent reference executor: the conformance gate, sampled shadow runs and on-demand diff. | Owner (decision 19) | — | Adopted (v3) | Reference runs never produce verdicts, receipts, caches or releases, and are never a fallback. |
| N69 | One storage abstraction (`BlobStore` with capabilities) and configurable tiers per data class: local filesystem, RustFS single- or multi-node, any S3-compatible store. | Owner (decision 23) | — | Adopted (v3) | Data classes routed separately: `actions/cache`, artifacts v4, promoted release artifacts, the mbx-cache store, warm-snapshot shipping. Defaults: local only on Docker and macOS; node NVMe plus a RustFS cluster per site on Kubernetes. Correctness lives in Velnor's cache layer (scoping, PR writes never reaching `main`, immutable keys, content verification on read), so a misconfiguration can only make things slower. |
| N70 | Native `actions/cache` with GitHub's exact semantics, branch scoping included; L0 node NVMe, L1 site RustFS, L2 cross-site replication; prefetch by the dispatcher; content-addressed chunks. | Owner (decision 22) | — | Adopted (v3) | Open, to verify in the runner's source: whether GitHub Official can be pointed at a Velnor endpoint compatible with cache service v2 by rewriting the job message's endpoints; until verified it uses GitHub's cache service, correct but slower. |
| N71 | Node-local bare mirrors of each repository, updated by push webhooks plus a catch-up; native checkout verifies the exact commit and tree, reflink-clones a pre-materialised `main` tree and rewrites only changed files; the same pattern for the crates.io index and crates, toolchains, mise tools and container images. | Owner (decision 24) | — | Adopted (v3) | Times follow the warm-snapshot rule, mirrors are read-only to jobs, new nodes seed from a site bundle, and the feature is configurable per platform. Estimated saving at today's volume: about 5 s (checkout), 4.5 s (crates) and 14 s (toolchain and tools) per job, about 62 runner-hours a day (design section 7.4). |
| N72 | Per-job-type migration: a job type moves when it passes conformance for that type, its old route is deleted in the same change, nothing moves back; every step lists its deletions. | Owner (decisions 16, 18) | — | Adopted (v3) | Order relieves the critical path first. ARC's velnor-new scale set and the ARC path that ran production jobs on the official runner are deleted after the last move; the official runner itself stays as GitHub Official and the reference executor (decisions 19, 20). |
| N73 | Review before reuse: every component of velnor-legacy and of velnor-new is reviewed, with a verdict (port as-is, port and refactor, redesign) and its reason. | Owner (decisions 14, 17) | — | Adopted (v3) | The verdicts are in design section 5.10; the delivery plan's review items confirm the ones that depend on unchecked facts. |
| N74 | The required gate is one App-written commit status, `Velnor / Verified`, bound to the App's `integration_id`, on PR heads and merge groups; no bypass actors; no Actions job in the required list. | Claude Opus 5.5 (Rust-only round); ruled Q1, Q2 (GPT-6.2-Astra, GPT-6.2-Sol, Claude Opus 5.5; Gemini 4 Carbon, Claude Fable 5.2, Muse Wonder, Gemini 4 Argon for the transition) | Gemini 4 Neon and Claude Fable 5.2 (reusable-workflow caller); Gemini 4 Argon and Muse Wonder (fail-closed Actions job as the lock) | Adopted (v3) | Ruled: a skipped job passes a required check, also through `needs`; integration 15368 is every workflow, so a PR-edited or dispatched workflow can mint an Actions-bound check (Q8); a status has one passing state where a check run has three. Actions `Required` stays only on push to `main`, for the release gate. Transition in four steps, with both kinds required during step 3. |
| N75 | A syscall read audit per sandbox (opens, stats, directory listings, failed lookups) decides whether a key may be narrower than the whole tree. | Claude Opus 5.5; ruled Q5 | GPT-6.2-Astra and GPT-6.2-Sol: observed reads cannot prove completeness | Adopted (v3) | Ruled: one read outside the closure widens that obligation's key to the whole tree; observed reads may propose declarations but never prove a key complete; a restricted run equalling an unrestricted one is not an enrolment proof. |
| N76 | Release binaries are scanned for banned legacy endpoint paths; Velnor reports as its runner version the newest official release it has passed. | Claude Opus 5.5 | — | Adopted (v3) | Turns decision 3 into a check, and GitHub's 63-day version window into a conformance deadline. |
| N77 | Batch work (builder, speculation, audits) in `cpu.idle` cgroups, frozen with `cgroup.freeze` under memory pressure instead of killed. | Claude Opus 5.5 | — | Adopted (v3) | Keeps the reserved merge path's latency while batch work uses idle CPU. |
| N78 | Release promotion by digest: release-profile bytes built once per closure key at merge time, qualified once on the exact bytes, packed and attested once; a tag runs one job (gate, fetch by digest, re-hash, `gh attestation verify`, publish) in about 15 s. | Claude Fable 5.2 (Rust-only round); also Claude Opus 5.5, Gemini 4 Carbon, Gemini 4 Argon | — | Adopted (v3) | Measured profile trade-off: testing the release build costs 320.8 / 365.7 CPU-s for `generator` / `ci-helper`, against 191.9 / 282.8 for debug tests plus release bytes built once, and a release-profile test build shares only 15 of 90 crates with the shipped binary (`panic = "abort"` against unwinding). Releases today take 190–434 s (tools) and 4,105–6,238 s (product) [M]. Ruling Q6: built once per key in the merge group, stamped with the group SHA, which becomes the `main` SHA. |
| N79 | Patch the commit into a fixed-size section after link instead of `option_env!("VELNOR_COMMIT")`, so release bytes recompile only when their closure changes. | Claude Fable 5.2 | Claude Opus 5.5 (build in the group, whose SHA becomes main's); GPT-6.2-Astra (an exact main build) | Experiment-gated (v3) | Ruled Q6: a lab item, not a default; it is what would make the product build once per closure instead of once per tip. The patched binary must pass the same smoke test and attestation. |
| N80 | The landed merge group's build output becomes the next generation; it is trusted when audit A1's cold rebuild of the same commit gives equal digests. | Claude Fable 5.2 | — | Adopted (v3) | Removes the builder's separate compile of every `main` advance (v2 compiled each change three times: PR, group and builder). |
| N81 | Audit A2: nightly runs with the clock shifted 8 and 30 days ahead through a shim. | Claude Fable 5.2 | — | Adopted (v3) | Finds time-dependent failures before the date arrives; receipts of wall-clock tests already expire in 24 h. |
| N82 | The harness is tested by mutants (at least 95% of about 30 killed), a protocol surface diff of about 40 files runs per official release, and a production-path canary runs every 10 min per site. | Claude Fable 5.2 | — | Adopted (v3) | Ten planted differences (v2) were too few to trust a green comparison; the canary reads every effect back through the API, and three failures in a row page the owner. |
| N83 | Compile reuse with per-unit provenance: sampled compile units are rebuilt from empty and compared, beside A1. | GPT-6.2-Astra (Rust-only round) | — | Adopted (v3) | A stale `OUT_DIR`, proc-macro output or cargo timestamp could turn a bad edit green; sampling units catches it faster than whole cold builds alone. |
| N84 | The first reused pass whose cold run fails revokes its class at once and transitively blocks open heads, queue entries and releases that relied on it. | GPT-6.2-Astra | — | Adopted (v3) | "Statistics discover defects. They never license a suspected false green." |
| N85 | No credential-bearing step runs before the job lease's first renewal succeeds; `lockedUntil` becomes a conservative monotonic deadline. | GPT-6.2-Astra | — | Adopted (v3) | A lease lost before the first renewal cannot leave credentials in use by a job GitHub has reassigned. |
| N86 | Idempotent publication with read-back; crates.io receives the exact packaged `.crate` bytes. | GPT-6.2-Astra, GPT-6.2-Sol | — | Adopted (v3) | A retried release cannot publish twice or publish bytes other than the verified ones. |
| N87 | The candidate cannot weaken its own tests: the expected inventory, planned arguments and feature policy come from the trusted planner, and a plan that drops obligations or tests `main`'s plan has lists them as a reduction in the `Required` summary. | GPT-6.2-Sol (Rust-only round) | — | Adopted (v3) | Differential cases forge passing output and empty discovery; the controller rejects authority the candidate provides. Listing reductions makes a quietly deleted test visible to review. |
| N88 | A third-site witness for any leader the two sites share (the ledger's), beside three k3s servers across failure domains. | GPT-6.2-Sol, GPT-6.2-Astra | — | Adopted (v3) | One k3s server cannot survive the loss of a node or site; two sites cannot elect a leader alone. |
| N89 | An Ed25519 signer daemon outside every sandbox and outside the agent's general process holds each node's receipt-signing keys. | Gemini 4 Argon (Rust-only round) | — | Adopted (v3) | A sandbox or a compromised agent path cannot mint receipts. |
| N90 | Read the broker's `registration_deprecates_at` and `runtime_deprecates_at` fields and alert on them. | Gemini 4 Argon | — | Adopted (v3) | GitHub announces drift in the protocol itself; reading it costs nothing. |
| N91 | Site-wide longest-first fan-out of a suite's test cases when summed time over one sandbox's threads exceeds the longest test plus the transfer cost. | Gemini 4 Argon, Gemini 4 Carbon, Gemini 4 Neon; earlier I067 and N04 | — | Adopted (v3) | Uses the cluster's idle threads only where the arithmetic pays. |
| N92 | One `tests/it` integration binary per package instead of 356 separate test binaries. | Gemini 4 Carbon (Rust-only round) | — | Adopted (v3) | 177 in the workflow renderer, 109 in the orchestrator, 44 in actions-cli and 26 in runner-host today; about 340 link passes saved per cold build. |
| N93 | Replace the subprocess loop of `non_inputs_differential.rs` (about 45,000 `git` processes) with in-process `gix` matching, keeping a 32-case differential against real `git`. | Gemini 4 Carbon | — | Adopted (v3) | A 679 s suite; estimated about 135 s → 2 s for the test, with the differential keeping its meaning. |
| N94 | Corpus floor: at least 240 semantics, 65 fault and 30 refusal cases beside the replayed real jobs. | Gemini 4 Carbon | — | Adopted (v3) | Sizes the harness so a green means coverage, not only the jobs that happened to run. |
| N95 | Parse every protocol message strictly: an unknown field is logged and alerted, never silently dropped. | Gemini 4 Neon (Rust-only round) | — | Adopted (v3) | Protocol drift shows on the first message that carries it, not after a behaviour change. |
| N96 | The compile jobserver's width follows the node's free CPUs at sandbox start (16 up to 64), while nextest's threads stay at the class's fixed CPU count. | Gemini 4 Neon | — | Adopted (v3) | A lone PR on an idle node builds faster without changing any key: compile parallelism changes no output, and the test thread count stays in the key. |
| N97 | Split every looped test (40, 12, 10, 6 and 4 cases) into one test per case and drop `test-threads=1` once tests use private temp directories and ports. | Gemini 4 Neon; earlier I059 | — | Adopted (v3) | The loops, not the cases, set today's floor. |
| N98 | Race a local delta build against a cross-node fill when a node lacks the needed generation; the first verified result wins and the other is cancelled. | Muse Wonder Full (Rust-only round) | — | Rejected | Overturned by ruling Q16: no transport is on the verdict path. A request goes to a node that holds its generation, or builds cold; fills run only in the background. |
| N99 | Deliver by the agent-fleet plan: per-job-type cohorts ordered to relieve the critical path, milestones in days set by the binding constraint (CI and queue throughput, reviews and owner decisions, conformance evidence, capacity), and a deletion list per milestone. | Delivery plan (synthesis), from the owner's decisions 15, 16 and 18 | — | Adopted (v3) | About 560 agent-hours of building and 100 of review, about 2.3 days of fleet time; coding does not set the dates. Committed targets by day 10.5 at P50 (13.5 at P90); stretch measured by day 16–18. |
| N100 | Land the phase-0 configuration first. | Delivery plan | — | Adopted (v3) | It cuts an agent PR's cycle from 94 to about 34 min at P50 and saves about 17 h of CI waiting at P50 (60 h at P90) on a critical path of about 14 dependent PRs [E]. |
| N101 | Per-type cutover criteria in counts: 100% of the type's cases on Kubernetes, every mutant caught, N replayed pairs with no unexplained difference, every step made to fail red as on the oracle, start latency no worse than ARC's at P90. | Delivery plan (D11) | — | Adopted (v3) | Nothing moves back after a move (decision 18), so the bar is set before it; the deterministic made-to-fail check is the zero-false-green test. |
| N102 | The final deletion keeps the official runner's image and launch code. | Owner (decisions 19, 20) amending the delivery plan's F01 | — | Adopted (v3) | The plan, written before decisions 19 and 20, deleted the official-runner image; GitHub Official and the reference executor need it. ARC's velnor-new scale set, the wrapper unless review keeps it, velnor-local's pair mode and the Turso journal are still deleted. |
| N103 | `velnor-ci release-gate` gains `--app-check 'Velnor / Verified'=velnor-verify` and `--push-check Required`; the helper is released and pinned before the ruleset changes. | Adjudication (Q2); GPT-6.2-Sol (one compatibility change, helper first) | — | Adopted (v3) | The gate today rejects every binding but 15368, and its PR-only exemption cannot carry the App status because the group SHA is the `main` SHA (`gate.rs:84-94,277-288`). |
| N104 | Phase-0 ruleset hardening: today's two contexts bound to integration 15368, `bypass_actors: []`, ALLGREEN, depth 2. | Adjudication (Q2, Q10) | — | Adopted (v3) | An admin bypass carried 7 of 136 merges in three days, and three commits with a failed `Required` sit on `main` [M]. |
| N105 | A REST reconciler every 10 s drives verification from state (labelled open PRs and queue branches); webhooks only shorten the delay; labels are never applied with `GITHUB_TOKEN`. | Claude Opus 5.5, Claude Fable 5.2, GPT-6.2-Astra, GPT-6.2-Sol; ruled Q3 | — | Adopted (v3) | Whether App webhooks fire for `GITHUB_TOKEN` label changes is unproven, and GitHub does not redeliver failed deliveries; the three REST reads were verified against the repository, including the `304` path. |
| N106 | The materialiser sets every source time; a planted older-time edit runs as a standing canary; the time rule alone never authorises a hit. | Adjudication (Q16); Claude Opus 5.5; GPT-6.2-Sol | — | Adopted (v3) | Reproduced: a changed file stamped older than the build passes `cargo test` without recompiling; `-Z checksum-freshness` is nightly-only on 1.99.1. |
| N107 | The oracle and every citation at the release tag (v2.338.2); conformance within 14 days of each release; per-version deadlines from the deprecations endpoint, with an alarm 21 days before `runtime_deprecates_at`; a critical update pages at once; the oracle in compare mode with a fuzzer against both official evaluators. | GPT-6.2-Astra, GPT-6.2-Sol, Gemini 4 Argon, Muse Wonder Full, Claude Fable 5.2; ruled Q4, Q15 | — | Adopted (v3) | The panel's source clone is unreleased `main`, 46 commits past the branch point, with three behaviours no release has. 2.337.0 stops on 2026-12-25 and 2.338.0 on 2027-01-04 [M]. |
| N108 | The Docker class: a private privileged daemon per request on tainted nodes, never shared; a skipped or missing Docker test fails the obligation; rootless and microVM daemons are lab D experiments. | Claude Fable 5.2, Muse Wonder, Gemini 4 Neon, Gemini 4 Argon; Claude Opus 5.5 (the experiment); GPT-6.2-Sol (missing tests fail); ruled Q19 | — | Adopted (v3) | Today's isolation, kept for the three binaries that need it; decision 8 removes the hostile author. |
| N109 | Four lab experiments with defaults that stay correct either way: A (GitHub semantics), B (build matrix B1–B4), C (shadow ledger C1–C2), D (Docker class). | Adjudication | — | Adopted (v3) | Design section 11.3 lists each case, its pass condition and the default in force. |
| N110 | Dependency `opt-level = 3` in the dev profile with `debug-assertions` and `overflow-checks` written out as on, and `turso_core`'s own line raised. | Gemini 4 Carbon, Gemini 4 Argon, Gemini 4 Neon (the claim); ruled Q7 | — | Adopted (v3) | Measured with both checks on: 67% less CPU and 74% fewer instructions on the slowest-class test; the checks cost 5% of instructions. No target counts the gain until lab B1. |
| N111 | Match the expression evaluator whose results the latest official runner returns, verified by conformance; port the new evaluator when GitHub switches (group 23: owner question D4 offers A, the default, and B, recommended). | Ruled Q4 (Claude Opus 5.5, Claude Fable 5.2; Muse Wonder Full); coordinator's framing | — | Owner decision | v2.338.2 returns the legacy `Expressions2` result on every path, even in compare mode, so matching it is what conformance requires; it conflicts with decision 3's "nothing legacy". Recommend matching current behaviour, because correctness comes first (design owner question D4). The ruling's alternative, porting the new evaluator now, would diverge from the oracle. |
| N112 | Agents act under an identity without repository administration. | Adjudication (Q2) | — | Adopted (v3) | Today agents label and merge under the owner's identity, which can edit the ruleset that constrains them. |
| N113 | Merge-group tree equality: 16% today; 83, 78, 74, 67, 56 and 37% at 1, 2, 2.5, 5, 10 and 20 min with tip-targeted verification and auto-enqueue; about 60% without auto-merge; plan on 75% at best. | Adjudication (Q11); GPT-6.2-Sol, GPT-6.2-Astra, Claude Opus 5.5 | — | Adopted (v3) | The 78–89% six models quoted came from landings, not queue entries; a group's base changes when another PR enters the queue. No verdict depends on the rate. |
| N114 | `CARGO_INCREMENTAL=0` on every build that mints a receipt or ships bytes; incremental only in lanes that mint nothing. | Claude Fable 5.2, Gemini 4 Carbon, Muse Wonder, Muse Wonder Full, GPT-6.2-Astra, GPT-6.2-Sol; ruled Q17 | Claude Opus 5.5 (on from phase 2) | Adopted (v3) | The cold digest audit does not cover incremental outputs; lab B4 enrols incremental builds only at zero differences. |
| N115 | Release bytes built once per key in the merge group, stamped with the group SHA (the release PR's group for the tools, smoke-tested as shipped as part of its verdict; each group for the product, outside the verdict); tests in the dev profile; no shared `verified` profile. | Claude Opus 5.5, Claude Fable 5.2; ruled Q6 | Muse Wonder Full (`verified` profile) | Adopted (v3) | Cargo ignores `panic` for test targets and the tools ship as musl with `panic = "abort"`, so no test build is the shipped code; the group SHA becomes the `main` SHA [M]. |
| N116 | The first ruleset update sets ALLGREEN, empty `bypass_actors`, both contexts bound to 15368, depth 2, timeout 90 and minimum wait 0, read back field by field; no admin merge in any procedure; a bad release is recovered by redeploying the previous image or a pre-approved break-glass verifier that writes a green only after a full cold pass. | GPT-6.2-Sol (final review), Claude Opus 5.5 (final review) | — | Adopted (v3 final) | Three failed group heads landed under HEADGREEN; decision 12 allows no merge without a green latest commit |
| N117 | Warm builds prove their inputs by content against the generation's manifest; an undeclared change builds the root cold; a dep-info blob check after cargo; a cold second witness until compile reuse graduates; receipts bind every executed binary's digest. | GPT-6.2-Sol, Claude Opus 5.5 (final reviews) | — | Adopted (v3 final) | Cargo can reuse an `OUT_DIR` whose undeclared input changed, and both twins would pass on it |
| N118 | Keys are whole-tree until a reviewed dependency contract, enforced by the read audit on every execution and tested with a sibling-file canary, enrols a narrower one. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | A trace cannot see a branch the run did not take |
| N119 | Verification records with two witness slots (distinct node, attempt, order and generation, or cold); audit re-execution never fills a slot; hedging only for preparation. | GPT-6.2-Sol, Claude Opus 5.5 (final reviews) | — | Adopted (v3 final) | One pass must not count twice, and a hang must not become green |
| N120 | C5 work runs on every request; no time-to-live reuse; an execution fingerprint in every key; instant verdicts only with zero C5 obligations and zero misses. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | A 24 h lifetime can cross the boundary a test asserts on |
| N121 | One fenced status writer per repository with a success index, epoch recheck, read-back, planned freeze and drain, and emergency `failure` writes. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | GitHub keeps a published success whatever the ledger's epoch says |
| N122 | A versioned obligation inventory with expected test IDs; reductions only by owner-signed entries; shadow parity on heads and exact groups before any Actions route is removed. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | 300 head comparisons do not prove every blocking check survives |
| N123 | GitHub Official runs obligations from a synthetic job message against node-agent endpoints, qualified before selection; `velnor-official` is maintained in both roles and its image is never deleted. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | The Worker completes jobs itself (`JobRunner.cs` at v2.338.2); decisions 19 and 20 |
| N124 | One authoritative cache index per repository × scope × key × version, insert-if-absent in the home site; GitHub's lookup order before any blob tier; blobs content-addressed only. | GPT-6.2-Sol, Claude Opus 5.5 (final reviews) | — | Adopted (v3 final) | An L0 prefix could mask an exact entry; two sites could finalise one key twice |
| N125 | Checkout from sealed bases with a full staged-tree hash before the first step. | GPT-6.2-Sol, Claude Opus 5.5 (final reviews) | — | Adopted (v3 final) | A stale unchanged file sits outside the rewrite set; the full hash costs about 0.1–0.3 s |
| N126 | One release authority (`velnor-ci release-gate`) at the tools release and both product entry points, before the ruleset changes. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | The product workflow embeds its own shell gate that never reads the App status |
| N127 | A1 journals one audit per first-parent commit with gap reconciliation, one SLO with a 6 h hard limit; durable product build claims by group SHA. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | An intermediate commit of a group gets no push run |
| N128 | A private Docker daemon per request on both backends, or refusal; memory reserved from measured peaks; restartable batch requeued at the watermark. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | Freezing frees no memory; host-daemon siblings are shared state |
| N129 | Proxy baselines labelled; targets restated as goals until joint traces support them. | GPT-6.2-Sol, Claude Opus 5.5 (final reviews) | — | Adopted (v3 final) | The 85/457 and 26.7/92.3 baselines come from stand-in clocks and cohorts |
| N130 | Shared work inherits its highest live claimant's priority; lab B3 measures the wait, fetch and cold crossover under load. | GPT-6.2-Sol (final review) | — | Adopted (v3 final) | A real group could wait behind its own frozen speculation |
| N131 | A standing canary with the checkout base a commit ahead of the generation and the file changed between them; `inputs restore` is not ported verbatim. | Claude Opus 5.5 (final review) | — | Adopted (v3 final) | `restore_inputs` stamps only files unchanged against the checkout (`inputs.rs:102-151`) |
| N132 | Every per-root contract includes each ancestor's tracked files and `.cargo/`, `.config/`, `.github/`, `.velnor/`; the read audit is eBPF on the file-lookup syscalls, `ENOENT` included. | Claude Opus 5.5 (final review) | — | Adopted (v3 final) | Root `rustfmt.toml` and `clippy.toml` govern `ci-helper/` and `generator/`, which have none |
| N133 | Executor selection, versions and digests are read from the base commit's `.velnor/`, certified identities only. | Claude Opus 5.5 (final review) | — | Adopted (v3 final) | A candidate must not choose its own verifier (C3) |
| N134 | The push `Required` job waits on the ledger's A1 result for its SHA (`velnor-ci audit wait`, 2.5 h, per-SHA concurrency). | Claude Opus 5.5 (final review) | — | Adopted (v3 final) | Push runs are not cancelled (`ci.yml:57-59`); a shared group would queue tips |


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

**Decision (after the design review).** Velnor stops placing jobs and places work. From phase 1
placement is mostly irrelevant: every node keeps a warm snapshot of `main`, refreshed after every
merge (S12), so any node is warm for the `main` scope, and class labels (gate, build, docker) route
workflow jobs under ARC. From phase 3 the push webhook calls `ensure`, and Velnor's own scheduler
puts each obligation on a node (N18). After the executor cutover the dispatcher holds every runner
identity, so it picks the node before it acquires the job (I003, by the owner's decision N57).

**Deciding evidence.** An idle ARC runner already starts the next job 2 s after the previous one
ends (Claude Fable 5.2 review, F4), so a dispatcher alone buys a few seconds per hop, and `ensure`
removes the hops. S6 shows that a dispatcher can do the Listener's job without moving any credential
into a sandbox, which the owner's decision now requires.

### (b) The executor

**Options.** The official Worker only (Gemini 4 Argon; the default for GPT-6.2-Astra and
GPT-6.2-Sol); a native executor for the `velnor-gen` profile (Claude Fable 5.2, Muse Wonder, Muse
Wonder Full; optional for Claude Opus 5.5); a general native worker (Gemini 4 Carbon).

**Decision (owner, binding, 2026-11-19; it overrides the review's choice).** Rust only: Velnor's
native executor runs every velnor-new job, and GitHub's official runner is used in production
neither as the executor nor as a fallback (N57). Its latest release is the conformance oracle in the
lab, and `velnor-conformance` compares the two on every executor change, nightly and within 24 h of
each official release (N58). Only the latest APIs are implemented (N59). No Node and no JavaScript
actions: the closed set of actions velnor-new's workflows use is implemented natively, and any other
action is refused before the job starts (N62). The credit goes to Gemini 4 Carbon's general native
worker (I006, adopted without Node) and to the native lane of Claude Fable 5.2, Muse Wonder and Muse
Wonder Full (I005). ARC and the official runner keep serving velnor-new until the cutover gate
passes (N64).

**What the review had decided, now overturned.** Two executors with one boundary: generated
obligations on Velnor's executor behind `ensure` (N18), and every workflow job on the unmodified
`Runner.Worker`, with the dispatcher, the hot Worker (N05), podless completion (N06) and a Worker
replacement (I005) gated behind the witness. The `ensure` path, the receipts and C1–C7 stay
unchanged; Velnor now executes the job around them.

**"Admit too late."** GPT-6.2-Sol and Gemini 4 Carbon showed that a job cannot be handed to a
different runner after `acquirejob` (a second acquire gets 409, `RunServiceHttpClient.cs:106-107`).
With the dispatcher holding every runner identity, the node is chosen before acquisition, so no
lease moves.

**Evidence, stated both ways (H1, H2).** Measured interpreter cost is 87–147 ms per action (Claude
Fable 5.2 logs), and an idle ARC runner starts a job in 2–3 s; estimated native gains range from 0.5
to 20 s per job, and none was measured. The speed therefore comes from locality and removed round
trips (the node's Git mirror, node stores, no action downloads, placement before acquisition), not
from the language. velnor-legacy built a native lane in 804k lines and archived it without an
accepted speedup (evidence §E.6–7); the conformance harness and the line budgets (I012) answer that
history. The owner chose a fully new stack for control over the whole path; both reasons are
recorded.

### (c) ARC

**Decision (owner, binding).** ARC with the official runner serves velnor-new only until the
executor cutover gate passes (N64): the lab corpus at 100% against the latest official release; two
weeks and at least 300 merges of twin-job shadow with every verdict difference explained; no P90
slowdown per job class; fault injection, the switch to the previous release, and control-plane and
site loss qualified. Until then its template is fixed: class scale sets, warm pools, honest sizes,
no dind where Docker is not needed, node mounts. The cutover change deletes ARC's velnor-new scale
sets; Velnor's dispatcher holds the scale-set sessions from then on (I003), and rollback is the
previous Velnor release, never ARC (I094 and N24 overturned). ChainArgos and jackin keep their ARC
until velnor-new's cutover has soaked for four weeks.

**Deciding evidence.** ARC is not slow today: its jobs queue 0.2 min median (evidence §A T4) and an
idle runner starts a job in 2–3 s (Claude Fable 5.2). The owner's Rust-only decision, not a measured
need, retires it; GPT-6.2-Astra's condition for replacing ARC, a measured need, was not met, and the
record says so.

### (d) The cache

**Decision (owner-approved, after the cluster measurement).** mbx plus a per-site `mbx-cache` is
kept as the cross-node layer: cold nodes about 40 s against 92 s, and lockfile changes on nodes
without a matching snapshot about 43 s, because of per-crate content reuse. It is not the per-job
hot path: a reflinked warm snapshot is 0.3 s, 0 crates and 2.5 MB, and plain cargo beat mbx on a
same-node lockfile change, 67 s against 88 s. Its value depends on one number, the share of build
jobs that land on a node without a warm, matching snapshot. Phase 1 measures it: if it is high
(frequent Renovate lockfile updates, new nodes, evictions), mbx earns its keep; if it is near zero,
the server is retired and plain cargo over snapshots plus incremental warmers remains. **Threshold
(N54):** keep `mbx-cache` while at least 10% of build jobs would otherwise start cold or mismatched
and are served from it, or while it saves at least 20 runner-minutes a day of foreground builds;
retire it after four weeks below both. The honest alternative: incremental warmers mean nodes are
rarely cold, and copying a snapshot from another node moves about the same 8.7 GiB as mbx; mbx's
unique strength is partial, per-crate reuse when dependencies differ.

**Deciding evidence.** S12 on two workers gives the clone, the 0-crate build and the cold,
cross-node and lockfile numbers above; today's full copy onto the ext4 `emptyDir` costs 8.6 s and
10.75 GiB per job; clones do not share page cache, and mbx restores by reflink on XFS as well, so
only hard links would. S11 on the Mac showed why the path is fixed: both engines rebuilt the same 6
workspace crates at a new path. Issue #585, on which the draft chose snapshots, decides nothing at
this size: a 5-crate lab on hosted runners, mbx without its store between ephemeral runs.

**What it needs.** Job work directories on XFS as a phase-1 prerequisite (I095); one fixed checkout
path; the freshness rule (N55), because the measurement took its file times from one archive; a
page-cache budget (N56); and for `mbx-cache` TLS, `MBX_REMOTE_TRUST_WRITES=true` for the builder
outside GitHub, garbage collection on, a disk budget (300 GiB per site to start), and writes only
from trusted builds of `main`. Jobs never run mbx: a job on a node without a matching snapshot takes
the warmer's refill or plain cargo over the older snapshot.

**Who argued what.** Gemini 4 Carbon proposed reflinked target snapshots and raised the page cache
(with overlayfs, now the remedy if the budget is exceeded); Claude Fable 5.2 (H12) and Muse Wonder
Full (H-11) asked for the experiment gate; Gemini 4 Argon raised cargo's target lock and the shared
jobserver; the owner kept mbx unless it measurably lost, and approved its role as the cross-node
layer.

**Trust.** Only trusted builds of `main` write the snapshot generations and `mbx-cache`; warmers
only read, and build jobs never contact the server. A request that can mint a reusable receipt
builds from a trusted generation plus its own delta (F-H16); there is no per-PR scope (I032). Merge
groups never write the trusted scope; their receipts become trusted when their SHA lands. Forks get
nothing. A lineage restart builds cold into a fresh `mbx-cache` namespace.

**The 48 of 48 miss.** S7 confirms the mechanism: the cache version is derived from the path, and
`${{ runner.temp }}` differs between hosted and ARC runners. Fixed absolute paths fix it while the
GitHub cache is still on the cluster's path (phase 0); node snapshots make it moot from phase 1. The
`ImageOS` and path fields in the state key break restores the same way, so fixing only the tip check
would still restore nothing (Claude Fable 5.2 review).

### (e) The merge queue

**Decision.**

- **Depth (after the design review):** `max_entries_to_build` stays 2 until the under-load
  false-failure sampler has data, then moves only when a replay with real arrivals and all failures
  counted shows more merges per hour: about 4 at ≤ 3% false failures, 8 at ≤ 2% with queue-tip
  bypass trees (I075). On the worst day the queue merged about 1.7–1.9 PRs per hour against 7–8
  enqueued, but at today's 8.1% discordance depth 4 disturbs 29% of windows and depth 8 49%.
- **Wait setting:** unchanged; it never applies with a minimum group of 1 (enqueue to merge-group
  run measured 19 s).
- **Auto-merge:** agents enable it when they open a PR (S2: 0 of 40) and stop catch-up merges.
- **Third run:** the push to `main` stops re-verifying a SHA its merge group passed (phase 1) and
  keeps its release and publication duties; a direct push runs everything.
- **Reuse of PR verdicts (after the design review):** merge groups stay full, as hard decision 3
  requires, and are satisfied by lookup: every obligation needs a receipt with its own
  input-addressed key (N01), per Cargo root from phase 2 and per package from phase 3. Exact-tree
  receipts alone are not enough: on the worst day no merge group tested the tree its PR run had
  tested, because `main` moved during hours of waiting, and on quieter days only 18% did (S4). The
  window model (N34) shows why the design changes that: at a 1–5 min window 78–89% of merge groups
  match. Merge-group selection against the queue base is withdrawn as unsound (F-H02), and HEADGREEN
  is stated honestly: C1 holds for group tips, and ALLGREEN is an owner decision (N28). Speculation
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

**Decision (owner, binding, with the owner's clarification).** Code is copied from velnor-legacy
into velnor-new where it targets the latest APIs and passes the oracle, then reviewed, adapted and
owned there from then on (N60): the expression engine (3,565 lines, transcribed from actions/runner
v2.337.0, with its tests), the broker, run-service and Results clients and the scale-set session,
the native checkout and cache adapters, the trust-scope model, and its incidents as regression
tests. Each piece enters with its tests. Its legacy-protocol paths stay out: the pipelines and
`distributedtask` APIs, the v1 cache service, JavaScript sidecars and Docker actions, and the
37k-line executor and runner files as wholes. **velnor-legacy itself is not reactivated:** it stays
archived, gets no new commits, is no git or path dependency of anything, and publishes no shared
crates. velnor-new's `crates/velnor-runner`, which runs the official runner in Docker, is superseded
as an executor (N61); its intent-call-record discipline, crash recovery, UID-based absence and
ownership proofs, and its wire types (`velnor-runner-github`) and lifecycle types
(`velnor-runner-core`) where they match the latest APIs, are reused with their code. Recovery state
stays small: a dispatcher journal of acquisitions and sealed lease records, because renewal and
force-fail need only the job token (S6). A crash-only agent with no journal is rejected: after a
crash `completejob` needs the plan id, job id, run-service URL and token, which exist only in the
job message. `velnor-runner-host` keeps serving the Mac host and local mode until the native
executor serves them (phase 5); the proposal to add a Kubernetes runtime to it is superseded (I011).

**Deciding evidence.** `velnor-runner-host` is 157k lines with about 60 open correctness issues
(evidence §D.10), its tests set the CI critical path, and its unit puts the JIT config, with the
runner key, inside the job container. velnor-legacy's native lane builds and passed 8,172 of 8,222
tests, but it also carries the legacy pipelines timeline API in `protocol.rs` and a cache service
for v1 and v2 clients (evidence §E), which is why pieces are copied and qualified one by one instead
of revived.

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
speculation, scheduled runs) is preempted. They stay on ARC until velnor-new's cutover has soaked
for four weeks, then move to Velnor.

## Hypotheses: final verdicts

| Hypothesis | Verdict | Numbers |
|---|---|---|
| H1. Rust everywhere, including native actions, speeds CI dramatically because native code beats interpreted code | **Fails as stated, and the owner adopted the stack anyway.** The language is not where the time is; locality and removed round trips are. The owner chose Rust only, with no Node and natively implemented actions, for control over the whole path (N57, N62); both reasons are recorded | Interpreter cost 87–147 ms per action [M, Claude Fable 5.2]; action steps 6–17 s per job, mostly network [M]; tool setup 10 s median [M, §A T9b]. The native actions read the node's Git mirror, node stores and tool store and download no action, which recovers those seconds [E]; a native executor's own gain was estimated at 0.5–20 s per job and never measured |
| H2. A from-scratch Rust runner beats wrapping the official runner and ARC | **Adopted by owner decision, with both executors kept.** Velnor's dispatcher replaces ARC, and two executors sit behind one interface: Velnor Native, the default, and GitHub Official, a deliberately chosen alternative and the permanent reference (N67, N68). The measured wins come from the verification path around the executor (receipts, warm snapshots, mirrors), which either executor gets | Job start 12–14 s under ARC on demand, 2–3 s idle [M]; Velnor targets ≤ 1 s P50 and ≤ 2 s P90 from the broker message [T]; per job type the native executor saves 15–30 s of set-up and up to a minute of cache restore [E], replaced by measured pairs from the harness |
| H3. Kubernetes is the right base | **Partly holds.** Right as the fleet layer; wrong as the job scheduler of record, and its defaults work against CI | Pod start about 6 s [M]; `emptyDir` on the ext4 OS disk [M, S10]; 2-CPU requests with 83.7-core bursts [M, §B.2.2]; runner-host 3.7 → 8.8 min under load [M, §A T6] |
| H4. Local caches beat remote ones, and a placement-aware built-in cache is a big win | **Holds for locality; the placement half is weaker than hypothesised; the engine is decided: plain cargo over a reflinked node snapshot on the hot path, mbx and a per-site `mbx-cache` across nodes while they pass a measured threshold.** Every node can hold the `main` caches, so placement mostly decides PR scope and site, not whether work is warm | GitHub cache restore of 1.92 GB took 44–61 s, longer than the cold build it replaced [M, #585]; cross-job reuse on the cluster today 0 [M, §A T9a]; on the Mac mbx and a cloned target cost about the same disk with clones and mbx 41% less without, and both rebuilt the same 6 crates at a new path [M, S11]; on a worker a reflink clone of the warm target took 0.25 s and the build 0.31 s with 0 crates, against 92 s cold and 8.6 s for today's full copy, and a second node filled from `mbx-cache` in 39.9 s [M, S12] |
| H5. Workflows can run only what a change needs | **Holds, and with the queue fixes it is the largest lever.** The biggest removal is not finer selection but never verifying identical inputs twice: merge groups stay full and become lookups by input key; selection relative to the queue is unsound and withdrawn | Push re-runs the merge group's SHA in 120 of 159 merges [M, §A T7] and fails falsely 8.1% of the time [M, S1]; heavy jobs ran in 97.4% of PR runs because build-script crates are forced [M, Claude Fable 5.2; S8]; per-root keys cut the root-jobs merge groups execute from 500 to 125 per 100 merges [E]; median PR verdict ≤ 90 s committed in phase 3, about 35 s estimated [T] |

## v3: the owner's decisions 1–24 and the Rust-only round

After v2 the owner made binding decisions 1–24 for a Rust-only stack, and all nine panel models
redesigned the system under decisions 1–12 (the Rust-only round), compared side by side. v3 applies
decisions 13–24 throughout (group 11), then the surviving ideas and findings of each redesign, one
model per group in order of impact (groups 12–20), then the agent-fleet delivery plan (group 21).
The contested facts (Q1–Q19 of the comparison) were then ruled on evidence; group 22 applies the
rulings, and where a ruling names a lab test the design runs on its safe default until the lab
reports.

### Owner decisions

| # | Decision | Design section | Rows it changed |
|---|---|---|---|
| 1, 20 | Velnor Native by default; GitHub Official a deliberately configured alternative behind one interface; executor identity in every result key | 5.3 | N57, I001; new N67 |
| 2, 19 | The official runner as the permanent reference executor: conformance gate, sampled shadow, on-demand diff; never verdicts, receipts, caches, releases or a fallback | 5.3, 5.7 | N58; new N68 |
| 3 | Latest APIs only | 5.3 | N59 |
| 4 | No Node in Velnor Native; closed native action set | 5.6 | N62 |
| 5, 14, 17 | Review everything; port only well-designed code; redesign the rest | 5.10 | N60, N61; new N73 |
| 6 | Experimental dependencies judged on evidence | 7.3 | N54 (kept), blaze rejected |
| 7 | GitHub stays the platform, merge queue included | 8.3 | — |
| 8 | No fork pull requests | 5.8 | I091, N65 |
| 9 | macOS release builds paused; every job on Velnor | 8.4 | N65 |
| 10 | Build once, verify once, reuse everywhere; releases promote | 4, 6, 8.4 | I032 |
| 11 | Correctness first; four-part soundness case per reuse | 1.2, 6.4 | N01 |
| 12 | CI on demand through the label flow; only a green latest commit merges | 8.2 | N09, N18 |
| 13, 21 | Platform-agnostic core; Kubernetes and Docker backends; capabilities | 5.4 | new N66 |
| 15 | Agent-made work sized by the real constraint | 11 | — |
| 16, 18 | Per-job-type migration; migrate and delete; no moving back | 11 | N64; new N72 |
| 22 | Native, fast `actions/cache` | 7.2 | new N70 |
| 23 | Configurable storage tiers | 7.1 | N54; new N69 |
| 24 | Node-local mirrors of the repository and every upstream | 7.4 | new N71 |

### Rulings on the contested questions

Ruled on evidence (GitHub documentation, code at the release tag, read-only API calls and
reproductions), not by vote. Q13 is closed by decision 7 and Q18 by decision 16.

| Q | Ruling | Confidence; lab | Design |
|---|---|---|---|
| Q1 | A skipped job passes a required check, also through `needs`; a skipped reusable-workflow caller leaves the inner check unreported, but that is undocumented and never the lock | settled; likely | 8.2 |
| Q2 | One required context, the App status `Velnor / Verified`, on PR heads and merge groups; Actions `Required` only on push to `main`; no bypass actors; `gate.rs` first | likely; lab A | 8.2 |
| Q3 | Not established: a REST reconciler every 10 s; webhooks only shorten the delay; no label with `GITHUB_TOKEN` | lab A | 8.2 |
| Q4 | v2.338.2 returns the legacy evaluator's result on every path; the ruling ports the new evaluator now. v3 states both options in owner question D4: A (the legacy port, default until the owner answers, per the manager's instruction) and B (the new evaluator, recommended); with compare-mode refusal both are oracle-equal on every accepted job (group 23) | settled | 5.3, 12 |
| Q5 | Cargo refuses a missing member, and a sparse sandbox turns a real failure green: full tree; keys narrowed only by audited reads; the key holds the exact package selection and features | settled | 6.1 |
| Q6 | Dev-profile tests; release bytes once per key in the merge group with the group SHA, smoke-tested as shipped, promoted by digest | likely | 8.4 |
| Q7 | Dependency `opt-level = 3` holds with assertions on: 67% less CPU; the checks cost 5% of instructions | likely; lab B1 | 8.5 |
| Q8 | A dispatched run's check lands on the head under 15368, so an Actions-bound check cannot be the lock | likely; lab A | 8.2 |
| Q9 | A verdict belongs to a commit; PRs at one SHA share it; no extra machinery | settled | 8.2 |
| Q10 | ALLGREEN now; depth 2, then 4 | settled; likely | 8.3 |
| Q11 | 16% today; 74–83% at 1–2.5 min and 67% at 5 min only with tip-targeted verification and auto-enqueue | likely; lab C1 | 8.3 |
| Q12 | Two executions before merge on different nodes, one cold audit after landing | likely; lab C2 | 1.2 |
| Q14 | About 5 s to a queued job and 8 s to a running one; not on the verdict path | settled | 3 |
| Q15 | v2.338.2; deadlines per version from the API | settled | 5.7 |
| Q16 | Reflink clone by default; overlayfs and transports in a head-to-head; no hard-linked base; the materialiser sets mtimes | lab B2, B3 | 7.3 |
| Q17 | `CARGO_INCREMENTAL=0` on every build that mints a receipt or ships bytes | lab B4 | 7.3 |
| Q19 | A private privileged daemon per request on tainted nodes | lab D | 5.8 |

### Group 11: decisions 13–24 applied

- The executor interface carries two executors and a role (production or reference); the dispatcher
  drives both, and ARC drives neither once the last job type has moved (N67, N68, N72).
- The `Platform` trait and capability model, with Kubernetes and Docker backends, conformance per
  backend, and microVM and Apple Containerization as later implementations of the same trait (N66).
- The `BlobStore` abstraction, tiers per data class and defaults per platform (N69); native
  `actions/cache` on those tiers (N70); node-local mirrors of the repository and every upstream,
  with the traffic and time they save (N71).
- Per-job-type migration with a deletion list per step (N72), and review verdicts for every reused
  component (N73, design section 5.10).
- Contested facts that could change a verdict run on their safe default (the table above), so no
  group depends on a ruling to stay correct.

### Group 12: what v3 takes from Claude Opus 5.5

| Idea or finding | Decision | Why |
|---|---|---|
| `Required` as an App-owned check, queued "Not requested" on unlabelled heads, bound by `integration_id`, no bypass actors | Adopted as the safe default (N74), pending ruling Q1, Q2, Q8 | A skipped job reports success; only the App can satisfy its queued check |
| Measured: cargo 1.99.1 refuses `cargo build -p a` when another member's sources are missing | Adopted: the full tree in every sandbox (N01), pending ruling Q5 | v2's sparse sandbox could not load the workspace |
| Syscall read audit including `ENOENT` | Adopted as detection and enrolment revocation (N75); narrower keys need a reviewed contract it enforces (group 23) | A trace cannot see an untaken branch, so it never licenses a hit |
| 11 test files read the wall clock; no test manifest declares proptest, quickcheck or rand | Adopted: wall-clock receipts expire in 24 h | Time and randomness are inputs (decision 11) |
| `check_run.rerequested` re-runs only failed obligations; `merge_group.destroyed` cancels | Adopted | Label-flow state machine (design §8.2) |
| Release built once in the merge group, so its SHA becomes `main`'s; smoke on the shipped musl bytes under qemu | Adopted as the safe default for Q6 | No rebuild at the tag |
| Binary scan for banned legacy endpoints; newest certified version as `runnerVersion` | Adopted (N76) | Decision 3 as a check; the version window as a deadline |
| `cpu.idle` batch class with `cgroup.freeze` | Adopted (N77) | Batch never delays the merge path |
| Cold tip audit within 2 h | Adopted as A1's P90 window, one SLO with a 6 h hard limit (group 23) | A1 detects after landing; before landing, content-proven inputs and the cold second witness keep a stale generation from minting a green |
| Third-party compiles counted at 2,260 of 463 crates (4.9×) | Recorded in the audit (design §4) | A second count beside 2,106 of 475 |
| Incremental compilation on from phase 2 | Not adopted yet: pending ruling Q17 | Needs a non-incremental audit of equality |
| Twin execution of changed obligations on two nodes | Adopted (N20 as amended) | Today's two pre-merge runs |

### Group 13: what v3 takes from Claude Fable 5.2

| Idea or finding | Decision | Why |
|---|---|---|
| Release promotion by digest; a tag job of about 15 s | Adopted (N78) | Releases rebuild today: 190–434 s and 4,105–6,238 s [M] |
| Profile measurement R3: 320.8 / 365.7 CPU-s to test the release build against 191.9 / 282.8 for debug tests plus release bytes once; 15 of 90 crates shared | Adopted: debug tests, release bytes once (N78); where is pending ruling Q6 | Testing the release build costs more and still does not test the shipped panic strategy |
| Commit stamped after link | Experiment-gated (N79), pending ruling Q6 | Half of all merges stop rebuilding both tools |
| Merge-group output harvested as the next generation | Adopted (N80) | Removes the third compile of every change |
| A1 per landed commit; A2 nightly with the clock shifted 8 and 30 days | Adopted (N81) | A stale generation and time bombs are both found early |
| Harness mutants, a protocol surface diff of about 40 files, a 10-min production-path canary | Adopted (N82) | The comparator itself must be shown to catch differences |
| Background steps (105 uses), sparse checkout (44 uses), input-level refusal, `create-github-app-token` | Adopted in the executor scope and the action set (design §5.6) | v2's executor scope missed them |
| The duplicate-work audit's numbers: 2,106 builds of 475 crates per run, 6.3–21.7 GB per job discarded, state seal 683 s per push, source qualification 1,007 s per event | Recorded in the design's audit (§4) | The basis for each reuse |
| Hedged test shards past P95 | Adopted for preparation only; a raced verification copy is an extra witness that must pass (group 23) | "First result wins" turned a hang into a green |
| `Velnor / Required` behind a skipped reusable-workflow caller, plus a second App check | Not adopted: pending ruling Q1; the App check alone is the safe default | The caller pattern rests on observed, undocumented behaviour |
| Two-slice executor cutover | Superseded by per-job-type migration (decision 16) | — |

### Group 14: what v3 takes from GPT-6.2-Astra

| Idea or finding | Decision | Why |
|---|---|---|
| Per-unit provenance for compile reuse; units reconstructed from empty | Adopted as detection (N83) | Stale build-script or proc-macro outputs are the realistic false green of a warm cache |
| First reused-pass, cold-fail revokes the class and blocks dependants | Adopted (N84) | Zero tolerance for false greens means revocation before statistics |
| First lease renewal before credential-bearing steps; a monotonic deadline | Adopted (N85) | Closes a window where a reassigned job's credentials are in use |
| Idempotent publication with read-back; exact `.crate` bytes | Adopted (N86) | Release retries stay safe |
| Zero secret leaks as a pass criterion | Adopted in the harness's gate | Masking is part of conformance |
| Observed read sets cannot prove completeness | Adopted: keys are whole-tree until a reviewed contract enrols (N75, group 23) | — |
| Revoke an existing green when the label is removed | Not adopted | The owner's flow makes the label start verification, never the gate; a verdict describes its tree |
| PR-scoped admission witness through the required-reviewer rule | Recorded: an alternative under pending ruling Q9 | The safe default keeps the check pending while a peer at the same SHA lacks the label |
| Distinct required samples instead of repeated runs | Recorded under pending ruling Q12 | Two parallel executions are today's count |
| Latest official runner v2.338.2, not the source checkout's v2.338.0 | Recorded under Q15; the harness tracks the release API | — |

### Group 15: what v3 takes from GPT-6.2-Sol

| Idea or finding | Decision | Why |
|---|---|---|
| A candidate cannot weaken its own tests: trusted inventory, arguments and policy; reductions listed | Adopted (N87) | A deleted or skipped test must be visible, not silent |
| v2.338.2 adds broker-session ownership on deletion and a WebSocket connectivity probe | Adopted in the protocol scope (design §5.3) | Decision 3 means the latest release's behaviour |
| A third failure-domain witness for shared leaders | Adopted (N88) | Two sites cannot elect a leader alone |
| Exact `.crate` publication | Adopted (N86) | — |
| Only 7 of 45 PR heads equalled their merge-group tree; v2's replays used integration trees | Adopted as the reason nothing counts on equal trees until a shadow measures it (pending ruling Q11) | — |
| Fix test set-up and transfer cost before buying faster hardware | Adopted in the hardware table | The cheaper fix first |
| App-owned commit status instead of a check run; managed topic refs and a ref-writer gateway for same-SHA aliasing | Recorded under pending rulings Q2 and Q9 | The safe defaults (App check; pending while a peer lacks the label) hold until the ruling |
| Optimised Linux bytes built once on landed `main` | Recorded under pending ruling Q6 | The merge group is the safe default |
| Pure sealed work starts on admission, before the job offer | Already in v3: work starts on the label or labelled push | — |

### Group 16: what v3 takes from Gemini 4 Argon

| Idea or finding | Decision | Why |
|---|---|---|
| Found: `velnor-gen` hard-codes hosted runners for release jobs | Adopted: velnor-gen's review verdict (design §5.10) and the release types' migration | Decision 9 puts every job on Velnor |
| Out-of-pod Ed25519 signer daemon | Adopted (N89) | Receipt keys out of every sandbox |
| Read `registration_deprecates_at` and `runtime_deprecates_at` | Adopted (N90) | Drift announced by GitHub itself |
| Site-wide longest-first fan-out | Adopted (N91) | Where the arithmetic pays |
| Hard-linked immutable `deps/` base to share page cache | Recorded under pending ruling Q16 | Reflink with a page-cache budget is the safe default |
| An always-run Actions `Required` job that fails closed | Recorded under pending ruling Q2 | The App check is the safe default |
| Pre-registered JIT identities; placement before acquisition | Already in v3 (design §5.5) | — |

### Group 17: what v3 takes from Gemini 4 Carbon

| Idea or finding | Decision | Why |
|---|---|---|
| 356 integration-test binaries into one per package | Adopted (N92) | About 340 link passes per cold build |
| `gix` in place of about 45,000 `git` subprocesses in one test | Adopted (N93) | A 679 s suite, with a differential kept |
| Corpus of 420 cases: 85 replayed, 240 semantics, 65 fault, 30 refusal | Adopted as the corpus floor (N94) | — |
| Latency floors: about 58 s with an Actions `Required` job (GitHub's job offer about 8 s), about 47 s with an App check | Adopted in the latency budget (design §3), pending ruling Q14 | The App check removes the offer from the verdict path |
| Multi-lower overlayfs with zstd layer streaming | Recorded under pending ruling Q16 | Reflink is the measured safe default |
| Firecracker microVMs for the Docker suites | Recorded under pending ruling Q19; the platform trait admits a microVM backend later | Privileged dind on tainted nodes is today's behaviour |
| A pre-push git hook streaming a thin pack before `git push` ends | Not adopted | It saves under half a second against a 1–2 s delivery |
| Every landed commit within 6 h; 5% re-execution | Superseded: A1 within 2 h at P90; 2% after about 3,000 clean | — |

### Group 18: what v3 takes from Gemini 4 Neon

| Idea or finding | Decision | Why |
|---|---|---|
| Strict parsing of protocol messages; unknown fields logged | Adopted (N95) | Drift detected on the first message |
| A lone job bursts to 48–64 threads | Adopted for compile width only (N96) | Test threads stay keyed |
| Every looped test split; `test-threads=1` removed | Adopted (N97) | — |
| A 10 s watchdog that relabels with the App when a label run is suppressed | Recorded under pending ruling Q3 | The reconciler every 30 s is the safe default |
| `ci / Required` through a skipped reusable-workflow caller | Recorded under pending rulings Q1, Q2 | The App check is the safe default |
| Overlay sandboxes, six per worker, sharing page cache | Recorded under pending ruling Q16 | — |
| Per-organisation floors and disk caps | Already in v3 as quotas and store budgets | — |
| Agents' workspaces hosted on the cluster | Not adopted in v3 | Outside this design's scope; agents verify through `velnor verify` |

### Group 19: what v3 takes from Muse Wonder Full

| Idea or finding | Decision | Why |
|---|---|---|
| Race a local rebuild against a cross-node fetch, chosen by prediction | Adopted (N98) | A duplicate that is measurably faster |
| Label-flow states as conformance cases | Adopted in the corpus (design §5.7) | — |
| A custom `verified` profile (opt-level 2, assertions on, unwind, no LTO) shared by tests and shipped bytes | Recorded under pending ruling Q6 | Debug tests plus release bytes once is the safe default |
| An always-failing guard job beside a label `if:` | Not needed: the App check is the safe default for Q2 | — |
| Sandboxes carved as cgroup slots by a privileged agent instead of a pod per job | Recorded as a Kubernetes-backend option behind the platform trait | The core does not care how a backend makes a sandbox |
| The Worker evaluates expressions twice, with a compare mode | Recorded as evidence for pending ruling Q4 | — |
| File-delta sync as the default cross-node transport | Recorded under pending ruling Q16 | `mbx-cache` keeps its owner-approved threshold |
| Every landed tree in full within 24 h | Superseded by A1 within 2 h at P90 | — |

### Muse Wonder: no separate group

Its surviving ideas are already in v3 from other groups or earlier rounds, so it gets no group of
its own: the closed native action set with refusal before the job and qualified SHAs (N62), the
reserved merge path (N38), per-class reporting (design §1.1), one builder per `main` tip with the
profile in its key (superseded by N80), and three parallel executions (recorded under pending ruling
Q12). Its "not requested" job that fails with a distinct gate reason is moot once `Required` is an
App check that stays pending (N74).

### Group 21: the delivery plan

- The phases are replaced by the agent-fleet plan (N99): per-job-type cohorts, milestones by day
  with the constraint that sets each date, and a deletion list per milestone; the phase-0
  configuration lands first (N100).
- Per-type cutover criteria are counted before each move (N101); after it, sampled shadow runs
  continue and a divergence is fixed forward.
- The plan's final deletion is amended by decisions 19 and 20: the official runner's image and
  launch code stay (N102).
- The first ten work items are the plan's first ten, with scope, acceptance and dependency.
- The owner questions are the plan's decision pack minus what the owner already decided: D13 (park
  the old runner's PRs) by decision 16, and D6 (the lab's reference executor) by decision 19.

### Group 22: the rulings applied

All 46 changes the rulings require are applied in this one group; each keeps its credit here.
Section numbers below are v3's.

| # | Change | Credited | v3 section |
|---|---|---|---|
| 1 | The App status `Velnor / Verified` on PR heads and merge groups; no conditional job is ever a gate | GPT-6.2-Astra | 8.2 |
| 2 | One real Actions `Required` on the `main` push for the release gate, recognised explicitly | GPT-6.2-Astra | 8.1, 8.2 |
| 3 | The oracle and citations at tag v2.338.2; a critical update alarms at once | GPT-6.2-Astra | 5.7 |
| 4 | Depth 2, then 4 on a measured gain | GPT-6.2-Astra | 8.3 |
| 5 | Observed reads propose declarations, never prove a key complete; the full tree first | GPT-6.2-Astra | 1.2, 6.1 |
| 6 | No Actions run on PR pushes; the context bound by `integration_id`; `merge_group.destroyed`; reconciliation | Claude Opus 5.5 | 8.1, 8.2 |
| 7 | The evaluator: written as "match what the latest runner returns; port the new one when GitHub switches", plus owner question D4 (the ruling said port the new one now) | Claude Opus 5.5 | 5.3, 14 |
| 8 | No sparse checkout; full tree plus a read audit with failed lookups and listings | Claude Opus 5.5 | 1.2, 6.1 |
| 9 | Changed obligations twice, on two nodes | Claude Opus 5.5 | 1.2 |
| 10 | Release bytes in the merge group with the group SHA; smoke on shipped musl bytes; no compile at the tag | Claude Opus 5.5 | 8.4 |
| 11 | Dev-profile tests; no shared profile | Claude Opus 5.5 | 8.4 |
| 12 | The planted older-mtime edit as a standing canary | Claude Opus 5.5 | 6.4 |
| 13 | `gate.rs`, the release step and pins migrate as one change, helper first | GPT-6.2-Sol | 8.2 |
| 14 | The detail view is a check run with a different name | GPT-6.2-Sol | 8.2 |
| 15 | Tree-equality figures from landings do not transfer; the entry replay and lab C | GPT-6.2-Sol | 8.3 |
| 16 | The plan from the released helper; a candidate cannot weaken its own gate | GPT-6.2-Sol | 1.2 |
| 17 | A missing, duplicate or skipped expected test fails, Docker tests included | GPT-6.2-Sol | 5.8 |
| 18 | The mtime rule alone never authorises a hit; the cold digest comparison stays | GPT-6.2-Sol | 6.4 |
| 19 | Permanent unmergeable canary PRs probed every 10 min; lab A as the proof | Claude Fable 5.2 | 5.7 |
| 20 | The copied `Expressions2` port as the harness's reference; a fuzzer against both official evaluators | Claude Fable 5.2 | 5.7, 5.10 |
| 21 | The cold audit of every landed commit is the third execution, reported by the push `Required` | Claude Fable 5.2 | 6.5 |
| 22 | The measured 5 s to queued and 8 s to running replaces "about 15 s" | Claude Fable 5.2 | 3 |
| 23 | `CARGO_INCREMENTAL=0` on every receipt-minting build | Claude Fable 5.2 | 7.3 |
| 24 | The stamp after link is a lab item | Claude Fable 5.2 | 8.4 |
| 25 | Phase 0: `bypass_actors: []`, 15368 bound, ALLGREEN | Adjudication | 8.2, 11.2, 12 |
| 26 | `gate.rs` gains `--app-check` and `--push-check` | Adjudication | 8.2 |
| 27 | Tree equality 83, 74–78, 67, 56 and 37%, with its two preconditions | Adjudication | 8.3, 3, 4, 6.3 |
| 28 | The source clone is unreleased `main`; deadlines from the deprecations endpoint | Adjudication | 5.7 |
| 29 | The materialiser sets mtimes; the false green reproduced; `-Z checksum-freshness` nightly-only | Adjudication | 6.4, 7.3 |
| 30 | A receipt key carries the exact package selection and features | Adjudication | 6.1, 6.4 |
| 31 | Dependency `opt-level = 3` with assertions on; both checks written out; `turso_core` raised | Adjudication | 8.5 |
| 32 | The reconciler's REST reads verified, including `304`; a workflow that labels mints an App token | Adjudication | 8.2 |
| 33 | Agents get an identity without repository administration | Adjudication | 5.8, 8.2 |
| 34 | A verdict belongs to a commit; no PR-scoped witness, no ref gateway | Adjudication | 8.2 |
| 35 | Both contexts required during the transition; absence blocks | Gemini 4 Carbon | 8.2 |
| 36 | Overlayfs and compressed layers as lab B candidates; no hard-linked base | Gemini 4 Carbon | 7.3 |
| 37 | Each obligation fixes its package selection (feature unification) | Gemini 4 Neon | 6.1 |
| 38 | The page-cache cost of clones sets the per-node limit until lab B2 | Gemini 4 Neon | 9 |
| 39 | The four-way `opt-level` matrix before any target counts the gain | Gemini 4 Neon | 8.5, 11.3 |
| 40 | `runtime_deprecates_at` read, with an alarm 21 days ahead | Gemini 4 Argon | 5.7 |
| 41 | The generator's hard-coded hosted release runners go in phase 0 | Gemini 4 Argon | 5.10, 11.2 |
| 42 | The oracle in compare mode; cases where the official evaluators differ are kept | Muse Wonder Full | 5.7 |
| 43 | The transport head-to-head with a kill criterion | Muse Wonder Full | 7.3, 11.3 |
| 44 | The Docker class with a private privileged daemon per request | Muse Wonder | 5.8 |
| 45 | "Not requested" is its own state, excluded from false-failure accounting | Muse Wonder | 1.3, 8.2 |
| 46 | Before the App exists, the only Actions label gate is an always-run job that fails without the label | Muse Wonder; Gemini 4 Argon | 8.2 |

Rows changed: N20, N28, N54, N74, N75, N79, N98; new N103–N115. The v3 figures the rulings moved:
merge-group tree equality (16% today, about 75% at best with auto-enqueue), GitHub's job offer
(about 5 s to queued, 8 s to running), the oracle version (v2.338.2) and its deadlines.

### Group 23: final review by GPT-6.2-Sol

Twenty-one findings, all verified against the text and the code; twenty fixed, one fixed in part.
Rows N116–N130 are new.

| # | Finding | Decision | v3 section |
|---|---|---|---|
| A1 | Phase 0 raised depth to 4 and kept HEADGREEN until V04; admin merges in bootstrap and D15 | Fixed: one first ruleset update (ALLGREEN, empty bypass, 15368, depth 2, timeout 90, wait 0) with read-back; phase 0 merges through the queue; D15 redeploys or uses a break-glass verifier that writes a real green (N116) | 5.9, 8.2, 8.3; plan 3.4 |
| A2 | Warm builds could certify an old binary | Fixed: inputs proven by content against the generation's manifest; undeclared changes build the root cold; dep-info blob check; cold second witness until graduation; receipts bind binary digests (N117) | 1.2, 6.4, 7.3 |
| A3 | A read trace cannot authorise a narrow key | Fixed: whole-tree keys; enrolment only by a reviewed contract the audit enforces on every execution, with a sibling-file canary (N118) | 1.2, 6.1 |
| A4 | One pass counted twice; first hedge result wins | Fixed: verification records with two witness slots from distinct nodes, attempts, orders and generations; audit re-execution never fills a slot; hedging for preparation only (N119) | 1.2, 6.2 |
| A5 | A matching tree bypassed fresh work; 24 h wall-clock lifetime | Fixed: C5 runs on every request; no time-to-live reuse; the execution fingerprint in every key; instant verdicts only for vectors with zero C5 and zero misses (N120) | 1.2, 6.1, 6.3 |
| A6 | Revocation does not reach GitHub's queue | Fixed: one fenced writer per repository with a success index, epoch recheck and read-back; planned freeze and drain; emergency `failure` writes that eject groups; lab A partition case (N121) | 6.5, 8.2 |
| A7 | Gate parity checked per head only | Fixed: a versioned obligation inventory with expected test IDs; owner-signed reductions; shadow on heads and exact groups, failures included; a separate parity report (N122) | 1.2, 8.1 |
| A8 | Official could not run obligations; the plan deleted its production route | Fixed: `Work::Obligation` with a synthetic job message and node-agent endpoints, receipts from observation, qualified before selection; `velnor-official` kept in both roles; F01 keeps the image (N123) | 5.3, 5.7; plan O-items, F01 |
| A9 | Cache content integrity is not cache authority | Fixed: one index per repository × scope × key × version, insert-if-absent in the home site; GitHub's lookup order before any blob tier; prefetch re-resolved; receipts never restore `actions/cache` (N124) | 7.1, 7.2 |
| A10 | The checkout verified a recorded label | Fixed: sealed bases and a full staged-tree hash before the first step; sparse, submodule and symlink rules; four tested cases; pinned upstream objects (N125) | 7.4 |
| A11 | Product publication used the old gate | Fixed: one released `velnor-ci release-gate` at the tools release and both product entry points before the ruleset changes; only PR and group Actions gates are deleted (N126) | 8.4, 8.2 |
| A12 | A1 and product builds had no durable work contract | Fixed: journalled audit per first-parent commit with gap reconciliation; one SLO, 6 h hard limit; durable product claims by group SHA (N127) | 6.5, 8.4 |
| A13 | Two label cutover specifications | Fixed: one state machine with permissions, webhooks, arming, reconciliation and lab cases; the plan points to it; carried draft labels are owner question D16; the interim step's authority is stated | 8.2; plan L-items |
| A14 | The default evaluator conflicts with decision 3 | Fixed in part: D4 states both options and recommends B; the plan builds both and ships one. The default stays A, by the manager's binding instruction to follow D4 (match the latest runner) until the owner answers | 5.3, 12; plan 2.4 |
| A15 | The test-floor shortcut cut the Git oracle | Fixed: the full differential stays until its paths are deleted or a matcher is qualified on the whole corpus; speed-ups keep its assertions | 8.5; plan T05 |
| A16 | Docker daemon ownership and freezing under memory pressure | Fixed: a private daemon per request on both backends, or refusal; reservations from measured peak; requeue at the watermark, freeze for CPU only (N128) | 5.4, 5.8, 9 |
| A17 | Estimates borrowed labels from other workloads | Fixed: proxies labelled; separate clocks; goals instead of committed targets; acquisition off the verdict path; 15 s promotion an estimate to be timed (N129) | 1.1, 1.4, 3, 8.4 |
| A18 | Savings charged duplicates selectively; depth 4 funded replay | Fixed: per-PR arithmetic with twins, misses, A1, product bytes and fresh work; ≤ 50 goal and ≤ 20 stretch; depth-2 throughput; the mirror multiplier marked an upper bound | 4, 7.4, 8.3 |
| A19 | The Native/Official delta charged ARC behaviour | Fixed: the difference marked unmeasured; paired runs on the same backend and stores; Worker and Node costs itemised | 5.3; plan O04 |
| A20 | Eligible merges could wait behind their own speculation | Fixed: shared work inherits the highest claimant's priority and moves to reserved capacity; lab B3 measures wait, fetch and cold crossovers (N130) | 6.2, 7.3, 9 |
| A21 | The DAG could not produce the promised system | Fixed: plan v2 with 121 items (Official, storage, cache index, RustFS, mirrors, ledger, witnesses, read audit, A1, A2, parity, release authority, canaries), the evidence matrix, smoke tests before deletions, recomputed dates; the `executor/` root stated once | 5.2, 10; plan |

### Group 24: final review by Claude Opus 5.5

Twenty-one findings, all verified. Eight were already fixed by group 23 and are cross-referenced;
twelve are fixed here; one (B21) is fixed by group 23 in a stronger form, and its own remedy is
rejected. Rows N131–N134 are new.

| # | Finding | Decision | v3 section |
|---|---|---|---|
| B1 | A hedged shard turns a hang green | Fixed by group 23 (A4); here a soundness row for hedging and the corrected log row | 6.2, 6.4 |
| B2 | The plan builds a check-run gate | Fixed by group 23 (A13, A21): plan L02 writes the status, the detail check is `Velnor / Details` | plan L02 |
| B3 | The plan predates decisions 19–24 and the rulings | Fixed by group 23 (A21, A8, A1, A12, A13): plan v2, Official kept, the missing items added, numbers aligned | plan |
| B4 | Compile reuse skips its shadow; twins share a generation | Fixed by group 23 (A2, A4): cold second witness until graduation, dep-info blob check | 7.3, 6.2 |
| B5 | The mtime diff base is ambiguous | Base fixed by group 23 (A2); here the `inputs restore` warning and a canary with the checkout base ahead of the generation (N131) | 7.3 |
| B6 | Per-root keys miss root configuration; no audit mechanism | Fixed: every per-root contract includes each ancestor's tracked files and `.cargo/`, `.config/`, `.github/`, `.velnor/`; the eBPF audit named; plan V07, V08 (N132) | 6.4; plan |
| B7 | D15 admin merge | Fixed by group 23 (A1) | 5.9, 12 |
| B8 | The candidate may choose its own verifier | Fixed: executor selection from the base commit, certified identities only; plan G05 (N133) | 5.3 |
| B9 | Targets and work on code day 7 deletes | Fixed: `Journal::open` target, T01 and the runner-host split dropped; the floor restated on surviving suites; cohort 3 moves only what review keeps; PR-class mix recomputed after F01 | 1.4, 8.5, 10.1; plan |
| B10 | D4 omits the compliant option | Fixed by group 23 (A14) | 5.3, 12 |
| B11 | Push `Required` kept and deleted; mechanics undefined | Contradiction fixed by group 23 (A11); here `velnor-ci audit wait`, a 2.5 h timeout and a per-SHA concurrency group (N134) | 8.1; plan G04 |
| B12 | Runner-minute targets lack arithmetic | Fixed by group 23 (A18); product bytes under durable claims (A12) | 4 |
| B13 | A1 can starve | Fixed: a reserved A1 slot per site; the SLO stated at 95% utilisation; a 2 h lag alarm | 6.5 |
| B14 | Twin orders and fan-out unspecified | Fixed: seeded batches through nextest filters, no order claimed until built; shard layout in the manifest; no receipt fan-out until lab C | 6.2; plan V03 |
| B15 | Disagreement ordering contradicts revocation | Fixed: the first disagreement suspends at once; five re-runs decide flake or revoke | 1.2 |
| B16 | The cross-executor cache case cannot pass yet; artifacts unclear | Fixed: the cache case waits for the endpoint; artifacts go to GitHub's v4 service with a read-through tier | 5.6, 5.7 |
| B17 | Checkout tree verified on a sample | Fixed by group 23 (A10) | 7.4 |
| B18 | Executability gaps | Fixed: queue-tip conflict verifies against `main` (group 23's state machine); failed records never reused; A1 compares cold digests at the fixed path; the `.velnor/` schema (G05); a no-`.git` census (V14) | 1.2, 6.2, 6.5, 8.2 |
| B19 | Numbers | Fixed: acquire 13 / 27 s (V5); 238–289 runner-hours; the 2 s kill is not in the `ci` profile; the owner table no longer splits | 1.3, 3, 7.4, 12 |
| B20 | The interim label flow restarts on any label | Fixed: named as an accepted interim cost; agents and Renovate label at creation only | 8.2; plan B07 |
| B21 | Storage invariants across sites | Fixed by group 23 (A9): names live in one home-site index, blobs are content-addressed. B's site-local first-writer-wins names are rejected: two sites could then bind one key to two manifests, which breaks key immutability | 7.1, 7.2 |

## Design review

The eight design critiques and the comparison that verified them are the input here; ids are the
comparison's. Group 9 applied the cluster cache measurement (S12). Each row says how the final
design answers the finding, and in which review group it was applied. A rejected recommendation says
why.

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
| F-H01 | HEADGREEN lets a failed entry land, so C1 fails for intermediate commits | 2 | C1 restated for tips; intermediate commits listed for bisection and release tooling; ALLGREEN is owner question 3 (N28) |
| F-H02 | Merge-group selection against the queue predecessor never selects a failed predecessor's change | 2 | Withdrawn (I054); merge groups stay full and are satisfied by key lookup; owner question 2 restated |
| F-B07 | Input-addressed keys now; selection becomes a lookup | 2 | Adopted: per root in phase 2 with the ledger moved there, per package in phase 3 behind the sparse-checkout experiment (N01) |
| F-H03 | An exact-tree receipt cannot satisfy a full merge group | 2 | Every obligation of the consuming plan needs a receipt with its key; a missing or zero-test report fails (I055) |
| F-B06 | Commit the targets on the short-window regime | 2 | Window model added to §6.6; targets restated from the review's consensus (N34) |
| F-H06, F-H07 | Cancelled-run stall and `edited` restarts | 2 | Admission span measured from phase 0; the witness removes the `always()` cause; `edited` dropped at the cutover (N33) |
| F-B09 | One execution per key: in-flight join and receipts as obligations finish | 2 | Adopted with the ledger (N29) |
| F-H30 | The heaviest root would run in the smallest class | 2 | Docker-needing test binaries are their own obligation beside the build (I084) |
| F-H17 | Signing and trust boundaries overlap | 2, 3 | Signer outside pods, append-only log, verification before a verdict, degraded mode (N30); class-bound keys in group 3 |
| F-B10 | Velnor lands the merge itself | 2 | Owner question 11, a phase-4 shadow first (N31) |
| F-B19 | Repeat changed tests; one counted retry for untouched ones | 1, 2 | Three runs for changed obligations (N20); the counted retry is owner question 12 (N32) |
| F-B18 | Split the 157k-line crate; one test binary per package | 2 | Adopted in phase 2; incremental state experiment-gated (I073) |
| F-H10, F-H12 | Closure keys miss spawned executables and assume bit-identical builds | 2 | Input keys need neither; digests only audit (N02); digest coverage detail in group 5 |
| F-H08 | C4 let a job's own report back a reusable receipt | 3 | Exception deleted; only agent-observed results with the expected inventory create receipts; forged-report lab test (N35) |
| F-H09 | The key leaves out event, ref, environment and runtime inputs | 1, 3 | C5 widened in group 1; the execution manifest joins every key (N36) |
| F-H17 | Signing and trust boundaries overlap | 3 | One key per node and class outside pods and the agent's service; docker keys never sign trusted; sandbox-scoped socket; audits on another node (N30) |
| F-H24 | Acquisition and failover gaps; one control plane | 3 | Intent before the call, epoch fencing, fault injection (N37); control-plane and site loss qualified before the phase-3 cutover (I093) |
| F-B20 | Reserve capacity for the merge path | 3 | One build slot and 2 gate CPUs per site; wait clock from eligibility; committed p90 ≤ 2 s (N38) |
| F-H19 | Completing a job without a pod is more than `completejob` | 3 | Synthetic completion limited to a versioned generated job with its whole contract proven (N06) |
| F-H18 | Disabling reuse does not reach posted greens or a partitioned site | 3 | Revocation epoch in every verdict; the released verifier stays authoritative during upgrades (N39) |
| F-B21 | PR-light, queue-full as the default | 3 | Rejected: it moves failures into the serial queue (N15) |
| F-H39 | Clock, cohort and sample size cannot support the targets | 3 | Clock at the accepted push; every PR class; 200 matched heads for a P90 (N40) |
| F-H03 | An exact-tree receipt cannot satisfy a full merge group | 2, 4 | The consuming inventory is recomputed from the protected policy and the actual event, and matched per obligation, configuration and test inventory (I055) |
| F-H33, F-B17 | The test floor behind the two loops | 1, 4 | Measured floor (125–138 s, a 924.8 s test) and the method: valid prefixes built once and cloned per case (I062) |
| F-H34 | The flake budget cannot be verified as written | 4, 7 | S1 called a discordance rate; pairs in both directions under load; no bound before about 300 pairs (N14, I119) |
| F-H29 | The warm pool does not fit a worker | 4, 5 | One budget for idle and running sandboxes; 24 GiB build requests, four per worker (I087) |
| F-H20 | The release gate needs a push run with one Actions `Required` job | 4, 6 | The push run keeps that job and skips per obligation (I053); release-gate requirements written out in group 6 |
| — | A corpus of known-bad changes; guardrails and accounting | 4 | Adopted (N41, N42, I117) |
| F-H23 | Free-slot capacity contradicts the protocol; merge-first admission untested | 5 | One scale set per event class and capability from phase 0; total capacity; lab test of offers (I015, I079) |
| F-H10 | A digest key misses executables the test spawns | 2, 5 | Input keys need no digest; where one joins a key it covers every spawnable binary (N02) |
| F-H29, F-B13 | Pod sizes and parallelism; slots instead of pods | 4, 5 | Explicit `CARGO_BUILD_JOBS` and test threads (I106); elastic slots and overlayfs as experiments (I082, candidate (c)) |
| F-H22 | Fork PRs would be refused instead of running hosted | 5 | Fork-conditional `runs-on` (I091) |
| F-H19 | A podless completion must carry a log | 3, 5 | It uploads and seals a log and sends one step result (§3.5) |
| F-H36 | The 2 s kill is in the default profile | 5 | Corrected; the generated workflow names its profile (N44) |
| F-H32 | Kubelet root move and read-only toolchain side effects | 5 | Fixed-size eviction thresholds, project quotas, toolchain mount tied to the workflow (I095, I024) |
| F-H43 | `cargo-deny` reads live advisories | 5 | Advisory database pinned in the repository (N43) |
| F-H05 | Depth 8 under HEADGREEN multiplies one failure | 5, 7 | The bisection herd is named; depth 8 waits for queue-tip bypass trees (§6.5); the gate on depth in group 7 |
| F-H25 | Lease renewal details for a dispatcher | 5 | Renew from `lockedUntil`; journal what a standby needs (N10) |
| F-H20 | The release gate needs one Actions `Required` job in a push run | 4, 6 | Written down in §6.4: integration 15368, exactly one `Required` job in `ci.yml`'s push run, the file name pinned; an App verdict would change `gate.rs`, the release workflows and the publisher together (I013) |
| F-H28 | Parallel lanes serialise on cargo's lock | 6 | One target lane per check kind, pre-built metadata, one CPU budget across lanes (N45) |
| F-H27 | Clones made before the job are wiped; warmer paths drift | 6 | The view is made after the sparse checkout; the builder builds at the request path (N46) |
| F-H41 | The cutover lacks fixtures, the mise pin and the profile | 6 | Added to work item 9 (I056) |
| F-H25 | Lease record fields; the upstream renewal loop's null dereference | 6 | Record fields listed; the loop is written fresh (§3.5) |
| F-H26 | The hot Worker races and starts before `.runner` exists | 6 | `.runner` and a public-only `.credentials` at pod creation; frozen, not recycled; timed first (N05) |
| F-H44 | Enforcement through the candidate's nextest wrapper | 6 | The trusted helper creates one namespace per test binary (N47) |
| F-B18 | `velnor-runner-local` extracted | 6 | Both configurations run; the large crate compiles once (I063) |
| F-B03 | Complete plan and verdict jobs in memory | 6 | Kept behind the gated dispatcher, with a one-day outputs experiment first (N06) |
| — | `cache-prune.yml` | 6 | Retired with the state seal (I029) |
| F-B04 | One real job with the obligation graph inside | 7 | Adopted for phase 2: one job named `Required` calls `ensure`; phase 3 turns it into a pure witness (N50, I056) |
| F-H14 | The reuse audit is too weak and one flake trips it | 7, 8 | Every landed tree executed once in full within 24 h (N48); new reuse classes shadowed at 100% until about 3,000 clean reuses per obligation kind; five re-runs before a disagreement counts; a confirmed one retires the key version; the bound is reported (I034) |
| F-H19 | Synthetic completion before checks are bound | 7 | Binding the required checks (N12) is a precondition of any synthetic completion |
| F-H12 | Nothing pins reproducibility | 7 | The reproducibility gate joins work item 1 (N22) |
| F-H34 | Quarantine in a non-blocking lane lowers confidence | 7 | Quarantine only with a blocking equivalent or an expiring owner exception (N14) |
| F-H38 | Clone and restore numbers are lab numbers | 7 | The phase-2 gate publishes view set-up time on `velnor-runner-host` by file count |
| F-H05 | Queue depth gated on a per-run rate | 7 | Depth from a replay with all failures at the under-load rate; 2 until data (I075) |
| — | Ledger observability; hardware trigger | 7 | Dashboards and the placement kill rule before phase 3 (N49); the high-clock rental in phase 1 and a worker trigger now (I104, I105) |
| F-B23 | Docker without a privileged sidecar | 7 | Experiment-gated, phase 3 (N51) |
| F-H11 | Making undeclared files absent is not fail-closed | 8 | Tree hashes include directory membership; the minting run logs `ENOENT` lookups outside the declared set; a class enrols only at a 100% enforced pass that matches an unrestricted run (N01, M4) |
| F-H13 | A file-read audit cannot make selection sound | 8 | Telemetry only; an obligation is skipped only on a key that proves its inputs unchanged (I069) |
| F-H39 | Gate sample sizes | 3, 8 | Phase gates on at least 300 merges or four weeks, with intervals beside each P50 and P90 (N40) |
| F-H40, F-H31 | Cancelled, superseded and batch work unpriced; every node rebuilding `main` | 8 | Runner-minutes split into completed, cancelled and superseded, with builder, speculation and audit CPU charged per merged PR (I117); build once per site and replicate (I030, I044) |
| F-B15 | Build each `main` generation once and replicate it | 8 | Adopted: one builder per site; overturns rebuilding on every node (I030, I044) |
| F-H26 | Hot-Worker delivery races | 6, 8 | A delivery handshake and kill-at-the-boundary fuzz join the hot-Worker experiment (N05) |
| F-H21 | Two verdict writers can race | 8 | One writer per repository and phase, behind a flag; a new writer shadows first (N52) |
| — | Watch results after merge | 8 | Revert rate, red-`main` rate, failures on receipt-passed trees and cost per merged PR, in work item 4 and the risks (N53) |
| F-B20 | Reserve capacity for the merge path | 3, 8 | The three-day cordon experiment for a merge and gate lane (N38) |
| F-B14 | Decide the compile cache by experiment | 1, 9 | Decided on S12: plain cargo over reflinked node snapshots on the hot path; mbx and a per-site `mbx-cache` as the cross-node layer, kept by a measured threshold (N22, N54) |
| F-H38 | Clone and restore numbers are lab numbers | 7, 9 | Measured on a worker for the runner workspace: a clone 0.25 s and 2.5 MB, a full copy on ext4 8.6 s and 10.75 GiB (S12); the phase-2 gate still publishes set-up time by file count |
| F-B13 | Overlayfs instead of per-file clones | 9 | Its set-up gain is moot at 0.25 s; it is the remedy if the page-cache budget is exceeded (N56) |
| F-H32 | Moving work directories off ext4 | 5, 9 | A phase-1 prerequisite: the local-path volumes are already XFS with reflink (I095) |
| F-B24 | Distributed work for cold builds | 9 | `mbx-cache` shares complete compile outputs across a site's nodes: a cold node in 39.9 s instead of 92 s (N17, N54) |
| — | Snapshot freshness, found in the cluster run's method | 9 | Unchanged files keep the snapshot's times and every other file is newer than every output; work item 1 tests it (N55) |
| — | Operating `mbx-cache` | 9 | TLS, `MBX_REMOTE_TRUST_WRITES=true` for the builder only, garbage collection, a starting budget of 300 GiB per site, writes only from trusted builds of `main` (N54) |
| — | Owner decision: Rust only | 10 | The native executor runs every job; the official runner is the lab oracle only, never the executor or a fallback (N57, N58); latest APIs only (N59); velnor-legacy code copied and owned, never revived (N60); `crates/velnor-runner` superseded as an executor (N61); no Node, a closed native action set (N62); `docs/goals.md:16` decided (N63); cutover gate and rollback (N64); hosted forks and macOS builds left as an owner question (N65). Overturned: I001, I004, I008, I094, N05, N24; changed: I003, I005, I006, I013, I016, I017, N06, N10, N18 |

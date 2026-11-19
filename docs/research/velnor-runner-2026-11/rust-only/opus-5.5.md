# Rust-only redesign by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor, Rust only: CI that is never the bottleneck (Claude Opus 5.5)

Panel answer to `rust-only-brief-v1.md`, 2026-11-19. Tags: **[M]** measured, with its source (an
evidence table such as §A T2, a v2 experiment, or a read I made this round under
`opus-5.5/work/ro/`); **[E]** estimate, with its arithmetic; **[T]** target. "v2" means
`v2/velnor-runner-architecture.md`. The official runner is cited at actions/runner `7afce9e`
(`src/...`), velnor-new at `d6be276a6c`.

## 1. Summary

- **The verdict is a check, not a workflow.** The Velnor GitHub App owns one required check, `Required`, bound in the ruleset to the App's integration id. It is written from webhooks (`pull_request` labeled, unlabeled, synchronize; `merge_group`). Unlabelled heads carry a *queued* check, which blocks merging. Nothing else can write it, and no Actions run is created per push.
- **Verification is a content-addressed ledger of obligations.** An obligation is per package and per configuration: build, clippy, doc, each test binary, a lint, or a release build. Each is keyed by its input closure and runs once per key, in agent-spawned sandboxes over reflinked warm snapshots of `main` (0.25 s and 2.5 MB [M]). Compilation is incremental, and tools, crate sources and the Git mirror live on the node. Merge groups and pushes to `main` are lookups, and a release promotes bytes built once in its merge group.
- **The native Rust executor runs every remaining Actions job:** release, schedules, the lab and qualification. It implements the closed action set, ports the *new* evaluator, and is held to the official runner by a differential oracle. It is off the merge path by design.
- **Confidence is unchanged, and checked by detection.** Changed obligations run twice in parallel on two nodes, matching today's two pre-merge runs. A cold audit of each `main` tip within 2 h replaces today's push run. Every reuse mechanism has a key, a read audit, a detector and an automatic off switch.
- **PR verdict P50/P90:** committed ≤ 60 s / ≤ 3 min, stretch 25 s / 90 s (about 2 s after an agent's pre-push verify). Today it is 30.4 / 99.3 min [M].
- **Push to merge P50/P90:** committed ≤ 2 min / ≤ 5 min, stretch 70 s / 3 min. Today it is 85 / 457 min [M]. The floor is GitHub's merge-queue latency of about 45 s [M] plus the change's own compile and its slowest affected test.
- **Cost:** about 17 sandbox-minutes per merged PR, against about 515 runner-minutes today [E, §2b]. GitHub-hosted minutes drop to zero, and a release rebuilds nothing.

## 2. The latency budget

**The PR traced.** It is the median merged PR: a change to 2–4 files in `generator/crates/` (the
47% class: generator or helper only [M, `proposals/fable-5.2.md`, 200 PRs]). Its final push
happens while the PR carries `velnor:verify`, and auto-merge has been set since the PR opened.
**The clock** runs from the later of the accepted final push and the label event to the merge.
Waiting for a human to add the label is not CI time, and the dashboard reports it separately.

| # | Stage | Today, P50 | Today's bottleneck | Design, P50 | How it is removed |
|---|---|---|---|---|---|
| 1 | GitHub event, webhook, job offer | run created 1–2 s after push; first job starts 3.7 min later (P90 26.3) [M §A T2] | 8 gate jobs on the Free plan's 20 hosted slots, shared with Source qualification (267 runs) and image candidates (306 runs) on 11-18 [M §A T11] | 1.5 s from push to the `pull_request.synchronize` delivery [E: GitHub creates push-event runs in 1–2 s, M v2 §3.4; work item 3 measures delivery] | No Actions run on the PR path; the webhook goes straight to the ledger |
| 2 | Acquire | "Set up job" 5.3 s, 4.4 s of it six serial action downloads; scale-set assignment 12 s [M, opus §1.4] | Runner start, action tarballs | 0 s on the PR path (Actions jobs elsewhere: ≤ 0.3 s, §3.11) | The verdict is computed by velnor-control, not by a job |
| 3 | Sandbox start | ARC pod 6 s, max 16 s, gated by the dind probe [M, 436 pods] | Pod creation plus privileged dind | 0.3 s: namespace sandbox ≤ 0.05 s [E] + reflink clone 0.25 s [M, cluster P2], overlapped with 4 | Agent-spawned sandboxes (§7); no pod, no dind |
| 4 | Checkout | 6 s per job [M §A T10]; Plan's full-history clone 12 s [M] | 31 clones per run | 0.6 s: fetch the pushed pack into the site mirror on the webhook, about 0.5 s [E: 1–2 RTT at 120 ms, M §B.5]; write the changed files into the clone ≤ 0.1 s [E] | Site Git mirror; only the diff against the snapshot's tree is written |
| 5 | Toolchain and tools | 14–15 s per job [M: tools 10, mise 2, rustup 1, MBX 1, manifest 1; §A T9b] | Per-job install into `runner.temp` | 0 s | Read-only node store keyed by the pins |
| 6 | Dependency fetch | 4 s fetch + 1 s restore; sources cache 0 of 48 hits [M §A T9a] | Cache key built from ARC's `runner.temp` path | 0 s; 3 s for the 10.5% of PRs that change a lockfile [M: mirror 2.7–3.3 s] | Node crate store keyed by `Cargo.lock` checksums; builds run `--offline` |
| 7 | Compile | 8.5 min critical path (`maintenance-helpers` at `-j2`); each job compiles 196–497 crates cold [M §A T3, T9a] | 0 cross-job reuse (0 of 3,636 restores) | 25 s committed, 8 s stretch [E: v2's 15 s generator delta (8.8 s on an M5 × 1.7) + 10 s to relink `velnor-ci`, which reaches `generator/` through path dependencies; incremental 1.5–2.8 s on an M5 (M `blaze.md` B3) × 1.7 + 3 s of links] | Warm `main` snapshot with incremental state; `-j` equals the node's free threads; clippy and doc in parallel lanes |
| 8 | Tests | The run waits for its slowest job: test steps 192–246 s in runner jobs, 486 s in maintenance [M §A T10] | 39 jobs run everything; single slow tests | 10 s [M: `velnor-ci` test step 10 s P50, §A T10]; the twin runs on a second node at the same time | Only obligations with a new key; longest-first scheduling |
| 9 | Result reporting | Required verdict: queue 1.6 min + 0.7 min run (22 s downloading reports) + Required queue 1.9 min [M §A T4] | Hosted queue, artifact round trips | 0.3 s: one check-run `PATCH` [E: 1 RTT] | Receipts land as obligations finish; the verdict is a query |
| 10 | **Required check (PR verdict)** | **30.4 min**, P90 99.3 [M, 98 PRs] | Sum of 1–9 | **38 s** committed path (1.5 + 0.6 + 25 + 10 + 0.3); 20 s stretch; ~2 s after a pre-push verify | — |
| 11 | Merge-queue entry and its run | green → enqueue 7.5 min (no PR used auto-merge) + enqueue → group 19 s + group run 24.4 min [M] | Agent procedure; the group reruns 39 jobs | **22 s**: enqueue on green ~2 s [E] + GitHub creates the group 19 s [M] + delivery 1 s + lookup 0.5 s. In 78–89% of merges the group's tree is the PR-tested tree at a ≤ 5 min window [E, v2 §6.6 replays]; otherwise only re-keyed obligations run, pre-verified during those 19 s (§8.5) | Auto-merge; exact-tree and per-obligation reuse; queue-tip speculation |
| 12 | The merge | group finished → merged 26 s [M] | GitHub's queue processing | 26 s [M] | Irreducible while GitHub's queue lands merges (decision 7) |
| | **Push to merge** | **85.1 min**, P90 456.6 [M] | | **86 s** committed path; 68 s stretch; ~50 s after a pre-push verify | |

**P90.** The P90 PR changes the runner workspace: 25% of PRs touch only it, and mixed and engine
PRs add more [M fable]. A one-file change to `velnor-runner-host` takes 61 s on an M5
non-incremental, which is about 100 s on EPYC, or 10.9 s incremental, about 19 s [E, × 1.7; M v2
§6.3]. Its slowest affected test is 125–138 s today [M] and ≤ 60 s after the phase-2 budget [T].
About 11–22% of merge groups see `main` move under an overlapping closure. They re-run 1–2 min of
obligations, mostly already done by speculation. The verdict P90 is 2 + 19 + 60 + 1 ≈ 82 s, so
≤ 3 min is committed with room for the non-incremental fallback. Push to merge P90 is the verdict
plus 45 s plus the residual re-run, ≤ 5 min.

**What becomes the bottleneck.** For the median PR, GitHub's 45 s queue latency (stages 11 and 12)
becomes the largest term. The PR's own compile is second and its tests third. No stage waits on a
queue, a download or a cold cache. Section 9 takes each remaining term to its floor.

## 2b. Duplicate-work audit

I read every workflow in `.github/workflows/`, velnor-gen's release renderer
(`crates/velnor-render/src/release_binaries.rs`) and its samples, and the `ci-helper` gate.
Costs are runner-minutes (job wall time) [M] unless marked: §A of the evidence, my earlier 10-run
samples, and 8-run job reads made this round for the release, source-preview and state seal workflows.
A design "sandbox-minute" is the same unit: the wall time a sandbox exists.

| # | Operation | Stages that run it today | Same input? | Today's cost | Decision | Mechanism |
|---|---|---|---|---|---|---|
| 1 | Plan job: clone, attested-state selection, tools, plan | PR, merge group, push: every CI run | Push: same SHA as the group in 120 of 159 [M T7] | 0.9 min + 5.5 min hosted queue per run [M T4] | Eliminate | velnor-control plans once per tree on the webhook (≤ 2 s), keyed by tree and released planner |
| 2 | Ledger CLI (`cargo build -p velnor-actions-cli`) | Every CI run | Yes | 0.88 min per run [M T3] | Eliminate | Released helper in the tool store, by version and SHA-256 |
| 3 | Toolchain and tools install | 31 jobs × every run | Yes (pins) | 14–15 s per job, 7.5 min per run [M] | Eliminate | Node tool store, once per pin per node |
| 4 | Crate fetch | Every Rust job | Yes (lockfiles) | 4–5 s per job, 2.3 min per run [M] | Eliminate | Node crate store; `--offline` |
| 5 | Checkout | Every job | Yes within a run | 6 s per job, 3.1 min per run [M] | Eliminate | Site mirror; the diff is written into the clone |
| 6 | JavaScript action overhead | Every job | Yes | 17 s per job, 8.8 min per run [M opus §1.4] | Eliminate | No actions on the verification path; native actions elsewhere |
| 7 | Dependency compile | Every Rust job of every run | Yes: same lockfile and toolchain | 2,260 compilations of 463 unique packages per run (4.9×) [M `closure.py`] | Once per (lockfile, toolchain, profile, flags) per site | Builder → `mbx-cache` → reflinked snapshot on every node |
| 8 | Workspace crates, test profile | Every job that includes them | Yes for unchanged crates | runner-host and local-mode each compile the same 404–407 crates [M] | Once per closure key | Snapshot plus obligation keys |
| 9 | Clippy and doc passes | Per job, every run | Yes for unchanged crates | 8–75 s and 10–65 s per job [M T10] | Once per key | Clippy and doc obligations; metadata pre-built in the snapshot |
| 10 | `maintenance-helpers` builds at `-j2` | Every run | Yes | 4.5 of its 8.5 min [M] | Once per key, no `-j` cap | Obligations for its packages |
| 11 | Image-publication fixture (a cold example build) | Every actions-cli job | Yes | 89 s per job [M T10] | Once, in the snapshot (`--all-targets`) | Snapshot |
| 12 | Tests | PR (3–4.8 runs), group, push | Yes for unchanged packages | ~25 min per run [E: sum of T10 test medians] | Once per key; changed obligations twice, in parallel | Receipts; the twin (§6.5) |
| 13 | Lints: deny ×5, machete, jactionlint, alint, fmt | Every run, hosted | Yes | 1.2 min + queues [M T4] | Once per key; deny reads a pinned RustSec commit | Lint obligations |
| 14 | Required verdict aggregation | Every run, hosted | — | 0.8 min + 3.5 min queue [M] | Eliminate | Ledger query |
| 15 | Full CI on the merge group | Merge group | Identical tree in 7 of 45 today [M T7]; 78–89% in the design [E] | 64.6 min [M T2] | Exact-tree verdict, else per-obligation lookup | Ledger |
| 16 | Full CI on push to `main` | Push | Same SHA as the group [M T7] | 62.0 min [M T2] | Eliminate | The group's `Required` check already sits on that SHA; promotion only |
| 17 | Source qualification | Every PR event, `edited` included | Mostly | 24.0 min per run, hosted [M, 10 runs] | Once per generator-closure key, only when labelled | Obligations |
| 18 | Job image candidate build and attest | Every push to every branch, `gh-readonly-queue/*` included [M runs list] | Yes unless image inputs change | 3.4 min per push [M, 10 runs] | Once per image-input key, on `main` | Key over Dockerfile, `runner.lock`, tool pins |
| 19 | State seal producers and sealers | Every push to `main` | Mostly | 19.7 min per push [M, 8 runs] | Eliminate | Builder generations replace the GitHub cache |
| 20 | Cache prune | Every push to `main` | — | ~0.5 min [E] | Eliminate | No GitHub cache in use |
| 21 | Source preview, Linux build and qualify | Every push to `main` | Yes unless the product closure changed | 7.4 min per push [M, 8 runs] | Once per product-closure key, in the merge group; tags promote it | Release obligation, site store |
| 22 | Source preview, Darwin | Every push to `main` | Yes | 22.6 macOS min per push [M] | Paused (decision 9) | — |
| 23 | velnor-ci and velnor-gen release builds at the tag (musl x64, musl arm64, macOS) | Every release | No: rebuilds a verified commit | 1.8 + 1.4 + 2.2 min [M, 8 runs] | Build once in the release's merge group; arm64 cross-compiled; macOS paused | Release obligation keyed by closure, target, profile and `VELNOR_COMMIT` |
| 24 | Product release builds, qualify, attest | Every product release | No: rebuilds what source preview built | ~29 min with macOS [E: same jobs] | Promote the preview's bytes for that SHA | Store and digest check |
| 25 | `SHA256SUMS`, attestations | Every release | — | < 0.2 min each [M] | Keep once: computed at build, verified and attested at promotion | Verdict record; native attest |
| 26 | Release smoke on the shipped bytes | Only `--version` and `check-tag` today | — | 0 | Add, ~10 s: run the musl binaries on the generator samples and helper fixtures | New obligation |
| 27 | Nightly full run (velnor-gen plans `15 8 * * *`) | Planned for velnor-gen's workflow; not run today | Mostly as `main` | ~60 min a day once enabled [E] | Replaced by the tip audit (row 29) | — |
| 28 | Cache warmers | None effective (row 19) | — | — | Builder once per `main` generation; nodes pull deltas | `mbx-cache`; cross-site build or transfer by measurement (§6.1) |
| 29 | Cold audit of a `main` tip | None (the push run stands in by accident) | Yes, on purpose | — | Keep, ≤ 12 a day, batched | §6.5 |
| 30 | Conformance runs | Lab only | Yes, on purpose | — | Keep, at batch priority, in the lab repository | §5 |
| 31 | Push N then push N+1 | Each push is a full run; 1,156 of 2,558 PR runs cancelled [M T7] | Partly | 52.9 min per completed run | Re-run re-keyed obligations only; push N's in-flight work that N+1 still needs is joined, not cancelled | Ledger join |
| 32 | Rebase-only push with the same tree | Full run | Yes | 52.9 min | Instant verdict | Tree key |
| 33 | Re-run with no code change (`edited`, `ready_for_review`, `reopened`) | Full run | Yes | 469 of 2,558 PR runs (18.3%) on an unchanged head [M F-H07] | Zero | Events that do not change the tree are ignored; a base retarget re-plans |
| 34 | Concurrent PRs, unchanged parts | Each PR retests everything | Yes | Included above | Reuse `main`'s receipts; one execution per new key across PRs | Receipts and join |
| 35 | Stacked PRs | Full run each | Partly | Included above | The upper PR reuses the base PR's receipts | pr-scope receipts |
| 36 | Two PRs with identical changes | Full run each | Yes | Included above | One verification | Tree key |
| 37 | Agent pre-push testing, then CI | Agents test on Macs; CI repeats it | No: a different machine | Unknown | `velnor verify` runs in the same sandbox class; CI reuses it | Same keys |
| 38 | Catch-up merges of `main` into PR branches | 7 of 15 PRs, 17 commits [M] | — | A full run each | Stop: the merge queue tests the combination | Agent procedure |

The design keeps two duplicates on purpose. **The twin** (row 12) costs zero wall time and adds
about 0.5 sandbox-min per verification, because it repeats only the changed obligations. **The tip
audit** (row 29) is the detector that cross-tree reuse requires. Release bytes are built exactly once,
in the release PR's group, because `VELNOR_COMMIT` is embedded through `option_env!`
(`crates/velnor-ci/src/io/mod.rs:78`) and only the group's SHA becomes `main`'s (§6.6).

| Per merged PR | Today, runner-min [E from M] | Design, sandbox-min [E] |
|---|---|---|
| PR verification | 206 (3.9 runs × 52.9) + source qualification 94 + image candidates 21 | 2.3 (1.5 labelled verifications × (1.0 + 0.5 twin)) + 1.0 agent pre-push |
| Merge group | 83 (1.29 groups per PR × 64.6) | 0.2 (re-keyed obligations in about 15% of groups) |
| Push to `main` | 62 + state seal 19.7 + prune 0.5 + source preview 29.0 (22.6 on macOS) | 0 + 0.5 (product build when its closure changes, about 20% of merges) |
| Builder, warmers, speculation | 0 effective | 1.5 + 0.3 |
| Audits | 0 | 11 (12 cold tip audits a day × 30 sandbox-min ÷ 33 merges a day) |
| **Total** | **≈ 515** (23 of them macOS) | **≈ 17** (audit ≈ 65%) |
| Compiled-output and cache traffic | ≈ 14,000 package compilations, all discarded; ≈ 0.6 GB GitHub-cache restores | ≈ 1.5 GB delta per `main` generation, written once per site [E: 0.5 GB generator, 3 GB runner, from the cluster's 6.8 GiB for 11 crates]; reflink clones 2.5 MB each; 0 GB GitHub cache |
| **Per release** (velnor-ci and velnor-gen) | Tag rebuild 3.9 min (3 targets, 2.2 on macOS), plus 3 full CI runs of the release PR (≈ 180 min) | 2.2 sandbox-min, once (2 targets × 1 min + 0.2 smoke) in the release PR's group; tag to publish 0.5 min; 0 rebuilds |

## 3. The native Rust runner

The executor is never on the merge path: PR and merge-group verdicts come from velnor-control (§8).
It runs every remaining Actions job (releases, schedules, image builds, qualification, the lab), so
a conformance gap delays a release, never a merge, and the cutover cannot regress push-to-merge.

### 3.1 Crates and components

A new Cargo root `runner/` (so its keys never couple to the five existing roots), with line budgets
checked in CI as in v2.

| Crate | Runs as | Responsibility | Origin | Budget |
|---|---|---|---|---|
| `velnor-gh` | library | Typed clients for every GitHub surface in §3.2, App auth, webhook signatures; unknown-field telemetry | velnor-legacy `scaleset/client.rs`, `session.rs`, run-service and Results parts of `protocol.rs`, re-reviewed | 8k |
| `velnor-control` | 2 replicas per site | Webhooks and reconciler, label state machine, released planner, ledger (obligations, receipts, scopes, epochs), check-run writer, speculation, audit scheduler, placement | new | 10k |
| `velnor-dispatch` | active + standby per site | Scale-set session, JIT identity pool, placement before acquisition, lease journal | new; JIT handling rules from velnor-new | 4k |
| `velnor-exec` | one process per Actions job, under the node agent | Job model, admission, new-evaluator expressions and templates, contexts, steps, composites, post steps, commands, masking, cancel, timeouts, renewal, Results | fresh, plus velnor-legacy `workflow_command.rs`, `command_files.rs`, `script_step.rs` | 18k |
| `velnor-actions-native` | inside `velnor-exec` | The closed set (§4) and refusal | velnor-legacy `checkout.rs`, `git_mirror.rs` | 8k |
| `velnor-node` | privileged DaemonSet | Sandboxes (namespaces and cgroups through youki's `libcontainer` crate), stores, snapshots, read audit (eBPF), result observation and receipt signing, builder and warmer mode, cleanup | new; absence proofs from velnor-new `core/src/ownership.rs` | 14k |
| `velnor-store` | 1 per site | Content-addressed blobs (test binaries, release artifacts, logs), the site Git mirror | new | 5k |
| `velnor-conformance` | lab | §5 | new, seeded by `qualification.yml` | 5k |

### 3.2 Protocol client: latest APIs only

- **In.** Scale-set sessions, long-poll with `lastMessageId` and `X-ScaleSetMaxCapacity`, ack,
  `acquirejobs`, `generatejitconfig` (`S/session_client.go:45,143-168,215-240,278-303`,
  `S/client.go:765-796`); broker `session`, `message`, `acknowledge`
  (`Sdk/WebApi/WebApi/BrokerHttpClient.cs:70-101,143,225-259`); run service `acquirejob`,
  `renewjob`, `completejob` (`Sdk/RSWebApi/RunServiceHttpClient.cs:70-90,123-160,184-200`);
  Results `Receiver` signed URLs and `WorkflowStepsUpdate` (`ResultsHttpClient.cs:610-632`); cache
  v2 and artifacts v4 twirp; the OIDC endpoint; and for the App, the Checks, REST and GraphQL APIs.
- **Out, and absent from the binary.** The AES session (`Runner.Listener/MessageListener.cs:458-495`),
  `_apis/distributedtask` plans, timelines and logs, `_apis/artifactcache`, and artifacts v3. A test
  scans the release binary's strings for those paths.
- **Drift detection is built into the types.** Every response captures unknown fields through
  `#[serde(flatten)]` and emits `protocol.unknown_field{type,name}`. An unknown enum variant that
  changes execution, such as a new step reference type, refuses the job: it fails closed and loudly.
- **Version.** Velnor sends as `runnerVersion` (`BrokerHttpClient.cs:86`) the newest official
  release that its corpus passed, with `disableUpdate`, and ignores `RunnerRefresh`. GitHub stops
  assigning jobs to runners 63 days behind (§F.6.1), so certification has a 7-day target and that
  hard deadline.
- **Secrets.** Admin and JIT credentials stay in the dispatcher. The job token travels only in the
  job message to `velnor-exec`, and its journal lives on tmpfs: it survives an agent restart, not a
  reboot, which loses the lease anyway.

### 3.3 The job-message model

Serde types mirror `AgentJobRequestMessage`
(`Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs:27-47,92-288,486-508`):

- plan and timeline references, job id and names, request id, `LockedUntil`;
- `Resources.Endpoints["SystemVssConnection"]`, with its token and its `Data` map of cache, Results,
  OIDC and feed URLs;
- `ContextData` as a typed `PipelineContextData` enum, `Variables` as value plus `isSecret`, and
  `MaskHints`;
- `Steps`, whose `ActionStep.Reference` is `RepositoryPath`, `Script` or `ContainerRegistry`, with
  condition, `continue-on-error`, timeout, env and inputs held as `TemplateToken`s;
- env, defaults, job outputs and containers as tokens, plus `FileTable`.

**Admission** is a pure function from the message to a plan or a refusal, and it runs before step 1.
Every `uses:` must be in the qualified set at a qualified SHA, or be a local composite whose own
`uses:` qualify. A composite is checked after checkout, before its first step. Container steps, job
containers and services are refused; velnor-new uses none outside the qualification fixtures, which
become refusal tests. A refusal fails the job with one annotation that names the action and lists
the supported set.

### 3.4 The expression engine

**The latest runner's results still come from the legacy evaluator.** Without a flag it uses
`PipelineTemplateEvaluator` (DTObjectTemplating and DTExpressions2) directly
(`src/Runner.Worker/ExecutionContext.cs:1551-1562`). Under `actions_runner_compare_workflow_parser`
or `ACTIONS_RUNNER_COMPARE_WORKFLOW_PARSER`, `PipelineTemplateEvaluatorWrapper` also runs the new
`WorkflowTemplateEvaluator`, over `Sdk/Expressions` and `Sdk/WorkflowParser`
(`PipelineTemplateEvaluatorWrapper.cs:19-20,40-55`). It records `TemplateEvaluatorMismatch`
telemetry (`:204-221`) and **still returns the legacy result** (`:312-318`).

Decision 3's "legacy evaluator" is therefore DTExpressions2. That is exactly what velnor-legacy
transcribed (`velnor-legacy/crates/velnor-runner/src/expression/mod.rs:1-14`) and what v2 keeps.

**The design ports the new evaluator as Velnor's only evaluator.** The sources are `Sdk/Expressions`
(6,947 C# lines), `WorkflowParser/ObjectTemplating` (7,100) and `WorkflowTemplateEvaluator` (986),
about 8k lines of Rust [E]. `hashFiles` is native Rust; the oracle spawns Node for it
(`Expressions/HashFilesFunction.cs:73,206`).

**Contract.** The lab oracle runs in compare mode. An evaluation the oracle flags as a mismatch is
an *upstream divergence*: it is recorded, and it is exempt from the equality gate only while a
velnor-gen lint proves that no generated workflow uses that expression shape. Every other result
must be equal to the oracle's. When GitHub flips the wrapper to return the new result, the
divergence cases become equality cases with no Velnor change.

### 3.5 Steps, composites and post steps

- **Steps.** `if:` defaults to `success()` in the step's scope. The runner handles
  `continue-on-error` with outcome against conclusion, `timeout-minutes`, `working-directory`,
  `defaults.run`, and shell templates: the bash default `--noprofile --norc -eo pipefail`, `sh -e`,
  and custom templates such as velnor-gen's `bash -p {0}`.
- **Composites.** Inputs map to `inputs`. Nested steps run in a composite scope with their own
  `steps` context, `github.action_path` and merged env. Outputs are mapped back, and status
  functions are evaluated with composite semantics.
- **Post steps.** A stack, registered by native actions with their `post-if` (checkout `always()`,
  cache save `success()`). They run in reverse order after the main steps, under failure and cancel
  as their condition says, with state carried through `GITHUB_STATE`.
- **Commands.** Workflow commands (`add-mask`, annotations, `group`, `debug`, `stop-commands`) and
  file commands: `GITHUB_ENV` with multi-line delimiters and blocked keys
  (`Runner.Worker/FileCommandManager.cs:181`), `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STATE`,
  `GITHUB_STEP_SUMMARY`. Annotations are capped at 10 per type, and messages at 4,096 characters
  (`ExecutionContext.cs:144-146`).

### 3.6 Contexts, env, secrets and masking

- **Contexts** come from `ContextData` (github, needs, inputs, vars, strategy, matrix), plus `runner.*`,
  `job.*`, `steps.*` and `env`. The `GITHUB_*` and `RUNNER_*` variables are set byte for byte as the
  oracle sets them, which the corpus checks.
- **Tokens in `run:` steps.** Only `ACTIONS_ID_TOKEN_REQUEST_*` reach `run:` steps
  (`ScriptHandler.cs:313-327`). The runtime token, cache and Results URLs stay inside native actions.
- **The masker** registers every secret variable, the job token, `MaskHints` (variable and regex) and
  `add-mask` values. Each is registered with the oracle's encodings: Base64 at three shifts,
  command-line, expression string, JSON, URI, XML, trimmed quotes and PowerShell
  (`Runner.Common/HostContext.cs:103-112`), and a multi-line secret is registered line by line.
- **Mechanics.** One Aho-Corasick automaton scans the live feed, uploaded logs, summaries,
  annotations and outputs. It holds back the longest secret's length at chunk boundaries so a split
  secret cannot leak. The corpus plants one leak per encoding.

### 3.7 Cancellation, timeouts and lock renewal

- **Renewal.** The first `renewjob` must succeed before step 1 (`Runner.Listener/JobDispatcher.cs:380-410`).
  Later renewals run every 60 s (`:775-776`) and retry until `lockedUntil` + 5 min (`:807-840`).
  The loop is written fresh, because the upstream overload dereferences null on failure (`:748-852`,
  v2 N10). A lost lease kills the sandbox and reports nothing.
- **Cancel.** `JobCancel` makes `cancelled()` true. The running step's process group gets SIGINT,
  then SIGTERM after 7.5 s, then `cgroup.kill` after 2.5 s more (`Runner.Sdk/ProcessInvoker.cs:32-33,448-481`).
  Then the `always()` and `cancelled()` steps and the post steps run, and the job completes as
  `cancelled`. The worst case is 10 s plus those steps. `cgroup.kill` takes every descendant; the
  agent proves absence by an empty cgroup.
- **Timeouts.** A step timeout cancels that step and fails it. A job timeout takes the cancel path.
  The conclusions are pinned by corpus cases.

### 3.8 Logs and timeline through Results

- **Timeline.** Step records go through `WorkflowStepsUpdate`, batched every 500 ms
  (`JobServerQueue.cs:34`).
- **Logs.** Step logs are append blobs (`GetStepLogsSignedBlobURL`, `CreateStepLogsMetadata`), plus
  the job log.
- **Live feed.** Lines go over the `FeedStreamUrl` websocket at the oracle's 250 ms cadence
  (`JobServerQueue.cs:32-33,91-94`). The feed is best effort and never blocks a step.
- **Completion.** Summaries go through the step-summary pair, and `completejob` carries outputs,
  `StepResult`s (`Contracts/StepResult.cs:14-52`), annotations and telemetry.
- **Ordering.** A step's log is sealed before its completed record, as the oracle orders them.

### 3.9 Crash safety

- **Journal discipline.** The rule is velnor-new's: persist the intent, make the call, record the
  outcome; unknown is never absent. Its ownership and absence proofs are copied, but not Turso.
  `Journal::open` dominates velnor-new's slowest tests (v2 §6.3), and a job journal needs only an
  fsynced append-only record file.
- **Process layout.** `velnor-exec` runs as one process per job under a subreaper shim, so an agent
  restart re-adopts running jobs through the journal and a control socket.
- **Executor crash.** A crashed executor's job is completed as failed with
  `InfrastructureFailureCategory`, using the journaled token, unless the journal shows `completejob`
  done. A 404 or 409 on `completejob` means it is already complete.
- **Dispatcher.** It persists acquisition intent before acquiring and the sealed message before
  handing it off.
- **Fault injection.** The lab kills processes at every journal write, every remote call and every
  step boundary, drops responses, and partitions the network. It asserts that no job is reported
  twice and no step runs twice.

### 3.10 What to copy, what to write

| Copy from velnor-legacy (into velnor-new, reviewed, then owned) | Copy from velnor-new | Write fresh |
|---|---|---|
| Scale-set admin client and session; broker, run-service and Results calls; `checkout.rs` and `git_mirror.rs`; workflow and file commands; script steps; composite scopes; its incidents as regression tests | Journal discipline; ownership and absence proofs; JIT secrecy rules (never in argv, env or labels); `velnor-ci` planner, gate and `inputs restore` mtime rule; velnor-gen and its closed-set lint; `velnor-local` as a harness | New-evaluator port and templates; masker; renewal and cancel; dispatcher; sandboxes and read audit; ledger and App checks. **Not taken:** the Expressions2 transcription, the 37k-line `executor.rs` and `runner.rs`, the `distributedtask` paths, cache v1, JavaScript sidecars, Docker actions |

### 3.11 Starting a job in the fewest milliseconds

1. **Pre-acquired identities.** The dispatcher keeps the scale-set long-poll open, plus four
   pre-generated JIT identities per site, each holding a broker long-poll. GitHub's assignment never
   waits for a registration. ARC instead patches replicas, creates a pod and registers a runner after
   `JobAvailable` (`A/docs/gha-runner-scale-set-controller/README.md:9-19`).
2. **Pipelining.** `JobAvailable` carries the repository, run id, workflow ref and labels
   (`S/types.go:21-57`). Placement picks a node at once. The node agent then builds the sandbox:
   template, store mounts, and a mirror checkout of the head SHA already known from the push webhook.
   The dispatcher acquires in parallel.
3. **Start.** `acquirejob`, then the first `renewjob`, then admission (< 1 ms), then step 1 in the
   prepared sandbox. There is no runtime to start, no action download and no Node.
4. **Numbers.** From the broker message to the first step is about 2 RTT, about 0.3 s [E: 120–150 ms
   RTT, M §B.5]. From `JobAvailable` it is 0.6–1 s, against 2–3 s on an idle ARC runner and 12–14 s
   on demand [M v2 §3.4].

Namespace sandboxes start in ≤ 50 ms, so they need no snapshot and restore. A microVM with
snapshot restore is kept for a future macOS or hostile class.

## 4. Native actions

The closed set is every `uses:` in today's `.github/` plus velnor-gen's outputs
(`generator/samples/*/expected`) [M: grep counts below]. velnor-gen emits only qualified actions at
qualified SHAs, which a generator test enforces. A Renovate SHA bump is qualified only after the
corpus passes at that SHA. Effects that leave the job (artifacts, cache entries, attestations,
outputs) must equal the upstream action's, and §5 checks them.

| Action (uses: today / velnor-gen) | Native design | Local instead of network | Today [M] → design [E] |
|---|---|---|---|
| `actions/checkout` v7.0.1 (61 / 75) | Native Git with the action's inputs and outputs: `ref`, `fetch-depth` (writes `.git/shallow` so history matches), `sparse-checkout` (today's Rust jobs use it), `persist-credentials`, `clean`, `fetch-tags`, `lfs`; a post step removes credentials | Objects from the site mirror through alternates, fetched on the push webhook | 6 s → ≤ 0.5 s |
| `actions/upload-artifact` v7.0.2, `download-artifact` v8.0.2 (152 / 111) | Artifacts v4 through Results `ArtifactService`; streaming zip; the same names, files and digests | Write-through to the site store by digest; a download in the same site reads locally | Part of 17 s of action overhead per job; 1.48 GB archive upload 36–43 s → about 1 s per GB in-site (none on the verdict path) |
| `actions/cache`, `/restore`, `/save` v6.1.0 (25 / 286) | Cache service v2 twirp with the action's key, restore-key, version and scope rules | Read-through of entry blobs on the node | 1–3 s, and 44–61 s for a 1.92 GB entry → ≤ 0.1 s on a hit. Velnor-run verification no longer caches through it |
| `jdx/mise-action` v5.2.6, `install=false` (55 / 61) | The SHA-pinned mise 2026.11.1 and the tool store on `PATH`; the action's env and outputs | Read-only tool store keyed by `mise.lock` | 2 s + 10 s tools → ≤ 0.05 s |
| `jdx/mr-boxington-action` v1.7.3 (12 / 0) | The pinned mbx linked from the tool store; the action's env and shims | Tool store | 1 s → 0.02 s; velnor-gen drops it |
| `actions/attest-build-provenance` v4.2.2, `actions/attest` (9 / 0) | OIDC token → Fulcio keyless certificate → DSSE in-toto statement → Rekor → `POST /repos/{o}/{r}/attestations`. For a promoted artifact, the predicate names the Velnor build receipt, so the provenance stays truthful | None: signing is network-bound | < 0.2 min → 1–2 s |
| `actions/create-github-app-token` v3.3.0 (0 / 6, in velnor-gen's `release.yml`; v2 misses it) | RS256 App JWT → installation token with the requested permissions; masked; a post step revokes it | None | → ~0.3 s |
| `tailrocks/velnor-new/actions/setup-velnor` (1 / 122) | Native: checks the version and SHA-256 inputs against the store's `velnor-ci`, falling back to the release download | Tool store | 1–3 s → 0.01 s |
| `asamarts/alint` v0.21.0 (1 / 0) | The pinned alint with the action's inputs | Tool store | 0.3 min + 5.9 min hosted queue → seconds, as a keyed obligation |
| Local composites (`.github/actions/*`, the qualification composite and post-fail fixtures) | Composite semantics (§3.5) | — | Most disappear with velnor-gen's output |
| `actions/github-script` v9.0.0 (3 / 0) | **Refused** (JavaScript). velnor-gen replaces the tag-ruleset drift check and two qualification steps with `velnor-ci` subcommands | — | — |
| `qualification/actions/docker` and the node24 fixtures | **Refused**; they become refusal cases in the corpus | — | — |

## 5. The conformance oracle

### 5.1 The differential harness

**Setup.** The `velnor-lab` repository has two runner pools on lab nodes, built from one OS image,
one tool store and one workspace layout (`/home/runner/_work/<repo>/<repo>`):

- `oracle`, running the latest official `actions/runner` release;
- `velnor`, running the candidate executor.

Each case is a workflow whose matrix runs the same job on both pools, from the same commit and event.

**The corpus** lives in `conformance/` in velnor-new and has four parts.

1. **Real jobs.** Every job velnor-gen generates for velnor-new (release, schedules, qualification) at
   pinned commits. Publication goes to the lab repository and lab registries. Recorded failures,
   cancellations and re-runs are included.
2. **Semantics.**
   - Expressions: both evaluators' vectors, plus grammar-generated cases run against the oracle in
     compare mode.
   - Contexts and status functions, in job and composite scope.
   - `continue-on-error`; step, job and `needs` outputs.
   - Each file command, with delimiters and blocked keys; workflow commands and the annotation caps.
   - Masking for each encoding, including secrets split across chunks.
   - Post-step order.
   - Cancellation at every step boundary and inside post steps; step and job timeouts.
   - Cache and artifact round trips, OIDC claims, and `hashFiles` globs.
3. **Refusals.** An unknown action, an unqualified SHA, JavaScript, and Docker.
4. **Protocol faults.** A record-and-replay proxy injects identical responses into both pools:
   acquire 409 and 422, renew failures, Results 5xx, and dropped websockets.

**Normalisation** removes ids, timestamps and durations; runner name, labels, version and machine;
the oracle's own lines (banner, action downloads, Node setup); ANSI codes; and the interleaving of
concurrent streams. Lines a user's step prints, and the `GITHUB_TOKEN` permission lines, are kept
and compared.

**Equality, after normalisation,** covers:

- step outcomes and conclusions, and outputs;
- probe dumps of `toJSON(github|env|runner|steps)`, the env and `PATH` each step sees, and expression
  results;
- annotations (level, file, line, title, message) and summary bytes;
- cache effects (keys, versions, sizes, content digests) and artifacts (names, files, digests);
- attestation subjects and predicate fields, without run ids or time;
- timeline structure (records, names, order, nesting, states) and the ordered log lines of each step.

Any other difference fails the case unless an owner-signed allow-list entry covers it, with a
reason, a scope and an expiry of at most 90 days. Evaluator divergences (§3.4) are counted in their
own bucket.

### 5.2 Running against each new official release

- **Release watch.** A watcher polls `GET /repos/actions/runner/releases/latest` every 15 min. On a
  new tag it verifies the published SHA-256, builds the oracle image and runs the corpus, which
  targets ≤ 2 h on lab batch capacity [T].
- **Certification.** At 100% the release becomes the version Velnor reports. Otherwise an issue
  opens with the diffs and the deadline (release date + 63 days, §F.6.1).
- **Other runs.** The corpus also runs nightly against the certified version, and as a blocking
  check on every change to `velnor-exec`, `velnor-actions-native` or `velnor-gh`.

### 5.3 Pass criteria for the cutover

The official runner leaves production when all of these hold:

1. 100% of cases pass on two consecutive nightly runs against the latest release, with every
   allow-list entry signed.
2. Every evaluator-divergence case is covered by a velnor-gen lint.
3. Velnor-new's real Actions jobs run as shadow twins for two weeks and at least 300 jobs (release
   dry runs into the lab repository, schedules), with zero unexplained differences.
4. The fault-injection suite is green (§3.9), and job start is ≤ 1 s at P90 from the broker message.
5. The harness catches all of at least 12 planted bugs: one unmasked secret per encoding class, a
   wrong `cancelled()`, a reordered post step, a lost annotation, a wrong output and a wrong shallow
   depth, among others.

The cutover can be this strict because merges never wait for it (§3).

### 5.4 Catching protocol drift within hours

| Detector | Catches | Time to detect |
|---|---|---|
| Strict types on production traffic (§3.2) | New fields, enum variants, HTTP statuses | Minutes |
| Lab canary every 15 min: acquire, renew, Results logs, timeline and summaries, annotations, cache v2, artifacts v4, OIDC, cancel | Broken or changed job protocol | ≤ 30 min (two failures page the owner) |
| Synthetic lab PR every 30 min: label → check → queue → merge | Changes to webhooks, Checks or `merge_group` | ≤ 1 h |
| Release watch plus the corpus | Semantic changes in a new runner | ≤ 2 h after release |
| GitHub changelog and deprecation feed | Announced removals | Days ahead (non-blocking) |

## 6. Compile, cache and test

### 6.1 The choice, from the measurements

| Option | Measured | Decision |
|---|---|---|
| Plain cargo over a reflink clone of the node's warm snapshot, at the fixed path `/w/velnor-new` | 0.25 s + 0.31 s no-op, +2.5 MB; full copy onto ext4 `emptyDir` 8.6 s and +10.75 GiB [M cluster P2, P3] | **The hot path** for every sandbox |
| mbx with a per-site `mbx-cache` | Fills a cold node in 39.9 s against 92 s; a lockfile change built on another node in 43.4 s against 67.4 s rebuilding locally; on the same node 88.4 s, slower than plain cargo's 67.4 s [M cluster M3, L1mb, L1m, L1p] | **Transport for `main` generations** (builder → nodes, site → site), never inside a sandbox. Kept or retired by v2's threshold (≥ 10% of builds served, or ≥ 20 runner-min a day) |
| blaze (canary 0.20261105.4) | None of 5 workspaces builds; cold 12× and edits 5–15× slower than cargo incremental; misses clippy `deny` (a false green); 18–44 GB RSS; ignores the 1.99.1 pin [M `blaze.md`] | **Rejected.** Revisit when all roots build and test with identical verdicts on a 1,000-fault corpus, beat cargo incremental on a worker, and use the pinned toolchain |
| Incremental state inside the snapshots | Edits: 1.5–2.8 s against 9.2–12.2 s (generator subset, M5) [M `blaze.md` B3]; `velnor-runner-host` 10.9 s against 61 s [M v2 §6.3] | **Adopt** in phase 2 under §6.5's case: −60–80% on the delta compile, the largest remaining term on the verdict path |
| overlayfs with the snapshot as lower layer | Clones share disk but not page cache: 14.29 GB per read clone [M cluster E1d]; overlay estimate 36 → 9 GiB for four jobs [E v2] | Phase-2 experiment. Adopt if build time stays within 5% and page cache falls ≥ 50% |
| Optimised dependencies in the test profile (`opt-level = 3` for non-workspace packages, assertions unchanged) | −66% CPU on the slowest test class, measured with dependency assertions off [M fable] | Measure with assertions on. Dependencies compile only in the builder, so PRs never pay |
| Line-table debuginfo for workspace crates, none for dependencies | −20–27% CPU on a runner-host rebuild; −10–24% cache [M opus; I108] | Adopt in phase 1 |
| Bazel or Buck2 remote execution; sccache | — | Rejected: a build-graph rewrite that leaves cargo semantics releases depend on; mbx already gives per-crate content addressing |

**Builder and sites.** The builder builds each `main` generation once and publishes the changed
crates' outputs through `mbx-cache`. Each node's warmer advances its snapshot from there, with 0
compiles. The other site either receives the delta or builds it, decided by one phase-1 measurement:
the cross-site fetch of a typical generation delta against a local delta build (decision 10). PR
builds never write a snapshot or `mbx-cache`; every PR build starts from a trusted generation.

### 6.2 Test-level scheduling across the cluster

- **Inside a sandbox.** nextest runs each test as its own process. Tests are ordered longest first by
  the per-test durations the ledger keeps, and test threads equal the node's free threads.
- **Across nodes, only when it pays:** when the summed test time divided by the threads exceeds the
  longest test plus the transfer time. Test binaries move by digest, and only changed ones; the
  whole 1.48 GB archive takes ~7 s at the measured 215 MB/s [M cluster M3]. Cold audits,
  lockfile-change groups and full runs use this. The median PR's affected tests fit one node.
- **Completeness.** Every expected test, taken from `nextest list` on the built binaries, must report
  exactly once. An obligation that runs zero tests fails.

### 6.3 Breaking the 125–138 s single-test floor

The floor is code, so the levers are code, and selection keeps every other PR off it.

1. **Affected-only.** A generator PR never runs `velnor-runner-host` tests.
2. **Journal template.** Migrate the template once per test-process tree, then `FICLONE` it per test,
   which takes microseconds. Today each helper iteration spends 21.8 s in Turso's parse and prepare
   [M opus].
3. **One test per case** for the 6- and 40-case loops. Prefix fixtures are built once and cloned per
   case; the 924.8 s receipt-integrity test builds the same state four times [M F-H33].
4. **Optimised dependencies** (−66% [M]).
5. **One high-clock node** for the residual serial tests (1.3–1.5× [E]).
6. **A per-test CPU budget.** Reported in phase 1, blocking for new tests from phase 2.

**Expected:** 21.8 s → about 7.4 s with optimised dependencies → ≤ 1 s with the template [E]. The
floor is ≤ 30 s committed in phase 2 and ≤ 10 s stretch in phase 3.

### 6.4 Keys

**What a key covers.** It hashes:

- the definition: command, features, profile, nextest profile and overrides, helper version;
- every `Cargo.toml` of the root, `Cargo.lock`, `.cargo/config.toml`, the toolchain file and
  `.velnor/rust-ci.toml`;
- the Git tree hash of every package in the obligation's closure, cross-root path dependencies
  included (`velnor-ci` → `generator/crates/*`);
- declared extra inputs: fixtures, `include_str!` targets, and `.velnor/runner.lock` for
  `velnor-runner-host`'s build script;
- the toolchain, image and tool-store digests, the env allow-list with its values, the CPU class and
  the path.

**Every manifest is in the key, and every sandbox holds the full tree.** Cargo 1.99.1 refuses
`cargo build -p a` when another member's `src/` or directory is missing [M, two-member workspace in
my scratch]. Features, patches and profiles also live in manifests. So a sparse checkout of the
closure (v2 §4.1) cannot load the workspace.

**Completeness is observed, not assumed.** The agent traces path syscalls per sandbox cgroup with
eBPF: `openat`, `statx`, `newfstatat`, `faccessat2`, `readlinkat` and `getdents64`, including
`ENOENT` results. A read by a build script or test outside the key's paths demotes the receipt to
exact-tree scope and opens a planner fix.

**The other inputs are closed.** Tests get a loopback-only network. No test manifest declares
proptest, quickcheck or rand [M: 0 manifests]. Eleven test files read the wall clock [M grep], so
their receipts expire in 24 h; other receipts expire in 7 days.

### 6.5 Verdict reuse without trusting anything unproven

**Receipts** are minted only by the node agent, from processes it started in a sandbox it created.
It observes every exit code and the expected test inventory. Each receipt is signed by a per-node,
per-class key held outside the sandboxes and is appended to a per-site log.

| Scope | Minted by | Who may reuse it |
|---|---|---|
| `pr` | PR verification and pre-push verification | Any PR or merge group whose key is identical |
| `landed` | Merge-group receipts whose SHA reached `main` | The same, and the push to `main` |
| `trusted` | `landed`, then covered by a passing tip audit | Releases (only this scope) |

PR results never write snapshots or caches.

**The twin.** Every obligation whose key has no receipt runs twice, on two nodes, in parallel, and
both must pass. That gives two pre-merge executions of changed code, today's PR run plus merge-group
run, and the tip audit is today's push run.

| # | Mechanism | Key; why complete | If incomplete | Detected by | Automatic fallback |
|---|---|---|---|---|---|
| 1 | Exact-tree verdict: rebase-only push, re-run, identical group tree | Merged tree + planner + toolchain, image and store digests + sandbox class. The tree holds every repository input; no network | A time- or order-dependent pass | Twin; tip audit within 2 h; 24 h validity | Flake path; a confirmed disagreement raises the policy epoch |
| 2 | Per-obligation closure key: group after `main` moved; concurrent and stacked PRs | §6.4, enforced by the read audit | An undeclared read outside the closure | 100% read audit when minting; shadow re-execution of every reuse until 3,000 clean per kind, then 2% on another node; tip audit | The kind falls back to exact-tree keys on the first disagreement; queued entries are re-verified at the new epoch |
| 3 | Warm snapshot (cargo's mtime fingerprints) | The trusted generation; every changed file newer than every output | A changed file with an older mtime is not rebuilt | Planted older-mtime test (work item 2); cold audits; daily lineage restart | That root builds cold |
| 4 | Incremental compilation | Not in the key: it must equal a clean build | rustc reuses a stale query result | Audits are cold and non-incremental; 2% of verifications re-run non-incremental | `CARGO_INCREMENTAL=0` fleet-wide |
| 5 | In-flight join; group reuse of `pr` receipts; agent pre-push | Same key, recomputed by the released planner on the consuming tree; signature, class and log checked | As 1 and 2 | As 1 and 2 | As 1 and 2; partial results satisfy nothing |
| 6 | No run on push to `main` | The `main` SHA equals the group head SHA that carries the App's green check | A direct push or another SHA | The push webhook compares SHAs | Full verification of that SHA |
| 7 | Release promotion | Closure, target, profile, toolchain, linker and C compiler, `VELNOR_COMMIT` = the group SHA | — : the bytes shipped are the bytes smoke-tested | `SHA256SUMS` re-checked at publish; release SHA must be in `trusted` | The release waits for the audit |
| 8 | Selection (skip what the change cannot reach) | `cargo metadata` graph + declared inputs; an unplaceable file selects everything; never relative to the queue | A missing dependency edge | Read audit; tip audit | Select the whole root |

### 6.6 The profile tension: debug tests, optimised releases

The shipped `velnor-ci` and `velnor-gen` use `opt-level = "s"`, thin LTO, `panic = "abort"`, strip
and musl (`[profile.release]` in `Cargo.toml` and `Cargo.toml`). Tests use the
dev profile, with debug assertions and overflow checks on.

- **Testing the release build: rejected.** Cargo ignores `panic` for test targets, because the
  harness needs unwinding, so release-profile tests still do not test the shipped bytes. Thin LTO on
  every test binary would slow every verdict; the two release binaries alone take 1.8 min cold on 4
  vCPU [M, 8 runs]. A release build also drops the overflow checks that catch bugs.
- **Building once at merge time, plus a smoke test: chosen.** The release build is an obligation of
  the release PR's group: a `[[binaries]]` version bump, which is exactly what release-plz tags. It
  is built once, with `VELNOR_COMMIT` set to the group's SHA, which becomes `main`'s. Then the
  shipped bytes are tested: the musl binaries run velnor-gen over `generator/samples/*` against the
  expected outputs, and `velnor-ci` over the helper fixtures, with aarch64 under qemu-user. It costs
  2.2 sandbox-min per release, against 3.9 min of tag rebuilds today [M], and it adds the one test
  today's release lacks.
- **Never silent.** The release job fetches artifacts by digest and refuses any digest that is not
  in the group's signed verdict. A missing artifact fails the release; it never starts a build.

## 7. Scheduling and the cluster

### 7.1 Sandboxes and placement

**Sandboxes are not pods.** The node agent creates them with youki's `libcontainer` crate. Each one
has:

- a user namespace, with root mapped to an unprivileged range;
- a mount namespace with read-only binds of the tool store, crate store and mirror, plus two
  writable paths: the reflink clone and a tmpfs `/tmp`;
- pid and net namespaces, with loopback only, except that a lockfile-changing build may reach the
  crates mirror;
- a cgroup v2 child of the agent pod's delegated subtree, with `memory.max`, `pids.max` 16384 and
  `cpu.weight`;
- seccomp `RuntimeDefault`, no capabilities, no host paths.

A sandbox starts in ≤ 50 ms [E], against a 6 s pod [M]. The agent pod requests velnor-new's share of
the node and has no CPU limit, so Kubernetes still accounts for it.

**Placement** is done by velnor-control in each site. It minimises predicted finish time:

- set-up: 0.3 s with a matching snapshot, 40 s with a refill;
- the run time predicted from the ledger's history;
- the node's load.

A twin always lands on a different node from its original.

### 7.2 Zero queue, zero cold start, all 480 threads

- **Two CPU classes, enforced by the kernel.** Interactive work is PR and group verification and
  release jobs, at `cpu.weight` 1000. Batch work is audits, the builder, speculation and conformance,
  at weight 10 with `cpu.idle = 1`, the cgroup form of `SCHED_IDLE`. Interactive work therefore
  displaces batch at the next scheduler tick, with no eviction and no queue.
- **Memory.** A batch sandbox is frozen through `cgroup.freeze` when an interactive one needs memory,
  and killed if the pressure persists. Batch work is idempotent and restarts.
- **No CPU limits.** cargo `-j` and nextest's threads equal the node's free threads at start, so a
  lone verification can use all 96. Each sandbox shares one jobserver across its lanes.
- **Admission is memory only.** An interactive sandbox reserves 16 GiB; the heaviest job peaked at
  15.7–16.8 GiB [M §B.2.2]. That is 6 per node and 30 in total, against an estimated peak of 3–6
  concurrent verifications [E: 33 merges a day × 1.5 verifications × ~1 min].
- **No cold node takes interactive work.** Every node keeps the last 3 `main` generations per root
  warm: about 22 GB a generation (9.0 + 10.6 GB + the small roots [M]), × 1.5 with incremental [E].
  A new node takes interactive work only after a 40 s fill [M]. Stores are provisioned when the
  agent starts, and the job image is already on every node (0 real pulls in 1,424 events [M §B.6]).
- **Spend idle CPU on latency.** After dedupe, velnor-new uses about 1–2% of the cluster's CPU
  [E: 17 sandbox-min × ~16 threads × 33 merges ≈ 150 thread-hours, against 11,520 a day]. The design
  spends the idle remainder on twins, speculation, warmers on every node, pre-push verifies, and
  spreading audit tests across nodes.

### 7.3 Isolation for trusted authors whose code may be buggy

- **The threat model.** Bugs, not malice: a test that deletes files, fills a disk, leaks processes,
  takes a port or corrupts a cache.
- **Containment.**
  - Every sandbox gets an XFS project quota (local-path enforces none) and `pids.max`.
  - `cgroup.kill` runs on exit, followed by an absence proof.
  - Each sandbox has its own port space, so the 12 serialised golden tests stop colliding across
    sandboxes.
  - Stores are read-only.
- **Outputs.** The agent harvests a sandbox's outputs when its build phase ends and before any test
  runs, so a test that writes into `target/` can touch no receipt and no snapshot.
- **Docker-needing tests** (`local_pair_public`, `local_host_public`, velnor-local's Docker suites):
  rootless dockerd inside the sandbox's user namespace is a phase-2 experiment, since overlay works
  in a user namespace on kernel 6.12. The fallback is v2's privileged dind class on one tainted node
  per site.
- **Forks.** None (decision 8). A PR whose head repository differs from its base never receives a
  green `Required`.

### 7.4 Both sites, the control plane and the other organisations

- **Per-site services.** Each site runs velnor-control (two replicas), velnor-dispatch (active and
  standby), velnor-store with its mirror, and `mbx-cache`. DE holds the long-retention stores on its
  34–50 TB disks [M §B.1.4].
- **Receipts across sites.** A request stays in one site. Receipts, a few KB each, replicate
  asynchronously between sites. A lookup that misses waits ≤ 1 s for replication before executing.
- **Control plane.** k3s has one server [M]. Sandboxes are created by agents, not pods, so its loss
  stops deployments, not verification. Three servers are still a precondition of the executor
  cutover, as v2 has it.
- **Other organisations.** ChainArgos and jackin keep ARC, with guaranteed CPU requests, their own
  namespaces and no shared stores. `cpu.idle` keeps Velnor's batch work off their CPU. Moving them
  to Velnor is phase 5.

### 7.5 Host and kernel tuning (no credit until measured)

| Change | Expected | Basis |
|---|---|---|
| Kubelet reservations (3 CPU, 6 GiB), `memory.available` 2 GiB, `podPidsLimit` 16384 | Removes the 1.2–2.4× stretch under load | [M §A T6, §B.1.3] |
| Work directories on the XFS local-path volume with project quotas | Reflink instead of an ext4 copy | [M cluster probe] |
| Batch class with `cpu.idle` | Verdict P99 unchanged while audits run | Measure: P99 with and without audits |
| Governor set to performance; THP `madvise` against `always` for rustc; dirty-writeback limits | 2–10%; unknown; smoother write bursts | Measure first; SMT on and kernel 6.12 stay |
| `wild` against the default rust-lld for test-binary links | Faster delta links | Measure |
| Same-site 10 Gbit/s | Only if audit spreading moves binaries often | §6.2 |
| tmpfs work directories | Rejected: reflinks cannot cross into tmpfs, and the slow tests are CPU-bound | v2 I099, I100 |

## 8. The merge queue and GitHub integration

### 8.1 The owner's label flow, on GitHub's own surface

The App subscribes to these webhooks:

- `pull_request`: opened, reopened, synchronize, labeled, unlabeled, ready_for_review, edited, closed;
- `merge_group`: checks_requested, destroyed (this event reaches GitHub Apps only [docs: webhook
  events and payloads]);
- `push`;
- `check_run` and `check_suite`: rerequested.

| Event | Label | Velnor's action | `Required` on that head |
|---|---|---|---|
| opened, reopened, ready_for_review, synchronize | absent | Creates the check run `queued`, titled "Not requested: add `velnor:verify`" (flow steps 1–2) | Pending, which blocks |
| labeled `velnor:verify` | — | Plans the head merged with its base and runs the missing obligations (step 3) | in_progress → success or failure |
| synchronize | present | New head, new check. Old-head work the new head does not need is killed now; shared work is joined (step 4) | Old SHA: `cancelled` (superseded) |
| unlabeled | — | Cancels in-flight work; no further runs. A completed verdict stays true for its head (step 5) | In progress → `queued`, "Not requested" |
| edited (title, body) | any | Nothing; a base change re-plans | Unchanged |
| `check_run.rerequested` | present | Re-runs only the failed obligations, charged to the flake budget | in_progress |
| `merge_group.checks_requested` / `.destroyed` | — | Verifies the group's head tree (§8.4) / cancels it | On the group's head SHA |
| push to `main` | — | Promotes receipts; builder, audit and release triggers | — |

Nothing else starts verification: there is no auto-labelling, and neither ready-for-review nor
auto-merge adds the label.

**Why a check, not a job.** "A job that is skipped will report its status as 'Success'. It will not
prevent a pull request from merging, even if it is a required check" [docs: using conditions to
control job execution]. A workflow job gated by `if:` on the label would therefore let an
unlabelled PR merge. The App's queued check can be satisfied by nobody but the App.

### 8.2 The ruleset

```json
{"name": "protect-main", "target": "branch", "enforcement": "active", "bypass_actors": [],
 "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
 "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}, {"type": "required_linear_history"},
  {"type": "pull_request", "parameters": {"required_approving_review_count": 0,
   "allowed_merge_methods": ["squash"], "required_review_thread_resolution": true,
   "dismiss_stale_reviews_on_push": true}},
  {"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": false,
   "do_not_enforce_on_create": false,
   "required_status_checks": [{"context": "Required", "integration_id": "<Velnor App id>"}]}},
  {"type": "merge_queue", "parameters": {"merge_method": "SQUASH", "grouping_strategy": "ALLGREEN",
   "max_entries_to_build": 5, "min_entries_to_merge": 1, "max_entries_to_merge": 5,
   "min_entries_to_merge_wait_minutes": 0, "check_response_timeout_minutes": 15}}]}
```

How this enforces the binding rules:

- **Entry.** A PR enters the queue only "once a pull request has passed all required branch
  protection checks" [docs: managing a merge queue]. The check belongs to the head SHA, so any push
  makes it pending again.
- **The group.** The queue requires the same check on the group's head SHA.
- **One writer.** `integration_id` binds `Required` to the App. Today any App, or a workflow with
  `statuses: write`, can post it (v2 N12).
- **No bypass.** The ruleset has no bypass actors. Break-glass means the owner edits the ruleset, and
  that edit is audit-logged.
- **ALLGREEN** makes every landed commit verified. An ejection costs seconds, because the rebuilt
  groups are lookups.
- **Timeout.** `check_response_timeout_minutes` falls from 90 to 15, because a group takes seconds.

### 8.3 How label events reach Velnor, and the fallback

GitHub documents that events triggered by `GITHUB_TOKEN` create no workflow run, and that
"Other `pull_request` activity types (such as `labeled`, `edited`, or `closed`) do not create
workflow runs" [docs: trigger a workflow]. The webhook docs
document no matching suppression of App deliveries. Delivery for a label added with `GITHUB_TOKEN`
is therefore an assumption. Work item 3 tests it in week 1: a lab workflow adds the label with
`GITHUB_TOKEN`, and `GET /app/hook/deliveries` shows whether a delivery arrived.

The design does not depend on the answer:

1. The owner labels through the UI or `gh`, with a user token, which is delivered.
2. A reconciler runs every 30 s. It searches open PRs with the label, compares their heads with
   their check runs, and starts what is missing. It also lists `gh-readonly-queue/main/*` every 10 s.
3. Failed deliveries are replayed through `POST /app/hook/deliveries/{id}/attempts`.

The worst case is a 30 s late start, never a wrong verdict.

Docs cited, read on 2026-11-19:

- [Using conditions to control job execution](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/using-conditions-to-control-job-execution)
- [Trigger a workflow](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
- [Managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
- [Webhook events and payloads](https://docs.github.com/en/webhooks/webhook-events-and-payloads)

### 8.4 No duplicate verification across PR, merge group and `main`

- **The PR** verifies the head merged with its base (§6.5).
- **The merge group.** The released planner recomputes every obligation on the group's tree. An
  identical tree takes the PR's verdict; otherwise each obligation is looked up by key. Obligations
  with new keys run as a twin.
- **`main`.** Nothing runs: the group's check already sits on the same SHA.
- **Releases** read `trusted` receipts only.

### 8.5 Speculation on the queue tip

Velnor reads the queue through GraphQL `mergeQueue(branch: "main")`, taking each entry's position
and head commit.
On every queue change, and when a green PR has auto-merge set, it predicts:

- the group trees `T_i = T_(i-1) ⊕ PR_i` (`git merge-tree`);
- the ejection trees without entry *k*, for *k* ≤ 2.

It runs their missing obligations at batch priority. A wrong guess costs idle CPU; a right one makes
the group's lookup instant. 122 of 193 group runs were built on unlanded group commits [M v2 M5], so
predicting stacked prefixes is the norm. The kill criterion is below 70% tree match over a two-week
shadow.

### 8.6 The `Required` check and the release-gate constraints

**Today's gate.** `velnor-ci release-gate` accepts `Required` only from GitHub Actions, and only from
`push` runs on the exact SHA (`crates/velnor-ci/src/gate.rs:15,137`).
`product-release.yml` verifies "main and latest Required CI".

**The change.** Work item 4 changes both in one PR. The gate accepts the App's `Required` on the
exact SHA, which is the group's check, plus the ledger's signed verdict: `trusted` scope and the
release artifact digests.

**The release workflow** triggers on `push` to `main`, because no CI run remains for `workflow_run`
to follow, and runs on the native executor: gate → release-plz PR or tag → publish the promoted
bytes, fetched by digest and checked against `SHA256SUMS` → attest.

### 8.7 Auto-merge and agent behaviour

- **Auto-merge.** Agents set squash auto-merge when they open a PR, so a PR enters the queue the
  moment its check turns green. Today 0 of 100 PRs set it, and the green → enqueue median is 7.5 min
  [M].
- **Procedure.** Agents stop merging `main` into branches and rebase only on conflict. Drafts push
  freely, and nothing runs.
- **Pre-push verification.** Before asking for review, agents run `velnor verify`, which pushes to
  the site mirror and verifies in the same sandbox class. When the owner labels a head with that
  tree, the verdict takes about 2 s.

### 8.8 The cancelled-run stall

Today a superseding push waits up to 2,203 s (626 s at P90) for the cancelled run's `always()`
`Required` job [M v2 §1.1]. In this design no run exists per push. Cancellation is a `cgroup.kill`
(< 10 ms) of the work the new head does not need, and shared work continues. In the Actions
workflows that remain, velnor-gen emits no `always()` job inside a cancellable concurrency group, and
a cancelled job ends in ≤ 10 s (§3.7).

## 9. What would make CI not a bottleneck at all

**The floor.** For a change Δ, the push-to-merge floor is

`hook (1 s) + plan (0.5 s) + C(Δ) + T(Δ) + report (0.3 s) + enqueue (2 s) + GitHub creates the group (19 s [M]) + hook and lookup (1.5 s) + GitHub merges (26 s [M])`.

- **Fixed terms: about 50 s,** of which 45 s is GitHub's merge queue.
- **C(Δ)** is the longest chain of incremental rustc invocations and links the change forces: about
  5 s for the median generator PR and about 19 s for `velnor-runner-host` [E].
- **T(Δ)** is the slowest test the change affects: about 5 s for the median, and 125–138 s for runner
  PRs today, ≤ 10 s after §6.3.

**For this repository:**

| Case | Floor |
|---|---|
| Change verified before the push (`velnor verify`, overlapping the agent's own work) | ≈ 50 s |
| Median PR without pre-push | ≈ 60 s |
| Runner-host PR after the test-floor work | ≈ 80 s |

**The design that reaches it.** Everything in §3–§8, with five conditions met: incremental compile;
the test floor at ≤ 10 s; the `velnor-runner-host` split so C(Δ) is bounded by a smaller crate;
queue speculation hitting ≥ 90%; pre-push verification as the agents' habit.

**Irreducible costs:**

1. GitHub's merge-queue latency, 45 s [M]. Only Velnor landing merges itself (v2 question 11) removes
   it, and decision 7 keeps GitHub's queue.
2. Event delivery, about 1–2 s.
3. The rustc critical path of the change, bounded by crate size.
4. The slowest test the change touches, bounded by test design.
5. About 6 API round trips at about 120 ms each.
6. Human time: the owner's label and review, which the metric reports beside CI and never inside it.

At this floor every remaining second belongs to GitHub or to the change's own compile and test, so
CI is not a bottleneck by construction.

## 10. Critique of v2

| v2 section | What it gets wrong or misses under the new decisions | Overturned by |
|---|---|---|
| §3.4, §6.1, §6.6, M1–M3 | Verification starts on every push webhook, which contradicts decision 12. There is no label flow, no pending state for unlabelled PRs, no handling of `labeled` and `unlabeled`, and no analysis of `GITHUB_TOKEN` delivery | §8.1 state machine; the clock starts at the later of push and label |
| §3.4 step 4, §6.1, §6.4 | `Required` stays an Actions job (integration 15368). Any workflow job named `Required` satisfies the ruleset, and an `if:`-skipped job reports Success [docs]. It needs a run per push and keeps a push run on `main` for release duties | App-owned check bound by `integration_id`; no push run; the gate reads the group's check on the same SHA |
| §3.1 table, §3.10 | Keeps velnor-legacy's Expressions2 transcription as the "one evaluator". The runner calls that one `_legacyEvaluator` and still returns its results (`PipelineTemplateEvaluatorWrapper.cs:19-20,312-318`); decision 3 bans it, and the expression gate is undefined where the two differ | Port the new evaluator; oracle in compare mode; velnor-gen lint (§3.4) |
| §3.5 | The closed set misses `actions/create-github-app-token`, which velnor-gen's `release.yml` uses, and treats `setup-velnor` as a downloading composite | §4 |
| §3.8, §6.1, §13 Q7 | A fork class on hosted runners, fork-conditional `runs-on`, and hosted macOS builds kept | Dropped (decisions 8 and 9); macOS disabled in phase 0 |
| §4.1 | "The sandbox is a sparse checkout of exactly the key's paths": cargo cannot load the workspace without every member's manifest and sources [M, cargo 1.99.1], and an absent file makes a negative lookup pass silently | Full tree plus the syscall read audit (§6.4) |
| §1.2, C6 | Three parallel executions of changed code and a full run of every landed tree within 24 h. Today's push run is post-merge, so today's pre-merge count is 2. v2 makes 4 executions, adds about one extra full run (≈ 60 min) per merge, and detects within 24 h | Twin plus a batched tip audit within 2 h (§6.5) |
| §5.1 | Incremental compilation "off until measured", though the measured edits are 4–6× faster (§6.1) | Adopted in phase 2 under §6.5 row 4 |
| §6.4, §11 | The release path is untouched: rebuilds at the tag, source preview rebuilt on every push (29 min, 22.6 of them macOS), the debug/release profile tension, the embedded `VELNOR_COMMIT`, and arm64 builds on a hosted ARM runner | Release obligations in the release's group; promotion; smoke on the shipped bytes (§2b rows 21–26) |
| §7, §3.7 | Pre-created single-use pods of a fixed 16 CPU and 24 GiB, 4 per worker: a lone PR cannot use 96 threads, an empty pool puts a 6 s pod start on the path, and k3s sits on the job path | Agent-spawned sandboxes; `cpu.weight` and `cpu.idle`; memory-only admission (§7) |
| §6.5 | HEADGREEN kept until speculation lands, although seconds-long groups make ALLGREEN cheap now and it verifies every landed commit | ALLGREEN in phase 2 |
| §1.4, §10 | Targets (phase 3: ≤ 90 s verdict, ≤ 3 min push-to-merge) assume a run on every push and an executor-dependent merge path. The duplicate audit misses state seal, source qualification, image candidates on every branch, and source preview | §1, §2b, §11 |
| Missing | Runner-version reporting against the 63-day window; webhook reconciliation and redelivery; `merge_group.destroyed`; `check_run.rerequested`; drift detection from production traffic | §3.2, §5.4, §8 |

**Kept from v2:** the receipt and ledger concept; "only what Velnor observed is evidence"; reflinked
snapshots as the hot path; the `mbx-cache` threshold; journal discipline; the test-floor programme;
and no selection relative to the queue.

## 11. Phases

Each exit gate needs at least 300 merges or four weeks, with intervals beside every P50 and P90. A
phase that misses its gate stops the next until the miss is explained. "Hit %" is the share of
compile units served from snapshots or `mbx-cache` (0% across jobs today [M]). "Conformance" is the
corpus pass rate against the latest official release.

| Phase | Scope | PR verdict P50 / P90 | Push to merge P50 / P90 | Min per merged PR | Hit % | Conformance |
|---|---|---|---|---|---|---|
| 0, week 1 | Work item 1; instrumentation; the week-1 experiments | ≤ 12 / ≤ 20 min | ≤ 25 / ≤ 60 min | ≤ 300 runner-min [E: 1.5 labelled runs × 52.9 + queue-only qualification 31 + group 83 + push 62 + seal 19.7 + Linux preview 7.4 ≈ 290] | baseline | corpus skeleton, rate published |
| 1, weeks 2–4 | Work items 2–5; executor MVP, oracle and native actions in the lab | ≤ 4 / ≤ 8 min | ≤ 6 / ≤ 15 min | ≤ 60 | ≥ 90% | ≥ 70% |
| 2, weeks 4–8 | Per-obligation keys with read audit, shadow then graduation; agent sandboxes; incremental; twin and tip audits; speculation; ALLGREEN; overlayfs and rootless-Docker experiments; executor cutover when §5.3 holds | ≤ 90 s / ≤ 4 min | ≤ 3 / ≤ 8 min | ≤ 25 | ≥ 98%; ≥ 85% of groups execute nothing | 100% (cutover gate) |
| 3, weeks 8–12 | `velnor verify`; test floor ≤ 30 s, then 10 s; `velnor-runner-host` split; audit test spreading; high-clock node; tuning | **≤ 60 s / ≤ 3 min** (stretch 25 s / 90 s) | **≤ 2 / ≤ 5 min** (stretch 70 s / 3 min) | ≤ 17 | ≥ 98%; ≥ 90% of groups | 100%, each new release certified within 7 days |
| 4, after week 12 | macOS back as a Mac host class under the same executor and oracle; ChainArgos and jackin to Velnor; a microVM class | Held | Held | Held | Held | 100% on both platforms |

**The first five work items:**

| # | Work item | Scope | Acceptance |
|---|---|---|---|
| 1 | **Stop the waste** (phase 0) | Disable macOS jobs (the `macos-arm64` entry in `release-rust-ci-tools.yml`, the Darwin jobs in `product-release-generator.yml`, anything consuming a macOS artifact). Image candidates on `main` and dispatch only; Source qualification only in the queue; drop `edited`. Interim label gate in `ci.yml`: PR triggers `labeled`, `unlabeled`, `synchronize`, `reopened`, and without the label the plan fails "not requested" (red until item 3 makes it pending). Regenerate so gates run on the cluster; remove the `-j2` caps. Agents set auto-merge and stop catch-up merges; required checks bound to integration 15368 now | No GitHub-hosted or macOS job on the PR or queue path; ≤ 300 runner-min per merged PR over 50 merges |
| 2 | **Warm snapshots and node stores** | XFS work volumes with project quotas; per-node snapshots at `/w/velnor-new`; the mtime rule with a planted older-mtime test; tool, crate and mirror stores; builder and `mbx-cache` per site. Measure a one-file edit, four concurrent jobs, incremental on and off, and a cross-site delta fetch against a local build | An unchanged tree compiles 0 crates with ≤ 1 s of set-up; the older-mtime file is rebuilt |
| 3 | **The Velnor App and velnor-control v0** | Label state machine and the queued `Required`; exact-tree ledger; `merge_group`; reconciler and redelivery; measure webhook latency and delivery of a `GITHUB_TOKEN` label; switch the ruleset to the App's `integration_id` | Webhook P50 ≤ 2 s over 50 lab pushes; a killed webhook still yields a correct verdict within 30 s; a workflow job named `Required` cannot satisfy the ruleset |
| 4 | **The release path** | release-gate v2 and no push run; release obligations in the release's group (x86_64 and aarch64 musl, qemu-user smoke); promotion with truthful attestations; source preview keyed by its closure | A lab release publishes bytes whose digests equal the group's build; nothing rebuilds |
| 5 | **Test floor and selection** | Journal template, one test per case, prefix fixtures; optimised dependencies measured with assertions on; build-script inputs declared; per-test budget report | `Journal::open` ≤ 1 s; no test over 60 s outside a shrinking list; a generator PR runs ≤ 4 min |

## 12. Risks

| Risk | Detection | Containment |
|---|---|---|
| **A false green** from reuse: an incomplete key, an incremental bug, a stale snapshot | 100% read audit when minting; the twin; 2% sampled re-execution on another node; cold tip audits within 2 h; daily lineage restarts | The kind falls back to exact-tree keys; the policy epoch rises and queued entries are re-verified; affected landed commits are listed and reverted by a normal PR; releases use `trusted` receipts only |
| **Protocol breakage:** broker, run service, Results, scale sets, webhooks, the merge queue | Strict types on production traffic; the 15 min canary; the 30 min synthetic PR; the release watch | PR and group verdicts do not use the executor; releases wait; fix inside the 63-day window or roll back to the previous Velnor release |
| **Velnor down, so no `Required` and merges stop** | Health checks, reconciler lag, the queue's 15 min timeout | Two replicas in each of two sites; idempotent `ensure`; the previous release. Break-glass is a logged ruleset edit by the owner, never a success Velnor did not verify |
| **Executor drift on release jobs** | The corpus on every change, nightly and per release; shadow twins | Block the release; roll back the executor |
| **The App key leaks, so `Required` can be forged** | GitHub audit log of check-run writers; the release gate also demands the ledger's signed verdict | The key is readable only by velnor-control; rotation drill; raise the epoch on suspicion |
| **Flakes rise with density, and the twin doubles exposure** | Paired-rerun sampler; budget ≤ 1% false failures per verification | Quarantine only with a covering check; lower density; batch stays under `cpu.idle` |
| **The owner's label becomes the wait** | Label latency reported beside push-to-merge | Agents pre-verify, so a label yields a verdict in about 2 s |
| **FI disks (3.5 TB) fill with incremental snapshots** | Store budgets; alerts at 70% | Three generations; LRU eviction; project quotas |
| **GitHub's queue latency (19 + 26 s) regresses** | Weekly P50 and P90 of both | Nothing inside Velnor's control, under decision 7; report it to GitHub |

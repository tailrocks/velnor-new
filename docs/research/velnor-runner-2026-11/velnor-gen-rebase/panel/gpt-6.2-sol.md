# velnor-gen rebase by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol (`gpt-6.2-sol`) |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebase the Velnor runner plan on velnor-gen

## 1. Verdict in ten lines

1. Keep the design's unifying goal. Build and verify each sealed input once, then promote the verified bytes under a separate release authority. (Owner decisions 10–12.)
2. Make the **verification contract an output of velnor-gen**. Compile the job graph, required obligations, and key recipes once, then give GitHub Actions and the runners the same contract. (`crates/velnor-model/src/plan.rs:22`, `crates/velnor-gen/src/lib.rs:76`.)
3. First restore a real merge gate. The pinned tip failed `Required`. The ruleset still allows a repository-admin bypass and has no integration binding for `Required`. (`regen/E-plan-audit.md:13`.) [Tip run](https://github.com/tailrocks/velnor-new/actions/runs/44584861409)
4. Replace quick-green label mode with proof for the latest head. Draft and ready transitions start no workload. An explicit label arms subsequent pushes. The queue then verifies its own exact subject. (`.github/workflows/rust-ci.yml:5–24,571–625`, owner decision 12.)
5. Reuse obligations with matching evidence. The existing same-SHA lookup would skip push-only obligations that the group never performed. Leave it off and replace it with proof checked per obligation. (`crates/velnor-rust/src/merge_group_skip.rs:9–26`, `.github/workflows/rust-ci.yml:211–267`.)
6. Keep one Rust workspace and the measured job-shape decision. This checkout has 18 crate-alone steps, with the older sample's 31 already reduced by engine deletion. (`Cargo.toml:4–25`, `.github/workflows/rust-ci.yml:32–34,214–267`, `57b65af0db`.)
7. Treat 2.9/6.2 min PR and 7.5/11.3 min push as transitional heavy-run P50/P90. The 0.6 min merge median describes the period with bypass, so the enforced baseline starts after repair. (`regen/C-measurements.md:26–46,67–72,192–198`.)
8. Fix the live tools release gate now and qualify promotion separately. The release checks ancestry and rebuilds both binaries, while a runner release family has no workflow. (`.github/workflows/release-rust-ci-tools.yml:86–186`, `release-plz.toml:30–106`.)
9. Develop the App, sealed verification, and measurement independently of a Rust Actions interpreter. Ship Native as the default after the owner amends the goals and each job type earns migration. (`docs/goals.md:3,16,75–77`, owner decisions 16, 18–20.)
10. Keep the official runner as a permanent oracle and an explicit production choice. Require separate evidence by executor and role, then remove the velnor-new ARC route as its final callers migrate. (Owner decisions 19–21.)

Repo citations are relative to the read-only checkout at `0eb2d76a5c`. A–E below refer to the reports in `regen/`. Owner decision
numbers refer to `docs/research/velnor-runner-2026-11/owner-decisions.md`. The detailed label contract is decision 12, and
decisions 15–17 govern delivery, migration, and reuse review. The 4.3-hour measurement cut ends before this tip. The clock for a
fresh baseline begins with green, equivalent, enforced verification. (`regen/C-measurements.md:10–19,192–198`.) Read-only GitHub
metadata confirmed the tip, its failed push run, the ruleset state, zero `Rust CI` merge-group runs, and the PR dispositions in §6.
The failed check's reported GitHub Actions App ID is 15368. Cluster readings come from the dated evidence pack.
(`docs/research/velnor-runner-2026-11/evidence.md:428–606`.)

## 2. The design mapped onto velnor-gen

### One contract per released planner

- **Generator input.** Keep `.velnor/rust-ci.toml` as the Rust CI input and `.velnor/workflows.toml` for declared workflows. Parse
  through `RawConfig`, `config_keys::KEYS`, and `RustCiConfig`. Use `WorkflowsPlan` for the declared cases. Put repository
  verification semantics in a typed `[verification]` table in the Rust config. Put storage credentials, network identities, and
  capacity in operator configuration. (`crates/velnor-rust/src/config.rs:20–78`, `crates/velnor-rust/src/config_keys.rs:4`,
  `crates/velnor-model/src/repo_config.rs:11`, `crates/velnor-workflows/src/plan.rs:13`.)
- **Generator output.** Derive proposed `VerificationContract`, `ObligationSpec`, `ObligationId`, and `KeyRecipe` in `velnor-model`
  alongside `WorkflowPlan`, `StepKind`, and `VerdictSpec`. In `velnor-rust::plan_with`, materialize semantic obligations before
  placing them in jobs. Derive the graph, gate spec, and a canonical manifest with stable IDs from that inventory. Keep
  semantically meaningful ordering and setup dependencies. (`crates/velnor-model/src/plan.rs:22–113`,
  `crates/velnor-model/src/steps.rs:29–132`, `crates/velnor-model/src/verdict.rs:64–80`, `crates/velnor-rust/src/plan.rs:50–118`.)
- `velnor-gen::expected_with_notes` calls `detect`, checks tool and helper pins, and renders the plan. Extend it to emit the
  contract with its schema and digest. `velnor-render::render` still emits YAML, using the fixed SHA-pinned action set and
  single-command checks. `velnor-gen check` compares all managed outputs to fresh generation. An unkeyed workflow header detects
  drift. Authenticate a published contract through the released compiler, approved base policy, and attestation.
  (`crates/velnor-gen/src/lib.rs:42–117`, `crates/velnor-render/src/lib.rs:74–122`,
  `crates/velnor-render/src/{rules.rs:16–46,header.rs:20–45}`.)
- **Runtime plan.** Refactor `velnor-ci::io::plan::run_plan` so the I/O adapter loads an immutable subject and the approved
  contract, then invokes one pure planner. Keep `select_event`, reverse dependency closure, `plan_groups`, and `policy_runs`. Add
  resolved obligation instances and plan identity to the existing machine-readable outputs. Read the candidate tree as input.
  Resolve requirement changes against approved base policy before the gate can accept them.
  (`crates/velnor-ci/src/io/plan.rs:38–92,145–194`,
  `crates/velnor-rust/src/{select_event.rs:12–44,select.rs:64–88,group_runs.rs:76,policy.rs:90–114}`.)
- **Versioning.** Record generator, helper, schema, resolved policy, and renderer digests. Carry full platform-specific executable
  digests. Qualify a new pin on the lab before the production pin changes. Coordinate helper/config/generated-file migration using
  `helper_gate::check_helper`, the existing lock, and the tools release family. (`.velnor/rust-ci.lock:1–14`,
  `crates/velnor-gen/src/helper_gate.rs:8–59`, `release-plz.toml:30–43`, `docs/goals.md:76–78`.)
- **Already available.** Recorded Cargo metadata, deterministic rendering and goldens, validated group and runner configuration,
  one typed cache-key renderer, and `velnor-ci gate` over typed requirements give this work an implementation path.
  (`crates/velnor-gen/tests/it/golden.rs`, `crates/velnor-gen/src/io/recorded.rs:19`, `crates/velnor-rust/src/groups.rs:25–167`,
  `crates/velnor-render/src/cache.rs:19–61`, `crates/velnor-ci/src/io/required.rs:31`.)

### Verification ledger and verdict keys

- **Generator-time schema.** Give every required obligation an ID, exact command and input schema, environment requirements, event
  eligibility, dependencies, expected result shape, and reuse class. Derive test-group membership, feature lanes, doctests,
  explicit harnesses, policy, and freshness from the same inventory. Use signed, narrowly scoped reductions for deleted obligations
  or expected test IDs. (`crates/velnor-rust/src/{plan.rs:77–106,required_job.rs:18–89,group_runs.rs:76}`,
  `docs/proposed/velnor-runner-architecture.md:73–77`.)
- **Plan-time key.** Version and hash a canonical typed encoding with domain separation. Include repository trust domain,
  definition digest, effective selection and configuration, frozen dependency and tool digests, full materialized tree, environment
  and filesystem rules, and executor and execution fingerprint. Include event, commit/ref or dependency digests when behavior reads
  them. Carry subject and exact tested tree in a separate signed plan binding so identical tree inputs can be reused across SHAs
  when commit identity is irrelevant. (`docs/proposed/velnor-runner-architecture.md:487–521`,
  `docs/research/velnor-runner-2026-11/owner-decisions.md:18–19,43–54`.)
- Resolve tool/image digests before scheduling. Defer a test-run key that needs binary digests until its build has proved the
  bytes. Absent resolved input means execute fresh or block if it cannot be resolved. The truncated `compat` and delta `base8`
  cache keys and the workflow header have different guarantees. (`docs/rust-ci-cache-keys.md:7–14,65–71`,
  `crates/velnor-render/src/header.rs:20–45`.)
- Start with the full tree per obligation. A workspace can have different clippy, default nextest, feature nextest, and doc
  obligations using that tree. Package selection already narrows tests. Narrow a verdict key only when enforcement covers all
  possible reads, including path modes, directories, absence, symlinks, tool configs, other roots, and processes it spawns.
  Dep-info and read traces propose or refute a contract. Enforce it inside the sandbox, including undeclared external input and
  nondeterministic dependencies. (`docs/hard-decisions.md:486–513`, `crates/velnor-rust/src/select.rs:64–106`,
  `docs/proposed/velnor-runner-architecture.md:487–518,568–596`.)
- **Runtime decision.** Keep `RustSelection::{None,Some,All}` for affected scope. Replace the independent skip bit with a
  disposition for each obligation, such as `Execute`, `Reuse(ValidatedEvidence)`, or `NotApplicable(Reason)`. Extend
  `required_job::spec`, `VerdictSpec`, and `velnor_rust::decide` to consume validated dispositions. Use `StepKind` and renderer
  conditions to project the plan, with the gate checking the same plan digest. (`crates/velnor-model/src/selection.rs:45–84`,
  `crates/velnor-rust/src/{required_job.rs:77–149,verdict.rs:207–316}`, `crates/velnor-render/src/gate.rs:49–105`.)
- `decide` currently judges job dispositions and checks package-level test counts. The new receipt validator compares exact
  expected IDs, duplicate and missing IDs, exit and teardown facts, run identity, and the original selection. Validate a no-test
  plan independently of a run's self-reported successful exit. Never accept a raw job output or an Actions artifact as a minting
  decision. (`crates/velnor-rust/src/verdict.rs:218–315`, `docs/implemented/job-trust-boundary.md:3–27`.)
- **Runner-side service.** Put durable `ensure`, leases, revocation, admission, and promotion in `crates/velnor-ledger`. Keep the
  contract and key types in `velnor-model`. Only the trusted node observer can attest executions. Two new-key witnesses must occupy
  distinct slots, nodes, attempts, order seeds, and verified generations or a cold build. Initialize the witness requirement in
  policy and test the independence assumption. (`docs/proposed/velnor-runner-architecture.md:97–112,523–556`.)
- The node observer and signer own a separate control channel and bind all executed binary digests, result IDs, source, sandbox and
  epoch. One receipt fills one slot. The ledger atomically reserves slots and records outcomes. It can join a running key, remove a
  superseded request's claim, and keep work needed by another claimant. An unavailable or revoked record requires fresh qualified
  evidence. (`docs/proposed/velnor-runner-architecture.md:546–556,598–624`.)

### App status, request flow, and merge subjects

- **Generator-time.** Teach `PrCi`, `PrRun`, `PullRequestType`, `plan_budget::triggers`, `required_job::verdict_job`, and
  `velnor-render::events` a strict label contract. Give the label one canonical spelling, `velnor:verify`. The recommended
  migration removes dogfood's `ci:run` and `ci:auto` routes, their quick-pass branches, and the label-removal job.
  (`crates/velnor-model/src/{repo_config.rs:146,plan.rs:115–127}`,
  `crates/velnor-rust/src/{plan_budget.rs:49–75,required_job.rs:106–148}`, `crates/velnor-render/src/events.rs:8–23,40–57`,
  `.github/workflows/rust-ci.yml:613–625`.)
- **Interim Actions gate.** The one required job must actively reject an unrequested or draft event whenever GitHub would otherwise
  count a skipped job as successful. Keep harmless unrelated events out of cancellation, test them against actual required-check
  resolution, and keep payload handling narrow. A small explicit denial may run during the interim. The workload remains idle. A
  lab must prove cancellation and latest-head enforcement before using this producer for protection.
  [GitHub required-check semantics](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
- **Final runtime.** `crates/velnor-control` owns label state, durable event reconciliation, and the single expected-source commit
  status `Velnor / Verified`. Read the contract from the released planner. The App posts `success` only for an admitted plan with
  complete, admitted evidence. Publish the optional `Velnor / Details` check run under a separate name. Use `statuses: write` only
  in the writer, `checks: write` only for details, and a scoped agent identity for label and enqueue operations.
  (`docs/proposed/velnor-runner-architecture.md:825–867`.)
- Draft pushes and ready events create no verification request. A ready label arms the current head, labelled pushes supersede it,
  and removing the label cancels remaining work. A verdict already published for the exact SHA survives disarming under the
  design's commit rule. A newly unlabelled head has no passing status. Recommend a carried draft label stays unarmed. Keep base
  retarget detection and re-evaluate subject and policy. (`docs/proposed/velnor-runner-architecture.md:838–867`,
  `docs/research/velnor-runner-2026-11/owner-questions.md:139–151,377–380`.)
- Check live state before publication. Use monotonic request IDs, durable per-PR state, a single writer for a repository, and
  reconciliation after a lost response. Out-of-order events, a new head, group destruction, and epoch changes must fence stale
  completions. Reconcile by poll when an event is missing. Test App and user label changes, as `GITHUB_TOKEN` label changes do not
  launch the required workflow.
  [GitHub workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
- Store head SHA, base SHA, computed tested tree, and policy epoch together. GitHub sees the commit status on the PR head or the
  exact group head. Enqueue also reads the live latest head. Verify how GitHub chooses head versus test-merge checks and how it
  handles two PRs at the same head. Share a requirement only under the same protection contract. A changed group or base always
  resolves its own plan.
  [GitHub required-check SHA selection](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

### Reuse at each stage

| Stage | Runtime behavior and evidence |
|---|---|
| Agent request | The node resolves a snapshot against the requested queue base using the released planner. It re-hashes the actual materialized tree. Qualified node receipts enter the candidate ledger, while a local advisory result never fills a slot. |
| Latest PR head | `velnor-control` independently resolves the current head under the active policy, looks up matching keys, and runs misses and fresh work. Changes after an agent run can change the result's tree even when the head tree itself is unchanged. |
| Merge group | Resolve from the real group commit and base, including prior entries. Use both witnesses for equal enrolled keys only. Execute misses and fresh obligations. An earlier prefix's failure blocks landing that prefix under ALLGREEN. |
| Default branch | Reconcile every landed first-parent commit, including intermediate commits when one push advances several. Check the matching group and landed proof, then run the independent cold landed-commit audit and any expressly post-merge operations. It can promote a candidate to trusted after those checks. |
| Release | Resolve from the tagged default-branch commit and the artifact bytes proven for it. Require the landed verification and its audit. Tag events authorize verified digest promotion. |

- Different input requires new evidence. Time, network, unseeded randomness, event context, publish actions, and git identity stay
  fresh until fixed and keyed. Equal trees alone cannot authorize reuse for those operations. Each reuse class gets cold
  comparison, seeded fault tests, and epoch revocation. First disagreement suspends the class and holds dependent releases.
  (`docs/proposed/velnor-runner-architecture.md:508–521,558–586,608–624`.)
- `velnor-ci merge-group-passed` is the existing prior-run lookup and is off in this config. It asks for a completed successful
  workflow run on the push SHA by event and file. `merge_group_skip::apply` marks the check steps, while the gate has a
  push-specific evidence exemption. Preserve its fail-wide error behavior and delete the original route when the typed disposition
  replaces it. Use it as a shadow telemetry source while queue proof is measured. (`.velnor/rust-ci.toml:4–32`,
  `crates/velnor-ci/src/merge_group.rs:42–85`,
  `crates/velnor-rust/src/{config_ci.rs:7–17,merge_group_skip.rs:9–57,verdict_plan.rs:99–115}`.)
- Keep candidate records reusable across matching, isolated requests after qualification. Trusted compilation generations and
  release bytes require a different admission policy. PR results do not supply trusted writable caches. Cold audit and generation
  qualification precede promotion. The existing MBX trust rule and PR delta scopes are precedents for the distinction.
  (`crates/velnor-ci/src/mbx_store.rs:31–48`, `docs/rust-ci-cache-keys.md:55–63`,
  `docs/proposed/velnor-runner-architecture.md:554–556,614–624`.)

### Release promotion by digest

- **Generator-time.** Extend `velnor-release::ReleasePlan` and `BinaryPlan` with a release-family producer contract, target/profile
  matrix, required smoke tests and provenance, and a tag promotion consumer. Render one producer and one verifier/publisher in
  `velnor-render::render_release` and `release_binaries.rs`. The repo ships one hand-written tools workflow today. Port it to one
  generated authority after proving matching archive names, `COMMIT`, and member digests.
  (`crates/velnor-release/src/plan.rs:58–100`, `crates/velnor-render/src/{release.rs:42,release_binaries.rs:257–293}`,
  `.github/workflows/release-rust-ci-tools.yml:140–253`, `generator/CONFIG.md:303–317,595–610`.)
- **Runtime.** Extend `velnor-ci::gate::release_gate` to require the configured push workflow or signed audit, exact tag/commit
  binding, and exact producer/artifact identity. In the App phase verify the expected App status and policy epoch through their own
  typed inputs. Validate current ruleset applicability and fail on unreadable evidence. Keep one release authority across tools and
  the future Linux `runner-v` route. (`crates/velnor-ci/src/{gate.rs:57–139,141–172,187–268,292–303,cli_release.rs:11–23}`,
  `release-plz.toml:45–106`.)
- **Build location.** Tests keep the development profile. Build optimized Linux musl bytes once per eligible version and target in
  a qualified producer, preferably in or beside the group under a read-only job. If the group SHA differs from the landed SHA,
  perform that build at the first confirmed landed SHA while the build embeds `VELNOR_COMMIT`. Bind the release to the real landed
  commit and digest. Introduce stamp-after-link only after proving the stamp followed by smoke and attestation has the intended
  semantics. (`Cargo.toml:113–118`, `.github/workflows/release-rust-ci-tools.yml:115–169`,
  `docs/proposed/velnor-runner-architecture.md:893–920`.)
- Verify and publish the same immutable artifact object and manifest. The privileged job checks out nothing, executes no candidate
  code, verifies provenance and digests on the bytes it downloads, and treats an existing identical release as already done.
  Quarantine conflicting bytes. Expired material blocks promotion. Retention follows slow release readiness and revocation windows.
  The current tools job already separates build and publication and checks digests.
  (`.github/workflows/release-rust-ci-tools.yml:106–112,187–253`, `crates/velnor-ci/src/cli_release.rs:30–34`.)
- Keep hosted build/provenance policy as the default until an owner-approved replacement binds the actual builder and tag source.
  The generated attest verifier rejects self-hosted attestations and its documented source digest names the attesting workflow
  context. That does not establish where the artifact was built. A hosted signature cannot fill the missing producer proof.
  (`crates/velnor-render/src/{release.rs:31,release_attest.rs:33–73}`, `generator/CONFIG.md:567–575`.)

- Retain one generated push audit reporter for release proof when the App becomes the sole PR/group gate. Replace
  the release helper's generic ruleset-name lookup with explicit App and push-audit proof requirements. The current
  `--pr-only-check` exemption and non-Actions App restriction cannot express that combined authority. Resolve the
  current `family_tag` and `attest` incompatibility before declaring an equivalent generated release.
  (`crates/velnor-ci/src/{gate.rs:246–268,292–303,cli_release.rs:11–23}`, `generator/CONFIG.md:575,595–610`.)

### Agent interface and execution

- **Generator-time and runtime.** The contract carries stable check names for `velnor check` and `velnor verify`. Expose the same
  resolved plan and explain output through `velnor-ci plan` and a proposed `velnor-ci obligation exec` for sealed execution. Extend
  `HelperCommand`, renderer dispatch, and the declared workflow helper allowlist where the new command is actually called.
  (`crates/velnor-model/src/steps.rs:158–167`, `crates/velnor-ci/src/io/plan.rs:38–92`, `crates/velnor-render/src/steps.rs:279`,
  `crates/velnor-workflows/src/helper_call.rs:9–47`.)
- **Runner-side.** A `velnor` client and `velnord` per-user daemon (called `velnorD` in the brief) accept requests, resolve before
  execution, cache advisory diagnostics locally, and stream stable JSON. `verify` submits an immutable snapshot from a private
  index and returns the exact tree and tested base. The remote service re-hashes it before admission. `ship` is an explicit
  verify/push/label/enqueue request, with the ledger independently resolving the observed pushed head.
  (`docs/proposed/velnor-runner-architecture.md:947–993`.)
- Keep ignored files out of snapshots, make the transmitted path inventory reviewable, restrict wip refs, and expire them. The
  node-local socket accepts requests only. Mac or sticky-session results remain advisory. Match wrapper calls only to exact argv,
  profile, environment and release contract. A scratch invocation keeps normal Cargo semantics. Start with explicit verbs and
  opt-in count-only measurements. Migrate the mise wrapper only after the paired replay warrants it and all generator/helper MBX
  callers have a decided replacement. (`mise.toml:13–23,47–49`, `generator/CONFIG.md:109–119`,
  `docs/research/velnor-runner-2026-11/delivery-plan.md:355–367`.)
- **Local starting point.** Reuse `velnor-local` scenario inputs, stable result formatting and replay evidence. It currently uses
  the official runner, the local runner.server stand-in and Rust cache emulator. It already reports which parts were exercised, and
  the development helper override belongs in its conformance workflow. It does not mint production receipts.
  (`local/README.md:3–10,56–62,91–100,230–243`, `crates/velnor-local/src/{cli.rs:20–60,verdict.rs:69–80}`.)

### Dispatcher, executors and platforms

- **Runner-side.** Add `velnor-dispatch` for assignment and placement. Review and retain `velnor-runner-github` scale-set messages,
  `create_session`, acquire, and JIT config types. Keep the protocol boundary between scale-set control and broker job delivery,
  run-service renew/complete, and Results. The latter remains work needed for a Native Actions job. Keep session administration
  separate from the sandbox and signer.
  (`crates/velnor-runner-github/src/{paths.rs:9–23,session/open.rs:55–100,session/jobs.rs:13}`,
  `regen/B-runner-as-built.md:98–120`.)
- `crates/velnor-executor` owns proposed `Executor::identity` and `run`, `Work::{Job,Obligation}`, and explicit
  production/reference roles. `velnor-exec` implements Native with a closed action set. `velnor-official` drives the pinned
  upstream runner. Its production choice earns its own records. Reference replay is isolated from publication and caches that a
  minting run can read. Generated `RunnerLabel` still emits GitHub routing, while the contract identifies certified executor
  capabilities by job type. (`crates/velnor-render/src/steps.rs:53–74`, `.velnor/runner.lock:17–21`,
  `docs/proposed/velnor-runner-architecture.md:258–305`.)
- **As built.** The current Rust host launches official-runner pairs on one Docker engine. ARC is the observed cluster route. The
  repo does not have a production Rust Actions job interpreter. Local mode extends the host-side admission and cleanup code, which
  is valuable conformance input. (`docs/proposed/macos-scaleset-runner.md:37–39,79–115,195–277`,
  `docs/research/velnor-runner-2026-11/evidence.md:533–562`, `regen/B-runner-as-built.md:59–125`.)
- **Platform boundary.** Put `Platform`, `EnvSpec`, `Capabilities`, `Observation` and typed ownership and absence proofs in a
  runner-side crate. `provision/attach/spawn/wait/signal/observe/teardown/recover` accept generic environments and opaque handles.
  Docker and Kubernetes own resource representations and cleanup. Translate immutable Docker identities or Pod UIDs before
  deletion, with `Unknown` retaining the reservation. (`docs/proposed/kubernetes-runtime.md:26–58,181–187`,
  `docs/proposed/velnor-runner-architecture.md:307–358`.)
- Retain `Capacity::reserve`, `CleanupProof::covers`, `reconcile::before_advertise` and the host's intent/call/outcome discipline
  after review. Adapt `connect_unix` and `audit_plan` for a Docker backend. Keep journaling inside the implementations that own the
  effects. Replace the Docker-pair journal layout only as its callers migrate. Preserve crash cases and the local qualification
  path through that change. (`crates/velnor-runner-core/src/{capacity.rs:45,ownership.rs:31}`,
  `crates/velnor-runner-host/src/{docker_client.rs:98,docker_spec.rs:113,reconcile.rs:138–169,journal/schema.rs:26}`.)
- The generator can emit a capability requirement for a Docker test group. Actual capability is verified at admission. A missing
  reflink selects a verified copy. A missing private Docker daemon refuses the Docker class. A failed Native job stays failed. Keep
  the official reference path, and migrate the production route per job type with one deletion at each handoff.
  (`generator/CONFIG.md:276–283`, `docs/research/velnor-runner-2026-11/owner-decisions.md:31–54,58–69`.)

### Storage, cache, mirrors, warm snapshots and MBX

| Component | Generator/helper starting point | Runner-side destination |
|---|---|---|
| Storage and tier policy | Model required artifact durability and cache hints with the contract. Current `CacheKey` and `render_key` own Actions key text. Keep repo controls declarative. (`crates/velnor-model/src/cache.rs:95`, `crates/velnor-render/src/cache.rs:19–61`.) | `velnor-storage::BlobStore` verifies immutable manifests and bytes. Local disk on Docker. Measure node NVMe and per-site RustFS/S3 on Kubernetes, with asynchronous cross-site copies only for warmth. Keep authoritative release retention and ledger consensus independent of caches. |
| `actions/cache` native and v2 service | Preserve YAML semantics and input/output names. `velnor-cache-rules::lookup` already defines exact/prefix lookup in scope. `velnor-local::protocol::route` and the cache conformance fixtures show a working local protocol. (`crates/velnor-cache-rules/src/lookup.rs:8–41`, `crates/velnor-local/src/protocol.rs:38–104`, `local/README.md:230–243`.) | `velnor-cache` owns a repository/ref/key/version index, immutable insert, blob finalize, and scope. Start Native directly over this index. Qualify Official v2 routing with the pinned Worker and cache action, including artifact passthrough. Prefetch only a resolved manifest and re-resolve on use. Any bad blob is a miss and has no verdict authority. |
| Git mirror and checkout | `velnor-ci` already validates the event checkout and reads base ranges. Give the planner exact commit/tree inputs and keep normal Actions checkout behavior where used. (`crates/velnor-ci/src/io/git.rs:36–95`, `.github/workflows/rust-ci.yml:62–66`.) | The node's mirror writer fetches exact objects, refreshes by immutable generation and verifies the complete staged tree before executing. Cover modes, symlinks, deletions, sparse paths and gitlinks. Workers read a pinned checkout without credentials or a mutable mirror mount. |
| Crate and tool mirrors | `[crates_mirror]`, `StepKind::CratesMirror`, the job-scoped helper wrapper, and `tools-local` are already in use. (`.velnor/rust-ci.toml:11–14`, `crates/velnor-render/src/helper_steps.rs:82–88`, `.github/workflows/rust-ci.yml:91–137`, `generator/CONFIG.md:45–68`.) | Extend to read-only checksum-verified crates and digest-verified tool and image packs. Approve upstream sources via locks. Isolate private source material and fetch credentials. The mirror improves transfer. It supplies only immutable inputs needed for a sealed run. |
| Warm snapshots and compile state | Start from the existing input manifest, content-to-mtime restore, dependency audit and PR delta. Treat them as cache mechanisms. (`crates/velnor-ci/src/inputs.rs:194`, `crates/velnor-ci/src/io/audit.rs:58–117`, `docs/rust-ci-cache-keys.md:38–63`.) | The node agent owns immutable generations, a fresh scratch clone, path normalization, trust, and cleanup. Compare every changed, added, removed or retyped path against the generation itself. Unsafe or unknown input forces a rebuild of its affected configurations or the whole root. Different verified generations or a cold build serve the two witnesses. |
| MBX | `mise.toml` wraps Cargo. `mbx-store` shares a node store only on a default-branch push or schedule. PRs and merge groups can seed a private store from a read-only directory. The current config enables the store, with no remote MBX setting. (`mise.toml:47–49`, `.velnor/rust-ci.toml:4–14`, `.github/workflows/rust-ci.yml:83–87`, `crates/velnor-ci/src/mbx_store.rs:31–55`.) | Measure today's seeded MBX, the PR delta, and a certified reflink generation on the same workload. Keep MBX for the classes where it wins cold/remote transport without weakening provenance. If plain Cargo with generations wins a class, move its callers together and delete the MBX steps for that class. |

- Keep the existing cache namespace boundaries and include token-derived repository/ref/version in the index. Chunks and digests
  provide data integrity while the index supplies authority. Run concurrency, immutable-key, cross-scope, stale-index and
  corrupted-content cases. An Actions cache hit is never evidence for a receipt. (`docs/rust-ci-cache-keys.md:55–63,90–119`,
  `docs/proposed/velnor-runner-architecture.md:673–703`.)
- Use the local filesystem before rolling out a distributed store, and price snapshot bytes, disk and page-cache contention, fsync,
  recovery, and eviction alongside restore time. Official reference runs retain upstream behavior for comparison. Treat RustFS
  topology and the Official endpoint rewriting claim as experiments tied to a real pin.
  (`docs/proposed/velnor-runner-architecture.md:641–671,697–703`, `generator/CONFIG.md:109–119`.)

## 3. Correctness first

### P0. Close the routes that can land or release unverified work

1. **Stop using bypass as the merge path and return main to green.** The cutover sample has 157 direct merges of 173, and 167
   captured head rollups include a failing `Required`. Some rollups contain both success and failure. Admin bypass is inferred from
   outcomes, settings, and the recorded merger. The tip push actually failed Lint, Test, the `local-mode` test and lint, then
   `Required`. The policy job skipped. (`regen/C-measurements.md:37–46,159–174`, `.github/workflows/rust-ci.yml:405–409,571–612`.)
   [Tip run](https://github.com/tailrocks/velnor-new/actions/runs/44584861409)

   - Owner freezes ordinary landings and release tags while the preconditions are tested. Diagnose current failures from the exact
     run, using #1555 for triage. Confirm the pinned helper archive and runner/tool environment, and rerun the real nextest suites
     with intended coverage. Fix actual lint, test, and source input drift. The earlier chrono advisory now has an explicit
     exception. Read the current Cargo Deny outcome before attributing a failure to it. (`deny.toml:15`, `5e7783a86e`.)
   - Resume through a successful exact-head and group proof, branch ruleset read-back, empty main bypass list, and an integration
     binding. Keep the merge freeze in any unsafe handoff interval. Owner applies the protected settings and approves any new
     recovery authority. Agents can repair code and collect the evidence.
2. **Eliminate the unlabelled/draft false green.** Draft `Required` prints a notice. The unlabelled ready job can pass after the
   quick set, while the strict helper gate and tests skip. Unrelated label events can skip the required job entirely. There is no
   `unlabeled` trigger. The hand-written lints still run on same-repo draft and unlabelled PR events.
   (`.github/workflows/rust-ci.yml:5–24,277–293,571–625`, `.github/workflows/ci.yml:4–14,41–50,68–78,118–143`.)
   `ci:run` requests one full run and the label-removal job deletes it at startup. `ci:auto` stays attached and requests
   tests on later pushes. (`.github/workflows/rust-ci.yml:277–287,613–625`, `generator/CONFIG.md:545–555`.)

   - Change the generator, the four lint routes, and the App request contract together. Reject a draft or unarmed head while
     allowing a truly empty selected plan to pass after required non-Rust and freshness obligations. Prove no queued stale green on
     new heads, unrelated events, cancellation and label removal. Owner decides the draft-carried label rule and any temporary
     gate-only Actions run. Decision 12 already settles the workload.
3. **Prove the required result came from the required policy.** Released binary digests protect the configured helper. `run_plan`
   still detects facts from the checkout and warns about weakened settings. The YAML itself can change in a PR, and an Actions
   integration ID identifies GitHub Actions rather than one approved workflow. Counts can miss removal of a required test or group.
   (`crates/velnor-ci/src/io/plan.rs:38–92`, `crates/velnor-rust/src/verdict.rs:233–271`, `.github/workflows/rust-ci.yml:585–609`.)

   - Use approved base policy and a released planner, byte-compare fresh candidate generation, and independently verify the
     expected obligation and test inventory. Shadow full parity with deliberate removal and mismatched provenance. Require
     dedicated App authority as soon as it has independently trustworthy evidence. Actions-source binding is a bridge with negative
     lab cases, including a competing workflow name and a check on the wrong subject. Owner authorizes the App and any signed
     reduction.
4. **Close release authority before promotion.** The tools gate checks ancestry and never calls the existing release check. The
   helper's generic `push_evidence` filters repository, event, branch and SHA, but aggregates jobs across push workflows. A release
   must prove the exact workflow, job and effective attempt that produced its authority.
   (`.github/workflows/release-rust-ci-tools.yml:86–105`, `crates/velnor-ci/src/gate.rs:141–172,369–395`.)

   - Finish the useful part of #1608 on the current layout. Verify the precise `rust-ci.yml` push and a successful `Required`, plus
     the tag binding and ancestry. Block pending, missing, skipped, cancelled, wrong workflow/App/attempt, and unreadable checks.
     The existing CLI can exit successfully with `ready=false`. Require the actual ready value at the caller or use a command that
     fails closed for a tag. (`crates/velnor-ci/src/io/mod.rs:322–337`.) In the App phase the release gate explicitly reads queue
     and landed-audit proofs. Protect tags and check the signed producer manifest before publish. Restoring rule 13 needs no policy
     change. New tag authority or provenance policy needs the owner. (`docs/goals.md:77`,
     `.github/workflows/release-rust-ci-tools.yml:16–85`.)
5. **Reinstate the queue with ALLGREEN.** It is configured as SQUASH, HEADGREEN, build depth 2, wait field 5 min and timeout 90
   min. `allow_auto_merge` is false. The observable run count for new `Rust CI` groups is zero.
   (`regen/C-measurements.md:37–46,159–174`, `.github/workflows/rust-ci.yml:13–19`.)
   [Queue contract](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

   - Owner sets ALLGREEN and an actual queue workflow. Read back branch/admin restrictions and prove admission and landing on
     separate exact SHAs. Use a failing predecessor, ejection/rebuild, stale success, missing check, two entries, and post-landing
     reconciliation. Keep depth 2 and timeout 90 min while green-tail data is sparse. Turn on auto-merge only when it drives
     guarded enqueue.

### P1. Restore missing checks and close trust boundaries

6. **Wire generator freshness as a required obligation.** `StepKind::GeneratorCheck` renders `velnor-gen check`, and no planner
   constructs it. The live workflow's `check-metadata` runs only when a root is selected and has a narrower scope.
   (`crates/velnor-model/src/steps.rs:108`, `crates/velnor-render/src/{steps.rs:279,helper_steps.rs:74–80}`,
   `crates/velnor-rust/src/plan.rs:77–106`, `.github/workflows/rust-ci.yml:142–144`.)

   - Emit it before candidate build/test execution through a released, digest-verified generator, with explicit install/tool inputs
     and a safe metadata collection environment. Scope it to all managed workflows and inputs, including declared qualification and
     no-Rust PRs. Enforce intent through the trusted producer because a candidate can edit its own workflow. Verify output drift,
     missing file, snapshot of metadata, and marker body changes. Owner approval is unnecessary for enforcing existing goals.
     (`crates/velnor-gen/src/io/mod.rs:119–128,174–195`, `docs/goals.md:73–77`.)
7. **Preserve every scheduled check through the plan-to-gate join.** `rust` exposes policy and test expectation outputs from
   `steps.plan`. Cargo steps after expiry use `steps.replan.outputs.*` first. On a full replan with broader scope, verdict and
   execution can disagree about policy and which tests were expected. The test gate currently knows reported counts per package,
   with expected-test presence from the plan. (`.github/workflows/rust-ci.yml:39–50,195–210,277–282`,
   `crates/velnor-rust/src/{required_job.rs:152–160,verdict.rs:233–273}`.)

   - Expose one effective plan, fail on missing/failed replan, and require its exact inventory at join. Exercise expired base,
     selected package with no tests, a broadened plan requiring tests and policy, all-ignored selection, filtered feature-lane
     omission, and grouped roots. Retain the merged lane guard and early listing. No owner change for keeping the selected checks
     mandatory. (`47ef96f8ea`, `0eb2d76a5c`.)
8. **Reject whole-workflow reuse until coverage is proved.** The lookup matches workflow file, event and SHA. Its skip list
   includes documentation, feature docs, crate-alone checks, and lane guard. Group runs omit post-merge docs and alone checks.
   Event-sensitive inputs and a changed policy epoch add separate reasons an equal SHA can be insufficient.
   (`crates/velnor-ci/src/merge_group.rs:45–85`, `crates/velnor-rust/src/merge_group_skip.rs:9–26`,
   `.github/workflows/rust-ci.yml:211–267,297–299`.)

   - Keep the option disabled. Measure coverage in shadow using the exact source and effective plan, then replace the Boolean with
     per-obligation evidence. Define which checks are required before merge. Until they move or receive a documented owner-approved
     scope, keep every post-merge obligation on push and nightly. Ask for approval of the new reuse contract and any changed check
     scope. (`docs/goals.md:34–45,66–78`, `docs/hard-decisions.md:14–19,223–229`.)
9. **Enforce cache, artifact and credential ownership at the job or sandbox boundary.** The checkout-free `Required` layout
   separates the aggregator from Rust execution. Cache actions and helper calls after repository code share that job's process/user
   boundary, and some actions receive runner credentials. Build state can also outlive job cancellation if cleanup is incomplete.
   (`docs/implemented/job-trust-boundary.md:3–15,38–59`, `.github/workflows/rust-ci.yml:300–404,571–612`.)

   - Run code with private scratch state and no gate keys, restrict service credentials to trusted I/O, and publish after the
     sandbox exits from sealed, verified data. Check immutable artifact IDs and all content at consumption. Treat `Cache` as
     advisory for performance and reporting. Isolation or integrity failures reject data before verification, and can require cold
     execution. Code enforces existing trust boundaries. Owner approves new infrastructure identities and physical changes.
     (`docs/goals.md:42–43,67,72,77`.)
10. **Restore comparability of qualification and lints.** Qualification is generator-owned and dispatch-only. Its compare job
    currently echoes a marker. `ci.yml` runs four separate lints, including duplicate deny and machete, with different scope and
    arguments from `policy`. The branch ruleset only sees `Required`. (`.velnor/workflows.toml:1–42,474–492`,
    `.github/workflows/qualification.yml:830–840`, `.github/workflows/ci.yml:41–157`,
    `.github/workflows/rust-ci.yml:405–409,480–485`, `regen/C-measurements.md:165–174`.)

    - Make qualification compare exact expected hosted/scale-set results, including intended failures and cache effects, then use
      it as conformance input. Move Alint and Jactionlint into `[[lint]]` and merge dependency checks with equal argument, tool and
      input coverage. Make the executed contract explicit and show original failures still fail. Owner decides about any
      intentional reduction or expansion of merge-blocking scope.

### P2. Keep selection conservative while cutting avoidable full runs

11. **Shallow fetch.** Depth 2 is enough for the current PR path when both parents are available. Push or group planning verifies
    the payload's base/head and ancestry with `range_base`. A deeper base outside the visible shallow history causes
    `BaseUnavailable` and all packages. The precise frequency for this checkout is unmeasured.
    (`.github/workflows/rust-ci.yml:62–66`, `crates/velnor-ci/src/{event.rs:40–65,io/git.rs:36–95,io/plan.rs:166–214}`,
    `crates/velnor-rust/src/select_event.rs:20–44`.)

    - Resolve event base/head in trusted Git I/O, verify ref identity and ancestry, and acquire only the needed objects with a
      bound on work. Keep the all-packages path for missing or ambiguous provenance. Fix in `velnor-ci::io::git` and the checkout
      step model. Test multi-commit push/group, missing object, rebase and stale payload fixtures against a full clone. No owner
      decision for unchanged selection semantics.
12. **Unknown workflow paths.** Only the exact Rust workflow gets `GeneratedWorkflow`. Other workflow paths can be unknown and
    force all. Classify a workflow path only with evidence of every check it can affect. Lint, release and setup files can affect
    verification. (`crates/velnor-rust/src/{classify.rs:29–97,select.rs:64–88}`, `docs/goals.md:40,66`.)

    - Generate specific input ownership from the obligations that read those files. Keep a full selection for changes to the Rust
      contract, toolchain, or inputs the planner cannot place. Verify workflow-only, action-only, release-only, sibling fixture and
      unknown-path changes against a full control. No owner approval for a proved map. An intentional check reduction needs D17.

## 4. Speed on the new baseline

### What the measurements say

| Clock | Observed P50 / P90 | Population and interpretation |
|---|---:|---|
| Old engine PR workflow | 26.8 / 73.2 min | 1,256 run records, success and failure duration statistics. Historical comparator. |
| Old engine merge-group workflow | 26.8 / 76.6 min | 289 runs, 169 successes, 104 failures, and 16 cancelled. All before the cutover. |
| Rust CI PR workflow | 1.1 / 5.8 min | 268 run records, 2 successes. Success/failure run clock, so the number mixes early failures and scopes. |
| Rust CI heavy PR | 2.9 / 6.2 min | 141 runs, 58 failed and 83 cancelled. Created to last job end. |
| Rust CI PR without heavy lanes | 0.8 / 1.2 min | 127 runs, 85 failed. Created to last job end. |
| Rust CI heavy push | 7.5 / 11.3 min | 74 runs, 72 failed. Another 98 push runs lacked heavy lanes and had a 0.9 min median. |
| Self-hosted queue by job | 2–13 s / 13–57 s | PR and push job groups in the cutover sample. Earlier hosted gates queued 287–368 s at P50 in the queue sample. |
| `rust-root` job run time | PR 67 / 365 s, push 371 / 560 s | The older multi-job generated workflow in C. This is evidence of a long Rust lane before the final shape. |
| Latest push to merge, by PR | 0.6 / 3.8 min | 172 post-cutover PRs with a push proxy. Pre-cutover 121.8 / 700.8 min, 187 PRs. Old approvals, queue churn and bypass changed the populations. |
| Tools release workflow | 4.1 / 4.8 min | 10 pushes post-cutover, 9 successes. Measure a release independently from the ordinary merge. |

Sources for the table are `regen/C-measurements.md:10–19,26–46,50–89,95–157,192–198`.

- The code at the tip has one critical chain `rust → policy → Required` when policy is selected. Checkout, tools, plan, cache
  restores, format, workspace clippy, workspace nextest build/test and feature work contribute inside `rust`. Main adds workspace
  docs, the 18 crate-alone checks and cache warming as applicable. The final `Cache` job can extend run wall beyond the verdict.
  (`.github/workflows/rust-ci.yml:34–144,195–305,405–485,571–612,627–666`.)
- `check_alone` is by package, and the `Documentation` command compiles `--workspace` when scheduled. The pinned workflow emits no
  doctest command. The generator supports it when input analysis requires it, under HD11. Top-level nextest selection is an
  execution filter over a workspace build. Cite the actual flags when comparing a local package run.
  (`.github/workflows/rust-ci.yml:205–299`, `docs/hard-decisions.md:486–513`.)
- Report C counts 1.29 PR runs, zero groups, and 0.99 pushes per merged PR for Rust CI. The observed 2.5 + 3.9 = 6.4 summed
  run-wall min per merge, plus the separate lint workflow, is a cost proxy. Runner occupancy and verification completeness need
  their own clocks. Group values for the current workflow are missing. (`regen/C-measurements.md:176–198`.)
- Current causes within the Rust job need fresh attribution. Record queue-to-start, checkout and history resolution, helper and
  tool restore, cache bytes and decompression, first cargo, rustc/link wall and CPU, nextest list/run and slow tests, docs and
  crate-alone, and save/upload. Record policy and gate finish as well. Keep #1748's overlapping listing and compare after a release
  actually carries it. (`crates/velnor-ci/src/io/early_list.rs:30–80`, `.velnor/rust-ci.lock:1–14`, `0eb2d76a5c`.)

- One additional trace at the pinned tip shows the new shape. The failed push's `rust` slot ran for 305 s. Default
  lint ran for 31 s, default and feature tests for 12 s each, and feature lint for 26 s. The policy job skipped.
  This is one failed observation of the current shape. [Pinned push](https://github.com/tailrocks/velnor-new/actions/runs/44584861409)

### Five levers, ranked by expected minutes off a guarded merge

These are planning ranges for an ordinary full labelled PR, including the group. They are hypotheses for paired tests, with
overlaps charged once. Main push and release savings appear under runner occupancy when they occur after landing.

1. **Enqueue immediately after head verification.** Historical green-to-enqueue P50 was 7.5 min and catch-up merges added work. If
   even half the manual wait survives under restored protection, the likely opportunity is several minutes per merge. Measure
   50–200 guarded merges using head-success, enqueue, group creation, group verdict and landing timestamps with the same entry
   load. Compare auto-enqueue with a manual trace, measure re-pushes avoided separately, and keep catch-up merges out of the normal
   route. The 5 min wait setting is the time to meet a minimum merge count, which is already one. Measure GitHub's contribution
   instead of subtracting a fixed five minutes. (`regen/C-measurements.md:37–46,165–169`,
   `docs/research/velnor-runner-2026-11/owner-questions.md:64–76,266–279`.)
   [GitHub merge-queue settings](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)
2. **Eliminate repeat verification on equal obligation keys.** A common group hit can save a Rust pass. At a hypothetical 65% full
   group hit rate and 3–6 min of equivalent work versus a 6 s lookup, the expected group saving is about 1.9–3.8 min per merge
   before fresh work. Calculate from traced obligation costs, including misses and ejected groups. Shadow both results for each
   hit, and measure verified landed proof separately. Reuse of agent work shifts the head's clock earlier and counts its occupied
   minutes once. (`regen/C-measurements.md:67–72,176–189`, `docs/proposed/velnor-runner-architecture.md:558–586,876–891`.)
3. **Reduce the Rust floor on misses.** First target the workspace build and longest test, then post-merge docs and alone compile
   fan-out for occupancy. A candidate saving of 0.5–1.5 min per required execution gives 0.7–2.0 min per merge with one head
   execution and 35% group misses, before extra witnesses and load effects. Pair seeded MBX plus PR delta with a verified warm
   generation and cold control. Include same-PR second push, a changed lock, a new source path, Docker and the longest test. Test
   shape alternatives using identical binaries and coverage. Record build counts, toolchain identity, link time, slowest suite and
   slowest witness, plus CPU, memory and queue time. (`docs/rust-ci-cache-keys.md:55–71`, `crates/velnor-rust/src/shape.rs:10–59`,
   `docs/hard-decisions.md:486–513`.)
4. **Remove the policy tail and duplicate lint work.** Aim to reclaim the serial policy interval above the main test job,
   provisionally 15–60 s where it runs. Use smaller pinned tool sets and start policy from the plan's inputs, overlapping
   independent verification where setup cost allows. Compare one job with inline policy, and a cheap shared plan with parallel
   policy. Preserve the current actual check arguments and coverage before deleting duplicate routes. Count verdict savings when
   policy is critical, and runner occupancy across every event. (`.github/workflows/rust-ci.yml:405–485,571–609`,
   `.github/workflows/ci.yml:68–134`.)
5. **Suppress unnecessary work and cut remaining job setup.** The main event-volume win is idle unrequested heads and prompt
   cancellation. Setup improvements save seconds per executed job. verify exact-range fetch, node tool and Git
   mirror hits, narrower Docker routing, and artifact transfer. Measure cancelled runner-minutes, events with zero admitted
   workload, supersede-to-first-action P90, affected-versus-all plans, and p90 admission under competing load. Use the cold result
   when the warm or exact-range proof is missing. The new median queue data gives no basis to budget minutes of hosted-gate
   savings. (`regen/C-measurements.md:56–69,146–157`, `.github/workflows/rust-ci.yml:5–24,57–116`,
   `crates/velnor-rust/src/select_event.rs:20–44`.)

### Targets and how to qualify them

- Freeze clocks as label or final armed push to success, final armed push to landing, and first actionable failure. Publish enqueue
  and GitHub residence components alongside gross latency. Record audit readiness and tag-to-published separately. For agent
  `verify`, also measure explicit request to landing and total CPU, so precomputation cannot hide work. Keep missing proofs,
  non-merges and ejections in the population with outcomes and survival times.
- **Baseline gate.** Use at least a green tip and a protected canary with the complete approved inventory, then measure a rolling
  set of successful, failing, cancelled and censored heads for each adopter. Collect three matched repeats minimum for mechanism
  measurements and enough per stratum for P90, with cold/warm, no-Rust, leaf, fan-out, lock/tool, feature-Docker, policy/config,
  and simultaneous group changes reported. The previous helper/generator-root 47% class is gone. (`docs/goals.md:49–61,78`,
  `docs/research/velnor-runner-2026-11/explainer.md:34–38`, `Cargo.toml:4–25`.)
- **Guarded Actions planning budget.** Use PR verdict P50/P90 of 4/9 min and push-to-merge of 9/22 min as initial engineering
  targets, then replace them with the measured baseline and a paired improvement budget. For a representative median scenario, 4
  min PR plus 4 min group plus 1 min transition gives 9 min. The tail allowance is 9 + 9 + 4 by allocation. Neither calculation
  adds observed marginal quantiles. The current 2.9/6.2 min heavy PR proxy comes largely from incomplete runs.
  (`regen/C-measurements.md:56–72,192–198`.)
- **Qualified reuse planning budget.** Aim first for 2/6 min PR verdict and 4/12 min push-to-merge against an equivalent workload
  with two qualified witnesses. A median target of 2 min head, short group hit and up to a minute or two of GitHub/action overhead
  leaves margin. Treat a miss as a joint slow path. At 65% hits below the long tail, overall P90 is about the 71st percentile of
  misses, because `(0.90 − 0.65) / 0.35 ≈ 0.71`. One cold witness can dominate that tail.
- **Fast cohort target.** Test 90 s/4 min PR and 3/8 min merge again only when the current miss/witness, audit and GitHub clocks
  make those budgets feasible. An already verified head with zero fresh work can target ≤10 s label to green at P50, with the
  preparation time reported. Keep 30 s/100 s and 100 s/4 min as experiments, with no release date attached.
  (`docs/proposed/velnor-runner-architecture.md:123–138,519–521`, `docs/research/velnor-runner-2026-11/delivery-plan.md:358`.)
- **Measurement path.** Extend `velnor-ci report`, `bench collect/compare/budget`, and `timings`, joining GitHub events to the
  effective plan digest and work receipts. Stage timers distinguish actual compile/output work, cache transfer and waiting. Keep
  `Cache` and speed budgets advisory for diagnosis. Require an owner decision for any tradeoff against an existing target.
  (`crates/velnor-ci/src/io/{mod.rs:307–388,timings.rs:16–59}`, `generator/CONFIG.md:168–190`, `docs/goals.md:59–61,95`.)
- **Speed claim.** Freeze inputs and planted failures before timing. Compare the repaired generated pipeline, a conventional warm
  Official baseline on equal hardware, the same ledger with Official and Native, and explicit agent preparation. Include cancelled
  work, speculative misses, cold audits, release builders and other-tenant load. Use interleaved arrivals and paired P50/P90
  confidence bounds per class. A hidden check, a false green, an unrun arm or a faster narrow class cannot support a general claim.
  (`docs/proposed/velnor-runner-architecture.md:1083–1112`.)

## 5. What to keep, change, drop

### Decisions on the audit groups

E classified the 174 items at its evidence cut as VALID 86, DONE 10, CHANGED 50, OBSOLETE 8, and CONFLICT 20. The dispositions
below account for the later merges and distinguish a source change from its rollout. (`regen/E-plan-audit.md:243–247`.)

- **VALID.** Keep the soundness contract, two witnesses, independent cold/shifted-clock audits, per-backend conformance, verified
  release bytes, typed storage and App privileges. Review the original runner code under decision 17. Put approval and parity ahead
  of platform or executor cutover. (`regen/E-plan-audit.md:76–132,265–329,530–549`.)
- **DONE.** Close the engine and layout parts of B02/B03/B10/B11/B12/G04 against the code. Treat cancelled-run tail measurement
  (B08), the A1/audit part of G04, native action qualification (A09), and runner test topology (T06/T07) as open where they exceed
  the work shipped. `0eb2d76a5c` overlaps test listing on the source side, and production adoption still requires a tools
  release/pin. `61067c17fc` and `47ef96f8ea` are merged. (`053fefa6a3`, `533fc35623`, `57b65af0db`, `.velnor/rust-ci.lock:1–14`.)
- **CHANGED.** Rebase on typed plan plus job roles, with 18 current members and one dogfood root. Drop an assumed scope win from
  the old helper/generator roots. Keep package-level affected selection without promising package-level evidence keys. Re-derive
  costs, release job types, and version-pinned adoption. (`Cargo.toml:4–25`, `crates/velnor-rust/src/select.rs:17–40`,
  `regen/E-plan-audit.md:211–245,325–387`.)
- **OBSOLETE.** Remove engine releases, engine `Required verdict`, state seal, source preview and per-push image workflows, the old
  fixture timings and removed fixture cleanup. Drop X13 as a production dependency while the supported plan emits no background
  group. Retain the supported subset and refusal conformance. Keep the archived legacy code as evidence and scrutinize licence and
  protocol fit before porting a function. (`053fefa6a3`, `533fc35623`, `57b65af0db`,
  `regen/E-plan-audit.md:243–245,287–295,376–383`.)
- **CONFLICT.** Resolve the goals and macOS spec explicitly. HD11 governs compile scope. HD3's old-engine prohibition is important
  context because it rejected coverage-based skipping. The new system needs its own approved, enforced proof. The project's rules
  call for one workflow lane when measurement supports it, verified release authority and reviewed reductions. They do not require
  the obsolete per-crate job graph. (`docs/goals.md:14–16,63–78`, `docs/hard-decisions.md:14–29,223–230,486–513`,
  `regen/E-plan-audit.md:40–56`.)
- **Correction to E on enabling reuse.** Reject E §3's suggestion to enable `skip_push_after_merge_group` after merely reviving the
  queue. It is a safe-miss lookup with unsafe check coverage in this configuration. Also reject converting a matching record to
  `RustSelection::None`. That loses why an obligation is required and makes gate parity unverifiable.
  (`regen/E-plan-audit.md:58–68`, `crates/velnor-rust/src/merge_group_skip.rs:9–26`, `.github/workflows/rust-ci.yml:211–267`.)
- **Additional gaps.** Explicitly track effective-plan mismatches after replan, pin drift under candidate config changes, no
  workflow identity in generic push evidence, the placeholder qualification join, and event semantics when a new workflow is the
  required check producer. Record the current profile around Turso. The dev overrides disable debug assertions and overflow checks
  for both Turso crates. Re-run tests with the intended checks enabled before relying on past claims about an optimized profile
  with assertions on. (`.github/workflows/rust-ci.yml:39–50,195–204`, `crates/velnor-ci/src/gate.rs:141–172`,
  `.github/workflows/qualification.yml:830–840`, `Cargo.toml:99–107`, `docs/proposed/velnor-runner-architecture.md:939–941`.)
- **Documentation.** Retain the old evidence at its historical pin. Correct current specs, code paths and reader-facing examples in
  the course of migration. Keep dated traces in research evidence and actual delivery notes in issues, with contract documentation
  evergreen. (`AGENTS.md:29–44`, `regen/E-plan-audit.md:176–207`.)

### Surviving streams and their first items

| Stream | Revised first item and dependency |
|---|---|
| B, bootstrap | N01 + new B01/B05. Recover exact-head and group proof, then enforce the single-source gate. B02/B03 engine rollout is complete or deleted. |
| RV, review | RV03 inventories current `WorkflowPlan`, gate and release authority first. RV01/RV02/RV05 choose reusable ownership/protocol/wrapper code after goals approval. RV04 records a live local scenario. |
| P, platforms | P01 starts with typed capability and observation proofs. Resolve #1324's pair-oriented trait and journal against the generic environment boundary before P02. Docker first for the lab. |
| O, Official | O01 reviews the existing launch wrapper and pinned upstream behavior. P02 and O02 establish ownership and reference isolation before synthesized obligation jobs. |
| X, Native | X01 becomes a supported-subset and provenance review in `crates/`. Prove released contract execution first. Implement a minimal acquired Actions job with modern protocol after owner approval, then expand after oracle parity. |
| A, actions | Start with inventory from `ALLOWED_ACTIONS`, pins, and actual reachable calls. A01 tests checkout equivalence. Replace needed generic JS with a typed helper where semantics permit, and qualify each migrated job's remaining action/input/SHA set. |
| H, conformance | H04 uses generated `qualification.yml`, failing/refusal cases and local probes. H03 first makes qualification compare real paired observations. |
| L, App gate | L01 proves GitHub identity, latest head and group resolution, skipped-job cases, same-SHA PRs and lost events. L02 runs in development with real proof validation. Replace the Actions PR/group gate after sealed parity and release migration. |
| G, workflow and inventory | G03 derives the released verification contract. G02 adds semantic routing. G01 migrates generated gate/policy/lints, whole Rust job, Docker feature class, then remaining declared/scheduled jobs. Releases have their own policy. |
| V, ledger and audit | V01 keys and validated dispositions, then V02 trusted observation and V03 witnesses. V04 begins with whole-tree shadow. V05 audits every landed commit. V08/V13 become optional, per-obligation enrollment. |
| AN, agent tools | Start AN04's explicit plan/resolve/verify protocol in advisory mode with AN05 trust boundaries. Finish snapshot and remote witnesses, then do AN01 wrapper count-only trials and evaluate routing. |
| C, compile and mirrors | Start C05's generation census with existing inputs, deltas and seeded MBX. C01/C02 extend existing stores. C03 matches exact Git subjects. Compare cold, seeded and snapshot paths before replacing any caller. |
| ST, storage | ST01 defines the verified blob API with local disk first. ST03 adds the cache index after conformance. ST06 qualifies RustFS per route. Release durability has a separate pass criterion. |
| T, test floor | BM08 redraws the one-root floor first. Preserve tests covering the current runner. Replace T02/T04/T05's deleted workloads with measured lints, local-mode, doc/alone, and nextest bottlenecks. |
| S, cluster | S01 measures cancellations, warm slots and occupancy with the new shape. Approve manifest location, scope and witness reservations before a capacity or disk migration. |
| R, release | R02 starts with #1608 and a producer-specific push check. Then qualify digest promotion for tools and deliberate Linux runner publication. R01 waits for owner provenance policy. |
| F, final deletion | Maintain a caller list for ARC, the old journal, legacy helper ports and config. Remove each replaced piece with its last caller and keep the deliberately selected Official executor. |
| BM, measurement | BM03/BM08 begin with recovered current CI and immutable benchmark inputs. BM14 exercises a real guarded queue. Re-use `velnor-ci bench` and telemetry. |
| DG, shared interfaces | DG01/DG04 extend the contract and key policy. DG08 freshness is immediate. DG03 test-run-by-binary is a later proof, and DG11 extends current reports with non-authoritative traces. |

The original stream list and item IDs are in `docs/research/velnor-runner-2026-11/delivery-plan.md:214–468,629–646`. E's
classifications are in `regen/E-plan-audit.md:243–450`. The current action allowlist is `crates/velnor-render/src/rules.rs:16–34`
with SHA pins in `crates/velnor-render/src/pinned_actions.rs:11–72`. The scope includes `actions/github-script`, Docker-related
actions, and declared qualification cases. Native admits only qualified combinations. That is a smaller supported set than all
generator features. (`generator/CONFIG.md:512–531`, owner decisions 4 and 20.)

### Carry E's missing work into the delivery order

| Items | Disposition |
|---|---|
| N01, N02, N03, N04, N06 | Phase 0. Green path, strict label gate, ruleset/queue, freshness and release proof. |
| N05, N07, N23 | Release policy and `runner-v` authority. Carry tag ruleset and App checks into the same qualification. |
| N08, N11, N12, N16, N21 | Owner text and deploy home, one platform proposal, naming, and evergreen contracts. Resolve before permanent deployment or deleting diagnostic tooling. |
| N09, N10, N17, N18, N19 | Re-baseline, build on bench/timings, remove redundant serial work, prove exact range and measure delta cost. |
| N13 | Keep advisory cache waste visible. Split diagnostic failures from data admission and from required checks. |
| N14 | Delete old `[stacks.rust]` and its ports only after the declared, qualification, and local callers migrate. Remove stale ownership heuristics and obsolete documentation with those callers. |
| N15 | Whole tree initially. Enroll smaller keys only under an enforced input contract. Multi-root groups are a shared build unit with separate root obligations. |
| N20, N22, N24 | Derive allowed and qualified action inventory, record the current local conformance scenario, and keep the renderer's size limit. Emit compact obligations outside YAML rather than adding a job per receipt. |

Source for these items is `regen/E-plan-audit.md:522–549`. The surviving selection config is `.velnor/config.toml` after
`57b65af0db`. The renderer already replans without deltas when they alone cause an oversize result.
(`crates/velnor-gen/src/plan_render.rs:15–40`, `crates/velnor-render/src/rules.rs:38–39`.)

## 6. Phase 0 and the first ten work items

### Starting inventory

| PR or work | State observed | Disposition |
|---|---|---|
| #1608, release gate | Open. D's clean merge-tree result predates removal of its old fixtures. GitHub reports DIRTY. | Salvage the exact push verification, replace deleted fixture changes with current coverage, and review which run/attempt can pass. |
| #1320, per-root audit | Closed unmerged, with #1073 open. | Read the old design as input. Build the smallest current per-root audit that proves no duplicate workspace compile and names the audited configuration. |
| #1200, mise platform matching | Open. D reported two commits behind and blocked by checks. Current GitHub metadata says DIRTY. | Bring together the already-reviewed platform resolution and current pins. Finish independently of authority changes. Include the relevant #1700 findings. |
| #1026, input snapshot and target dirs | Open and DIRTY. D identified wide legacy changes and stacked history. | Retain the behavior and reproduce each symptom. Implement sibling and custom target-dir fixes as narrow current-layout PRs. Validate #1730 review findings before resuming work. |
| #1297, dependency batch | Open and DIRTY. The proposed lockfile layout predates the single workspace. | Have Renovate regenerate on the root workspace. Review policy and pin changes independently. Carry a verified security fix as a separate change if it is urgent. |
| #1274, #1335, #1747, #1748 | On the pinned tip. | Delete stale replay work from the schedule. Test combinations and helper adoption. Keep the source-level listing improvement distinct from a released helper run. |
| #1751 | New open research draft, observed in PR list metadata. | Treat it as owner planning material, outside the implementation dependency graph. |

Sources are `regen/D-inventory.md:15–39`, `61067c17fc`, `47ef96f8ea`, `57b65af0db`, and `0eb2d76a5c`.
[Current PR list](https://github.com/tailrocks/velnor-new/pulls) ·
[Closed audit PR](https://github.com/tailrocks/velnor-new/pull/1320) ·
[Outstanding audit issue](https://github.com/tailrocks/velnor-new/issues/1073)

### Sequence and exit criteria

- Immediately request the owner freeze covered merges and tool tags. Phase 0 depends on a full head and group canary using the
  released graph and real tests, then a green repaired main. Try the repair in the lab first, then on the exact repair nominee
  through the protected route. If the current producer cannot prove that nominee, keep the freeze until qualified recovery
  authority can. Reject any bootstrap that depends on permitting an unverified merge.
- Once canaries prove the intended producer, retire the admin bypass, bind `Required` to the observed Actions integration for the
  interim, restore ALLGREEN, and enable protected enqueue. Keep ruleset deletion, linear-history, non-fast-forward, pull-request
  and thread-resolution protections. Check the aggregate rule on the branch and all applicable policies. Confirm no unmanaged
  alternative producer can satisfy the protected status.
- The owner can apply the protective settings during the freeze. Resume ordinary merges only after exact status/read-back, green
  main and an end-to-end queue landing. Validate that the descriptor's workload and checking scope stayed the same through the
  landing. Any required recovery authority still checks the exact candidate under current policy.
- Parallelize diagnosis, lab rehearsals and the owner packet. Put one writer on the production generator config, the generated
  outputs and helper pin. Schedule the release gate before unfreezing tags. The App and reusable evidence work can use lab-only
  identities and shadow observations. Authority has one active version. A controlled handoff drains the queue, activates the new
  producer and deletes the predecessor's PR/group route.
- Size the first ten as 48–83 implementation agent-hours and 18–22 review agent-hours. Split estimates above eight implementation
  hours into independently verified slices. The readiness date depends on owner actions, green candidate and tool release
  turnaround, and queue evidence. At 2.4 qualified groups per hour, 100 groups alone would take about 42 hours. The live queue
  count of zero makes that throughput a scenario. (`regen/C-measurements.md:37–46`,
  `docs/research/velnor-runner-2026-11/delivery-plan.md:651–666,782–790`.)

### 1. Publish the owner decision and evidence packet

- Goal. Bind Phase 0 to current measurements and owner decisions, with a single merge/release freeze and a documented recovery
  path.
- Inputs and files. `docs/research/velnor-runner-2026-11/owner-questions.md`, `docs/goals.md:3,16,41,75–78`, `docs/roadmap.md`, and
  #1555. Capture expected ruleset parameters and current metadata as evidence for the change.
- Done when. Decisions in §7 are recorded with defaults or explicit owner gates. The packet names current source identity, baseline
  window and exact acceptance criteria.
- Size and roles. 2 implementation agent-hours, `gpt-6.1-sol @high`. 1 review agent-hour, `gpt-6.1-sol @xhigh`.

### 2. Repair the pinned green path under #1555

- Goal. Establish a green current tip and full labelled/queue canary with no waiver of an intended check.
- Files. `.github/workflows/{rust-ci,ci}.yml`, `.velnor/{rust-ci.lock,rust-ci.toml,rust-metadata.json,rust-inputs.lock}`,
  `actions/setup-velnor/action.yml`, and the specific lint/test sources named by the failing pinned jobs. Use the exact run
  metadata above and the #1555 evidence. The advisory exception already exists in `deny.toml:15`.
- Done when. Reproduce real failures with the intended release helper/pin, fix their first wrong state, and pass the pinned
  workflow and relevant nextest suites on an exact candidate. Make a head and group canary pass and a planted fault fail before
  normal merges restart. A green run with empty or reduced scope earns no acceptance.
- Size and roles. 8–16 implementation agent-hours across scoped repairs, `gpt-6.1-sol @high`. 2–4 review agent-hours,
  `gpt-6.1-sol @high`, with an independent security review of launcher changes.

### 3. Make tools release require exact verified push evidence

- Goal. Finish #1608's live obligation for the sole current tool publisher.
- Files. `.github/workflows/release-rust-ci-tools.yml:86–105`, `crates/velnor-ci/src/{gate.rs,cli_release.rs}`, and
  `crates/velnor-gen/tests/it/release_workflow.rs`. Use live tests or current generator goldens. The old OpenTofu golden and
  manifest were deleted in #1747.
- Done when. Exact default-branch push and authorised workflow/attempt succeed for the tagged SHA, and a wrong source, tag, skipped
  gate, no run, pending or conflicting status blocks. Dry-run on a real pass and on an ancestor whose run failed. Match the current
  release pin and close #1608 when its behavior is represented.
- Size and roles. 5–8 implementation agent-hours, `gpt-6.1-sol @high`. 2 review agent-hours, `gpt-6.1-sol @xhigh`.

### 4. Replace quick-green with the strict label gate

- Goal. Require evidence for the latest PR head and run no CI workload on draft or unarmed heads. Use one label and keep the queue
  independent.
- Files. `.velnor/rust-ci.toml`, `crates/velnor-model/src/{repo_config.rs,plan.rs}`,
  `crates/velnor-rust/src/{config.rs,plan_budget.rs,required_job.rs}`, `crates/velnor-render/src/events.rs`, the separate lint
  triggers, `generator/CONFIG.md`, and event goldens.
- Done when. One event matrix covers label attach/remove, label on draft, new head, unrelated label, retarget, conversion back to
  draft, same-SHA reopen, cancellation and queue. A candidate that lacks full required evidence cannot merge. If an Actions-only
  gate cannot pass the lab's GitHub semantics checks, use the App producer before resuming normal merges. Delete the quick success
  and one-shot label job with the migration.
- Size and roles. 5–8 implementation agent-hours, `gpt-6.1-sol @high`. 2 review agent-hours, `gpt-6.1-sol @xhigh`.

### 5. Require generated-file freshness and an approved inventory

- Goal. Catch workflow/config drift and scope removal, including when the Rust selection is empty.
- Files. `crates/velnor-rust/src/{plan.rs,required_job.rs}`, `crates/velnor-model/src/steps.rs:108`,
  `crates/velnor-render/src/helper_steps.rs:74–80`, `crates/velnor-gen/src/{lib.rs,io/mod.rs,helper_gate.rs}`, plus lock, config,
  and goldens.
- Done when. An independently approved, pinned generator checks the exact candidate with only needed tools. Altered generated body,
  missing declared workflow, stale metadata, and a proposed obligation reduction all exercise their defined failure paths. A
  re-generated authorized change passes. Derive fast empty plans with proof.
- Size and roles. 4–7 implementation agent-hours, `gpt-6.1-sol @high`. 1–2 review agent-hours, `gpt-6.1-sol @xhigh`.

### 6. Enable the verified merge queue

- Goal. Put exact-head and exact-group success ahead of landing. Keep the activation procedure auditable and under owner control.
- Inputs and files. `.github/workflows/rust-ci.yml:5–24,571–612`, owner-managed `protect-main` and settings, and process text in
  `docs/roadmap.md` if the procedure changes.
- Done when. Owner read-back confirms ALLGREEN, no bypass, source binding, intended timeout/concurrency, and auto-merge where it
  enqueues. Draft, unrequested, wrong workflow, stale and group-failure canaries stay blocked. A complete green head can enqueue
  and land. Supersession and ejection create correctly bound new groups.
- Size and roles. 3–6 implementation agent-hours, `gpt-6.1-sol @high`, plus owner time. 2 review agent-hours, `gpt-6.1-sol @xhigh`.

### 7. Consolidate policy checks and the effective plan

- Goal. Make one check inventory mandatory for the selected scope, including after a full replan. Remove duplicate policy work and
  its unnecessary serial tail.
- Files. `.github/workflows/ci.yml:41–157`, `.velnor/rust-ci.toml`,
  `crates/velnor-rust/src/{plan.rs,lane.rs,policy.rs,lint_jobs.rs,required_job.rs}`, and `crates/velnor-render/src/gate.rs`.
- Done when. Each prior lint failure has a matching failing case in the new graph. Freshness and syntax lint run for their inputs.
  An expired target and changed policy/test expectations reach the same gate output consumed by execution. Measure and choose
  inline or parallel policy at equal runner budget. Delete original lint routes once all checks are verified in the generated path.
- Size and roles. 4–8 implementation agent-hours, `gpt-6.1-sol @medium`. 2 review agent-hours, `gpt-6.1-sol @high`.

### 8. Finish platform-specific lock resolution

- Goal. Bring the useful parts of #1200 onto the root workspace and retain exact platform identity for tools.
- Files. The current tool matching in `crates/velnor-rust/src/{mise_lock.rs,lint_tools.rs}`, helper preparation, root `mise.lock`,
  and the applicable generator/helper integration tests identified in #1200 and #1700.
- Done when. Fully pinned Linux x64/arm64 requests resolve only their supported lock rows, bad or ambiguous pins fail before
  install, and unchanged platforms keep the same keys. Record current Cargo Deny and Alint results instead of relying on D's
  earlier diagnosis.
- Size and roles. 3–6 implementation agent-hours, `gpt-6.1-sol @high`. 1–2 review agent-hours, `gpt-6.1-sol @high`.

### 9. Rework the audit and snapshot pieces on the current workspace

- Goal. Satisfy #1073 without resurrecting #1320's branch. Carry #1026's sibling-input and custom-target correctness with bounded
  changes.
- Files. `crates/velnor-ci/src/{inputs.rs,io/audit.rs,io/audit/setup.rs}`, current input save/restore I/O,
  `crates/velnor-rust/src/{audit_job.rs,groups.rs,root_jobs.rs}`, and corresponding render/golden paths.
- Done when. Warm audit checks the exact root/configuration it claims and compiles no workspace twice. A missing sibling input or
  unsafe target rejects or forces the correct cold path. Exercise root, nested, shared group, custom target, and stale manifest
  tests. Run the current `velnor-new` scenario after repairing its obsolete path if needed. Replace legacy implementations with the
  tested caller changes.
- Size and roles. 8–14 implementation agent-hours across two or three changes, `gpt-6.1-sol @high`. 3 review agent-hours,
  `gpt-6.1-sol @xhigh`.

### 10. Establish the new baseline and export the first contract

- Goal. Baseline the guarded system and derive a read-only obligation manifest from the same plan/pin that generates YAML.
- Files. `crates/velnor-{model,rust,gen}`, `crates/velnor-ci/src/io/{mod.rs,timings.rs}`, the current metrics schema and
  `generator/samples/`.
- Done when. Exact selected obligation IDs match executed plans in the lab and the guarded cohort, with machine-readable timings
  and no dropped tests. Compare shape, two check phases and the range fallback. Establish an empty-plan control, stratified full
  workload, cold control and the slowest surviving tests. Record who produced each observation. Select the next speed lever from
  measured critical-path data.
- Size and roles. 6–8 implementation agent-hours, `gpt-6.1-sol @medium`. 2 review agent-hours, `gpt-6.1-sol @high`.

## 7. Owner questions, rewritten

### Original D1–D26

| ID and disposition | Decision or revised question | Recommendation |
|---|---|---|
| D1, obsolete | Release the engine to move gate jobs. | Close. The generator cutover moved the gates and #1747 deleted the engine. |
| D2, reword | Enable automatic enqueue after qualified latest-head success and stop catch-up merging? | Yes, once guarded. Measure the actual queue transition. Keep the owner decision explicit because auto-merge is off. |
| D3, reword | When should the owner remove bypass, bind the interim context and switch the queue to ALLGREEN? | Freeze now. Apply with green repair and canary evidence before reopening. `Required verdict` is gone. Qualify the App transition separately. |
| D4, reword | Which evaluator and accepted subset fit the selected official runner pin? | Use option B if it passes differential parity against the pinned Worker, or hold the unsupported cases. Choose one implementation. |
| D5, absent | No question with this ID appears in the owner sheet. | Leave the slot empty. |
| D6, answered | Official for conformance. | Keep the reference role. Decisions 19–20 also allow a separately chosen, qualified production role. |
| D7, open | Approve the dedicated status App and the separate scoped agent identity? | Yes. Writer credentials stay outside jobs. Bound read-only state reconciliation, and keep details outside branch protection. |
| D8, reword | Approve a manifest home and namespace deploy identity? Which node changes are justified? | Use the existing operator-owned deployment repo provisionally. Approve Git-reviewed, scoped deploys. Gate disk changes and RustFS on node-loss and performance results. |
| D9, obsolete | Pre-approve engine-only releases. | Close. Ask about tool-release automation only if eligibility, smoke, lab adoption and tags are enforced. |
| D10, answered | ALLGREEN and two witnesses. | Implement both. Measure witness correlation and gate cost. Keep preparation speed from changing independence. |
| D11, reword | Accept per-capability admission and counts for the new cohorts? | Start with seeded semantic and refusal cases, roughly 100–150 pairs for gate/policy and Docker, 300 for the Rust graph, and 30 release pairs plus 3 dry runs if relevant. Hold on any unresolved difference. |
| D12, answered with an implementation condition | Use the group/landing path once to build release bytes. | Keep dev-profile tests. Bind optimized release bytes to the actual landed SHA. Build at the landed SHA if the group and landed SHAs differ while the binary embeds commit identity. |
| D13, reopen | Narrow runner-host maintenance until component review and replacement? | Continue correctness and ownership fixes for active behavior. Freeze speculative feature work. Reuse the verified invariants, and decide the journal/wrapper boundary through RV01/RV05. |
| D14, done | Delete the engine. | Close #1747's portion, and move residual helper-selection migration to N14. |
| D15, reword | How does a broken release recover with protection enabled? | Redeploy a known released digest under the same authority. Prequalify a cold two-node recovery procedure with the current inventory and a fenced writer. Keep the head/group blocked until the requisite proof exists. |
| D16, reword | Does an already labelled draft arm at ready? | No under the binding current flow. A ready-label attach arms. Include both possibilities in lab tests and change only by explicit decision. |
| D17, open | Who can approve a required-scope reduction? | Owner-signed, exact diff and policy epochs, with expiry for exceptions. A legitimate test rename requires a reviewed stable-ID migration. |
| D18, reword | When would a direct Velnor merge train justify changing decision 7? | Keep GitHub's queue. Record 50–200 guarded merges and shadow only. Bring back any benefit and the semantics differences. |
| D19, answered by current rule | Start work before the label? | Keep explicit requests. Allow content prefetch with no execution or evidence, if measured and bounded. |
| D20, open | Rent high-clock capacity? | Measure the rebuilt floor first. Rent one comparator node after a CPU-bound chain is observed. Add two witness-capable nodes only for a qualified gain and fingerprint policy. |
| D21, open | Let one compile feed both witnesses? | Keep independent compile evidence. A single artifact can serve independent shards of one witness after proof. |
| D22, open | Pay for external comparison arms? | Use the strong warm Official baseline first and one paid comparator when the contract is stable. Name measured arms and matched budget in any speed claim. |
| D23, open | Automatically verify unpublished commits? | Defer. Ship explicit verify/ship, then decide with measured reuse, supersession and available capacity. |
| D24, open | Give `interactive` priority over PRs? | Start equal to PRs. Raise only within a measured per-identity CPU budget with merge, gate and audit reservations insulated. |
| D25, open | Permit sticky agent workspaces on cluster nodes? | Pilot two identities after the sealed request path proves isolation. Sticky workspace results are advisory. |
| D26, reword | Add arm64 executor and Rust test coverage beyond today's release builds? | The current release already builds and smokes natively on hosted arm64. Add an advisory native executor/test lane when demand merits it. Require explicit arm64 conformance before counting arm64 receipts. |

Sources are `docs/research/velnor-runner-2026-11/owner-questions.md:15–319,348–402`, owner decisions 7–24,
`regen/E-plan-audit.md:485–504`, `57b65af0db`, and `.github/workflows/release-rust-ci-tools.yml:115–169`.

### New decisions from E and this review

- **Q-N1. Amend the rule book for Native, the platform backends, and a deliberate Official choice?** Yes. Approve the text below
  and a scoped update to the active macOS spec. Keep the generator's hosted portability and local Official conformance. Approve
  changes to goal lines 43, 59 or 75 only as needed for specific trust/cache/local contracts. (`docs/goals.md:3,16,43,59,75`,
  `docs/proposed/macos-scaleset-runner.md:37–39,318–323`.)
- **Q-N2. Which label and what happens before it?** Use `velnor:verify`, retire dogfood one-shot and auto labels, and honor D16's
  literal default. Keep the workload idle. The owner may accept a short interim blocking Actions check where GitHub needs it. Use
  an explicit App/user label action and prove reconciliation. (`.github/workflows/rust-ci.yml:5–24,571–625`, owner decision 12.)
- **Q-N3. Retire the admin bypass and require a specific source?** Yes. Prepare during the freeze and resume only on proof. Bind
  the interim `Required` to Actions after verifying its identity and limitations, then bind `Velnor / Verified` to the dedicated
  App after parity. Owner applies and reads back, including inherited rules. (`regen/C-measurements.md:37–46,159–174`.)
- **Q-N4. Does the queue remain in the flow?** Yes. Keep ALLGREEN, SQUASH, depth 2 and measured auto-enqueue. Set the wait field to
  zero if lab behavior and the minimum-entry policy warrant it. Revisit timeout/depth only after measured group verdicts and flake
  data. (`regen/C-measurements.md:159–174`, owner decisions 7 and 12.)
- **Q-N5. Which provenance is acceptable for release?** Keep hosted release production for now. Keep native arm64, hold macOS
  release builds, and fix the current gate. Approve a self-hosted producer only after verifying attested builder, source, bytes,
  and key policy. Add `runner-v` tag authorization and workflow together when Linux runner delivery is in scope.
  (`crates/velnor-render/src/release_attest.rs:33–73`, `.github/workflows/release-rust-ci-tools.yml:16–85,115–169`,
  `release-plz.toml:45–106`.)
- **Q-N6. How should public fork events be rejected?** The product scope is same-repo. Approve admission that keeps outside
  contributions idle and prevents reaching self-hosted resources. Remove the dogfood fork execution route as the new producer takes
  over. Preserve the generic generator's protective defaults until an owner-approved consumer migration.
  (`generator/CONFIG.md:5–16`, `docs/goals.md:43`, owner decision 8.)
- **Q-N7. Change the job/target topology?** Keep the one-job default and HD11. Require paired cold/warm evidence and unchanged
  required checks for any new shape. Use isolated writable targets for concurrency and share only verified immutable outputs.
  (`crates/velnor-rust/src/shape.rs:18–59`, `docs/goals.md:68–70`, `docs/hard-decisions.md:486–513`.)
- **Q-N8. Enable an exact-result ledger under goals rule 2?** Approve a narrow contract with versioned immutable inputs, witnessed
  evidence, shadow re-execution, a revocable epoch and a measured cold control. Leave historical baseline skips retired. Require
  approval separately for narrower input contracts. (`docs/hard-decisions.md:14–19,223–229`, `docs/goals.md:40–42`, owner decision
  11.)
- **Q-N9. Maintain or freeze runner-host until F01?** Maintain correctness where the code is used for qualification or an
  Official/Docker dependency. Approve a component retirement plan after RV01/RV05 names the surviving guarantees. The recent
  hardening has landed. (`a619478fb2`, `94da9295e0`, `.velnor/rust-ci.toml:26–32`.)
- **Q-N10. Where does dated evidence live?** Keep reproducible snapshots with their revision and decision. Keep current manuals
  evergreen. Move implementation status and PR-specific notes to issues or records keyed by revision. (`AGENTS.md:40–44`.)
- **Q-N11. Extend the backlog executor catalog?** Keep the present issue automation until the owner approves schema changes. Store
  model and effort explicitly in the new work packets. Map to supported executor labels only where the actual dispatch system can
  run them. (`docs/roadmap.md:35–50`, `regen/E-plan-audit.md:193–207`.)
- **Q-N12. Which docs, crate-alone, four lint and release-check outcomes must block landing?** Approve the precise obligation
  table. Recommend requiring applicable docs, crate-alone and lints before landing unless a narrower rule is explicitly adopted.
  Schedule cold audits and cache certification after landing. Measure added cost. Existing placement makes this a real scope
  decision. (`.velnor/rust-ci.toml:5–9`, `.github/workflows/rust-ci.yml:211–267,571–609`, `.github/workflows/ci.yml:41–157`.)
- **Q-N13. Who may activate an emergency stop when stored GitHub successes outlive revocation?** Grant a scoped, audited authority
  to hold queue/landing and tag publication, with read-back and a named resumption condition. Exercise a lost response and a writer
  outage in the lab. A new writer can resume after stale writes are resolved.
  (`docs/proposed/velnor-runner-architecture.md:603–624`.)

E's question list is `regen/E-plan-audit.md:506–520`. These owner approvals are dependencies of implementation.

### Exact proposed replacement for docs/goals.md line 16

Replace the whole paragraph at `docs/goals.md:16` with the following text, subject to the owner's explicit approval.

> Velnor serves Rust projects on GitHub Actions. On self-hosted runners, Velnor Native is the default executor and runs supported
> jobs in Rust. A project may explicitly select GitHub Official for a qualified job type. Velnor also runs GitHub's official runner
> in a separate reference role for conformance. Only production-role evidence can satisfy verification or release policy. Every job
> keeps its configured executor, and the verification key identifies that executor. Generated workflows may use GitHub-hosted
> runners under the project's runner and release policy.

For the same amendment, update the code ownership paths in lines 11–12, specify Docker and Kubernetes support in the vision and
active runner spec, and align rule 11 with the local/reference roles. Ask for a separate explicit approval to replace line 41 when
admitting ledger evidence, and to update rules 12 and 13 for the App and promotion. Proposed wording for the invariant in line 41
is below.

> **Green means verified.** The required status passes only when every applicable obligation has current passing evidence for its
> verified inputs and executor. An obligation that requires tests must account for every expected test. A missing, cancelled,
> invalid, or revoked result blocks the verdict.

## 8. What a generator-owned contract makes possible

### Compile the policy and prove that its projections agree

- Emit a canonical graph ahead of time with obligation IDs, allowed configuration transitions, semantic dependencies, capabilities,
  tool activation, and required receipt shape. A production obligation runner interprets this smaller graph. Full Actions execution
  remains for accepted Actions job types.
- Lower the graph to Actions and direct execution. Test exact parity by comparing semantic IDs, command/environment identities,
  selected binary/test set, and join rule on the same subject. Give each projection a deliberately independent reader for
  conformance. Generating equal-looking YAML cannot prove equal results. (`crates/velnor-gen/src/lib.rs:76–103`,
  `crates/velnor-rust/src/plan.rs:50–118`, `crates/velnor-render/src/lib.rs:74–122`.)
- Make the graph difference part of every check. A candidate can propose new config and generated output. Verify both under a
  trusted generator. Show added, changed and removed obligations, the authorizing policy and every test stable-ID change. Use a
  signed reduction or explicitly verified migration. Activate accepted changes only after landing and qualification. This handles
  helper/config migrations whose old compiler cannot parse the new schema.
- Give the release commit a machine-readable binding between the approved old requirements, qualified new schema and producer, and
  exact changes being approved. Run any prospective semantics in the lab, without giving them status authority. Replace the old
  deployment and generated paths together.
- Treat a genuine `NotApplicable` as a proof obligation too. Build an end-to-end test where a new fixture, config relocation, or
  changed test filter must flip the disposition. Give the gate explicit reasons and evidence rather than reading the absence of a
  job as a pass. (`docs/goals.md:40–41,66,71`, `crates/velnor-rust/src/verdict.rs:207–316`.)

### Resolve keys early, then spend resources only on missing work

- Write key recipes at generation time. Evaluate them against an exact trusted subject and frozen digests at plan time.
  Canonicalize a *semantic* execution graph so moving policy earlier or placing two roots in one job leaves eligible keys intact,
  while changes to a meaningful command, input, tool, resource limit or order invalidate them.
- Split provenance by stage. An immutable build record binds its output digests. A test record refers to the binaries it actually
  executed, its enforced runtime inputs and its accepted inventory. Changing a runtime fixture with an unchanged binary requires
  new test evidence. Changing something outside an enrolled verified contract can reuse a genuinely unaffected test.
- Start wide and require explicit enforcement to narrow. A trace of one directory listing does not prove a future negative lookup
  or glob. Future reduced keys need a virtualized input view or enforced path/namespace policy whose violations block. Keep the
  complete subject available for conformance and cold audit. Measure documentation, lint and test dependencies independently.
  (`docs/proposed/velnor-runner-architecture.md:66–89,499–507,568–596`.)
- Put key resolution before queue and pod acquisition. For an exact hit, authorize via the ledger and publish after a fresh
  subject/policy check. For a miss, request the required witness slots together, bias placement towards verified tools and warm
  generations, and retire superseded requests without killing shared work. Compute estimated avoidable critical-path seconds from
  the graph and actual hit distribution.
- Use the same plans in an agent's request, the group and the post-landing audit. An agent can see which obligation waits on a
  particular digest or external input and request precisely its missing fresh work. This gives pre-push and group reuse one
  meaning.

### Keep authority across external writes and failures

- Ledger leases and a database compare-and-swap fence internal writes. GitHub commit-status publication is an external operation. A
  stale request can complete after an expired lease or revocation. Put the credential behind one publishing authority, record the
  request ID before each write, and require read-back after an ambiguous response. Separate epoch transition from normal evidence
  publication.
- Commit status is a repository/SHA/context claim. Define one semantic claim under the protected target when different PRs share a
  SHA. Reconcile live label/head/group state before first publication. An approved replan on a changed base gets new evidence as
  needed. A base/dependency change still has to pass the exact group check.
- A prior `success` can remain visible during an outage. Define a real owner-approved stop barrier for landings and publication,
  exercise it with a stale writer and a destroyed group, and document the exposure window honestly. A drained queue and read-back
  of all affected open heads are prerequisites of emergency resumption. Preserve the immutable audit history of already landed
  commits. (`docs/proposed/velnor-runner-architecture.md:603–624,838–867`.)
- Keep blob storage availability independent of verdict authority. A corrupt cache goes cold. A missing receipt or uncertain policy
  blocks approval until verifiable work completes. A broken release artifact blocks publication. That separation lets each service
  degrade appropriately at a site partition.

### Soundness checks for cheap paths

| Path | Complete proof | False result to guard against | Detection | Outcome when proof breaks |
|---|---|---|---|---|
| Affected selection | A complete event range plus trusted inputs, graph and applicability policy. | A missing base or source read omits a selected obligation. | Full-range differential tests, fixture and configuration mutations, nightly full suite. | Resolve missing history, then select all on doubt. |
| Whole-tree receipt | Full tree, exact definition, frozen external inputs and fingerprint, active two-witness proof. | Same tree with changed tool, environment or fresh input passes under a previous key. | Receipt validation, a changed-field canary, independent cold replay and landed audit. | New key or fresh work. Revoke the epoch on a contradictory result. |
| Narrow or binary-key receipt | Enforced runtime input contract, including absence and directory contents, and proven executed binary and child digests. | An unobserved read or unchanged binary hides a changed data input. | Reject undeclared reads, run a sibling-file or runtime-fixture mutation, and independently re-execute some hits. | Suspend enrollment. Run with the full-tree key or fresh. |
| In-flight or agent reuse | An exact resolved key and qualified observation. A join never copies a cancelled result or fills a witness twice. | An advisory local pass, torn snapshot, or stale session counts. | Node tree re-hash, issuer and epoch checks, and parity under changed bases and concurrent cancellation. | Remove stale claims. Execute missing witnesses. |
| Warm target, PR delta or MBX | Approved source scope, a compatible manifest, sealed generation or private state, checked output identity for evidence. | Cargo reports fresh despite changed sources or a stale build script result. | Digest comparison with cold builds, older-mtime and new-input canaries, cache scope tests. | Invalidate the artifact or generation. Rebuild cold. |
| Cache, tool or mirror fetch | Authoritative scope/index selection and actual bytes matching the immutable manifest or lock. | A stale prefix or remote copy overrides the intended entry or tree. | Oracle cache probes, manifest and full checkout verification, scope and corruption tests. | Cache miss or verified refetch. Refuse unresolved locked sources. |
| Release digest promotion | The current gate and producer receipt bind the exact tagged commit, platform, profile, artifact ID and bytes. | A valid but unrelated artifact, or a later changed manifest, is promoted. | Verify signer, source, target and digests on the bytes about to publish. | Hold the release until the approved bytes are available. |

Sources are `docs/proposed/velnor-runner-architecture.md:568–596`, `docs/rust-ci-cache-keys.md:38–71`, and
`docs/goals.md:40–45,76–78`.

### Generate conformance and benchmarks from the same inventory

- Derive the Native admission profile from the generated graph, pinned action manifest and each call's actual input set. Reject
  unsupported YAML semantics at admission. Keep explicit outcomes for unsupported Node, Docker, service, reusable workflow and
  event features. Compare against GitHub Official on each backend for every admitted shape, including missing, failed, and
  cancelled outputs. (`crates/velnor-render/src/rules.rs:16–34`, `generator/CONFIG.md:498–531`.)
- Generate the benchmark manifest from `RepoFacts`, the exact plan and frozen arrivals. Include warm unchanged, changed package,
  tool or lock, shared workspaces and Docker. Each case carries the expected test IDs, negative mutations, cold control and
  capacity budget. Use the existing `bench` and `timings` formats for clocks. Publish raw measurements keyed by the plan, helper
  and test corpus digests. (`crates/velnor-ci/src/io/{mod.rs:307–388,timings.rs:16–59}`, `docs/goals.md:78,95`.)
- Make an obligation census the first experiment. Measure the fraction requiring a fresh request, full-tree hits across exact queue
  bases, compiler dependency reuse, correlated failures, and any grade of acceptable narrowed key. Use the observed values to
  choose whether to invest first in test grouping, cache transport, the scheduler, or the full Native Actions executor.
- A 3,000-case shadow run with zero disagreement gives an upper bound near 0.1% at 95% under appropriate sampling assumptions.
  Correlated repetitions lower the effective sample. Start the post-enrollment sample at 2%, then measure how quickly
  that rate detects drift under real traffic. Keep independent cold canaries and reject a reuse class on its first
  unexplained difference. Estimate false-red budgets separately from the zero-tolerance policy for false greens.
  (`docs/proposed/velnor-runner-architecture.md:90–118,1089–1106`.)
- Retire per-event duplicate implementations. The durable performance gain comes from one approved graph that can explain, execute,
  prove, and measure the same obligation regardless of arrival path. Runner compatibility and measured executor latency then become
  specific claims for the supported job set.

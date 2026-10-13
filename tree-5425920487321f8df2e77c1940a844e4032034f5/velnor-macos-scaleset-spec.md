# Velnor macOS Scale Set runner — implementation specification

**Status:** Proposed implementation contract, not a claim of implemented or deployed support.  
**Research date:** 3 October 2026.  
**Implementation repository:** `tailrocks/velnor-new`.  
**First consumer:** `ChainArgos/java-monorepo`.  
**Initial host:** macOS, using an explicitly selected, existing local Docker engine.  
**Runtime:** The unmodified official GitHub Actions runner, in an Ubuntu 26.04 Linux container, registered through the GitHub Runner Scale Set service using JIT.

The companion `velnor-macos-scaleset-goal.md` is self-contained: it includes the execution instructions and this entire specification. Configuration, commands, types, paths marked **proposed** below must be implemented; they are not advertised as existing CLI functionality.

## 1. Outcome and non-negotiable boundaries

Deliver a native Rust `velnor-host` executable that connects this Mac to a repository, manages a long-running user service, consumes Scale Set demand, and creates disposable official runner environments. Extend `velnor-actions` to generate hosted-only, scale-set-only, and paired execution from one logical plan. Roll out the qualified product to ChainArgos, preserving every current required check and making the execution location visible.

The canonical initial selectors are:

```yaml
# GitHub-hosted baseline
runs-on: ubuntu-26.04

# Velnor-managed official runner; register both labels on the Scale Set
runs-on: [velnor, ubuntu-26.04-scale-set]
```

The Scale Set name is `ubuntu-26.04-scale-set`. `velnor` is the explicit product marker. The selector is routing, not authorization. Do not register the hosted selector `ubuntu-26.04` on local workers. Do not replace this with an ordinary persistent self-hosted runner or assume that `--ephemeral` plus polling GitHub REST workflow jobs implements the Scale Set protocol. Current GitHub documentation describes multiple labels and non-Kubernetes custom scale-set clients. Verify the exact selector through a real GitHub run before promotion. [S18]

### 1.1 What “1:1” means here

Define and verify three separate claims:

| Claim | Required evidence |
|---|---|
| Official Actions execution semantics | The genuine official runner handles actions, steps, file commands, logs, job credentials, post steps, services, cancellation, and completion. Velnor does not implement another Actions interpreter. |
| Declared environment equivalence | Same Ubuntu release family, execution architecture, locked project toolchain, command/configuration/feature inputs, and explicitly supported capabilities. Record actual versions, not just desired configuration. |
| Workload parity | Every expected verification task really executes in both locations at the same source and plan identity, and both complete successfully with equivalent semantic results. |

This does **not** mean that a Docker container is identical to GitHub's hosted VM. Kernel, hardware, CPU instructions, system services, nested virtualization, network identity, filesystem topology, privilege boundary, and `runner.environment` can differ. A custom Ubuntu image is not automatically the hosted runner's software inventory. Standard `ubuntu-26.04` is currently documented as an x64 public-preview hosted image; `ubuntu-26.04-arm` is a separate architecture. Record preview status and actual hosted image version in qualification. [S19]

The primary profile must be **Linux/amd64**, matching the consumer's existing hosted label and x86_64 bootstrap. On Apple Silicon this may require emulation. Detect and report host architecture, Docker VM architecture, container platform, and emulation separately. Never silently substitute ARM64. A later Linux/arm64 profile requires a distinct selector, such as `ubuntu-26.04-arm-scale-set`, and a matching ARM64 hosted baseline. [S10], [S19], [S20]

### 1.2 In scope / out of scope

In scope: native macOS service; one repository-scoped initial connection; official Scale Set/JIT lifecycle; configurable fixed maximum `N` concurrent job lifecycles; Docker actions, job containers, service containers, Compose/Buildx/Testcontainers qualification; immutable images; durable recovery; generated routing; paired verification; release and ChainArgos rollout.

Out of scope: a native replacement for the official runner; a second workflow/task interpreter; Kubernetes/ARC deployment; Velnor-managed VMs, Firecracker, libvirt, or fleet provisioning; Linux host implementation; native Darwin/Xcode jobs; dashboard/TUI; a Go controller sidecar; a new distributed cache service. The existing Docker provider may use a Linux VM internally; “no Velnor-managed VM” must not be misrepresented as “no VM exists on macOS.” [S20]

Do not port the old monolithic product or its native runner. Use its relevant code, specifications, failures, and tests as reference material.

## 2. Audited starting point

### 2.1 Immutable research anchors

| Repository / reference | Observed revision |
|---|---|
| `tailrocks/velnor-new` main / `v0.1.0` tag | `c57c700459bbe1549fe7eedcb7d8689585c38986` |
| `tailrocks/velnor` main | `3f6633252963efef0d71244aadae36516a11601e` |
| Legacy PR #1133 head | `27049c4bfca42d9d5a1e8cbbe584a2658ee5a77d` |
| Legacy PR #1135 head | `3074bb36c2fe4f9ca0b34deb19a67acc3eb5a9c8` |
| `ChainArgos/java-monorepo` main | `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` |
| `actions/scaleset` source | `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5` |
| `actions/runner` source inspected | `d7bc179baf11a02110b46cfbbc4040f74ac3f60a` |

These are research anchors, not perpetual deployment pins. At execution, re-read current heads, enumerate open PRs again, and record deltas before choosing implementation and release pins. The official runner source revision inspected is not asserted to be the version of any particular released binary.

### 2.2 What `velnor-new` already provides

The root is an eight-package, virtual Rust workspace using edition 2024, explicit members, exact dependency pins, and strict lint/structure policy. The packages are `velnor-actions-contract`, `velnor-actions-rust`, `velnor-actions-tofu`, `velnor-actions-mise`, `velnor-actions-actionlint`, `velnor-actions-workflow-renderer`, `velnor-actions-orchestrator`, and `velnor-actions-cli`. There are no runner packages in that root workspace. [S01], [S02]

The important existing boundaries are sound and must remain: pure stack-neutral contracts; Rust/Cargo analysis in the Rust adapter; OpenTofu analysis in its own adapter; tool invocation construction in Mise; generic workflow serialization in the renderer; composition in the orchestrator; a thin Clap CLI. The existing executable is `velnor-actions`, with `init`, `plan`, and `generate`. [S02], [S03]

The actual routing extension points are precise:

| Existing source | Observed limitation / required change |
|---|---|
| `crates/velnor-actions-contract/src/config/mod.rs` | Strict schema 1 configuration, registered Rust and OpenTofu stacks. Add a deliberate routing schema revision, not an unvalidated string escape hatch. |
| `.../config/workflow.rs` | `WorkflowConfig.runner_label` selects only compiled hosted labels. The current catalog default is `ubuntu-26.04`; custom scale-set labels do not fit this model. |
| `.../workflow/ir.rs` | `Job.runs_on` is a `String` documented as a literal Ubuntu label. Replace with validated typed routing. |
| `.../workflow/ir.rs` | Dispatch inputs are currently string-only; typed hosted/scale-set/both choices require a real IR and renderer extension. |
| `.../workflow/*` | Existing plan, task identity, report, artifact, cache, trust, and aggregation contracts are the integration point. Do not invent an independent runner task graph. |
| Renderer / orchestrator | Expand one logical graph into eligible execution lanes, then render generic IR. Do not duplicate stack detection per provider. |
| Actionlint adapter | Generate the custom label configuration and validate all emitted workflow shapes. |

The release state must be read from evidence, not old introductory prose. `v0.1.0` is published with Linux x64 and macOS ARM64 assets; its tag resolves to `c57c700...`. ChainArgos's generated workflow downloads the Linux asset and verifies SHA-256 `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5`. The release API reported `immutable: false`. The release object's `target_commitish` is not a substitute for resolving the tag and verifying the build manifest/attestation. Do not infer a provenance defect from that field alone, and do not consider a matching checksum alone to prove trusted build provenance. [S09], [S10], [S11]

### 2.3 Documentation that must be reconciled

The current architecture and deferred runner specification explicitly separate generator and runner workspaces. The deferred runner plan also says one slot, Linux/ARM64 first, classic self-hosted label arrays, a restricted `runner-rust-ci-v1` sidecar, and **no Scale Sets, services, nested Docker, or arbitrary marketplace actions** in its initial scope. Those restrictions conflict with this new request. [S02], [S04]

Create `docs/proposed/macos-scaleset-runner.md` as the canonical active specification and a small decision record linked from `docs/README.md`, `docs/proposed/README.md`, and `docs/deferred/README.md`. Mark the superseded sections of `docs/deferred/self-hosted-runner.md`; retain its still-applicable lifecycle, credential, storage, and separation requirements. Do not leave two incompatible normative plans active.

Explicitly supersede: the ban on Scale Sets/services/nested Docker; mandatory one-slot-only product scope; implicit ARM64 execution; classic self-hosted routing; and the restricted native-interpreter profile as a prerequisite for official mode. Preserve: separate workspace, no generator-adapter dependency in the runtime, one task/cache authority, exact runner lock, durable recovery, truthful conclusions, and no host home/socket exposure.

Read the complete documentation inventory before implementation, including architecture, bootstrap/release, generated-file ownership, CLI/workflow/task/cache/parallelism/tooling/quality/version contracts, implementation plan, OpenTofu, Rust dependency/test policy, release coverage, implemented evidence, references, and PR-1 adoption/disposition records. A “proposed” file, historical green check, or reported test count is not current live qualification. [S03], [S05], [S06]

This research inspected the principal architecture and runner documents, current manifests/configuration/IR, official protocol/runtime paths, both open legacy PRs and their relevant diffs, and consumer bootstrap. It is not a claim that every line of every historical document or the 69-file legacy PR received an exhaustive security audit. The implementation evidence ledger must close that audit coverage explicitly.

### 2.4 Legacy implementation and open PR disposition

The old implementation has substantial Scale Set infrastructure under `crates/velnor-runner/src/scaleset/`: authentication, registration, sessions, demand, acquisition, allocator, shared ledger, durable intents, listener, convergence, recovery, daemon integration, and official worker/DinD code. Its three-provider plan supplies useful invariants, but its Debian/bastion rollout and native runtime are not the scope of this project. [S12], [S13]

At the research snapshot there were **two open legacy PRs**, both drafts. Their returned discussion timelines were empty. Recheck review state and new comments at execution; neither a draft nor a PR's reported test result is production proof.

| Reference | Reuse as requirements / tests | Do not import wholesale |
|---|---|---|
| #1133, 69 files | Go-compatible null/omission handling; strict lane result census; immutable Docker identity and cleanup; scale-set-only service lifecycle; `disableUpdate=true` registration invariant; release artifact IDs; safe promotion rollback; cache verification without checkout mutation | Old native runtime, duplicated schema-1/schema-2 workflow frameworks, Debian packaging, broad CLI/TUI, unrelated legacy scanner structure |
| #1135, 5 files | Stage strict artifacts; consume the producer's immutable artifact ID; independent fresh verifier; reject missing/empty/symlinked paths and case/ancestor collisions; make consumers depend on verification | Old workflow framework or a name-only artifact handoff |

In #1133, inspect `velnor-model/src/scheduler.rs`, `velnor-control/src/journal.rs`, `velnor-runner/src/docker_lease.rs`, `velnor-runner/src/scaleset/registration.rs`, `velnor-runner/src/runner.rs`, `velnor-tools/src/{lane_compare,github_live_collector,evidence_check}.rs`, and the product/release transport changes. The registration fix validates identities after create/adopt/races and requires the server to retain disabled runner self-updates. This must be carried into the new design. [S14], [S15]

In #1135, inspect both renderer variants and configuration tests. Port the invariant into the new typed artifact/report system once, not the old duplicate implementations. Preserve attribution and licenses for copied code; maintain a table of copied/adapted/rejected reference material with reasons and regression tests. [S16]

### 2.5 ChainArgos starting point

At the consumer snapshot, `.velnor/config.toml` is schema 1, uses default branch `main`, and selects Rust `mbx` plus `cargo_nextest`. The `.github` tree contains one generated workflow, `workflows/ci.yml`, plus its supporting agent/actionlint files; the workflow blob is 358,635 bytes. The inspected beginning already runs hosted `ubuntu-26.04`, uses pinned tools and the released Velnor binary, and generates/diffs `.github` for drift checking. Do not assume a small workflow or a single workspace. [S07], [S08], [S10]

The repository explicitly contains live Java/Kotlin backend services alongside Rust migration work and frontends. Inventory all logical tasks and existing required contexts before claiming coverage. A Rust-only migration must not be described as complete Java/frontend/platform verification. Do not invent a Java adapter merely because the repository is named `java-monorepo`; add a typed adapter or qualification task capability only where the actual coverage inventory requires it. Existing production code is not to be deployed as part of this runner rollout. [S08]

## 3. Canonical Rust package architecture

### 3.1 Four new packages, one new operator binary

Preserve the root generator workspace and introduce an independent virtual runner workspace inside the same repository:

```text
Cargo.toml                         # Existing generator workspace; excludes crates/velnor-runner
Cargo.lock
crates/velnor-actions-*/            # Existing eight generator packages
crates/velnor-runner/
  Cargo.toml                       # Independent virtual workspace
  Cargo.lock
  crates/
    velnor-runner-core/
    velnor-runner-github/
    velnor-runner-host/
    velnor-runner-cli/              # Exactly one binary: velnor-host
  fixtures/
    protocol/
    qualification/
.velnor/runner.lock                 # Reviewed release/platform/image/protocol dependency lock
images/runner/ubuntu-26.04/         # Immutable runner image recipe and entrypoint
images/dind/                       # Reviewed private-Docker recipe/configuration
```

Use four packages because they isolate pure state, external protocol, host effects, and CLI, rather than introducing a generic provider framework. This extends the deferred architecture instead of attaching asynchronous host/Docker dependencies to `velnor-actions` or reintroducing the old `velnorctl` monolith. [S04]

| Package | Owns | Must not own |
|---|---|---|
| `velnor-runner-core` | Validated IDs, capacity and demand model, lifecycle transitions, ownership records, evidence identities, redacted error classifications | HTTP, Docker, database effects, subprocesses, workflow interpretation |
| `velnor-runner-github` | Rust Scale Set wire/client implementation, App/token refresh, registration/session/acquisition/JIT, authoritative run/job reads | Container lifecycle, repository command execution, task planning |
| `velnor-runner-host` | Controller composition, local journal, Docker client and ownership, worker supervision, Keychain, launchd, local IPC, logs, diagnostics and recovery | Parsing/reimplementing GitHub steps; scanning consumers to build another task plan |
| `velnor-runner-cli` | Clap parsing, typed requests, presentation, documented exit codes, daemon entrypoint | Scheduling/protocol/business logic |

Dependency direction: CLI → host → GitHub/core; GitHub → core. The host may use existing pure `velnor-actions-contract` report/identity types at the evidence boundary. No runner package depends on the Rust, Tofu, Mise, renderer, or generator orchestrator packages. No generator package depends on runner packages. Do not move GitHub job interpretation into “core” under a different name.

Mirror the established edition, dependency pinning, unsafe prohibition, strict Clippy/rustdoc, line limits, test placement, and licensing policy. A root `cargo ... --workspace` does not cover a nested workspace: generation, dependency checks, source-closure hashes, freshness, formatting, tests, and release manifests must enumerate both. Verify this with a failing fixture in each workspace. Do not weaken root rules to accommodate the runner. [S01], [S03]

### 3.2 Recommended module seams

Use cohesive modules, not one file per trivial type. Split according to the repository's existing size limits:

```text
core/src/{identity,capacity,demand,lifecycle,ownership,evidence,error}.rs

github/src/auth/{app,token,redaction}.rs
github/src/wire/{registration,session,messages,jit}.rs
github/src/{client,registration,session,acquire,listener,run_census,retry}.rs

host/src/{controller,reconcile,drain,config,diagnostics}.rs
host/src/journal/{schema,migrations,capacity,intents,workers,messages}.rs
host/src/docker/{context,client,identity,volumes,network,events,cleanup}.rs
host/src/worker/{provision,jit,supervise,logs}.rs
host/src/macos/{keychain,launchd,power}.rs
host/src/ipc/{protocol,server,client}.rs
host/src/evidence/{collect,verify,compare}.rs

cli/src/{main,args,dispatch,output}.rs
```

These paths are proposed. Fit them to actual conventions without changing ownership. Keep public interfaces narrow; use newtypes rather than passing interchangeable strings/integers; keep secrets out of `Debug`, `Display`, serialized records and tracing fields.

Use Tokio with bounded channels and tracked task lifetimes. Prefer a typed asynchronous Docker API adapter (Bollard is an appropriate candidate) for lifecycle/events/attach, with explicit socket and API negotiation. Docker context discovery may use a fixed read-only `docker context` argument vector. Do not use a default API constructor that can silently select another socket or provider. Qualify and pin the exact library release under current dependency policy. [S27]

Use the deferred plan's embedded local `turso` storage only after its exact release passes required durability, transaction, crash, and macOS qualification. Do not substitute Turso Cloud, remote replication, or the older libSQL SDK because their names look similar. A demonstrated failure of a required durability property needs an explicit architecture decision and passing alternative evidence, not silent substitution. Do not fabricate a release number from this document. [S04], [S28]

## 4. Operator experience and host configuration

### 4.1 Proposed command contract

```bash
# Uses the current local Docker context, persists its identity, imports existing gh auth securely.
velnor-host connect \
  --repo ChainArgos/java-monorepo \
  --scale-set ubuntu-26.04-scale-set \
  --platform linux/amd64 \
  --max-jobs 2 \
  --auth github-cli \
  --install-service

velnor-host status
velnor-host status --json
velnor-host doctor
velnor-host doctor --probe
velnor-host logs --follow
velnor-host drain --wait
velnor-host resume
velnor-host service install
velnor-host service start
velnor-host service stop
velnor-host service uninstall
velnor-host daemon run
velnor-host compare --repo ChainArgos/java-monorepo --run-id <ID> --attempt <N>
velnor-host disconnect --drain --wait
```

The first connection defaults to `max_jobs=1` when omitted; qualification proves `N>1`. `--docker-context <name>` is an explicit override. Initially support one active repository connection on this host; reject a second incompatible connection with a precise diagnostic rather than operating hidden independent capacity pools. Design scope IDs so multi-repository support is possible later, without implementing a fleet now.

`connect` is an idempotent orchestration command, not a command that permanently stays attached to the terminal. It performs read-only discovery, validates platform/capabilities and authorization, imports credentials, resolves the repository and group, reconciles the Scale Set, writes secret-free local configuration, installs/starts the service when requested, then waits for a bounded readiness result. Failure reports completed and pending phases and preserves recoverable state. It must not say “ready” merely because a process exists.

Prefer GitHub App credentials for unattended operation. Support IDs plus an imported private key reference, and existing GitHub CLI credentials for an intuitive first setup. Do not expose a PAT in command-line arguments. Import through in-process APIs/stdin, store in macOS Keychain, and retain only a reference in TOML. Do not assume `gh` credentials have runner administration permissions. Check authorization and report missing scopes/permissions without printing token values. [S18]

`doctor` is read-only by default. `--probe` explicitly authorizes short-lived, owned Docker probes and their cleanup. `disconnect` drains and only deletes remote resources whose creation/ownership is recorded; adopting a pre-existing set does not grant permission to delete it. Normal stop/restart never deletes the Scale Set. Force termination is a separately named, explicit operation with failed/cancelled outcomes—not an implicit behavior of graceful stop.

### 4.2 Proposed local TOML

This is separate from repository `.velnor/config.toml`. Paths and secret references are generated by `connect`; the example is not a credential-bearing file.

```toml
schema = 1

[github]
repository = "ChainArgos/java-monorepo"
scale_set_name = "ubuntu-26.04-scale-set"
labels = ["ubuntu-26.04-scale-set", "velnor"]
credential_ref = "keychain:com.tailrocks.velnor.host/chainargos"
# Resolved repository/group/set IDs are persisted after validated registration.

[host]
max_jobs = 2
min_idle_runners = 0

[docker]
context = "resolved-local-context"
platform = "linux/amd64"
# Persist the verified endpoint and engine identity in protected host state.

[execution]
profile = "ubuntu-26.04-x64-official-dind-v1"
runner_lock = "/absolute/path/to/verified/runner.lock"

[trust]
policy = "trusted-repository-v1"
fork_pull_requests = "deny"
```

Resolve platform image digests from the verified lock/product manifest, not from an unpinned registry tag. Reject unknown fields and invalid IDs, unsafe paths, reserved labels, non-local endpoints, contradictory auth options, and unsupported platforms before registration.

### 4.3 macOS service contract

Use a per-user LaunchAgent that starts `velnor-host daemon run` in the foreground. Do not double-fork or detach from launchd. Install a deterministic plist owned by the current user, with absolute binary/config paths, controlled environment and restrictive permissions. Keep state under `~/Library/Application Support/Velnor/`, logs under `~/Library/Logs/Velnor/`, and an appropriately short, private Unix socket path. Do not use a root LaunchDaemon merely to reach a user's Docker context. [S26]

The CLI and daemon communicate over a versioned local IPC protocol. Verify peer user identity where supported, restrict directory/socket access, bound request sizes and request deadlines, and redact responses. Every mutation goes through the single daemon/journal authority. A second daemon must fail safely before creating a second session.

LaunchAgent operation requires the user session and the Docker engine. Handle Docker starting late, engine restart, network loss, sleep/wake, locked Keychain, and logout explicitly. Preserve daemon readiness states such as `waiting_for_engine`, `waiting_for_credentials`, `reconciling`, `ready`, `draining`, and `degraded`. A process can be running but not ready to accept jobs. [S26]

Provide an explicit power policy to prevent system sleep while executing admitted work while permitting display sleep; do not change global sleep preferences behind the user's back. Use a scoped, tested mechanism. Do not promise operation while the Mac is powered off, logged out, asleep, or the engine is unavailable.

Ship versioned native macOS binaries, checksums and provenance. Test actual installation, launchd restart and upgrade on the target Mac. Stable code-signing identity and notarization belong in the release gate where credentials are available; otherwise label the distribution's actual signing status and keep that release qualification gate open. Never imply that a checksum is Apple signing or that signing guarantees third-party firewall behavior.

## 5. Rust Scale Set control plane

### 5.1 Authority and protocol coverage

Use `actions/scaleset` at the audited revision as the protocol authority, especially `client.go`, `session_client.go`, `listener/listener.go`, and `examples/dockerscaleset/scaler.go`. The Go SDK is a reference implementation; do not ship or shell out to it in production. Build sanitized fixtures that independently demonstrate compatibility, including behavior changed in the old PR. [S17], [S21], [S22], [S23]

Implement the complete control-plane sequence:

| Operation | Required behavior |
|---|---|
| Authentication | PAT or GitHub App → correct repository-scoped registration/Actions service credentials; separate installation, registration, session and JIT lifetimes |
| Registration | Resolve exact repository and runner group; get/create/adopt the named set; validate positive IDs, scope, returned name/group/labels, disabled updates; safely handle create races |
| Session | Create and own one message session; renew/refresh as required; detect conflicts without deleting another host's session |
| Poll | Long-poll with the upstream capacity header and last-message semantics; distinguish idle/empty responses from failures; use initial statistics |
| Demand | Parse `JobAvailable`, `JobStarted`, `JobCompleted`, and statistics; preserve upstream request/job IDs and message identity |
| Acquisition | Call Scale Set `AcquireJobs`; validate the actual acquired IDs; reconcile partial/uncertain results |
| Provision | Generate scale-set-scoped JIT and launch one ephemeral official runner per required worker |
| Acknowledgement | Acknowledge only after the batch's effects are safely replayable and durable; never acknowledge before required acquisition |
| Convergence | Reconcile from authoritative assigned counts plus observed worker lifecycles, including idle polls and restarts |
| Shutdown | Stop new admissions, drain, preserve diagnostics and state, close the owned session appropriately; do not delete the set on restart |

The scale-set JIT API is `/_apis/runtime/runnerscalesets/{scaleSetId}/generatejitconfig` relative to the correct Actions service base. It is not the classic repository REST `generate-jitconfig` route. Source all paths, API versions, headers, status codes and refresh rules from the pinned client; do not synthesize them from memory. [S21]

The official listener calls the scaler before deleting a message. Its Docker example explicitly acquires offered jobs and states that acquiring already-acquired IDs is a no-op. The example's in-memory maps, panic paths, and JIT-in-Docker-environment shortcut are not a production durability or security design. Reuse the protocol behavior, not its simplifications. [S22], [S23]

### 5.2 Wire correctness

Treat GitHub wire DTOs and validated internal types as separate layers. Local TOML is strict; an external Go/JSON wire model may legitimately have optional fields, omitted fields, or `null`. Implement per-field behavior matching the reference, not a blanket “default everything” deserializer. Required semantic identity still fails validation if missing or zero. Unknown optional fields may be tolerated for forward compatibility; unknown message kinds must produce a visible unsupported/reconciliation path, not be silently discarded and acknowledged.

Fixtures must cover null vs omitted statistics, Go scalar defaults, zero UUID/time representations where allowed, malformed values, overflowing IDs, message ID zero, duplicate deliveries, reordered start/completion observations, empty batches, partial acquisition, and a non-empty initial statistics snapshot. The SDK's synthetic initial message marker is not a real server message to acknowledge. Do not assume message IDs are contiguous. Preserve 64-bit request IDs end to end. [S14], [S22]

Use bounded, typed retries. Refresh on the reference's authentication-expiry conditions using single-flight token refresh; avoid a refresh stampede. Treat permission denial as an actionable authorization defect, not an infinite retry. Honor rate-limit and retry hints with jitter and maximum delays. Sanitize service-returned URLs and redirects before forwarding credentials; bind them to the configured GitHub/service trust policy. Do not log raw response bodies that may carry JIT/session credentials.

### 5.3 Durable capacity and admission

Use exactly one host-wide capacity authority. A permit covers a **top-level job lifecycle**, including its official runner, private Docker daemon and nested descendants. Reserved, acquiring, uncertain, provisioning, idle-assignable, running, finishing and cleaning lifecycles all consume capacity until they are resolved. A failed cleanup is not free capacity.

The controller starts at zero idle runners; pre-pull images instead of consuming every slot with speculative idle registrations. `max_jobs=N` bounds all local work. A future warm-idle option must use the same authority and be counted. Retain legitimate consumer tool/test concurrency settings equally in both lanes; do not inject hidden per-slot CPU/RAM quotas or compiler settings and then claim the same execution profile. Report Docker VM resource ceilings as actual host limits.

Order locally eligible demand by durable first-observed time and an immutable tie-breaker. Redelivery must not make an old request younger. This is local admission fairness, not a promise of global GitHub queue order: GitHub assigns eligible pending work to idle runners and a JIT worker is not inherently bound to one request ID. Do not derive worker capabilities or authorization from an assumed one-request/one-runner affinity. Use one homogeneous capability/trust profile per set. [S13], [S23]

`Statistics.TotalAssignedJobs` is population authority, not the length of the latest event batch. Do not add it to local reservations and double-count work. Determine desired runner population using explicit reconciled buckets: active workers, provisioning commitments, assigned work awaiting workers, terminal-but-cleaning resources, and unresolved external effects. The maximum-capacity header must implement upstream **total capacity**, not an accidentally substituted free-slot count. Test header behavior, drain behavior and already-assigned work against the reference and a real session. [S17], [S22], [S23]

### 5.4 Transaction/effect ordering

No database transaction can atomically commit a GitHub request and a Docker mutation. Use durable intent, idempotent operations, bounded retries and reconciliation:

1. Persist the received batch and idempotently apply start/completion observations.
2. Reconcile occupied capacity, validate eligible demand against trusted policy, and durably reserve grants before acquisition.
3. Persist an acquisition intent tied to scope, session epoch and exact request-ID set.
4. Call `AcquireJobs`. On success persist actual returned IDs. On ambiguous transport failure retain an uncertain intent and its reservations; retry/reconcile the same logical operation under the reference semantics.
5. Persist worker/provisioning intent before generating JIT or creating Docker resources. Use stable, unique ownership operation IDs.
6. Make every partial external operation discoverable after process death. JIT is never persisted; recovery may require resolving/removing an unused runner and issuing a fresh JIT rather than replaying a spent secret.
7. Acknowledge only when every relevant batch effect is completed or represented by a tested durable recovery path. Never acknowledge away an unacquired offer that is the only retained way to obtain the work without proving acquisition/re-offer semantics.
8. Release capacity only when required terminal observations, diagnostics and owned cleanup are confirmed. An ordinary Rust `Drop` must not silently release a durable occupied slot.

Persist and recover at least: daemon identity/epoch, scope/set/session identity without secrets, message and request deduplication, first-seen order, grants, acquire/provision/cleanup intents, actual runner identities, actual Docker identities, lifecycle observations, evidence-export state, schema version and migration history. Store transactionally consistent state, not an unbounded append-only JSON log masquerading as a journal.

Do not claim exactly-once external business side effects across GitHub reruns. The contract is no extra Velnor-originated execution from message replay, recoverable uncertain effects, and no false success.

### 5.5 Stop, restart and cancellation

The official runner receives GitHub job cancellation and owns step/post-step semantics. Velnor must allow its normal termination sequence, collect authoritative results, and apply a bounded escalation only when the worker cannot complete. Do not translate a container exit code into a successful GitHub job conclusion.

`drain` stops new admissions and prevents new assignable capacity while allowing already assigned/running work to finish. Distinguish administrative drain, service restart, user cancellation, job timeout, runner death, engine death, and host shutdown. A controller restart should adopt still-running owned workers; it must not kill them just to simplify bookkeeping. If the engine has lost them, reconcile the actual GitHub status and report infrastructure failure/uncertainty until resolved.

On restart, reconcile journal + exact Docker objects + GitHub state **before** advertising capacity. Detect engine identity changes and stop automatic mutations. Replacing a Docker engine or changing context requires an explicit migration/repair operation; absence on another engine is not evidence that old resources were cleaned.

## 6. Official runner image and Docker worker topology

### 6.1 Immutable image product

Build a reviewed Ubuntu 26.04 image containing a checksum-verified official runner distribution and the documented capability inventory needed by the generated workflows. Preserve the official runner unmodified. Include its required runtime dependencies and pinned Docker CLI/Compose/Buildx, Git, shell, certificates, sudo behavior and other explicitly qualified tools. Project Rust/Mise/tool versions remain controlled by the generator, not a competing daemon tool installer.

Pin the Ubuntu base digest, runner distribution version/checksum, runner image digest, private DinD image digest, platform, image manifest, and upstream protocol reference. Record the recipe/source closure and resolved package/tool inventory. Pin multi-platform child manifests, not merely a manifest-list tag. The lock must never accept `latest` as deployed identity.

Compare against the current `actions/runner-images` Ubuntu 26.04 inventory. Implement the consumer's required software/capability coverage, enumerate supported differences, and fail unqualified demands explicitly. Do not assert that the small official runner container has the hosted VM's entire preinstalled catalog. [S19], [S25]

Enforce disabled runner self-updates as a registration invariant and verify the actual running version. Immutable images need a scheduled, generated update/requalification process: GitHub requires disabled-auto-update runners to stay current within its deadline and may block critically outdated versions earlier. Do not use pinning as a reason to run an unsupported runner indefinitely. [S14], [S18]

### 6.2 Per-worker topology

```text
macOS: velnor-host (Rust, current user; management credentials and journal)
  |
  | management-only local Docker socket, exact context/engine identity
  v
Existing Docker Linux VM
  |
  +-- worker A: private privileged DinD container
  |     +-- nested job/service/action/BuildKit containers for A only
  +-- worker A: unmodified official Linux runner container
  |     +-- shares A's DinD network namespace
  |     +-- sees only A's private Docker socket
  |     +-- sees identical absolute workspace/temp/action/tool paths in DinD
  +-- worker A: unique named volumes for work, socket, private Docker data
  |
  +-- worker B: a completely separate copy of the above
```

The runner container is not itself privileged. The DinD container has only the privileges required for the qualified private daemon configuration. This is not a hostile-code sandbox equivalent to per-job hosted VMs: DinD and Docker share the provider VM's kernel. Treat the Mac/provider account and daemon-management socket as a trusted administrative boundary. No job is given the outer engine socket, host home, Keychain, SSH agent, controller configuration, or management token.

Use Linux-engine named volumes for work, temporary paths and sockets, avoiding macOS shared-filesystem socket and case-sensitivity assumptions. Two layers of paths matter: the runner passes absolute bind-mount source paths to the inner daemon, so those paths must resolve to the same data in the DinD container. This includes runner work, temp, downloaded actions, externals and tool directories as required by the pinned official runner. Test each mount with actual container actions and job containers. [S24]

The official runner includes paths that mount `/var/run/docker.sock`. Therefore the standard socket path **inside both the runner and the inner daemon's filesystem** must resolve to that worker's private socket, never the outer host socket. A named-volume-backed socket directory with a verified standard-path mapping is an acceptable implementation. Merely exporting a custom `DOCKER_HOST` is not proof that marketplace Docker actions and official mount logic work. [S24]

Joining the runner to the private DinD network namespace is intended to preserve localhost access for service-container published ports and Testcontainers. Verify both host-style jobs and `container:` jobs, service-name DNS, dynamic and fixed published ports, bind mounts, BuildKit networking, cleanup helpers, and parallel same-port workers. Do not expose the inner daemon over unauthenticated TCP or publish it to macOS.

The inspected official runner also checks its container/cgroup environment in `AssertCompatibleOS`. Qualify the actual provider's cgroup namespace behavior; do not assume Docker-in-Docker works merely because `docker version` succeeds. Do not patch the official runner or spoof `/proc` to bypass that check. A supported namespace/provider configuration or documented official-hook design must be proven before advertising the affected capability. [S24]

### 6.3 JIT delivery and credential hygiene

The host generates a JIT configuration for the Scale Set only after a durable provisioning intent exists. Deliver it through a short-lived attached stdin channel to a tiny, immutable entrypoint, with no terminal echo. The entrypoint reads exactly one bounded secret value and starts the official runner using its supported JIT input. The upstream runner consumes/removes `ACTIONS_RUNNER_INPUT_*` environment variables; use that supported mechanism inside the short-lived process, not a secret in Docker's persistent configuration. [S23], [S29]

Required prohibitions: no JIT/admin token in Docker `Config.Env`, `Cmd`, labels, image layers, build args, host argv, journal, TOML, launchd plist, persistent env file, exception body, debug dump or evidence archive. Do not derive a debug serialization for secret wrappers. Zeroize owned buffers where practical and redact downstream errors without embedding raw body text.

A process inside the worker may transiently observe its own runner configuration or spent JIT material; do not claim container root cannot inspect it. The critical separation is that long-lived host administration credentials never enter the worker and ephemeral material is not retained in control-plane metadata. Verify both positive runner startup and negative `docker inspect`/log/journal searches using test canaries.

### 6.4 Owned cleanup, not global cleanup

Label every outer Docker object with daemon UUID, worker UUID, operation generation, scope and role. Store immutable container/network IDs and image identities returned by Docker. Names are discovery aids, not sufficient delete authority. For named volumes, which lack the same immutable-ID model, use unique never-reused names, validate ownership labels and controller exclusivity, and document the residual assumption that the outer Docker administrator is trusted.

Before mutation, re-read and attest the object identity and relevant mount/network/image/owner projection. Never call host-wide `docker system prune`, delete by a broad name prefix, or clean another process's container because its name resembles a Velnor resource. Inner-runner cleanup commands operate only against the job's private daemon; final outer cleanup removes that worker's private engine and volumes after diagnostic export.

Delete in a safe dependency order, tolerate already-absent owned objects, reconcile partial cleanup after crashes, and quarantine ownership mismatch. A missing response is not a successful delete. Store only a sanitized inspect projection; arbitrary inspect output can contain secrets. Test same-name foreign objects and objects substituted between observation and deletion. [S13], [S14]

## 7. Generator routing, lane expansion and schema migration

### 7.1 Explicit schema evolution

Introduce a routing-capable schema revision (use schema 2 unless current repository evolution has already reserved that version). Implement a deterministic one-time migration from the current schema; do not reinterpret a schema-1 `runner_label` as a provider name. The migration must preserve all existing workflow, stack, resource, discovery and action settings and produce a reviewable diff. New features are never silently ignored by an old parser.

The target command is `velnor-actions config migrate --to 2 --write`. Preview should print/report the same proposed configuration without writing. Schema-1 consumers remain usable on their existing immutable generator version until explicitly migrated. Keep the existing hosted-only generation behavior unchanged for unchanged inputs; require a deliberate schema/pin/tree update for new routing.

Proposed routing configuration:

```toml
schema = 2

[workflow]
default_branch = "main"

[stacks.rust]
compile_driver = "mbx"
test_runner = "cargo_nextest"

[execution]
default_profile = "local"
mode = "both"                  # hosted | scale-set | both
hosted_profile = "hosted"
scale_set_profile = "local"

[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"

[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["ubuntu-26.04-scale-set", "velnor"]
platform = "linux/amd64"
capability_profile = "ubuntu-26.04-x64-official-dind-v1"

[execution.parity]
required = true
execute_verification = true
# Gate/timeout numbers must be selected and documented from the canary measurements.

# Keys reference stable logical planner job IDs, not rendered display names.
[execution.overrides.plan]
profile = "hosted"
role = "control"

[execution.overrides.required]
profile = "hosted"
role = "control"
```

Resolve semantics unambiguously: `execution.mode` is optional. With no mode or dispatch override, exact workload overrides take precedence over `default_profile`. An explicit workflow-dispatch mode takes precedence over configured `execution.mode`; either explicit mode forces eligible workloads onto the declared hosted profile, scale-set profile, or both profiles. Control/single-writer safety rules still apply. In `both`, an eligible workload executes on **both** paired profiles regardless of its normal default or override. A workload exclusion from forced placement or required pairing needs a predeclared capability/trust justification in the plan. The controller/control role is not a user escape hatch to bypass verification: validate role eligibility against planner-generated job kinds.

Unknown profile, platform mismatch, unknown logical job, conflicting overrides, duplicate labels, unsupported capability or selector collision must fail generation. The same plan report must show normal default, effective event/dispatch mode, each job's resolved profile(s), and every justified non-paired job. Schema field names above may be refined only with an explicit updated contract and fixtures, not left as contradictory examples.

### 7.2 Typed IR and expansion ownership

In `velnor-actions-contract`, introduce validated routing/profile/role types and provider-aware result identities. The renderer accepts a typed `RunsOn` representation, not arbitrary YAML strings. Hosted labels remain catalog-validated; scale-set labels use a distinct validated type. Emit a stable, canonical label order that includes the requested selector. Add typed `container`, `services`, dispatch choice and other workflow structures only where needed by the qualified contract; never create raw-YAML or arbitrary-shell configuration escape hatches.

The orchestrator creates one logical task graph, selects affected work once, then expands eligible workloads to `(logical_job_id, profile)` instances. Remap dependency edges within the same lane; maintain explicitly shared trusted control nodes. Preserve logical task IDs independently of physical job IDs. Providers must not change command/profile/features/fixtures/tool selection. Stack adapters must not learn Docker or host scheduling.

Example rendered job IDs and names:

```text
rust_test_backend__hosted    “Rust test backend / GitHub hosted / Linux x64”
rust_test_backend__local     “Rust test backend / Velnor Scale Set / Linux x64”
compare                     “Compare hosted and Velnor execution”
required                    “Required”
```

Duplicate all eligible verification workloads, not publication/deployment/business side effects. Planning, authoritative policy, final required aggregation, parity comparison and queue monitoring stay hosted. Signing, registry publication, tags/releases, deployment, baseline promotion and any other external mutation have one explicit writer. Disable matrix fail-fast for qualification. Branch protection continues to depend on the stable fail-closed required result.

The user's “each job on both” applies to comparable workload jobs. Record every control/single-writer exception before execution so no hidden exclusion can turn reduced coverage into a complete parity claim. Do not duplicate a production deployment just to obtain two green jobs.

### 7.3 Generated-file ownership and self-hosting

All workflow files introduced or changed by this campaign must be emitted by `velnor-actions`, including qualification, image release, macOS binary release and monitoring workflows. Preserve existing `.github` supporting files and local-action references according to the generated-file contract. Implement typed generator support where a release/qualification capability is absent. Do not copy old workflow bodies or patch generated YAML after generation.

Update actionlint label metadata, schema tests, snapshot/golden output, whole-tree ownership checks, unexpected-file checks, regeneration in a fresh directory, local-reference resolution, and policy validators. Include nested runner workspace changes in affected-work and candidate-source-closure calculation. A change to runner-only files must actually trigger their CI; a documentation-only change must not accidentally compile every workspace.

Retain publish-then-pin: ordinary consumers use an already released verified generator; candidate output is qualified in disposable trees; source is reviewed/merged under repository policy; new assets are published and verified; a promotion commit updates the pin, manifest and generated tree atomically. No normal consumer source-compilation fallback when a release asset is missing. [S02], [S06], [S10], [S13]

## 8. Cache, artifact and parity evidence contracts

### 8.1 Cache reuse without false verification

Preserve the generator's ownership of cache policy. The daemon must not invent its own competing interpretation of Cargo, MBX, Mise or task-result reuse. Ordinary repeated runs should restore declared tools and dependency/compiler caches rather than redownloading or recompiling them unnecessarily. The ephemeral worker does not require the cache itself to be disposable; it requires isolation and verifiable cache provenance. [S02], [S06]

For initial rollout use the existing GitHub cache transport and properly restored tool/source/compiler directories. A later host-local optimization must use explicit repository/trust/platform/toolchain/image compatibility namespaces, ownership and leases; never share a mutable Cargo target directory among unrelated concurrent jobs. Persistent caches must not contain JIT, GitHub credentials, checkouts masquerading as cache state, or another worker's private Docker socket/data.

In paired qualification, **both lanes must actually execute verification**. Reuse source/download/compiler caches where identities permit, but disable cross-run and cross-provider successful-test-result substitution for the paired tasks. A hosted “tests passed” result cannot satisfy the local lane, and vice versa. Record executed/reused status by task; an expected execution classified as reused is not qualifying evidence.

Trust and environment identity belong in executable/result cache keys. Cross-provider compiler cache reuse requires explicit compatibility; default result/artifact identity must include the provider/profile. Keep PR reads/writes consistent with the current generator's push-only trusted writer policy unless a separate reviewed cache-policy change qualifies a new behavior. Test main→PR read paths and prohibit untrusted promotion into trusted namespaces. [S30]

Measure at least cold baseline, immediately repeated warm run, changed Rust source, changed lockfile, changed toolchain, changed image, changed platform, corrupted cache and concurrent producers. For an unchanged warm run, retained available tool/source cache content must not be downloaded again just because the job is ephemeral. Report any legitimate network fetch and its cause; do not demand impossible “no network ever” behavior from GitHub/artifact/control-plane operations.

### 8.2 Evidence identity and immutable transport

Create an expected execution ledger before the workload fanout. Distinguish PR head SHA, tested merge SHA and workflow-definition SHA: use the planner's explicitly chosen checkout identity consistently in both lanes and validate it against GitHub. Each expected record binds:

```text
repository ID + source SHA + run ID + run attempt + workflow identity
+ generator/source-closure identity + logical plan digest + logical task/job ID
+ execution profile/provider + platform
+ command/configuration/features/fixture/input digest
```

Reports additionally record actual runner name/ID, scale-set ID where applicable, Docker engine/worker generation, actual image digest, actual official runner version, actual tools, task execution disposition, timing and semantic result manifest. Keep desired and observed values distinct. A GitHub job ID, runner request ID and local worker UUID are different identifiers and require an explicit verified mapping.

Upload immutable run/attempt/lane-specific artifacts. Capture the producer's returned artifact ID and pass that exact ID to consumers/verifiers. Verify manifest identity and digests after download. Do not download “the latest” artifact by a mutable or ambiguous name, and do not accept a same-SHA report from another run attempt. Stage required outputs into a stable snapshot before upload; do not attest a mutable workspace path and then upload whatever later occupies it. [S14], [S16]

Archive handling is an input boundary: bound compressed/uncompressed sizes, reject path traversal/absolute paths, symlinks/hardlinks/devices, duplicate paths, case-fold collisions, file/descendant collisions and unexpected contents. Required files must be regular, present and non-empty where the schema requires that. Do not execute downloaded content to verify it.

### 8.3 Strict comparison algorithm

The comparison job runs a pinned, verified verifier on GitHub-hosted infrastructure with minimal read permissions; it must not execute repository-controlled scripts merely to decide whether their claims are true. It consumes the predeclared ledger, complete GitHub job census for the exact attempt, immutable reports and host observations. Existing `velnor-actions` internal report/aggregation machinery should own generated-CI verification; `velnor-host compare` exposes the equivalent operator-facing inspection without requiring a macOS binary to run on Linux.

Enumerate every page of GitHub jobs and artifacts; follow the exact attempt, not a first-page/latest-attempt convenience endpoint. Confirm the API run/attempt/source identities, expected job conclusions and actual runner identities. A worker self-report is not alone proof of where a GitHub job ran.

Fail or return `NOT_PROVEN` for: missing expected lane/task; unexpected duplicate or conflicting result; skipped/cancelled/timed-out/failed job; wrong source/attempt/plan/profile; missing required artifact; unknown runner mapping; unavailable authoritative conclusion; execution replaced by cached success; or unsupported capability. A timeout fetching evidence is not equivalent to success.

Compare semantic outputs: test counts and identities, pass/fail/ignored outcomes with declared expectations, structured diagnostics, build manifests, generated-file digests where deterministic, and functional assertions. Do not compare every log byte or blindly strip timestamps and then declare equivalence. Any normalization must be typed, narrow and justified. Nondeterministic binaries need a reproducibility policy, not blanket hash equality or blanket hash exemption.

Expected exclusions must be known before execution: unaffected work, native-platform-only work, hosted control jobs and one-writer mutations. A failed local task cannot be reclassified as excluded after the fact. A hosted success cannot override a local infrastructure failure.

### 8.4 Queue, progress and performance

A self-hosted job can remain queued when no matching runner is online; a normal job execution timeout is not a sufficient queue watchdog. Generate hosted monitoring/recovery behavior with a bounded admission/queue deadline and a visible failed/not-proven result when the local provider is unavailable. The watchdog must start independently of the queued local jobs, not depend on their completion. Any cancellation authority is a separately scoped trusted operation restricted to the exact monitored repository/run; the ordinary evidence comparator remains read-only. Do not silently reroute the same required local lane to hosted infrastructure. A separately requested hosted-only run is a different execution mode, not local parity evidence. [S18]

Provide human and JSON status for queue age, session health, acquired/provisioning/running/cleaning counts, occupied vs maximum capacity, last successful poll, image/runner identities and last actionable error. Use event-driven Docker observation plus bounded reconciliation, not busy polling or repeated full host scans.

Measure queue wait, acquisition, provisioning, image pull, tool restore/install, dependency fetch, compilation, tests, artifact transfer, cleanup and end-to-end time-to-green. Record Mac/Docker VM resources and emulation. Hosted-vs-Mac timings are useful operational comparisons, not controlled evidence that one implementation is faster on identical hardware. Compare same-host before/after changes for optimization claims. Keep CPU-intensive research commands and build/test concurrency bounded; index the repository once and use scoped searches.

## 9. Security and trust model

The initial deployment is a trusted-repository runner, not a public arbitrary-code service on a personal Mac. Define allowed repository ID, approved workflow/ref/event policy and a dedicated Scale Set capability profile. Fork PR jobs stay hosted. Same-repository PR trust requires explicit policy and verified metadata; it is not implied by a target repository name in a scale-set offer.

Scale Set event payloads do not necessarily contain complete PR head-repository/trust information. Either validate with the authoritative GitHub run/PR metadata or restrict initial admitted events to those whose trust can be proven. Use a separate approved qualification workflow/ref for pre-merge local tests. Do not run untrusted head code via `pull_request_target` or a privileged `workflow_run` bridge. Workflow labels and a user's editable YAML cannot by themselves be the security gate. [S13]

Separate four credential classes: host administration/App private key; short-lived service/session tokens; one-use runner JIT; job-scoped credentials delivered by the official runner. Never transport the first three to arbitrary repository steps. Preserve the official runner's normal job-scoped credential handling, OIDC/cache/artifact behavior and masking rather than stripping required runtime tokens globally and breaking Actions.

No management endpoint is exposed to a container or network peer. The local IPC server validates ownership and request shape. Docker's outer management socket is accessible only to the host controller/trusted user. A job-private Docker socket grants root-equivalent power over that worker's private engine, not permission to mutate the outer daemon.

Review service-returned URLs, redirects, environment inheritance, executable/path selection, generated YAML injection, action pinning, archive extraction, credential redaction, symlink/TOCTOU cleanup, Keychain access, update provenance, cache poisoning, PR admission, privilege boundary and availability independently. Record threats that remain inherent to trusted DinD on a shared Linux VM; do not describe labels or containers as cryptographic isolation.

## 10. Rust pseudocode and structural invariants

These are architectural pseudocode excerpts. Domain types, constructors and database operations must be implemented and tested; the snippets are not claimed to compile as standalone programs. Keep production code free of `unwrap`, `expect`, panic/todo stubs and unconstrained stringly typed state.

### 10.1 Routing types: hosted labels cannot impersonate scale-set labels

```rust
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunsOn {
    Hosted(HostedUbuntuLabel),
    ScaleSet(ScaleSetSelector),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleSetSelector {
    name: ScaleSetName,
    labels: BTreeSet<RunnerLabel>,
    platform: LinuxPlatform,
}

impl ScaleSetSelector {
    pub fn validate(&self, profile: &CapabilityProfile) -> Result<(), ConfigError> {
        if !self.labels.contains(&RunnerLabel::velnor()) {
            return Err(ConfigError::MissingVelnorLabel);
        }
        if !self.labels.contains(self.name.as_label()) {
            return Err(ConfigError::MissingScaleSetLabel);
        }
        if self.labels.iter().any(RunnerLabel::is_reserved_hosted_label) {
            return Err(ConfigError::HostedLabelOnLocalRunner);
        }
        if self.platform != profile.platform() {
            return Err(ConfigError::PlatformMismatch);
        }
        Ok(())
    }
}
```

Construction validates legal label characters, lengths, uniqueness and canonicalization. Renderer output is derived only from this type; no free-form YAML `runs-on` string.

### 10.2 State transitions return effects; they do not perform them

```rust
#[derive(Debug, Clone)]
pub enum WorkerState {
    Reserved { grant: GrantId },
    Acquiring { intent: AcquireIntentId },
    AcquisitionUncertain { intent: AcquireIntentId },
    Provisioning { intent: ProvisionIntentId },
    IdleAssignable { runner: RunnerIdentity, owned: OwnedResources },
    Running { runner: RunnerIdentity, job: GitHubJobIdentity, owned: OwnedResources },
    Finishing { result: ObservedConclusion, owned: OwnedResources },
    Cleaning { owned: OwnedResources },
    Quarantined { reason: OwnershipFailure },
    Released,
}

pub fn transition(
    state: &WorkerState,
    event: &WorkerEvent,
) -> Result<Transition, StateError> {
    match (state, event) {
        (WorkerState::Cleaning { owned }, WorkerEvent::CleanupVerified(proof))
            if proof.covers(owned) =>
        {
            Ok(Transition::release_after_verified_cleanup(proof.clone()))
        }
        (_, WorkerEvent::OwnershipMismatch(reason)) =>
            Ok(Transition::quarantine_without_releasing_capacity(reason.clone())),
        (_, WorkerEvent::Redelivered(id)) =>
            Ok(Transition::idempotent_observation(*id)),
        _ => validate_and_plan_transition(state, event),
    }
}
```

Use distinct records for GitHub terminal conclusion and local cleanup outcome. A failed job can have successful cleanup; successful job execution can still leave an occupied quarantined slot. Neither fact erases the other.

### 10.3 Persist intent before network effects; retain uncertainty

```rust
async fn acquire_batch(
    journal: &Journal,
    github: &ScaleSetClient,
    scope: &SessionIdentity,
    offers: &[ValidatedOffer],
) -> Result<(), ControllerError> {
    // Transaction: deduplicate, reserve durable capacity, persist exact intent.
    // An empty result means no eligible capacity; it is not a successful acquisition.
    let Some(intent) = journal.reserve_acquisition(scope, offers).await? else {
        return Ok(());
    };

    match github.acquire_jobs(scope, intent.request_ids()).await {
        Ok(actual_ids) => {
            // Reject IDs outside the requested set; account for partial acquisition.
            journal.record_acquired(&intent, &actual_ids).await?;
        }
        Err(error) if error.effect_is_uncertain() => {
            // No permit release here. Reconciliation owns resolution.
            journal.record_acquisition_uncertain(&intent, error.class()).await?;
        }
        Err(error) => {
            // Roll back only with proof that no acquisition happened.
            journal.record_definite_acquisition_failure(&intent, error.class()).await?;
        }
    }
    Ok(())
}
```

Do not acknowledge the enclosing message merely because this function returned `Ok(())`. The batch controller must inspect durable admission/deferred/uncertain/provisioning state and apply the reference-compatible acknowledgement predicate. Retry an ambiguous acquisition with the same logical ID set when upstream semantics permit; do not allocate an unrelated second grant.

### 10.4 Lifecycle supervisor: bounded concurrency and replay-safe acknowledgement

```rust
async fn process_message(
    controller: &Controller,
    message: &ScaleSetMessage,
) -> Result<(), ControllerError> {
    let batch = controller.journal.record_received(message).await?;
    controller.apply_observations_idempotently(&batch).await?;
    controller.reconcile_owned_workers().await?;
    controller.admit_and_acquire_eligible(&batch).await?;
    controller.converge_from_assigned_statistics(&batch).await?;

    if controller.journal.ack_is_replay_safe(&batch).await? {
        controller.github.acknowledge(batch.message_identity()).await?;
        controller.journal.record_acknowledged(&batch).await?;
    }
    Ok(())
}
```

The final journal write may fail after the external acknowledgement. Recovery must therefore treat the previous durable state as replay-safe, not assume database ACK and remote ACK commit together. Use an actor or short transaction boundaries; never hold a mutex/database transaction across an HTTP call. Track spawned provisioning and cleanup tasks, cancellation and join failures. Do not let an unobserved background task mutate state after its controller epoch is fenced out.

### 10.5 Exact result-set verification must detect duplicates before collecting

```rust
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

pub fn verify_complete_results(
    expected: &ExpectedExecutionSet,
    observed: &[VerifiedExecutionReport],
    github: &VerifiedJobCensus,
) -> Result<ParityProof, EvidenceError> {
    let mut indexed = BTreeMap::new();
    for report in observed {
        expected.validate_identity(report.identity())?;
        match indexed.entry(report.execution_key().clone()) {
            Entry::Vacant(slot) => { slot.insert(report); }
            Entry::Occupied(_) => return Err(EvidenceError::DuplicateExecution),
        }
    }
    if indexed.len() != expected.len() {
        return Err(EvidenceError::IncompleteExecutionSet);
    }
    for item in expected.iter() {
        let report = indexed.get(item.key()).ok_or(EvidenceError::MissingExecution)?;
        github.require_success_on_expected_runner(item, report)?;
        report.require_actual_verification_execution()?;
        report.require_verified_artifact_identity()?;
    }
    compare_declared_semantics(expected, &indexed)
}
```

Never build a map that silently overwrites duplicates before checking them. The GitHub census itself must be complete and bound to the exact run attempt. Verification functions return typed proof objects only after checks—not booleans set by a caller.

## 11. Verification matrix and acceptance gates

Maintain `docs/implemented/macos-scaleset/verification.md` and machine-readable evidence. Every requirement has an ID, test/reference, observed result and evidence location. Distinguish `PASS`, `FAIL`, `BLOCKED_EXTERNAL`, and `NOT_RUN`. A skipped test is not a pass. PR-reported tests are reference claims until reproduced.

### G0 — Repository and policy baseline

- Resolve and record current source/PR/release/tag/manifest identities and actual open-PR census; read all applicable agent, architecture, review and release rules.
- Record a baseline job/task/required-context inventory for both Velnor and ChainArgos. Include nested workspaces and existing image/release rules.
- Reconcile the deferred specification with the new scope before implementation. Map still-applicable generator entry gates to current evidence and fix genuine prerequisites; do not label historical records as current passes.
- Verify a clean starting tree or preserve unrelated dirty work; never overwrite other agents' changes.

### G1 — Pure contracts and protocol conformance

- Config migration round-trip/idempotence and unknown-field tests; illegal hosted/local labels; platform/profile conflicts; exact per-job routing; trust admission.
- Property/state-machine tests: occupancy never exceeds N; no release without cleanup proof; no duplicate acquisition/provision from replay; stale epochs cannot mutate current state.
- HTTP mock/reference fixtures: exact paths/auth/headers/statuses, null/omitted fields, message zero, initial stats, empty poll, partial IDs, connection lost before/after acceptance, refresh single-flight, rate-limit backoff, conflict, unknown events and replay-safe ACK.
- Cross-check sanitized fixtures against pinned upstream source and a real Scale Set trace; mocks written from the implementation's own assumptions are insufficient.

### G2 — Durable storage and recovery

- Crash/fault injection before and after every journal transaction, AcquireJobs, JIT request, Docker create/start, runner connection, message ACK, job completion, log export and cleanup step.
- Reopen the real embedded database after forced process termination; verify grants/intents/message history and migrations. Exercise disk full, partial writes, failed commit, corrupt/incompatible schema and durable migration rollback/backup strategy.
- Live workers survive controller restart where supported; lost workers reconcile without claiming success. Empty/stale journal, changed Docker engine and another daemon's session do not trigger destructive cleanup.
- Uncertain acquisition and uncertain deletion retain capacity until resolved; deduplication works across restart, not just within one process.

### G3 — Docker and secret isolation

- Validate actual image/runner version, non-privileged runner, private DinD, absence of outer socket<LOCAL_SECRET_STORE> mounts, no exposed Docker TCP endpoint.
- Use canary secrets to check host argv, Docker metadata, labels, logs, TOML, plist, journal, diagnostics and published artifacts. Do not export the canary itself in test evidence.
- Verify same absolute paths for work/temp/actions/externals/tools; non-ASCII/spaced project paths; case-sensitive guest behavior; standard socket mapping; UID/GID permissions; inner registry-auth cleanup.
- Kill each container at each lifecycle stage; substitute foreign objects with colliding names; test malformed ownership labels and immutable-ID mismatch; ensure unrelated Docker resources survive.
- Validate available amd64 execution and nested image platform behavior on the target Mac. Report emulation; never label arm64 execution as x64.

### G4 — Genuine Actions feature qualification

Run the same generated qualification suite on hosted Ubuntu 26.04 and the real Scale Set. Include ordinary `run` steps, pinned JavaScript and composite actions, local actions, Dockerfile/container actions, `container:` jobs, workflow services, health checks, `localhost` and service DNS, job outputs/env/path files, secret masking, post actions, cache restore/save, artifacts, checkout of exact SHA, submodules/LFS where required, and permissions/OIDC scenarios with no production mutation.

Include Compose, Buildx build, Testcontainers and its cleanup helper, bind mounts, multi-container dependencies, concurrent workers using the same logical port, cancellation with a running service, failed main action with successful post cleanup, and intentionally failing steps. Expected-negative workflows must actually fail in GitHub; a surrounding qualification report passes only after independently confirming those expected failures.

Qualify the official runner's container/cgroup compatibility check on the actual Docker provider. No `act` emulation, native step simulator or fake GitHub logs can satisfy this gate. [S24]

### G5 — Generator and evidence negatives

- Hosted-only, local-only, both, default profile and exact overrides; paired graph dependency remapping; control jobs and single writers; no duplicate publish/deploy/baseline mutation.
- Byte-identical generation for identical inputs; actionlint; policy/security checks; full `.github` ownership; fresh-directory regeneration; consumer fixture rename to catch repository-name special cases.
- A failure added to either workspace must make the correct CI fail. Documentation-only changes must retain correct affected selection without unnecessary full builds.
- Drop a lane, duplicate a report, swap artifact ID, alter run attempt/source/plan/profile, use a stale same-SHA report, omit an API page, return skipped/cancelled/timed-out results, and inject unsafe archives: every required comparison fails closed.
- Disable/restore task-result reuse correctly for qualification; verify both lanes actually execute tests.
- Stop the Mac/daemon before a queued test: hosted watchdog reports no local qualification and does not silently run that lane on hosted infrastructure.

### G6 — macOS service and release

- Fresh user installation; no shell-profile/PATH dependence; repeated connect; existing/adopted set; service install/start/stop/uninstall; foreground mode; duplicate daemon; private IPC; JSON output stability.
- User logout/login, locked Keychain, absent/late/restarted Docker, network disconnection/reconnection and sleep/wake. Scoped power behavior must release its assertion/process on drain and shutdown.
- Upgrade from the prior build while drained; verify signed/checksummed assets, journal migration, atomic binary/config switch and rollback. Never downgrade an incompatible journal in place.
- Verify generated release workflows, exact image and binary provenance, runner update policy and actual macOS service behavior. Linux unit tests alone do not prove LaunchAgent operation.

### G7 — ChainArgos paired qualification

- Inventory exact current workload tasks and required contexts before/after; explain non-Rust coverage instead of silently dropping it.
- Publish verified generator/runner/image products, then update consumer configuration and generated files with the actual published generator.
- Prove a hosted baseline, then an approved small local canary at N=1, then N=2 with more than N eligible jobs, then complete paired execution at the same source/plan.
- Run cold and immediately repeated warm workflows. Verify correct tools/source/compiler reuse and real verification execution on both providers.
- Include representative Rust tests, Docker-dependent tests and the actual required Java/frontend/task capabilities discovered in the inventory; any additional typed generator capability must have its own contract and tests.
- Exercise cancellation, daemon restart with running jobs, engine loss/recovery and queue unavailability using safe qualification work—not production jobs.
- Retain actual workflow/run/job/artifact URLs and IDs, source/attempt identities, host/image/runner facts, sanitized lifecycle traces, expected/observed result counts and cleanup proof.

### G8 — Promotion and rollback

- Keep required checks and approval rules; do not merge simply because the coding agent finished. Follow each repository's actual commit/DCO/review policies.
- Verify eligible trusted workload default can be scale-set, paired dispatch remains available, and hosted-only recovery is explicit. Hosted control/required jobs remain available to diagnose local outages.
- Verify final PR and merged-main executions; final generated tree is produced by the pinned published generator, not an unpublished workspace binary.
- Rollback regenerates from the same supported schema with hosted placement, preserves required coverage, drains local work and retains evidence/state. Never delete another operator's set or wipe caches as a rollback shortcut.

### Required deterministic commands

Use the repository's pinned tool acquisition and established wrapper where applicable. The equivalent gates must cover both workspaces:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
cargo deny check --locked
alint validate-config
alint check --fail-on-warning
bash scripts/check-freshness.sh

cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace
cargo test --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace --doc
# Add an explicit, version-verified cargo-deny invocation for this manifest too.
```

Do not fabricate a tool flag from memory if the pinned tool uses a different order or option. Update the canonical local verification script and generated jobs to perform these checks once per necessary source closure. Benchmark resource usage and run independent work in parallel without oversubscribing the Mac or sharing conflicting mutable build directories.

## 12. Implementation workstreams and ordered rollout

The execution agent must actually delegate when its runtime provides subagents. Use explicit owners and shared interfaces to keep parallel work safe; use one integration branch per repository unless a technical reason requires another. Independent review must not be performed only by the author of the change.

| Workstream | Main ownership | Depends on |
|---|---|---|
| A: repository/documentation audit | Current source/PR inventory, superseding ADR, requirements/evidence ledger | None |
| B: generator routing and migration | Contract, schema, logical graph fanout, renderer/actionlint, control/single-writer rules | A's routing contract |
| C: Rust Scale Set protocol | Wire fixtures, auth/registration/session/acquisition/JIT, conformance | A's source pins and core IDs |
| D: durable supervisor | Core state, journal, grants/intents, reconciliation and IPC | Core interfaces; fake C/E adapters initially |
| E: Docker/image worker | Image product, private DinD, named paths/socket, secret delivery, owned cleanup | Core ownership contract |
| F: macOS CLI/service/release | `velnor-host`, Keychain, launchd, installation/upgrade, binary packaging | D's IPC/config contract |
| G: parity/security verification | Complete census, artifact verifier, negative tests, independent trust review | B/C/D/E integration contracts |
| H: ChainArgos rollout | Inventory, canary, both-provider verification, consumer promotion/rollback | Published qualified products and G gates |

The parent coordinates contracts, resolves conflicts, integrates and runs deterministic gates. Do not parallelize by letting several agents edit the same central files without ownership. Assign reviews for protocol correctness, durability, security, macOS behavior and generator coverage independently.

### Milestone sequence

1. **Document and baseline:** current-head/source/PR audit; disposition ledger; reconcile conflicting runner roadmap; freeze typed interfaces and expected outcomes.
2. **Offline implementation:** workspace/core/schema/CLI skeleton with real typed interfaces; protocol fixtures; journal/property/fault tests; deterministic renderer and evidence negatives. Stubs must not survive as enabled production functionality.
3. **Local Docker qualification:** immutable image, official runner packaging, private Docker semantics, secret delivery, ownership and recovery with controlled local probes.
4. **Real GitHub canary:** repository-scoped set/JIT, N=1, one generated approved qualification workflow; collect real source/runner/job identity.
5. **Concurrency and lifecycle:** N>1 with queue pressure, services/Docker actions/Testcontainers, crashes/restarts/cancellation and strict artifact comparison.
6. **Publish products:** generated hosted producers build/verify/attest runner image and native binary, publish exact assets, then promote generator metadata and generated trees. Do not bootstrap trust from a PR-provided checksum.
7. **ChainArgos qualification:** preserve current coverage, migrate configuration with published generator, execute both lanes cold/warm and negative lifecycle cases, verify exact artifacts and main after merge.
8. **Cutover:** eligible trusted workloads default to local; hosted control and explicit hosted recovery remain; both mode remains an ordinary generated option; finish runbooks and measured evidence.

Keep a frequently updated `docs/implemented/macos-scaleset/{checklist,evidence,decisions,legacy-disposition}.md` set. The checklist is small, stable requirement IDs linked to detailed tests; do not multiply competing plans. Every “done” entry names a concrete proof.

## 13. Definition of done and truthful stopping conditions

This goal is complete only when the product is implemented, released through the generated trusted path, installed on the actual macOS host, running official JIT Scale Set workers, and ChainArgos's required qualified workloads have succeeded in both locations with complete comparison evidence. Final main must use the published pinned generator and retain required checks. All owned canary leftovers are cleaned or explicitly retained for diagnostics with reasons.

The final report must contain exact commits, PRs, release assets/digests, host/provider/architecture, Scale Set and official runner identity, real workflow/run/job/artifact IDs, expected and observed coverage, test/negative-test counts, cold/warm measurements, cleanup/recovery results, and rollback instructions. Never include secrets.

When a real external prerequisite is unavailable—macOS host access, existing authorized GitHub credentials, Keychain user consent, signing credentials, protected environment approval, or repository permission—attempt the applicable read-only/probe path, record the exact failure and finish all remaining actionable implementation/testing work. Keep the affected live/release gate `BLOCKED_EXTERNAL` or `NOT_RUN`; do not call the rollout complete. Do not invent credentials, weaken approval rules, claim inaccessible tests ran, wait indefinitely on an unchanged prerequisite, or conceal a blocker with a hosted fallback. This is an evidence requirement, not permission to stop merely because the work is difficult.

## 14. Primary references and source map

The following identifiers are used inline. Repository paths are pinned to the research snapshot where practical; execution must separately record any newer adopted revision.

[S01]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/Cargo.toml "Velnor Actions workspace manifest"
[S02]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/proposed/architecture.md "Current generator architecture and config boundaries"
[S03]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/AGENTS.md "Current repository quality and execution rules"
[S04]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/deferred/self-hosted-runner.md "Deferred runner architecture to supersede explicitly"
[S05]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/README.md "Documentation index"
[S06]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/proposed/README.md "Full proposed-contract inventory"
[S07]: https://github.com/ChainArgos/java-monorepo/blob/5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45/.velnor/config.toml "Consumer configuration"
[S08]: https://github.com/ChainArgos/java-monorepo/blob/5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45/README.md "Consumer architecture and production Java/Rust distinction"
[S09]: https://github.com/tailrocks/velnor-new/releases/tag/v0.1.0 "Published Velnor Actions release"
[S10]: https://github.com/ChainArgos/java-monorepo/blob/5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45/.github/workflows/ci.yml "Actual generated consumer workflow and release bootstrap"
[S11]: https://api.github.com/repos/tailrocks/velnor-new/git/ref/tags/v0.1.0 "Resolved release tag identity"
[S12]: https://github.com/tailrocks/velnor/blob/3f6633252963efef0d71244aadae36516a11601e/crates/velnor-runner/src/scaleset/mod.rs "Legacy Scale Set implementation map"
[S13]: https://github.com/tailrocks/velnor/blob/3f6633252963efef0d71244aadae36516a11601e/plans/bastion-three-provider-ci/spec.md "Legacy durable capacity, ownership and parity design"
[S14]: https://github.com/tailrocks/velnor/pull/1133/files "Legacy open PR: protocol, ownership and evidence fixes"
[S15]: https://github.com/tailrocks/velnor/blob/27049c4bfca42d9d5a1e8cbbe584a2658ee5a77d/crates/velnor-runner/src/scaleset/registration.rs "PR-head registration and disabled-update invariant"
[S16]: https://github.com/tailrocks/velnor/pull/1135/files "Legacy open PR: immutable required-artifact verification"
[S17]: https://github.com/actions/scaleset/tree/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5 "Pinned official Scale Set reference"
[S18]: https://docs.github.com/en/actions/reference/runners/self-hosted-runners "Official scale-set, routing, ephemeral/update/auth/network requirements"
[S19]: https://docs.github.com/en/actions/reference/runners/github-hosted-runners "Hosted Ubuntu labels, platforms and VM distinction"
[S20]: https://docs.docker.com/desktop/features/networking/ "Docker Desktop macOS VM/network boundary"
[S21]: https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/client.go "Scale Set registration and JIT API"
[S22]: https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/listener/listener.go "Official listener ordering and acknowledgement"
[S23]: https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/examples/dockerscaleset/scaler.go "Official Docker example, AcquireJobs idempotence and population logic"
[S24]: https://github.com/actions/runner/blob/d7bc179baf11a02110b46cfbbc4040f74ac3f60a/src/Runner.Worker/ContainerOperationProvider.cs "Official job/service container mounts and compatibility checks"
[S25]: https://github.com/actions/runner-images "Official hosted software-image definitions; pin adopted Ubuntu inventory at execution"
[S26]: https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html "Apple launchd/LaunchAgent process and user-session requirements"
[S27]: https://docs.rs/bollard/latest/bollard/struct.Docker.html "Typed async Docker client; explicitly bind and qualify the chosen version/socket"
[S28]: https://docs.rs/turso/latest/turso/ "Embedded Turso Rust crate; qualify exact release rather than assuming remote/libSQL semantics"
[S29]: https://github.com/actions/runner/blob/d7bc179baf11a02110b46cfbbc4040f74ac3f60a/src/Runner.Listener/CommandSettings.cs "Official JIT command/environment input handling"
[S30]: https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-contract/src/workflow/ir.rs "Current workflow IR, dispatch inputs and trusted cache-save condition"

### Additional exact implementation-reading paths

- New generator: [`config/workflow.rs`](https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-contract/src/config/workflow.rs), [`config/mod.rs`](https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-contract/src/config/mod.rs), [`workflow/mod.rs`](https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-contract/src/workflow/mod.rs).
- Release coverage: [`docs/proposed/release-coverage.md`](https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/proposed/release-coverage.md); follow its linked implementation files and known gaps.
- Scale Set polling/refresh/acquisition: [`session_client.go`](https://github.com/actions/scaleset/blob/e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5/session_client.go).
- Docker action mounts: [`ContainerActionHandler.cs`](https://github.com/actions/runner/blob/d7bc179baf11a02110b46cfbbc4040f74ac3f60a/src/Runner.Worker/Handlers/ContainerActionHandler.cs), especially the standard Docker socket mount.
- Legacy private Docker: [`worker/dind.rs`](https://github.com/tailrocks/velnor/blob/3f6633252963efef0d71244aadae36516a11601e/crates/velnor-runner/src/scaleset/worker/dind.rs), [`worker/ownership.rs`](https://github.com/tailrocks/velnor/blob/3f6633252963efef0d71244aadae36516a11601e/crates/velnor-runner/src/scaleset/worker/ownership.rs), [`demand.rs`](https://github.com/tailrocks/velnor/blob/3f6633252963efef0d71244aadae36516a11601e/crates/velnor-runner/src/scaleset/demand.rs).
- PR-head ownership: [`docker_lease.rs`](https://github.com/tailrocks/velnor/blob/27049c4bfca42d9d5a1e8cbbe584a2658ee5a77d/crates/velnor-runner/src/docker_lease.rs).

Record the remaining documentation inventory and every legacy PR changed file in `legacy-disposition.md` during execution. Required dispositions are `adapt with tests`, `already satisfied`, `out of current scope`, or `superseded`, each with a correctness-based reason. No relevant safety fix is discarded merely because it originated in an unmerged PR; no unmerged fix is assumed correct merely because it exists.

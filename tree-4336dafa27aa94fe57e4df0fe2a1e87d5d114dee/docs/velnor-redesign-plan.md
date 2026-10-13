# Velnor Redesign: Extracted Project Plan

**Source:** [Shared ChatGPT conversation: “Redesign Rust GitHub Actions Runner”](https://chatgpt.com/share/6ab8e9dc-7a40-83ec-927c-a111832cb2f3)  
**Extracted:** 2026-09-27  
**Purpose:** Consolidate the conversation’s requirements, research findings, architecture proposals, constraints, implementation plan, and final scope correction.

> The conversation contains two design iterations. The first proposes a minimal self-hosted runner as the immediate product. The user then explicitly moves that runner to a later version and makes the V1 product a Rust GitHub Actions workflow generator. This document preserves the first proposal as roadmap/context and treats the corrected V1 scope as authoritative.

## 1. Final product definition

Velnor V1 is a workflow compiler and small CI task executor for Rust repositories. It scans workspaces and crates, builds a dependency-aware work plan, emits readable GitHub Actions workflows, and executes focused tasks through a prebuilt Rust binary, Mise, and MBX. It targets GitHub-hosted runners first and dogfoods itself.

The division of responsibility is:

- **Velnor:** discover Rust structure, decide necessary work, plan affected crates, generate workflows and task declarations, explain cache misses, and report outcomes.
- **Mise:** install and lock tools; provide qualified deterministic task-result caching.
- **MBX:** reuse compatible Rust compilation work.
- **GitHub Actions:** schedule generated repository-local jobs and workflows.

Do not reimplement these tools’ responsibilities. V1 is not a self-hosted runner, although Velnor’s own binary can run locally on macOS.

### Product progression

| Stage | Scope |
|---|---|
| V1 | Rust discovery, affected-crate planning, focused CI workflow generation and execution, Mise/MBX integration, caching, diagnostics, and self-dogfooding on GitHub-hosted runners. |
| V2 | macOS self-hosted execution using Docker. Reuse V1 workflows, task definitions, and cache conventions. |
| V3 | Debian self-hosted execution using the same supervisor and execution contracts. |
| Later runner milestone | Native Velnor GitHub Actions runtime, kept distinct from the V1 task executor and from running the official runner in a container. |

### Explicit V1 exclusions

No Docker client, runner registration, Scale Sets, Kubernetes, database, web server, TUI, guest agent, fleet configuration, generic workflow interpreter, remote cache service, deployment features, or arbitrary action execution. Do not import the old runner crate or its UI/test dependency graph.

## 2. User requirements

The user’s corrected request specifies:

1. Scan Rust projects, Cargo workspaces, and crates.
2. Generate best-practice, high-performance GitHub Actions workflows.
3. Use Mise for Rust, Rust components, and Rust-related binaries.
4. Avoid repeating downloads, builds, compilation, or execution when valid cached results satisfy the task.
5. Cache aggressively while preserving correctness and speed.
6. Use the latest stable MBX release through an explicit, locked tool update.
7. Keep workflow jobs and steps focused and readable; do not combine fmt, Clippy, build, and tests in opaque steps. Use MBX for compilation paths.
8. Fail fast before slower work when task dependencies permit.
9. Parallelize independent work where safe.
10. Test crates/components separately by default; retain genuine cross-crate integration tests as explicit targets.
11. Dogfood the product by using Velnor’s generator for Velnor’s own CI.
12. Keep the product minimal now and extend later to macOS Docker and Debian self-hosted runners.

The earlier request had asked for two self-hosted runtime modes on a macOS host: an official GitHub runner in a Linux container and a fully native Rust implementation of the Actions runner protocol. The user then moved that entire runner product to a later version. Its useful architecture and safety findings are retained in Section 11.

## 3. Cache contract and correctness

Translate “never repeat cached work” into this invariant:

> Never repeat expensive work when Velnor can establish that a valid, compatible, sufficiently trusted cached result already satisfies the requested task. Report every miss and deliberate re-execution with its reason.

Distinguish three levels of reuse:

| Reuse level | Reuses | Does not prove |
|---|---|---|
| Tool/download cache | Installed tools and Rust components; downloaded registry and Git dependency sources. | That compilation or tests succeeded. |
| Compilation cache | Compatible compiler outputs and supported build state. | That a test executable passed. |
| Task-result cache | A successful deterministic task result, declared outputs, and execution evidence. | That undeclared external state is unchanged. |

MBX hits are compilation reuse, never proof of passing tests. Cargo still runs tests and doctests unless a separate, qualified task-result cache proves reuse is valid. A cached exit code without required reports/artifacts is incomplete.

GitHub-hosted jobs usually run on fresh machines; restoring an archive still transfers bytes, and GitHub caches are immutable and can be evicted. Permanent zero-download behavior across independent machines cannot be promised. The practical goal is no avoidable origin downloads, tool installs, compilation, or deterministic task execution on a compatible warm path. Stale success is a correctness defect; necessary validation is not waste.

### Cache identity and policy

Cache/task identity must include the applicable closure of:

- Task definition and arguments.
- Local source and configuration dependency closure.
- Lockfiles and resolved dependencies.
- Tool identities/components, features, profile, target, compiler flags.
- Relevant environment values, fixtures, generated inputs, and execution environment.
- Cache schema and trust boundary.

Make ambient environment influences explicit. Mise’s cache audit is advisory; it does not prove determinism. Start with qualified formatting/generation checks, complete-identity Clippy, and explicitly qualified deterministic tests. Run tests with clocks, randomness, network services, undeclared files, or mutable external dependencies unless those factors are controlled and fingerprinted. Never treat publishing, deployment, or notifications as ordinary cached validation results. Provide uncached audit mode and distinguish **reused**, **executed**, and **not selected** outcomes.

### MBX and GitHub cache behavior

- Keep Cargo registry/Git source caching separate from MBX compilation state.
- Qualify MBX portable objects mode with sources cached separately; verify MBX’s current documented interface before implementing.
- Configure the MBX action to reuse the Mise-installed binary and check its reported version; avoid a second floating installer.
- Use compatibility prefixes (schema/platform/compiler/MBX format/workspace/task family) and immutable snapshots keyed by declared-input digest. Restore older compatible prefixes so MBX can validate objects.
- Include crate/config identity where parallel jobs could save different payloads. Do not use commit SHA as the only meaningful identity. Do not save unchanged data or retain unbounded historical archives.
- Do not archive the MBX store through both a generic cache action and the MBX action. Caching the Mise directory does not guarantee Rustup state is cached.
- Sequential tasks in one crate job may share a target directory. Concurrent Cargo processes require separate mutable target directories; MBX may provide compatible object sharing/deduplication. GitHub archives do not provide cross-machine distributed single-flight compilation.
- Default-branch CI should produce useful warm caches. Do not add an unconditional “build everything” warm-up pipeline.
- Restore trusted baseline caches in pull requests. Do not promote pull-request-produced executable cache contents to release trust just because they have checksums. Isolate production release builds from remotely restored compiler outputs where MBX guidance requires it.

## 4. Workspace and CLI proposal

The corrected V1 proposal uses three crates and one shipped binary:

```text
velnor/
├── Cargo.toml
├── Cargo.lock
├── mise.toml
├── mise.lock
├── .velnor/
│   ├── config.toml
│   └── generator.lock
├── crates/
│   ├── velnor-model/
│   ├── velnor-workflow/
│   └── velnor/
├── fixtures/rust-workspaces/
└── .github/workflows/ci.yml
```

- `velnor-model`: typed workspaces, packages, dependency edges, tasks, execution identities, workflow policy; mostly pure functions.
- `velnor-workflow`: workspace discovery, affected-task planning, deterministic workflow/task generation, generated-file ownership.
- `velnor`: one binary with CLI, subprocess execution, environment validation, structured output, and CI summaries.

Suggested CLI surface (proposed, not existing):

```text
velnor init
velnor scan
velnor generate
velnor generate --check
velnor plan
velnor run <task-id>
velnor explain <task-id>
```

The earlier runner design proposed four crates and two binaries: `velnor-core`, `velnor-github`, `velnor-host`, and `velnor-workflow`; a small daemon and host runtime; one durable journal; direct JIT registration; and fixed worker slots. That structure is deferred with the runner, not part of V1.

## 5. Discovery and affected-work planning

Use Cargo’s structured metadata interface to discover workspaces, packages, targets, and dependency edges. Model additions, removals, renames, dependency changes, and uncertain inputs. Select changed crates plus necessary reverse dependencies; provide an explain view. Be conservative when the base/head package graph is incomplete or inputs cannot be confidently attributed.

Default to per-crate pipelines/jobs. Preserve real cross-crate integration coverage as explicit integration targets or test packages rather than restoring one monolithic workspace job. Represent doctests and intentionally empty test suites explicitly so coverage is never silently lost.

The V1 planner should still produce a meaningful required check when no Rust compilation is necessary. Avoid workflow-level path filters that leave required checks pending. Use a small planning job and matrix outputs; enforce output-size limits and fail clearly when the plan exceeds its supported envelope.

## 6. Workflow behavior and task execution

Generated workflows should be repository-local, easy to inspect, and the executable output of authoritative configuration. Generate YAML deterministically from a small typed model; quote scalars correctly; reject arbitrary raw YAML fragments. Validate generated workflows with a qualified validator and golden fixtures. Do not ask consumers to invoke a large remote Velnor workflow that hides their CI structure.

Keep steps explicit, for example:

1. Restore/install tools.
2. Restore dependency sources.
3. Restore MBX compilation state.
4. Verify the prepared environment.
5. Run formatting.
6. Run Clippy.
7. Build test executables.
8. Run unit/integration tests (Nextest where qualified).
9. Run doctests separately.
10. Save eligible cache updates.
11. Report timings and reuse.

Warm-path steps that become no-ops should explain why. Pass task identifiers through validated environment values; do not interpolate uncontrolled repository data into shell scripts. Keep a final required gate that distinguishes: no work required by a valid plan; all planned work passed or was validly reused; required task failed; required task cancelled; expected work never ran; planning/generation failed. Only the first two succeed.

Fail fast when dependencies permit. There is an explicit tradeoff between starting all tests immediately and guaranteeing no tests start before lint succeeds. Use supported GitHub parallel/background/synchronization constructs only where genuinely useful and supported by pinned runner and validator; per-crate matrix jobs provide core parallelism without depending on these features. Avoid opaque shell backgrounding.

Use Nextest for ordinary unit/integration tests after qualification. Separate test compilation and execution using Nextest’s binary metadata/reuse interfaces; doctests remain their own MBX-backed task. Do not emit redundant “check everything, build everything, clippy everything, build tests, test everything, build again” sequences. Different compiler modes/features/profiles may require separate work. Start with default features and explicitly declared configurations; do not blindly enable `--all-features` because combinations may be invalid.

## 7. Toolchain management and version updates

Mise is the only tool-installation authority: Rust, required components, MBX, Nextest, and opted-in Rust tools are declared in `mise.toml` and its committed lockfile. Use Mise’s supported Rust `mr_boxington` integration rather than a separate `mbx setup` hook.

Resolve latest stable versions during an explicit tooling update; commit exact identities and execute the locked versions in CI. The source report observed MBX 1.18.0 as latest stable on 2026-09-25. Do not query “latest” independently in matrix jobs. One update mechanism should advance Mise, MBX, Rust, Nextest, and full-SHA action pins, regenerate workflows, and run qualification.

Separate preparation (restore/install tools and sources) from verification. Disable Mise automatic tool installation during task execution after preparation. Run Cargo locked/offline once required sources are prepared. Diagnose missing preparation inputs explicitly. Prefer prebuilt Rust tools; do not run `cargo install` for MBX, Nextest, or Velnor on each CI run.

## 8. Sources of authority and workflow security

| Source | Authority |
|---|---|
| Cargo manifests and lockfiles | Rust package structure and dependencies. |
| `mise.toml` / `mise.lock` | Tools and tool identities. |
| `.velnor/config.toml` | CI policy and explicit exceptions. |
| `.velnor/generator.lock` | Generator distribution identity, schema, and action pins. |
| Generated workflows and task definitions | Executable output; never maintained independently. |

Do not add a second handwritten project manifest duplicating Cargo structure or commands. Extract narrow useful concepts from old code—scanning, execution, reuse, generated-file ownership—not its fleet, promotion, APT, and unrelated modules.

Use full-SHA action pins, read-only default permissions, carefully scoped publishing permissions, and no privileged execution of untrusted pull-request code. Use pull-request plus default-branch CI without duplicate push/PR validation; support merge queues when used. A skipped workflow must not leave required checks unresolved. Keep planning outputs small. Never expose a privileged release trust path to untrusted cache contents.

## 9. Dogfooding, fixtures, and qualification

Bootstrap normal CI using a pinned, prebuilt Velnor binary. Verify its identity and execute it; do not compile Velnor just to decide which jobs run. Keep business logic in Rust, not an expanding installer script.

When Velnor changes:

1. Pinned bootstrap binary runs established CI.
2. Build the candidate binary once through MBX.
3. Candidate checks deterministic generation and expected generated output.
4. Candidate executes fixture tasks.
5. Reuse that exact candidate artifact in qualification jobs.

Do not require an older generator to reproduce a newer output contract. Workflow YAML generated during a running workflow cannot retroactively add jobs to it. Qualify generated workflows with committed fixture workflows or a disposable consumer repository that runs candidate-generated files.

Rust fixtures should cover:

- Standalone package and multi-crate workspace.
- Shared dependency chain, reverse-dependency selection, and independent workspaces.
- Build scripts/shared fixture inputs; feature-specific targets.
- Failing fmt, lint, and tests; doctests and intentionally empty suites.
- Renamed/deleted packages and base/head graph handling.
- Warm rerun and one-file change.
- Negative cache tests changing environment, fixture, compiler options, dependencies, or generated files; every relevant result must invalidate.

Measure total and queue time; bootstrap/tool preparation; cache restore/import/export/upload; origin downloads; compiler work; MBX hit/miss/bypass/unavailable; tests; task reuse; selected/executed/omitted tasks. Separate cold runs, warm runs, image pulls, tool install, and Velnor overhead. Compare per-crate parallelism against setup/transfer overhead. The plan proposes a two-minute warm-path target for a small fixture and normal dogfood path on a named runner, treating misses as regressions to investigate, never reasons to drop checks. A source review cannot guarantee timing; first establish a baseline on the actual runner.

## 10. Dependency and maintainability guidance

Proposed small V1 dependencies:

| Dependency | Purpose |
|---|---|
| `serde`, `serde_json` | Typed plans, metadata, reports. |
| `toml` | Small configuration files. |
| `cargo_metadata` | Cargo’s structured metadata interface. |
| `globset` | Declared input/exclusion matching. |
| `blake3` | Content fingerprints. |
| `clap` | CLI. |
| `thiserror` | Typed component errors. |
| `anyhow` | Application error context. |
| `tempfile` | Tests and temporary outputs. |

Use `std::process` for Git, Mise, MBX, and Nextest. No async runtime, embedded HTTP stack, or YAML runtime parser is needed for V1. Keep a small typed workflow representation and deterministic renderer; do not embed MBX or Nextest as libraries just to avoid binaries.

Preserve the Rust baseline: packages under `crates/`, tests in separate files, small entry points; suggested limits 400 lines for ordinary Rust files and 150 for `main.rs`/`lib.rs`, without broad grandfathering. The earlier runner proposal suggested ~350 production lines. Do not build-script-scan Git history or regenerate unrelated assets during builds. Keep development/test profiles distinct from release settings; tune using Cargo timings rather than copying aggressive release optimizations into the edit loop. Keep focused Clippy rules and error handling, but do not make Velnor a general Rust policy engine.

## 11. Earlier self-hosted-runner proposal (deferred)

This was the first assistant response, before the user moved runner functionality to later versions. It proposed a macOS Rust host supervising Linux Docker workloads, with two selectable runtime modes:

- `github-self-hosted`: direct repository-scoped JIT registration and an unmodified official runner in an ephemeral Linux container.
- `velnor`: a real native Rust Actions runner protocol implementation in the macOS process, with repository code executed only in a Linux container.

Initial topology: one Mac, one configured repository, one selected runtime, one slot first then a fixed `N` slots, one daemon lock and host-wide slot limit. Qualify the two modes sequentially. Docker Desktop executes Linux workloads in its Linux VM; this does not provide native Xcode, Simulator, or Darwin execution. Report control host (`macOS/arm64`), execution platform (`Linux/arm64`), and runtime separately.

The proposed official path used `POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`, then reserve slot, persist provisioning identity, request JIT config, start owned container, pass JIT startup config, preserve diagnostics, observe termination, clean owned resources, and replenish capacity. GitHub schedules assigned work; JIT registration does not reserve a particular job. A successful runner process exit does not prove a workflow passed; GitHub job conclusion is authoritative. Qualify runner auto-updates and refresh pinned images appropriately.

The native path would perform JIT config, auth, broker session, job assignment, run-service acquisition/renewal, admission, Docker execution, logs/step results, completion, and session/registration cleanup. Preserve auth refresh, leases, cancellation, live reporting, acknowledgement ordering, and distinct retryable/terminal/ownership-conflict errors. A local mock is not enough; complete one real job visibly in GitHub before expanding.

The proposed compatibility profile (`rust-ci-v1`) was deliberately narrow: one job with sequential Bash `run` steps; fresh checkout at exact assigned SHA; literal job/step environment; `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_OUTPUT`; only explicitly supported direct expression references; ordinary failure skips later ordinary steps; correct cancellation and live text logs; no arbitrary `uses:`, custom `if`, `continue-on-error`, reusable actions, job services, nested container actions, or local workflow DAG. Inspect admission payload and reject unsupported execution semantics before repository code runs. Implement environment file semantics correctly (subsequent-step effect, reserved `GITHUB_*`/`RUNNER_*`, blocked `NODE_OPTIONS`, multiline delimiter handling, byte limits, partial writes); never execute these file contents as host shell. Generate exact-SHA checkout; never substitute branch tip. Keep controller admin token out of workloads.

Host/runtime guidance: resolve active Docker context/socket and freeze endpoint/daemon identity; do not hardcode `/var/run/docker.sock`. Use a narrow Bollard wrapper for create/start/exec/inspect/logs/stop/remove/volumes and Docker API negotiation. Prefer Docker named volumes for Linux workspaces/caches, measured against bind mounts. Keep one SQLite journal for attempts, owned resources, and pending native completion reports. Lifecycle: `Empty → Registering → Idle → Running → Finishing → Cleaning → Empty`. Persist identities before external resources; label resources with installation/slot/attempt; reconcile after restart; do not replay uncertain steps or globally prune resources. Separate cleanup from GitHub result-delivery retries. Cancellation must stop the owned container/process tree, verify termination, and use bounded graceful then forced stop.

Security: keep registration credentials on host; pass only JIT/job-scoped data as required. Never mount host home or Docker socket into jobs. Avoid JIT material in persistent container metadata/logs. Bound/redact diagnostics and restrict repo/event allowlist to trusted work.

The initial runner dependency suggestions were `serde`/`serde_json`, `toml`, `tokio`, async `reqwest`, `jsonwebtoken`/RSA/key-format support/`base64`, `bollard`, `rusqlite`, `thiserror`, `anyhow`, `tracing`/`tracing-subscriber`, `clap`, `uuid`, optional `tokio-tungstenite`/`futures-util`, and test-only `tempfile`/`wiremock`. These were proposals, not a tested lockfile. Audit HTTP features carefully: do not accidentally drop OAuth form support; select one qualified non-vendored TLS provider.

That runner plan proposed a `velnor-rust-smoke` project (tiny lib/bin, tests, `Cargo.lock`, Mise tasks `ci:fmt`, `ci:clippy`, `ci:test`, `ci:identity`) reporting SHA, execution platform, Rust version, runtime. Qualification would prove success and real failure, env/file-command behavior, exact SHA, cancellation, recovery at side-effect boundaries, slot capacity, cache invalidation, credential isolation, and rejection before code execution. Then qualify a narrow Linux-compatible profile of `donbeave/essential-mac`, not its macOS-specific behavior or whole fleet.

Its six gates were: establish independent boundary; prove official container mode on Mac with real pass/fail jobs; prove one native protocol run step; add generated Rust profile and parity; lifecycle/recovery/security/caches with fault injection; measure and adopt with thin service install. Four suggested workstreams: GitHub protocol, host execution, generator/consumer, independent verification. Autoscaling was the main deferred runner feature; retain cancellation, ownership tracking, and correct GitHub reporting.

## 12. V1 implementation gates

1. **Minimal workspace:** three crates, one binary, configuration schema, generated-file ownership, fixture skeleton. Builds without legacy runner/UI dependencies.
2. **Discovery and planning:** workspace/package graph, affected selection, explain output. Correctly handle additions, removals, dependency changes, and uncertain inputs.
3. **First generated workflow:** early formatting gate, parallel per-crate pipelines, focused steps, meaningful required result. Passing and failing fixture workflows.
4. **Tool and compilation reuse:** locked Mise setup, source caches, qualified MBX pin (source plan says 1.18.0-or-newer at report time), compatible snapshot keys. Warm runs avoid unnecessary origin fetches and compilation.
5. **Task-result reuse:** qualified Mise caching, declared input closure, trust rules, invalidation tests. Exact-input reuse and changed-input invalidation both pass.
6. **Full dogfooding:** candidate binary qualification, self-generated CI, minimal binary publication, real consumer. No manually maintained alternate CI path.

Final V1 performance strategy: **select less work → reuse qualified task results → restore tools and sources → reuse compilation → parallelize remaining work → explain every miss.**

## 13. Conversation research notes and linked references

The first response described its evidence as a source-based architecture review of `tailrocks/velnor` at commit `42590cead02a75f097fd4013a09d8d6348ec3f93`, dated 2026-09-27—not a measured compile or live-run audit. It observed a ten-member workspace; `velnor-workflow` combining generation and CI execution with default TUI and runner crate in dev tests; a migration scaffold and broad operational subsystems; guest binaries, Debian/systemd packaging, fleet policies, MicroVM assets; a substantial Scale Set adapter; `checkout.rs` about 180 KB and generated `ci-main.yml` about 90 KB. File sizes were maintenance-surface examples, not complexity/compile measurements. Recommendation: extract behavior, protocol contract, and relevant tests rather than crates wholesale; record original and upstream revisions; port narrow tests; reject unsupported protocol semantics instead of silently ignoring fields.

References cited in the conversation include:

- [GitHub REST API: self-hosted runners](https://docs.github.com/en/rest/actions/self-hosted-runners)
- [GitHub secure use reference](https://docs.github.com/en/actions/reference/security/secure-use)
- [GitHub workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)
- [GitHub workflow events](https://docs.github.com/en/actions/using-workflows/events-that-trigger-workflows)
- [GitHub workflow commands](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands)
- [GitHub dependency caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
- [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
- [GitHub self-hosted runners](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)
- [GitHub Actions runner source](https://github.com/actions/runner)
- [Docker contexts](https://docs.docker.com/engine/manage-resources/contexts/)
- [Docker storage volumes](https://docs.docker.com/engine/storage/volumes/)
- [Mise Rust support](https://mise.jdx.dev/lang/rust.html)
- [Mise settings](https://mise.jdx.dev/configuration/settings.html)
- [Mise task caching](https://mise.jdx.dev/tasks/caching.html)
- [MBX GitHub Action](https://mr-boxington.jdx.dev/github-action)
- [MBX repository](https://github.com/jdx/mr-boxington)
- [Nextest running tests](https://nexte.st/docs/running/)
- [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [Cargo timing reports](https://doc.rust-lang.org/cargo/reference/timings.html)
- [GitHub Scale Set client](https://github.com/actions/scaleset)

The shared conversation itself is the source for these notes. Tool/version documentation can change; verify references during implementation and explicit tool-update work.

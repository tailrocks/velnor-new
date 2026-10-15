# GitHub Actions Official Reference & Compatibility Sources

## Purpose

This document establishes the authoritative sources of truth for GitHub Actions protocols, runner execution, scale set listeners, and runner environment images.

When designing workflows in the Velnor V1 generator or implementing runner infrastructure in `crates/velnor-runner`, developers and agents must **always check the original GitHub Actions source code for compatibility** and never guess protocol contracts or operational semantics.

---

## Official Source Repositories

### 1. Runner Core & Execution Protocol
- **Repository**: [actions/runner](https://github.com/actions/runner)
- **Role**: Official GitHub Actions runner daemon and execution engine.
- **Reference Areas**:
  - `src/Runner.Worker`: Job execution, step runner pipelines, command listeners, output and environment parsing.
  - `src/Runner.Listener`: Runner registration, session management, message polling, and job acquisition.
  - `src/Runner.Common`: Shared contracts, constants, job message schemas, and configuration formats.

### 2. Auto-scaling Runner Scale Sets
- **Repository**: [actions/scaleset](https://github.com/actions/scaleset)
- **Role**: Official controller and listener for GitHub Actions Runner Scale Sets.
- **Reference Areas**:
  - Scale set message protocol and session acquisition contracts.
  - Ephemeral runner lifecycles, graceful teardown, and runner pool auto-scaling contracts.
  - Compatibility with GitHub Enterprise Server and GitHub.com scale set endpoints.

### 3. Runner Environments & Tooling Inventories
- **Repository**: [actions/runner-images](https://github.com/actions/runner-images)
- **Role**: Canonical image definitions and software inventories for GitHub-hosted runners.
- **Reference Areas**:
  - Toolcache directory hierarchies (`RUNNER_TOOL_CACHE`, `AGENT_TOOLSDIRECTORY`).
  - Pre-installed utility versions, PATH layouts, and environment configurations.
  - OS-specific tool locations for Ubuntu Linux and macOS runner images.

---

## Compatibility Rules and Verification Procedures

1. **Protocol Fidelity Over Assumptions**:
   - `actions/runner` and `actions/scaleset` define the exact wire protocols, message shapes, and validation behaviors.
   - Any runner implementation or generator assumption must be verified against the official C#/Go source code in these repositories.

2. **Always Check Original Source Code**:
   - When resolving ambiguous runner behavior, environment variable precedence, or error handling semantics, inspect the official source code directly.
   - Do not rely on third-party summaries or stale documentation when the official source is available.

3. **Traceability in Design & Implementation**:
   - Every protocol decision, compatibility shim elimination, or runner contract in `crates/velnor-runner` must cite the specific source file and mechanism in `actions/runner` or `actions/scaleset`.
   - Reference updates and version bumps must be accompanied by compatibility verification against updated upstream sources.

---

## 2026 GitHub Actions & Dependabot Platform Contracts

### 1. Native Parallel Steps (`parallel:`) — `2026-06-25`
- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-06-25-actions-run-steps-in-parallel/), `actions/runner` v2.337.0+):
  - Jobs can group up to **10** independent sub-steps (`run:` or `uses:`) under a step-level `parallel:` block on the same runner.
  - Sub-steps execute concurrently against the shared workspace and runner environment without spinning up additional VMs or repeating checkout/cache setup.
  - **Constraints**:
    - Maximum of 10 sub-steps per `parallel:` group (`MAX_PARALLEL_SUBSTEPS = 10`).
    - Sub-steps cannot nest `parallel:` blocks.
    - Sub-steps cannot reference sibling sub-step outputs (`steps.<sibling-id>.outputs.*`); downstream sequential steps after the `parallel:` block completes can read sub-step outputs.
    - If any sub-step fails (unless `continue-on-error: true` is set on the sub-step), the parent `parallel:` step fails.
- **Velnor Implementation**:
  - `velnor-actions-contract` models `StepKind::Parallel { steps: Vec<ParallelSubStep> }`, `ParallelSubStep`, and `ParallelSubStepKind` (`Shell` / `Uses`), enforcing `1..=MAX_PARALLEL_SUBSTEPS` (10), unique step/sub-step IDs, and forbidding sibling `steps.<id>.outputs` references across sub-steps in the same `parallel:` group.
  - `velnor-actions-workflow-renderer` renders `StepKind::Parallel` as native `parallel:` YAML sequences.
  - `velnor-actions-actionlint` gates native `parallel:` step emission on `ActionlintCapabilities::supports_native_parallel_steps()` (default `false` for pinned `actionlint` `v1.7.12` until upstream `rhysd/actionlint` adds `parallel:` schema support).

### 2. Workflow Execution Protections — `2026-09-17`
- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-17-actions-new-workflow-execution-protections/)):
  - GitHub separates general workflow execution permissions from **restricted untrusted-actor triggers** that can run workflow code on a privileged ref or react to untrusted public events:
    - `pull_request_target`
    - `issue_comment`
    - `issues`
    - `discussion`
    - `discussion_comment`
    - `fork`
    - `gollum`
    - `label`
    - `public`
    - `watch`
    - `workflow_run`
  - Public repositories default to blocking workflows triggered by `pull_request_target`, `issue_comment`, `issues`, `discussion`, and `discussion_comment` from non-collaborators unless explicitly approved or relaxed by repository/organization policy.
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `ALLOWED_WORKFLOW_EVENTS` (`pull_request`, `push`, `merge_group`, `workflow_dispatch`, `schedule`), `RESTRICTED_ACTION_EVENTS` (the 11 restricted triggers above), and `DEFAULT_BLOCKED_PUBLIC_REPO_EVENTS`, along with `is_allowed_workflow_event`, `is_restricted_action_event`, `validate_workflow_trigger_event`, and `validate_workflow_trigger_events`.
  - Velnor's `Trigger` IR is structurally disjoint from `RESTRICTED_ACTION_EVENTS`: generated workflows never emit `pull_request_target`, `workflow_run`, or comment/issue triggers, ensuring public-repository forks execute under `pull_request` with read-only permissions and no secrets without requiring manual maintainer approval for restricted triggers.

### 3. Cache Access Control (`cache-mode`) — `2026-09-10`
- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-10-actions-control-cache-access-in-workflows-with-cache-mode/)):
  - Workflows and jobs can declare `cache-mode` at the workflow root or per job (`jobs.<job_id>.cache-mode`) to enforce runtime-level GitHub Actions cache service permissions:
    - `write`: read and write cache entries (default on standard events).
    - `read`: read-only cache access; cache writes are skipped/ignored without failing the job.
    - `write-only`: write cache entries without restoring existing entries.
    - `none`: disable all cache reads and writes.
    - Dynamic expressions (for example, `${{ github.event_name == 'pull_request' && 'read' || 'write' }}`).
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `CacheMode` (`Read`, `Write`, `WriteOnly`, `None`, `Expression(String)`) and optional `cache_mode: Option<CacheMode>` on both `WorkflowIr` and `Job`, with `CacheMode::for_pull_request_policy(PullRequestCachePolicy)` mapping `PullRequestCachePolicy::ReadOnly` to `CacheMode::Expression("${{ github.event_name == 'pull_request' && 'read' || 'write' }}")` and `PullRequestCachePolicy::SharedSeed` to `CacheMode::Write`.
  - `velnor-actions-workflow-renderer` emits top-level and job-level `cache-mode:` when `cache_mode` is `Some(...)` and omits it when `None`.
  - `velnor-actions-actionlint` tracks `native_cache_mode` in `ActionlintCapabilities` (`supports_native_cache_mode()`, `qualify_native_cache_mode()`, `check_native_cache_mode()`), defaulting to `false` for pinned `actionlint` `v1.7.12` until upstream `actionlint` recognizes `cache-mode`.

### 4. Dependabot Default Package Cooldown & Velnor Freshness — `2026-07-14`
- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-07-14-supply-chain-defense-by-default-dependabot-adds-3-day-package-cooldown/)):
  - Dependabot applies a **3-day default cooldown** (`cooldown.default-days: 3`, configurable from `1` to `90` days) to newly published package versions for version updates, reducing exposure to compromised releases during the initial publication window while keeping **security updates immediate**.
- **Velnor Implementation**:
  - `velnor-actions-freshness` supports an optional `default_cooldown_days` integer header key in `.velnor/version-policy.toml` (`DEFAULT_COOLDOWN_DAYS = 3`, bounded `MIN_COOLDOWN_DAYS = 1` to `MAX_COOLDOWN_DAYS = 90`).
  - When `default_cooldown_days` is omitted from `.velnor/version-policy.toml`, the freshness policy header check defaults to `3` days; values outside `1..=90` or non-integers fail the `policy-header` gate.
  - Security advisory scanning via `cargo-deny` remains immediate and is never delayed by `default_cooldown_days`.

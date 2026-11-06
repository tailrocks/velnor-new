# GitHub Actions Official Reference & Compatibility Sources

## Purpose

This document establishes the authoritative sources of truth for GitHub Actions protocols, runner execution, scale set listeners, and runner environment images. For artifact upload semantics, see the [pinned action source reference](github-actions-artifact-reference.md). For installation token transport, see the [official token compatibility reference](github-installation-token-reference.md).

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

The pinned [`xcode-27` runner image inventory](https://github.com/actions/runner-images/blob/9329c776c4a38d3dcb31fc78b8180005041c07fb/images/macos/xcode-27-arm64-Readme.md)
defines the available macOS/Xcode versions and installation paths. Velnor represents this ARM64 image as the typed `xcode-27-arm64` runner (`runs-on: xcode-27`).
Consumer qualification binds the exact `version`, `build`, `sdk_version` and `developer_dir` through `system_tools`; the image's default Xcode is not a substitute.
Velnor probes and runs the named task with that `DEVELOPER_DIR` and never changes the host's `xcode-select` setting.

---

## Compatibility Rules and Verification Procedures

### Performance review sources

The [review gates](../reviews/ci-performance.md) use these primary sources:

| Source | Review consequence |
|---|---|
| [Dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching) | Entries are immutable and branch-scoped; eviction and service failure require cold correctness. GitHub's default is 10 GB per repository; paid expansion does not change Velnor's adopted budget. |
| [Pinned cache restore implementation](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/restoreImpl.ts) and [save implementation](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts) | `cache-hit` reports exact key matching, not payload validity; partial matches need reconciliation, and unchanged exact entries need no save. |
| [Separate cache save](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/save/README.md) | Save reusable state after successful production; unrelated later test failures must not discard it. Velnor's writer and cancellation rules still apply. |
| [Job outputs](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/pass-job-outputs) and [workflow artifacts](https://docs.github.com/en/actions/tutorials/store-and-share-data) | Use validated job outputs for bounded metadata and identity-checked artifacts for files; never use a best-effort cache as the sole required handoff. |
| [Required status checks](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks) | Skipped jobs can report success and filtered workflows can leave checks pending; preserve Velnor's planner and fail-closed `Required` aggregation. |
| [Actions limits](https://docs.github.com/en/actions/reference/limits) | Verify current workflow, matrix, concurrency and cache request limits for representative consumer scale. |

These are review criteria, not new runtime behavior or cache capacity. Existing
[cache decisions](cache-decisions.md) and stricter
[compiled-state admission](compiled-state-persistence.md#budget) remain binding.

### Ledger event and source identity

The runner's [`GitHubContext.GetRuntimeEnvironmentVariables`](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/GitHubContext.cs)
exports the supplied `sha`, `workflow_sha`, `event_name`, and `event_path` context values as environment variables.
[`ExecutionContext.WriteWebhookPayload`](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/ExecutionContext.cs)
writes the event context to `event.json` and sets its exported path when that context exists.
GitHub's [pull-request event contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
identifies `GITHUB_SHA` as the test merge commit; the logical pull-request source is `github.event.pull_request.head.sha`.

Planning and ledger emission therefore share Velnor's event classifier and reference resolver.
Emission reads the bounded event file, rejects malformed JSON, and requires the independently resolved event and head to match the validated staged Plan before writing a ledger.
Fork pull requests use the contract event `fork`; `workflow_dispatch` uses `qualification`.
The original runner ref and workflow SHA remain unchanged, and Required keeps its strict source and event comparisons.
Public CLI regressions cover a synthetic merge SHA distinct from the logical head, both event normalizations, and rejection without a ledger when event context is missing, malformed, or changed.

### Native release job authority

Aggregate workflow status alone never establishes caller authority; verify the expected producer job independently.
The official [attempt-specific jobs endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt) supplies the bounded complete inventory selecting one generated role's positive job ID. The [individual job endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#get-a-job-for-a-workflow-run) refreshes its identity, status and conclusion using existing Actions read permissions. The runner's [GitHubContext export](https://github.com/actions/runner/blob/1ba885c2065fd9be317808ab3fd406f392635cf7/src/Runner.Worker/GitHubContext.cs) exports `GITHUB_JOB`; Velnor maps this machine identifier to the generated display name through a closed mapping checked against actual rendered jobs.
Native guards authenticate exact source, workflow, run and attempt; aggregate `queued` or `in_progress` requires no conclusion plus the unique expected job executing with no conclusion. Only immutable locators are cached; subsequent guards fetch the individual job again. Historical signed-run evidence remains separate from the current publisher, recovery or tap caller's authority.

### Reusable workflow signer identity

The [GitHub CLI attestation verifier](https://cli.github.com/manual/gh_attestation_verify)
supports a reusable workflow as the signer through `--signer-workflow`. GitHub's
[OIDC claim reference](https://docs.github.com/en/actions/reference/security/oidc)
defines `job_workflow_ref` and `job_workflow_sha` for the reusable workflow,
while `workflow_ref` and `workflow_sha` identify the caller workflow. Fulcio's
[official identity template](https://github.com/sigstore/fulcio/blob/e8104e83e11a651c7e9e712c02a57d5b43d19d5d/config/identity/config.yaml)
maps the reusable workflow claims to `buildSignerURI`/`buildSignerDigest`,
builds the certificate subject alternative name from the reusable workflow ref,
and maps caller claims to `buildConfigURI`/`buildConfigDigest`. The source
repository digest and run invocation URI identify the caller run.

The Runner-family publisher checks the Actions run as
`product-release.yml`, checks the exact repo-qualified referenced-workflow
path/ref/SHA for `product-release-images.yml`, and verifies attestations with
that callee as signer. The source selected by the caller can differ from the
caller's workflow-head SHA, so source identity comes from the closed producer
record, committed lock, OCI revision and successful same-source CI run rather
than from the certificate's caller source digest.

### Step shell resolution and `addPath`

[`ScriptHandler.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Worker/Handlers/ScriptHandler.cs) resolves a step's shell with `WhichUtil.Which(command, …, prependPath)`, where `prependPath` holds every `core.addPath`/`GITHUB_PATH` entry ahead of the job `PATH`. In a composite action (`IsActionStep`) even `shell: bash` takes this route. [`WhichUtil.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Sdk/Util/WhichUtil.cs) returns a fully qualified command as given when the file exists and never searches `PATH` for it; a custom shell string is split at its first space and must contain `{0}`. `jdx/mise-action` adds the Mise `bin/` and, by default, `shims/` with `core.addPath`, so after a tools-cache restore a bare shell or helper name can resolve to restored bytes. Velnor's restore checks therefore use `/bin/bash --noprofile --norc -p -e -o pipefail {0}` and reset `PATH` to `/usr/bin:/bin:/usr/sbin:/sbin` first ([hermetic checks](tools-cache-v2.md#the-checks-run-hermetically)).

### Workflow YAML sharing

GitHub's official [workflow reuse reference](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#yaml-anchors-and-aliases) documents same-document environment mapping aliases and an entire job alias containing a steps sequence. These are YAML node substitution, not a reusable
workflow or composite-action call: expressions and environment values retain
their use-site job and step context. The [official announcement](https://github.blog/changelog/2025-09-18-actions-yaml-anchors-and-non-public-workflow-templates/)
records workflow anchor/alias support.

Velnor uses a narrower grammar only when a workflow exceeds its unchanged
500,000-byte transport limit: exact job `env`, step `env`/`with`, whole step mapping, and existing `run` scalar reuse. It never moves execution, credentials,
permissions, dependencies, caches or outputs into a different scope. Definitions precede uses in one globally unique document namespace. Staged shell validation
normalizes only those emitted forms before its existing ancestry, effective-shell and ShellCheck checks. Input is bounded to 500,000 bytes, nesting to 32 levels,
and normalization work to 262,144 nodes and 16 MiB; merge keys, tags on shared nodes, forward/cyclic references and other alias scopes fail closed.

This Velnor admission does **not** extend to local action manifests. The original
runner's [legacy `YamlObjectReader.EvaluateCurrent`](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/YamlObjectReader.cs)
rejects scalar/mapping/sequence anchors and alias events, but
[`ActionManifestManager` at runner v2.337.0](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/ActionManifestManager.cs)
loads action manifests through the separate `GitHub.Actions.WorkflowParser`
reader. The legacy implementation alone does not prove that package's behavior.
Action-manifest alias compatibility is not qualified by this workflow-only
change; Velnor emits no new aliases into composite `action.yml` files.

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

## GitHub Actions & Dependabot Platform Contracts

### 1. Native Step Concurrency (`background`, `wait`, `wait-all`, `cancel`, `parallel`)

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-06-25-actions-steps-can-now-be-run-in-parallel/), [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepsbackground), `actions/runner` v2.335.0+; pinned runner v2.338.0 `src/Runner.Worker/BackgroundStepCoordinator.cs`, `JobExtension.cs`):
  - `background: true` on a `run` or `uses` step starts it asynchronously; the job continues to the next step. A composite action may run as a background step but cannot declare background steps or `parallel:` internally.
  - `wait` (one id or a list) and `wait-all` (no arguments) block until the referenced or all active background steps finish; neither accepts `if:` and both always run. Outputs and environment changes of a background step become visible only after a wait that includes it.
  - A failed background step fails the next `wait`/`wait-all` that includes it (unless the step sets `continue-on-error`); the coordinator merges member results into the wait step's result, so later default-`success()` steps are skipped while `always()` steps still run. An implicit `wait-all` runs before post-job cleanup for unjoined members.
  - Background steps are linked to the job cancellation token, so job `timeout-minutes` and run cancellation cancel them. `cancel` terminates one member (SIGTERM, then SIGKILL after a 7.5 s grace).
  - At most 10 background steps of one job run concurrently (`DefaultMaxBackgroundSteps = 10`); additional members queue until a slot is free.
  - `parallel:` is sugar for a group of background steps followed by a wait.
- **Velnor Implementation**:
  - `Step::parallel` in `velnor-actions-contract` marks a member of a contiguous group; members may not carry an id, a typed role or an internal operation. `velnor-actions-workflow-renderer` (`parallel_steps`) renders each member with `background: true` and joins each group with one generated `Wait for parallel checks` `wait-all` step; composite renderers, lane sharing and obligation extraction reject or skip parallel members.
  - Each Rust crate job uses one such group for its checks (see [hard decisions](../hard-decisions.md#1-one-job-per-crate-with-parallel-check-steps) and [native Rust crate jobs](../implemented/native-rust-crate-jobs.md)).
  - Pinned `jactionlint` v2.3.0 parses and validates `background:` members and the body-less `wait-all` join natively (rejecting `background: "yes"`, a `wait-all` with `run:`, and a step with neither `run` nor `uses`), so generation-time and CI lint pass no ignore pattern ([hard decision 10](../jactionlint-decision.md)). The superseded actionlint v1.7.12 could not parse the keys (rhysd/actionlint#693). Pinned `zizmor` v1.31.1 (support since v1.27.0) parses the syntax.

### 2. Workflow Execution Protections

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
  - Velnor's `Trigger` IR is structurally disjoint from `RESTRICTED_ACTION_EVENTS`: generic generated workflows never emit `pull_request_target`, `workflow_run`, or comment/issue triggers, ensuring public-repository forks execute under `pull_request` with read-only permissions and no secrets without requiring manual maintainer approval for restricted triggers.

### 3. Cache Access Control (`cache-mode`)

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode)):
  - Workflows and jobs can declare `cache-mode` at the workflow root or per job (`jobs.<job_id>.cache-mode`) to enforce runtime-level GitHub Actions cache service permissions:
    - `write`: read and write cache entries (default on standard events).
    - `read`: permit cache restores and deny saves at the service boundary; pinned-client handling is specified below.
    - `write-only`: write cache entries without restoring existing entries.
    - `none`: disable all cache reads and writes.
  - Whether `cache-mode` accepts `${{ }}` expressions is UNCONFIRMED: the workflow-syntax and dependency-caching pages list only the four literals. Details and the enforcement and default rules are in entry 5 below.
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `CacheMode` (`Read`, `Write`, `WriteOnly`, `None`, `Expression(String)`) and optional `cache_mode: Option<CacheMode>` on both `WorkflowIr` and `Job`, with `CacheMode::for_pull_request_policy(PullRequestCachePolicy)` mapping `PullRequestCachePolicy::ReadOnly` to `CacheMode::Expression("${{ github.event_name == 'pull_request' && 'read' || 'write' }}")` and `PullRequestCachePolicy::SharedSeed` to `CacheMode::Write`.
  - `velnor-actions-workflow-renderer` emits top-level and job-level `cache-mode:` when `cache_mode` is `Some(...)` and omits it when `None`.
  - Pinned `jactionlint` v2.3.0 parses workflow- and job-level `cache-mode` literals (`read`, `write`, `write-only`, `none`) and rejects any other value, including a `${{ }}` expression; `JactionlintCapabilities::check_step_syntax(StepSyntax::CacheMode)` records the capability and generation vets it.

### 4. Dependabot Default Package Cooldown & Velnor Freshness

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-07-14-supply-chain-defense-by-default-dependabot-adds-3-day-package-cooldown/)):
  - Dependabot applies a **3-day default cooldown** (`cooldown.default-days: 3`, configurable from `1` to `90` days) to newly published package versions for version updates, reducing exposure to compromised releases during the initial publication window while keeping **security updates immediate**.
- **Velnor Implementation**:
  - `velnor-actions-freshness` supports an optional `default_cooldown_days` integer header key in `.velnor/version-policy.toml` (`DEFAULT_COOLDOWN_DAYS = 3`, bounded `MIN_COOLDOWN_DAYS = 1` to `MAX_COOLDOWN_DAYS = 90`).
  - When `default_cooldown_days` is omitted from `.velnor/version-policy.toml`, the freshness policy header check defaults to `3` days; values outside `1..=90` or non-integers fail the `policy-header` gate.
  - Security advisory scanning via `cargo-deny` remains immediate and is never delayed by `default_cooldown_days`.

### 5. Cache rules verified against original sources

Client behavior below is bound to [`actions/cache` at `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`](https://github.com/actions/cache/tree/55cc8345863c7cc4c66a329aec7e433d2d1c52a9), which bundles `@actions/cache` 6.1.0, and the corresponding [toolkit source at `0786132e6a1a4451c2d392bbbf481c2b172d4312`](https://github.com/actions/toolkit/tree/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src).
Platform behavior comes from GitHub's [dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching), [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax), [Actions limits](https://docs.github.com/en/actions/reference/limits) and [REST cache API](https://docs.github.com/en/rest/actions/cache). Requalify client behavior when its pin changes and verify current service limits before relying on them.

#### 5.1 Storage limits, eviction and retention

- GitHub defaults to 10 GB per repository and seven days without access; administrators can configure storage and retention. Storage above the free allowance is billed. Velnor's adopted quota remains binding regardless of paid platform expansion.
- Eviction removes least-recently-accessed entries. An over-limit save can succeed before eviction; successful saving therefore does not prove sufficient admission headroom.
- Billing-budget exhaustion can make caches read-only. Never assume a larger configured quota guarantees a successful write.
- The REST API exposes storage- and retention-limit controls. Resolve their documented routes and permissions before implementing administration; do not infer them from the cache data-plane API.

#### 5.2 Immutable entries and save ownership

- Existing cache contents cannot be updated. Identity includes key, version and scope; the same key can identify distinct entries in different scopes or versions.
- The pinned action [skips saving an exact primary-key hit](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts). Concurrent reservation refusal is a nonfatal save outcome; it does not prove which producer supplied complete state.
- The [version hash](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/cacheUtils.ts) includes paths, compression, platform-specific settings and a salt. Review writer ownership against the complete identity.
- The combined action defaults to [`post-if: success()`](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/action.yml); `save-always` is deprecated. Use a separately guarded save when successful production must survive unrelated later validation failures.

#### 5.3 Restore matching

- Search tries an exact primary key, partial primary-key matches and ordered restore-key prefixes within the current branch before repeating on the default branch. For multiple partial matches, newest means creation time, not last access.
- Matching order is a documented service contract; the client only sends keys and version. A partial match requires payload reconciliation before use.
- The pinned client permits at most ten total keys, at most 512 characters per key and no commas. A restore must match the cache version; gzip and zstd entries do not cross-restore.
- `cache-hit` proves only an exact primary-key match, never validity, completeness or current-source validation.

#### 5.4 Cache scope and trust

- Runs can restore current-branch and default-branch caches; pull requests can also restore their base-branch scope. Child, sibling and unrelated tag scopes are isolated.
- Pull-request writes belong to `refs/pull/<number>/merge`, not the default branch, and cannot be restored by the base branch or other pull requests.
- Reuse across later pushes to the same pull request is inferred from its stable merge-ref scope. Require producer/consumer logs proving that path before claiming it works; same-run access does not establish it.
- Fork pull requests can read base/default-branch caches. Never place secrets or private state there, and preserve Velnor's read-only trust policy.

#### 5.5 `cache-mode` and client enforcement

- Accepted literals are `read`, `write`, `write-only` and `none`; a job-level setting overrides its workflow setting. The service enforces the effective mode through scoped tokens.
- Trusted triggers default to `write` and restricted low-trust triggers to `read`. The documented default does not establish `pull_request` behavior; use explicit Velnor policy.
- Pinned toolkit 6.1.0 does not inspect `ACTIONS_CACHE_MODE`. A denied save still attempts reservation and emits a warning; denied restore uses its generic failure path. Do not mistake service denial for skipped client work.
- A reusable workflow cannot exceed its caller's explicit mode; without one, a callee can request `write`. Explicit write access on a restricted trigger restores poisoning risk.
- Expression support remains unqualified: the platform references list literals, and pinned `jactionlint` rejects expressions. Velnor must use qualified syntax.

#### 5.6 Compression and archive construction

- The pinned [compression selector](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/cacheUtils.ts) chooses `ZstdWithoutLong` when zstd is available, otherwise gzip. It does not select the legacy `--long=30` variant for new saves.
- [Tar construction](https://github.com/actions/toolkit/blob/0786132e6a1a4451c2d392bbbf481c2b172d4312/packages/cache/src/internal/tar.ts) uses `zstdmt` on non-Windows and `zstd -T0` on Windows, with no compression-level flag. Qualify the actual zstd default before budgeting it.
- Readers retain support for the legacy `--long=30` format, whose window can require 1 GB. Archive names are `cache.tzst` or `cache.tgz`, and compression participates in version identity.
- Creation uses `tar --posix -P` and a manifest. Absolute path forms are retained; platform-specific flags include delayed directory restore on macOS and GNU tar's `--force-local` on Windows.

#### 5.7 Extraction and failure handling

- Restore downloads and extracts within one action step using `tar -xf ... -P -C ...`. No user inspection step runs between download and extraction; debug listing is not filtering.
- The pinned extraction path has no expanded-size, entry-count or free-space admission and does not prevent absolute paths or overwriting existing files. Download completeness checks do not provide these protections.
- Cache contents are unsigned. Verify restored paths and executable state before use; a cache hit does not authenticate them.
- Restore errors return a miss after logging; partial extraction is not rolled back. Cold fallback must handle that partially materialized state safely.

#### 5.8 Entry size, request limits and retries

- Only the legacy v1 save path checks a 10 GiB compressed archive limit, with a GitHub Enterprise Server exception. The v2 client has no equivalent check; a service-side v2 per-entry cap is unqualified. Repository quota is a separate constraint.
- Documented per-repository rates are 200 new-entry uploads, 1,500 downloads and 400 deletions per minute. Account for all writers, readers and pruning together.
- The pinned client reports `retry-after` on HTTP 429 and throws without retry. Its retryable statuses are 502/503/504, with two default retries and a 5,000 ms delay. A nonfatal save failure does not establish successful persistence.

#### 5.9 Authorized writers

- Default-branch writes are permitted by default for `push`, `workflow_dispatch`, `repository_dispatch`, `delete`, `registry_package`, `page_build` and `schedule`. Other events resolving to that branch, including `pull_request_target`, `issue_comment` and `workflow_run`, default to read-only; an explicit write-capable mode changes that boundary.
- Dispatches to another ref write to that ref's scope. Pull-request writes are confined to their merge ref.
- Fork pull-request save behavior, merge-group save behavior and the pull-request default mode remain unqualified service behavior. Velnor keeps PR/fork/merge-group jobs read-only regardless.
- The pinned action's [event gate](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/utils/actionUtils.ts) checks only that `GITHUB_REF` exists. Enforce Velnor's writer policy independently of client permissiveness.

#### Qualification boundaries

Service matching and scope rules rely on official documentation, not public service implementation. Require live qualification for same-PR cross-push reuse, service-specific size limits and warning behavior before making acceptance claims. Upgrading an action or toolkit version requires rechecking cache-mode handling, archive flags and failure outcomes; newer-client behavior does not qualify the pinned client.

## Native preview publication API boundaries

The [workflow-jobs REST contract](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt)
provides the attempt-specific endpoint
`GET /repos/{owner}/{repo}/actions/runs/{run_id}/attempts/{attempt_number}/jobs`.
The job response includes `run_id`, `head_sha`, and `head_branch`; it does not
include a `run_attempt` property. Velnor binds the attempt through that endpoint,
checks those returned identities and the single successful `Required` job, then
re-fetches the latest workflow run and rejects a changed run ID or attempt.
Executable API fixtures omit the nonexistent job property so they cannot conceal
an incompatibility with the upstream response shape.

The [repository REST contract](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository)
provides `GET /repos/{owner}/{repo}/immutable-releases` with `enabled` and
`enforced_by_owner`. The [release REST contract](https://docs.github.com/en/rest/releases/releases)
returns each published release's `immutable` status; its assets expose byte sizes
and `sha256:` digests. Native publication must check the prerequisite before any
unfinished mutation and independently verify the resulting immutable release.
Draft creation, asset uploads, and publication are separate mutations. No
observation authorizes replacing an asset, forcing a tag, or deleting a release.

The immutable-release settings GET requires Administration read permission. The
workflow token cannot request this permission through `permissions: contents`.
Native publication therefore uses the separate `PREVIEW_IMMUTABILITY_READ_TOKEN`
environment secret only for this fixed settings route, while release mutations
continue using the workflow token. The settings credential is never forwarded
to repository tasks, the signer, or tap promotion.

Release responses do not include `make_latest`; it is a request-only string enum.
`gh api --field`
converts literal booleans and expands `@file` values, so native release requests
use `--raw-field` for string fields and `--field` only for draft/prerelease
booleans. Both channels publish with `make_latest: false`; preview releases are
also prereleases, while stable releases remain ordinary releases without being
forced to latest. The release response has no `make_latest` field, so Velnor
verifies the returned draft/prerelease/immutable state and exact assets without
assuming that a newly published stable release becomes latest.

### Review-completion provider qualification

The [official Codex GitHub review guide](https://learn.chatgpt.com/docs/third-party/github)
documents requested and automatic reviews, but does not establish a supported
machine-state completion protocol. Native merge admission therefore continues
to require the separately authenticated `velnor.review-completion.v1` check;
reactions, table prose, and an absent finding are not substitutes.

A provider summary or completion marker does not qualify an adapter. Require a
supported completion contract binding the full head to complete code and security
finding dispositions; observed App implementation details are insufficient.

GitHub's [IssueComment GraphQL fields](https://docs.github.com/en/graphql/reference/issues#issuecomment)
expose the latest editor and edit time. Any future adapter must authenticate
the actual numeric App, author, and latest editor, independently fetch the PR's
full current head, parse closed machine fields, and revalidate all current
feedback. Positive exact-head completion and forged/edited-state rejection
remain deployment gates. Unattended merging stays disabled until that proof
exists; an unsupported success enum must fail closed.

The [required-review documentation](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews)
distinguishes a comment from approval: a blocking change request requires the
reviewer's subsequent approval or an actual review dismissal. A later comment
must not clear that request. Native feedback aggregation must preserve this
distinction independently of response order, and must reject unknown states.
GitHub also exposes [current per-user review connections](https://docs.github.com/en/graphql/reference/pulls#pullrequest);
any server-side opinion query must retain complete pagination and bind the same
PR/head as the full feedback snapshot.

Native capture uses paginated, server-derived opinionated state alongside all
underlying feedback records. Comment reviews and dismissed reviews must not be
mistaken for an active approval or change request. Unexpected opinionated states
still fail closed.

### Pull-request response boundaries

The pull-request listing omits the `merged` Boolean. Native completed
reconciliation must fetch each matching PR's detail
before trusting that state. A non-null merge SHA alone is insufficient because
the [pull-request API](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request)
also uses that field for a synthetic test merge before completion.

The [Git Database commit endpoint](https://docs.github.com/en/rest/git/commits#get-a-commit-object)
returns `message` at the top level; `commit.message` belongs to the repository
commit response. Protocol fixtures must model the endpoint actually requested.
Historical proof reconstruction binds the original branch base and commit parent,
while the actual PR detail base is retained separately. Selected open and merged
PRs must match that original base. A closed, unmerged historical PR may have
observed a later base without losing its original proof or feedback obligations.

The exact-head feedback query includes numeric actor identity for both `User` and
`Bot` in `latestOpinionatedReviews`, including `Bot.databaseId`. Selecting only the
`User` fragment makes bot reviews unidentifiable. Protocol coverage must include
a bot's outstanding change request.

GitHub documents that applicable rulesets layer with classic branch protections;
rules are aggregated and the most restrictive duplicated rule applies. A strict
status-check source therefore continues to require an up-to-date branch when
another source has loose checks. Native protection resolution must union required
checks and retain strictness from any authenticated effective source, rather than
rejecting an otherwise valid loose contributor. Classic PR-review protection is
nullable; disabled review protection contributes no nested PR-bypass allowances.
Missing controls must not prevent complementary authenticated sources from being
combined; malformed present controls must not become positive protection evidence.
Sources: [ruleset layering](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets),
[strict and loose status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets),
and [classic branch protection API](https://docs.github.com/en/rest/branches/branch-protection).

GitHub's [protected-branch documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
states that after required status checks pass, commits may be pushed directly to
the protected branch or merged through a pull request. Strict status checks only
require the PR branch to be current before merging. Therefore a workflow whose
contract promises PR-only promotion must independently require the server-side
pull-request rule; required checks and a client-side PR workflow are insufficient.

## Job budgets and cancellation

The pinned runner's [executable-job schema](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Sdk/DTPipelines/workflow-v1.0.json#L55-L75)
accepts `timeout-minutes` on a job with `runs-on`. GitHub's
[job-timeout contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes)
says expiration automatically cancels that job; its default is 360 minutes.
GitHub's [Actions limits](https://docs.github.com/en/actions/reference/limits)
cap each GitHub-hosted job at six hours of execution time (self-hosted: five
days), so Velnor's configurable upper bound is 360 minutes.
Velnor must explicitly emit a timeout between one and 360 minutes (five by
default; never GitHub's implicit default) on every
executable job, including CI, qualification, native image, signing, publication,
recovery, and reusable-workflow callees. GitHub provides no workflow-wide timeout
keyword: queued time, environment approval, and successive jobs are separate
from this per-job execution budget.

A [reusable-workflow caller](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations#supported-keywords-for-jobs-that-call-a-reusable-workflow)
allows a restricted set of job keys that excludes `timeout-minutes`. Emit the
limit on every concrete job inside the called workflow; adding it to a `uses`
caller is invalid. The runner schema above describes executable jobs, not the
GitHub service's reusable-call expansion; it does not establish caller syntax.

Cancellation is not an instantaneous physical process cutoff. The listener
[receives the server's cancellation message](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Listener/JobDispatcher.cs#L140-L154)
and [starts a separate worker cancellation window](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Listener/JobDispatcher.cs#L1269-L1286).
The worker [re-evaluates the running step's condition](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/StepsRunner.cs#L145-L185)
before canceling its token. The process invoker allows
[7.5 seconds for SIGINT and 2.5 seconds for SIGTERM](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Sdk/ProcessInvoker.cs#L32-L33),
then [kills the process tree if those signals did not finish the process](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Sdk/ProcessInvoker.cs#L443-L465).
The [official cancellation contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-cancellation)
also permits a five-minute cancellation grace before server-forced termination.
These mechanisms cannot substantiate a claim that all processes disappear
exactly 300 seconds after job start.

Native producer execution therefore needs its own monotonic deadline shared
across preparation, acquisition, probes, task execution, and output admission,
with process-group termination and bounded reaping reserved within that budget.
Starting a fresh timeout for each phase would permit cumulative overruns.
Expiration must prevent a success proof or publication handoff; GitHub's outer
job limit additionally bounds checkout, tool installation, and source fetch.

## Native preview after completed CI

The dedicated native preview renderer admits only the canonical CI completion
trigger, outside the generic trigger IR. GitHub documents that `workflow_run`
uses the default-branch commit/ref, can receive write tokens, and also fires for
unsuccessful upstream completions. Its name and branch filters do not establish
trusted source identity. See the [official event contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).

Pinned runner commit
[`9abfe11d2abd9de09d3a8d933d7ed701ea0518a3`](https://github.com/actions/runner/commit/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3)
exports the actual [event, ref, SHA, and workflow SHA](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/GitHubContext.cs#L14-L67)
and [writes the webhook payload](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Runner.Worker/ExecutionContext.cs#L1320-L1337).
The service's event selection and privilege semantics are documented contracts,
not behavior proven by this runner implementation.

Before producer execution, eligibility authenticates the canonical CI workflow,
successful same-repository push run/attempt, Required job and latest exact-A CI.
Source A may precede workflow authority B and protected main tip H only when
`A <= B <= H` is independently proven. Foreign, PR, failed or superseded CI
cannot authorize preview. No CI artifact selects A. Signer and publisher retain
isolated permissions and independent revalidation. Receipts bind source A;
certificates and standard SLSA workflow claims bind B's producing `workflow_run`.
See [provider A/B evidence](../implemented/immutable-preview-api-qualification.md#workflow-run-source-and-authority-observation).

The pinned [completed-event schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/workflow_run/completed.schema.json)
requires the event action, repository, workflow, and workflow-run envelope; the
[run schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/common/workflow-run.schema.json)
provides the workflow ID/path/name, repository and head repository, attempt,
source, and terminal status used by admission. The envelope's workflow identity
must agree with independently fetched canonical CI metadata.

## Native completion trigger scanner disposition

Pinned zizmor 1.31.1 (since [retired](../jactionlint-decision.md#zizmor-is-retired); jactionlint honours the annotation), original commit
`effdb35fa7868863f11655db256d9008922fa427`, emits `dangerous-triggers`
[unconditionally for `workflow_run`](https://github.com/zizmorcore/zizmor/blob/effdb35fa7868863f11655db256d9008922fa427/crates/zizmor/src/audit/dangerous_triggers.rs#L51-L98).
The rule includes a hidden location covering the entire `on` block. Its inline
annotation can therefore suppress a dangerous sibling trigger as well; placement
on the canonical CI scalar does not narrow that upstream suppression scope.
Qualification must cover sibling suppression and prove that unrelated
template-injection findings remain active.

Velnor accepts this finding only for its dedicated authenticated native completion
protocol. Production structural admission requires exactly the native completion
and manual recovery events, with canonical CI, completed activity and main branch
filters. Mutation tests add dangerous sibling events while preserving the
annotation and require rejection before output. The generic restricted-event
policy, full staged scanner invocation, and other audits remain unchanged. An
inline annotation alone is never sufficient release authority.

## Recursive MBX input matching qualification

Use the pinned runner v2.337.0 [hashFiles implementation](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Misc/layoutbin/hashFiles/index.js)
to qualify root and scoped patterns against deep Cargo members, custom member
paths, build scripts and source members named `target` below another directory.
Coverage must prove digest changes, actual workspace target exclusion and scoped
sibling isolation. Pattern compatibility does not establish hosted performance or
Cargo artifact reuse.

GitHub's [expression reference](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#hashfiles)
specifies recursive `**` patterns and ordered `!` exclusions. The patterns exclude
only the selected workspace's mutable target, never every directory named target.
The native Rust regression tests also bind these patterns to the rendered cache
keys and unchanged generation, job, toolchain and restore-prefix boundaries.

See [credential-free preview progress encoding](../implemented/native-preview-progress.md)
for the bounded diagnostic output protocol and its runner-source references.

See [native release asset upload](../implemented/native-release-upload.md) for
the qualified upload-host, authentication and binary-input source contract.
See [optional dependency-seed failure isolation](../implemented/optional-dependency-seed.md)
and [immutable preview API qualification](../implemented/immutable-preview-api-qualification.md).
See [explicit-token tool launch](../implemented/explicit-token-tool-launch.md).

## Pull Request Merge References

[GitHub's mergeability guidance](https://docs.github.com/en/rest/guides/using-the-rest-api-to-interact-with-your-git-database#checking-mergeability-of-pull-requests) defines asynchronous calculation of the test merge commit after a REST pull-request GET. GitHub requires polling that endpoint until `mergeable` is `true` or `false` before reading the merge reference. Two equal reference observations while `mergeable` is null do not establish freshness.

Velnor bounds this polling by attempts and the existing public-operation deadline. Each response must retain the original pull-request identity, repository identities, base and head, state, draft status, and merge status. After calculation finishes, Velnor retains the exact-reference checks, ordered base/head parent checks, and unchanged-reference recheck. The merge reference does not replace the independently verified pull-request context.

## Merge queue entry identity

Use GitHub's [MergeQueueEntry schema](https://docs.github.com/en/enterprise-cloud%40latest/graphql/reference/pulls#mergequeueentry) to authenticate the current queue entry. Keep its base and queue head distinct from the original pull-request head; do not infer queue topology or batching from one observed commit.

A cutover author check must bind the current queue entry's base and head to the authenticated merge-group context, then authenticate the associated pull request and its current author. A queue branch name alone is not author evidence. The pull-request test merge reference and an assumed two-parent queue commit are not substitutes for queue membership. Missing, stale, ambiguous or truncated membership must fail closed.

## Private Bun artifact admission

GitHub documents that authenticated users with repository read access can
download workflow artifacts. Artifact retention does not restrict that audience.
A public consuming repository therefore cannot receive a plaintext private-asset
handoff. Source: [Downloading workflow artifacts](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/download-workflow-artifacts).

The generated native admission step uses the current
`GET /repos/{owner}/{repo}` response rather than trigger-time event visibility.
It requires matching identity, Boolean private status and private visibility.
The existing bounded native GitHub transport supplies fixed-origin HTTPS,
workflow-token authentication and a deadline. Source:
[Get a repository](https://docs.github.com/en/rest/repos/repos?apiVersion=2026-03-10#get-a-repository).

## Local reusable workflow identity

The [workflow-run API](https://docs.github.com/en/rest/actions/workflow-runs#get-a-workflow-run) identifies reusable workflows through `referenced_workflows`, including their path and resolved `sha`; a SHA-pinned entry can omit `ref`. Bind the resolved workflow identity independently of the caller's source identity and do not require optional fields to establish a SHA-pinned reference.

A reusable signer's certificate URI can use a branch-qualified ref while its resolved workflow SHA is carried separately. The certificate also identifies the caller workflow and SHA, event, run ID and attempt. Field decoding establishes representation only: admission must verify the signed attestation, trusted issuer and exact workflow/source digests.

## Commit archive retrieval

GitHub's [repository tarball API](https://docs.github.com/en/rest/repos/contents#download-a-repository-archive-tar) responds with a redirect, which the pinned `gh` client follows. GitHub CLI's [redirect discussion](https://github.com/cli/cli/issues/8055) describes that behavior. Publication independently verifies selected archive bytes and executable modes against the authenticated commit tree; archive permission metadata and a successful download alone do not establish source identity. Limits are 64 MiB for the compressed response, 256 MiB for the expanded stream, 16 MiB per selected file and 128 MiB for selected content.

## Attestation history and bundle retrieval

GitHub's [repository attestation endpoint](https://docs.github.com/en/rest/repos/attestations#list-attestations) lists attestations for a `sha256:<digest>` subject and paginates using Link headers, with at most 100 entries per page. The response may include an inline bundle or a signed `bundle_url`; GitHub's [breaking changes](https://docs.github.com/en/enterprise-cloud@latest/rest/about-the-rest-api/breaking-changes) describe removing inline bundles. The pinned [GitHub CLI 2.102.0 client](https://github.com/cli/cli/blob/v2.102.0/pkg/cmd/attestation/api/client.go) fetches URL bundles without GitHub API credentials and decodes raw Snappy data. Its attestation lookup limit is finite and capped at 1,000, so an exact run cannot be selected reliably from a fixed prefix of subject history. The [verification command](https://cli.github.com/manual/gh_attestation_verify) accepts a JSONL bundle file containing multiple attestations.

Velnor enumerates the bounded history to completion, retrieves signed bundles without the GitHub token, verifies a bounded JSONL batch cryptographically, and then enforces the exact source, signer, run and attempt on the selected certificate. Oversized, invalid or incomplete evidence fails closed.

## Image publication inventory and registry errors

GitHub's REST endpoints list [workflow-run artifacts](https://docs.github.com/en/rest/actions/artifacts#list-workflow-run-artifacts), [jobs for a run attempt](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt), and [runs for a workflow](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-workflow) with `per_page`, `page`, and `total_count`. The workflow-run endpoint limits a filtered search to 1,000 results; Velnor's CI search uses the `branch`, `event`, and `head_sha` filters. A capped search is not evidence that an omitted run does not exist.

The image publisher requests 100 entries per page and admits at most 20 pages, 2,000 entries, and 16 MiB of cumulative serialized response JSON for an inventory. It requires the same `total_count` on every page, the expected number of entries per page, and unique nonzero IDs. Missing, inconsistent, duplicated, or over-budget evidence fails publication. Each API response is also bounded before JSON parsing.

The [OCI Distribution Specification error table](https://github.com/opencontainers/distribution-spec/blob/4f7750b2ee7ab20238c4e866072e2178d746560b/spec.md#error-codes) distinguishes `NAME_UNKNOWN`, an unknown repository, from `MANIFEST_UNKNOWN`, an unknown manifest. Both represent absence for the authenticated image tag lookup; `UNAUTHORIZED` and `DENIED` do not. The pinned [Crane 0.22.1 error formatter](https://github.com/google/go-containerregistry/blob/v0.22.1/pkg/v1/remote/transport/error.go#L43-L70) prints a single structured error's code and message without its HTTP status. Velnor therefore binds the exact requested reference and manifest URL, accepts only those two absence codes with a bounded single-line message, and rejects other or ambiguous diagnostics. This relies on the registry's OCI error-code semantics; the Crane output does not establish that the response had a particular HTTP status.

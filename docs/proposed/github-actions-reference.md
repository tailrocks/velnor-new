# GitHub Actions Official Reference & Compatibility Sources

## Purpose

This document establishes the authoritative sources of truth for GitHub Actions protocols, runner execution, scale set listeners, and runner environment images. For artifact upload semantics, see the [pinned action source reference](github-actions-artifact-reference.md). For installation token transport, see the [official token compatibility reference](github-installation-token-reference.md).

When designing workflows in the Velnor V1 generator or implementing runner infrastructure in `crates/velnor-runner`, developers and agents must **always check the original GitHub Actions source code for compatibility** and never guess protocol contracts or operational semantics.

---

### Branch-only image candidate trigger (checked 2026-11-05)

GitHub's [push filter syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpushbranchestagsbranches-ignoretags-ignore) says a branch-only filter does not run for tag pushes. Its [filter pattern reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#filter-pattern-cheat-sheet) defines `**` to include slash-separated names. The job-image candidate workflow uses `branches: ["**"]` to retain branch candidate runs without rebuilding or attesting on a tag push. Manual dispatch remains available; publication retains its separate main-branch dispatch gate.

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

The official [`xcode-27` runner image inventory](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md)
identifies an ARM64 macOS 27.0.1 host and lists Xcode 27.1 build `27A9275` at `/Applications/Xcode_27.1_Release_Candidate.app`. Velnor represents this
host as the typed `xcode-27-arm64` runner (`runs-on: xcode-27`) and binds consumer qualification to the exact Xcode version/build through `system_tools`.
The image's default Xcode is 27.0, so a task needing Xcode 27.1 pins `version = "27.1"`, `build = "27A9275"`, `sdk_version = "27.0"`, and
`developer_dir = "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer"`. Velnor probes and runs the named task with that `DEVELOPER_DIR` and never changes
the host's `xcode-select` setting.

---

## Compatibility Rules and Verification Procedures

### Ledger event and source identity (checked 2026-10-28)

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

### Native release job authority (checked 2026-10-21)

Jackin run `39495895585`, attempt 1, demonstrated executing producer jobs while the aggregate run remained `queued`; requiring only `in_progress` incorrectly rejected them before checkout. Aggregate status alone never establishes caller authority.
The official [attempt-specific jobs endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt) supplies the bounded complete inventory selecting one generated role's positive job ID. The [individual job endpoint](https://docs.github.com/en/rest/actions/workflow-jobs#get-a-job-for-a-workflow-run) refreshes its identity, status and conclusion using existing Actions read permissions. The runner's [GitHubContext export](https://github.com/actions/runner/blob/main/src/Runner.Worker/GitHubContext.cs) exports `GITHUB_JOB`; Velnor maps this machine identifier to the generated display name through a closed mapping checked against actual rendered jobs.
Native guards authenticate exact source, workflow, run and attempt; aggregate `queued` or `in_progress` requires no conclusion plus the unique expected job executing with no conclusion. Only immutable locators are cached; subsequent guards fetch the individual job again. Historical signed-run evidence remains separate from the current publisher, recovery or tap caller's authority.

### Step shell resolution and `addPath` (checked 2026-10-22)

[`ScriptHandler.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Worker/Handlers/ScriptHandler.cs) resolves a step's shell with `WhichUtil.Which(command, …, prependPath)`, where `prependPath` holds every `core.addPath`/`GITHUB_PATH` entry ahead of the job `PATH`. In a composite action (`IsActionStep`) even `shell: bash` takes this route. [`WhichUtil.cs`](https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Sdk/Util/WhichUtil.cs) returns a fully qualified command as given when the file exists and never searches `PATH` for it; a custom shell string is split at its first space and must contain `{0}`. `jdx/mise-action` adds the Mise `bin/` and, by default, `shims/` with `core.addPath`, so after a tools-cache restore a bare shell or helper name can resolve to restored bytes. Velnor's restore checks therefore use `/bin/bash --noprofile --norc -p -e -o pipefail {0}` and reset `PATH` to `/usr/bin:/bin:/usr/sbin:/sbin` first ([hermetic checks](tools-cache-v2.md#the-checks-run-hermetically)).

### Workflow YAML sharing (checked 2026-10-19)

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
runner's [legacy `YamlObjectReader.EvaluateCurrent`](https://github.com/actions/runner/blob/main/src/Sdk/DTPipelines/Pipelines/ObjectTemplating/YamlObjectReader.cs)
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

## 2026 GitHub Actions & Dependabot Platform Contracts

### 1. Native Step Concurrency (`background`, `wait`, `wait-all`, `cancel`, `parallel`) — `2026-06-25`

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
  - Velnor's `Trigger` IR is structurally disjoint from `RESTRICTED_ACTION_EVENTS`: generic generated workflows never emit `pull_request_target`, `workflow_run`, or comment/issue triggers, ensuring public-repository forks execute under `pull_request` with read-only permissions and no secrets without requiring manual maintainer approval for restricted triggers.

### 3. Cache Access Control (`cache-mode`) — `2026-09-10`

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode)):
  - Workflows and jobs can declare `cache-mode` at the workflow root or per job (`jobs.<job_id>.cache-mode`) to enforce runtime-level GitHub Actions cache service permissions:
    - `write`: read and write cache entries (default on standard events).
    - `read`: read-only cache access; cache writes are skipped/ignored without failing the job.
    - `write-only`: write cache entries without restoring existing entries.
    - `none`: disable all cache reads and writes.
  - Whether `cache-mode` accepts `${{ }}` expressions is UNCONFIRMED: the workflow-syntax and dependency-caching pages list only the four literals. Details and the enforcement and default rules are in entry 5 below.
- **Velnor Implementation**:
  - `velnor-actions-contract` defines `CacheMode` (`Read`, `Write`, `WriteOnly`, `None`, `Expression(String)`) and optional `cache_mode: Option<CacheMode>` on both `WorkflowIr` and `Job`, with `CacheMode::for_pull_request_policy(PullRequestCachePolicy)` mapping `PullRequestCachePolicy::ReadOnly` to `CacheMode::Expression("${{ github.event_name == 'pull_request' && 'read' || 'write' }}")` and `PullRequestCachePolicy::SharedSeed` to `CacheMode::Write`.
  - `velnor-actions-workflow-renderer` emits top-level and job-level `cache-mode:` when `cache_mode` is `Some(...)` and omits it when `None`.
  - Pinned `jactionlint` v2.3.0 parses workflow- and job-level `cache-mode` literals (`read`, `write`, `write-only`, `none`) and rejects any other value, including a `${{ }}` expression; `JactionlintCapabilities::check_step_syntax(StepSyntax::CacheMode)` records the capability and generation vets it.

### 4. Dependabot Default Package Cooldown & Velnor Freshness — `2026-07-14`

- **Upstream Contract** ([Changelog](https://github.blog/changelog/2026-07-14-supply-chain-defense-by-default-dependabot-adds-3-day-package-cooldown/)):
  - Dependabot applies a **3-day default cooldown** (`cooldown.default-days: 3`, configurable from `1` to `90` days) to newly published package versions for version updates, reducing exposure to compromised releases during the initial publication window while keeping **security updates immediate**.
- **Velnor Implementation**:
  - `velnor-actions-freshness` supports an optional `default_cooldown_days` integer header key in `.velnor/version-policy.toml` (`DEFAULT_COOLDOWN_DAYS = 3`, bounded `MIN_COOLDOWN_DAYS = 1` to `MAX_COOLDOWN_DAYS = 90`).
  - When `default_cooldown_days` is omitted from `.velnor/version-policy.toml`, the freshness policy header check defaults to `3` days; values outside `1..=90` or non-integers fail the `policy-header` gate.
  - Security advisory scanning via `cargo-deny` remains immediate and is never delayed by `default_cooldown_days`.

#### 5.5 Cache rules verified against original sources (checked 2026-11-04)

Pinned action: `actions/cache` `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` (tag `v6`, `package.json` 6.1.0, line 3), which bundles `@actions/cache` 6.1.0
(`package-lock.json:41-42`). Toolkit source read at `actions/toolkit@0786132e6a1a4451c2d392bbbf481c2b172d4312` (commit "Bump cache package to v6.1.0"),
paths `packages/cache/src/...`. Docs read from `github/docs@main` (raw Markdown, fetched 2026-11-04):
`content/actions/reference/workflows-and-actions/dependency-caching.md` (call it DC), `.../workflow-syntax.md` (WS), `content/actions/reference/limits.md` (LIM).
Line numbers for DC, WS, LIM are into those files as fetched; they drift. Cited docs pages:
[dependency caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[limits](https://docs.github.com/en/actions/reference/limits), [REST cache API](https://docs.github.com/en/rest/actions/cache).

Verdict key: TRUE, FALSE, PARTIAL (true with a stated correction or gap).

#### 5.1 Size limit, eviction, 7-day expiry: PARTIAL (the 10 GB figure is a default, not a hard limit)

- DC:356 "GitHub will remove any cache entries that have not been accessed in over 7 days. There is no limit on the number of caches you can store, but the total size of all caches in a repository is limited. By default, the limit is 10 GB per repository, but this limit can be increased by enterprise owners, organization owners, or repository administrators. Any usage beyond 10 GB is billed to your account."
- Eviction: reusable `cache-eviction-policy.md` "the cache eviction policy will create space by deleting the caches in order of last access date, from oldest to most recent." and "If you exceed the limit, GitHub will save the new cache but will begin evicting caches until the total size is less than the repository limit." (`github/docs@main:data/reusables/actions/cache-eviction-policy.md`, `cache-eviction-process.md`). So LRU by last access, and the over-limit save still succeeds first.
- 2025-2026 change: [changelog 2025-11-20](https://github.blog/changelog/2025-11-20-github-actions-cache-size-can-now-exceed-10-gb-per-repository/) (summarised by fetch, not verbatim): admins can raise the size limit past 10 GB, pay as you go; every repository keeps 10 GB free; defaults are 10 GB and 7 days; a retention limit (days) is now configurable and cascades enterprise to organization to repository.
- DC:367 "Repositories owned by users can configure up to 10 TB per repository. ... Increasing the limit beyond the default 10 GB will incur additional costs, if that storage is used."
- DC:374 "If you have limits configured, and you exceed a budget, your cache will become read-only until your billing status is resolved, or your usage goes beneath the free limit of 10GB". So a Velnor design must not assume 10 GB is fixed, and a repository above 10 GB can be read-only under a budget.
- REST has repository cache storage-limit and retention-limit get/set endpoints (page lists "Gets GitHub Actions cache storage limit for a repository" and "Sets ... retention limit"). Exact endpoint paths not recorded; see Not verified.
- The 7-day rule is the default retention, not a constant: a configured retention limit changes it.

#### 5.2 Key immutability: TRUE (client behavior inferred for the refusal path)

- DC:34 "You cannot change the contents of an existing cache. Instead, you can create a new cache with a new key."
- `actions/cache@55cc834:tips-and-workarounds.md:11` "A cache today is immutable and cannot be updated."
- Client: `toolkit@0786132e:packages/cache/src/cache.ts:513-515` (v1) and `:649-651` / `cache.ts` v2 `CreateCacheEntry` failure: `Unable to reserve cache with key ${key}, another job may be creating this cache.` The save is logged at info level (`cache.ts:529-531`, `core.info('Failed to save: ...')`) and the step succeeds. The server decision (reserve refused because key+version+scope exists) is server-side and not in client source; the client treats any non-ok reservation as a refused save.
- `actions/cache@55cc834:src/saveImpl.ts:50-54` skips saving when the restore step reported an exact primary-key hit: "Cache hit occurred on the primary key ${primaryKey}, not saving cache."
- Uniqueness is key plus version plus scope (`cacheUtils.ts:136-158` version hash of paths, compression, `windows-only`, salt; docs DC:170 scope). The same key can exist once per ref and per version.

#### 5.3 Restore order: PARTIAL (the stated order omits key-prefix matching and the branch-outer loop)

- DC:28-30 "First, it searches for an exact match to your provided `key`. If no exact match is found, it will search for partial matches of the `key`. If there is still no match found, and you've provided `restore-keys`, these keys will be checked sequentially for partial matches."
- DC:174 "The `cache` action searches the `restore-keys` in sequential order. When a key doesn't match directly, the action searches for keys prefixed with the restore key. If there are multiple partial matches for a restore key, the action returns the most recently created cache." Newest means most recently created, not most recently accessed.
- DC:170 "searches for cache hits for `key` and the cache version in the branch containing the workflow run ... If there are still no hits in the current branch, the `cache` action retries the same steps on the default branch." DC example "Example of search priority" lists all of key, prefix 1, prefix 2 on the feature branch before the same list on `main`.
- Client passes `key` and `restoreKeys` unchanged to the server: `cache.ts:312-320` (`GetCacheEntryDownloadURLRequest {key, restoreKeys, version}`); proto comment `generated/results/api/v1/cache.ts:127` "Restore keys used for prefix searching". Ordering logic is server-side, so only the docs prove it.
- Limits (client): at most 10 keys in total (`cache.ts:181-185`, `:293-297`), key length at most 512 and no commas (`cache.ts:81-93`). A hit needs the same `version` (paths plus compression method), so a Gzip entry never serves a zstd restore and the reverse.
- `cache-hit` is true only for an exact `key` match (`action.yml` output description "an exact match was found for the primary key").

#### 5.4 Scope: PARTIAL (each clause below; the last point is inferred, not documented)

- DC:243 "Workflow runs can restore caches created in either the current branch or the default branch (usually `main`). If a workflow run is triggered for a pull request, it can also restore caches created in the base branch, including base branches of forked repositories."
- DC:245 "Workflow runs cannot restore caches created for child branches or sibling branches." Tags are likewise isolated from other tags.
- DC:247 "When a cache is created by a workflow run triggered on a pull request, the cache is created for the merge ref (`refs/pull/.../merge`). Because of this, the cache will have a limited scope and can only be restored by re-runs of the pull request. It cannot be restored by the base branch or other pull requests targeting that base branch." So "main cannot read it" is TRUE.
- DC:279 "Caches created by a `pull_request` run are already scoped to the merge ref (`refs/pull/.../merge`) and cannot be written to the default branch's scope."
- `actions/cache@55cc834:README.md:113` "The cache is scoped to the key, version, and branch. The default branch cache is available to other branches."
- A later push to the same PR: NOT stated by any official source. The docs say only "re-runs of the pull request". Evidence for restore: scope is the ref name (`cacheHttpClient.ts:134` prints `scope ${process.env['GITHUB_REF']}`; REST docs: "To reference a pull request use refs/pull/<number>/merge"), and the ref name `refs/pull/N/merge` does not change between pushes while only the merge SHA does. By that mechanism a later push can restore an earlier push's entry. Verdict: INFERRED TRUE, not proven. Live check on `tailrocks/velnor-new`: one entry `refs/pull/414/merge` created 2026-10-27T17:37:07Z, last accessed 17:38:58Z (`gh api repos/tailrocks/velnor-new/actions/caches`), too close to separate a same-run read from a later-push read. Settling it needs two pushes to one PR with a restore log; no such run was made for this entry.
- Fork PR runs also use `refs/pull/N/merge` and can read the base and default branch scope; DC:69 warns that anyone who can open a PR can read base-branch caches.

#### 5.5 `cache-mode`: PARTIAL (values and precedence TRUE; the pinned client does not read the variable)

- DC:289-300 "Use the `cache-mode` workflow key to grant jobs the least amount of cache access they need. You can set `cache-mode` at the workflow level, at the job level, or both. A job-level value overrides the workflow-level value for that job." WS:521 "A value set here overrides any workflow-level `cache-mode` for this job only."
- Values (DC:293-298, WS:415-420): `read` restore yes, save no; `write` yes, yes; `write-only` restore no, save yes; `none` no, no. WS:523 "The accepted values are `read`, `write`, `write-only`, and `none`".
- Default when omitted (DC:306-307): trusted trigger `write`, low-trust trigger `read`. Trusted triggers (DC:268-275): `push`, `workflow_dispatch`, `repository_dispatch`, `delete`, `registry_package`, `page_build`, `schedule`. `pull_request` is "not affected" (DC:279) and is not placed in either column; its effective default is not stated (a PR run does write to its merge-ref scope today).
- Enforcement is server-side: WS:413 "Access is enforced with scoped cache tokens, so a job cannot restore or save caches beyond the mode it is granted." DC:315 "The runner exposes the effective mode in the `ACTIONS_CACHE_MODE` environment variable, and the `actions/cache` action and the `@actions/cache` toolkit honor it."
- Pinned-version gap: `ACTIONS_CACHE_MODE` handling arrived in `@actions/cache` 6.2.0 (`toolkit@main:packages/cache/RELEASES.md` "6.2.0 - Honor the `ACTIONS_CACHE_MODE` environment variable"). Pinned 6.1.0 has only the write-denied handling: `cache.ts:42` `CACHE_WRITE_DENIED_PREFIX = 'cache write denied:'`, `:502-512` surfaced as `core.warning` (`:74`). `grep -c ACTIONS_CACHE_MODE` on `dist/save/index.js` and `dist/restore/index.js` at the pinned SHA returns 0. Under `read`, a save at the pinned SHA still attempts a reserve, is denied by the service, and logs a warning, not an informational skip. Under `write-only` or `none`, restore handling at 6.1.0 is a service-side read denial, surfaced by 6.2.0+ code as a "cache read denied:" warning; at 6.1.0 it falls into the generic `Failed to restore:` warning path (`cache.ts` catch at `:236-251`).
- Reusable workflows (DC:318-324): a caller's explicit mode caps the called workflow; over-request fails validation; with no explicit caller mode the callee can request `write`.
- A write-capable mode on a low-trust trigger overrides the read-only default (DC "Bypassing the default untrusted-trigger cache restriction") and reintroduces poisoning risk.
- Expressions: no official page read documents `${{ }}` in `cache-mode`. WS and DC list only the four literals. Expressions in `cache-mode` are UNCONFIRMED, and the pinned `jactionlint` rejects them (see entry 3). The earlier changelog URL returned HTTP 404 on 2026-11-04; the live page is the [cache-mode changelog](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode) (found by search, content not fetched).

#### 5.6 Compression: PARTIAL (zstd default; the `--long=30` flag is NOT used for saves in the pinned version)

- `cacheUtils.ts:102-112` `getCompressionMethod`: runs `zstd --quiet` for the version; empty output returns `CompressionMethod.Gzip`, otherwise returns `CompressionMethod.ZstdWithoutLong`. It never returns `CompressionMethod.Zstd` (the `--long` variant), though the enum and code paths exist (`constants.ts:6-12`). Same logic in `actions/cache@55cc834:dist/save/index.js:43381-43392`.
- Create command (`tar.ts:225-241`): non-Windows `--use-compress-program 'zstdmt'` for `ZstdWithoutLong`; Windows `"zstd -T0"`; with the (unused) `Zstd` variant `zstdmt --long=30`. Comment at `tar.ts:208-212` "zstdmt is equivalent to 'zstd -T0' ... --long=# ... Maximum is 30 (1GB) on 32-bit OS and 31 (2GB) on 64-bit. Using 30 here because we also support 32-bit self-hosted runners."
- Level: no `-#` flag is passed anywhere in `tar.ts`, so zstd's own default level applies (3 per zstd's manual; not re-fetched, see Not verified). Threads: `-T0`.
- Decompress (`tar.ts:175-203`): `unzstd` (non-Windows, `ZstdWithoutLong`) or `unzstd --long=30` for the `Zstd` variant. A reader must handle both; a `--long=30` entry needs window up to 1 GB.
- Fallback when zstd is absent: gzip via tar `-z`, file `cache.tgz` (`tar.ts:202-204`, `:242-244`; `constants.ts:1-4`). Archive names: `cache.tzst` or `cache.tgz`. Compression method is hashed into the cache version, so the two do not cross-restore.
- Tar create flags (`tar.ts:73-88`): `--posix -cf <cache.tzst> --exclude <cache.tzst> -P -C <GITHUB_WORKSPACE|cwd> --files-from manifest.txt`; darwin adds `--delay-directory-restore`, Windows GNU tar `--force-local`. Archive entries therefore keep the path forms given in the manifest, including absolute paths.

#### 5.7 Extraction: TRUE for "extracted in the restore step, no inspection point"; no size limit found

- Extract command (`tar.ts:90-99`): `tar -xf <archive> -P -C <workingDirectory>` plus the decompress program (`--use-compress-program unzstd`) and the platform extras (`--delay-directory-restore` on darwin). `-P` (`--absolute-names`) keeps absolute names and leading `/`, so an entry can write outside the workspace; `-C` is `GITHUB_WORKSPACE` (`tar.ts:165`). No `--no-same-owner`, `--no-overwrite-dir`, `--keep-old-files`, or `--exclude` is passed, so existing files are overwritten.
- Order in `restoreCacheV2` (`cache.ts:344-364` of the 6.1.0 source): download to a temp dir, then `extractTar(archivePath, compressionMethod)` inside the same action step, then `core.info('Cache restored successfully')`. `listTar` runs only when `core.isDebug()` and only after download, before extract; it prints names, it does not filter. The restore step is a single process: no user step runs between download and extraction. Inspection is only possible after extraction, by a later user step.
- Size during extraction: `grep` of `tar.ts`, `cache.ts`, `downloadUtils.ts` finds no check of uncompressed size, entry count, or free space. The only size figures are the compressed archive size log line (`cache.ts:225-230`) and a download completeness check (`downloadUtils.ts:187-196`, "Incomplete download. Expected file size: ..."). A zstd bomb is bounded only by the runner disk.
- Cache contents are not signed (DC "Best practices": "Cache contents are not signed or verified, and any workflow run that can read a cache may extract its contents. Extracted caches may modify files that are subsequently executed in a workflow run").
- Failure handling: any restore error, including a `tar` failure, is a warning (or `core.error` for HTTP 5xx) and the step returns a miss (`cache.ts:236-251`). The partial extraction is not rolled back.

#### 5.8 Per-entry size and rate limits: PARTIAL (a 10 GiB per-archive check exists only in the legacy v1 client path; rates TRUE)

- Per-archive limit: `cache.ts:467` (inside `saveCacheV1`, which starts at `:435`; `saveCacheV2` at `:564` has no such check) `const fileSizeLimit = 10 * 1024 * 1024 * 1024 // 10GB per repo limit` and `:472-477` "Cache size of ~N MB (B B) is over the 10GB limit, not saving cache." skipped when `isGhes()`. `constants.ts:40` `CacheFileSizeLimit = 10 * Math.pow(1024, 3) // 10GiB per repository`. This is a 10 GiB cap on one compressed archive, only on the v1 service path (the v1 client is chosen by `getCacheServiceVersion()`). The v2 save path has no client check; any per-entry limit in the v2 service is not documented, and the docs state only the per-repository limit. A server 400 on reserve is reported as "is over the data cap limit, not saving cache" (`cache.ts:496-498`).
- Rates: LIM:52-54 "Each repository is limited to 200 cache entry uploads per minute. If this limit is exceeded, subsequent cache upload attempts will fail until the rate limit resets." "Each repository is limited to 1500 cache entry downloads per minute." "Each repository is limited to 400 cache delete operations per minute." Same figures at DC:363. Only new-entry uploads count, not job-run downloads for upload ([changelog 2026-01-16](https://github.blog/changelog/2026-01-16-rate-limiting-for-actions-cache-entries/), page shows "Retired"; fetched summary).
- Client on 429: `internal/shared/cacheTwirpClient.ts:115-127` reads `retry-after`, logs "You've hit a rate limit, your rate limit will reset in N seconds", and throws `RateLimitError` without retry. Retryable statuses are 502/503/504 (`requestUtils.ts:24-34`); defaults 2 retries, 5000 ms (`constants.ts:20-23`). A failed save is a warning, so a 429 never fails the job.
- Key limits: 512 characters, no commas, at most 10 keys per restore (`cache.ts:81-93`, `:181-185`).

#### 5.9 Which runs can save: PARTIAL (the answer depends on the scope the run writes to)

- Default-branch scope (DC:268-277): only `push`, `workflow_dispatch`, `repository_dispatch`, `delete`, `registry_package`, `page_build`, `schedule` can create or overwrite. "Runs triggered by any other event that resolves to the default branch are given read-only access ... such as `pull_request_target`, `issue_comment`, and `workflow_run`." So `workflow_run` saving to the default-branch scope: FALSE by default; TRUE only with an explicit write-capable `cache-mode` (DC "Bypassing" section).
- `schedule` and `workflow_dispatch`: TRUE (listed as trusted). A `workflow_dispatch` on a non-default ref writes to that ref's scope.
- `pull_request`: writes to `refs/pull/N/merge` only (DC:247, :279).
- Fork `pull_request`: `actions/cache@55cc834:README.md:119-121` "A common case is a workflow triggered by a pull request from a fork: such runs can restore existing caches but may not be permitted to save new ones." and "the save step does not fail the job ... reports the denial once as a warning". No official page states the fork-PR rule as always-denied or always-allowed; do not assume either. Treat fork-PR saves as unproven.
- `merge_group`: not covered by the docs read. Not in the trusted list; its ref is `refs/heads/gh-readonly-queue/...`, so scope would be that ref (inferred). Velnor's cache-decisions row 5 already assumes merge groups never write.
- Client gate: `actions/cache@55cc834:src/utils/actionUtils.ts:35-37` `isValidEvent()` is only `GITHUB_REF` being set, so the action itself imposes no event limit; denial comes from the service token.

#### Version notes

- Pinned `@actions/cache` 6.1.0; latest at `toolkit@main` is 6.3.0 (`packages/cache/RELEASES.md`): 6.2.0 adds `ACTIONS_CACHE_MODE` honoring and a "cache read denied:" warning; 6.3.0 isolates Windows BSD tar scratch files. Neither changes tar flags or compression selection in the files read.
- `actions/cache` v6 default `post-if: success()` and `save-always` deprecated (`action.yml`), unrelated to the rules above.

#### Not verified

- Server-side matching order and "newest wins" were verified from docs only; the service source is not public.
- Zstd default level 3 comes from zstd's manual, not re-fetched here.
- Exact REST paths for repository cache storage and retention limits; DC and the REST page confirm they exist.
- Whether the service rejects an entry over 10 GiB on a repository with a raised limit.
- Later push to the same PR restoring an earlier push's entry: inferred from the ref-keyed scope, no direct experiment (no run was made).
- Fork `pull_request` save behavior, `merge_group` save behavior, and `pull_request` default `cache-mode` value are not documented.
- The 2025-11-20 and 2026-01-16 changelogs were read through a summarizing fetch, not verbatim; the 2026-09-10 changelog was not read (only its search snippet).
- Did not run live a restore on the pinned SHA to see the `read`-mode warning text.

## Native preview publication API boundaries (2026-10-18)

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

Release responses do not include `make_latest` (confirmed against the official
Velnor v0.1.12 API response). It is a request-only string enum. `gh api --field`
converts literal booleans and expands `@file` values, so native release requests
use `--raw-field` for string fields and `--field` only for draft/prerelease
booleans. Both channels publish with `make_latest: false`; preview releases are
also prereleases, while stable releases remain ordinary releases without being
forced to latest. The release response has no `make_latest` field, so Velnor
verifies the returned draft/prerelease/immutable state and exact assets without
assuming that a newly published stable release becomes latest.

### Review-completion provider qualification (2026-10-18)

The [official Codex GitHub review guide](https://learn.chatgpt.com/docs/third-party/github)
documents requested and automatic reviews, but does not establish a supported
machine-state completion protocol. Native merge admission therefore continues
to require the separately authenticated `velnor.review-completion.v1` check;
reactions, table prose, and an absent finding are not substitutes.

Observed Codex App `1144995` records do not yet qualify an adapter. Jackin tap
PR 508 reports failed review because of usage limits. Public OpenAI repository
records sampled during this investigation carried running or incomplete state.
The [App-authored unsloth summary](https://github.com/unslothai/unsloth/pull/10502#issuecomment-5584670251)
contains a completed security marker, but its merge gate is disabled and it
does not bind complete code and security finding dispositions to one full head.
These are observed implementation bytes, not a supported provider API.

GitHub's [IssueComment GraphQL fields](https://docs.github.com/en/graphql/reference/issues#issuecomment)
expose the latest editor and edit time. Any future adapter must authenticate
the actual numeric App, author, and latest editor, independently fetch the PR's
full current head, parse closed machine fields, and revalidate all current
feedback. Positive exact-head completion and forged/edited-state rejection
remain deployment gates. Unattended merging stays disabled until that proof
exists; this research does not authorize a guessed success enum.

The [required-review documentation](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews)
distinguishes a comment from approval: a blocking change request requires the
reviewer's subsequent approval or an actual review dismissal. A later comment
must not clear that request. Native feedback aggregation must preserve this
distinction independently of response order, and must reject unknown states.
GitHub also exposes [current per-user review connections](https://docs.github.com/en/graphql/reference/pulls#pullrequest);
any server-side opinion query must retain complete pagination and bind the same
PR/head as the full feedback snapshot.

Live GraphQL qualification confirmed the distinction. On `rust-lang/rust`
PR 164353, the opinionated connection retained an approval and omitted a later
comment review. On [godotengine/godot PR 118057](https://github.com/godotengine/godot/pull/118057),
the complete reviews connection retained a dismissed review while the opinionated
connection was empty. On [github-linguist/linguist PR 7763](https://github.com/github-linguist/linguist/pull/7763),
the opinionated connection retained only the latest active change request and
excluded a different reviewer's dismissed request. Native capture uses this
paginated server-derived state alongside all underlying feedback records.
Unexpected opinionated states still fail closed.

### Historical pull-request response boundaries

Live tap API responses confirm that the pull-request listing omits the `merged`
boolean. Native completed reconciliation must fetch each matching PR's detail
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
`Bot` in `latestOpinionatedReviews`. Live GitHub GraphQL introspection on
2026-10-18 confirmed `Bot.databaseId`; selecting only the `User` fragment would
make a bot's opinionated review unidentifiable and block otherwise valid review
history. A scripted protocol test exercises a bot's outstanding change request.

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

## Job budgets and cancellation (2026-10-18, range widened 2026-10-19)

Compatibility was checked against original `actions/runner` commit
[`9abfe11d2abd9de09d3a8d933d7ed701ea0518a3`](https://github.com/actions/runner/commit/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3).
The runner's [executable-job schema](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Sdk/DTPipelines/workflow-v1.0.json#L55-L75)
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

## Native preview after completed CI (2026-10-18)

The dedicated native preview renderer admits only the canonical CI completion
trigger, outside the generic trigger IR. GitHub documents that `workflow_run`
uses the default-branch commit/ref, can receive write tokens, and also fires for
unsuccessful upstream completions. Its name and branch filters do not establish
trusted source identity. See the [official event contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).

Original runner commit
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

The payload shape was checked against original `octokit/webhooks` commit
`aa80c24a3f55354465b8b1438d33dd5602956d0d`: the
[completed-event schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/workflow_run/completed.schema.json)
requires the event action, repository, workflow, and workflow-run envelope; the
[run schema](https://github.com/octokit/webhooks/blob/aa80c24a3f55354465b8b1438d33dd5602956d0d/payload-schemas/api.github.com/common/workflow-run.schema.json)
provides the workflow ID/path/name, repository and head repository, attempt,
source, and terminal status used by admission. The envelope's workflow identity
must agree with independently fetched canonical CI metadata.

## Native completion trigger scanner disposition (2026-10-18)

Pinned zizmor 1.31.1 (since [retired](../jactionlint-decision.md#zizmor-is-retired); jactionlint honours the annotation), original commit
`effdb35fa7868863f11655db256d9008922fa427`, emits `dangerous-triggers`
[unconditionally for `workflow_run`](https://github.com/zizmorcore/zizmor/blob/effdb35fa7868863f11655db256d9008922fa427/crates/zizmor/src/audit/dangerous_triggers.rs#L51-L98).
The rule includes a hidden location covering the entire `on` block. Its inline
annotation can therefore suppress a dangerous sibling trigger as well; placement
on the canonical CI scalar does not narrow that upstream suppression scope.
Independent pinned-binary controls confirmed that sibling behavior and verified
that unrelated template-injection findings remain active.

Velnor accepts this finding only for its dedicated authenticated native completion
protocol. Production structural admission requires exactly the native completion
and manual recovery events, with canonical CI, completed activity and main branch
filters. Mutation tests add dangerous sibling events while preserving the
annotation and require rejection before output. The generic restricted-event
policy, full staged scanner invocation, and other audits remain unchanged. An
inline annotation alone is never sufficient release authority.

## Recursive MBX input matching qualification (2026-10-19)

The runner's original [hashFiles implementation](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Misc/layoutbin/hashFiles/index.js)
from v2.337.0 was fetched by exact commit and checked against Git blob
`8313c78812fa4c034608950617523d6769b3f87e` and SHA-256
`fbcad57b299a699a29f1c750949073d2b9c7ad4b3b742ca3355947f99fcf7e56`.
Its unmodified bundled matching engine ran offline on Node 26.11.1 with a clean
environment and temporary workspace. The root and scoped pattern sets cover deep
Cargo members, custom member paths, build scripts and source members named
`target` below another directory. Seventeen independent mutations check digest
changes, actual workspace target exclusion and scoped sibling isolation. This
qualifies pattern behavior, not hosted performance or Cargo artifact reuse.

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

Checked against [GitHub's mergeability guidance](https://docs.github.com/en/rest/guides/using-the-rest-api-to-interact-with-your-git-database#checking-mergeability-of-pull-requests) on 2026-10-26 UTC.

A REST pull-request GET starts asynchronous calculation of the test merge commit. GitHub requires polling that endpoint until `mergeable` is `true` or `false` before reading the merge reference. Two equal reference observations while `mergeable` is null do not establish freshness.

Velnor bounds this polling by attempts and the existing public-operation deadline. Each response must retain the original pull-request identity, repository identities, base and head, state, draft status, and merge status. After calculation finishes, Velnor retains the exact-reference checks, ordered base/head parent checks, and unchanged-reference recheck. The merge reference does not replace the independently verified pull-request context.

## Merge queue entry identity

Checked against GitHub's [MergeQueueEntry schema](https://docs.github.com/en/enterprise-cloud%40latest/graphql/reference/pulls#mergequeueentry) and a live public queue response on 2026-10-27 UTC.

The captured entry for [DataFusion pull request 26770](https://github.com/apache/datafusion/pull/26770) identified base commit `c18e5d974c49382ba892fa9f89ce71d3aa0ab8ae`, queue head commit `97ff724f2c20f906277dc29fa8ba66a5824538a7`, and original pull-request head `a8448250c3fea792fcbe17130de5a6eed35aef9b`. GitHub's merge-group workflow runs used the queue head, not the original pull-request head. The queue commit had one parent, the base commit. These observations cover one queue entry; they do not establish batching behavior.

A cutover author check must bind the current queue entry's base and head to the authenticated merge-group context, then authenticate the associated pull request and its current author. A queue branch name alone is not author evidence. The pull-request test merge reference and an assumed two-parent queue commit are not substitutes for queue membership. Missing, stale, ambiguous or truncated membership must fail closed.

## Private Bun artifact admission (2026-10-27)

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

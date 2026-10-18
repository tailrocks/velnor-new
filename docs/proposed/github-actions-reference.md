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

The official [`xcode-27` runner image inventory](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md)
identifies an ARM64 macOS 27.0.1 host and lists Xcode 27.1 build `27A9275`
at `/Applications/Xcode_27.1_Release_Candidate.app`. Velnor represents this
host as the typed `xcode-27-arm64` runner (`runs-on: xcode-27`) and binds
consumer qualification to the exact Xcode version/build through `system_tools`.
The image's default Xcode is 27.0, so a task needing Xcode 27.1 pins
`version = "27.1"`, `build = "27A9275"`, `sdk_version = "27.0"`, and
`developer_dir = "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer"`.
Velnor probes and runs the named task with that `DEVELOPER_DIR` and never changes
the host's `xcode-select` setting.

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

## Five-minute job budgets and cancellation (2026-10-18)

Compatibility was checked against original `actions/runner` commit
[`9abfe11d2abd9de09d3a8d933d7ed701ea0518a3`](https://github.com/actions/runner/commit/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3).
The runner's [executable-job schema](https://github.com/actions/runner/blob/9abfe11d2abd9de09d3a8d933d7ed701ea0518a3/src/Sdk/DTPipelines/workflow-v1.0.json#L55-L75)
accepts `timeout-minutes` on a job with `runs-on`. GitHub's
[job-timeout contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes)
says expiration automatically cancels that job; its default is 360 minutes.
Velnor must explicitly emit a timeout between one and five minutes on every
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

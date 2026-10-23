# Standalone Mise task-suite contract

**Status:** Proposed specification. The generated workflow behavior is being
implemented behind `[[workflow.task_suites]]`; repository owners must review
the generated files and their task bodies.

This contract defines bounded, compile-capable Mise tasks that run in their own
generated companion workflows. It does not change `[[workflow.tasks]]`, the
primary `ci.yml` workflow, or the `Required` fan-in. A task suite is
repository-controlled code and receives no repository task credential.

## 1. Configuration

Each entry MUST provide a sorted, unique lowercase `id`, exact `mise_task`,
`source`, fixed `runner`, bounded `timeout_minutes`, generated workflow slug,
and a non-empty `events` list. `source.mise_config` names a repository-local
`mise.toml`; `source.working_directory` MUST be that config directory or a
descendant. The task name, path, runner, timeout, and slug are validated
before generation. Unknown fields are rejected.

```toml
[[workflow.task_suites]]
id = "nightly-evidence"
mise_task = "evidence:collect"
runner = "linux-x64"
timeout_minutes = 30
workflow = "evidence"
events = ["schedule"]
schedule = ["0 3 * * *"]

[workflow.task_suites.source]
mise_config = "mise.toml"
working_directory = "."

[[workflow.task_suites.artifacts]]
name = "evidence"
paths = ["target/evidence/manifest.json"]
when = "success"
retention_days = 45
```

The supported runner values are `linux-x64`, `macos-arm64`, `macos-26-arm64`,
and `xcode-27-arm64`. The task MAY compile, unlike a `VerificationTask`.
Compilation uses fixed `CARGO_BUILD_JOBS=2` and
`NEXTEST_TEST_THREADS=2` bounds; callers cannot raise those limits. The
declared task must use its locked Mise tool closure. Tool selection rejects
undeclared or unlocked tools, source installs, unsupported backends, and
invalid Rust toolchain sources before emitting a workflow. No Nextest profile
is required.

## 2. Events and placement

V1 accepts only `push` and `schedule`. A push workflow is restricted to the
configured default branch. A scheduled suite MUST list one or more validated
cron expressions; `schedule` without a cron, or a cron without the `schedule`
event, is invalid. A generated workflow may contain suites with different
event sets only when each job checks its own exact event and schedule. Its
top-level triggers are the union of those declarations.

Generated companion workflows MUST NOT trigger on pull requests,
`merge_group`, or `workflow_dispatch`. Their jobs MUST NOT join primary CI or
`Required`. No concurrency group is generated: GitHub replaces an older
pending run when a concurrency group receives another run, even when
`cancel-in-progress` is false. Omitting the group keeps each generated push
run independently observable.

Every job checks out the event commit by `github.sha` with full history and
`persist-credentials: false`. A later default-branch tip MUST NOT replace the
source tree for an older push event.

## 3. Credentials and execution

Task suites use the repository's locked Mise configuration for task and tool
resolution. Config evaluation, tool installation, and task execution use
`--no-env` / `--no-hooks` isolation and a private Mise home. Task code receives
no GitHub token or private-fetch credential. Job permissions are limited to
`contents: read`, which is used for checkout; no Actions artifact-read
capability is exposed to the task. A task that needs authenticated APIs or
artifact downloads requires a separately approved Velnor-owned fetch contract.

The suite is an additional generated workflow, not a new workflow-event mode
for the primary CI IR. The normal generated tree owns its workflow file:
hand-maintained or preserved ownership collisions are rejected, generation
and freshness checks include the file, and removing the declaration retires
the generated workflow.

## 4. Artifacts

An optional `artifacts` list declares literal artifact names, upload
conditions (`failure`, `always`, or `success`), and retention from 1 to 90 days
(default 14). Each artifact declaration MUST contain exactly one
repository-relative file, directory, or wildcard path pattern. That pattern
MUST match at least one uploadable file whenever its upload condition is
active. To require several paths, declare a separate artifact for each path
with a distinct literal name. The renderer emits a separate upload step for
each declaration, so a missing path fails even when another declared path has
files. Shard or partition name templates are not accepted.

A generated artifact name includes an unambiguous length-prefixed encoding of
the suite ID and declared artifact name, plus `github.run_id` and
`github.run_attempt`. Distinct suite/artifact pairs therefore have distinct
artifact identities, and reruns keep distinct identities too.
Retention for an evidence window MUST include enough time for that window and
its predecessor/scheduling margin.

See the [pinned artifact upload source reference](github-actions-artifact-reference.md)
for the action's missing-file and path matching semantics.

An empty artifact list emits no upload. Uploaded artifacts are outputs of
unprivileged repository task jobs and do not authorize publication, protected
checks, or conclusions in `Required`.

## 5. Scheduled evidence snapshots

A schedule-only suite MAY declare an `evidence_snapshot` input to consume
bounded history from the same repository:

```toml
[workflow.task_suites.evidence_snapshot]
window_days = 31
workflow_sources = [
  { kind = "primary_ci" },
  { kind = "task_suite", suite_id = "push-head" },
]
artifact_sources = [
  { suite_id = "push-head", artifact_name = "ledger" },
]
```

The input accepts only the generator-owned `ci.yml` metadata source and exact
push-enabled task-suite workflows on the configured default branch. Artifact
references must name declared outputs from those suites; arbitrary workflow
paths, API endpoints, headers, commands, or credentials are not configurable.
Every in-window producer attempt, including failures and reruns, must have its
own attempt-qualified artifact. The latest run immediately before the window
is retained as temporal context; it does not replace an in-window run or
require an archive. Missing producers, attempts, pages, artifacts, or metadata
fail acquisition instead of silently shrinking the evidence set.

The generated scheduled workflow separates acquisition from task execution.
A trusted fetch-only job uses the digest-verified Velnor helper and a narrowly
scoped read token to capture complete metadata pages and original artifact ZIP
bytes. It does not check out the repository or run Mise or repository code.
It uploads one indexed bundle bound to the collector's run and attempt. A
separate consumer job downloads that exact same-run bundle, verifies its
specification, complete file set, identities, and digests, then stages the
verified snapshot under `.velnor/evidence` before the credential-scrubbed task
runs. The task receives only `VELNOR_GITHUB_EVIDENCE_ROOT`; it receives no
GitHub token or authenticated network fallback.

Generation requires a verified immutable helper release that includes these
operations. Source support does not make the currently published helper
release compatible, and a generated workflow must not pin an unreleased or
unverified helper version. Hosted repository qualification is still required
to prove live API and archive access, retention, and scheduled delivery.

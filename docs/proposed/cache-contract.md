# Velnor V1 Cache and Report Contract

Status: proposed; no implementation is claimed. Defines task identity, cache ownership/trust, reports, and
final result aggregation.

## 1. Task identity and canonical digests

Every generated task has a stack-neutral `TaskIdentity` with these fields:

```text
schema_version
stack_id, project_root, and component_id
task_kind, task_id, argv, and repository-relative working directory
configuration (target, profile, features, flags, task contract, compile driver, and test runner)
declared input paths and content digests
toolchain, platform, and explicit environment identities
output contract and generator identity
stack_extension (typed, versioned adapter data)
```

The contract crate owns this envelope and canonical serialization. Each detector supplies a typed
`stack_extension`; Mise supplies tool, environment, and generated-task identity. Only the orchestrator
combines them and decides reuse eligibility. The renderer never computes cache identity. An unknown extension
schema disables reuse and baseline coverage.

For `stack_id = "rust"`, `stack_extension` is a `RustTaskIdentityExtension` containing the Cargo package ID,
normalized manifest path, workspace/local-package graph digest, target kinds and names, features and required
features, Rust target/profile, detected `compile_driver = cargo|mbx`, detected
`test_runner = cargo_test|cargo_nextest`, Cargo config and build-script input
digests, `.config/nextest.toml` digest for Nextest profiles, Rust task kind, and
selected test/archive identity. It MUST NOT include advisory `rust-toolchain.toml` content unless the task actually
consumes it; the project tool files are inspection inputs only.

Velnor MUST reject undeclared task inputs before execution. Environment values that affect behavior MUST be
declared; secrets MUST never enter an identity or report. Publishing, deployment, notification,
network-service, clock-dependent, random-dependent, and undeclared-external-state tasks are ineligible for
task result reuse.

Canonical bytes MUST be UTF-8 JSON with lexicographically sorted object keys, declared array order, no
insignificant whitespace, normalized repository paths using `/`, and no non-finite numbers. The schema MUST
reject duplicate keys. The digest MUST be BLAKE3 over those bytes and encoded as `b3-` plus 64 lowercase hex
characters. Any canonicalization change requires a schema version bump.

The cache identity MUST contain these exact fields:

```text
schema_id       = "v1"
stack_id        = registered detector ID
repository_id   = BLAKE3(canonical remote origin or configured repository ID)
project_root    = normalized repository-relative detected project root
component_id    = detector-defined stable component identity
workspace_id    = BLAKE3(repository_id, stack_id, project root, inventory)
lane_id         = BLAKE3(workspace_id, component ID, task/build kind, configuration, writer lane)
platform_id     = BLAKE3(OS, architecture, exact runs-on label, ImageOS, ImageVersion, execution target)
toolchain_id    = BLAKE3(Velnor-pinned tool IDs, versions, components, and selected driver/runner)
cache_format_id = BLAKE3(adapter-reported Cargo or MBX compiler/cache formats and generation)
stack_extension_id = BLAKE3(canonical typed adapter extension)
input_digest    = BLAKE3(TaskIdentity canonical bytes)
```

`rust-toolchain.toml`, `mise.toml`, and `mise.lock` are not execution authorities and are excluded from
`toolchain_id`. They remain explicit repository inputs for scan findings and recommendations; Velnor never
writes them. Their digests MUST NOT enter task `input_digest` for Velnor-generated compile, lint, test, or
documentation tasks. Inspection reports may include their digests. `workspace_id`, `lane_id`, `platform_id`,
`toolchain_id`, and `cache_format_id` MUST be recorded in the plan/report. For
MBX profiles, Rust's cache format includes the reported MBX object format and
cache-generation; if MBX cannot report them, preflight MUST fail instead of
guessing compatibility. Cargo profiles use their Cargo source/build-cache
format and emit no MBX object-cache steps. Velnor MUST compute
`compatibility_id` as BLAKE3 of the canonical object
containing `schema_id`, `repository_id`, `workspace_id`, `lane_id`, `platform_id`, `toolchain_id`,
`cache_format_id`, and `stack_extension_id`.

Trust is an access and promotion boundary, not semantic task input. Keep trust out of `input_digest`; encode
it in cache namespaces, permissions, and baseline proof. This permits PRs to read exact trusted baseline
evidence without allowing PR results to become trusted. An input digest MUST include task definition and
arguments, working directory, complete declared input closure, dependency and tool identities, relevant
environment, generator/schema, and output contract. Unknown or dynamic inputs disable result reuse and
baseline coverage.

`lane_id` MUST also include task/build configuration and a distinct concurrent writer lane. Concurrent Cargo
writers never share a target directory. The complete graph, change selection, trusted baseline artifact, and
status definitions are specified in the [parallelism and affected-work
contract](parallelism-and-selection-contract.md).

The GitHub cache key MUST be:

```text
velnor-v1-${layer}-${trust}-${compatibility_id}-${snapshot_id}
```

`layer` is `sources`, `mbx`, or `task`; `trust` is `trusted` or `pr`; every digest is `b3-` plus 64 lowercase
hex characters. `snapshot_id` is the full `input_digest` for an exact task result, a source-input digest for
Cargo sources, and a unique run/matrix closure digest for MBX. The complete key MUST be no longer than 512
bytes; the generator MUST fail if it is longer. Restore prefixes MAY omit `snapshot_id` only for the same
compatibility ID. A commit SHA alone MUST NOT be a cache identity.

## 2. V1 Rust cache paths, transport, and fallback

The Cargo/MBX paths below map generic cache rules for V1 Rust. Future adapters define their own paths and
compatibility fields under the same invariants.

Each path has one owner:

| Data | Owner | Rule |
|---|---|---|
| Mise tools and Rust components | Compiled-in generator catalog, executed by Mise | Embed and invoke exact versions; disable project config, env files, and hooks |
| Cargo registry and Git sources | Velnor source layer | Exclude credentials; separate from MBX |
| Compiler objects and scheduler state | `jdx/mr-boxington-action` when MBX is selected | The action owns the object format (`github-cache-mode: objects`). Generated jobs set `ACTIONS_CACHE_MODE=read` so its post does not export inside the live store. `mbx cache export` writes one directory at `$RUNNER_TEMP/mbx-single-bundle`; `actions/cache` archives only that directory; `mbx cache import` loads it. Velnor does not reimplement the object format |
| Mutable target directory | Matrix job | Reuse sequentially; never share concurrently |
| Successful task result | Mise task cache | Use only for qualified deterministic tasks and complete outputs |

The orchestrator decides whether each cache operation is allowed and records that decision in the plan. The
workflow renderer serializes approved GitHub cache restore/save operations from typed workflow IR; it does not
choose keys, trust, eligibility, or save timing. Mise installs the exact MBX binary. The pinned
Mr. Boxington action owns the object format. The archive rule below is the transport.

Cache save is allowed only after its producer succeeded, the current run is trusted for that namespace, and
the export has a useful delta. A task-result cache hit, failed/cancelled task, untrusted PR, empty export, or
unavailable producer MUST NOT trigger a trusted save. Restore and save destinations MUST not overlap active
compiler writers.

The generated workflow MUST use these paths. `CARGO_TARGET_DIR` is never an archive path:

```text
VELNOR_CACHE_ROOT     = $RUNNER_TEMP/velnor/cache
CARGO_HOME            = $VELNOR_CACHE_ROOT/cargo
CARGO_SOURCE_PATHS    = $CARGO_HOME/registry $CARGO_HOME/git
CARGO_TARGET_DIR      = $RUNNER_TEMP/velnor/target/<lane_id>
MBX_TARGET_DIR        = $RUNNER_TEMP/velnor/target/<lane_id>
MISE_TASK_CACHE_DIR   = $VELNOR_CACHE_ROOT/mise-task
MISE_TASK_ARTIFACTS   = $MISE_TASK_CACHE_DIR/task-artifacts/v2
REPORT_DIR            = $RUNNER_TEMP/velnor/<run-key>/<matrix-key>
```

`CARGO_HOME` is set to this isolated path for every generated task. The source archive MUST include only
`registry/` and `git/`, never credentials or other files under Cargo home. Mise stores task artifacts under
`$MISE_TASK_CACHE_DIR/task-artifacts/v2`; CI sets that environment variable before Mise starts and archives
only that directory.

`actions/cache/restore` and `actions/cache/save` MAY archive `CARGO_SOURCE_PATHS`,
qualified `MISE_TASK_ARTIFACTS`, and `$RUNNER_TEMP/mbx-single-bundle`. The action
owns the MBX object format. `ACTIONS_CACHE_MODE=read` skips its in-store post,
which exhausted runner disk (run `37114238559`). A miss, a missing directory, or
a failed `mbx cache import` continues cold. Velnor MUST NOT reimplement either
format. A Cargo-profile job does not invoke the Mr. Boxington action.

Velnor MUST NOT configure Mise `task.cache.remote_url`, remote namespaces, remote tokens, or OIDC task-cache
credentials in V1. There is no Velnor cache server. The selected task-result transport is an opaque GitHub
archive of the Mise local directory above; Mise remains the only authority for task keys, checksums, output
replay, and cache invalidation.

Task-result caching is disabled until Gate 6. Before that gate, workflows use direct pinned `mise exec`
commands; no task definition is written into the repository. At Gate 6, the Mise adapter may write a
versioned, deterministic task TOML file under `$RUNNER_TEMP/velnor/tasks/` and invoke that task with the
pinned Mise binary. The temporary task file MUST declare fixed command arguments, complete `sources`, explicit
`outputs` (including `outputs = []` when appropriate), and complete `cache.command_inputs`. Its first line
MUST carry the Velnor Actions version marker without a date. It MUST NOT read, create, or modify project
`mise.toml`, `mise.lock`, or `.mise/tasks`.

Before Gate 6, task-cache access remains off. After Gate 6, only qualified tasks MAY enable Mise's
experimental artifact cache. The task command is invoked as:

```text
mise run --task-cache <read-only|read-write|off> <task-name> --file <runner-temp-task-file>
```

Local runs use `local-only` by default. PR and `merge_group` runs use `read-only`; protected default-branch
pushes use `read-write`; release jobs use `off`. The exact task TOML cache field is a schema change and MUST
be enabled only with Gate 6 qualification fixtures; no task-cache behavior is implied before that gate.

Pull requests and merge groups MAY restore trusted default-branch archives but MUST NOT write trusted or
release archives. They MUST NOT promote PR-produced executable contents. Fork pull requests are read-only.
Protected default-branch pushes MAY write trusted archives only after the task passes and its report is
complete. Release jobs MUST reject PR archives and MUST NOT restore or save MBX or task-result archives;
release outputs use a clean or trusted-source-only path.

Cache restore MUST verify compatibility and ownership before use. A missing archive is `no_entry`; a GitHub
restore/save failure is `cache_unavailable`; failed MBX import is `cache_corrupt`; a failed task-cache
read/write is a Mise-reported miss. Corrupt, incomplete, or mismatched data MUST be discarded and the task
MUST execute when otherwise eligible. A cache miss MUST never fail a task by itself. A cache save failure MUST
leave a successful task successful and report `cache_write_disabled` or `cache_unavailable`.

Velnor MUST NOT save unchanged data, use an unbounded stable key, globally prune caches, or restore one cache
layer through two mechanisms. MBX objects, Cargo sources, and Mise task artifacts MUST remain independently
restorable. The default branch MUST produce warm snapshots through ordinary CI; no unconditional warm-up
workflow is allowed.

## 3. Miss reasons, artifacts, and report schema

The only allowed `miss_reason` values are:

```text
no_entry
compatibility_mismatch
input_digest_mismatch
trust_scope_mismatch
cache_unavailable
cache_corrupt
cache_expired
cache_write_disabled
task_not_eligible
task_result_incomplete
forced_uncached
tool_missing
source_missing
```

Each task report MUST be JSON with this shape:

```json
{
  "schema": 1,
  "task_report_id": "task-r123-a1-m-<16 hex>-<16 hex>",
  "run_key": "r123-a1",
  "event": "pull_request",
  "trust": "pr",
  "matrix_id": "pkg:velnor-actions-contract|features:default|target:host|profile:test",
  "matrix_key": "m-<16 lowercase hex characters>",
  "task_id": "stack/rust/crates/velnor-actions-contract/clippy/default",
  "task_digest": "b3-<64 lowercase hex characters>",
  "status": "reused|executed|empty_partition|not_selected|failed|cancelled",
  "not_selected_reason": null,
  "cache": {"layer": "task", "key": "...", "result": "hit|miss|not_attempted", "miss_reason": null},
  "exit_code": 0,
  "duration_ms": 1234,
  "outputs": []
}
```

Reports are run-scoped evidence written outside the repository; they are never task-cache outputs and are
never reused across runs. `outputs` lists only declared outputs of the task itself. Reports MUST contain no
tokens, credentials, or secret environment values. `reused`, `executed`, `empty_partition`, and `not_selected`
are distinct. An `empty_partition` is valid only for one test shard whose exact inventory manifest assigns no
tests while the complete selected test inventory is nonempty. A cache hit is `reused` only when all declared
outputs are present and verified. A baseline-covered obligation is represented in the complete plan and final
coverage report, not as an execution-matrix task report. Its proof MUST name the exact trusted baseline
artifact and task/input identities. A failed or cancelled task MUST retain its report and MUST NOT save a
successful task result. `task_report_id` is `task-<run-key>-<matrix-key>-<task-digest-prefix>`, where
`task-digest-prefix` is the first 16 lowercase hexadecimal characters of the task digest without its `b3-`
prefix. A duplicate task-report ID is a planning or reporting error. `not_selected_reason` is required when
status is `not_selected` and MUST be one of `upstream_failed`, `not_in_plan`, `unsupported`, or
`cancelled_by_policy`.

Each matrix job MUST also write one aggregate report at
`$RUNNER_TEMP/velnor/<run-key>/<matrix-key>/matrix-report.json`:

```json
{
  "schema": 1,
  "report_id": "report-r123-a1-m-<16 lowercase hex characters>",
  "run_key": "r123-a1",
  "matrix_id": "pkg:velnor-actions-contract|features:default|target:host|profile:test",
  "matrix_key": "m-<16 lowercase hex characters>",
  "status": "passed|failed|cancelled|not_run",
  "expected_task_ids": ["..."],
  "task_report_ids": ["..."],
  "tasks": [{"task_report_id": "...", "status": "executed", "exit_code": 0}],
  "selected": 4,
  "reused": 0,
  "executed": 4,
  "empty_partition": 0,
  "not_selected": 0,
  "failed": 0,
  "cancelled": 0
}
```

The matrix aggregate MUST contain exactly one task report for every scheduled task ID, including
`not_selected` reports created after an upstream failure. `tasks` and `task_report_ids` MUST be sorted by task
ID. The matrix artifact named by `artifact_id` MUST contain `matrix-report.json` and the task report files
under `tasks/`; no report may be written only to the job log.

The plan MUST also contain one obligation record per task, including the decision (`execute`,
`reused_from_task_cache`, or `covered_by_trusted_baseline`), reason, task/input digest, and cache/baseline
evidence reference. The matrix contains only `execute` obligations. The final job validates every obligation
against its required evidence.

The final job writes `$RUNNER_TEMP/velnor/<run-key>/final-report.json` with report ID `final-<run-key>` and
uploads `velnor-final-<run-key>`. It MUST record the `plan_id`, every expected matrix `report_id`, every
downloaded artifact ID, each required job ID and conclusion (including `alint` when emitted), the
computed final result, and selected/reused/executed/empty-partition/covered/failed/cancelled/blocked/not-run
counts. Its `status` MUST use the final result enum below. `required_job_results` MUST include every
non-matrix required job's ID and conclusion, including `alint` when emitted. The final report is an
aggregate and is not a task-cache input.

Candidate validation, when required, writes one report with ID `candidate-<run-key>-<target-key>` and artifact
ID `velnor-candidate-<run-key>-<target-key>`. Its schema-1 shape is:

```json
{
  "schema": 1,
  "report_id": "candidate-r123-a1-x86-64-unknown-linux-gnu",
  "run_key": "r123-a1",
  "source_commit": "<40 lowercase hex characters>",
  "target": "x86_64-unknown-linux-gnu",
  "artifact_sha256": "<64 lowercase hex characters>",
  "generator_version": "<exact-semver>",
  "status": "passed|failed|cancelled",
  "checks": ["generate-check", "fixtures", "policy"]
}
```

The candidate artifact contains the executable and a manifest with the same source commit, target, version,
and SHA-256. The report MUST verify the manifest before running any candidate command. Candidate artifacts are
run-scoped qualification outputs and MUST NOT be restored by ordinary cache keys or treated as a promoted
generator binary. The merge re-checks the head-bound attestation (`commit == plan.head`) in candidate mode;
manifest fields beyond the commit are consumer-asserted audit data at merge time (S10/D6 residual).

Field grammars here are normative over examples. `*_digest` fields are
path-independent semantic identities: BLAKE3 over canonical JSON with
absolute paths stripped and repository-relative POSIX paths. Artifact
`*_id` values (`velnor-<kind>-<run-key>-…`) are derived NAMEs for
upload/download matching only. Bare numeric service IDs are permitted
ONLY in evidence records (`baseline.proof` and the final report's
`downloaded_artifacts` array), never in identities or NAME derivation
beyond the run-key correlator. Platform, toolchain, cache-format, or
declared-input changes invalidate. `shard-1-of-1` runs in one step;
multi-shard entries use archive-plus-partition fan-out. Publication is
atomic: validate in staging, then rename-swap; failure leaves prior
bytes byte-identical.

## 4. Final gate and exit rules

Every matrix entry uploads the artifact named by its matrix `artifact_id` with `if: always()`.
`candidate`, when present, uploads its derived candidate artifact and report with `if: always()`.
`required` MUST download exactly the plan artifact `velnor-plan-<run-key>`, every expected matrix
artifact, and the candidate artifact when `generator_validation = "candidate"`. It MUST fail when an expected
artifact or report is absent, malformed, duplicated, or has an unknown schema. Artifact names MUST be matched
against the plan's derived IDs; wildcard downloads are prohibited.

The final job MUST validate, in order:

1. `plan.json` and `matrix.json` agree and their `plan_id`/`run_key` match the
current workflow run.
2. The set of expected matrix IDs in the plan equals the set of downloaded
matrix report IDs exactly.
3. Every matrix report contains exactly the task IDs declared by its matrix
entry, and every task report ID is unique and has the matching task digest.
4. If candidate validation is required, the candidate report is present,
references the expected source commit and target artifact, and is passing.
5. The result precedence below is applied only after structural validation.
6. The plan's `generator` `{version, sha256}` MUST equal the lock record
for the runner target (consumers: the embedded release descriptor);
mismatch is `planning_failed`.

The final gate computes exactly one result:

| Result | Condition | Exit |
|---|---|---:|
| `no_work` | Valid plan has zero obligations, and every always-required plan/policy/workflow/format check passed | 0 |
| `passed` | Every obligation is `reused`, `executed`, validly `empty_partition`, or exactly `covered` by trusted baseline evidence | 0 |
| `failed` | A valid required task report records failure, a nonzero child exit, or required candidate validation fails | 1 |
| `cancelled` | Any required task was cancelled | 1 |
| `blocked` | A required task was blocked (`not_selected`), below cancelled | 1 |
| `not_run` | A selected matrix entry lacks a valid report and no task failure was reported | 1 |
| `planning_failed` | Discovery, generation, formatting, matrix validation, or plan-report validation failed | 1 |

`required` MUST run with `if: always()` and MUST be the branch-protection check. A skipped job, missing
report, cache miss, or omitted task MUST NOT be treated as success. The final summary MUST show selected,
reused, executed, empty-partition, covered, failed, cancelled, blocked, and not-run counts plus every
cache/baseline miss reason.

If more than one result condition applies, choose the result in this order: `planning_failed`, `failed`,
`cancelled`, `blocked`, `not_run`, then `passed` or `no_work`. A shard with `empty_partition` is valid only with its
inventory proof and is counted separately in the final report. A real failed task takes precedence over
dependent tasks blocked by that failure. If no task failure is reported, a missing, malformed, or duplicate
matrix report is `not_run`.

## 5. Human plan report

`velnor-actions plan` MAY describe cache layers and eligibility from the
validated workflow plan. It MUST NOT restore, write, or publish cache contents.
Cache reads and writes belong only to generated workflow execution. `plan`
MUST NOT claim cache hits, task-result reuse, or baseline coverage because it
does not perform event-time lookup or execute the generated workflow. It may
show which cache layers would be used and which planned tasks are eligible;
the evidence result remains unknown until workflow execution.

## 6. Acceptance gates

| Gate | Required evidence |
|---|---|
| Determinism | Two generation runs from identical inputs produce byte-identical YAML and task IDs |
| Topology | Triggers, permissions, concurrency, action SHAs, plan, matrix, and final jobs match this contract |
| Selection | Fixtures prove reverse-dependency selection, rename/delete handling, and conservative fallback |
| Focused execution | Clippy gates each crate; detected test runner and doctests are visible separate tasks |
| Tool boundary | CI proves locked Mise tools and the detected Cargo/MBX route; direct Cargo/tool installers are rejected |
| Cache ownership | Sources, MBX objects, target directories, and task results have no duplicate owner |
| Cache correctness | Warm reuse works; each changed identity component invalidates the relevant result |
| Trust | PR runs cannot write trusted/release caches or use release credentials |
| Fallback | Disabled, unavailable, corrupt, and incomplete task caches execute normally and report the reason |
| Final status | Missing, failed, cancelled, skipped, and no-work cases produce the exact final result above |
| Dogfood | Velnor's own committed workflow is generated and checked by Velnor |
| Performance | Warm fixture and dogfood runs report setup, cache, compiler, test, and queue time separately |

The implementation MUST add fixtures for a standalone package, multi-crate workspace, shared dependency chain,
build script/shared fixture, feature target, doctest, failing format/Clippy/test, empty suite, renamed
package, warm rerun, and negative cache identities. A green result with missing qualification runs is not
acceptance.

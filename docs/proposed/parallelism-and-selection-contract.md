# Velnor V1 parallelism and affected-work contract

**Status:** Proposed. Nothing in this specification is implemented yet.

This contract defines how `velnor-actions` auto-detects registered stacks,
selects obligations, omits work with valid evidence, and emits parallel
GitHub Actions steps and jobs. It extends the [workflow contract](workflow-contract.md)
and [cache/report contract](cache-contract.md).

## 1. Required result invariant

For each plan, Velnor MUST enumerate the complete set of required validation obligations before generating executable work. Every obligation MUST end in exactly one state:

```text
executed successfully
reused from a qualified task-result cache
covered by a trusted successful baseline
failed
cancelled
blocked by a failed prerequisite
```

The final gate succeeds only when each obligation is executed successfully, validly reused, or covered by a trusted baseline. Parallel scheduling MUST NOT remove an obligation. A missing plan entry, task report, test shard, or baseline proof is a failure or causes conservative execution; it is never implicit success.

Use `executed`, `reused`, and `covered` as distinct evidence. MBX compilation reuse is not a task-result hit. `unaffected` is a planner explanation for why a task is `covered`; it is not a fourth cache layer.

Velnor Actions is a stack-generic generator with a deterministic detector
registry. V1 registers `velnor-actions-rust` under stack ID `rust`; a stack
without a registered detector produces no inventory and never changes the CLI.
Non-Rust files MAY still be declared inputs to a detected Rust task, including
schemas, fixtures, scripts, native build inputs, and doctest documentation.
The Rust adapter computes Rust inventory and affected components;
`velnor-actions-mise` owns tool preparation/cache integration; the orchestrator
selects, reuses, and schedules generic tasks; the renderer only emits workflow IR.

### 1.1 Registered detectors and TOML ignores

The orchestrator builds one sorted repository file index, then invokes each
registered detector once in ascending `stack_id` order. A registry entry has a
canonical `stack_id`, detector schema/version, and detector implementation.
Each detector receives the repository root, tracked/untracked mode, and all
paths after built-in exclusions; the user ignore list is withheld until
detection completes. Detection returns generic
`DetectedProject { stack_id, project_root, inputs, components, diagnostics }`;
duplicate `(stack_id, project_root)` records are an error. The orchestrator
then applies `[stacks].ignore`, a sorted TOML list of exact registered stack
IDs. For example, `ignore = ["rust"]` retains Rust detections in scan results
with status `ignored` but produces no Rust tasks. Unknown IDs and duplicate
entries fail configuration validation. Path exclusions remain in
`[discovery].exclude` and are applied before detectors run. A stack adapter
decides its component/task extension only for retained detections.

## 2. Reference run and lessons

The supplied Mise run is `36299979911`, PR `13697`, using workflow source revision `7d0040ab683c8bf0d013f7d5defa6b1b14e79243`. The E2E job overlaps artifact download with image pull, then runs four branches: branch 0 handles tranches 0 then 4, branch 1 handles 1 then 5, branch 2 handles 2 then 6, and branch 3 handles 3 then 7. Each tranche runs with `E2E_JOBS=2`, so the configured fan-out can reach eight test containers. The lint job overlaps preparation and independent validation (dependency checks, standalone-script checks, lint, and MSRV), then runs default-feature and all-feature Clippy with separate target directories. It runs MSRV before the memory-heavy Clippy group.

| Observed interval | Wall time |
|---|---:|
| E2E artifact download / image pull | 5s / 28s; group 28s |
| E2E test branches | 20:25, 20:26, 19:19, 16:55; group 20:26 |
| Lint preparation / independent checks | 21s / 4:38 |
| Default / all-feature Clippy | 7:49 / 7:43; group 7:49 |
| Whole workflow / longest macOS job | 47:58 / 47:06 |

The E2E lane-duration sum divided by group wall time is 3.77. This is an overlap ratio, not a measured speedup or CPU-time reduction. The longest macOS job dominated this workflow’s completion time, so Velnor MUST report end-to-end critical path as well as task durations.

The workflow has `MBX_DISABLE=1`; it does not demonstrate MBX cache performance. Its ordinary E2E selection excludes slow tests, retries failures, and the E2E job page records a failed tranche attempt despite overall run success. Velnor MUST NOT copy those omissions or report a retry pass as a first-attempt pass. Borrow the visible parallel groups, build-once artifact reuse, target isolation, explicit barriers, bounded fan-out, and failure aggregation.

## 3. Task graph and scheduling model

The planner MUST construct a typed graph before rendering workflow YAML. A task node MUST contain:

| Field | Required meaning |
|---|---|
| `task_id` | Stable ID rooted at `stack_id/project_root/component_id`, then task kind, configuration, and shard identity when applicable |
| `stack_id` / `component_id` | Detector ID plus its stable component identity; for Rust, component identity is Cargo package ID plus workspace and manifest path |
| `task_kind` | Detector-defined task such as format, clippy, test-build, test-run, doctest, docs, or policy |
| `configuration` | Toolchain, components, target, features, profile, flags, and generator schema |
| `input_digest` | Digest of every declared semantic input for this task |
| `depends_on` | Data producers that must finish before this task consumes outputs |
| `gated_by` | Required quality gates that must pass, even if they produce no task input |
| `reads` / `writes` | Paths and external resources read or mutated |
| `outputs` | Reports, binaries, generated files, or artifacts required downstream |
| `resource` | Resource class and bounded CPU, memory, network, or service demand |
| `lane_id` | Stable identity for an isolated mutable Cargo target/cache lane |
| `cache_policy` | Whether compilation or task-result reuse is allowed and under which identity |

Edges MUST distinguish data dependency, quality gate, report/artifact dependency, and resource exclusion. The planner MUST NOT infer that two tasks are independent merely because they have different names. Unknown custom task effects are exclusive and serial until explicitly declared and qualified. The cache contract defines the generic identity envelope and each adapter's typed extension; Rust task nodes MUST carry the `RustTaskIdentityExtension` fields before selection or reuse is considered.

Scheduling MUST be deterministic: select obligations, validate evidence, deduplicate identical task identities, apply read/write conflicts, then emit ready tasks within configured resource limits. Every generated step remains visible by purpose. Do not collapse the graph into one opaque shell step or unmanaged shell backgrounding.

## 4. Changed-work selection

### 4.1 Discover the candidate change set

For pull requests, compare the merge candidate with its exact base commit. For `merge_group`, plan against the merge-group candidate and its recorded base. For local mode, include tracked changes, staged changes, untracked files, and deletions. The orchestrator passes the same change set to every registered detector. A Git command failure, missing base, incomplete detector inventory, or unclassified path MUST broaden the plan; it MUST NOT yield an empty successful plan.

Each detector constructs its own component graph and reverse-selection closure. For the Rust detector, obtain package IDs and target metadata from Cargo; construct local dependency edges for normal, build, development, optional, and target-specific path dependencies. Consider both base and head manifests when a package, path, or dependency edge is added, removed, or renamed. Resolve duplicate names by Cargo package ID and manifest path.

If component A is used by B, and B by C, a relevant change to A selects A, B, and C. A change limited to C does not select an unrelated component. Initially propagate production and build input changes through all reverse dependents; refine test-only edges only after fixture evidence proves it safe.

### 4.2 Inputs that broaden or invalidate selection

The following changes invalidate every affected task unless a narrower complete dependency is proven:

- a registered detector's project manifest, lockfile, membership, graph, feature, or tool configuration;
- Velnor's generator config/version, generated task definitions, workflow pins, policy/lint changes, shared generators, fixtures, target/linker configuration, or runner image/platform identity;
- dependency topology changes, incomplete detector metadata, untracked project roots, or paths outside a detected project;
- unknown files that cannot be classified as irrelevant to every declared task.

For Rust, the adapter records `Cargo.lock`, manifests, `.cargo/config*`, local
graph, features, target/profile, build-script inputs, and selected test/archive
identity in `RustTaskIdentityExtension`. Changes to advisory
`rust-toolchain.toml`, `mise.toml`, or `mise.lock` refresh findings only because
generated execution uses Velnor's exact pins; a task that actually consumes
one of them MUST declare its digest and broaden selection.

`build.rs` and its `rerun-if-changed` inputs are task inputs. If a build script reads undeclared files, directories, environment variables, network state, or ambient host state, exact reuse and baseline coverage are disabled for that package and relevant dependents; execute them conservatively. A Markdown or non-Rust file is not irrelevant when a task declares it as a source or fixture.

If a task reads Git commit/ref, submodule state, or generated version metadata,
those values are explicit inputs and participate in its digest. Do not reuse a
task result merely because its Rust source files are unchanged when the task
observes a different revision identity.

Tasks that depend on wall clock, randomness, live network responses, mutable
services, or undeclared external state are `always_run` unless Velnor controls
that input and records its identity. A cacheable flag alone never proves that
an input is static or captured.

### 4.3 Explain each decision

`plan.json` MUST list every obligation and give its decision and reason. Example:

```text
parser / test-run: execute; foundation/src/token.rs changed via parser → foundation
storage / test-run: covered; exact trusted baseline task and input digests match
cli / clippy: execute; Cargo.lock changed
```

The generated matrix MUST be derived from this exact plan; matrix jobs MUST NOT rediscover packages or independently filter tests.

## 5. Trusted baseline coverage

Affected selection needs evidence for omitted obligations. Velnor MUST use an immutable `baseline.json` uploaded as a run-scoped artifact only after the protected default-branch `push` passes the required final gate. Pull requests, forks, merge groups, other branch pushes, failed runs, and cancelled runs MUST NOT publish trusted baseline evidence.

The manifest MUST bind repository identity, exact source commit, protected workflow path/ref/event, run ID/attempt, successful final result, generator version/digest/schema, compatibility identity, and a task entry for each covered obligation. Each task proof MUST bind task ID, task digest, input digest, Cargo graph identity, toolchain/components, MBX format, platform/target, profile/features, and the direct successful execution run. When a later baseline carries a prior proof forward, retain the original proof run ID and record the carrying run separately. Schema 2 requires `parent` on every manifest and `carried_from` on every task entry; direct publications set both to `null`. A carried manifest includes the complete immediate parent manifest, and each carried task binds that parent by its source commit, run ID, artifact ID/name, and canonical manifest digest. Validation walks the full chain, requires the same repository, ref, workflow, generator version/digest, and compatibility identity at every node, rejects non-prior or repeated run IDs, and checks expiry. The full canonical JSON is limited to 1 MiB and 32 manifests; invalid or oversized lineage fails closed to execute-all.

The artifact contains one UTF-8 `baseline.json` and no other payload. Its minimum shape is:

```json
{
  "schema": 2,
  "repository_id": "b3-...",
  "source_commit": "<40 lowercase hex>",
  "ref": "refs/heads/main",
  "event": "push",
  "workflow_ref": "owner/repo/.github/workflows/ci.yml@refs/heads/main",
  "run_id": 12345,
  "run_attempt": 1,
  "final_status": "passed",
  "generator_version": "<semver>",
  "generator_sha256": "<64 lowercase hex>",
  "compatibility_id": "b3-...",
  "artifact_id": 12345,
  "artifact_name": "velnor-baseline-<source-commit>-<compatibility-id>",
  "tasks": [{
    "task_id": "stack/rust/crates/parser/test-run/default",
    "task_digest": "b3-...",
    "input_digest": "b3-...",
    "closure_digest": "b3-...",
    "proof_run_id": 12345,
    "carried_from": null,
    "observed_run_id": 12345
  }],
  "parent": null
}
```

The artifact name is `velnor-baseline-<source-commit>-<compatibility-id>`, with full IDs. Its bytes are hashed and that digest is recorded by the current plan. The planner obtains a run ID only from a completed successful run of the exact generated workflow on the protected default-branch ref at the exact base commit; then it requests that run's exact artifact ID. It validates artifact metadata and the manifest before comparison. It MUST NOT select the newest successful run from another commit and treat it as the base proof. A previous proof may be carried forward only when the current trusted run revalidates the same exact task and input identity.

For PR, merge-group, and protected default-branch push planning, resolve the
baseline after inventory and obligation identities are known. The base is the
PR base SHA, merge-group base SHA, or push event's `before` SHA, respectively.
The pinned `gh` CLI MUST be a Mise-managed tool. The orchestrator asks the Mise
adapter to execute fixed `gh run list`/`gh run download` arguments through the
pinned Mise environment, passing only the full base SHA, generated workflow
path, protected default-branch name, and expected artifact name. It filters
results again for exact `headSha`, `refs/heads/<default>`, `event=push`, and
successful conclusion; it then downloads by exact run ID and artifact name
into a fresh temporary directory. It accepts no arbitrary URL, run ID, shell
fragment, or artifact wildcard from project config. After reading and
validating `baseline.json`, it removes the downloaded archive and extracted
temporary files. This avoids a second GitHub HTTP client and reuses Mise as the
tool installer.

Generated workflows set workflow-level `contents: read` and omit `actions`.
Repository-policy `plan` and `required` receive job-level `contents: read` and
`actions: read` for authenticated artifact lookup; job permissions replace
workflow permissions. `plan` binds `GH_TOKEN` and `GH_REPO` only to its
internal baseline-lookup step, while `required` binds them only to the exact
report-fetch step. Consumer plans do not request Actions access; a lookup miss
records `baseline_unavailable` and schedules affected obligations.
Before any repository task starts, Velnor removes `GH_TOKEN`, `GITHUB_TOKEN`,
`ACTIONS_RUNTIME_TOKEN`, and other undeclared action credential variables from
the child environment. No `pull_request_target`, PR write token, wildcard
artifact download, executable in the baseline, or PR-produced proof is allowed.
The published baseline contains evidence only: no source, executable,
credentials, or task-cache directories. If `gh` is missing, API access fails,
or download fails, planning records `baseline_unavailable` and schedules
affected obligations normally; it still fails if the Rust inventory itself
is incomplete.

A task is `covered` only when the baseline entry’s entire compatibility and input identity matches the current obligation. Baseline lookup failure is an optimization miss: missing, expired, inaccessible, malformed, wrong-ref, wrong-commit, stale-schema, or incomplete evidence causes all otherwise-unproven obligations to execute. Record the precise reason. An incomplete task inventory is a planning failure, not a baseline miss.

Repository-wide policy, generated-file freshness, workflow security, and
dependency-advisory checks run on every candidate by default; baseline
coverage applies to package validation obligations. A check backed by changing
external data, such as an advisory database, may be skipped only when the
exact external-data identity/freshness window is part of its obligation and
the baseline satisfies the policy's maximum age. Otherwise it runs again.

The planner MUST classify every obligation before emitting the matrix:

```text
execute
reused_from_task_cache
covered_by_trusted_baseline
```

The matrix contains only `execute` obligations. The plan and final report contain all obligations. `required` validates direct reports for executed work, complete outputs for task-cache reuse, and exact manifest entries for baseline coverage. A plan with obligations all covered is `passed`, not `no_work`; `no_work` means there are no obligations.

Baseline trust is separate from the semantic task digest. Trust controls who may read/write/promote evidence; it is not source content. PR/merge jobs may read trusted baseline evidence but cannot publish trusted baseline or cache state. Mise task-result caching remains a separate, later-qualified layer. MBX hits never count as task reuse or baseline coverage.

## 6. Parallel GitHub workflow generation

GitHub `parallel:` runs its child steps concurrently and implicitly joins them. `background: true` launches an identified child that must later be joined with `wait`/`wait-all` or cancelled explicitly. A background step’s outputs and environment changes are unavailable until its wait completes. GitHub allows at most ten concurrent background steps in one job; additional work queues. These constructs are job-step syntax and cannot be placed inside composite actions.

Velnor may emit native GitHub `parallel`/`background`/`wait` syntax only when the exact pinned actionlint version used by generation and CI parses and validates that syntax. The currently verified actionlint v1.7.12 does not support these GitHub step keys, so V1 MUST NOT emit them by default. Until a qualified actionlint release supports them, parallelize through independent workflow jobs and matrices. Never emulate background work with shell `&`, `wait`, or detached processes. A future syntax-capability update must qualify successful joins, failed children, cancellation, output visibility, and timing before enabling the syntax. A wait remains a synchronization point, not work to add to child durations.

Overlap independent preparation, such as downloading a verified build artifact while pulling a pinned test image. Do not overlap installation with consumers that need the installed tool or environment exports. Restore independent cache layers concurrently only when paths do not overlap. Cache export/save begins only after all writers join.

### Default Rust validation topology

1. Planning, structural policy, generated-file freshness, and format validation run early.
2. Each affected package/configuration is a matrix obligation. Independent entries run concurrently with a finite `max-parallel` derived from the configured runner budget.
3. Within a package, required Clippy configurations that are supported become independent matrix obligations, each with its own target directory/lane. Unsupported all-feature combinations are not generated; they require explicit declared feature configurations.
4. After Clippy passes, use the workspace's detected test runner. Cargo-test projects run `cargo test` as one visible task. Nextest projects may create one archive and fan out partitions as separate jobs when measured suite size justifies transfer/setup costs. Doctests and docs remain distinct visible tasks. Other independent packages continue and report their result.
5. The final job joins the complete expected task/report set and decides the required status.

The workflow matrix MUST set `fail-fast: false`: one package failure must not cancel unrelated required package work. This does not weaken fail-fast inside a package: dependent test tasks do not start after Clippy failure. Newer commits may cancel superseded workflow runs through the workflow concurrency group; the final result of the active candidate still requires complete evidence.

GitHub background concurrency, matrix jobs, Cargo build jobs, and Nextest test threads are separate limits. Velnor MUST set an explicit finite budget for each and avoid accidental multiplication. One matrix job MUST use at most ten simultaneous background steps. Resource classes include at least lightweight, network, compiler CPU/memory, test CPU, service-backed test, and exclusive. If the runner has no reliable capacity data, use conservative fixed limits and report them; do not assume more parallelism is faster.

## 7. Cargo target isolation and MBX

Concurrent Cargo writers MUST use different target directories. A stable lane identity includes package/task family, target, features, profile, compiler configuration, and concurrent-writer lane. The planner MUST insert a resource exclusion when two tasks would write the same target directory, generated destination, database, snapshot path, HOME/config, or cache-export destination.

For MBX profiles, MBX supplies compiler-output reuse and compiler resource coordination; it does not make simultaneous writes to one Cargo target directory safe and does not prove tests passed. Each lane’s target path and selected driver MUST be recorded. Cache restore completes before compilation starts. Cache export follows the join of all compiler writers. Separate target directories are not automatically persisted by a cache action: Velnor MUST declare their owner and verify the actual restored/saved paths. Cargo profiles use the separately owned Cargo source/cache layers and MUST NOT install or invoke MBX.

Parallel Clippy may increase peak memory. Where the measured runner cannot safely support both configurations, schedule them in separate groups while retaining both checks. Resource barriers are valid optimization choices; removing checks is not.

## 8. Build once and partition tests safely

Only a detected Nextest profile uses Nextest build reuse. Compile each required test configuration once into a qualified archive, then execute that archive. Small suites use one Nextest invocation. Large suites fan out partition jobs only when measurement shows execution savings exceed extra runner setup and artifact transfer. Until actionlint supports native step parallel syntax, Velnor MUST use job-level matrix fan-out rather than `parallel:` steps. Each partition uses a unique extraction directory. The archive identity MUST include source/input digest, package, target, features, profile, toolchain, linker/runtime requirements, Nextest version, and archive format. Cargo-test profiles do not create a Nextest archive. An archive is compiled executable data, so trust and retention rules must match its source.

`manifest-key` is the normalized repository-relative path to the Cargo manifest
without the trailing `/Cargo.toml`; the root manifest uses `root`. Every
package task ID uses `stack/<stack-id>/<component-key>/<task-kind>/<configuration>`; a
partition adds `/shard-<index>-of-<count>`. Package names are display labels,
not task identity, because Cargo permits duplicate names in separate
workspaces.
Only detected Nextest profiles have shard IDs in `execute_task_ids`; Cargo-test
profiles have one test obligation. A Nextest partition job invokes its exact
Mise/MBX command and writes one report. Shard count defaults to 1; explicit
per-manifest overrides MUST NOT exceed the test-process budget. Unconfigured
suites are not auto-sharded. Change the shard count only after timing evidence
shows added job fan-out helps.

`strategy.max-parallel` MUST equal `workflow.max_parallel_jobs`. All compiler
tasks share `resources.compiler_process_budget`; MBX coordinates compiler
workers only for MBX profiles. Nextest test processes use the configured test
budget. The planner rejects zero budgets, shard counts beyond the test budget,
or a requested concurrency above known runner capacity.

Only Nextest profiles use these qualified command shapes:

```sh
mbx nextest archive --package <package-name> --profile ci --cargo-profile test --locked --archive-file target/nextest/tests.tar.zst
mbx nextest list --profile ci --archive-file target/nextest/tests.tar.zst --locked --message-format json --partition hash:<index>/<count>
mbx nextest run --profile ci --archive-file target/nextest/tests.tar.zst --extract-to "$RUNNER_TEMP/velnor/nextest/<matrix-key>/<partition-id>" --locked --no-tests fail --partition hash:<index>/<count>
```

The first command runs once for the exact package/configuration archive. `test_inventory` runs once per configured partition and records sorted test identities. An empty full inventory fails unless Cargo metadata proves no applicable tests exist. An empty partition is reported without starting a test process. Every consumer uses the same archive and a unique extraction directory; it MUST NOT compile again or write into the archive. A single-shard run avoids artifact transfer. Archive identity includes the exact source, configuration, toolchain, runtime, and platform.

These invocations are generated argument vectors run through Mise. Select the
Cargo or MBX prefix from the detected workspace profile. Append sorted feature
and target arguments at archive creation; the archive records build options.
The partition invocation executes the validated archive and MUST NOT compile
again. Cargo-test profiles do not use these commands or archives.

Use Nextest hash partitioning (`hash:<index>/<count>`) when partitioning is enabled. Keep small suites in one test-run step; process startup and extraction can cost more than splitting saves. Velnor MUST use measured historical durations only as weights, never as a way to omit tests. Unknown tests receive a default weight and remain selected.

Before accepting a sharded result, Velnor MUST compare the expected selected test identity set from `test_inventory` with the exact task reports for all partitions:

```text
union(shards) == selected_tests
intersection(each pair of shards) == empty
```

Identity includes package, target, features, binary, and test name. Missing, duplicate, unexpected, or malformed shard results fail final aggregation. An `empty_partition` is accepted only when its inventory assignment is empty and the complete test inventory is not empty. Empty suites fail unless the plan explicitly proves the target contains no tests and records that obligation as a valid no-test target; blanket `--no-tests pass` is forbidden.

Use Nextest’s supported archive, list, partition, resource-group, and JUnit interfaces; pin the tool version and validate machine-readable schema. Test threads and compiler build jobs have separate budgets. Resource groups/serial groups handle shared services; a per-process test-thread limit does not coordinate separate shard tasks. V1 sets retries to zero. Any later retry feature needs an explicit result state for retry-passed tests, and such results MUST NOT qualify as ordinary deterministic task-cache success or trusted direct-pass evidence.

## 9. Result and performance accounting

The final report MUST include counts of obligations executed, task-result reused, baseline covered, failed, cancelled, blocked, and not run. For each task record input/compatibility identity, lane/resource assignment, queue/start/finish time, process exit, cache outcomes, report/artifact IDs, test partition inventory when applicable, and reason for selection or coverage. Do not store secret values.

Measure queue time, critical-path wall time, runner time, task durations, cache restore/export/transfer, tool preparation, origin downloads, compiler work, MBX outcomes, test time, resource/lock wait, and selected/reused/covered counts separately. Parallel child durations overlap; never add both child durations and group wall time. Compare the same required obligation set and runner class for sequential and parallel measurements. Report an overlap ratio as descriptive only, not as a speedup claim.

## 10. Required fixtures and acceptance

The public `velnor-actions plan` command MUST use the same dependency graph,
task identities, workflow IR, and renderer output as `generate`. It prints
detected workspaces/crates, local dependency edges, planned job groups and
steps, parallel groups, and ignored-work reasons in stable human-readable
order. It reports the full configured crate matrix; event-time affected
selection happens in generated CI. It MUST NOT execute checks, query cache
contents, claim cache/baseline hits, or expose internal `plan.json`. Tests MUST
compare plan facts with the workflow generated from the same fixture and verify
the command leaves the repository byte-identical.

Before enabling affected-work omission or parallel groups in production generation, fixtures MUST prove:

- independent artifact download and image preparation overlap, then join before use;
- a failed parallel child is reported while successful siblings retain reports;
- one package’s Clippy failure blocks only its dependent tasks, while unrelated packages still finish;
- matrix `fail-fast: false` preserves independent package results;
- concurrent Cargo tasks receive separate target dirs and bounded resource slots;
- cache restore/export never overlaps a writer;
- exact trusted baseline covers an unchanged task, but missing/expired/malformed/wrong-commit baseline schedules it;
- changed leaf, shared dependency, dependency diamond, deleted/renamed package, `build.rs` input, fixture, Velnor's pinned toolchain, `Cargo.lock`, and generated workflow/task policy invalidate the correct closure; edits to optional `rust-toolchain.toml`, `mise.toml`, or `mise.lock` change inspection findings only;
- unknown/dynamic inputs broaden execution and never produce false `covered`;
- test archive identity mismatch, missing shard, duplicate test, omitted test, and extra test all fail final aggregation;
- a retry is reported separately and is not cached as a clean deterministic pass;
- no obligations yields `no_work`; covered obligations yield `passed`.

An optimization is accepted only when the required obligation set and final result match the sequential reference plan. Performance acceptance compares measured time to the final required result and resource cost; it does not accept fewer checks as a speedup.

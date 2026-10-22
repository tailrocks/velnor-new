**Status:** Proposed end-to-end workflow specification. The current task-execution manifest and resolver boundary is described below; generated-workflow acceptance remains pending.

# Velnor Actions V1 Rust workflow-task execution contract

The public `velnor-actions plan` command performs analysis only; it never
executes Rust tasks. Task execution exists only in generated GitHub workflows.
The generated workflow job named `plan` is internal and distinct from
the local command. This is the Rust adapter's execution extension. Future stack adapters define
their own workflow steps; the orchestrator handles typed task definitions
without assuming Cargo, Clippy, MBX, or Nextest. Internal task identities are
used in plans and reports only. Velnor V1 does not generate Mise task files.

## Fixed Rust execution phases

Rust obligations keep their existing task identities, argument vectors and
execution manifests. Their owning execution job is selected by the Rust
adapter's closed lint/tests/docs phase, independently of package configuration.
Formatting and Clippy belong to lint; executable builds, tests and doctests
belong to tests; documentation building belongs to docs. Empty phases emit no
job. Unknown kinds are rejected rather than omitted.

A test executable build and its consumers remain on the same runner. Declared
quality and data gates inside a phase remain ordered local obligations. Gates
owned by another phase become exact owning-job dependencies. Required validates
all phase conclusions and their exact task/report ownership; timeout, absent
reports or a skipped predecessor cannot be promoted as success. Each phase has
its own cumulative job execution budget of `workflow.crate_timeout_minutes`
(default five, bounded `1..=360`).

Plan and report artifact names bind both workflow run and attempt. A retry MUST
rerun the complete workflow to create the new attempt's Plan and all reports.
Failed-jobs-only reruns fail closed; they MUST NOT consume earlier-attempt plans
or reports to make a partial retry appear complete.

## Current manifest and resolver boundary

This section records the implemented data contract for generated task
execution. It does not claim that the surrounding workflow proposal or hosted
qualification has passed acceptance.

The renderer writes canonical JSON to
`.github/velnor/task-execution-manifest-v1.json`. The file starts with the
standard generated-file marker containing the renderer version, followed by
one canonical JSON object and a final newline. The resolver receives the
expected renderer version independently and checks the exact marker before
parsing the body.

The object has schema `1` and a `tasks` map keyed by stable task ID. Each
record carries `task_id`, the existing planner `task_digest`, canonical
`toolchain_inputs`, fixed `argv`, sorted `env`, `matrix_id`, `matrix_key`,
optional `matrix_max_parallel`, `report_helper_version`, and
`execution_digest`. The full BLAKE3 execution digest is computed over
canonical JSON containing every record field except itself. The full digest
is the only resolver selector. The resolver requires exactly one matching
record, derives `task_id` from that record, and then checks the planner's
executable decision, task digest, matrix ID, and matrix key. The plan digest
and execution digest remain separate bindings.

Unknown fields, duplicate JSON keys, noncanonical JSON, mismatched map keys,
invalid records, stale renderer markers, and digest mismatches fail closed.
The repository-contained reader enforces symlink and size checks. The
manifest limit is 8 MiB and 4,096 records.

The manifest digest binds the original typed record, including literal
`${{ runner.temp }}` expressions. Only after manifest and plan checks pass,
the resolver replaces that exact expression in argv and environment values
with the runner's absolute `RUNNER_TEMP`. Its statically supplied
`VELNOR_RUNTIME_RUNNER_TEMP` must equal `RUNNER_TEMP`; other, malformed, or
unresolved expressions are rejected. The private operation
`resolve-task-execution-v1` receives the full digest through
`VELNOR_TASK_EXECUTION_DIGEST` and the independently selected renderer
version through `VELNOR_GENERATOR_VERSION`. It reads the workspace manifest
and staged plan, emits one data frame, and never spawns the selected command.

The NUL-framed data protocol has fixed field order: magic
`VELNOR-TASK-EXECUTION-V1`; task ID; full execution digest; plan task digest;
matrix ID; matrix key; report-helper version; matrix-cap-present flag; cap or
empty field; argv count and argv fields; environment count and sorted key/value
pairs; then `END`. Every field is NUL-terminated, including `END`; embedded
NULs and trailing or truncated fields are invalid. The frame is at most 1 MiB,
with at most 512 argv elements and 64 environment entries. The generated
wrapper checks the report-helper version, validates complete frame
consumption, reconstructs arrays without shell evaluation, then executes the
argv directly with the validated environment. The workflow action input is
the full `digest`; task ID stays in the manifest and plan-coverage condition,
not as a second selector.

Each Rust crate job MUST expose its owned obligations as named steps, preceded
once by the common preparation applicable to its tools and inputs. It MUST NOT
emit placeholder work. A validated no-op for an applicable
obligation MUST report its reason; no-target tasks remain excluded by the
existing planning rule.

The job's action prelude checks out the selected commit with the pinned
`actions/checkout` and `persist-credentials: false`, initializes the pinned Mise action without project config
or environment loading, and uses Mise to install the exact selected Rust,
components, and tools. Mise honors `components`/`profile` only from
`mise.toml`, which config-less CI invocations cannot use, and installs the
minimal rustup profile otherwise, so components arrive through the fixed
`Prepare Rust components` step (`mise exec rust@<exact> --
rustup component add --toolchain <exact>-<triple> clippy rustfmt`):
the pinned toolchain's own rustup, writing only Velnor-owned tool homes.

For an MBX profile, a Rust-only preflight first verifies the exact selected
Rustup shim and toolchain. The pinned v1.7.2 `jdx/mr-boxington-action` then
installs and owns production MBX at exact version `1.25.0`, with
`github-cache-mode: objects`. Velnor keeps the action's default compiler
identity key and binds cache generation to the action SHA, actual runner environment,
GitHub job ID,
MBX version, and `MBX_SHARE_OUT_DIR=0` / `MBX_GC_AUTO=0` policy. The action's
stable logical object path is `$RUNNER_TEMP/velnor/mbx`; runner job namespaces
provide physical isolation. The workflow does not install MBX through Mise.
A following guard checks both `mbx --version` and `mbx cache dir` through the
selected Rust Mise environment, requiring the exact version and the expected
action-owned store before the build. Automatic GC stays off while task results
are in flight. After the final foreground workspace consumer and report step,
a cleanup guarded by the successful version/store check runs `mbx clean`; this
removes managed workspace outputs while leaving the action-owned object store
and receipts available to the action's post export.

This generation does not yet bind the canonical `platform_id` from the exact
runner label, runtime `ImageOS`, and execution target required by
the cache contract. Current MBX cache hits are therefore unqualified across
image changes; a bounded runtime identity bridge and fail-cold behavior are
still required before those hits can be treated as compatible.

In ordinary task workflows, `ACTIONS_CACHE_MODE` is `write` only on a
protected default-branch push and `read` otherwise. The separate authorized
`workflow_dispatch` qualification job is a run-bound writer exception; its
permissions, explicit save input, and run/attempt/source-SHA cache generation
are specified in the cache contract. The action owns the ordinary MBX
object-store post step. Preflight exports the validated runner-private
`MBX_CACHE_DIR` through `GITHUB_ENV`; the action step receives the same
runner-derived value explicitly so its main and post phases use that store.
Velnor does not transport the live object store.
Cargo source archives and qualified Mise task artifacts remain separate
owners. Run `37114238559` is historical evidence for the retired read-only
workaround, not qualification. Protected-writer saves, fresh-reader restores,
success/failure post cleanup, cold-run parity, and disk usage still require
measurement against the exact generated workflow and runner.

For a Cargo-profile task job, the MBX action and native MBX installation are
absent. A separate plan-job pre-seed may select the native MBX owner when it
must build Velnor's source helper; that preparation does not install MBX into
the Cargo-profile task job.

Common preparation for each phase follows the existing tool and source ownership:

1. `Prepare pinned tools`: install exact Velnor policy tools through a fixed
   Mise invocation that loads no project config (`--no-config` plus
   `MISE_NO_CONFIG=1`; project env/hooks disabled, Velnor-owned tool
   homes). Download bytes trust upstream TLS plus exact catalog pins;
   the committed `mise.lock` is never consulted at install time.
2. Verify the selected Rust toolchain, compile driver, test runner, target and
   runner platform, and report optional tool-file findings.
3. Restore the Cargo source cache owned by Velnor and the selected compiler
   cache when applicable. Keep their existing independent trust/write rules.
4. Verify prepared inputs through locked/offline checks before obligations.

The one crate job (a [hard decision](../hard-decisions.md#1-one-job-per-crate-with-parallel-check-steps))
executes these obligations:

- Sequentially first: the Nextest test-executable build. Nextest is selected
  only with explicit project usage; otherwise the existing Cargo-test command
  compiles and runs tests inside the parallel group.
- Then one parallel group joined by `wait-all`: package formatting when
  explicitly configured (workspace formatting remains owned by `plan`),
  Clippy, the detected test runner (each shard keeps its complete partition
  coverage), doctests, and package documentation with rustdoc warnings denied.
  Clippy and documentation compile in their own Cargo target lanes.

Every runnable task has exactly one owner. Same-job data gates MUST refer to
strictly earlier sequential obligations; no parallel member may gate on
another. The checks carry no quality gates on each other: each must pass on
its own, and a failed member fails the job at the join. A gate owned by
another job MUST become an exact owning-job `needs` dependency. Different
packages or configurations may run concurrently only when no declared
dependency orders them. There is no global all-package lint barrier.

Each crate job retains eligible cache saves and exact task/report publication
under its `workflow.crate_timeout_minutes` job budget; the report upload runs
after the join. `Required` joins every emitted crate job and admits all expected
task proofs and job conclusions. A missing, skipped, canceled or failed required
job cannot be treated as success. Default Rust validation uses literal crate
jobs, not a per-task matrix. Explicit named Nextest suites retain their
separately declared matrix/archive fan-out and `fail-fast: false` policy; that
suite mechanism does not redefine the default crate owners.
See the [parallelism and affected-work contract](parallelism-and-selection-contract.md)
for resource bounds, exact baseline coverage and complete test partitioning.

All compile and test commands run through exact Velnor-pinned Mise tools with
project config, env files, and hooks disabled. The Rust adapter records one
`RustExecutionProfile` per workspace:

```text
compile_driver = cargo | mbx
test_runner = cargo_test | cargo_nextest
evidence = [{ path, line, command_or_setting, strength }]
```

Strong MBX evidence is executable project intent: a Mise Rust setting with
`mr_boxington = true`, a Cargo rustc wrapper naming MBX, an executable task,
script, or existing hand-written workflow that invokes `mbx`. Strong Nextest
evidence is an executable task/script/workflow invoking `cargo nextest` or
`nextest run/archive/list`. Ordinary `cargo test` invocation selects Cargo's
test runner. Lockfile entries, comments, documentation, and Velnor-generated
workflows are non-evidence and MUST NOT influence selection; installed tools,
cache directories, and README examples are transient evidence (see table below). If both competing ordinary test runners are explicitly used,
generation fails with `ambiguous_test_runner` and lists evidence. If neither is
used, default to `cargo test` and print a Nextest recommendation only.

Evidence strength is normative:

| Strength | Selects profile | Examples |
|---|---|---|
| `durable` | Yes | Mise Rust `mr_boxington = true`; Cargo rustc wrapper naming MBX; executable task, script, or hand-written workflow invoking `mbx`, `cargo nextest`, or `nextest run/archive/list`; ordinary `cargo test` invocation for the Cargo test runner |
| `transient` | Only with explicit declaration | Installed tools, cache directories, README examples |
| `non-evidence` | Never | Lockfile entries, comments, documentation, Velnor-generated workflows |

When the selected profile rests on transient evidence only, `plan` MUST
record a `transient_profile_evidence` finding and `generate` MUST exit 1
with instructions to declare explicit sticky keys. The sticky keys are
`[stacks.rust] compile_driver = "cargo" | "mbx"` and `[stacks.rust]
test_runner = "cargo_test" | "cargo_nextest"` (see
[architecture §3](architecture.md)). A declared key that conflicts with
durable evidence fails closed with exit 1. Every profile records its
provenance (declared keys and evidence records with path, line, and
command-or-setting) in the plan finding and the generation report.
Persistence is the user-edited `.velnor/config.toml` only; `generate`
MUST never write configuration.

The profile is per workspace, appears in the concise generation report, and is
part of every Rust task and cache identity. Switching Cargo/MBX or Cargo
test/Nextest invalidates prior result evidence. Metadata discovery uses the
Mise adapter's exact Cargo tool request; if MBX exposes a qualified metadata
equivalent, the adapter may use it, otherwise Cargo metadata is an explicit
discovery-only exception.

Do not run `cargo install`, ad hoc Rust component installers, absolute Cargo
paths, or `taiki-e/install-action`. Until task-result caching is qualified,
commands use direct `mise exec` with `--no-config --no-env --no-hooks`;
afterward only qualified cached tasks may use
the temporary task definition. Nextest MUST compile its selected configuration
once and reuse its archive/metadata for execution; it does not run doctests.
Project `rust-toolchain.toml`, `mise.toml`, and `mise.lock` remain byte-identical.

For a matrix entry, generated workflow steps MUST use these commands. The generator appends sorted
`--features <...>` and explicit `--target <triple>` arguments when the matrix entry requires them; `host`
omits `--target`.

| Task kind | Cargo profile | MBX profile |
|---|---|---|
| `clippy` | `mise --no-config --no-env --no-hooks exec rust@<rust> -- cargo clippy --package <package> --all-targets --locked -- -D warnings` | `mise --no-config --no-env --no-hooks exec rust@<rust> mr-boxington@<mbx> -- mbx clippy --package <package> --all-targets --locked -- -D warnings` |
| `test` | `mise --no-config --no-env --no-hooks exec rust@<rust> -- cargo test --package <package> <metadata-derived-non-doc-target-flags> --locked` | `mise --no-config --no-env --no-hooks exec rust@<rust> mr-boxington@<mbx> -- mbx test --package <package> <metadata-derived-non-doc-target-flags> --locked` |
| `nextest` | `mise --no-config --no-env --no-hooks exec rust@<rust> cargo-nextest@<nextest> -- cargo nextest run --package <package> --locked` | `mise --no-config --no-env --no-hooks exec rust@<rust> mr-boxington@<mbx> cargo-nextest@<nextest> -- mbx nextest run --package <package> --locked` |
| `doctest` | `mise --no-config --no-env --no-hooks exec rust@<rust> -- cargo test --package <package> --doc --locked` | `mise --no-config --no-env --no-hooks exec rust@<rust> mr-boxington@<mbx> -- mbx test --package <package> --doc --locked` |
| `doc` | `mise --no-config --no-env --no-hooks exec rust@<rust> -- cargo doc --package <package> --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` | `mise --no-config --no-env --no-hooks exec rust@<rust> mr-boxington@<mbx> -- mbx doc --package <package> --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` |

The adapters construct these command families as argument vectors; the table is
not shell text. Clippy names exactly one package. For Cargo-test mode, the
adapter emits only existing `--lib`, `--bins`, `--tests`, `--examples`, and
`--benches` flags from Cargo metadata; it excludes doctests because they have a
separate step. If no such target exists, record `valid_no_test_targets` and
emit no test command. Cargo-test mode compiles and runs through one step.
Nextest mode has separate build/archive,
inventory, and execution steps only when measured suite size requires
partitioning; small suites use a single `nextest run` step. When `[test_sharding]`
configures `default_shards == 1` (single shard `shard-1-of-1` with no `by_manifest > 1` override),
`cargo nextest run` (or `mbx nextest run`) continues to emit `--no-tests fail` without
`--partition` (byte-identical to `0.1.8`). When `default_shards > 1` or `by_manifest > 1` configures
`n` shards (`n > 1`), each shard step emits `--no-tests warn --partition hash:<i>/<n>` for shard `i`
in `1..=n` so crates with fewer tests than shards do not fail with Nextest exit code 4 on an empty
hash bucket. Empty selected
suites fail unless Cargo metadata proves that the package has no applicable
test target. A mixed test runner is never selected by guesswork.

Each generated workflow command uses the Velnor-owned environment:

```text
MISE_LOCKFILE=0 MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1 \
mise --no-config --no-env --no-hooks exec <tool>@<exact-version>... -- <fixed executable> <fixed arguments>
```

The generated workflow sets `MISE_RUSTUP_HOME` and `MISE_CARGO_HOME`, and exports
`RUSTUP_TOOLCHAIN` only when no repository `rust-toolchain.toml` or `rust-toolchain`
pin is active (since `RUSTUP_TOOLCHAIN` has higher rustup precedence than repository
toolchain files). The effective Rust version resolves from `rust-toolchain.toml`
(`[toolchain] channel = "X.Y.Z"` or `"X.Y.Z-<target-triple>"`), legacy `rust-toolchain`,
`[stacks.rust] toolchain = "X.Y.Z"`, or `[tools] rust = "X.Y.Z"`, falling back to the
compiled catalog default; malformed toolchain files, non-exact channels (such as `"stable"`,
`"1.99"`, or `"nightly-..."`), or any mismatch between `rust-toolchain.toml`,
`rust-toolchain`, and `.velnor/config.toml` fail closed (`invalid_rust_toolchain`,
`conflicting_rust_toolchain`, or `conflicting_rust_toolchain_files`). `[tools]` validates
tool names against the overridable catalog allowlist (`rust`, `mr-boxington`, `gh`,
`jactionlint`, `shellcheck`, `cargo-nextest` / `nextest`, and `opentofu`),
requires exact `X.Y.Z` semver format (`invalid_tool_version`), and rejects specifying
both `nextest` and `cargo-nextest` simultaneously (`duplicate_nextest_tool_override`). Exec steps MUST NOT
load or modify consumer `mise.toml`, `mise.lock`, or environment files;
the install step loads no config either, so no generated step reads
any repository config path. The lock audit is hygiene for local-dev
`mise install`, never runtime verification (checksums are TOFU; see
[tooling-input §1.1](tooling-input-contract.md)). Each
obligation writes one task report under
`$RUNNER_TEMP/velnor/<run-key>/<matrix-key>/tasks/<task-report-id>.json`. The generated
workflow then writes `matrix-report.json` under
`$RUNNER_TEMP/velnor/<run-key>/<matrix-key>/matrix-report.json`, even when an upstream
step failed. It MUST create `not_selected` reports for obligations skipped
because of that failure. Report validation and aggregation are internal
orchestrator behavior rendered into workflow steps; no public CLI command is
used.

## Named Nextest suites and workflow dispatch

Repository configuration may declare named Nextest suites under `[[stacks.rust.suites]]` and manual dispatch inputs under `[workflow.dispatch.inputs.<name>]`:

```toml
[workflow.dispatch.inputs.test_filter]
type = "string"
description = "Optional Nextest filterset expression"
required = false

[workflow.dispatch.inputs.tier]
type = "choice"
description = "Execution tier"
required = false
default = "smoke"
options = ["smoke", "full", "capture", "all"]

[[stacks.rust.suites]]
id = "tier-smoke"
profile = "smoke"
run_ignored = "only"
partitions = 2
build = "archive"
required = true
triggers = ["pull_request", "push", "workflow_dispatch"]
dispatch_inputs = { tier = ["smoke", "all"] }
filter_input = "test_filter"
artifacts = [{ name = "tier-smoke-{shard}", paths = ["target/snapshots/actual", "target/snapshots/diff"], when = "failure", retention_days = 14 }]

[[stacks.rust.suites]]
id = "tier-full"
profile = "full"
run_ignored = "only"
partitions = 12
build = "archive"
timeout_minutes = 5
triggers = ["schedule", "workflow_dispatch"]
schedule = ["0 2 * * *"]
dispatch_inputs = { tier = ["full", "all"] }
filter_input = "test_filter"

[[stacks.rust.suites]]
id = "tier-capture"
profile = "full"
run_ignored = "only"
partitions = 12
build = "archive"
timeout_minutes = 5
triggers = ["workflow_dispatch"]
allow_failure = true
dispatch_inputs = { tier = ["capture"] }
filter_input = "test_filter"
artifacts = [{ name = "tier-capture-{shard}", paths = ["target/snapshots/actual"], when = "always", retention_days = 14 }]
```

Each `[[stacks.rust.suites]]` entry validates fail-closed before rendering:

- `id`: unique lowercase identifier (`[a-z0-9-]+`, no leading/trailing `-`); reserved IDs (`plan`, `required`, `required-gate`, `jactionlint`, `rust`) are rejected.
- `profile`: Nextest profile name (`[a-zA-Z0-9_-]+`); any profile other than `"default"` MUST exist as `[profile.<name>]` in the workspace's `.config/nextest.toml`.
- `run_ignored`: optional `"default" | "only" | "all"` (alias `"ignored-only"` for `"only"`), passing `--run-ignored only` or `--run-ignored all` when non-default; `[stacks.rust] run_ignored` sets the workspace-level default for standard crate tasks.
- `partitions`: `1..=64` (default `1`), rendered as a GitHub Actions `strategy.matrix.shard` (`[1..=N]`, `fail-fast: false`) with `VELNOR_SUITE_SHARD: ${{ matrix.shard }}` in step `env` and `--partition "hash:$VELNOR_SUITE_SHARD/<N>"` passed to `nextest run`, independent of `resources.test_process_budget`.
- `build`: `"inline" | "archive"` (default `"inline"`). When `partitions > 1` and `build = "archive"`, Velnor emits `suite-build-<id>` (`nextest archive` + archive artifact upload) and `suite-<id>` (`needs: [suite-build-<id>]`, downloading the archive and running `cargo nextest run --archive-file ... --workspace-remap . --extract-to . --extract-overwrite`).
- `timeout_minutes`: `1..=360` (default `5`).
- `test_threads`: optional `1..=64`, exported as `NEXTEST_TEST_THREADS`.
- `runner_label`: optional catalog runner label override; defaults to `workflow.runner_label` or the latest pinned default.
- `workflow`: optional standalone workflow slug (`[a-z0-9][a-z0-9_-]*` without `.yml`, default `"suites"` emitting `.github/workflows/suites.yml`); reserved generator slugs (`ci`, `freshness`, `release`, `binary-release`, `tofu-apply`, `qualification`, `product-release`, `monitoring`) are rejected.
- `triggers`: non-empty deduplicated list of `"pull_request" | "push" | "schedule" | "workflow_dispatch"` (default `["pull_request", "push"]`). Suites with `pull_request` or `push` emit into `ci.yml`; suites with `schedule` or `workflow_dispatch` emit into `.github/workflows/<workflow>.yml`.
- `schedule`: non-empty list of 5-field cron expressions with validated numeric bounds (`minute 0..=59`, `hour 0..=23`, `day-of-month 1..=31`, `month 1..=12`, `day-of-week 0..=7`); required when `triggers` includes `"schedule"` and rejected otherwise.
- `required`: boolean (default `false`); requires `pull_request` or `push` trigger and adds the suite's `ci.yml` jobs to `Required`'s `needs`.
- `cancel_in_progress`: optional boolean overriding concurrency `cancel-in-progress` on standalone suite workflows (default `false`).
- `allow_failure`: boolean (default `false`), setting job `continue-on-error: true` when enabled.
- `filter` and `filter_input`: optional static Nextest filterset passed via `-E`, and optional `[workflow.dispatch.inputs.<name>]` key bound at runtime via `VELNOR_SUITE_FILTERSET` (requires `workflow_dispatch` trigger). When both `filter` and `filter_input` are set on a suite, `filter_input` is appended as an additional `-E` argument when non-empty, intersecting with the static `filter`.
- `dispatch_inputs`: optional map of declared `[workflow.dispatch.inputs.<name>]` keys to non-empty allowed value lists gating the suite's `if:` condition on `workflow_dispatch` runs (requires `workflow_dispatch` trigger; `choice` values must exist in the input's `options`).
- `package` and `manifest_path`: optional workspace member package (`--package <name>`, otherwise `--workspace`) and normalized repo-relative Cargo manifest path (`--manifest-path <path>`), both validated against discovered Cargo metadata.
- `env`: additional static POSIX environment variables (`[A-Za-z_][A-Za-z0-9_]*`, single-line values without `${{`). Reserved keys and prefixes (`PATH`, `VELNOR_*`, `NEXTEST_*`, `MISE_*`, `RUSTUP_*`, `CARGO_*`, `GITHUB_*`, `ACTIONS_*`, `RUNNER_`) are rejected.
- `artifacts`: optional upload declarations with `name` (supporting `{shard}` and `{partition}` placeholders), non-empty repo-relative `paths`, `when = "failure" | "always" | "success"` (default `"failure"`, mapping to `failure()`, `always()`, `success()`), and `retention_days` (`1..=90`, default `14`). The rendered upload artifact name is `velnor-suite-r${{ github.run_id }}-a${{ github.run_attempt }}-<id>-<name>`, omitting the extra `<id>-` prefix when `<name>` already equals `<id>` or starts with `<id>-`.

Each `[workflow.dispatch.inputs.<name>]` entry defines a manual dispatch input (`[A-Za-z_][A-Za-z0-9_-]*`) with `type = "choice" | "string"`, optional single-line `description`, `required` (default `false`), optional `default`, and `options` (required and non-empty for `"choice"`, rejected for `"string"`, with `default` required to be a member of `options` when both are set). In standalone suite workflows (`suites.yml`), the auto-generated `suite` choice input (`"all"` plus each dispatchable suite `id`) is emitted only when at least one dispatch-triggered suite in the workflow omits `dispatch_inputs`.

Design rationale:
- `[[stacks.rust.suites]]` lives under `stacks.rust` rather than generic `[[checks]]` or `[[workflow.tasks]]` because suites reuse the Rust stack's resolved compile driver (`cargo` or `mbx`), pinned `rust` and `cargo-nextest` catalog versions, Cargo lockfile/source caching, and `.config/nextest.toml` profile validation without requiring a consumer `mise.toml` or duplicate `[[qualified_tools]]` pins.
- Schema 1 supports `[[stacks.rust.suites]]`, `[workflow.dispatch]`, `[workflow] preserve_workflows`, and `[agents] claude_md` directly (as does Schema 2); Schema 2 `[execution]` governs Scale Set versus GitHub-hosted runner routing and `qualification.yml` selection rather than gating multi-workflow suite emission.
- `partitions` renders a GitHub Actions `strategy.matrix.shard` (`[1..=N]`) rather than `N` separate job definitions or single-job sequential steps, paired with `build = "archive"` (`cargo nextest archive` once in `suite-build-<id>` and `--archive-file` across shards) or `build = "inline"`, so each shard executes on its own runner VM independent of per-runner `resources.test_process_budget`.
- Both first-class `[[stacks.rust.suites]]` and the `[workflow] preserve_workflows` allowlist are provided: `[[stacks.rust.suites]]` covers typed Nextest tiers, schedules, manual dispatch, and artifact uploads directly, while `preserve_workflows` offers a narrow, fail-closed escape hatch for non-Rust or custom repository workflows without weakening `.github/workflows/**` ownership or drift detection.


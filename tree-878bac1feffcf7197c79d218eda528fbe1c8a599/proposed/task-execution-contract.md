**Status:** Proposed implementation specification. No task execution behavior is implemented.

# Velnor Actions V1 Rust workflow-task execution contract

The public `velnor-actions plan` command performs analysis only; it never
executes Rust tasks. Task execution exists only in generated GitHub workflows.
The generated workflow job named `plan` is internal and distinct from
the local command. This is the Rust adapter's execution extension. Future stack adapters define
their own workflow steps; the orchestrator handles typed task definitions
without assuming Cargo, Clippy, MBX, or Nextest. Internal task identities are
used in plans and reports only. Velnor V1 does not generate Mise task files.

Each matrix job MUST expose these named steps. A step MAY be a validated no-op, but it MUST write a
report explaining why.

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
Rustup shim and toolchain. The pinned v1.6 `jdx/mr-boxington-action` then
installs and owns production MBX at exact version `1.21.1`, with
`github-cache-mode: objects`. Velnor keeps the action's default compiler
identity key and binds cache generation to the action SHA, actual runner environment,
GitHub job ID,
MBX version, and `MBX_SHARE_OUT_DIR=0` / `MBX_GC_AUTO=1` policy. The action's
stable logical object path is `$RUNNER_TEMP/velnor/mbx`; runner job namespaces
provide physical isolation. The workflow does not install MBX through Mise.
A following guard checks both `mbx --version` and `mbx cache dir` through the
selected Rust Mise environment, requiring the exact version and the expected
action-owned store before the build.

This generation does not yet bind the canonical `platform_id` from the exact
runner label, runtime `ImageOS`/`ImageVersion`, and execution target required by
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

1. `Prepare pinned tools`: install exact Velnor policy tools through a fixed
   Mise invocation that loads no project config (`--no-config` plus
   `MISE_NO_CONFIG=1`; project env/hooks disabled, Velnor-owned tool
   homes). Download bytes trust upstream TLS plus exact catalog pins;
   the committed `mise.lock` is never consulted at install time.
2. `Verify toolchain`: verify Rust, selected compile driver, selected test
   runner, target, runner platform, and report optional tool-file findings.
3. `Restore Cargo sources`: restore only the Cargo source cache owned by Velnor.
4. `Restore compiler objects`: use MBX's supported portable-object restore
   path only when this workspace's detected profile selects MBX; otherwise
   this step reports `not_applicable`.
5. `Verify prepared inputs`: run locked/offline preparation checks.
6. `Clippy`: run the matrix entry's fixed Clippy command and stop this job on
   failure.
7. `Build test executables`: build the selected test configuration once.
8. `Unit and integration tests`: run the detected test runner for this
   workspace. Nextest is selected only with explicit project usage; otherwise
   invoke `cargo test`. V1 uses one command per package/configuration.
9. `Doctests`: run `cargo test --doc` through the selected compile driver.
11. `Documentation`: build package docs with rustdoc warnings denied.
12. `Save eligible caches`: save only permitted immutable snapshots.
13. `Report timings and reuse`: write the final machine-readable report.

The matrix job MUST execute Clippy before test compilation. Different matrix entries MUST run in
parallel. Matrix `fail-fast` MUST be false so a package failure does not cancel independent package
obligations. Test build and test execution steps MUST NOT run for a package whose Clippy task failed. See
the [parallelism and affected-work contract](parallelism-and-selection-contract.md) for native
background/wait groups, resource bounds, exact baseline coverage, and complete test partitioning.
Formatting runs once in `plan` unless the package has an explicit formatting configuration.

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
[architecture §3](../content/../content/docs/proposed/architecture.mdxx)). A declared key that conflicts with
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
commands use direct `mise exec`; afterward only qualified cached tasks may use
the temporary task definition. Nextest MUST compile its selected configuration
once and reuse its archive/metadata for execution; it does not run doctests.
Project `rust-toolchain.toml`, `mise.toml`, and `mise.lock` remain byte-identical.

For a matrix entry, generated workflow steps MUST use these commands. The generator appends sorted
`--features <...>` and explicit `--target <triple>` arguments when the matrix entry requires them; `host`
omits `--target`.

| Task kind | Cargo profile | MBX profile |
|---|---|---|
| `clippy` | `mise --no-config exec rust@<rust> -- cargo clippy --package <package> --all-targets --locked -- -D warnings` | `mise --no-config exec rust@<rust> mr-boxington@<mbx> -- mbx clippy --package <package> --all-targets --locked -- -D warnings` |
| `test` | `mise --no-config exec rust@<rust> -- cargo test --package <package> <metadata-derived-non-doc-target-flags> --locked` | `mise --no-config exec rust@<rust> mr-boxington@<mbx> -- mbx test --package <package> <metadata-derived-non-doc-target-flags> --locked` |
| `nextest` | `mise --no-config exec rust@<rust> cargo-nextest@<nextest> -- cargo nextest run --package <package> --locked` | `mise --no-config exec rust@<rust> mr-boxington@<mbx> cargo-nextest@<nextest> -- mbx nextest run --package <package> --locked` |
| `doctest` | `mise --no-config exec rust@<rust> -- cargo test --package <package> --doc --locked` | `mise --no-config exec rust@<rust> mr-boxington@<mbx> -- mbx test --package <package> --doc --locked` |
| `doc` | `mise --no-config exec rust@<rust> -- cargo doc --package <package> --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` | `mise --no-config exec rust@<rust> mr-boxington@<mbx> -- mbx doc --package <package> --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` |

The adapters construct these command families as argument vectors; the table is
not shell text. Clippy names exactly one package. For Cargo-test mode, the
adapter emits only existing `--lib`, `--bins`, `--tests`, `--examples`, and
`--benches` flags from Cargo metadata; it excludes doctests because they have a
separate step. If no such target exists, record `valid_no_test_targets` and
emit no test command. Cargo-test mode compiles and runs through one step.
Nextest mode has separate build/archive,
inventory, and execution steps only when measured suite size requires
partitioning; small suites use a single `nextest run` step. Empty selected
suites fail unless Cargo metadata proves that the package has no applicable
test target. A mixed test runner is never selected by guesswork.

Each generated workflow command uses the Velnor-owned environment:

```text
MISE_LOCKFILE=0 MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1 \
mise --no-config exec <tool>@<exact-version>... -- <fixed executable> <fixed arguments>
```

The generated workflow sets `MISE_RUSTUP_HOME`, `MISE_CARGO_HOME`, and exact
`RUSTUP_TOOLCHAIN`, then runs the fixed command vector. Exec steps MUST NOT
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

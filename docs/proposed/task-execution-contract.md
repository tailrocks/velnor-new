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

The job's action prelude checks out the selected commit with pinned
`actions/checkout` and `persist-credentials: false`, initializes pinned Mise
without project config, environment files, or hooks, and uses Mise to install
the exact selected Rust toolchain and policy tools. For MBX profiles, the same
`Prepare pinned tools` step also installs the exact catalog-pinned MBX release.
Config-less Mise installs the minimal rustup profile, so the fixed
`Prepare Rust components` step invokes the selected toolchain's rustup to add
Clippy and rustfmt into Velnor-owned tool homes. For MBX only, the pinned
`jdx/mr-boxington-action` runs with `backend: local` and the exact selected
MBX version; its stock GitHub object-cache path is disabled. Each job gets a
fresh private MBX store. The official MBX CLI owns the object format and exports
one opaque directory at
`$RUNNER_TEMP/mbx-single-bundle` with `mbx cache export`; pinned `actions/cache`
steps transport that directory, and `mbx cache import` loads it. The same cache
actions transport Cargo sources and qualified Mise task artifacts, never the
live MBX store. A miss, absent directory, or failed import continues cold.
`37114238559` records ENOSPC from the former in-store action post. It motivates
external transport, not a claim of lower peak disk use. Hosted workload
qualification remains pending. Cargo profiles
omit MBX setup and bundle steps.

The renderer centrally injects one strict MBX/Rust identity preflight in each
applicable crate job and in the plan-job pre-seed restore path, immediately
before the first MBX action or command. With `--no-config --no-env --no-hooks`, it uses
Mise's exact-pin resolution (`mise where`) to find the MBX and Rust install
roots, then checks that their executables exist and are executable. It requires
`mbx --version` to match the exact catalog pin and verifies the selected
toolchain-qualified `rustc` version command reports the exact Rust release.
Missing or mismatched roots or versions fail before
MBX runs. The preflight is the sole publisher of
the validated MBX and Rust roots through `GITHUB_PATH`; later action and compile
steps reuse them. Qualification workflow bootstraps also install both exact
Rust and MBX catalog pins and use this single preflight/path flow.

1. `Prepare pinned tools`: install exact Velnor policy tools through fixed
   Mise invocations with `--no-config --no-env --no-hooks` and
   `MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1`. Use Velnor-owned tool
   homes; MBX profiles also install the exact MBX catalog pin. Download bytes
   trust upstream TLS plus exact catalog pins; the committed `mise.lock` is
   never consulted at install time.
2. `Verify toolchain`: verify Rust, selected compile driver, selected test
   runner, target, runner platform, and report optional tool-file findings.
3. `Restore Cargo sources`: restore only the Cargo source cache owned by Velnor.
4. `Restore compiler objects`: restore only the opaque bundle with pinned
   `actions/cache`, then import it through the official MBX CLI when this
   workspace selects MBX; otherwise report `not_applicable`.
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
partitioning; small suites use a single `nextest run` step. Empty selected
suites fail unless Cargo metadata proves that the package has no applicable
test target. A mixed test runner is never selected by guesswork.

Each generated workflow command uses the Velnor-owned environment:

```text
MISE_LOCKFILE=0 MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1 \
mise --no-config --no-env --no-hooks exec <tool>@<exact-version>... -- <fixed executable> <fixed arguments>
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

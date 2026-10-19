# Native Rust crate jobs

Each Rust package/configuration owns exactly one generated job,
`Rust / <crate>` (`rust-<crate>`). This is a
[hard decision](../hard-decisions.md#1-one-job-per-crate-with-parallel-check-steps);
it replaces the 0.1.20 Lint/Tests/Documentation phase jobs completely. There
is no per-phase job, phase suffix, phase type or cross-phase gate left.

## Job layout

1. Shared setup, once: checkout, the Velnor helper, the plan, pinned tools and
   Rust components, Cargo sources, the MBX target cache restore and the locked
   offline source fetch.
2. `Build test executables` (Nextest profiles only), sequential: the one
   same-job data producer. Every same-job gate references a strictly earlier
   obligation, so producers are ordered before the parallel group.
3. One parallel group: Format (when configured per package), Clippy,
   Unit and integration tests (each shard), Doctests and Documentation. Each
   member renders as a GitHub `background: true` step; one generated
   `Wait for parallel checks` `wait-all` step joins the group.
4. `Upload crate reports` (`if: always()`), MBX cleanup and cache saves.

A failed member fails the `wait-all` join (runner `BackgroundStepCoordinator`,
see the [runner reference](../proposed/github-actions-reference.md)), so the
job fails; the always-on report upload runs after the join, when every member
has written its report. Required still joins every crate job and admits the
exact task/report inventory. `workflow.crate_timeout_minutes` is the literal
timeout of the one job; background steps are linked to job cancellation.
GitHub runs at most ten background steps of a job at a time and queues the
rest. A job with a single runnable check keeps it sequential. OpenTofu root
jobs keep sequential obligations.

The Rust adapter no longer emits Clippy quality gates on build, tests, doctests
or documentation, nor a doctest gate on documentation: the checks are
independent and each must pass on its own. The only remaining Rust edge is the
Nextest data dependency on its build.

## Cargo target lanes

Cargo holds a target-directory lock for a whole build. Sharing one target
directory would serialize the group behind whichever check compiles longest.
After the sequential build, test execution and doctests reuse the workspace
target (their Cargo invocations only confirm fresh units). Clippy and
documentation need check-mode dependency metadata the build does not produce,
so each compiles in its own lane through `CARGO_TARGET_DIR`:

| Check | Target directory |
|---|---|
| Build, tests, doctests | `<workspace>/target` |
| Clippy | `<workspace>/target/velnor-clippy` |
| Documentation | `<workspace>/target/velnor-doc` |
| Format | none (no compilation) |

The lanes live inside the workspace `target/`, which is exactly the payload the
MBX action's `github-cache-mode: target` saves and restores, so every lane is
cached under the crate job's single cache key; the action prunes each nested
profile the same way as `target/debug`. MBX shares one compiler CPU/memory
budget and deduplicates identical in-flight compilations across target
directories (upstream "Parallel Cargo steps" guidance). The final MBX cleanup
also preserves each lane's `doc/` output, which the restore step puts back.
The contract admits `CARGO_TARGET_DIR` only as a workspace-relative
`target/velnor-<lane>` path.

## Compile parallelism limit

Cargo defaults to one compiler process per vCPU. A crate whose lib and
lib-unittest rustc processes each need several GiB can exhaust a 2 vCPU,
~7 GB hosted runner when the two compile together. `resources.compiler_process_budget`
is the repository-wide ceiling (it bounds the preview producer's
`build_jobs`); it does not change crate jobs by itself. A per-package option
lowers one crate's compile parallelism:

```toml
[stacks.rust.crates.platform-preview-app]
build_jobs = 1 # 1..=64, never above resources.compiler_process_budget
```

The key is the exact Cargo package name; a name no discovered Rust task
belongs to fails generation (`unknown_rust_package`), so a typo cannot leave
the cap silently unapplied. `build_jobs` sets `CARGO_BUILD_JOBS` in the
environment of every Cargo step of that package's crate job: `Build test
executables` and each parallel check (Clippy, tests, doctests, Documentation),
whose separate Cargo invocations otherwise each default to one rustc per
vCPU. Every invocation, not only the build step, is capped, so no single Cargo
run (including a test or doctest run that finds a stale unit) can start the
default number of large compilations. The parallel lanes still run beside one
another, so the job's worst case is the number of concurrently compiling
lanes times `build_jobs` rustc processes. Other crates keep Cargo's default. The setting is not part of
task identity: it changes how fast a task compiles, not what it produces.

## actionlint bridge

Pinned actionlint 1.7.12 does not parse native step concurrency
(rhysd/actionlint#693). Velnor emits the `background`/`wait-all` spelling, in
which actionlint still validates every member, and passes exactly two anchored
`-ignore` patterns (`STEP_PARALLELISM_IGNORES`) to the staged generator
validation and to every generated repository-wide actionlint step (the CI
`actionlint` job and both release eligibility gates share
`workflow_lint_args`): the unknown `background` key on
a `run`/`uses` step, and the body-less join step. The `parallel:` sugar is not
used because actionlint skips its children entirely. Pinned zizmor (1.27+)
parses the syntax. The bridge disappears when a pinned actionlint parses the
keys (`ActionlintCapabilities::parse_native_step_parallelism`).

Composite actions cannot declare background steps. Lane-paired jobs and
over-cap obligation extraction therefore keep a job with a parallel group
literal in the workflow instead of moving it into a composite.

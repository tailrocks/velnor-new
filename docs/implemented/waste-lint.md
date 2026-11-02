# Waste lint

Generation runs a waste lint on the final job graph. A finding stops
generation. Nothing is written.

The lint code is in `crates/velnor-actions-workflow-renderer/src/waste_lint.rs`
and `waste_lint_env.rs`. The lint runs in `finalize_jobs` and in the suites
workflow render, after the cache doctor.

## Scope

A rule applies to a rust-bearing job. A job is rust-bearing when its inferred
tools include a `rust@` or `rust[profile=minimal]@` spec. The scope is the one
function `is_rust_bearing` in `rust_job_env.rs`.

The binder `rust_job_env::bind` sets `Job.env` to `JobEnv::RustToolchain` on
every rust-bearing job. `JobEnv::RustToolchain` renders `RUSTUP_AUTO_INSTALL: "0"`
at job level. It renders in the CI workflow and in the suites workflows.

## Rules

| Rule | Finding | Fix |
|---|---|---|
| W2 `job_context_env` | A shell step runs `cargo`, `rustup`, `rustc`, `mise` or `mbx` without all four toolchain homes. Or a home differs from its standard value. | Set `CARGO_HOME`, `MISE_CARGO_HOME`, `MISE_RUSTUP_HOME` and `RUSTUP_HOME` to the standard values in the step. |
| W3 `rustup_auto_install` | A rust-bearing job has no job-level `RUSTUP_AUTO_INSTALL=0`. Or a step sets `RUSTUP_AUTO_INSTALL` to another value. | Bind the job to the rust toolchain job env. Do not set the value per step. |

The four homes stay in the step env. GitHub does not allow the `runner`
context in a job-level `env`, so `${{ runner.temp }}/velnor/cargo` cannot be a
job-level value.

## Not covered

- Steps of kind `Action`, `TaskExecution`, `NativeCheckExecution` and `Internal`
  are not checked for W2. The task helper owns the task environment.
- Under `consumer-v1`, the group ledger step is an `Internal` step. W2 does not
  check it.
- Workflows from `mise_task_suites_emit`, release and qualification do not use
  the job graph. The lint does not cover them.
- Jobs that run an exec word but install no Rust, such as `cargo-machete`, are
  out of scope.

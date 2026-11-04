# Waste lint

Generation runs a waste lint on the final job graph. A finding stops
generation. Nothing is written.

The lint code is in `crates/velnor-actions-workflow-renderer/src/waste_lint.rs`,
`waste_lint_env.rs` (W2, W3) and `waste_lint_graph.rs` (W1, W4, W5, W6). The lint runs in `finalize_jobs` and in the suites
workflow render, after the cache doctor.

## Scope

Two predicates in `rust_job_env.rs` define the scope.

- A job installs rust when its inferred tools include a `rust@` or
  `rust[profile=minimal]@` spec.
- A job is rust-bearing when it installs rust, or when any of its shell or task
  steps runs a toolchain word. The command text is tokenized on whitespace and
  on `; & | ( ) ' " \ < >`. A token equal to `cargo`, `rustup`, `rustc` or
  `mbx` is a toolchain word, so `cargo-machete` and `$CARGO_HOME` are not. A
  `mise` command with `--locked` before `install` also counts, because it
  installs whatever the lockfile selects, including rust.

W2 applies to jobs that install rust. It covers two kinds of step: a shell step
that runs a toolchain word, and a shell step with the Rust verifier role
(`VerifyRustManifest`, which includes the restored-toolchain check).

W3 applies to every rust-bearing job. The W3 step-override half applies to every
job, rust-bearing or not.

The binder `rust_job_env::bind` sets `Job.env` to `JobEnv::RustToolchain` on
every rust-bearing job. `JobEnv::RustToolchain` renders `RUSTUP_AUTO_INSTALL: "0"`
at job level. It renders in the CI workflow, in the suites workflows and in
`freshness.yml`. The freshness render binds and lints its single job through
`waste_lint::bind_and_check_standalone`.

A job that runs the rustup `cargo` proxy at the repository root without
installing rust auto-installs the toolchain named by `rust-toolchain.toml`.
Such a job is either bound to the job env, or it calls a binary directly.
`cargo-machete` does the second: it runs `cargo-machete <paths>` and never
starts the proxy.

## Rules

| Rule | Finding | Fix |
|---|---|---|
| W1 `duplicate_task` | Two non-producer jobs run the same task digest, or two producer jobs do. A job and its `__hosted` and `__local` lane twins count as one job. One producer may repeat the digest of one crate job. | Run each task digest in one job. |
| W2 `job_context_env` | A shell step of a job that installs rust runs `cargo`, `rustup`, `rustc`, `mbx`, or `mise` with a rust spec, without all four toolchain homes. Or a shell Rust verifier step of such a job has no `RUSTUP_HOME` or `CARGO_HOME`. Or a home differs from its standard value. A `mise` command without a rust spec is not counted. A `jactionlint` verifier is not counted. | Set `CARGO_HOME`, `MISE_CARGO_HOME`, `MISE_RUSTUP_HOME` and `RUSTUP_HOME` to the standard values in the step. In a verifier step, set `RUSTUP_HOME` and `CARGO_HOME`. |
| W3 `rustup_auto_install` | A rust-bearing job has no job-level `RUSTUP_AUTO_INSTALL=0`. Or a step of any job sets `RUSTUP_AUTO_INSTALL` to another value. | Bind the job to the rust toolchain job env. Do not set the value per step. |
| W4 `duplicate_restore` | A job restores the same cache key twice. The cache doctor's classifier decides what a restore is, so a step without a role counts. The two sources key expressions (`steps.cache-keys.outputs.sources_key` and `needs.plan.outputs.sources_key`) are one key. `restore-keys` is ignored. | Restore each key once per job. |
| W5 `non_producer_save` | A job with no producer kind has an `actions/cache/save` step whose condition is not the literal `false`. No role is exempt. Stack family saves render as `false` in read jobs, so they pass. | Save from a producer job only. |
| W6 `unconsumed_restore_output` | A job other than `plan` has an output that reads `steps.<id>.outputs` of a restore step or of the mbx bundle key step, and no other job reads `needs.<job>.outputs.<name>`. `toJSON(needs)` is not a read. | Delete the output, or read it in another job. |

The verifier script reads `RUSTUP_HOME` and, in the restored phase,
`CARGO_HOME`. Without them it falls back to `$HOME/.rustup` and checks a
toolchain the job does not run. The audit of generated verifier steps accepts
exactly those two homes in an inline verifier, so W2 requires those two and no
more.

The four homes stay in the step env. GitHub does not allow the `runner`
context in a job-level `env`, so `${{ runner.temp }}/velnor/cargo` cannot be a
job-level value.

## Not covered

- Steps of kind `Action`, `TaskExecution`, `NativeCheckExecution` and `Internal`
  are not checked for W2. A verifier step that calls a composite action is an
  `Action` step. The composite sets `RUSTUP_HOME` and `CARGO_HOME` in its own
  step env, and the test
  `every_rust_verifier_reads_the_toolchain_home_its_job_installs_into` checks
  the rendered result, including the composite env, against the install step.
  The task helper owns the task environment. The verification task bootstrap
  exports its own private `CARGO_HOME` and `RUSTUP_HOME`, so W2 does not apply
  to jobs that only run lockfile installs or `mbx`.
- Under `consumer-v1`, the group ledger step is an `Internal` step. W2 does not
  check it.
- Workflows from `mise_task_suites_emit` do not use the job graph. The lint
  does not cover them.
- Release and qualification workflows do not use the job graph. The lint does
  not cover them. The test
  `committed_generated_workflows_have_auto_install_off_or_are_listed_uncovered`
  lists them in `UNCOVERED_WORKFLOWS`: `job-image-candidate.yml`,
  `product-release-binary.yml`, `product-release-generator.yml`,
  `product-release-images.yml`, `product-release.yml` and `qualification.yml`.
  The list can only shrink. T5 is measured on main-push CI, and covering these
  workflows is a tracked follow-up.
- W6 counts any raw text `needs.<job>.outputs.<name>` in another job as a read
  (`consumed` and `reads` in `waste_lint_graph.rs`). It matches the text of a
  job condition, `runs-on`, job outputs, and the conditions, inputs, scripts,
  arguments and env values of steps. It does not check that the text sits in an
  expression, and it does not check that the reader's `needs` list the
  producer. Text outside an expression, or in a job that cannot read the
  producer's outputs, therefore hides an unconsumed output. This limitation is
  known and not fixed.
- `ci.yml` is excluded from that committed-file test because it uses YAML
  aliases. The tests that render the CI workflow cover it.

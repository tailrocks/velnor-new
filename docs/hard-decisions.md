# Hard decisions

## One Rust job per package

Each Rust package has one generated job, whatever its task configurations.
The job prepares its tools and Cargo sources once. It builds the test
executables once. It runs formatting, Clippy, documentation, tests, and
doctests as parallel steps. One `wait-all` step joins those steps. A failed
step fails the job. The report upload runs after the join, even after failure.

Clippy and documentation use separate Cargo target directories. This lets
them run beside test execution without a shared target lock. Tests and
doctests use the test executables from the build step.

The generator gives each Rust package the job ID `rust-<crate>` and the
display name `Rust / <crate>`. Each obligation keeps its own configuration,
task identity, and report identity. `Required` checks every package job and
every task report.

OpenTofu keeps one job per root. Its obligations keep their declared order.
Its existing bounded lane staging stays in use.

## Configurable job timeout

`[workflow] crate_timeout_minutes` sets the timeout for each Rust package job
and each OpenTofu root job. The default is `5`. Accepted values are `1..=360`.
The generator rejects zero and values greater than `360`.

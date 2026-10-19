# Hard decisions

A hard decision is a choice the human owner made explicitly. It is not open to
agent judgment, optimization, or "improvement".

**Agents must never silently change a hard decision.** Changing, weakening,
reverting, or working around one requires the human owner's explicit approval
in the conversation that makes the change. Until that approval exists, code,
generated workflows, configuration, repository rules, and documentation must
keep satisfying every decision below. If a decision appears to conflict with
another requirement, stop and ask the owner; do not resolve the conflict by
editing the decision away.

## 1. One job per crate with parallel check steps

**Decision.** Each Rust package/configuration owns exactly one generated CI
job named `Rust / <crate>` (job id `rust-<crate>`, for example
`Rust / eth-processor-app`). The job prepares checkout, pinned tools, Cargo
sources and the MBX cache once and builds the test executables once. Format,
Clippy, Documentation, Unit and integration tests, and Doctests then run as
parallel steps inside that job, using GitHub Actions native step concurrency
(`background: true` members joined by one `wait-all`).

**Rationale.** Version 0.1.20 split every crate into Lint, Tests and
Documentation jobs. Each repeated checkout, tool setup, cache restore and
compilation, and the jobs ran as a chain (Tests needed Lint; Documentation
needed both). For `eth-processor-app` that was about 6, 12 and 4.7 minutes of
runner time with the shared setup paid three times. One job pays setup once
and overlaps the independent checks.

**Invariants.**

- Every check still runs and must pass on its own: a failed member fails the
  job at the join, and the always-on report upload still runs afterwards.
  Required still demands every task report and every crate job.
- Clippy and Documentation compile in their own Cargo target lanes
  (`target/velnor-clippy`, `target/velnor-doc`, inside the MBX target cache
  payload) so Cargo's target-directory lock never serializes test execution
  and doctests behind them. Tests and doctests reuse the built executables.
- `workflow.crate_timeout_minutes` bounds the one crate job (decision 2).
- The pinned actionlint does not parse the native step keys yet; the narrow,
  documented bridge in `velnor-actions-actionlint` (`STEP_PARALLELISM_IGNORES`)
  suppresses only the unknown `background` key and the body-less join step.

**Must not be reintroduced without owner approval:** per-phase crate jobs
(`-lint`, `-tests`, `-docs` or any other split of one crate's checks across
jobs), a serial chain of the check steps, quality gates that make one check
wait for another, shell backgrounding (`&`) instead of native step
concurrency, or concurrent Cargo writers sharing one target directory.

## 2. Configurable crate-job timeout

**Decision.** `[workflow] crate_timeout_minutes` sets the literal
`timeout-minutes` of every crate job (each Rust crate job and each OpenTofu
root job). The default is `5`; accepted values are `1..=360`. Zero or more than
360 is rejected, never clamped. See
[job timeout limits](implemented/job-timeout-limits.md).

## 3. Commit trailers

**Decision.** Commits use the Claude co-author trailer
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. No repository rule,
ruleset, check, or hook may require any other agent trailer. A human sign-off
(`Signed-off-by`) may be added but must not be required.

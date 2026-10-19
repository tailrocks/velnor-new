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
  Required still demands every task report and every crate job the plan
  schedules; a crate job the plan marks `not_affected` must be skipped
  (decision 4).
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

## 4. Pull requests test only what they affect

**Decision.** On `pull_request` (fork pull requests included), the plan
classifies every crate job as `run` or `not_affected` from the pull request's
merge-base diff, and a `not_affected` crate job is skipped as a whole job
through a job-level `if:` on the plan's `not_affected_jobs` output. Touching one
library used by one app runs exactly those two crate jobs; touching one app runs
only that app's job. `push`, `merge_group`, and `workflow_dispatch` run every
job. The baseline-coverage skip path (trusted-baseline artifacts and
`covered_tasks` step gates) is removed completely; it is not a second skip
mechanism. See [affected selection](implemented/affected-selection.md).
Approved by the human owner on 2026-10-19, together with the decision 1
amendment ("every crate job the plan schedules").

**Rationale.** In a monorepo most pull requests touch one or two crates, but
every crate job paid runner start, checkout, tool setup, cache restore, and a
build. Baseline coverage never skipped anything in practice: it needed a green
default-branch run at the exact merge base, and it still started every crate
job to skip steps inside it.

**Invariants.**

- Fail closed: any doubt selects everything. Unknown or unowned files, Git
  failures, a missing base, untracked files, any `Cargo.lock` change, root Cargo
  configuration, `.velnor/**` and `.github/**`, a workspace without a complete
  resolved graph, and an owned symlink that escapes, dangles, or loops all run
  every job. Selection never narrows on an error.
- Unowned → all. A changed path is narrowed only when a package directory, a
  scoped Cargo tool config, a workspace root, a declared
  `[[stacks.rust.inputs]]` rule, or a tracked symlink claims it. No `ignore`
  list exists until in-job runtime-read capture exists.
- The dependency graph is the full resolve from
  `cargo metadata --locked --offline --all-features` at head, unioned with base
  and head path edges, so registry crates reach `[patch]`ed and vendored crates.
- Packages with a build script, and sharded archives whose source is not
  content-bound, always run until in-job read verification exists.
- The default branch and the merge queue are always full runs.
- `Required` compares every planned crate job's disposition with its observed
  result: `not_affected` + skipped passes; `run` + skipped fails
  (`planned_job_skipped`); `not_affected` + ran fails (`gate_mismatch`); a
  missing, invalid, or failed plan fails. A planned job is never assumed
  successful from its conclusion alone.
- A base retarget re-plans (`edited` is a `pull_request` trigger type), and a
  partial re-run resolves the plan by the attempt that produced it.

**Must not be reintroduced without owner approval:** baseline-gated skipping or
any other second skip mechanism, hand-written per-job path filters, an `ignore`
list without runtime-read capture, narrowing on any selection error, affected
mode on `push`, `merge_group`, or `workflow_dispatch`, or a `Required` rule that
passes a skipped job the plan scheduled.

## 5. Tool versions come from the project's Mise config

**Decision.** Tool versions come from the project's `mise.toml` (and
`mise.lock`). Generated workflows never copy a project-declared version;
only tools absent from the project config are pinned by Velnor. A Rust
toolchain file (`rust-toolchain.toml`, `rust-toolchain`) counts as a Rust
declaration, because Mise (idiomatic version files) and rustup both read it.

**Mechanism.**

- Generation classifies every catalog tool (Rust, MBX, Nextest, GitHub CLI,
  actionlint, shellcheck, zizmor, OpenTofu, release-plz) as project-declared
  when the first parseable root `mise.toml`/`.mise.toml` `[tools]` table names
  it under any backend spelling (for example
  `"aqua:nextest-rs/nextest/cargo-nextest"` or `core:rust`), or, for Rust,
  when a toolchain file exists. Generated text then carries
  `${VELNOR_TOOL_<NAME>}` instead of the version: shell form in `run:` text
  and task argv, `${{ env.VELNOR_TOOL_<NAME> }}` in `with:`, `env:`, and cache
  keys.
- Every job (or composite action) that reads such a reference gets one
  generated **Resolve project tool versions** step right after its Mise
  setup. It reads each declaring source with
  `mise --no-config --no-env --no-hooks config get`, requires all sources to
  agree on one exact `X.Y.Z`, and appends `VELNOR_TOOL_<NAME>=<version>` to
  `$GITHUB_ENV` (plus `RUSTUP_TOOLCHAIN` where job env used to carry the
  Rust pin). The generated task helper materializes the same variables in
  task argv and env after the manifest and plan bindings match, and fails
  closed on a missing or loose value.
- Tools the project does not declare keep the exact Velnor catalog pin (or
  a `.velnor/config.toml [tools]` override). A `.velnor/config.toml` pin
  that disagrees with a project declaration fails generation.
- Cache identities that bound a version now bind the project config: the
  tools-cache `d` input appends `hashFiles` over `mise.toml`, `.mise.toml`,
  `mise.lock`, `.mise.lock`, and both Rust toolchain files; Cargo-source and
  MBX keys and MBX action inputs read the resolved `env` value. Plan
  identities bind the concrete declared versions while the generated record
  (task digests, argv) stays version-free.
- A pull request that changes a root project tool source (`mise.toml`,
  `.mise.toml`, `mise.lock`, `.mise.lock`, `rust-toolchain.toml`,
  `rust-toolchain`) selects every crate job under hard decision 4, because CI
  runs the versions those files declare.
- Bumping a declared version in `mise.toml` therefore needs no regeneration.
  Adding or removing a catalog tool from the project config still does (it
  changes which commands carry a reference), as does an option change such
  as `mr_boxington = true`.

**Rationale.** The project's own Mise config is what developers run locally;
copying its versions into generated workflows made every bump a
regeneration and let the two drift (a consumer's CI ran Nextest 0.9.148
while its `mise.toml` declared 0.9.146). One source of truth removes that
whole class of drift.

**Security.** CI keeps `--no-config --no-env --no-hooks` (and
`MISE_NO_CONFIG=1`) on every install and exec. GitHub runners set `CI=true`,
under which Mise auto-trusts repository config and evaluates templates such
as `{{ exec(...) }}`; loading a fork's config would therefore run its code.
`mise config get` reads stored TOML values without loading or expanding
the config, so no template, hook, `[env]`, task, plugin, setting
(`url_replacements`), or wrapper from the checkout is evaluated. A fork PR
may change `mise.toml`: that tests its own toolchain, which is expected,
and its only effect is a different exact version of the same tool through
the same backend. The resolver accepts only `[0-9.]` values in exact
`X.Y.Z` shape before writing `$GITHUB_ENV`, so no value can inject another
variable or line; symlinked sources are refused. Composite actions carry
`# zizmor: ignore[github-env]` on that one validated write. Credential-bearing
native security operations keep their qualified fixed pins (GitHub CLI and
Cosign in release publication and preview promotion), so repository config
cannot select those executables.

**Trade-offs.** `mise.lock` checksums are not enforced in CI: with
`--no-config` Mise does not consult the lock (as before this decision). The
lock audit stays generation-time hygiene, and the lock participates in the
tools-cache identity. Project-declared catalog tools need an exact
`X.Y.Z` pin; a loose or templated value is still referenced (never replaced
by a Velnor pin) and the resolver fails the job with the project's value.
A declared version bypasses Velnor's freshness qualification of its catalog
pin.

**Must not be reintroduced without owner approval:** copying a
project-declared version into any generated file, loading the project's
Mise config (or trusting it) in a generated step to resolve versions, or
letting a Velnor pin override a project declaration.

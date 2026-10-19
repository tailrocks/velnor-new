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
  (decision 3).
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

## 3. Pull requests test only what they affect

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
The `complete = true` input rule, which lets a package with an unbounded
read be selected only by its declared inputs, was approved by the human owner
on 2026-10-20 (PR #192 review question; the alternative, running every such
package on every pull request, was declined).

**Rationale.** In a monorepo most pull requests touch one or two crates, but
every crate job paid runner start, checkout, tool setup, cache restore, and a
build. Baseline coverage never skipped anything in practice: it needed a green
default-branch run at the exact merge base, and it still started every crate
job to skip steps inside it.

**Invariants.**

- Fail closed on every detectable doubt. Unknown or unowned files, Git
  failures, a missing base, a merge checkout whose first parent is not the
  pull request's base (`stale_merge_ref`), untracked files, a modified tracked
  file, any `Cargo.lock` change, root Cargo configuration, `.velnor/**` and
  `.github/**`, a workspace without a complete resolved graph, and an owned
  symlink that escapes, dangles, or loops all run every job. Selection never
  narrows on an error.
- Unowned → all, per stack. Rust ownership is computed over the full change
  set: a path is narrowed only when a package directory, a scoped Cargo tool
  config, a workspace root, a declared `[[stacks.rust.inputs]]` rule, or a
  tracked symlink claims it. A path an OpenTofu root owns but no Rust rule
  claims still runs every Rust job; the Tofu split feeds only Tofu root
  selection. No `ignore` list exists until in-job runtime-read capture exists.
- Unverified reads force a package (`undeclared_reads`): a package with a
  build script, or whose tracked `.rs` sources read outside the paths it
  claims (`include_str!`/`include_bytes!`/`include!`/`#[path]`, a
  `CARGO_MANIFEST_DIR` path with a `..` component or `parent()`, a relative
  `..` file-system read), counts as changed on every pull request until it
  declares those inputs (an unbounded read only through a rule with
  `complete = true`), and its dependents run with it. An unchanged sharded
  archive whose source is not content-bound fails the plan instead of running
  alone (unreachable today: the closure digest always binds it).
- Residual risk, named: the read scan is static. A read through a computed
  path with none of those anchors, a spawned process, a non-Rust file, or a
  macro that expands to a read is invisible to it, and a declared input is
  the owner's unverified statement. Pull requests can therefore still skip a
  crate whose undetected read changed; the full runs on `push` and
  `merge_group` are the backstop until in-job read verification exists.
- The dependency graph is the full resolve from
  `cargo metadata --locked --offline --all-features` at head, unioned with base
  and head path edges, so registry crates reach `[patch]`ed and vendored crates.
- The default branch, the merge queue, and qualification dispatches are always
  full runs. `Plan::validate` and `Required` both reject `not_affected` work on
  those events (`not_affected_on_full_event`), whatever the planner did.
- `Required` compares every planned crate job's disposition with its observed
  result: `not_affected` + skipped passes; `run` + skipped fails
  (`planned_job_skipped`); `not_affected` + ran fails (`gate_mismatch`); a
  missing, invalid, or failed plan fails. A planned job is never assumed
  successful from its conclusion alone.
- A cancelled run never satisfies protection. The protected `Required` check
  is the `required-gate` job: it runs under `always()` and fails unless the
  verdict job (`Required verdict`, `!cancelled()`) succeeded, because GitHub
  reports a skipped required check as successful.
- A base retarget re-plans (`edited` is a `pull_request` trigger type), and a
  partial re-run resolves the plan by the attempt that produced it.

**Must not be reintroduced without owner approval:** baseline-gated skipping or
any other second skip mechanism, hand-written per-job path filters, an `ignore`
list without runtime-read capture, narrowing on any selection error, affected
mode on `push`, `merge_group`, or `workflow_dispatch`, a `Required` rule that
passes a skipped job the plan scheduled, a protected check a cancelled run can
leave skipped, one stack's ownership hiding another stack's claims, or a forced
package whose dependents do not run.

## 4. Tool versions come from the project's Mise config

**Decision.** Project configuration declares project tool versions. Generated
workflows do not copy these versions. Velnor pins tools that the project does
not declare. The root files `rust-toolchain.toml` and `rust-toolchain` also
declare Rust versions. Mise and rustup read these files.

**Mechanism.**

- Generation checks every catalog tool against every project source that
  Mise reads. A source declares a tool when its `[tools]` data names that tool
  under any backend spelling. Sources include `mise.toml`, `.mise.toml`,
  `mise.<env>.toml`, `mise.local.toml`, their dot-prefixed forms,
  `.config/mise.toml`, `.config/mise/config.toml`,
  `.config/mise/conf.d/*.toml`, `mise/config.toml`, `.mise/config.toml`, and
  `.tool-versions`. Rust sources also include `rust-toolchain.toml` and
  `rust-toolchain`. The shared `PROJECT_MISE_CONFIG_PATTERNS` list controls
  generation, run-time resolution, and change selection.
- Generated text uses `${VELNOR_TOOL_<NAME>}` for each declared version.
  The `run:` text and task arguments use the shell form. The `with:`, `env:`,
  and cache key values use `${{ env.VELNOR_TOOL_<NAME> }}`.
- Generation stops and names the source when a declaration is unusable.
  It stops for a loose version, `latest`, a range, a template, an array, a
  target-triple channel, or a leading zero. It reports
  `project_tool_version_not_exact`. It reports
  `conflicting_project_tool_versions` when sources disagree. It reports
  `malformed_project_tool_source` when a source does not parse. This includes
  malformed `mise.toml` beside valid `rust-toolchain.toml`. It also stops for
  a symlinked source. It reports `nested_rust_toolchain_not_supported` when
  a Rust toolchain file below the root has another version. Rustup would use
  that file for commands in its directory. The resolver reads root sources.
  A Velnor pin never replaces a declaration that the generator cannot use.
- Each job or composite action that uses a project version gets one
  **Resolve project tool versions** step after Mise setup. It reads each
  source with `mise --no-config --no-env --no-hooks config get`. It reads
  `.tool-versions` and plain-text `rust-toolchain` as text. It does not run
  these files. The step rejects unparseable and symlinked sources. It requires
  all declarations for a tool to use the same exact `X.Y.Z` version, without
  leading zeros. It writes `VELNOR_TOOL_<NAME>=<version>` to `$GITHUB_ENV`.
  It also sets `RUSTUP_TOOLCHAIN` when the job environment used the Rust pin.
  The task helper sets the same variables in task arguments and the
  environment after the manifest and plan bindings match. It stops when a
  value is missing or loose.
- Tools that the project does not declare keep the exact Velnor catalog pin or
  a `.velnor/config.toml [tools]` override. Generation fails if that override
  disagrees with a project declaration.
- Cache identities that bind a version also bind project config. The
  tools-cache `d` input hashes `mise.toml`, `.mise.toml`, `mise.lock`,
  `.mise.lock`, and both Rust toolchain files. This list does not yet include
  every source listed above. The cache design owns this list. Cargo-source and
  MBX keys and MBX action inputs use the resolved value. Plan identities,
  including the MBX cache-format generation, bind declared versions. Generated
  records (task digests and arguments) do not include those versions.
- A pull request selects every crate job when a project source changes a
  declared catalog version. Change selection parses each changed source at
  base and head with the generator's reader. It compares declared versions,
  not file contents. A task edit, Node pin, or comment in `mise.toml` alone
  selects no crate job. A lock file (`mise.lock`, `.mise.lock`) always selects
  every crate job. Any doubt selects every crate job. This includes a parse
  error or a Git read error for either side.
- A declared version change in `mise.toml` does not need regeneration. Adding
  or removing a catalog tool does need regeneration because it changes which
  commands use a project reference. An option change, such as
  `mr_boxington = true`, also needs regeneration.

**Rationale.** Developers use the project's Mise config on their computers.
Copying its versions into generated workflows forced regeneration after each
version change. It also caused version drift. For example, consumer CI ran
Nextest 0.9.148 while `mise.toml` declared 0.9.146. One source prevents this
drift.

**Security.** CI uses `--no-config --no-env --no-hooks` for every install and
exec command. CI also sets `MISE_NO_CONFIG=1`. GitHub runners set `CI=true`.
Mise then trusts repository config and evaluates templates such as
`{{ exec(...) }}`. A fork config could therefore run code.

`mise config get` reads stored TOML values. It does not load or expand the
config. It does not evaluate templates, hooks, `[env]`, tasks, plugins,
settings such as `url_replacements`, or wrappers from the checkout. A fork
pull request can change `mise.toml`. CI then tests the declared version of
the same tool through the same backend.

The resolver accepts only `[0-9.]` values in exact `X.Y.Z` form. It checks
each value before it writes to `$GITHUB_ENV`. This prevents a value from
adding a variable or line. The resolver also rejects symlinked source files.
It does not write raw values to logs. It replaces control text, such as
`::error::`, before it reports an error. Composite actions use
`# zizmor: ignore[github-env]` only on the validated write.

The GitHub CLI uses a token for baseline lookup, required-report fetch, and
release publication. Cosign signs releases. Velnor keeps its qualified pins
for both tools, even when the project declares them. Project config cannot
select these credential-bearing executables.

A job without checkout cannot read project config. It does not install Rust.
The generator removes its hoisted `RUSTUP_TOOLCHAIN` reference from that job.
Any other project version reference without checkout fails generation.

**Trade-offs.** CI does not check `mise.lock` checksums. Mise does not read
the lock with `--no-config`. The lock audit runs during generation. The lock
still affects the tools-cache identity.

Project-declared catalog tools need an exact `X.Y.Z` version. The generated
text keeps a loose or templated value as a project reference. The resolver
then stops the job because the value is not exact. A project version skips
Velnor's freshness check for the matching catalog pin.

**Do not reintroduce these rules without owner approval.**

- Do not copy a project-declared version into a generated file.
- Do not load or trust project Mise config in a generated resolver step.
- Do not let a Velnor pin override a project declaration.

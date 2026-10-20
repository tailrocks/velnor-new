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
sources and the compiled dependency cache once and builds the test executables
once. Format,
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
  (`target/velnor-clippy`, `target/velnor-doc`, seeded from the
  dependency-seed family) so Cargo's target-directory lock never serializes test execution
  and doctests behind them. Tests and doctests reuse the built executables.
- `workflow.crate_timeout_minutes` bounds the one crate job (decision 2).
- The pinned actionlint does not parse the native step keys yet; the narrow,
  documented bridge in `velnor-actions-actionlint` (`STEP_PARALLELISM_IGNORES`)
  suppresses only the unknown `background` key and the body-less join step.

- The dependency-seed job is not a crate job: it runs no checks and is not in
  Required, so this decision governs crate jobs only. Crate jobs restore
  compiled caches read-only. See the
  [cache decisions](proposed/cache-decisions.md) (owner-approved 2026-10-20).

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

**Decision.** Tool versions come from the project's `mise.toml` (and
`mise.lock`). Generated workflows never copy a project-declared version;
only tools absent from the project config are pinned by Velnor. A Rust
toolchain file (`rust-toolchain.toml`, `rust-toolchain`) counts as a Rust
declaration, because Mise (idiomatic version files) and rustup both read it.

**Mechanism.**

- Generation classifies every catalog tool (Rust, MBX, Nextest, actionlint,
  shellcheck, zizmor, OpenTofu, release-plz) as project-declared
  when **any** project source names it under any backend spelling (for
  example `"aqua:nextest-rs/nextest/cargo-nextest"` or `core:rust`). The
  sources are every location Mise reads `[tools]` from at the project root:
  `mise.toml`, `.mise.toml`, `mise.<env>.toml` and `mise.local.toml` (and
  their dot-prefixed forms), `.config/mise.toml`, `.config/mise/config.toml`,
  `.config/mise/conf.d/*.toml`, `mise/config.toml`, `.mise/config.toml`, and
  `.tool-versions`; for Rust also `rust-toolchain.toml` and `rust-toolchain`
  (plain channel or TOML form). One list
  (`PROJECT_MISE_CONFIG_PATTERNS`) drives generation, the run-time resolver,
  and change selection. Generated text then carries
  `${VELNOR_TOOL_<NAME>}` instead of the version: shell form in `run:` text
  and task argv, `${{ env.VELNOR_TOOL_<NAME> }}` in `with:`, `env:`, and cache
  keys.
- Generation fails, naming the tool and the file, instead of guessing: a
  declared version that is not an exact `X.Y.Z` (loose, `latest`, a range, a
  template, an array, a target-triple channel, a leading zero:
  `project_tool_version_not_exact`), two sources that disagree
  (`conflicting_project_tool_versions`), a source that does not parse
  (`malformed_project_tool_source`, including a malformed `mise.toml` next to
  a valid `rust-toolchain.toml`), a symlinked source, and a Rust toolchain
  file below the root that declares anything but the root version
  (`nested_rust_toolchain_not_supported`: rustup would honor it for commands
  run in that directory while the resolver reads root sources only). A Velnor
  pin never replaces a declaration the generator cannot use.
- Every job that reads such a reference gets one generated **Resolve project
  tool versions** step after both its Mise setup and source checkout,
  immediately after the later prerequisite and ahead of tools-cache identity
  and restore. A composite action resolves after its own Mise setup, or first
  in a shared-lane composite whose callers provide Setup Mise and checked-out
  source. It does not add a checkout. It reads every source above with
  `mise --no-config --no-env --no-hooks config get` (`.tool-versions` and a
  plain-text `rust-toolchain` are read as text, never executed), fails on an
  unparseable or symlinked source, requires all declarations of a tool to
  agree on one exact `X.Y.Z` (no leading zeros), and appends `VELNOR_TOOL_<NAME>=<version>` to
  `$GITHUB_ENV` (plus `RUSTUP_TOOLCHAIN` where job env used to carry the
  Rust pin). The generated task helper materializes the same variables in
  task argv and env after the manifest and plan bindings match, and fails
  closed on a missing or loose value.
- Tools the project does not declare keep the exact Velnor catalog pin (or
  a `.velnor/config.toml [tools]` override). A `.velnor/config.toml` pin
  that disagrees with a project declaration fails generation.
- Cache identities that bound a version now bind the resolved value: the
  tools-cache `d` input appends, for exactly the declared tools the job
  installs, `${{ env.VELNOR_TOOL_<NAME> }}` and `${{
  env.VELNOR_TOOL_<NAME>_LOCK }}` (the SHA-256 of that tool's `mise.lock`/
  `.mise.lock` `checksum`/`url` rows for the runner platform, read by a
  fixed text parser, or `unlocked`), never `hashFiles` of whole files. The
  resolved version already reflects every source above, so a bump in any of
  them changes the key, while editing a task, a comment, or another tool
  leaves the tools cache warm. Cargo-source and MBX keys and MBX action
  inputs read the resolved `env` value. Plan identities, including the MBX
  cache-format generation, bind the concrete declared versions while the
  generated record (task digests, argv) stays version-free.
- A pull request selects every crate job when a project tool source changes
  a **declared catalog version**: change selection parses each changed
  source at base and head with the generator's own reader and compares the
  declared versions, not the files. Editing a task, a Node pin, or a comment
  in `mise.toml` selects nothing on its own. A lock file (`mise.lock`,
  `.mise.lock`) always selects everything, and any doubt (a source that does
  not parse, git unable to read either side) selects everything.
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
`X.Y.Z` shape (and lock digests only as 64 hex characters or `unlocked`)
before writing `$GITHUB_ENV`, so no value can inject another variable or
line; symlinked sources and lock files are refused, and a value is never
echoed raw into the log (control text such as `::error::` is replaced). Composite actions carry
`# zizmor: ignore[github-env]` on that one validated write. The GitHub CLI
runs with a token in every generated use (plan baseline lookup, `Required`
report fetch, release publication) and Cosign signs releases, so both keep
their qualified fixed pins even when the project declares them: repository
configuration never selects a credential-bearing executable. A job without a
checkout cannot read the project config; such a job installs no Rust, so a
hoisted `RUSTUP_TOOLCHAIN` reference is dropped there and any other
reference fails generation.

**Trade-offs.** `mise.lock` checksums are not enforced in CI: with
`--no-config` Mise does not consult the lock (as before this decision). The
lock audit stays generation-time hygiene, and each installed tool's lock
entries participate in the tools-cache identity. Project-declared catalog tools need an exact
`X.Y.Z` pin; a loose or templated value is still referenced (never replaced
by a Velnor pin) and the resolver fails the job with the project's value.
A declared version bypasses Velnor's freshness qualification of its catalog
pin.

**Must not be reintroduced without owner approval:** copying a
project-declared version into any generated file, loading the project's
Mise config (or trusting it) in a generated step to resolve versions, or
letting a Velnor pin override a project declaration.

## 7. Private dependency credentials

**Decision.** A GitHub App installation token (`contents: read`, an exact
repository allowlist derived from the Cargo.lock git sources and
`.velnor/deps.lock`) is minted, used and **revoked** inside Velnor-owned
fetch steps before any repository-controlled step runs. `GITHUB_TOKEN` with
`packages: read` is used only in the registry phase of
`bun install --ignore-scripts`. No credential reaches Gradle, Cargo build
scripts, Bun lifecycle scripts or Mise tasks. Maven artifacts for private
consumers ship as release-asset bundles fetched with the App token, because
GitHub Packages Maven cannot serve other private repositories with a scoped
credential. Specification:
[private sources contract](proposed/private-sources-contract.md).

**Status.** Adopted per the split brief; owner may revisit. Recorded
2026-10-20 together with the release-asset Maven bundle choice. The
`velnor-deps` App does not exist yet; creating it is an owner blocker.

**Rationale.** Without it a private sibling dependency forces a long-lived
broad credential (a machine-user classic PAT or per-producer deploy keys)
into jobs that run repository code. A token that is scoped to the exact lock
closure and dead before repository code starts leaves a compromised
dependency or build script nothing to steal.

**Invariants.**

- The token never coexists with repository-controlled code: mint, fetch and
  revoke form one block ahead of every such step.
- Only the `PrivateFetch` credential policy keeps
  `VELNOR_PRIVATE_FETCH_TOKEN` and `VELNOR_NPM_READ_TOKEN`; every other
  policy strips every `*_TOKEN`.
- Fork pull requests and `pull_request_target` never receive the token;
  an unavailable credential fails the job closed.

**Must not be reintroduced without owner approval:** a long-lived or
repository-wide credential for dependency fetch (classic PAT, deploy keys),
a credential binding in any step other than the fetch step, a fetch that
runs build scripts or repository hooks while the token is live, or
`pull_request_target` to obtain secrets.
## 8. Publication only from the default branch or a release tag

**Decision.** Container images (and later packages) are published only from a
`push` to the default branch or from a release tag, after `Required` succeeded
for that SHA, by jobs that check out and execute no repository code. Tags
promote the digest built and attested on the default branch; a tag never
rebuilds an image. Pull requests build and validate images but hold neither
`packages: write` nor `id-token: write`.

**Rationale.** Publication is a new authority boundary. Keeping every
repository-controlled step (Dockerfile `RUN`, prebuilt compilation) in
unprivileged jobs, and consuming only same-run artifacts with verified digests
in the privileged ones, keeps provenance bound to one source SHA and workflow.

**Status.** Adopted per the split brief; owner may revisit. Specification:
[image publication contract](proposed/image-publication-contract.md) and
[package publication contract](proposed/package-publication-contract.md).

**Image jobs placement.** The privileged image jobs live in `publish.yml`, the
workflow whose IR owns the authority boundary, and consume the artifacts of the
push's successful `ci.yml` run (same commit, same event) rather than same-run
artifacts; `ci.yml` keeps its closed permission set and credential-free steps.
Adopted per the split brief; owner may revisit.

**Tag flow and release-asset bundles.** The same boundary covers release
assets. The trigger set is closed in the typed IR: tag pushes only for
`publish.yml`, with no `pull_request`, `pull_request_target`,
`workflow_run`, `workflow_dispatch` or `schedule` variant, so no config field
can add one. Every publication job transitively needs one authority job
(`contents: read`, `checks: read`, no checkout) that verifies the tag is
annotated, targets the pushed commit, is reachable from the default branch,
has a green `Required` check run from GitHub Actions at that SHA, names a
declared artifact and carries the manifest version the tag claims. Release
bundles are the one tag-time build: a deterministic archive (sorted entries,
zero mtimes, root owner) of files at a commit reachable from the default
branch, built by a read-only job that runs no repository code. Roles are
fixed (build, attest with OIDC and attestations, publish with
`contents: write` in the protected environment, the last two without
checkout) and permissions equal the role matrix exactly. Release assets are
immutable: an existing asset fails the job. The concurrency group is one
never-cancelled queue per pushed ref, because GitHub keeps one pending run
per group and a per-artifact group would silently drop a queued release.

**Release-asset Maven bundle choice.** Adopted per the split brief; owner may
revisit. GitHub Packages Maven is repository-scoped and cannot serve private
cross-repository consumers with a scoped CI credential, so Java libraries
ship as release-asset Maven bundles consumed through the same App-token fetch
as Cargo git dependencies. Rejected: a machine-user classic PAT in every
consumer.

**Must not be reintroduced without owner approval:** publishing from a pull
request or fork event, a privileged job that checks out or runs repository
code, a release tag that rebuilds instead of promoting, BuildKit cache
backends and mounts that hand a credential to the build, or overwriting or
deleting a published release asset or moving a published tag.

**Owner-approved rolling preview exception (2026-10-20).** The consumer native
preview channel reuses one explicitly qualified existing mutable `preview`
release and advances its lightweight tag only by a normal descendant update.
It may replace that channel's generation-owned assets through the authenticated
rolling transaction. Repository immutability remains enabled. This exception
never permits deleting/recreating/redrafting/republishing the preview release,
force-moving a tag, changing permanent stable releases, or weakening image and
release-bundle publication. See the [native channel
contract](proposed/native-preview-contract.md); no other publication rule above
is changed.

**Manual stable version and tag authority (2026-10-20).** The owner requires
existing release-plz version management and a permanent stable version tag before
exact-tag binary builds. The fixed qualified manual preparation phase may inspect
version metadata and Git state to propose the version/changelog PR, with native
config disabling tag/release/registry publication. It must not run repository
tasks, builds, hooks or repository-selected release-plz configuration. Permanent
tag creation uses native source-free exact-SHA authority after durable pre-tag
intent and protected human approval. This removes release-plz's mutable
associated-PR source selection from irreversible tag mutation. Tag, signer and
publisher jobs remain source-free; no general privileged checkout or execution
is authorized. The release is published once with complete signed assets and
stays immutable. See [manual stable coordination](proposed/binary-release-contract.md).

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

## Scope

These decisions bind the existing engine in `crates/`, its configuration in
`.velnor/`, the workflows it generates, and the scripts, images and fixtures
that support it. They do not bind the new Rust generator: [the goals](goals.md) are its rule book and state which code each
rule book governs.

## 4. Tool versions come from the project's Mise config

**Decision.** Tool versions come from the project's `mise.toml` (and
`mise.lock`). Generated workflows never copy a project-declared version;
only tools absent from the project config are pinned by Velnor. A Rust
toolchain file (`rust-toolchain.toml`, `rust-toolchain`) counts as a Rust
declaration, because Mise (idiomatic version files) and rustup both read it.

**Mechanism.**

- Generation classifies every catalog tool (Rust, MBX, Nextest, jactionlint,
  shellcheck, OpenTofu, release-plz) as project-declared
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
  (`conflicting_project_tool_versions`; every `rust-toolchain.toml` or `rust-toolchain`, root or nested, must also mirror every Mise config's rust entry in `components`, `targets` and `profile`, `rust_toolchain_mirror_mismatch`, adopted with the owner's standard layout 2026-10-22, owner may revisit), a source that does not parse
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

## 6. One job per Bun workspace member with parallel script steps

**Decision.** Each Bun workspace member owns exactly one generated CI job named
`Bun / <package name>` (job id `bun-<slug>`, for example `Bun / @chainargos/ui`). A
single-package root (no `workspaces`) is one member. The job prepares checkout, pinned tools
and the dependency install once; the member's `package.json` scripts (`typecheck`, `lint`,
`test`, `build` by default) then run as parallel steps (`background: true` members joined by
one `wait-all`). The only accepted JavaScript lockfile is the text `bun.lock`.
Specification: [Bun contract](proposed/bun-contract.md).

**Status.** Adopted per the split brief; owner may revisit. It extends decision 1 to a new stack.
Suites: coordinator decision, 2026-10-22, under the owner's autonomy rule
([record](proposed/bun-browser-suites.md#decision-record)); owner may revisit.

**Rationale.** Decision 1's rationale holds for Bun: checkout, tool setup and `bun install`
are paid once per job and the independent checks overlap. A workspace member is the unit
whose scripts share one `node_modules` view and whose affected selection is exact (its
directory plus its `workspace:` dependents), so a one-member change does not run the other
members. Foreign lockfiles (`package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, binary
`bun.lockb`) fail generation instead of adding a second package-manager adapter.

**Invariants.**

- Repository scripts never see a credential. The registry phase of `bun install --ignore-scripts`
  is the only step that holds `GITHUB_TOKEN` with `packages: read` (decision 7), and it reads a
  Velnor-written `bunfig.toml`, never the repository's registry settings.
- Generation fails on a repository `bunfig.toml` that sets a registry, scope, token or
  certificate (`bunfig_registry_override`).
- Bun suites are not member jobs: declared `suites` run the scripts they claim in their own
  jobs (`bun-<slug>--<suite>`, optionally a Playwright shard matrix) beside the one member
  job, as Rust suites sit beside crate jobs. No suite means byte-identical output.

**Must not be reintroduced without owner approval:** a second package-manager adapter, one shared
job for several members, scripts run before the install credential is gone, a repository-controlled
registry in the install phase, or a split of a member's scripts across jobs other than `suites`.

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
`velnor-deps` App exists (id 5394413); Bun jobs pin release assets too.

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

**Release-asset Maven bundle choice.** See the preserved decision in
[release publication decisions](release-publication-decisions.md).

**Must not be reintroduced without owner approval:** publishing from a pull
request or fork event, a privileged job that checks out or runs repository
code, a release tag that rebuilds instead of promoting, BuildKit cache
backends and mounts that hand a credential to the build, or overwriting or
deleting a published release asset or moving a published tag.

**Native release channel decisions (2026-10-22).** Immutable per-source preview
publication and manual stable version/tag authority are recorded in
[release publication decisions](release-publication-decisions.md).

**Gradle stack decisions (HD-4a adapter version grammars, HD-5 one job per Gradle project) and the release-asset Maven bundle choice** are recorded in [Gradle stack contract](proposed/gradle-contract.md#decisions); adopted per the split brief, owner may revisit. **Host deployment (HD-9)** stays operator-run, outside generated CI/CD: [host deployment decision](host-deployment-decision.md); adopted per the split brief, owner may revisit. **Workflow linter (hard decision 10)**: jactionlint replaces actionlint completely and is the only workflow linter, per the owner's instruction of 2026-10-22, and also retires the separate zizmor job: [jactionlint decision](jactionlint-decision.md).

## 11. Compile steps build the whole workspace, not the selected packages

**Decision.** In generated Rust CI (velnor-gen), lint, the test build, the
nextest archive, doctests and `cargo doc` run with `--workspace` instead of
`-p` for the selected packages. Tests still run only the selected packages,
filtered with `-E`. Alone-checks, docs and doctests are still planned from the
selection.

**Cause.** Cargo unifies features only over the selected packages. A `-p` run
therefore builds shared dependencies with a different feature set than the
workspace-wide build, so it rebuilds them and breaks the cache (reproduced on
cargo 1.99.1; `velnor-lab` `leaf` compiled `hex`, `lib` and `app` instead of
`app`).

**Stable-cargo constraint.** `resolver.feature-unification = "workspace"`
would fix it but needs `-Zfeature-unification`, which stable cargo ignores.

**Costs.** Doctests of every crate run whenever any selected crate has
doctests, because cargo never caches them. A cold or evicted cache builds the
whole workspace, not the selected closure. Lint covers unselected crates, so a
warning there fails an unrelated pull request.

**Goals.** This bends [goals.md](goals.md) promise 1 ("A run checks only what
it selects") for doctests and lint scope. No number in the goals changes and
`docs/goals.md` is not edited.

**Status.** Adopted; owner may revisit. Revisit when
workspace feature unification stabilizes, then restore `-p` everywhere.

# PR #1 review feedback proposal

PR: [#1](https://github.com/tailrocks/velnor-new/pull/1) · Actions run: [36574247809](https://github.com/tailrocks/velnor-new/actions/runs/36574247809/job/109426247322?pr=1)

## Purpose and scope

This is the living record of review feedback for PR #1. Add future feedback to the relevant group below, or create a new group when needed. Research the project and its conventions when that helps clarify a point.

This proposal records expected behavior and review criteria. It does not authorize implementation. Work in this thread is limited to analyzing the PR and editing this review document; do not change source code, workflows, configuration, tests, or other project files.

## 1. CI job graph

### Feedback

The generated workflow has far too many jobs. It creates separate jobs for individual task obligations; these are not useful human-facing CI jobs.

### Expected behavior

For this repository, generate one Rust job per discovered crate: seven Rust jobs for the seven crates. Group/tag the crate jobs as Rust, for example `Rust / velnor-actions-cli`. Also generate a separate job for each repository-wide validator, including Alint and Actionlint, plus other global checks required by the repository.

Each crate job contains the relevant checks as steps:

1. Format with `cargo fmt`.
2. Lint with `cargo clippy`.
3. Test with the runner detected for that project.

Each repository-wide validator runs in its own job with an intuitive name. Keep `Alint`, `Cargo Deny`, `Cargo Machete`, and `Actionlint` as four distinct jobs. If the workflow also runs other independent validators, such as Zizmor, give each one its own job too. Do not combine these tools under umbrella jobs such as `Policy` or `Workflow Lint`; each tool checks a different concern and must appear independently in CI.

Do not create separate visible jobs for crate-scoped build, doc, doctest, format, clippy, nextest, or other individual task obligations. Keep each crate's checks together in its crate job. The expected graph is seven Rust crate jobs plus one distinct job for every repository-wide validator; it is not limited to eight jobs when the repository has more global checks.

### Workflow file name

The generated path `.github/workflows/velnor.yml` exposes the generator's product name in the consuming repository. Name the main consolidated CI workflow `.github/workflows/ci.yml` instead. Keep the workflow's displayed name `CI`, which it already uses.

GitHub requires workflow files to live in `.github/workflows/` and use `.yml` or `.yaml`; it does not mandate a basename. GitHub's official starter workflows use descriptive purpose or stack names, including [`rust.yml`](https://github.com/actions/starter-workflows/blob/main/ci/rust.yml). For this repository's single workflow that runs the CI checks, use the familiar concise `ci.yml` name. If workflows are later split by purpose, name each after its responsibility, such as `release.yml`.

Do not include `velnor` or another generator/vendor name in generated workflow filenames.

Sources: [GitHub workflow syntax and file requirements](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#about-yaml-syntax-for-workflows) and [GitHub's official starter workflow examples](https://github.com/actions/starter-workflows/tree/main/ci).

### Alint configuration

Configure Alint with the repository's OSS, GitHub Actions, Rust, lockfile, and tracked-artifact baselines. Also enforce Rust edition 2024 in every crate manifest:

```yaml
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/ci/github-actions@v1
  - alint://bundled/rust@v1
  - alint://bundled/ci/github-actions@v1
  - alint://bundled/hygiene/lockfiles@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1

rules:
  # Lock every Cargo.toml to edition 2024.
  - id: rust-edition-2024
    kind: toml_path_equals
    paths: "crates/*/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: error
```

### Per-job caching

Every job that installs tools or downloads/builds dependencies must have an explicit cache strategy appropriate to its toolchain. Restore and save its cache where the workflow trust policy permits. Jobs run on isolated runners, so a cache restored or populated by another job in the same workflow run is not available unless explicitly persisted through the cache backend.

For Rust crate jobs:

- Keep `jdx/mise-action` caching enabled for Mise-managed tools, but do not treat that as the Cargo cache. Mise setup caching does not replace caching Cargo's registry and compiled `target` artifacts.
- When MBX is detected, set up the pinned Rust toolchain first, then use the pinned `jdx/mr-boxington-action` in every crate job. Choose the cache backend and payload deliberately; seven independent crate jobs make the size and duplication tradeoff important. Do not assume the action's `target` default is the right setting for the whole matrix.
- Put MBX cache setup before every Cargo fetch/build/test command in that job. The current workflow runs `cargo fetch --locked` before the MBX action, so the later cache restore cannot prevent those downloads.
- Decide between MBX's `target` and `objects` GitHub cache modes using measured archive sizes and warm-run timings. `target` includes a pruned Cargo `target` tree and registry, which can speed a stable workspace rerun but can repeat shared dependency artifacts and registry contents in every crate cache. `objects` is a smaller, more portable compiler-artifact payload, but omits Cargo's registry; use it only with an explicit shared registry strategy. Separate per-crate `objects` archives can still duplicate common compiler objects.
- Ensure the cache includes the Cargo home actually used by the build. The current generated workflow sets `MISE_CARGO_HOME` to `$RUNNER_TEMP/velnor/cargo`; a cache of only the default `~/.cargo` path will not warm that separate directory.
- Do not use seven crate-specific keys for a payload whose purpose is to be shared. GitHub cache entries are immutable; parallel jobs that miss on the same key cannot update a common entry independently. Give each cache layer one clear owner/write policy. If using per-crate `target` archives, keys must distinguish crates to prevent one crate from restoring another crate's incomplete/different tree, and their total size must be measured. If using a shared registry snapshot, seed/save it once and let crate jobs restore it; do not have seven parallel jobs race to save the same key. Include platform/architecture and Rust/Cargo configuration in the key or restore boundary, and use compatible restore prefixes only when the restored content remains safe to reuse.
- For Rust jobs without MBX, use an appropriate Cargo cache such as [`Swatinem/rust-cache`](https://github.com/Swatinem/rust-cache), pinned by full commit SHA. Do not stack it over the same Cargo `target` and registry paths already cached by MBX; duplicate caches add restore/save work.
- Global validator jobs should cache their own installed tools/dependencies where those tools support it. Do not attach Rust build caches to unrelated jobs.
- On a compatible cache restore that contains all locked dependencies, skip online `cargo fetch` and run Cargo in offline mode. Treat a miss or missing locked source as a legitimate cold-cache path that can fetch and populate the cache; do not issue `cargo fetch` unconditionally before attempting the restore.

The linked run demonstrates the gap: its task job runs `cargo fetch --locked` before the MBX cache action, puts Cargo home under `$RUNNER_TEMP`, and selects `github-cache-mode: objects` in [the generated workflow](../../.github/workflows/velnor.yml#L313). The pinned [MBX v1.5.0 action documentation](https://github.com/jdx/mr-boxington-action/blob/9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3/README.md) says object mode omits the Cargo registry. Its [pinned action inputs](https://github.com/jdx/mr-boxington-action/blob/9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3/action.yml) expose custom cache keys and default to the target payload. The [current MBX GitHub Action guide](https://mr-boxington.jdx.dev/github-action) recommends the target archive for a stable workspace layout and documents scoped same-repository PR cache saving; the currently pinned v1.5.0 action does not expose that PR-save input. Cargo's [CI caching guidance](https://doc.rust-lang.org/cargo/guide/cargo-home.html#caching-the-cargo-home-in-ci) describes the registry and Git directories needed to avoid redownloading sources; GitHub documents cache key matching and immutable cache entries in its [dependency caching guide](https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/caching-dependencies-to-speed-up-workflows).

### Cache duplication and size budget

The goal is one stored copy of each reusable cache layer, not the same files saved under role-specific or per-crate keys. GitHub-hosted jobs have separate runners, so a shared GitHub cache entry is still downloaded independently by every job that restores it; sharing the key reduces stored copies and upstream downloads, but does not eliminate per-job transfer. A remote content-addressed cache can avoid storing full overlapping archives and transfer only relevant cached results, but jobs still need those results locally. GitHub cache keys are immutable, and its documented default repository limit is 10 GB; entries not accessed for more than seven days are eligible for eviction, with least-recently-used eviction once the limit is exceeded. Many large immutable snapshots can therefore create cache churn and make later runs redownload or rebuild dependencies. See [GitHub's cache limits and eviction policy](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy).

#### Current cache evidence

A read-only `gh cache list --repo tailrocks/velnor-new --limit 100` on 2026-09-30 returned five Mise tool caches: `velnor-final` (60.08 MiB), `velnor-task` (52.02 MiB), `velnor-plan` (65.83 MiB), `velnor-policy` (66.85 MiB), and `velnor-workflow-lint` (51.57 MiB), about **296.35 MiB total**. Their keys have the same platform, Mise version, and Mise configuration inputs, but different job-role suffixes; the generated workflow saves the same `~/.local/share/mise` path under those keys. This is direct evidence of role-based cache duplication. The exact archive contents were not inspected, so the total is cache storage reported by GitHub, not a byte-for-byte estimate of identical files.

That cache listing showed no Cargo registry, Cargo `target`, or MBX GitHub cache entries. In the linked run, the unconditional `cargo fetch --locked` occurs before MBX restore, and MBX `objects` does not include the registry. Thus the current setup spends time downloading Cargo sources and has no visible reusable Cargo registry cache in the repository's listed GitHub caches.

#### Cache layers and options

| Layer / approach | Reuse behavior | Storage and speed tradeoff |
| --- | --- | --- |
| Mise tools | Enable `jdx/mise-action`'s built-in cache and use the same compatible cache identity in jobs with the same Mise inputs. Remove role-specific custom `mise-tools-*` entries and avoid a second cache over the same Mise directory. | Stores one compatible snapshot instead of one per job role. Each runner still restores its own copy; benchmark restore time and bytes transferred as well as storage. |
| Cargo registry | Keep registry/index and downloaded crate sources in one shared Cargo-home cache, keyed by platform and relevant Cargo/Rust configuration. Restore it in all Rust jobs; give one controlled writer responsibility for seeding a new immutable snapshot. Point it at the actual `MISE_CARGO_HOME`, not an unused default Cargo home. | Prevents each job from independently fetching the same registry packages from crates.io and stores one shared snapshot, but all seven runners still download that snapshot from the cache. Avoid caching all of Cargo home blindly: Cargo documents a sufficient subset (`.crates.toml`, `.crates2.json`, `bin/`, `registry/index/`, `registry/cache/`, and `git/db/`) because registry archives and extracted sources can otherwise be duplicated. |
| MBX GitHub `target` mode | Restore/save a per-crate target payload, as supported by the pinned action. | Simplest warm rerun for one stable job; also includes registry content. Across seven crate jobs, common dependency builds and registry data can be stored repeatedly. Use only if measured archive sizes, transfer time, and eviction history show the total remains comfortably within the repository budget. |
| MBX GitHub `objects` mode + shared registry | Use MBX object payloads for compiler outputs and a separate, shared Cargo registry cache. | The object payload is smaller and portable and omits registry data, so a separate registry cache is required. Per-crate object archives can still repeat common dependency objects; measure their combined size and restore time. |
| MBX remote cache + shared registry | Point all crate jobs at a common MBX remote cache for compiler action objects; separately reuse Cargo registry sources. | MBX's content-addressed remote cache shares compiler results across ephemeral runners without storing a full overlapping GitHub archive for each crate; each job downloads only the results it needs. A cache server adds infrastructure, authentication/permissions, and operational cost; an S3-compatible object store is simpler but does not provide the server's batching/in-flight deduplication behavior. Keep untrusted PRs read-only. |

MBX itself does **not** replace Cargo registry caching. Its GitHub `target` payload can include registry data, while its `objects` payload and remote compiler cache do not. Its documented comparison also warns against configuring multiple cache actions to save the same files. Therefore do not combine MBX `target` mode with `Swatinem/rust-cache` or another archive over the same target/registry paths. For the seven-crate fan-out, the best candidate to benchmark is a single shared registry cache plus a shared MBX remote cache; if remote cache infrastructure is out of scope, compare (a) seven measured `target` archives with (b) seven `objects` archives plus one registry snapshot. Select based on total stored bytes and end-to-end warm-run time, not per-job cache-hit status alone.

Sources: [Cargo's Cargo-home CI caching guidance](https://doc.rust-lang.org/cargo/guide/cargo-home.html#caching-the-cargo-home-in-ci), [MBX cache comparison](https://mr-boxington.jdx.dev/compared), [MBX remote cache](https://mr-boxington.jdx.dev/remote-cache), [MBX GitHub Action guide](https://mr-boxington.jdx.dev/github-action), and [GitHub cache limits and eviction](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy).

**Warm-run acceptance:** after a successful trusted run has seeded the cache, the next run with the same toolchain, lockfiles, Cargo settings, target/features, and source snapshot must hit the relevant per-job caches. It must not repeat `Updating crates.io index`, `Downloading crates ...`, or full dependency compilation. A changed Rust source file may require compiling the affected crate; changed dependencies, toolchain, target, or build settings may require a new cache. GitHub caches can be absent or evicted, so no workflow can guarantee a hit after those inputs change or the cache is unavailable. MBX's default GitHub backend saves on default-branch pushes and restores read-only for pull requests. To warm repeated runs of a changed same-repository PR, require a PR-scoped cache-save policy supported by a pinned action version; fork PRs must remain read-only. Otherwise, only the default-branch cache can be relied on to seed PR runs.

Warm-run evidence must include cache hit/miss, stored archive size, bytes transferred across all jobs, restore/save duration, and whether old entries are evicted over sequential runs. Compare total CI wall time and the repository-wide cache footprint across all seven crate jobs. A cache hit is not sufficient evidence if the workflow still transfers multiple large copies of the same common dependencies.

### Evidence in the linked run

The linked run has 47 jobs:

- 42 task-matrix jobs: 7 build, 7 clippy, 7 nextest, 7 doc, 6 doctest, and 8 fmt (7 crates plus workspace fmt).
- 5 support jobs: Alint, Plan, Policy, Workflow Lint, and Required. `Policy` currently groups Cargo Deny, Cargo Machete, and Zizmor; split those validators into separate jobs. Rename `Workflow Lint` to `Actionlint` when that is the tool it runs. Alint, Cargo Deny, Cargo Machete, Actionlint, and Zizmor must each be independently visible. Plan and Required are orchestration/status jobs, not validator or crate-check jobs.

The linked job is the `fmt` task for `velnor-actions-mise`. The matrix expands from the plan output in [the generated workflow](../../.github/workflows/velnor.yml#L253). The configured Rust profile selects MBX and Cargo Nextest in [`.velnor/config.toml`](../../.velnor/config.toml#L31).

The Plan job also runs the first package's `fmt` task while that task is present in the matrix. As a result, `velnor-actions-actionlint` formatting runs twice. Formatting should have one owner.

## 2. Human-readable job names

### Feedback

Current names expose technical task, runner, path, and matrix details. Developers cannot tell the purpose of a job from its name. Job names must be intuitive, clear, and understandable without knowing Velnor internals. Jobs describe repository checks; they are not vendor-branded.

### Expected behavior

- Use a stack tag and crate identity, such as `Rust / velnor-actions-cli`.
- Get crate identity from `[package].name` in `Cargo.toml`; use the crate directory name as fallback.
- Never add a `Velnor` prefix to any job. This applies to both the GitHub-visible job name and the job identifier. Use purpose-based identifiers such as `alint`, `actionlint`, or `rust_<crate>`.
- Make every visible job name state what it checks or which crate it checks, such as `Alint`, `Actionlint`, or `Rust / velnor-actions-cli`.
- Do not expose task IDs, matrix IDs, task digests, Mise task paths, source line numbers, or tool-routing details in display names.
- Name repository-wide validator jobs after the actual tool, such as `Alint`, `Cargo Deny`, `Cargo Machete`, `Actionlint`, or `Zizmor`; do not use an umbrella name like `Policy` for several validators.
- Use clear step names such as `Format`, `Clippy`, and `Tests (Cargo Nextest)` or `Tests (Cargo Test)`.

Remove the user-visible automatic naming derived from discovered task names or metadata. Derive display names from the detected stack and crate identity. Internal identifiers may remain technical where report wiring requires them, but must not use a Velnor prefix or leak into GitHub-visible names.

## 3. Project scanning, tool detection, and overrides

### Feedback

Workflow generation must scan repository-local configuration to determine how the project builds and tests. A fixed runner choice or automatic choice based on an opaque task name can be wrong for the project. Apply the same detection and override model to all stack features that affect generated GitHub Actions commands.

### General rule

1. Detect project-local evidence during workflow generation and reflect it in the generated workflow.
2. If the repository has no evidence for an optional tool or feature, use the ecosystem's standard default.
3. Let users override detection or defaults in `.velnor/config.toml`. An explicit Velnor setting wins.
4. Do not claim to detect machine-global settings. If a tool is configured only on a developer's computer, use the normal default unless the user declares the tool in `.velnor/config.toml`.

Research ecosystem conventions before choosing defaults. Do not make users repeat standard project configuration in Velnor.

### Rust test runner

- Scan project manifests and configuration to determine how tests run.
- Recognize `.config/nextest.toml` as evidence that the project uses Cargo Nextest.
- If Nextest configuration defines `[profile.ci]`, use that profile for CI and follow Cargo Nextest's established defaults for other settings.
- When project evidence indicates Cargo's built-in test runner, use `cargo test`.
- Allow `.velnor/config.toml` to override the detected runner when the project uses a different setup.

### Mise setup

When repository scanning detects Mise configuration, generated CI must use `uses: jdx/mise-action` to set up the project's Mise environment and tools in the jobs that need it. Do this automatically from repository evidence; do not require users to repeat detected Mise setup in `.velnor/config.toml`.

#### Recommended `jdx/mise-action` setup

Use the action after checkout in each job that needs the repository's Mise tools. GitHub Actions jobs have separate environments, so setup in one job does not provide tools to another.

```yaml
- uses: jdx/mise-action@c2a87611a18de5b3828c5652fe268e992400cb5c # v4.3.0
  with:
    version: 2026.9.16 # project-pinned Mise version
    sha256: b6f8757201f6a2ee799f45f3f52ef7ca0b4071523637dc3b0b24264dd3333518
```

This illustrates the repository's current action and Mise pins; generated values must stay synchronized with the selected release and runner platform. Pin third-party actions to a full commit SHA and keep the release tag as a comment for humans. The `sha256` input verifies the downloaded Mise binary; it is separate from the action's commit SHA.

Recommended behavior:

- Let the action read the checked-out repository's `mise.toml`. Set its `version` input from the repository's Mise CLI pin (for example, `.mise-version`); do not assume the action infers its own version from that file. Do not generate synthetic `mise_toml` or `tool_versions` inputs when the repository already has its own Mise files.
- Keep the action's normal setup enabled: `install`, `cache`, `env`, and `export_path` default to `true`. The action should install declared tools, cache them, load Mise environment variables, and put shims on `PATH`. Only override these defaults for a documented need.
- Avoid separate hand-written Mise install and cache steps when the action handles them. The PR's current workflow disables the action's install/cache/env defaults and adds custom setup/cache plumbing; simplify this unless each custom step has a specific required behavior.
- Pin Mise to the repository's selected version. A committed `mise.lock` is the reproducible tool-version lock; current v4.3.0 action documentation says it detects that lock and installs with locked resolution. This repository currently has no `mise.lock`, so lockfile policy should be decided explicitly.
- The default `github_token` is `${{ github.token }}`; do not add broader token permissions just for this action. Leave bootstrap and Wings disabled unless the project has a deliberate need for them; Wings requires OIDC permission and a subscription.
- Treat caching as an optimization, not a correctness requirement. The job must also pass on an empty cache.

**Rust cache caveat:** upstream issue [#215](https://github.com/jdx/mise-action/issues/215) reports a restored Rust toolchain missing `rustfmt`, causing `cargo fmt` to fail. Since these crate jobs require formatting and Clippy, verify that `rustfmt` and `clippy` are available on both cold and warm cache runs. If the cache path loses components, explicitly ensure the components after setup or disable/adjust Mise caching for the affected Rust jobs until it is reliable.

Sources: [jdx/mise-action v4.3.0 README](https://github.com/jdx/mise-action/blob/v4.3.0/README.md), [v4.3.0 action inputs and defaults](https://github.com/jdx/mise-action/blob/v4.3.0/action.yml), [Mise CI guidance](https://mise.jdx.dev/continuous-integration.html), [Mise lockfile](https://mise.jdx.dev/dev-tools/mise-lock.html), [Mise Rust backend](https://mise.jdx.dev/lang/rust.html), and [GitHub guidance for third-party actions](https://docs.github.com/en/actions/reference/security/secure-use#using-third-party-actions).

### Rust build driver: Mr. Boxington (MBX)

Detect Mr. Boxington from repository-local Mise configuration. This repository has this Cargo wrapper in [`mise.toml`](../../mise.toml#L1):

```toml
wrappers = { cargo = { command = "mbx", env = { MBX_CARGO_SHIM_MODE = "1" } } }
```

MBX and Mise are separate detections: a Mise config selects the Mise setup action; the Cargo wrapper or explicit Velnor override selects the MBX action and command.

The MBX detection result changes the actual generated CI steps:

- If MBX is detected from repository configuration, or explicitly enabled in `.velnor/config.toml`, include `uses: jdx/mr-boxington-action` in each Rust crate job and run tests with `mbx test`.
- Otherwise, omit `jdx/mr-boxington-action` and run tests with `cargo test`.

When the repository-level Mise wrapper is present, generate the MBX path so CI follows the project's configuration. If no repository-local MBX evidence exists, default to Cargo. A machine-global MBX setup cannot be found by scanning the repository; users who want MBX in that case must set the build driver in `.velnor/config.toml` (for example, `compile_driver = "mbx"`). The Velnor setting also lets users override a detected repository choice.

**Runner interaction to resolve:** the earlier runner-detection rule selects Cargo Nextest when `.config/nextest.toml` is present, while this MBX rule specifies `mbx test`. Define the generated command when both MBX and Nextest are selected; do not silently discard either configuration signal.

### Mise task handling

Do not automatically select `.mise/tasks/*` entries just because scanning finds them or their names resemble a check. A Mise task's name does not reliably describe its behavior. Generate standard ecosystem commands from detected project configuration and best practices. Use a custom Mise task only when the user explicitly selects/configures it in Velnor.

## 4. Local CLI and documentation

### Feedback

The CLI exists in this PR, but the README calls it “proposed” and gives no local build/run instructions. Users need a clear way to try it.

### Local commands

From this checkout:

```sh
cargo run --locked -p velnor-actions-cli -- --help
cargo run --locked -p velnor-actions-cli -- plan
preview="$(mktemp -d /tmp/velnor-preview.XXXXXX)"
cargo run --locked -p velnor-actions-cli -- generate --output-dir "$preview"
```

The CLI operates on the Git repository containing the current directory. To use it on another repository, build the binary in this checkout and run `target/debug/velnor-actions` from the target repository's root. `init` creates `.velnor/config.toml`; this checkout already has one. `plan` writes no repository files. `generate` without `--output-dir` replaces the target repository's `.github` tree, so use a preview directory first.

## 5. Review acceptance checklist

- [ ] This repository generates seven Rust crate jobs and separate jobs for Alint, Cargo Deny, Cargo Machete, Actionlint, and each other repository-wide validator such as Zizmor.
- [ ] Every repository-wide validator checks global repository files/settings in its own clearly named job; aggregate/orchestration jobs do not create extra per-task fan-out.
- [ ] Cargo Deny, Cargo Machete, Actionlint, and other independent validators are not grouped under umbrella jobs such as `Policy` or `Workflow Lint`.
- [ ] Alint extends the specified OSS, GitHub Actions, Rust, lockfile, and no-tracked-artifacts bundles, and enforces edition 2024 for every `crates/*/Cargo.toml`.
- [ ] Each tool/dependency job restores an appropriate cache; MBX cache setup runs before any Cargo command.
- [ ] Mise tool caching has one compatible shared cache identity for jobs with the same Mise inputs; role-specific duplicate `mise-tools-*` snapshots and a second cache over the same Mise path are removed.
- [ ] The Rust cache design explicitly chooses between measured per-crate MBX `target` archives, per-crate MBX `objects` plus one shared Cargo registry cache, or shared MBX remote cache plus one shared registry cache. The choice accounts for all seven jobs' total cache footprint and restore/save time.
- [ ] No two cache mechanisms archive/save the same Cargo target, registry, MBX object, or Mise tool paths.
- [ ] Shared caches have an explicit writer policy: immutable GitHub cache entries are not written concurrently under the same key; Cargo registry data is seeded once and restored by crate jobs, or the design documents another race-safe approach.
- [ ] Cargo cache paths match the actual Cargo home and target directory used by the job; a complete compatible restore skips online fetch and runs Cargo offline, while cold/missing dependencies fetch only as needed.
- [ ] Cache sizes, hit/miss results, restore/save durations, and eviction/churn are measured across sequential runs; the total must retain headroom under GitHub's cache quota.
- [ ] After a trusted default-branch run seeds the cache, an unchanged sequential run reuses Cargo registry and build artifacts and avoids full dependency compilation; only changed inputs rebuild.
- [ ] Repeated same-repository PR runs can save to a PR-scoped cache using a pinned action version that supports it; fork PRs remain read-only.
- [ ] The generated main CI workflow is `.github/workflows/ci.yml`, with displayed workflow name `CI`; generated filenames do not expose Velnor or another generator/vendor name.
- [ ] Each Rust job is grouped/tagged as Rust and named from the Cargo package, with the folder name as fallback.
- [ ] Each crate job contains format, Clippy, and test steps; no per-task job fan-out remains.
- [ ] Test-runner detection follows project scan results, including `.config/nextest.toml` and `[profile.ci]`, unless an explicit Velnor override is set.
- [ ] When repository scanning detects Mise configuration, the generated jobs that need the Mise environment use `jdx/mise-action`.
- [ ] The Mise action is pinned to a full commit SHA, follows checkout, and uses the repository's pinned Mise version and verified binary checksum where configured.
- [ ] The action uses its standard install/cache/env/path setup unless a concrete project need requires an override; duplicate manual Mise setup is removed.
- [ ] Mise tool installation remains reproducible and succeeds with a cold cache; if `mise.lock` is adopted, CI uses locked resolution.
- [ ] Rust formatting and Clippy components are available with both cold and warm Mise caches, or the Rust cache is adjusted to avoid missing components.
- [ ] Build-driver detection uses the repository's Mise Cargo wrapper for MBX when present; without local evidence it defaults to Cargo, unless an explicit Velnor override is set.
- [ ] MBX mode includes `jdx/mr-boxington-action` and uses `mbx test`; Cargo mode omits the action and uses `cargo test`.
- [ ] The combined MBX plus Cargo Nextest case has an explicit command rule.
- [ ] No Mise task is auto-selected from its name; custom task use requires explicit Velnor configuration.
- [ ] Every job name and job identifier has no Velnor prefix; each visible job name clearly states the check or crate, without internal task metadata.
- [ ] Formatting runs once per intended scope.
- [ ] README explains how to build and run the implemented CLI locally.
- [ ] Any later implementation review starts only after this feedback proposal is accepted as the source of requirements.

# Velnor V1 Workflow Contract

Status: proposed specification; implementation requires Velnor dogfooding.

This document fixes V1 defaults; unsupported configuration MUST be rejected.

## 1. Product boundary

Velnor Actions is a stack-generic workflow generator, not a task interpreter. Registered Rust and OpenTofu
adapters derive task proposals from repository evidence. `velnor-actions-native` contains isolated audited
domain proposals behind typed façades; typed verification declarations join the same workflow IR. GitHub
Actions runs jobs on hosted or configured Scale Set runners, while V1 never runs scripts locally or manages
runner lifecycle. Docker supervision and a distributed cache service are deferred.

Each registered stack adapter owns its discovery and task proposals. The Mise adapter owns tool selection,
command construction and execution, and cache integration. The actionlint crate owns actionlint configuration
and capabilities. The orchestrator composes stack/tool proposals, selection, cache evidence, and scheduling.
The workflow renderer emits generic GitHub Actions YAML. V1 Rust work selects Cargo versus MBX and Cargo test
versus Nextest independently from explicit repository evidence; it does not impose MBX or Nextest on consumers.

Product packages live under `crates/` with the ownership and dependency boundaries in
[architecture §1](architecture.md). Common packages remain contract, Mise, actionlint, workflow renderer,
orchestrator and CLI. Rust and OpenTofu retain stack packages; `velnor-actions-native` contains isolated
named domain adapters behind typed façades and mandatory module boundary tests. Unrestricted generic
packages such as `velnor-model`, `velnor-core`, `velnor-util` and `velnor-common` are forbidden.

```text
velnor-actions-contract            Stack-neutral workflow and task contracts
velnor-actions-rust                All Rust/Cargo scanning and task proposals
velnor-actions-tofu                OpenTofu discovery and task proposals
velnor-actions-native              Isolated audited native domain proposals and semantic helper sources
velnor-actions-mise                Mise tool, command, and cache adapter
velnor-actions-actionlint          actionlint config, pin metadata, and workflow validation
velnor-actions-workflow-renderer   Generic GitHub Actions YAML renderer
velnor-actions-orchestrator        Combines stack, tool, contract, and renderer crates
velnor-actions-cli                 Clap frontend; declares the velnor-actions binary
```

The CLI delegates to the orchestrator. Rust, Mise, and actionlint crates are independent; only the orchestrator
composes them. The Rust crate does not construct Mise commands or files. The Mise crate does not parse Cargo
metadata or implement Rust selection. The actionlint crate does not execute tools; the Mise crate does that.
The workflow renderer does not know Rust, Mise, or actionlint configuration syntax. The contract crate
contains only stack-neutral generator types and MUST NOT become a generic utility or application-model
package.

The public CLI MUST expose exactly these commands:

```text
velnor-actions init
velnor-actions plan
velnor-actions generate [--output-dir PATH]
```

`init` resolves the Git repository root with `git rev-parse --show-toplevel` from the current working
directory and writes the sample `.velnor/config.toml` there. It MUST never create configuration in a child
directory. `plan` and `generate` resolve the same root and run the same
analysis. `plan` prints the human summary without writes; `generate` writes the
rendered `.github` tree.
With no `--output-dir`, it stages generated output together with preserved repository-owned `.github` entries,
then atomically replaces the directory under [generated-file §3](generated-file-contract.md).
With `--output-dir PATH`, PATH is the exact fresh preview root; it MUST be absent or empty, and the command
writes `PATH/.github` without modifying the repository. CI and local callers MUST choose a unique directory
under `/tmp` or the runner temp directory.

Workflow files under `.github/workflows` MUST be at most 500,000 UTF-8 bytes, including the marker, within GitHub Actions' [published 500 KB per-file limit](https://docs.github.com/en/actions/reference/limits). Larger files fail with `workflow_too_large:<path>:<actual_bytes>:500000` before writes; V1 never truncates or leaves partial output.

The CLI has no public scan, doctor, task, report, root, stack, profile, format, or check options. `plan`
prints only the concise human report specified in the CLI contract; it does not expose internal JSON
plan/report formats or execute tasks. The generated GitHub job named `plan` is an internal workflow
job, distinct from the local `velnor-actions plan` command. The public CLI MUST never hide an unbounded
collection of commands behind a user-facing `run` subcommand.

The orchestrator MUST run every registered detector in deterministic registry order. Each detector returns
zero or more typed detections and task proposals. The orchestrator applies `.velnor/config.toml`'s
`[stacks].ignore` list, then composes every remaining selected detection into one plan: stack-neutral
preparation and policy obligations are deduplicated by identity, stack tasks are namespaced by stack ID, and
the required final gate includes every selected obligation. A selected detection MUST contribute proposals or
an explicit no-work result; it MUST NOT be silently discarded.

V1 registers Rust/Cargo and OpenTofu detectors; only those selected stacks produce obligations. Unregistered
stacks receive no validation claim. With no selected Rust or OpenTofu detection, the scan is empty and no-work.
Invalid Cargo manifests and OpenTofu inputs fail with their path and diagnostic. Future adapters add detectors
and proposals to this same composition model; they do not add stack subcommands.
`generate` performs detection, scan, plan, validation, and rendering in one command. Internal plan and report
values are typed artifacts, not a public JSON API. Stack ignores have no CLI override.

## 2. Repository inputs and generated outputs

The following files are authoritative:

| File | Authority |
|---|---|
| `Cargo.toml`, member manifests, `Cargo.lock` | Packages, targets, dependencies, and resolution |
| Compiled-in version catalog | Exact latest tool versions, runner-label inventory, and generator-release identity used by generated workflows |
| `.velnor/version-policy.toml` | Velnor-repository-only mirror of the compiled catalog for freshness and release checks; consumers do not need this file |
| `rust-toolchain.toml`, `mise.toml`, `mise.lock` | Optional repository-owned inspection inputs; Velnor never writes them |
| `.velnor/config.toml` | Stack-neutral workflow policy, selected-stack exclusions, and explicit exceptions |
| `.velnor/generator.lock` | Velnor repository bootstrap override and mirror of bundled action pins; not required or generated for consumers |
| `.github/workflows/**` and exact declared generated paths | Generator-owned outputs; other `.github` entries remain repository owned |

The supported configuration boundary is:

```toml
[workflow]
policy = "consumer-v1" # or "velnor-repository-v1"
name = "CI"
default_branch = "main"
generator_validation = "bootstrap"
max_parallel_jobs = 2

[stacks]
ignore = []

[stacks.rust]
configurations = [{ name = "default", features = ["default"], target = "host" }]
```

`workflow.policy` is stack-neutral: `consumer-v1` is the only valid policy for consumer repositories;
`velnor-repository-v1` is accepted only when the canonical repository identity is `tailrocks/velnor-new`.
Use `GITHUB_REPOSITORY` in CI and normalized `origin` locally; mismatch or unavailable identity fails closed.
The reserved policy enables Velnor's Alint/dependency-security jobs and candidate validation. `stacks.ignore`
contains exact registered stack IDs and has no CLI override. Rust `configurations` control generated Rust
task variants after detection. Unknown keys, duplicate configurations, unknown stack IDs, and invalid values
MUST fail before workflow output is written.

The generator MUST render deterministically, reject raw YAML fragments, and fail on invalid generated output.
Tool-file findings are advisory; generation never writes them. Generation preserves repository-owned
nonworkflow `.github` entries under [generated-file §3](generated-file-contract.md), then atomically replaces
the combined tree. With `--output-dir PATH`, PATH is a fresh exact preview root containing that same tree
and the repository is unchanged.

The generator MUST discover workspaces through Git file enumeration followed by Cargo metadata. It MUST handle
nested workspaces, standalone packages, additions, deletions, renames, path dependencies, build scripts,
feature requirements, doctests, and shared configuration. It MUST select reverse dependents of changed local
packages. When the comparison base or graph is uncertain, it MUST broaden the plan.

## 3. Workflow topology

The generator MUST emit these triggers:

```yaml
on:
  pull_request:
    types: [opened, synchronize, reopened, ready_for_review]
  push:
    branches: [the-default-branch]
  merge_group:
```

The default branch name MUST come from explicit repository configuration or the local `refs/remotes/origin/HEAD`
symbolic ref. If neither is available, generation fails with an instruction to set `workflow.default_branch`; it
never assumes `main` or fetches a remote. The generator MUST NOT combine broad branch pushes with pull-request
path filters. A valid planning job MUST run even when no stack task is selected.

The main `ci.yml` emitter currently emits only the pull-request, default-branch
push, and merge-group triggers shown above. Its closed `WorkflowConfig` has no
`[workflow.verification]` table, and its `Trigger` sets `workflow_dispatch` and
`schedule` absent. This scope is specific to main CI: the separately typed
Velnor Freshness workflow retains its schedule and manual trigger, and the
schema-2 Qualification workflow retains `workflow_dispatch` when requested by
`execution.workflows`. Typed `[[workflow.tasks]]` declarations add verification
jobs to main CI without adding triggers. The former proposal for main-CI
schedule/manual alert and failure simulation is deferred; it does not describe
generated main-CI output.

Every generated workflow MUST set only the permissions required by its jobs.
At workflow level, generated CI MUST set `contents: read` and MUST omit
`actions`. GitHub denies omitted token scopes when a permissions map exists.
Only the repository-policy `plan` and the `required` job may set job-level `actions: read`; both maps MUST
retain `contents: read` because job permissions replace workflow permissions.
Repository-policy `plan` uses Actions read to authenticate the bounded prior-baseline lookup,
binding `GH_TOKEN` and `GH_REPO` only to its internal `plan-v1` step. `required`
uses Actions read to fetch exact current-run report artifacts and a validated
baseline artifact, binding those variables only to its report-fetch step.
Other jobs MUST NOT receive `actions: read` or a nonempty Actions token.

```yaml
permissions:
  contents: read
```

All other permissions MUST be absent or `none`. Release publication uses a
separate workflow with explicit permissions; fork pull requests receive no write access. `[stacks.rust.release]`
adds `release.yml` (see [release contract](release-contract.md)) with per-job permissions that MUST NOT weaken
this default.

The workflow concurrency group MUST be:

```yaml
group: velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

For `workflow.policy = "consumer-v1"`, compose detections into `plan`, stack task jobs, `actionlint`, and `required`.
V1 registers Rust and OpenTofu detectors; selected stacks are any subset of `{rust, tofu}`. All-Tofu groups
use `tofu-<slug>`; Rust and mixed groups use `rust-<slug>`. Future adapters add namespaced jobs without CLI selectors.

Typed `[[workflow.tasks]]` verification jobs join `Required` under either workflow policy; schema-2 routing, fail-closed admission, and authoring limits are defined in the [tooling input contract](tooling-input-contract.md).

The following labels are GitHub Actions job IDs only. They are not Cargo packages, executable
names, generated command labels, or CLI subcommands. The sole Velnor executable is `velnor-actions`.

1. `plan`: checkout; setup Mise through its pinned action with exact catalog `version`/`sha256`; acquire
   the bootstrap asset from the generated descriptor and verify it. Consumers embed the generating release version, target URL, and
   digest; Velnor uses the matching `.velnor/generator.lock` record and checks equality. Install exact tools
   from the embedded catalog, including GitHub CLI. Under repository policy, only the internal `plan-v1` step
   receives `GH_TOKEN` and `GH_REPO` for authenticated baseline lookup. It classifies obligations and emits the bounded matrix and
   complete plan report. In bootstrap mode it checks generated files with the locked binary; candidate mode
   does not invoke the candidate or require the bootstrap to reproduce its output.
2. `rust-<slug>`: one job per selected crate, grouping its obligations in contract order. It runs the focused
steps in the [task execution contract](task-execution-contract.md) and uploads one stack-neutral report after failure too.
3. `actionlint`: checkout without persisted credentials; setup Mise with pinned action, version,
   and SHA-256; install exact
   Actionlint and ShellCheck versions with project config, env, and hooks disabled; run
   `mise --no-config exec actionlint@<exact> shellcheck@<exact> -- actionlint -color` from the root. This
   required job reads `.github/actionlint.yaml` and checks every generated workflow, even with no Rust stack.
4. `required`: `if: always()`, depends on base and enabled policy jobs, validates reports/conclusions, and is the required status check.

Only for `workflow.policy = "velnor-repository-v1"`, emit one independent job per repository validator plus generated `release.yml` and `velnor-qualification.yml` workflows rendered from typed workflow IR, each with an explicit `permissions:` block (least privilege for its role). `alint`
checks out source with `persist-credentials: false` and runs the full-SHA-pinned `asamarts/alint` action with `path: .`,
`config: .alint.yml`, `format: github`, and `fail-on-warning: true`. `cargo-deny`, `cargo-machete`, and `zizmor`
each run their single dependency/security check in their own job; no umbrella grouping exists.
Each job conclusion is required evidence; Velnor does not assume validator-specific report artifacts.
All repository-policy job conclusions are required even with no crate selected. `consumer-v1` MUST
NOT assume `.alint.yml`, `deny.toml`, or Velnor's layout and MUST NOT emit repository validators. GitHub Action
steps use Velnor's compiled-in registry and exact config overrides; they are not Mise-managed tools.

When `workflow.generator_validation = "candidate"`, the generator MUST also emit `candidate` between
`plan` and `required`. It runs the fixed candidate-build Mise invocation specified in the
architecture contract; that invocation is a workflow step, not a separately named task or executable. The job
uploads the candidate binary, runs generation/fixture qualification against that exact artifact, and uploads
one candidate report. The candidate job MUST depend on `plan` but MUST NOT provide execution inputs consumed
by crate jobs; those always derive from the locked bootstrap binary. The candidate binary MUST NOT emit execution inputs in any step. Consumer repositories do not
emit this job.

Under `workflow.policy = "velnor-repository-v1"`, the `cargo-deny`, `cargo-machete`, and `zizmor` jobs MUST run
even when no crate is selected; `alint` follows the separate contract above. actionlint
remains in the always-on `actionlint` job. `plan` runs
formatting for each selected stack configuration. Rust documentation warnings run in each Rust task matrix
after doctests. Clippy, the detected test runner, and rustdoc are package-scoped in the Rust adapter; a workspace-wide compile
is not a default pull-request check. MSRV verification runs in the pinned toolchain-update qualification
workflow, tests every product crate against the exact declared `rust-version`, and is not repeated on every
pull request. `Required` depends on the plan, every crate job, the lint and validator conclusions, exact baseline coverage
validation, and the candidate report when candidate mode is enabled.

The final job MUST have stable display name `Required`; branch protection requires that exact check.
The `ci.yml` control jobs use one literal, versioned Ubuntu label. Without `workflow.runner_label`, Velnor MUST
use the latest pinned x64 policy label (`ubuntu-26.04` currently); older hosted labels require policy support
and an explicit `.velnor/config.toml` value. Typed verification tasks use declared hosted runners; schema-2 routing may place
eligible Linux work on the configured Scale Set or emit both copies (see [tooling input](tooling-input-contract.md)).
Velnor MUST reject `*-latest`, unversioned or unsupported hosted labels, expressions, matrices, and aliases in
every generated `runs-on`. Scale Set selectors MUST match validated `[execution]`. The plan records its control
label and whether it came from `latest_default` or `config_override`.

Every generated job MUST carry a per-job `timeout-minutes` below GitHub's 360 minute default; the bound is a required typed Job IR field (1–360 minutes),
rendered right after `runs-on`. Per-kind defaults follow the measured green-run walls in `docs/implemented/performance.md`: 30 minutes for crate-shaped jobs, 10 minutes for `plan`, `required`, validator, and release jobs.

`plan` MUST upload its plan report with `if: always()` and fail above a 512 KiB canonical `matrix.json` artifact or, when the workflow promotes an output-fed matrix, above 256 expanded jobs or 900,000 aggregate UTF-16 bytes across promoted job outputs. The artifact cap is independent of job outputs; count actual `matrix.include` entries only on the dynamic path because static artifact rows are not matrix jobs. GitHub documents a 1 MB per-job output limit approximated in UTF-16 and a 256-job matrix maximum in its [workflow syntax reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax).
`plan` MUST report a clear planning error and request a broadened or reduced plan instead of truncating entries.

The generated jobs MUST use these step sequences and commands. Action steps are the pinned allowlisted actions
above; every shell step is generated with fixed arguments and may not contain repository-provided shell text.

Workflow shell may invoke Mise directly for exact tool installation, generated command steps, and the fixed
candidate-build bootstrap. Every command step MUST use a fixed `mise --no-config exec` invocation with exact
tool pins, except a qualified Gate 6 task-cache step, which uses a Velnor-versioned task TOML under
`$RUNNER_TEMP`. It MUST NOT invoke private or removed `velnor-actions` subcommands or look up a repository
Mise task file. Checkout, artifact transfer, and Alint are fixed GitHub Actions primitives. Planning and
report aggregation are fixed internal workflow steps. Any helper binary is staged in runner temporary storage
and never written to the repo.

`plan` steps, in order:

1. `Checkout`: `actions/checkout` at the event's intended commit with `persist-credentials: false`.
2. `Setup Mise`: use the bundled full-SHA-pinned `jdx/mise-action` with exact catalog `version` and `sha256`,
`install: false`, and `env: false`; project tool files, task definitions, and hooks must not run.
3. `Prepare pinned tools`: run `mise install --no-env --no-hooks <tool>@<exact>...` using exact versions
embedded in the generator release catalog; set Velnor-owned `MISE_RUSTUP_HOME`/`MISE_CARGO_HOME`. The committed
`mise.lock` is verified (see [tooling-input §1.1](tooling-input-contract.md)); project env and hooks stay disabled.
4. `Acquire Velnor`: for consumers, download the exact target asset and verify SHA-256 from the embedded
release descriptor; for Velnor, use and verify the matching `.velnor/generator.lock` record.
5. `Format`: run the generated fixed format command through pinned Mise; use
the detected Rust compile driver (MBX only when project evidence selects it).
6. `Check generated files`: invoke the public `velnor-actions generate
--output-dir "$RUNNER_TEMP/velnor-actions-\${GITHUB_RUN_ID}-\${GITHUB_RUN_ATTEMPT}"` in bootstrap mode;
compare its `.github` tree byte-for-byte with the committed tree. Candidate mode performs the same check with
the candidate binary. Only this step and the `candidate` qualification checks may invoke the candidate artifact; every other step uses the bootstrap-descriptor helper version (see [cli §1 and §6](cli-contract.md)).
7. `Plan`: run the generated fixed planner step with the exact event
comparison refs. It writes the schema-1 plan to `$RUNNER_TEMP/velnor/<run-key>/plan.json`. Both refs MUST
identify the exact event comparison, including the pull request merge result where applicable. The step uses
direct pinned Mise commands or a locked helper staged under the runner's temporary directory.
8. `Publish plan`: validate the plan, emit the bounded `matrix.include` output,
and upload `velnor-plan-<run-key>` with `if: always()`.

Under `velnor-repository-v1`, Alint uses the full-SHA-pinned `asamarts/alint` action against Velnor's `.alint.yml`; the final gate checks its conclusion. The `cargo-deny`, `cargo-machete`, and `zizmor` jobs use pinned Mise. `consumer-v1` emits none of these jobs and needs no policy-only files.

Crate-job steps are the named steps in the task execution contract. Each consumes obligations from `plan` and
MUST NOT rediscover stacks or packages. `required` downloads the plan and expected matrix artifacts, merges the
reports through a fixed internal step, and publishes the final report with `if: always()`. The merge step runs
even after a crate job fails or is cancelled; it uses pinned Mise or a locked temporary helper.

The workflow run key is `r<github.run_id>-a<github.run_attempt>`, created by `plan` and passed unchanged to every
job. Local runs use `local` and do not upload artifacts. The key MUST NOT enter task or cache identities.

Actions MUST be selected only from this allowlist:

```text
jdx/mise-action
jdx/mr-boxington-action
actions/checkout
actions/cache/restore
actions/cache/save
actions/upload-artifact
actions/download-artifact
asamarts/alint
Swatinem/rust-cache
```

Velnor's compiled-in action registry supplies the latest stable release, full
40-character SHA, matching `# vX.Y.Z` comment, and action metadata. Per-project
exact overrides are permitted only through `.velnor/config.toml` and must
validate against the registry contract. The default current pins are listed in
the [version policy](version-policy.md). The workflow renderer MUST emit
`jdx/mise-action` for Mise setup, `actions/checkout` for source access,
cache restore/save for their respective cache phases, and upload/download
artifact actions for required reports or transferred outputs. It MUST emit
`jdx/mr-boxington-action` only when the Rust detector selects MBX, and `Swatinem/rust-cache` only for
Cargo-only repositories (registry-only, shared key, never over MBX-owned paths).
`asamarts/alint` is limited to Velnor's own repository-policy job. Branches,
moving refs, `pull_request_target`, `actions/setup-*`, and
`taiki-e/install-action` MUST NOT appear. `actionlint` and `zizmor` MUST
validate generated workflows. No tag exception exists: `asamarts/alint` pins
a full SHA in `alint` like every other action.

`velnor-actions generate` MUST render a staging tree containing `.github/`,
run the exact pinned actionlint binary through Mise with the staging tree as
its working directory (so it loads that tree's `.github/actionlint.yaml`), and
validate action/action-input schemas before replacing repository output.
Any diagnostic fails generation and leaves the existing `.github` tree
unchanged. The generated `actionlint` job repeats actionlint in CI.

## 4. Generic matrix contract and V1 Rust payload

Matrix entry shape, adapter metadata ownership, canonical output agreement and artifact
identity rules are defined in [the workflow matrix contract](workflow-matrix-contract.md).

## 5. Task execution

Rust task profiles follow the [task execution contract](task-execution-contract.md); future adapters own typed profiles.

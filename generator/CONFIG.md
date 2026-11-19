# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

## Fork pull requests

A pull request from a fork must never run on a self-hosted runner. When `runner` is not a GitHub-hosted label, the generator renders `runs-on` of every job, including the `Required` gate, as an expression. It selects `runner` when the event is not a pull request or the pull request head repository is this repository, and `fork_runner` otherwise. No job is skipped for a fork, so the gate sees the real result of every job and fails when one fails. The one exception is a job that calls a reusable workflow with `uses`: it has no `runs-on`, and the callee may run on a self-hosted runner, so the generator adds the same condition to its `if` (joined with the job's own `if` in one expression) and a pull request from a fork skips the call. A deleted fork has no head repository, so it counts as a fork.

`fork_runner` is a GitHub-hosted label. It defaults to `ubuntu-latest`, to `ubuntu-24.04-arm` when `runner` is a Linux arm64 label and to `macos-15` when `runner` is an Apple silicon macOS label. The generator refuses a `fork_runner` that is not GitHub-hosted or that does not match the platform of `runner`. It also refuses a self-hosted `runner` label that holds anything beyond letters, digits, `-`, `_` and `.`, because the label is written into an expression.

```toml
runner = "ubuntu-26.04-scale-set"
fork_runner = "ubuntu-24.04"
```

## Optional cargo fingerprint log

`cache_debug = true` in `.velnor/rust-ci.toml` makes every generated cargo step set `CARGO_LOG` from the repository variable `VELNOR_CACHE_DEBUG` and adds a step that uploads the log when that variable is `1`. It is `false` by default, and then the generated workflow reads no repository variable. A repository that sets it also declares `VELNOR_CACHE_DEBUG` to its workflow linter. The key needs a helper release that knows it (`0.1.82` or later), because older helpers reject unknown keys.

## Optional crates mirror

The new generator accepts an explicit mirror in `.velnor/rust-ci.toml`. It is off when the table is absent. It applies only when the configured job runner exactly matches the mirror's runner label. Every emitted mirror step requires the actual runner context to report `runner.environment == 'self-hosted'` and excludes fork pull requests. Hosted runners do not use the mirror, regardless of their label; a missing or unrecognized runner environment also skips setup.

```toml
runner = "ubuntu-26.04-scale-set"

[crates_mirror]
runner = "ubuntu-26.04-scale-set"
registry = "sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/"
cainfo = "/opt/velnor/crates-mirror/ca-bundle.pem"
```

The registry must use sparse HTTPS and end with `/`. The CA bundle must have an absolute path and already exist on the runner. The runner's image supplies it. The generator requires a pinned `velnor-ci` release of at least `0.1.81` when this table is present, including when the helper is built from source. The released helper configures Cargo before that bootstrap build.

Each opted-in job runs `velnor-ci crates-mirror` before Cargo or tool installation. It writes `$RUNNER_TEMP/velnor-cargo/config.toml` with `source.crates-io.replace-with = "velnor-crates-mirror"`, the sparse registry and `http.cainfo`. It leaves `$HOME/.cargo` and `$CARGO_HOME` untouched and refuses an existing job configuration that it does not own. Repeated setup replaces the owned configuration. Generation requires a valid HTTPS registry URL with a host. Cargo reports registry or CA failures as failures; it does not silently switch to public crates.io.

A Cargo wrapper on `PATH`, also exported as `CARGO`, passes the absolute configuration with Cargo's `--config` option. It runs a per-toolchain hard link of Cargo under a distinct executable name so Cargo preserves the wrapper in `CARGO` for build scripts and tests. When a hard link is unavailable, it installs a copy once. This keeps the mirror when a child command changes `CARGO_HOME` or runs from a temporary directory, and supports `cargo +toolchain`. Rustup selections from `rust-toolchain.toml`, directory overrides, or `+toolchain` remain in effect for child commands run from temporary directories. Calling a toolchain's Cargo by an unrelated absolute path bypasses `PATH` and `CARGO` and is outside this invocation contract.

Mise-managed Cargo retains its configured command wrappers, including MBX, and their environment. Each outer invocation activates Mise through a temporary data directory under `$RUNNER_TEMP`. Existing Mise data and other command wrappers delegate to their original locations; Cargo shim changes stay job scoped. Cargo's configured home and Rustup's configured home retain Mise's normal precedence. Native child commands inherit the wrapper environment and selected Cargo executable.

Writing the configuration only above the checkout misses temporary directories. Writing it only in Cargo's home misses a changed home. A configuration at `/.cargo/config.toml` would reach unrelated directories but needs filesystem-root access and changes unrelated jobs. The wrapper uses a per-job configuration instead.

This repository selects its scale-set runner and names its mirror in this configuration. Its existing engine and maintenance workflows are separate from the new generator and do not consume this setting. Running a generated Rust workflow requires the compatible helper release.

## Optional shared MBX store

`mbx_shared_store = true` in `.velnor/rust-ci.toml` makes every job run `velnor-ci mbx-store` right after the helper is installed. It needs `velnor-ci` 0.1.84 or later and is off by default.

The runner host may set `VELNOR_MBX_SHARED_DIR` in the runner pod. The workflow `env` context cannot see the runner process environment, so the helper decides at run time. On a trusted event (a push or schedule on the default branch), when the directory is absolute, exists, is not a symlink and contains no symlink, and is writable, the helper appends `MBX_CACHE_DIR=<dir>` and a pod-private `MBX_SHIMS_DIR=$RUNNER_TEMP/velnor-mbx-shims` to `$GITHUB_ENV`. A pull request, a fork, a `merge_group` run, or any other event never gets the shared directory. A directory that fails a check produces a `::warning::` and the job keeps MBX's own per-job store. Without the variable the step changes nothing.

## Test setup steps

A test that needs a fixture, such as a built example executable or a provisioned service, declares one step per fixture. The generator runs each step before the first test step of every job of that root, in the order written. Each step is one command, because a generated `run:` value is one command with no shell logic; the logic lives in a script the repository commits. The script exports variables to later steps through `$GITHUB_ENV`.

```toml
[[test_setup]]
root = "."
name = "Prepare the fixtures"
run = "bash scripts/ci-fixtures.sh"
```

`root` is the folder of the Cargo root's `Cargo.toml`, or `.` for the repository root, and it must name a root of this repository. The step runs in that folder. It follows the same gate as the test steps, so a run that selects no tests skips it. A job that runs no tests has none. With `shape = "groups"` the test group job installs only cargo-nextest, so a step there must not need Rust. A `run` value with an expression, a line break or any shell syntax is refused.

## Declared workflows

`.velnor/workflows.toml` makes the generator write one workflow file, `.github/workflows/<file>.yml`, for each `[[workflow]]` table. It is the only place that declares extra workflows: the `[[workflow]]` table of the earlier `fast/1290-source-preview` draft in `rust-ci.toml` is gone, and the reusable-workflow section that stood here is replaced by this one. The file is off when it is absent, and then the generated files are byte-identical to the output without it. `file` is lower case letters, digits and dashes; `rust-ci` and `release` belong to other outputs and are refused. A workflow sets `permissions: {}` at the top, and each job lists only the scopes it names. The samples `reusable-workflows`, `declared-workflow` and `declared-workflows` use every key below.

```toml
[[workflow]]
file = "release-checks"
name = "Release checks"
runner = "velnor-linux-x64"      # default ubuntu-latest
fork_runner = "ubuntu-latest"    # default ubuntu-latest, must be GitHub-hosted
if = "github.event_name != 'merge_group'"

[workflow.concurrency]           # optional; a job may set the same inline: concurrency = { group = "...", cancel_in_progress = false }
group = "${{ github.workflow }}-${{ github.head_ref || github.run_id }}"
cancel_in_progress = true        # default false

[workflow.on]                    # absent: the workflow is reusable (workflow_call)
merge_group = true
schedule = ["23 4 * * 1"]

[workflow.on.push]
branches = ["main"]
tags = ["rust-ci-v*"]            # the tag prefix is a value you choose
paths_ignore = ["docs/**", "**.md"]

[workflow.on.pull_request]
types = ["opened", "synchronize", "reopened"]
branches = ["main"]
paths = ["crates/**", "Cargo.lock"]

[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
type = "choice"
options = ["quick", "full"]
default = "quick"

[[workflow.job]]
id = "plan"
if = "github.event_name == 'push'"
timeout_minutes = 5
permissions = { contents = "read" }
outputs = { matrix = "steps.plan.outputs.matrix" }

[[workflow.job.step]]
uses = "checkout"

[[workflow.job.step]]
id = "plan"
name = "Plan"
run = "printenv MODE"
env = { MODE = "${{ inputs.mode }}" }

[[workflow.job]]
id = "check"
needs = ["plan"]
environment = "release"

[workflow.job.matrix]            # one source: from_output or axes
from_output = "plan.matrix"
fail_fast = false
max_parallel = 2

[[workflow.job.step]]
name = "Check"
run = "echo check"
continue_on_error = true
env = { TARGET = "${{ matrix.target }}" }

[[workflow.job]]
id = "assets"
needs = ["check"]
uses = "build-assets"            # a workflow declared earlier in this file
permissions = { contents = "read", id-token = "write", attestations = "write" }
with = { version = "${{ github.sha }}", dry_run = false }
```

### Triggers

Every workflow lists its events under `[workflow.on]`. The generator writes them in one fixed order whatever order you list them in: `workflow_call`, `workflow_dispatch`, `schedule`, `push`, `pull_request`, `merge_group`, `workflow_run`. Without `[workflow.on]` the workflow has `workflow_call` alone.

- `workflow_call = true`. The inputs are `[[workflow.input]]` and the outputs are `[[workflow.output]]` of the workflow. An input has `name`, an optional `description`, `type` (`string`, `boolean` or `number`, default `string`), an optional `default` of that type, and `required`, which is `true` unless there is a `default`. A number is a whole number of zero or more. An output has `name`, an optional `description` and `value = "<job>.<output>"`, where the job declares that output (or is a job that calls a workflow with that output); it renders `${{ jobs.<job>.outputs.<output> }}`.
- `[workflow.on.workflow_dispatch]` with `[[workflow.on.workflow_dispatch.input]]`. The input keys are the same, and `type` may also be `choice` (with `options`, and a `default` that is one of them) or `environment`.
- `schedule = ["<five-field cron>", ...]`. A schedule that can fire twice less than 5 minutes apart (`*/1`, `*/4`, `0,3`, `*/7`, which fires at :56 and :00) is refused, because 5 minutes is the shortest interval GitHub runs a schedule at.
- `[workflow.on.push]` needs `branches` or `tags` or both, which are GitHub filter patterns. An unfiltered `push` is refused. Nothing in the generator knows a tag prefix: `rust-ci-v*`, `v*` and `sdk-preview-*` are all just values.
- `[workflow.on.pull_request]` needs `types`, and takes optional `branches`. `pull_request_target` does not exist in this model, so a declared workflow never runs fork code with secrets.
- `paths` or `paths_ignore` (never both on one trigger, as in GitHub) on `push` and `pull_request` skip the run when no changed file matches, or when every changed file is ignored. They are written `paths` and `paths-ignore`, after `branches` and `tags`. A `paths` or `paths_ignore` of `"**"` is refused. GitHub ignores path filters for a tag push and for `merge_group`, and a required check that a path filter skips stays pending on the pull request, so a workflow behind a required check either has no path filter or is not required. `workflow_run` has no path filters.
- A `branches` list that holds `"**"` is refused as redundant on `push`, `pull_request` and `workflow_run`: leave `branches` out to match every branch (a `push` without `branches` needs `tags`). `tags = ["**"]` is allowed, since without `tags` a push of a tag starts nothing.
- `merge_group = true`.
- `[workflow.on.workflow_run]` needs `workflows` (the names of the workflows it follows), takes `types` (default `completed`) and optional `branches`. A `workflow_run` workflow runs in the context of the default branch. Do not check out another ref: `with` cannot read `github.event.*`, so a step cannot name the head of the run it follows.
- Inputs of `workflow_call` and `workflow_dispatch` are both read as `inputs.<name>`.

### Concurrency

`[workflow.concurrency]` and the `concurrency` key of a job take `group` and `cancel_in_progress` (default `false`), and are written as `concurrency: {group, cancel-in-progress}` after `permissions`. A superseded run is cancelled only when two runs share a group, so a port that should drop runs for an older commit names a group per branch or pull request.

- A group is plain text of letters, digits and `- _ . : /` (up to 200 bytes) with these expressions in `${{ }}` and no others: `github.workflow`, `github.ref`, `github.event.pull_request.number` and `github.head_ref || github.run_id`. `github.head_ref || github.run_id` is the pull request branch, or a group of one run for a push; `github.ref` is one group per branch, tag or pull request ref.
- `cancel_in_progress = true` needs `github.ref` or `github.head_ref || github.run_id` in the group. A group of `github.workflow` alone would let a run on one branch cancel the runs on every other, release runs included, and is refused.
- A job that calls a reusable workflow may set `concurrency` too. Nothing but the group and the flag is configurable, so a group never reads `secrets`, `inputs`, `matrix` or `github.event.*` text an author of a pull request controls.

### Jobs

- `needs` lists earlier jobs of the workflow. `if` is one expression, with or without `${{ }}`. A workflow `if` and a job `if` both apply: the job gets `(workflow if) && (job if)`.
- `runner` sets the runner of one job; the workflow `runner` is the default. A runner label is letters, digits, dashes, underscores and dots. A GitHub-hosted label is `ubuntu-`, `macos-` or `windows-` followed by `latest` or a version number (`ubuntu-24.04`, `ubuntu-24.04-arm`); `ubuntu-` alone is a label like any other. A label that is not a GitHub-hosted one gets the fork fence: `runs-on` is `<runner>` for a push, a schedule and a pull request from this repository, and `fork_runner` for a pull request from a fork.
- `timeout_minutes` is 1 to 360, default 30.
- `permissions` accepts these scopes: `actions`, `artifact-metadata`, `attestations`, `checks`, `contents`, `deployments`, `id-token`, `issues`, `packages`, `pages`, `pull-requests`, `security-events` and `statuses`, each `read`, `write` or `none` (`id-token` has no `read`). A job with none gets `permissions: {}`. `checkout` needs `contents: read` on a private repository, so name it.
- `environment = "<name>"` is for step jobs. A deployment environment is what holds secrets and approval.
- `outputs` maps a name to `steps.<step id>.outputs.<name>` of a step of the same job.
- `helper = true` puts the pinned `velnor-ci` install step first in the job.
- A matrix has one source. `from_output = "<job>.<output>"` renders `fromJSON(needs.<job>.outputs.<output>)`; that job must be in `needs` and declare the output. `axes = { os = ["a", "b"], ... }` renders a static matrix of up to 256 combinations, and `matrix.<axis>` is then the only matrix key a value may read. `fail_fast` and `max_parallel` are optional.
- `uses = "<workflow>"` makes the job call a reusable workflow, and then the job has no steps, `runner`, `timeout_minutes`, `environment`, `helper` or `outputs`. `<workflow>` is one of: the `file` of a workflow declared earlier in this file (its inputs are checked, its `boolean` and `number` inputs are written as such, and the job must hold every permission the called jobs ask for); `./.github/workflows/<name>.yml`; or `owner/repo/.github/workflows/<name>.yml@<full commit SHA>` (exactly 40 lower case hex digits). A `./.github/workflows` or pinned callee is not read, so its jobs get no fork fence and its permissions are not checked: a workflow that a `pull_request` or `merge_group` event starts is refused if it calls one, or calls a declared workflow that calls one. Such a call is allowed from `push`, `schedule`, `workflow_dispatch`, `workflow_call` and `workflow_run`. `with` passes inputs, each plain text or exactly one expression. No `secrets` are passed, ever. A later job reads `needs.<job>.outputs.<name>` for the outputs of a declared workflow.

### Cases

A `[[workflow.case]]` is one qualification case that runs on a hosted runner and on a scale-set runner. The generator writes two jobs from it, `<id>-hosted` and `<id>-scale-set`, with the same steps, named `<title> / GitHub hosted` and `<title> / Velnor Scale Set`, and runs them in parallel.

- The workflow needs a `workflow_dispatch` input `mode` of type `choice`, and `hosted_runner` (GitHub-hosted) and `scale_set_runner` (not GitHub-hosted). The scale-set job gets the fork fence of its runner like any other job.
- `modes` lists the `mode` options that run the case, and defaults to the case `id`. Each job gets `if: inputs.mode == '<mode>'` joined with `||`; a case `if` is joined to it with `&&`.
- `title` defaults to the `id`. `timeout_minutes` and the `[[workflow.case.step]]` tables follow the job rules above. A case has no `needs`, `permissions`, `environment` or `outputs`; declare a job for those.

### Steps

- A step is exactly one of `run`, `uses` and `helper`. A name is optional for `uses` and `helper` steps. `continue_on_error = true` renders `continue-on-error: true` on that step only.
- `run` is one simple command: no line break, no `;`, `|`, `&`, `$(`, backtick, `if`, `for`, `while` or `case`, and no `${{ }}`. Pass values through `env`. A command that runs `velnor-ci` needs `helper = true` on its job.
- `helper = "<command>"` renders `run: velnor-ci <command>` and needs `helper = true` on its job.
- `uses` names a SHA-pinned action from this fixed list: `checkout`, `mise`, `upload-artifact`, `download-artifact`, `cache-restore`, `cache-save` and `attest` (`actions/attest-build-provenance`). `with` sets its inputs. Adding an action is a change to this list, not to the model.
- Locked inputs: `checkout` always has `persist-credentials: false`; `mise` always has `cache: false` and `cache_save: false`; `attest` always has `push-to-registry: false`. Setting one to anything but `false` is refused, and setting it to `false` is accepted and written once. `attest` is refused unless its job has `id-token = "write"` and `attestations = "write"`.
- An `if` on a step may read `needs.<needed job>.result`, `steps.<earlier step id>.outcome` and `github.*`, besides what a value may read. An `if` on a job may read `needs`, `inputs` and `github.*` only.
- An `env` or `with` value is plain text or exactly one expression. The allowed expressions are `github.token`, `github.repository`, `github.sha`, `inputs.<declared input>`, `matrix.<key>` in a matrix job, `steps.<earlier step id>.outputs.<name>` and `needs.<needed job>.outputs.<declared output>`. Anything else (including `secrets` and `github.event.*`), and any `needs` on a job declared later, is refused with the path of the problem. This holds for a `workflow_run` workflow on a self-hosted runner as well, so the head branch, SHA and commit message of the run it follows (`github.event.workflow_run.*`), which an author of a pull request controls, reach a step only through an `if`.

### Later

A `workflow_run` name is checked for form only, not against the workflows that exist: the generator reads the names of declared workflows from this file and does not read the `name:` of a hand-written file or of `rust-ci` (which is configurable), so a name that matches nothing is a workflow that never starts.
